use std::{fs::File, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use player_plugin_abi::{VESPER_PIXEL_NV12, VesperVideoFormat};
use player_plugin_loader::native::NativeVideoFrame;
use player_render_vulkan::{
    ImportedWin32VideoImage, VulkanColorFrame, VulkanColorInput, VulkanFrameLease,
    VulkanPresentStatus, Win32TextureHandleKind, Win32VulkanPresenter, read_shader_words,
};
use player_runtime::PlayerVideoSurfaceKind;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_RESOURCE_MISC_HW_PROTECTED, D3D11_RESOURCE_MISC_RESTRICT_SHARED_RESOURCE,
    D3D11_RESOURCE_MISC_RESTRICT_SHARED_RESOURCE_DRIVER, D3D11_RESOURCE_MISC_RESTRICTED_CONTENT,
    D3D11_RESOURCE_MISC_SHARED, D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX,
    D3D11_RESOURCE_MISC_SHARED_NTHANDLE, D3D11_TEXTURE2D_DESC, ID3D11Device, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12;
use windows::Win32::Graphics::Dxgi::{
    DXGI_SHARED_RESOURCE_READ, IDXGIDevice, IDXGIResource, IDXGIResource1,
};
use windows::Win32::UI::WindowsAndMessaging::{GWLP_HINSTANCE, GetWindowLongPtrW};
use windows::core::Interface;

use super::WindowsSurfaceAttachTarget;

const VULKAN_SHADER_ENV: &str = "VESPER_WINDOWS_VULKAN_COLOR_FILTER_SPIRV";

pub(super) struct VulkanPresenter {
    presenter: Win32VulkanPresenter<VulkanWork>,
    epoch: u64,
    submitted: bool,
}

struct VulkanWork {
    // The imported image must go away before the decoder can reuse its memory.
    _image: Arc<ImportedWin32VideoImage>,
    frame: NativeVideoFrame,
}

impl VulkanFrameLease for VulkanWork {
    fn complete(self, accepted: bool) {
        let Self {
            _image: image,
            frame,
        } = self;
        drop(image);
        frame.complete(accepted);
    }
}

struct SharedTextureHandle(HANDLE, Win32TextureHandleKind);

impl Drop for SharedTextureHandle {
    fn drop(&mut self) {
        if matches!(self.1, Win32TextureHandleKind::Nt) {
            // SAFETY: CreateSharedHandle transferred this NT handle to us. KMT handles are borrowed.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
}

impl VulkanPresenter {
    pub(super) fn try_open(
        device: &ID3D11Device,
        target: WindowsSurfaceAttachTarget,
    ) -> Result<Option<Self>> {
        let Some(path) = std::env::var_os(VULKAN_SHADER_ENV) else {
            return Ok(None);
        };
        let shader = read_shader_words(&mut File::open(path)?)?;
        ensure!(
            target.kind == PlayerVideoSurfaceKind::Win32Hwnd && target.handle != 0,
            "Win32 Vulkan requires an attached HWND"
        );
        let hwnd = HWND(target.handle as *mut _);
        let dxgi: IDXGIDevice = device.cast()?;
        // SAFETY: the DXGI device and attached HWND remain live during route selection.
        let (adapter, instance) = unsafe {
            (
                dxgi.GetAdapter()?.GetDesc()?,
                GetWindowLongPtrW(hwnd, GWLP_HINSTANCE),
            )
        };
        let mut luid = [0; 8];
        luid[..4].copy_from_slice(&adapter.AdapterLuid.LowPart.to_le_bytes());
        luid[4..].copy_from_slice(&adapter.AdapterLuid.HighPart.to_le_bytes());
        // SAFETY: the caller owns the window and gives this route exclusive presentation.
        // Vulkan WSI retains its surface object through present-fence retirement.
        let mut presenter = unsafe {
            Win32VulkanPresenter::open_win32(
                target.handle as isize,
                instance,
                luid,
                Arc::new(()),
                &shader,
            )
        }?;
        presenter.begin_epoch(1)?;
        Ok(Some(Self {
            presenter,
            epoch: 1,
            submitted: false,
        }))
    }

    pub(super) fn begin_epoch(&mut self) -> Result<()> {
        let epoch = self
            .epoch
            .checked_add(1)
            .context("Win32 Vulkan presentation epoch exhausted")?;
        self.presenter.begin_epoch(epoch)?;
        self.epoch = epoch;
        Ok(())
    }

    pub(super) fn submitted(&self) -> bool {
        self.submitted
    }

    pub(super) fn drain(&mut self) -> Result<()> {
        self.presenter
            .drain_presentation(Duration::from_millis(100))?;
        Ok(())
    }

    pub(super) fn import(
        &self,
        device: &ID3D11Device,
        handle: usize,
        subresource: u32,
        format: VesperVideoFormat,
    ) -> Result<Arc<ImportedWin32VideoImage>> {
        ensure!(
            format.pixel_format == VESPER_PIXEL_NV12,
            "Win32 Vulkan filter requires NV12"
        );
        let raw = handle as *mut _;
        // SAFETY: the decoded-frame lease retains the ID3D11Texture2D for this import.
        let texture =
            unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }.context("missing D3D11 texture")?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: the live texture supplies its descriptor and device.
        let producer = unsafe {
            texture.GetDesc(&mut desc);
            texture.GetDevice()?
        };
        ensure!(producer == *device, "unexpected D3D11 producer device");
        ensure!(
            desc.Format == DXGI_FORMAT_NV12
                && desc.MipLevels == 1
                && desc.SampleDesc.Count == 1
                && subresource < desc.ArraySize
                && desc.Width >= format.width
                && desc.Height >= format.height,
            "D3D11 texture shape is incompatible with Win32 Vulkan NV12 import",
        );
        ensure!(
            desc.MiscFlags & D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX.0 as u32 == 0,
            "D3D11 producer did not negotiate non-keyed sharing",
        );
        let restricted = D3D11_RESOURCE_MISC_HW_PROTECTED.0
            | D3D11_RESOURCE_MISC_RESTRICTED_CONTENT.0
            | D3D11_RESOURCE_MISC_RESTRICT_SHARED_RESOURCE.0
            | D3D11_RESOURCE_MISC_RESTRICT_SHARED_RESOURCE_DRIVER.0;
        ensure!(
            desc.MiscFlags & restricted as u32 == 0,
            "protected D3D11 textures cannot enter Vulkan filters"
        );
        let shared = if desc.MiscFlags & D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0 as u32 != 0 {
            let resource: IDXGIResource1 = texture.cast()?;
            // SAFETY: the resource advertises NT sharing and the handle is only used in this process.
            let handle =
                unsafe { resource.CreateSharedHandle(None, DXGI_SHARED_RESOURCE_READ.0, None) }?;
            SharedTextureHandle(handle, Win32TextureHandleKind::Nt)
        } else if desc.MiscFlags & D3D11_RESOURCE_MISC_SHARED.0 as u32 != 0 {
            let resource: IDXGIResource = texture.cast()?;
            // SAFETY: the texture lease retains the allocation backing this borrowed KMT handle.
            let handle = unsafe { resource.GetSharedHandle() }?;
            SharedTextureHandle(handle, Win32TextureHandleKind::Kmt)
        } else {
            bail!("Media Foundation returned a non-shareable D3D11 texture");
        };
        // SAFETY: the caller observed D3D11 completion on the same device and
        // retains the clear producer lease through Vulkan retirement.
        let image = unsafe {
            ImportedWin32VideoImage::import_nv12(
                self.presenter.device_context(),
                shared.0.0 as isize,
                shared.1,
                desc.Width,
                desc.Height,
                desc.ArraySize,
                subresource,
            )
        }?;
        Ok(Arc::new(image))
    }

    pub(super) fn present(
        &mut self,
        frame: NativeVideoFrame,
        format: VesperVideoFormat,
        image: Arc<ImportedWin32VideoImage>,
    ) -> Result<bool> {
        // SAFETY: the producer completed before import. The work retains both the
        // imported memory and D3D11 sample until the presentation queue retires.
        let result = unsafe {
            self.presenter.present(
                self.epoch,
                VulkanWork {
                    _image: Arc::clone(&image),
                    frame,
                },
                VulkanColorFrame {
                    format,
                    gain: 1.0,
                    input: VulkanColorInput::Win32(&image),
                },
            )
        }?;
        if let Some(result) = result {
            self.submitted = true;
            if matches!(result, VulkanPresentStatus::OutOfDate { .. }) {
                bail!("Win32 Vulkan surface is out of date");
            }
            return Ok(true);
        }
        Ok(false)
    }
}
