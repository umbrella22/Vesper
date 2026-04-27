use std::sync::Arc;

use anyhow::Context;
use player_backend_ffmpeg::{
    CompressedVideoPacket, FfmpegBackend, VideoDecodeInfo as BackendVideoDecodeInfo,
    VideoDecoderMode as BackendVideoDecoderMode, VideoPacketSource, VideoPacketStreamInfo,
};
use player_core::MediaSource;
use player_platform_desktop::{
    DesktopVideoFrame, DesktopVideoFramePoll, DesktopVideoSource, DesktopVideoSourceBootstrap,
    DesktopVideoSourceFactory, merge_runtime_fallback_reason,
    probe_platform_desktop_source_with_options,
    probe_platform_desktop_source_with_video_source_factory_and_options,
};
use player_plugin::{
    DecoderMediaKind, DecoderNativeDeviceContext, DecoderNativeHandleKind, DecoderPacket,
    DecoderReceiveNativeFrameOutput, DecoderSessionConfig, NativeDecoderSession, VesperPluginKind,
};
use player_plugin_loader::{
    DecoderPluginCapabilitySummary, DecoderPluginCodecSummary, DecoderPluginMatchRequest,
    LoadedDynamicPlugin, PluginDiagnosticRecord, PluginDiagnosticStatus, PluginRegistry,
};
use player_runtime::{
    PlayerDecoderPluginVideoMode, PlayerMediaInfo, PlayerPluginCodecCapability,
    PlayerPluginDecoderCapabilitySummary, PlayerPluginDiagnostic, PlayerPluginDiagnosticStatus,
    PlayerRuntime, PlayerRuntimeAdapter, PlayerRuntimeAdapterBootstrap,
    PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory, PlayerRuntimeAdapterInitializer,
    PlayerRuntimeBootstrap, PlayerRuntimeError, PlayerRuntimeErrorCode, PlayerRuntimeEvent,
    PlayerRuntimeInitializer, PlayerRuntimeOptions, PlayerRuntimeResult, PlayerRuntimeStartup,
    PlayerVideoDecodeInfo, PlayerVideoDecodeMode, register_default_runtime_adapter_factory,
};
use std::collections::VecDeque;

pub const WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID: &str = "windows_software_desktop";
pub const WINDOWS_NATIVE_FRAME_PLAYER_RUNTIME_ADAPTER_ID: &str = "windows_native_frame_desktop";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsNativeFrameBackendKind {
    D3D11,
    Dxva,
}

#[derive(Debug, Clone)]
pub struct WindowsNativeFrameRoadmap {
    pub adapter_id: &'static str,
    pub preferred_backend: WindowsNativeFrameBackendKind,
    pub accepted_handle_kinds: &'static [&'static str],
}

pub fn windows_native_frame_roadmap() -> WindowsNativeFrameRoadmap {
    WindowsNativeFrameRoadmap {
        adapter_id: WINDOWS_NATIVE_FRAME_PLAYER_RUNTIME_ADAPTER_ID,
        preferred_backend: WindowsNativeFrameBackendKind::D3D11,
        accepted_handle_kinds: &["D3D11Texture2D", "DxgiSurface"],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowsSurfaceAttachTarget {
    kind: player_runtime::PlayerVideoSurfaceKind,
    handle: usize,
}

#[derive(Debug, Clone)]
struct WindowsRuntimeDiagnostics {
    video_decode: PlayerVideoDecodeInfo,
    plugin_diagnostics: Vec<PlayerPluginDiagnostic>,
}

struct WindowsRuntimeAdapterInitializer {
    inner: Box<dyn PlayerRuntimeAdapterInitializer>,
    diagnostics: WindowsRuntimeDiagnostics,
    fallback: Option<WindowsRuntimeAdapterFallback>,
    runtime_fallback: Option<WindowsRuntimeActiveFallback>,
}

#[derive(Debug, Clone)]
struct WindowsNativeFrameSelection {
    preferred_backend: WindowsNativeFrameBackendKind,
    plugin_path: std::path::PathBuf,
}

struct WindowsRuntimeAdapterFallback {
    inner: Box<dyn PlayerRuntimeAdapterInitializer>,
    diagnostics: WindowsRuntimeDiagnostics,
    fallback_reason: String,
}

#[derive(Clone)]
struct WindowsRuntimeActiveFallback {
    source: MediaSource,
    options: PlayerRuntimeOptions,
    fallback_reason: String,
}

#[derive(Debug)]
struct WindowsNativeFrameVideoSourceFactory {
    plugin_path: std::path::PathBuf,
    preferred_backend: WindowsNativeFrameBackendKind,
    video_surface: player_runtime::PlayerVideoSurfaceTarget,
}

struct WindowsNativeFrameVideoSource {
    packet_source: VideoPacketSource,
    stream_info: VideoPacketStreamInfo,
    session: Box<dyn NativeDecoderSession>,
    presenter: Box<dyn WindowsNativeFramePresenter>,
    surface_target: WindowsSurfaceAttachTarget,
    end_of_input_sent: bool,
}

trait WindowsNativeFramePresenter: Send {
    fn backend_kind(&self) -> WindowsNativeFrameBackendKind;
    fn accepted_handle_kind(&self) -> DecoderNativeHandleKind;
    fn decoder_device_context(&self) -> Option<DecoderNativeDeviceContext>;
    fn attach(&mut self, target: WindowsSurfaceAttachTarget) -> PlayerRuntimeResult<()>;
    fn reset(&mut self) -> PlayerRuntimeResult<()>;
    fn present(&mut self, handle: usize) -> PlayerRuntimeResult<()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum WindowsD3D11PresenterState {
    #[default]
    Detached,
    AttachedAwaitingDevice,
    AttachedNoDevice,
}

#[derive(Debug, Default)]
struct WindowsD3D11NativeFramePresenterSkeleton {
    state: WindowsD3D11PresenterState,
    attached_target: Option<WindowsSurfaceAttachTarget>,
}

impl WindowsNativeFramePresenter for WindowsD3D11NativeFramePresenterSkeleton {
    fn backend_kind(&self) -> WindowsNativeFrameBackendKind {
        WindowsNativeFrameBackendKind::D3D11
    }

    fn accepted_handle_kind(&self) -> DecoderNativeHandleKind {
        DecoderNativeHandleKind::D3D11Texture2D
    }

    fn decoder_device_context(&self) -> Option<DecoderNativeDeviceContext> {
        None
    }

    fn attach(&mut self, target: WindowsSurfaceAttachTarget) -> PlayerRuntimeResult<()> {
        self.attached_target = Some(target);
        self.state = WindowsD3D11PresenterState::AttachedAwaitingDevice;
        Ok(())
    }

    fn reset(&mut self) -> PlayerRuntimeResult<()> {
        self.state = WindowsD3D11PresenterState::Detached;
        self.attached_target = None;
        Ok(())
    }

    fn present(&mut self, _handle: usize) -> PlayerRuntimeResult<()> {
        if self.state == WindowsD3D11PresenterState::Detached {
            return Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::BackendFailure,
                "windows D3D11 native-frame presenter is not attached to a surface target yet",
            ));
        }
        if self.state == WindowsD3D11PresenterState::AttachedAwaitingDevice {
            self.state = WindowsD3D11PresenterState::AttachedNoDevice;
            return Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::BackendFailure,
                "windows D3D11 native-frame presenter is not attached to a device/context yet",
            ));
        }
        Err(PlayerRuntimeError::new(
            PlayerRuntimeErrorCode::BackendFailure,
            "windows D3D11 native-frame presenter skeleton is not implemented yet",
        ))
    }
}

fn windows_native_frame_presenter_for_backend(
    backend: WindowsNativeFrameBackendKind,
) -> PlayerRuntimeResult<Box<dyn WindowsNativeFramePresenter>> {
    match backend {
        #[cfg(target_os = "windows")]
        WindowsNativeFrameBackendKind::D3D11 | WindowsNativeFrameBackendKind::Dxva => {
            windows_d3d11_presenter::WindowsD3D11NativeFramePresenter::new()
                .map(|presenter| Box::new(presenter) as Box<dyn WindowsNativeFramePresenter>)
        }
        #[cfg(not(target_os = "windows"))]
        WindowsNativeFrameBackendKind::D3D11 | WindowsNativeFrameBackendKind::Dxva => {
            Ok(Box::new(WindowsD3D11NativeFramePresenterSkeleton::default()))
        }
    }
}

#[cfg(target_os = "windows")]
mod windows_d3d11_presenter {
    use std::ffi::c_void;

    use player_plugin::{
        DecoderNativeDeviceContext, DecoderNativeDeviceContextKind, DecoderNativeHandleKind,
    };
    use player_runtime::{PlayerRuntimeError, PlayerRuntimeErrorCode, PlayerRuntimeResult};
    use windows::Win32::Foundation::{HMODULE, HWND, RECT};
    use windows::Win32::Graphics::Direct3D::{
        D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_10_0,
        D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
    };
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION,
        D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
        D3D11_VIDEO_PROCESSOR_CONTENT_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC,
        D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
        D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_STREAM,
        D3D11_VIDEO_USAGE_PLAYBACK_NORMAL, D3D11_VPIV_DIMENSION_TEXTURE2D,
        D3D11_VPOV_DIMENSION_TEXTURE2D, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
        ID3D11Resource, ID3D11Texture2D, ID3D11VideoContext, ID3D11VideoDevice,
    };
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM,
        DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_NV12, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
    };
    use windows::Win32::Graphics::Dxgi::{
        DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG,
        DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter, IDXGIDevice,
        IDXGIFactory2, IDXGISwapChain1,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
    use windows::core::Interface;

    use super::{
        WindowsNativeFrameBackendKind, WindowsNativeFramePresenter, WindowsSurfaceAttachTarget,
    };

    pub struct WindowsD3D11NativeFramePresenter {
        device: ID3D11Device,
        context: ID3D11DeviceContext,
        swap_chain: Option<IDXGISwapChain1>,
        attached_hwnd: Option<usize>,
        swap_chain_size: Option<(u32, u32)>,
    }

    #[allow(dead_code)]
    fn assert_presenter_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<WindowsD3D11NativeFramePresenter>();
    }

    impl WindowsD3D11NativeFramePresenter {
        pub fn new() -> PlayerRuntimeResult<Self> {
            let feature_levels = [
                D3D_FEATURE_LEVEL_11_1,
                D3D_FEATURE_LEVEL_11_0,
                D3D_FEATURE_LEVEL_10_1,
                D3D_FEATURE_LEVEL_10_0,
            ];
            let mut device = None;
            let mut context = None;
            let mut selected_feature_level = D3D_FEATURE_LEVEL::default();
            // SAFETY: output pointers are valid for the duration of the call;
            // the created COM interfaces are owned by the returned wrapper.
            unsafe {
                D3D11CreateDevice(
                    None::<&IDXGIAdapter>,
                    D3D_DRIVER_TYPE_HARDWARE,
                    HMODULE::default(),
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                    Some(&feature_levels),
                    D3D11_SDK_VERSION,
                    Some(&mut device),
                    Some(&mut selected_feature_level),
                    Some(&mut context),
                )
            }
            .map_err(|error| runtime_error("D3D11CreateDevice", error))?;

            Ok(Self {
                device: device.ok_or_else(|| {
                    PlayerRuntimeError::new(
                        PlayerRuntimeErrorCode::BackendFailure,
                        "D3D11CreateDevice did not return a device",
                    )
                })?,
                context: context.ok_or_else(|| {
                    PlayerRuntimeError::new(
                        PlayerRuntimeErrorCode::BackendFailure,
                        "D3D11CreateDevice did not return an immediate context",
                    )
                })?,
                swap_chain: None,
                attached_hwnd: None,
                swap_chain_size: None,
            })
        }

        fn ensure_swap_chain(&mut self) -> PlayerRuntimeResult<()> {
            let hwnd_handle = self.attached_hwnd.ok_or_else(|| {
                PlayerRuntimeError::new(
                    PlayerRuntimeErrorCode::BackendFailure,
                    "windows D3D11 native-frame presenter is not attached to a Win32 HWND",
                )
            })?;
            let hwnd = HWND(hwnd_handle as *mut c_void);
            let size = client_size(hwnd)?;
            if size.0 == 0 || size.1 == 0 {
                return Err(PlayerRuntimeError::new(
                    PlayerRuntimeErrorCode::BackendFailure,
                    "windows D3D11 native-frame presenter cannot present into a zero-size HWND",
                ));
            }
            if let Some(swap_chain) = self.swap_chain.as_ref() {
                if self.swap_chain_size == Some(size) {
                    return Ok(());
                }
                // SAFETY: the swapchain belongs to this presenter and is not
                // accessed concurrently while the runtime is presenting.
                unsafe {
                    swap_chain.ResizeBuffers(
                        2,
                        size.0,
                        size.1,
                        DXGI_FORMAT_B8G8R8A8_UNORM,
                        DXGI_SWAP_CHAIN_FLAG(0),
                    )
                }
                .map_err(|error| runtime_error("IDXGISwapChain::ResizeBuffers", error))?;
                self.swap_chain_size = Some(size);
                return Ok(());
            }

            let dxgi_device: IDXGIDevice = self
                .device
                .cast()
                .map_err(|error| runtime_error("ID3D11Device::cast<IDXGIDevice>", error))?;
            // SAFETY: dxgi_device is valid and returns the adapter that owns
            // the D3D11 device.
            let adapter = unsafe { dxgi_device.GetAdapter() }
                .map_err(|error| runtime_error("IDXGIDevice::GetAdapter", error))?;
            // SAFETY: adapter is a DXGI object; querying its parent factory is
            // the documented way to create a swapchain on the same adapter.
            let factory: IDXGIFactory2 = unsafe { adapter.GetParent() }
                .map_err(|error| runtime_error("IDXGIAdapter::GetParent", error))?;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: size.0,
                Height: size.1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                Stereo: false.into(),
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                Flags: 0,
            };
            // SAFETY: device, hwnd, and desc remain valid for the call; the
            // resulting swapchain is owned by this presenter.
            let swap_chain = unsafe {
                factory.CreateSwapChainForHwnd(
                    &self.device,
                    hwnd,
                    &desc,
                    None,
                    None::<&windows::Win32::Graphics::Dxgi::IDXGIOutput>,
                )
            }
            .map_err(|error| runtime_error("IDXGIFactory2::CreateSwapChainForHwnd", error))?;
            self.swap_chain = Some(swap_chain);
            self.swap_chain_size = Some(size);
            Ok(())
        }

        fn present_texture(&mut self, texture: &ID3D11Texture2D) -> PlayerRuntimeResult<()> {
            self.ensure_swap_chain()?;
            let swap_chain = self.swap_chain()?.clone();
            let back_buffer = self.back_buffer()?;
            let mut desc = Default::default();
            // SAFETY: texture is a valid borrowed D3D11 texture interface.
            unsafe { texture.GetDesc(&mut desc) };
            if is_nv12(desc.Format) {
                return self.present_nv12_texture(texture, &desc, &back_buffer, &swap_chain);
            }
            if !is_bgra_swapchain_compatible(desc.Format) {
                return Err(PlayerRuntimeError::new(
                    PlayerRuntimeErrorCode::BackendFailure,
                    format!(
                        "windows D3D11 presenter expected BGRA or NV12 texture output, got DXGI format {}",
                        desc.Format.0
                    ),
                ));
            }
            let source: ID3D11Resource = texture
                .cast()
                .map_err(|error| runtime_error("ID3D11Texture2D::cast<ID3D11Resource>", error))?;
            let target: ID3D11Resource = back_buffer
                .cast()
                .map_err(|error| runtime_error("ID3D11Texture2D::cast<ID3D11Resource>", error))?;
            // SAFETY: both textures belong to the same D3D11 device when the
            // plugin honors the shared native_device_context contract.
            unsafe {
                self.context.CopyResource(&target, &source);
                self.context.Flush();
            }
            // SAFETY: the swapchain is valid and owned by this presenter.
            unsafe { swap_chain.Present(1, DXGI_PRESENT(0)) }
                .ok()
                .map_err(|error| runtime_error("IDXGISwapChain::Present", error))
        }

        fn present_nv12_texture(
            &mut self,
            texture: &ID3D11Texture2D,
            desc: &windows::Win32::Graphics::Direct3D11::D3D11_TEXTURE2D_DESC,
            back_buffer: &ID3D11Texture2D,
            swap_chain: &IDXGISwapChain1,
        ) -> PlayerRuntimeResult<()> {
            let video_device: ID3D11VideoDevice = self
                .device
                .cast()
                .map_err(|error| runtime_error("ID3D11Device::cast<ID3D11VideoDevice>", error))?;
            let video_context: ID3D11VideoContext = self.context.cast().map_err(|error| {
                runtime_error("ID3D11DeviceContext::cast<ID3D11VideoContext>", error)
            })?;
            let output_size = self.swap_chain_size.ok_or_else(|| {
                PlayerRuntimeError::new(
                    PlayerRuntimeErrorCode::BackendFailure,
                    "windows D3D11 native-frame presenter has no swapchain size",
                )
            })?;
            let content_desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                InputFrameRate: DXGI_RATIONAL {
                    Numerator: 60,
                    Denominator: 1,
                },
                InputWidth: desc.Width,
                InputHeight: desc.Height,
                OutputFrameRate: DXGI_RATIONAL {
                    Numerator: 60,
                    Denominator: 1,
                },
                OutputWidth: output_size.0,
                OutputHeight: output_size.1,
                Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
            };
            // SAFETY: the content description is initialized and lives for the
            // duration of the call.
            let enumerator = unsafe { video_device.CreateVideoProcessorEnumerator(&content_desc) }
                .map_err(|error| {
                    runtime_error("ID3D11VideoDevice::CreateVideoProcessorEnumerator", error)
                })?;
            // SAFETY: the enumerator belongs to this device and index 0 is the
            // default rate-conversion processor.
            let processor = unsafe { video_device.CreateVideoProcessor(&enumerator, 0) }
                .map_err(|error| runtime_error("ID3D11VideoDevice::CreateVideoProcessor", error))?;
            let input_desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                FourCC: 0,
                ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_VPIV {
                        MipSlice: 0,
                        ArraySlice: 0,
                    },
                },
            };
            let output_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
                },
            };
            let source: ID3D11Resource = texture
                .cast()
                .map_err(|error| runtime_error("ID3D11Texture2D::cast<ID3D11Resource>", error))?;
            let target: ID3D11Resource = back_buffer
                .cast()
                .map_err(|error| runtime_error("ID3D11Texture2D::cast<ID3D11Resource>", error))?;
            let mut input_view = None;
            // SAFETY: texture/enumerator are valid D3D11 objects from the same
            // device, and input_desc is fully initialized.
            unsafe {
                video_device.CreateVideoProcessorInputView(
                    &source,
                    &enumerator,
                    &input_desc,
                    Some(&mut input_view),
                )
            }
            .map_err(|error| {
                runtime_error("ID3D11VideoDevice::CreateVideoProcessorInputView", error)
            })?;
            let mut output_view = None;
            // SAFETY: back_buffer/enumerator are valid D3D11 objects from the
            // same device, and output_desc is fully initialized.
            unsafe {
                video_device.CreateVideoProcessorOutputView(
                    &target,
                    &enumerator,
                    &output_desc,
                    Some(&mut output_view),
                )
            }
            .map_err(|error| {
                runtime_error("ID3D11VideoDevice::CreateVideoProcessorOutputView", error)
            })?;
            let input_view = input_view.ok_or_else(|| {
                PlayerRuntimeError::new(
                    PlayerRuntimeErrorCode::BackendFailure,
                    "ID3D11VideoDevice::CreateVideoProcessorInputView returned no view",
                )
            })?;
            let output_view = output_view.ok_or_else(|| {
                PlayerRuntimeError::new(
                    PlayerRuntimeErrorCode::BackendFailure,
                    "ID3D11VideoDevice::CreateVideoProcessorOutputView returned no view",
                )
            })?;
            let mut stream = D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: true.into(),
                OutputIndex: 0,
                InputFrameOrField: 0,
                PastFrames: 0,
                FutureFrames: 0,
                ppPastSurfaces: std::ptr::null_mut(),
                pInputSurface: std::mem::ManuallyDrop::new(Some(input_view)),
                ppFutureSurfaces: std::ptr::null_mut(),
                ppPastSurfacesRight: std::ptr::null_mut(),
                pInputSurfaceRight: std::mem::ManuallyDrop::new(None),
                ppFutureSurfacesRight: std::ptr::null_mut(),
            };
            // SAFETY: processor, output view, and stream input view are valid
            // and belong to the same D3D11 device/context.
            let blt_result = unsafe {
                video_context.VideoProcessorBlt(
                    &processor,
                    &output_view,
                    0,
                    std::slice::from_ref(&stream),
                )
            };
            // `D3D11_VIDEO_PROCESSOR_STREAM` uses ManuallyDrop fields because
            // it mirrors the C ABI. Drop the COM references we placed inside
            // the stream after the synchronous blit returns.
            unsafe {
                std::mem::ManuallyDrop::drop(&mut stream.pInputSurface);
                std::mem::ManuallyDrop::drop(&mut stream.pInputSurfaceRight);
            }
            blt_result
                .map_err(|error| runtime_error("ID3D11VideoContext::VideoProcessorBlt", error))?;
            // SAFETY: the swapchain is valid and owned by this presenter.
            unsafe { swap_chain.Present(1, DXGI_PRESENT(0)) }
                .ok()
                .map_err(|error| runtime_error("IDXGISwapChain::Present", error))
        }

        fn swap_chain(&self) -> PlayerRuntimeResult<&IDXGISwapChain1> {
            self.swap_chain.as_ref().ok_or_else(|| {
                PlayerRuntimeError::new(
                    PlayerRuntimeErrorCode::BackendFailure,
                    "windows D3D11 native-frame presenter has no swapchain",
                )
            })
        }

        fn back_buffer(&self) -> PlayerRuntimeResult<ID3D11Texture2D> {
            let swap_chain = self.swap_chain()?;
            // SAFETY: buffer 0 exists on the swapchain and is returned as an
            // owned D3D11 texture interface for the current back buffer.
            unsafe { swap_chain.GetBuffer(0) }
                .map_err(|error| runtime_error("IDXGISwapChain::GetBuffer", error))
        }
    }

    impl WindowsNativeFramePresenter for WindowsD3D11NativeFramePresenter {
        fn backend_kind(&self) -> WindowsNativeFrameBackendKind {
            WindowsNativeFrameBackendKind::D3D11
        }

        fn accepted_handle_kind(&self) -> DecoderNativeHandleKind {
            DecoderNativeHandleKind::D3D11Texture2D
        }

        fn decoder_device_context(&self) -> Option<DecoderNativeDeviceContext> {
            Some(DecoderNativeDeviceContext {
                kind: DecoderNativeDeviceContextKind::D3D11Device,
                handle: self.device.as_raw() as usize,
            })
        }

        fn attach(&mut self, target: WindowsSurfaceAttachTarget) -> PlayerRuntimeResult<()> {
            if target.kind != player_runtime::PlayerVideoSurfaceKind::Win32Hwnd
                || target.handle == 0
            {
                return Err(PlayerRuntimeError::new(
                    PlayerRuntimeErrorCode::InvalidArgument,
                    "windows D3D11 native-frame presenter requires a non-null Win32 HWND",
                ));
            }
            self.attached_hwnd = Some(target.handle);
            self.swap_chain = None;
            self.swap_chain_size = None;
            self.ensure_swap_chain()
        }

        fn reset(&mut self) -> PlayerRuntimeResult<()> {
            self.swap_chain = None;
            self.swap_chain_size = None;
            Ok(())
        }

        fn present(&mut self, handle: usize) -> PlayerRuntimeResult<()> {
            if handle == 0 {
                return Err(PlayerRuntimeError::new(
                    PlayerRuntimeErrorCode::BackendFailure,
                    "windows D3D11 native-frame presenter received a null texture handle",
                ));
            }
            let raw = handle as *mut c_void;
            // SAFETY: the decoder plugin owns the texture handle until the host
            // calls release_native_frame; cloning AddRefs for the duration of
            // this present call only.
            let texture = unsafe {
                ID3D11Texture2D::from_raw_borrowed(&raw)
                    .ok_or_else(|| {
                        PlayerRuntimeError::new(
                            PlayerRuntimeErrorCode::BackendFailure,
                            "windows D3D11 native-frame presenter received an invalid texture handle",
                        )
                    })?
                    .clone()
            };
            self.present_texture(&texture)
        }
    }

    fn client_size(hwnd: HWND) -> PlayerRuntimeResult<(u32, u32)> {
        let mut rect = RECT::default();
        // SAFETY: hwnd is provided by winit/raw-window-handle and is only used
        // synchronously to read the current client rect.
        unsafe { GetClientRect(hwnd, &mut rect) }
            .map_err(|error| runtime_error("GetClientRect", error))?;
        let width = (rect.right - rect.left).max(0) as u32;
        let height = (rect.bottom - rect.top).max(0) as u32;
        Ok((width, height))
    }

    fn is_bgra_swapchain_compatible(format: DXGI_FORMAT) -> bool {
        format == DXGI_FORMAT_B8G8R8A8_UNORM || format == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
    }

    fn is_nv12(format: DXGI_FORMAT) -> bool {
        format == DXGI_FORMAT_NV12
    }

    fn runtime_error(operation: &str, error: windows::core::Error) -> PlayerRuntimeError {
        PlayerRuntimeError::new(
            PlayerRuntimeErrorCode::BackendFailure,
            format!("{operation} failed: {error}"),
        )
    }
}

struct WindowsRuntimeAdapter {
    inner: Box<dyn PlayerRuntimeAdapter>,
    video_decode: PlayerVideoDecodeInfo,
    runtime_fallback: Option<WindowsRuntimeActiveFallback>,
    pending_runtime_fallback_events: VecDeque<PlayerRuntimeEvent>,
}

#[derive(Debug, Clone)]
pub struct WindowsHostRuntimeProbe {
    pub adapter_id: &'static str,
    pub capabilities: PlayerRuntimeAdapterCapabilities,
    pub media_info: PlayerMediaInfo,
    pub startup: PlayerRuntimeStartup,
}

pub fn windows_runtime_adapter_factory() -> &'static dyn PlayerRuntimeAdapterFactory {
    static FACTORY: WindowsSoftwarePlayerRuntimeAdapterFactory =
        WindowsSoftwarePlayerRuntimeAdapterFactory;
    &FACTORY
}

pub fn install_default_windows_runtime_adapter_factory() -> PlayerRuntimeResult<()> {
    register_default_runtime_adapter_factory(windows_runtime_adapter_factory())
}

pub fn open_windows_host_runtime_uri_with_options(
    uri: impl Into<String>,
    options: PlayerRuntimeOptions,
) -> PlayerRuntimeResult<PlayerRuntimeBootstrap> {
    open_windows_host_runtime_source_with_options(MediaSource::new(uri), options)
}

pub fn probe_windows_host_runtime_uri_with_options(
    uri: impl Into<String>,
    options: PlayerRuntimeOptions,
) -> PlayerRuntimeResult<WindowsHostRuntimeProbe> {
    probe_windows_host_runtime_source_with_options(MediaSource::new(uri), options)
}

pub fn probe_windows_host_runtime_source_with_options(
    source: MediaSource,
    options: PlayerRuntimeOptions,
) -> PlayerRuntimeResult<WindowsHostRuntimeProbe> {
    if !cfg!(target_os = "windows") {
        return Err(PlayerRuntimeError::new(
            PlayerRuntimeErrorCode::Unsupported,
            "windows host runtime strategy can only be probed on Windows targets",
        ));
    }

    let initializer = PlayerRuntimeInitializer::probe_source_with_factory(
        source,
        options,
        windows_runtime_adapter_factory(),
    )?;

    Ok(WindowsHostRuntimeProbe {
        adapter_id: WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
        capabilities: initializer.capabilities(),
        media_info: initializer.media_info(),
        startup: initializer.startup(),
    })
}

pub fn open_windows_host_runtime_source_with_options(
    source: MediaSource,
    options: PlayerRuntimeOptions,
) -> PlayerRuntimeResult<PlayerRuntimeBootstrap> {
    if !cfg!(target_os = "windows") {
        return Err(PlayerRuntimeError::new(
            PlayerRuntimeErrorCode::Unsupported,
            "windows host runtime strategy can only be initialized on Windows targets",
        ));
    }

    PlayerRuntime::open_source_with_factory(source, options, windows_runtime_adapter_factory())
}

#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsSoftwarePlayerRuntimeAdapterFactory;

impl PlayerRuntimeAdapterFactory for WindowsSoftwarePlayerRuntimeAdapterFactory {
    fn adapter_id(&self) -> &'static str {
        WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
    }

    fn probe_source_with_options(
        &self,
        source: MediaSource,
        options: PlayerRuntimeOptions,
    ) -> PlayerRuntimeResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
        if !cfg!(target_os = "windows") {
            return Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::Unsupported,
                "windows desktop adapter can only be initialized on Windows targets",
            ));
        }

        let inner = probe_platform_desktop_source_with_options(
            WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
            source.clone(),
            options.clone(),
        )?;
        let media_info = inner.media_info();
        let selection = select_windows_native_frame_candidate(&media_info, &options);
        if let Some(selection) = selection.clone() {
            let fallback_diagnostics = windows_runtime_diagnostics(&media_info, &options, None);
            let fallback_source = source.clone();
            let fallback_options = options.clone();
            let native_inner = probe_platform_desktop_source_with_video_source_factory_and_options(
                WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                source,
                options,
                Arc::new(WindowsNativeFrameVideoSourceFactory {
                    plugin_path: selection.plugin_path.clone(),
                    preferred_backend: selection.preferred_backend,
                    video_surface: fallback_options
                        .video_surface
                        .expect("windows native-frame selection requires a video surface"),
                }),
                windows_native_frame_decoder_capabilities(selection.preferred_backend),
            )?;
            let diagnostics = windows_runtime_diagnostics(
                &native_inner.media_info(),
                &fallback_options,
                Some(&selection),
            );
            return Ok(Box::new(WindowsRuntimeAdapterInitializer {
                inner: native_inner,
                diagnostics,
                fallback: Some(WindowsRuntimeAdapterFallback {
                    inner,
                    diagnostics: fallback_diagnostics,
                    fallback_reason:
                        "windows native-frame initialization failed; selected software desktop path"
                            .to_owned(),
                }),
                runtime_fallback: Some(WindowsRuntimeActiveFallback {
                    source: fallback_source,
                    options: fallback_options,
                    fallback_reason:
                        "windows native-frame runtime failed during playback; selected software desktop path"
                            .to_owned(),
                }),
            }));
        }

        let diagnostics = windows_runtime_diagnostics(&media_info, &options, None);
        Ok(Box::new(WindowsRuntimeAdapterInitializer {
            inner,
            diagnostics,
            fallback: None,
            runtime_fallback: None,
        }))
    }
}

impl PlayerRuntimeAdapterInitializer for WindowsRuntimeAdapterInitializer {
    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        self.inner.capabilities()
    }

    fn media_info(&self) -> PlayerMediaInfo {
        self.inner.media_info()
    }

    fn startup(&self) -> PlayerRuntimeStartup {
        apply_windows_runtime_diagnostics(self.inner.startup(), &self.diagnostics)
    }

    fn initialize(self: Box<Self>) -> PlayerRuntimeResult<PlayerRuntimeAdapterBootstrap> {
        let Self {
            inner,
            diagnostics,
            fallback,
            runtime_fallback,
        } = *self;
        match inner.initialize() {
            Ok(bootstrap) => Ok(wrap_windows_runtime_bootstrap(
                bootstrap,
                diagnostics,
                runtime_fallback,
            )),
            Err(native_error) => {
                let Some(fallback) = fallback else {
                    return Err(native_error);
                };
                let mut diagnostics = fallback.diagnostics;
                diagnostics.video_decode.fallback_reason = Some(merge_runtime_fallback_reason(
                    fallback.fallback_reason.as_str(),
                    native_error.message(),
                    diagnostics.video_decode.fallback_reason.take(),
                ));
                let mut bootstrap = fallback.inner.initialize()?;
                bootstrap.startup =
                    apply_windows_runtime_diagnostics(bootstrap.startup, &diagnostics);
                Ok(wrap_windows_runtime_bootstrap(bootstrap, diagnostics, None))
            }
        }
    }
}

impl DesktopVideoSourceFactory for WindowsNativeFrameVideoSourceFactory {
    fn open_video_source(
        &self,
        source: MediaSource,
        _buffer_capacity: usize,
        interrupt_flag: Option<Arc<std::sync::atomic::AtomicBool>>,
    ) -> anyhow::Result<DesktopVideoSourceBootstrap> {
        let backend = FfmpegBackend::new().context("failed to initialize FFmpeg backend")?;
        let probe = backend
            .probe_with_interrupt(source.clone(), interrupt_flag.clone())
            .context("failed to probe media source for Windows native-frame decoder")?;
        let packet_source = backend
            .open_video_packet_source_with_interrupt(source, interrupt_flag)
            .context("failed to open FFmpeg packet source for Windows native-frame decoder")?;
        let stream_info = packet_source.stream_info().clone();
        let plugin = LoadedDynamicPlugin::load(&self.plugin_path).with_context(|| {
            format!(
                "failed to load Windows native-frame decoder plugin {}",
                self.plugin_path.display()
            )
        })?;
        let factory = plugin.native_decoder_plugin_factory().ok_or_else(|| {
            anyhow::anyhow!("decoder plugin does not export a v2 native-frame API")
        })?;
        if !factory
            .capabilities()
            .supports_codec(&stream_info.codec, DecoderMediaKind::Video)
        {
            anyhow::bail!(
                "windows native-frame decoder plugin `{}` does not support {} video",
                factory.name(),
                stream_info.codec
            );
        }

        let mut presenter = windows_native_frame_presenter_for_backend(self.preferred_backend)?;
        let surface_target = WindowsSurfaceAttachTarget {
            kind: self.video_surface.kind,
            handle: self.video_surface.handle,
        };
        presenter.attach(surface_target)?;
        let session = factory
            .open_native_session(&DecoderSessionConfig {
                codec: stream_info.codec.clone(),
                media_kind: DecoderMediaKind::Video,
                extradata: stream_info.extradata.clone(),
                width: stream_info.width,
                height: stream_info.height,
                prefer_hardware: true,
                require_cpu_output: false,
                native_device_context: presenter.decoder_device_context(),
                ..DecoderSessionConfig::default()
            })
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let session_info = session.session_info();
        let decode_info = BackendVideoDecodeInfo {
            selected_mode: BackendVideoDecoderMode::Hardware,
            hardware_available: true,
            hardware_backend: session_info
                .selected_hardware_backend
                .or_else(|| Some(format!("{:?}", self.preferred_backend))),
            decoder_name: session_info
                .decoder_name
                .unwrap_or_else(|| format!("{:?}", self.preferred_backend)),
            fallback_reason: None,
        };

        Ok(DesktopVideoSourceBootstrap {
            source: Box::new(WindowsNativeFrameVideoSource {
                packet_source,
                stream_info,
                session,
                presenter,
                surface_target,
                end_of_input_sent: false,
            }),
            decode_info,
            probe,
        })
    }
}

impl DesktopVideoSource for WindowsNativeFrameVideoSource {
    fn recv_frame(&mut self) -> anyhow::Result<Option<DesktopVideoFrame>> {
        loop {
            match self.poll_frame(true)? {
                DesktopVideoFramePoll::Ready(frame) => return Ok(Some(frame)),
                DesktopVideoFramePoll::Pending => continue,
                DesktopVideoFramePoll::EndOfStream => return Ok(None),
            }
        }
    }

    fn try_recv_frame(&mut self) -> anyhow::Result<DesktopVideoFramePoll> {
        self.poll_frame(false)
    }

    fn seek_to(
        &mut self,
        position: std::time::Duration,
    ) -> anyhow::Result<Option<DesktopVideoFrame>> {
        self.session
            .flush()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        self.presenter.reset()?;
        self.presenter.attach(self.surface_target)?;
        self.packet_source.seek_to(position)?;
        self.end_of_input_sent = false;
        self.recv_frame()
    }

    fn buffered_frame_count(&self) -> usize {
        0
    }

    fn set_prefetch_limit(&self, _limit: usize) {}
}

impl WindowsNativeFrameVideoSource {
    fn poll_frame(&mut self, blocking: bool) -> anyhow::Result<DesktopVideoFramePoll> {
        let mut packets_submitted = 0usize;
        loop {
            match self
                .session
                .receive_native_frame()
                .map_err(|error| anyhow::anyhow!(error.to_string()))?
            {
                DecoderReceiveNativeFrameOutput::Frame(frame) => {
                    return windows_native_frame_poll_with_presenter(
                        self.session.as_mut(),
                        self.presenter.as_mut(),
                        &self.stream_info,
                        frame,
                    );
                }
                DecoderReceiveNativeFrameOutput::Eof => {
                    return Ok(DesktopVideoFramePoll::EndOfStream);
                }
                DecoderReceiveNativeFrameOutput::NeedMoreInput => {}
            }

            if self.end_of_input_sent {
                return Ok(DesktopVideoFramePoll::Pending);
            }

            match self.packet_source.next_packet()? {
                Some(packet) => {
                    self.send_packet(packet)?;
                    packets_submitted = packets_submitted.saturating_add(1);
                    if !blocking && packets_submitted >= 4 {
                        return Ok(DesktopVideoFramePoll::Pending);
                    }
                }
                None => {
                    self.send_end_of_stream()?;
                    self.end_of_input_sent = true;
                }
            }
        }
    }

    fn send_packet(&mut self, packet: CompressedVideoPacket) -> anyhow::Result<()> {
        self.session
            .send_packet(
                &DecoderPacket {
                    pts_us: packet.pts_us,
                    dts_us: packet.dts_us,
                    duration_us: packet.duration_us,
                    stream_index: packet.stream_index,
                    key_frame: packet.key_frame,
                    discontinuity: packet.discontinuity,
                    end_of_stream: false,
                },
                &packet.data,
            )
            .map(|_| ())
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    }

    fn send_end_of_stream(&mut self) -> anyhow::Result<()> {
        self.session
            .send_packet(
                &DecoderPacket {
                    stream_index: u32::try_from(self.stream_info.stream_index).unwrap_or(u32::MAX),
                    end_of_stream: true,
                    ..DecoderPacket::default()
                },
                &[],
            )
            .map(|_| ())
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    }
}

fn windows_native_frame_poll_with_presenter(
    session: &mut dyn NativeDecoderSession,
    presenter: &mut dyn WindowsNativeFramePresenter,
    _stream_info: &VideoPacketStreamInfo,
    frame: player_plugin::DecoderNativeFrame,
) -> anyhow::Result<DesktopVideoFramePoll> {
    let presentation_time = frame
        .metadata
        .pts_us
        .and_then(duration_from_micros)
        .unwrap_or(std::time::Duration::ZERO);
    let width = frame.metadata.width;
    let height = frame.metadata.height;
    if frame.metadata.handle_kind != presenter.accepted_handle_kind() {
        let _ = session.release_native_frame(frame);
        anyhow::bail!(
            "windows {:?} native-frame presenter only accepts {:?} handles",
            presenter.backend_kind(),
            presenter.accepted_handle_kind()
        );
    }
    let present_result = presenter
        .present(frame.handle)
        .map_err(|error| anyhow::anyhow!(error.message().to_owned()));
    let release_result = session
        .release_native_frame(frame)
        .map_err(|error| anyhow::anyhow!(error.to_string()));
    present_result.and(release_result)?;
    Ok(DesktopVideoFramePoll::Ready(
        DesktopVideoFrame::native_presented(presentation_time, width, height),
    ))
}

fn duration_from_micros(value: i64) -> Option<std::time::Duration> {
    if value < 0 {
        return None;
    }
    Some(std::time::Duration::from_micros(value as u64))
}

impl PlayerRuntimeAdapter for WindowsRuntimeAdapter {
    fn source_uri(&self) -> &str {
        self.inner.source_uri()
    }

    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        self.inner.capabilities()
    }

    fn media_info(&self) -> &PlayerMediaInfo {
        self.inner.media_info()
    }

    fn presentation_state(&self) -> player_runtime::PresentationState {
        self.inner.presentation_state()
    }

    fn has_video_surface(&self) -> bool {
        self.inner.has_video_surface()
    }

    fn is_interrupted(&self) -> bool {
        self.inner.is_interrupted()
    }

    fn is_buffering(&self) -> bool {
        self.inner.is_buffering()
    }

    fn playback_rate(&self) -> f32 {
        self.inner.playback_rate()
    }

    fn progress(&self) -> player_runtime::PlaybackProgress {
        self.inner.progress()
    }

    fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
        let mut events = self
            .inner
            .drain_events()
            .into_iter()
            .map(|event| match event {
                PlayerRuntimeEvent::Initialized(startup) => PlayerRuntimeEvent::Initialized(
                    apply_video_decode_diagnostics(startup, &self.video_decode),
                ),
                other => other,
            })
            .collect::<Vec<_>>();
        while let Some(event) = self.pending_runtime_fallback_events.pop_back() {
            events.insert(0, event);
        }
        events
    }

    fn dispatch(
        &mut self,
        command: player_runtime::PlayerRuntimeCommand,
    ) -> PlayerRuntimeResult<player_runtime::PlayerRuntimeCommandResult> {
        self.inner.dispatch(command)
    }

    fn advance(&mut self) -> PlayerRuntimeResult<Option<player_runtime::DecodedVideoFrame>> {
        match self.inner.advance() {
            Ok(frame) => Ok(frame),
            Err(error)
                if should_trigger_windows_runtime_fallback(&error)
                    && self.runtime_fallback.is_some() =>
            {
                self.activate_runtime_fallback(error.message())?;
                self.inner.advance()
            }
            Err(error) => Err(error),
        }
    }

    fn next_deadline(&self) -> Option<std::time::Instant> {
        self.inner.next_deadline()
    }
}

impl WindowsRuntimeAdapter {
    fn activate_runtime_fallback(
        &mut self,
        runtime_error_message: &str,
    ) -> PlayerRuntimeResult<()> {
        let Some(fallback) = self.runtime_fallback.take() else {
            return Ok(());
        };

        let mut bootstrap =
            player_platform_desktop::open_platform_desktop_source_with_options_and_interrupt(
                WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                fallback.source,
                fallback.options,
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
            )?;
        let fallback_reason = merge_runtime_fallback_reason(
            fallback.fallback_reason.as_str(),
            runtime_error_message,
            None,
        );
        bootstrap.startup = apply_windows_runtime_diagnostics(
            bootstrap.startup,
            &WindowsRuntimeDiagnostics {
                video_decode: PlayerVideoDecodeInfo {
                    selected_mode: PlayerVideoDecodeMode::Software,
                    hardware_available: true,
                    hardware_backend: Some(format!(
                        "{:?}",
                        windows_native_frame_roadmap().preferred_backend
                    )),
                    fallback_reason: Some(fallback_reason),
                },
                plugin_diagnostics: Vec::new(),
            },
        );

        self.inner = bootstrap.runtime;
        if let Some(video_decode) = bootstrap.startup.video_decode.as_ref() {
            self.video_decode = video_decode.clone();
        }
        self.pending_runtime_fallback_events.extend(
            player_platform_desktop::runtime_fallback_events(runtime_error_message),
        );
        Ok(())
    }
}

fn should_trigger_windows_runtime_fallback(error: &PlayerRuntimeError) -> bool {
    if error.code() != PlayerRuntimeErrorCode::BackendFailure {
        return false;
    }
    let message = error.message().to_ascii_lowercase();
    message.contains("windows native-frame presenter/runtime skeleton is not implemented yet")
        || message.contains("windows d3d11 native-frame presenter")
        || message.contains("failed to open windows native-frame video source")
}

fn select_windows_native_frame_candidate(
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
) -> Option<WindowsNativeFrameSelection> {
    if options.decoder_plugin_video_mode != PlayerDecoderPluginVideoMode::PreferNativeFrame {
        return None;
    }
    let registry = windows_decoder_plugin_registry(media_info, options)?;
    select_windows_native_frame_candidate_from_registry(media_info, options, &registry)
}

fn select_windows_native_frame_candidate_from_registry(
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
    registry: &PluginRegistry,
) -> Option<WindowsNativeFrameSelection> {
    if options.decoder_plugin_video_mode != PlayerDecoderPluginVideoMode::PreferNativeFrame {
        return None;
    }
    if !options
        .video_surface
        .is_some_and(is_windows_video_surface_target)
    {
        return None;
    }
    let best_video = media_info.best_video.as_ref()?;
    let request = DecoderPluginMatchRequest::video(best_video.codec.clone());
    let record = registry.best_native_decoder_for(&request)?;
    Some(WindowsNativeFrameSelection {
        preferred_backend: windows_native_frame_roadmap().preferred_backend,
        plugin_path: record.path.clone(),
    })
}

fn windows_runtime_diagnostics(
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
    selection: Option<&WindowsNativeFrameSelection>,
) -> WindowsRuntimeDiagnostics {
    let roadmap = windows_native_frame_roadmap();
    let mut plugin_diagnostics = Vec::new();
    let selected_mode = if selection.is_some() {
        PlayerVideoDecodeMode::Hardware
    } else {
        PlayerVideoDecodeMode::Software
    };
    let mut fallback_reason = if selection.is_some() {
        None
    } else {
        media_info.best_video.as_ref().map(|video| {
            merge_runtime_fallback_reason(
                "selected FFmpeg software path",
                &format!(
                    "Windows native-frame target prefers {:?} with handles {} for {} video",
                    roadmap.preferred_backend,
                    roadmap.accepted_handle_kinds.join(", "),
                    video.codec
                ),
                None,
            )
        })
    };

    if let Some(registry) = windows_decoder_plugin_registry(media_info, options) {
        plugin_diagnostics.extend(
            registry
                .records()
                .iter()
                .map(player_plugin_diagnostic_from_record),
        );
        if selection.is_none() {
            fallback_reason = apply_windows_decoder_plugin_note(
                fallback_reason,
                media_info,
                options,
                selection,
                &registry,
            );
        }
    } else if selection.is_none() {
        fallback_reason = apply_windows_native_frame_preference_note(
            fallback_reason,
            media_info,
            options,
            selection,
        );
    }

    WindowsRuntimeDiagnostics {
        video_decode: PlayerVideoDecodeInfo {
            selected_mode,
            hardware_available: media_info.best_video.is_some(),
            hardware_backend: Some(format!(
                "{:?}",
                selection
                    .map(|selection| selection.preferred_backend)
                    .unwrap_or(roadmap.preferred_backend)
            )),
            fallback_reason,
        },
        plugin_diagnostics,
    }
}

fn windows_decoder_plugin_registry(
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
) -> Option<PluginRegistry> {
    let best_video = media_info.best_video.as_ref()?;
    if options.decoder_plugin_library_paths.is_empty() {
        return None;
    }
    Some(PluginRegistry::inspect_decoder_support(
        &options.decoder_plugin_library_paths,
        DecoderPluginMatchRequest::video(best_video.codec.clone()),
    ))
}

fn apply_windows_decoder_plugin_note(
    fallback_reason: Option<String>,
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
    selection: Option<&WindowsNativeFrameSelection>,
    registry: &PluginRegistry,
) -> Option<String> {
    let fallback_reason =
        apply_windows_decoder_registry_note(fallback_reason, media_info, registry);
    apply_windows_native_frame_preference_note(fallback_reason, media_info, options, selection)
}

fn apply_windows_decoder_registry_note(
    fallback_reason: Option<String>,
    media_info: &PlayerMediaInfo,
    registry: &PluginRegistry,
) -> Option<String> {
    let best_video = media_info.best_video.as_ref()?;
    let request = DecoderPluginMatchRequest::video(best_video.codec.clone());
    let report = registry.report();
    let note = if registry.supports_decoder(&request) {
        let supported = registry
            .records()
            .iter()
            .filter(|record| record.status == PluginDiagnosticStatus::DecoderSupported)
            .map(plugin_diagnostic_label)
            .collect::<Vec<_>>();
        format!(
            "decoder plugin found {}/{} candidate(s) for {} video: {}; native-frame playback requires PreferNativeFrame and a Win32 HWND",
            report.decoder_supported,
            report.total,
            best_video.codec,
            supported.join(", ")
        )
    } else {
        format!(
            "decoder plugin paths configured for {} video: {}/{} supported, {} unsupported codec, {} load failed, {} non-decoder",
            best_video.codec,
            report.decoder_supported,
            report.total,
            report.decoder_unsupported,
            report.failed,
            report.unsupported_kind
        )
    };
    Some(match fallback_reason {
        Some(existing) if !existing.is_empty() => format!("{existing}; {note}"),
        _ => note,
    })
}

fn apply_windows_native_frame_preference_note(
    fallback_reason: Option<String>,
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
    selection: Option<&WindowsNativeFrameSelection>,
) -> Option<String> {
    if options.decoder_plugin_video_mode != PlayerDecoderPluginVideoMode::PreferNativeFrame {
        return fallback_reason;
    }
    let Some(best_video) = media_info.best_video.as_ref() else {
        return fallback_reason;
    };
    let reason = if options.decoder_plugin_library_paths.is_empty() {
        Some(format!(
            "native-frame decoder plugin playback requested for {} video but no decoder plugin paths are configured; selected FFmpeg software path",
            best_video.codec
        ))
    } else if options.video_surface.is_none() {
        Some(format!(
            "native-frame decoder plugin playback requested for {} video but no Windows video surface is available; selected FFmpeg software path",
            best_video.codec
        ))
    } else if !options
        .video_surface
        .is_some_and(is_windows_video_surface_target)
    {
        Some(format!(
            "native-frame decoder plugin playback requested for {} video but the configured surface is not a Win32 HWND; selected FFmpeg software path",
            best_video.codec
        ))
    } else if selection.is_none() {
        Some(format!(
            "native-frame decoder plugin playback requested for {} video but no matching Windows native-frame decoder is available; selected FFmpeg software path",
            best_video.codec
        ))
    } else {
        None
    };

    let Some(reason) = reason else {
        return fallback_reason;
    };
    Some(match fallback_reason {
        Some(existing) if !existing.is_empty() => format!("{existing}; {reason}"),
        _ => reason,
    })
}

fn plugin_diagnostic_label(record: &PluginDiagnosticRecord) -> String {
    record
        .plugin_name
        .clone()
        .unwrap_or_else(|| record.path.display().to_string())
}

fn player_plugin_diagnostic_from_record(record: &PluginDiagnosticRecord) -> PlayerPluginDiagnostic {
    PlayerPluginDiagnostic {
        path: record.path.display().to_string(),
        plugin_name: record.plugin_name.clone(),
        plugin_kind: record.plugin_kind.map(plugin_kind_label).map(str::to_owned),
        status: match record.status {
            PluginDiagnosticStatus::Loaded => PlayerPluginDiagnosticStatus::Loaded,
            PluginDiagnosticStatus::LoadFailed => PlayerPluginDiagnosticStatus::LoadFailed,
            PluginDiagnosticStatus::UnsupportedKind => {
                PlayerPluginDiagnosticStatus::UnsupportedKind
            }
            PluginDiagnosticStatus::DecoderSupported => {
                PlayerPluginDiagnosticStatus::DecoderSupported
            }
            PluginDiagnosticStatus::DecoderUnsupported => {
                PlayerPluginDiagnosticStatus::DecoderUnsupported
            }
        },
        message: record.message.clone(),
        decoder_capabilities: record
            .decoder_capabilities
            .as_ref()
            .map(player_decoder_capability_summary_from_loader),
    }
}

fn player_decoder_capability_summary_from_loader(
    summary: &DecoderPluginCapabilitySummary,
) -> PlayerPluginDecoderCapabilitySummary {
    PlayerPluginDecoderCapabilitySummary {
        codecs: summary
            .typed_codecs
            .iter()
            .map(player_decoder_codec_summary_from_loader)
            .collect(),
        legacy_codecs: summary.codecs.clone(),
        supports_native_frame_output: summary.supports_native_frame_output,
        supports_hardware_decode: summary.supports_hardware_decode,
        supports_cpu_video_frames: summary.supports_cpu_video_frames,
        supports_audio_frames: summary.supports_audio_frames,
        supports_gpu_handles: summary.supports_gpu_handles,
        supports_flush: summary.supports_flush,
        supports_drain: summary.supports_drain,
        max_sessions: summary.max_sessions,
    }
}

fn player_decoder_codec_summary_from_loader(
    summary: &DecoderPluginCodecSummary,
) -> PlayerPluginCodecCapability {
    PlayerPluginCodecCapability {
        media_kind: match summary.media_kind {
            DecoderMediaKind::Video => "video",
            DecoderMediaKind::Audio => "audio",
        }
        .to_owned(),
        codec: summary.codec.clone(),
    }
}

fn plugin_kind_label(kind: VesperPluginKind) -> &'static str {
    match kind {
        VesperPluginKind::PostDownloadProcessor => "post_download_processor",
        VesperPluginKind::PipelineEventHook => "pipeline_event_hook",
        VesperPluginKind::Decoder => "decoder",
    }
}

fn is_windows_video_surface_target(surface: player_runtime::PlayerVideoSurfaceTarget) -> bool {
    surface.kind == player_runtime::PlayerVideoSurfaceKind::Win32Hwnd && surface.handle != 0
}

fn windows_native_frame_decoder_capabilities(
    _backend: WindowsNativeFrameBackendKind,
) -> PlayerRuntimeAdapterCapabilities {
    PlayerRuntimeAdapterCapabilities {
        adapter_id: WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
        backend_family: player_runtime::PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
        supports_audio_output: true,
        supports_frame_output: false,
        supports_external_video_surface: true,
        supports_seek: true,
        supports_stop: true,
        supports_playback_rate: true,
        playback_rate_min: Some(player_runtime::MIN_PLAYBACK_RATE),
        playback_rate_max: Some(player_runtime::MAX_PLAYBACK_RATE),
        natural_playback_rate_max: Some(player_runtime::NATURAL_PLAYBACK_RATE_MAX),
        supports_hardware_decode: true,
        supports_streaming: true,
        supports_hdr: true,
    }
}

fn apply_windows_runtime_diagnostics(
    mut startup: PlayerRuntimeStartup,
    diagnostics: &WindowsRuntimeDiagnostics,
) -> PlayerRuntimeStartup {
    startup.video_decode = Some(match startup.video_decode.take() {
        Some(existing) if existing.fallback_reason.is_some() => existing,
        Some(mut existing) => {
            existing.hardware_backend = diagnostics.video_decode.hardware_backend.clone();
            existing.hardware_available = diagnostics.video_decode.hardware_available;
            existing.fallback_reason = diagnostics.video_decode.fallback_reason.clone();
            existing
        }
        None => diagnostics.video_decode.clone(),
    });
    for diagnostic in &diagnostics.plugin_diagnostics {
        if startup.plugin_diagnostics.iter().any(|existing| {
            existing.path == diagnostic.path && existing.status == diagnostic.status
        }) {
            continue;
        }
        startup.plugin_diagnostics.push(diagnostic.clone());
    }
    startup
}

fn apply_video_decode_diagnostics(
    mut startup: PlayerRuntimeStartup,
    video_decode: &PlayerVideoDecodeInfo,
) -> PlayerRuntimeStartup {
    if startup.video_decode.is_none() {
        startup.video_decode = Some(video_decode.clone());
    }
    startup
}

fn wrap_windows_runtime_bootstrap(
    bootstrap: PlayerRuntimeAdapterBootstrap,
    diagnostics: WindowsRuntimeDiagnostics,
    runtime_fallback: Option<WindowsRuntimeActiveFallback>,
) -> PlayerRuntimeAdapterBootstrap {
    let PlayerRuntimeAdapterBootstrap {
        runtime,
        initial_frame,
        startup,
    } = bootstrap;
    PlayerRuntimeAdapterBootstrap {
        runtime: Box::new(WindowsRuntimeAdapter {
            inner: runtime,
            video_decode: diagnostics.video_decode.clone(),
            runtime_fallback,
            pending_runtime_fallback_events: VecDeque::new(),
        }),
        initial_frame,
        startup: apply_windows_runtime_diagnostics(startup, &diagnostics),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        WINDOWS_NATIVE_FRAME_PLAYER_RUNTIME_ADAPTER_ID, WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
        WindowsD3D11NativeFramePresenterSkeleton, WindowsD3D11PresenterState,
        WindowsNativeFrameBackendKind, WindowsNativeFramePresenter, WindowsRuntimeActiveFallback,
        WindowsRuntimeAdapter, WindowsSoftwarePlayerRuntimeAdapterFactory,
        WindowsSurfaceAttachTarget, open_windows_host_runtime_source_with_options,
        probe_windows_host_runtime_source_with_options,
        select_windows_native_frame_candidate_from_registry,
        windows_native_frame_poll_with_presenter, windows_native_frame_presenter_for_backend,
        windows_native_frame_roadmap, windows_runtime_diagnostics,
    };
    use player_backend_ffmpeg::VideoPacketStreamInfo;
    use player_core::MediaSource;
    use player_platform_desktop::merge_runtime_fallback_reason;
    use player_plugin::{
        DecoderMediaKind, DecoderNativeDeviceContext, DecoderNativeFrame,
        DecoderNativeFrameMetadata, DecoderNativeHandleKind, DecoderReceiveNativeFrameOutput,
        DecoderSessionInfo, NativeDecoderSession, VesperPluginKind,
    };
    use player_plugin_loader::{
        DecoderPluginCapabilitySummary, DecoderPluginCodecSummary, PluginDiagnosticRecord,
        PluginDiagnosticStatus, PluginRegistry,
    };
    use player_runtime::{
        PlayerDecoderPluginVideoMode, PlayerRuntimeAdapter, PlayerRuntimeAdapterBackendFamily,
        PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory, PlayerRuntimeCommand,
        PlayerRuntimeCommandResult, PlayerRuntimeError, PlayerRuntimeErrorCode, PlayerRuntimeEvent,
        PlayerRuntimeOptions, PlayerRuntimeResult, PlayerVideoDecodeInfo, PlayerVideoDecodeMode,
    };
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    #[test]
    fn windows_factory_matches_host_support() {
        let factory = WindowsSoftwarePlayerRuntimeAdapterFactory;

        if cfg!(target_os = "windows") {
            let Some(test_video_path) = test_video_path() else {
                eprintln!("skipping Windows fixture-backed test: test-video.mp4 is unavailable");
                return;
            };
            let result = factory.probe_source_with_options(
                MediaSource::new(test_video_path),
                PlayerRuntimeOptions::default(),
            );
            let initializer =
                result.expect("windows host should support the windows desktop adapter");
            let capabilities = initializer.capabilities();
            assert_eq!(
                capabilities.adapter_id,
                WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
            );
            assert_eq!(
                capabilities.backend_family,
                PlayerRuntimeAdapterBackendFamily::SoftwareDesktop
            );
        } else {
            let result = factory.probe_source_with_options(
                MediaSource::new("fixture.mp4"),
                PlayerRuntimeOptions::default(),
            );
            let error = match result {
                Ok(_) => panic!("non-windows hosts should reject the windows adapter"),
                Err(error) => error,
            };
            assert_eq!(error.code(), PlayerRuntimeErrorCode::Unsupported);
        }
    }

    #[test]
    fn windows_host_probe_matches_factory_support() {
        if cfg!(target_os = "windows") {
            let Some(test_video_path) = test_video_path() else {
                eprintln!("skipping Windows fixture-backed test: test-video.mp4 is unavailable");
                return;
            };
            let result = probe_windows_host_runtime_source_with_options(
                MediaSource::new(test_video_path),
                PlayerRuntimeOptions::default(),
            );
            let probe = result.expect("windows host should support the windows host runtime probe");
            assert_eq!(probe.adapter_id, WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID);
            assert_eq!(
                probe.capabilities.backend_family,
                PlayerRuntimeAdapterBackendFamily::SoftwareDesktop
            );
        } else {
            let result = probe_windows_host_runtime_source_with_options(
                MediaSource::new("fixture.mp4"),
                PlayerRuntimeOptions::default(),
            );
            let error = result.expect_err("non-windows hosts should reject the windows host probe");
            assert_eq!(error.code(), PlayerRuntimeErrorCode::Unsupported);
        }
    }

    #[test]
    fn windows_host_open_matches_factory_support() {
        if cfg!(target_os = "windows") {
            let Some(test_video_path) = test_video_path() else {
                eprintln!("skipping Windows fixture-backed test: test-video.mp4 is unavailable");
                return;
            };
            let result = open_windows_host_runtime_source_with_options(
                MediaSource::new(test_video_path),
                PlayerRuntimeOptions::default(),
            );
            let bootstrap =
                result.expect("windows host should support the windows host runtime open helper");
            assert_eq!(
                bootstrap.runtime.adapter_id(),
                WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
            );
        } else {
            let result = open_windows_host_runtime_source_with_options(
                MediaSource::new("fixture.mp4"),
                PlayerRuntimeOptions::default(),
            );
            let error = match result {
                Ok(_) => panic!("non-windows hosts should reject the windows host opener"),
                Err(error) => error,
            };
            assert_eq!(error.code(), PlayerRuntimeErrorCode::Unsupported);
        }
    }

    #[test]
    fn windows_native_frame_roadmap_prefers_d3d11_handles() {
        let roadmap = windows_native_frame_roadmap();

        assert_eq!(
            roadmap.adapter_id,
            WINDOWS_NATIVE_FRAME_PLAYER_RUNTIME_ADAPTER_ID
        );
        assert_eq!(format!("{:?}", roadmap.preferred_backend), "D3D11");
        assert_eq!(
            roadmap.accepted_handle_kinds,
            ["D3D11Texture2D", "DxgiSurface"]
        );
    }

    #[test]
    fn windows_runtime_diagnostics_stay_software_while_advertising_roadmap() {
        let diagnostics = windows_runtime_diagnostics(
            &player_runtime::PlayerMediaInfo {
                source_uri: "fixture.mp4".to_owned(),
                source_kind: player_runtime::MediaSourceKind::Local,
                source_protocol: player_runtime::MediaSourceProtocol::File,
                duration: None,
                bit_rate: None,
                audio_streams: 0,
                video_streams: 1,
                best_video: Some(player_runtime::PlayerVideoInfo {
                    codec: "H264".to_owned(),
                    width: 1920,
                    height: 1080,
                    frame_rate: Some(60.0),
                }),
                best_audio: None,
                track_catalog: Default::default(),
                track_selection: Default::default(),
            },
            &PlayerRuntimeOptions::default(),
            None,
        );

        assert_eq!(
            diagnostics.video_decode.selected_mode,
            PlayerVideoDecodeMode::Software
        );
        assert_eq!(
            diagnostics.video_decode.hardware_backend.as_deref(),
            Some("D3D11")
        );
        let fallback = diagnostics
            .video_decode
            .fallback_reason
            .as_deref()
            .unwrap_or_default();
        assert!(fallback.contains("selected FFmpeg software path"));
        assert!(fallback.contains("D3D11Texture2D"));
        assert!(fallback.contains("DxgiSurface"));
    }

    #[test]
    fn windows_native_frame_candidate_uses_hardware_diagnostics() {
        let media_info = player_runtime::PlayerMediaInfo {
            source_uri: "file:///tmp/test.mp4".to_owned(),
            source_kind: player_runtime::MediaSourceKind::Local,
            source_protocol: player_runtime::MediaSourceProtocol::File,
            duration: None,
            bit_rate: None,
            audio_streams: 0,
            video_streams: 1,
            best_video: Some(player_runtime::PlayerVideoInfo {
                codec: "H264".to_owned(),
                width: 1920,
                height: 1080,
                frame_rate: Some(60.0),
            }),
            best_audio: None,
            track_catalog: Default::default(),
            track_selection: Default::default(),
        };
        let options = PlayerRuntimeOptions::default()
            .with_decoder_plugin_video_mode(PlayerDecoderPluginVideoMode::PreferNativeFrame)
            .with_decoder_plugin_library_paths([std::path::PathBuf::from(
                "/tmp/fake-d3d11-decoder",
            )])
            .with_video_surface(player_runtime::PlayerVideoSurfaceTarget {
                kind: player_runtime::PlayerVideoSurfaceKind::Win32Hwnd,
                handle: 1,
            });
        let registry = windows_native_plugin_registry("H264");
        let selection =
            select_windows_native_frame_candidate_from_registry(&media_info, &options, &registry);
        let diagnostics = windows_runtime_diagnostics(&media_info, &options, selection.as_ref());

        assert!(selection.is_some());
        assert_eq!(diagnostics.plugin_diagnostics.len(), 1);
        assert_eq!(
            diagnostics.video_decode.selected_mode,
            PlayerVideoDecodeMode::Hardware
        );
        assert_eq!(
            diagnostics.video_decode.hardware_backend.as_deref(),
            Some("D3D11")
        );
        assert!(diagnostics.video_decode.fallback_reason.is_none());
    }

    #[test]
    fn windows_native_frame_candidate_requires_explicit_opt_in() {
        let media_info = media_info_with_video_codec("H264");
        let options = PlayerRuntimeOptions::default()
            .with_decoder_plugin_library_paths([std::path::PathBuf::from(
                "/tmp/fake-d3d11-decoder",
            )])
            .with_video_surface(player_runtime::PlayerVideoSurfaceTarget {
                kind: player_runtime::PlayerVideoSurfaceKind::Win32Hwnd,
                handle: 1,
            });
        let registry = windows_native_plugin_registry("H264");

        let selection =
            select_windows_native_frame_candidate_from_registry(&media_info, &options, &registry);

        assert!(selection.is_none());
    }

    #[test]
    fn windows_native_frame_prefer_mode_without_surface_records_fallback() {
        let media_info = media_info_with_video_codec("H264");
        let options = PlayerRuntimeOptions::default()
            .with_decoder_plugin_video_mode(PlayerDecoderPluginVideoMode::PreferNativeFrame)
            .with_decoder_plugin_library_paths([std::path::PathBuf::from(
                "/tmp/fake-d3d11-decoder",
            )]);
        let registry = windows_native_plugin_registry("H264");
        let selection =
            select_windows_native_frame_candidate_from_registry(&media_info, &options, &registry);
        let diagnostics = windows_runtime_diagnostics(&media_info, &options, selection.as_ref());

        assert!(selection.is_none());
        assert!(
            diagnostics
                .video_decode
                .fallback_reason
                .as_deref()
                .unwrap_or_default()
                .contains("no Windows video surface is available")
        );
    }

    #[test]
    fn windows_native_frame_prefer_mode_rejects_non_windows_surface() {
        let media_info = media_info_with_video_codec("H264");
        let options = PlayerRuntimeOptions::default()
            .with_decoder_plugin_video_mode(PlayerDecoderPluginVideoMode::PreferNativeFrame)
            .with_decoder_plugin_library_paths([std::path::PathBuf::from(
                "/tmp/fake-d3d11-decoder",
            )])
            .with_video_surface(player_runtime::PlayerVideoSurfaceTarget {
                kind: player_runtime::PlayerVideoSurfaceKind::MetalLayer,
                handle: 1,
            });
        let registry = windows_native_plugin_registry("H264");
        let selection =
            select_windows_native_frame_candidate_from_registry(&media_info, &options, &registry);
        let diagnostics = windows_runtime_diagnostics(&media_info, &options, selection.as_ref());

        assert!(selection.is_none());
        assert!(
            diagnostics
                .video_decode
                .fallback_reason
                .as_deref()
                .unwrap_or_default()
                .contains("not a Win32 HWND")
        );
    }

    #[test]
    fn windows_candidate_probe_wraps_initializer_with_hardware_diagnostics() {
        if cfg!(target_os = "windows") {
            let Some(test_video_path) = test_video_path() else {
                eprintln!("skipping Windows fixture-backed test: test-video.mp4 is unavailable");
                return;
            };
            let initializer = WindowsSoftwarePlayerRuntimeAdapterFactory
                .probe_source_with_options(
                    MediaSource::new(test_video_path),
                    PlayerRuntimeOptions::default()
                        .with_decoder_plugin_library_paths([std::path::PathBuf::from(
                            "C:/tmp/fake-d3d11-decoder.dll",
                        )])
                        .with_decoder_plugin_video_mode(
                            PlayerDecoderPluginVideoMode::PreferNativeFrame,
                        )
                        .with_video_surface(player_runtime::PlayerVideoSurfaceTarget {
                            kind: player_runtime::PlayerVideoSurfaceKind::Win32Hwnd,
                            handle: 1,
                        }),
                )
                .expect("windows software adapter probe should succeed");
            let startup = initializer.startup();
            assert_eq!(
                startup
                    .video_decode
                    .as_ref()
                    .map(|decode| decode.selected_mode),
                Some(PlayerVideoDecodeMode::Software)
            );
        }
    }

    #[test]
    fn windows_candidate_initialize_falls_back_to_software_diagnostics() {
        let diagnostics = windows_runtime_diagnostics(
            &player_runtime::PlayerMediaInfo {
                source_uri: "fixture.mp4".to_owned(),
                source_kind: player_runtime::MediaSourceKind::Local,
                source_protocol: player_runtime::MediaSourceProtocol::File,
                duration: None,
                bit_rate: None,
                audio_streams: 0,
                video_streams: 1,
                best_video: Some(player_runtime::PlayerVideoInfo {
                    codec: "H264".to_owned(),
                    width: 1920,
                    height: 1080,
                    frame_rate: Some(60.0),
                }),
                best_audio: None,
                track_catalog: Default::default(),
                track_selection: Default::default(),
            },
            &PlayerRuntimeOptions::default(),
            None,
        );
        let fallback = merge_runtime_fallback_reason(
            "windows native-frame initialization failed; selected software desktop path",
            "failed to open Windows native-frame video source",
            diagnostics.video_decode.fallback_reason.clone(),
        );

        assert!(fallback.contains("selected software desktop path"));
        assert!(fallback.contains("failed to open Windows native-frame video source"));
        assert!(fallback.contains("D3D11Texture2D"));
    }

    #[test]
    fn windows_runtime_adapter_falls_back_on_native_frame_runtime_failure() {
        let mut adapter = WindowsRuntimeAdapter {
            inner: Box::new(FakeWindowsRuntime {
                capabilities: PlayerRuntimeAdapterCapabilities {
                    adapter_id: WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                    backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
                    supports_audio_output: true,
                    supports_frame_output: false,
                    supports_external_video_surface: true,
                    supports_seek: true,
                    supports_stop: true,
                    supports_playback_rate: true,
                    playback_rate_min: Some(0.5),
                    playback_rate_max: Some(3.0),
                    natural_playback_rate_max: Some(2.0),
                    supports_hardware_decode: true,
                    supports_streaming: true,
                    supports_hdr: true,
                },
                media_info: player_runtime::PlayerMediaInfo {
                    source_uri: "file:///tmp/test.mp4".to_owned(),
                    source_kind: player_runtime::MediaSourceKind::Local,
                    source_protocol: player_runtime::MediaSourceProtocol::File,
                    duration: None,
                    bit_rate: None,
                    audio_streams: 0,
                    video_streams: 1,
                    best_video: Some(player_runtime::PlayerVideoInfo {
                        codec: "H264".to_owned(),
                        width: 1920,
                        height: 1080,
                        frame_rate: Some(60.0),
                    }),
                    best_audio: None,
                    track_catalog: Default::default(),
                    track_selection: Default::default(),
                },
                advance_error: Some(PlayerRuntimeError::new(
                    PlayerRuntimeErrorCode::BackendFailure,
                    "windows native-frame presenter/runtime skeleton is not implemented yet",
                )),
            }),
            video_decode: PlayerVideoDecodeInfo {
                selected_mode: PlayerVideoDecodeMode::Hardware,
                hardware_available: true,
                hardware_backend: Some("D3D11".to_owned()),
                fallback_reason: None,
            },
            runtime_fallback: Some(WindowsRuntimeActiveFallback {
                source: MediaSource::new("file:///tmp/test.mp4"),
                options: PlayerRuntimeOptions::default(),
                fallback_reason:
                    "windows native-frame runtime failed during playback; selected software desktop path"
                        .to_owned(),
            }),
            pending_runtime_fallback_events: VecDeque::new(),
        };

        let error = adapter
            .advance()
            .expect_err("fallback opener should fail on non-windows host");

        assert!(matches!(
            error.code(),
            PlayerRuntimeErrorCode::Unsupported | PlayerRuntimeErrorCode::InvalidSource
        ));
        assert!(adapter.runtime_fallback.is_none());
    }

    #[test]
    fn windows_presenter_skeleton_accepts_d3d11_handles() {
        let presenter = WindowsD3D11NativeFramePresenterSkeleton::default();

        assert_eq!(
            presenter.backend_kind(),
            WindowsNativeFrameBackendKind::D3D11
        );
        assert_eq!(
            presenter.accepted_handle_kind(),
            player_plugin::DecoderNativeHandleKind::D3D11Texture2D
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn windows_presenter_factory_routes_to_d3d11_skeleton() {
        let presenter =
            windows_native_frame_presenter_for_backend(WindowsNativeFrameBackendKind::Dxva)
                .expect("presenter factory should create a D3D11 presenter");

        assert_eq!(
            presenter.backend_kind(),
            WindowsNativeFrameBackendKind::D3D11
        );
        assert_eq!(
            presenter.accepted_handle_kind(),
            DecoderNativeHandleKind::D3D11Texture2D
        );
    }

    #[test]
    fn windows_d3d11_presenter_attach_sets_target_state() {
        let mut presenter = WindowsD3D11NativeFramePresenterSkeleton::default();

        presenter
            .attach(WindowsSurfaceAttachTarget {
                kind: player_runtime::PlayerVideoSurfaceKind::Win32Hwnd,
                handle: 42,
            })
            .expect("attach should succeed");

        assert_eq!(
            presenter.state,
            WindowsD3D11PresenterState::AttachedAwaitingDevice
        );
        assert_eq!(
            presenter.attached_target,
            Some(WindowsSurfaceAttachTarget {
                kind: player_runtime::PlayerVideoSurfaceKind::Win32Hwnd,
                handle: 42,
            })
        );
    }

    #[test]
    fn windows_d3d11_presenter_reset_restores_detached_state() {
        let mut presenter = WindowsD3D11NativeFramePresenterSkeleton::default();

        let first = presenter
            .present(1)
            .expect_err("first present should fail as unattached");
        assert!(
            first
                .message()
                .contains("not attached to a surface target yet")
        );
        presenter
            .attach(WindowsSurfaceAttachTarget {
                kind: player_runtime::PlayerVideoSurfaceKind::Win32Hwnd,
                handle: 1,
            })
            .expect("attach should succeed");
        let second = presenter
            .present(1)
            .expect_err("second present should hit skeleton path");
        assert!(
            second
                .message()
                .contains("not attached to a device/context yet")
        );
        let third = presenter
            .present(1)
            .expect_err("third present should hit skeleton path");
        assert!(third.message().contains("skeleton is not implemented yet"));
        presenter.reset().expect("reset should succeed");
        let fourth = presenter
            .present(1)
            .expect_err("present after reset should be detached again");
        assert!(
            fourth
                .message()
                .contains("not attached to a surface target yet")
        );
    }

    #[test]
    fn windows_native_frame_source_releases_mismatched_handle_kind() {
        let released_handles = Arc::new(Mutex::new(Vec::new()));
        let mut session = FakeWindowsNativeSession {
            released_handles: released_handles.clone(),
            next_frame: None,
        };
        let mut presenter = FakeWindowsPresenter {
            accepted_handle_kind: DecoderNativeHandleKind::D3D11Texture2D,
            presented_handles: Arc::new(Mutex::new(Vec::new())),
        };

        let error = windows_native_frame_poll_with_presenter(
            &mut session,
            &mut presenter,
            &fake_video_stream_info(),
            DecoderNativeFrame {
                metadata: DecoderNativeFrameMetadata {
                    media_kind: DecoderMediaKind::Video,
                    format: player_plugin::DecoderFrameFormat::Nv12,
                    codec: "H264".to_owned(),
                    pts_us: Some(0),
                    duration_us: Some(33_000),
                    width: 1920,
                    height: 1080,
                    handle_kind: DecoderNativeHandleKind::DxgiSurface,
                },
                handle: 7,
            },
        )
        .expect_err("mismatched handle kind should fail");

        assert!(
            error
                .to_string()
                .contains("only accepts D3D11Texture2D handles")
        );
        assert_eq!(*released_handles.lock().expect("released handles"), vec![7]);
    }

    #[test]
    fn windows_native_frame_source_reaches_presenter_for_supported_handle_kind() {
        let released_handles = Arc::new(Mutex::new(Vec::new()));
        let presented_handles = Arc::new(Mutex::new(Vec::new()));
        let mut session = FakeWindowsNativeSession {
            released_handles: released_handles.clone(),
            next_frame: None,
        };
        let mut presenter = FakeWindowsPresenter {
            accepted_handle_kind: DecoderNativeHandleKind::D3D11Texture2D,
            presented_handles: presented_handles.clone(),
        };

        let error = windows_native_frame_poll_with_presenter(
            &mut session,
            &mut presenter,
            &fake_video_stream_info(),
            DecoderNativeFrame {
                metadata: DecoderNativeFrameMetadata {
                    media_kind: DecoderMediaKind::Video,
                    format: player_plugin::DecoderFrameFormat::Nv12,
                    codec: "H264".to_owned(),
                    pts_us: Some(0),
                    duration_us: Some(33_000),
                    width: 1920,
                    height: 1080,
                    handle_kind: DecoderNativeHandleKind::D3D11Texture2D,
                },
                handle: 9,
            },
        )
        .expect_err("presenter skeleton should still report unimplemented");

        assert!(
            error
                .to_string()
                .contains("not attached to a device/context yet")
                || error
                    .to_string()
                    .contains("presenter skeleton is not implemented yet")
        );
        assert_eq!(*released_handles.lock().expect("released handles"), vec![9]);
        assert_eq!(
            *presented_handles.lock().expect("presented handles"),
            vec![9]
        );
    }

    struct FakeWindowsRuntime {
        capabilities: PlayerRuntimeAdapterCapabilities,
        media_info: player_runtime::PlayerMediaInfo,
        advance_error: Option<PlayerRuntimeError>,
    }

    struct FakeWindowsNativeSession {
        released_handles: Arc<Mutex<Vec<usize>>>,
        next_frame: Option<DecoderReceiveNativeFrameOutput>,
    }

    struct FakeWindowsPresenter {
        accepted_handle_kind: DecoderNativeHandleKind,
        presented_handles: Arc<Mutex<Vec<usize>>>,
    }

    impl NativeDecoderSession for FakeWindowsNativeSession {
        fn session_info(&self) -> DecoderSessionInfo {
            DecoderSessionInfo {
                decoder_name: Some("fake-windows-native-session".to_owned()),
                selected_hardware_backend: Some("D3D11".to_owned()),
                output_format: Some(player_plugin::DecoderFrameFormat::Nv12),
            }
        }

        fn send_packet(
            &mut self,
            _packet: &player_plugin::DecoderPacket,
            _data: &[u8],
        ) -> Result<player_plugin::DecoderPacketResult, player_plugin::DecoderError> {
            Ok(player_plugin::DecoderPacketResult { accepted: true })
        }

        fn receive_native_frame(
            &mut self,
        ) -> Result<DecoderReceiveNativeFrameOutput, player_plugin::DecoderError> {
            Ok(self
                .next_frame
                .take()
                .unwrap_or(DecoderReceiveNativeFrameOutput::NeedMoreInput))
        }

        fn release_native_frame(
            &mut self,
            frame: DecoderNativeFrame,
        ) -> Result<(), player_plugin::DecoderError> {
            self.released_handles
                .lock()
                .expect("released handles")
                .push(frame.handle);
            Ok(())
        }

        fn flush(&mut self) -> Result<(), player_plugin::DecoderError> {
            Ok(())
        }

        fn close(&mut self) -> Result<(), player_plugin::DecoderError> {
            Ok(())
        }
    }

    impl WindowsNativeFramePresenter for FakeWindowsPresenter {
        fn backend_kind(&self) -> WindowsNativeFrameBackendKind {
            WindowsNativeFrameBackendKind::D3D11
        }

        fn accepted_handle_kind(&self) -> DecoderNativeHandleKind {
            self.accepted_handle_kind.clone()
        }

        fn decoder_device_context(&self) -> Option<DecoderNativeDeviceContext> {
            None
        }

        fn attach(&mut self, _target: WindowsSurfaceAttachTarget) -> PlayerRuntimeResult<()> {
            Ok(())
        }

        fn reset(&mut self) -> PlayerRuntimeResult<()> {
            Ok(())
        }

        fn present(&mut self, handle: usize) -> PlayerRuntimeResult<()> {
            self.presented_handles
                .lock()
                .expect("presented handles")
                .push(handle);
            Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::BackendFailure,
                "windows native-frame presenter skeleton is not implemented yet",
            ))
        }
    }

    fn fake_video_stream_info() -> VideoPacketStreamInfo {
        VideoPacketStreamInfo {
            stream_index: 0,
            codec: "H264".to_owned(),
            width: Some(1920),
            height: Some(1080),
            frame_rate: Some(60.0),
            extradata: Vec::new(),
        }
    }

    fn media_info_with_video_codec(codec: &str) -> player_runtime::PlayerMediaInfo {
        player_runtime::PlayerMediaInfo {
            source_uri: "file:///tmp/test.mp4".to_owned(),
            source_kind: player_runtime::MediaSourceKind::Local,
            source_protocol: player_runtime::MediaSourceProtocol::File,
            duration: None,
            bit_rate: None,
            audio_streams: 0,
            video_streams: 1,
            best_video: Some(player_runtime::PlayerVideoInfo {
                codec: codec.to_owned(),
                width: 1920,
                height: 1080,
                frame_rate: Some(60.0),
            }),
            best_audio: None,
            track_catalog: Default::default(),
            track_selection: Default::default(),
        }
    }

    fn windows_native_plugin_registry(codec: &str) -> PluginRegistry {
        PluginRegistry::from_records(vec![PluginDiagnosticRecord {
            path: std::path::PathBuf::from("fixture-windows-decoder"),
            status: PluginDiagnosticStatus::DecoderSupported,
            plugin_name: Some("fixture-windows-decoder".to_owned()),
            plugin_kind: Some(VesperPluginKind::Decoder),
            decoder_capabilities: Some(DecoderPluginCapabilitySummary {
                typed_codecs: vec![DecoderPluginCodecSummary {
                    codec: codec.to_owned(),
                    media_kind: DecoderMediaKind::Video,
                }],
                codecs: vec![format!("Video:{codec}")],
                supports_native_frame_output: true,
                supports_hardware_decode: true,
                supports_cpu_video_frames: false,
                supports_audio_frames: false,
                supports_gpu_handles: true,
                supports_flush: true,
                supports_drain: true,
                max_sessions: Some(1),
            }),
            message: Some(format!(
                "fixture-windows-decoder advertises Video {codec} support with native-frame output"
            )),
        }])
    }

    impl PlayerRuntimeAdapter for FakeWindowsRuntime {
        fn source_uri(&self) -> &str {
            &self.media_info.source_uri
        }

        fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
            self.capabilities.clone()
        }

        fn media_info(&self) -> &player_runtime::PlayerMediaInfo {
            &self.media_info
        }

        fn presentation_state(&self) -> player_runtime::PresentationState {
            player_runtime::PresentationState::Playing
        }

        fn has_video_surface(&self) -> bool {
            true
        }

        fn playback_rate(&self) -> f32 {
            1.0
        }

        fn progress(&self) -> player_runtime::PlaybackProgress {
            player_runtime::PlaybackProgress::new(Duration::ZERO, None)
        }

        fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
            Vec::new()
        }

        fn dispatch(
            &mut self,
            _command: PlayerRuntimeCommand,
        ) -> PlayerRuntimeResult<PlayerRuntimeCommandResult> {
            Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::Unsupported,
                "fake windows runtime dispatch is not implemented",
            ))
        }

        fn advance(&mut self) -> PlayerRuntimeResult<Option<player_runtime::DecodedVideoFrame>> {
            if let Some(error) = self.advance_error.take() {
                return Err(error);
            }
            Ok(None)
        }

        fn next_deadline(&self) -> Option<Instant> {
            None
        }
    }

    fn test_video_path() -> Option<String> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../test-video.mp4");
        path.canonicalize()
            .ok()
            .map(|path| path.to_string_lossy().into_owned())
    }
}
