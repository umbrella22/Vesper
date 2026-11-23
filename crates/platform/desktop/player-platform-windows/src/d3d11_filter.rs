//! Build-time Slang color filter for the D3D11 presentation path.

use std::ffi::c_void;
use std::fs::File;
use std::io::Read;

use anyhow::{Context, Result, ensure};
use player_plugin_abi::{VESPER_PIXEL_NV12, VESPER_PIXEL_P010, VesperVideoFormat};
use windows::Win32::Graphics::Direct3D::D3D_SRV_DIMENSION_TEXTURE2D;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_UNORDERED_ACCESS,
    D3D11_BUFFER_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC1, D3D11_SHADER_RESOURCE_VIEW_DESC1_0,
    D3D11_TEX2D_SRV1, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, ID3D11Buffer, ID3D11ClassLinkage,
    ID3D11ComputeShader, ID3D11Device, ID3D11Device3, ID3D11DeviceContext,
    ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11UnorderedAccessView,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_NV12, DXGI_FORMAT_P010, DXGI_FORMAT_R8_UNORM, DXGI_FORMAT_R8G8_UNORM,
    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R16_UNORM, DXGI_FORMAT_R16G16_UNORM, DXGI_SAMPLE_DESC,
};
use windows::core::Interface;

const MAX_SHADER_BYTES: usize = 256 * 1024;

pub(super) struct ColorFilter {
    device: ID3D11Device3,
    shader: ID3D11ComputeShader,
    constants: ID3D11Buffer,
    images: Option<FilterImages>,
}

#[derive(PartialEq, Eq)]
struct ImageKey {
    input: DXGI_FORMAT,
    texture_width: u32,
    texture_height: u32,
    output_width: u32,
    output_height: u32,
}

struct FilterImages {
    key: ImageKey,
    input: ID3D11Texture2D,
    planes: [Option<ID3D11ShaderResourceView>; 2],
    output: ID3D11Texture2D,
    target: ID3D11UnorderedAccessView,
}

impl ColorFilter {
    pub(super) fn from_host_selection(device: &ID3D11Device) -> Result<Option<Self>> {
        let Some(path) = std::env::var_os("VESPER_D3D11_COLOR_FILTER_DXBC") else {
            return Ok(None);
        };
        let mut bytecode = Vec::new();
        File::open(path)
            .context("open the selected D3D11 DXBC shader")?
            .take((MAX_SHADER_BYTES + 1) as u64)
            .read_to_end(&mut bytecode)?;
        ensure!(
            bytecode.starts_with(b"DXBC") && bytecode.len() <= MAX_SHADER_BYTES,
            "the selected shader must be build-time D3D11 DXBC"
        );
        let device3 = device
            .cast::<ID3D11Device3>()
            .context("D3D11 plane views")?;
        let mut shader = None;
        let mut constants = None;
        let desc = D3D11_BUFFER_DESC {
            ByteWidth: size_of::<[u32; 8]>() as u32,
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            ..Default::default()
        };
        // SAFETY: bytecode, the descriptor, and both output slots live through these calls.
        unsafe {
            device.CreateComputeShader(
                &bytecode,
                None::<&ID3D11ClassLinkage>,
                Some(&mut shader),
            )?;
            device.CreateBuffer(&desc, None, Some(&mut constants))?;
        }
        Ok(Some(Self {
            device: device3,
            shader: shader.context("D3D11 returned no compute shader")?,
            constants: constants.context("D3D11 returned no parameter buffer")?,
            images: None,
        }))
    }

    /// # Safety
    /// The decoder lease must cover all commands issued here. `handle` is a
    /// live `ID3D11Texture2D` belonging to this immediate context's device.
    pub(super) unsafe fn encode(
        &mut self,
        context: &ID3D11DeviceContext,
        handle: usize,
        array_slice: u32,
        format: VesperVideoFormat,
    ) -> Result<ID3D11Texture2D> {
        let ptr = handle as *mut c_void;
        // SAFETY: the caller retains the decoder texture lease through GPU completion.
        let source = unsafe { ID3D11Texture2D::from_raw_borrowed(&ptr) }
            .context("D3D11 filter requires a decoder texture")?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: the descriptor is writable and the texture was borrowed above.
        unsafe { source.GetDesc(&mut desc) };
        let (pixel_format, bit_depth) = match desc.Format {
            DXGI_FORMAT_NV12 => (VESPER_PIXEL_NV12, 8),
            DXGI_FORMAT_P010 => (VESPER_PIXEL_P010, 10),
            _ => anyhow::bail!("D3D11 color filter requires NV12 or P010"),
        };
        ensure!(
            desc.MipLevels == 1
                && desc.SampleDesc.Count == 1
                && array_slice < desc.ArraySize
                && format.pixel_format == pixel_format
                && format.color.bit_depth == bit_depth
                && format.width > 0
                && format.width <= desc.Width
                && format.height > 0
                && format.height <= desc.Height
                && format.crop_x == 0
                && format.crop_y == 0
                && (format.crop_w == 0 || format.crop_w == format.width)
                && (format.crop_h == 0 || format.crop_h == format.height)
                && format.rotation_deg == 0
                && format.mirror_flags == 0
                && format.color.hdr_kind == 0
                && !matches!(format.color.transfer, 16 | 18),
            "D3D11 reference filter requires an unrotated SDR subresource"
        );
        let key = ImageKey {
            input: desc.Format,
            texture_width: desc.Width,
            texture_height: desc.Height,
            output_width: format.width,
            output_height: format.height,
        };
        if self
            .images
            .as_ref()
            .is_none_or(|current| current.key != key)
        {
            self.images = Some(FilterImages::new(&self.device, key)?);
        }
        let images = self
            .images
            .as_ref()
            .context("missing D3D11 filter images")?;
        let params = [
            format.width,
            format.height,
            u32::from(bit_depth),
            u32::from(format.color.range),
            u32::from(format.color.primaries),
            u32::from(format.color.matrix),
            u32::from(format.color.transfer),
            1.0_f32.to_bits(),
        ];
        let writes = [Some(images.target.clone())];
        let constants = [Some(self.constants.clone())];
        let unbind_write = [None];
        // SAFETY: all views and resources use this device. The source subresource
        // and copy dimensions are valid. Calls are ordered on one immediate
        // context, and the UAV is unbound before the video processor reads it.
        unsafe {
            context.CopySubresourceRegion(&images.input, 0, 0, 0, 0, source, array_slice, None);
            context.UpdateSubresource(&self.constants, 0, None, params.as_ptr().cast(), 0, 0);
            context.CSSetShader(&self.shader, None);
            context.CSSetConstantBuffers(0, Some(&constants));
            context.CSSetShaderResources(0, Some(&images.planes));
            context.CSSetUnorderedAccessViews(0, 1, Some(writes.as_ptr()), None);
            context.Dispatch(format.width.div_ceil(16), format.height.div_ceil(8), 1);
            context.CSSetUnorderedAccessViews(0, 1, Some(unbind_write.as_ptr()), None);
            context.CSSetShaderResources(0, Some(&[None, None]));
        }
        Ok(images.output.clone())
    }
}

impl FilterImages {
    fn new(device: &ID3D11Device3, key: ImageKey) -> Result<Self> {
        let make_texture = |width, height, format, bind_flags| -> Result<ID3D11Texture2D> {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: format,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: bind_flags,
                ..Default::default()
            };
            let mut texture = None;
            // SAFETY: output storage is writable and the descriptor is immutable for the call.
            unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }?;
            texture.context("D3D11 returned no filter texture")
        };
        // Decoder-only arrays cannot provide shader views. Copy the selected
        // slice into one sampleable 2D resource on the GPU.
        let input = make_texture(
            key.texture_width,
            key.texture_height,
            key.input,
            D3D11_BIND_SHADER_RESOURCE.0 as u32,
        )?;
        let plane_formats = if key.input == DXGI_FORMAT_NV12 {
            [DXGI_FORMAT_R8_UNORM, DXGI_FORMAT_R8G8_UNORM]
        } else {
            [DXGI_FORMAT_R16_UNORM, DXGI_FORMAT_R16G16_UNORM]
        };
        let mut planes = [None, None];
        for (index, plane_format) in plane_formats.into_iter().enumerate() {
            let desc = D3D11_SHADER_RESOURCE_VIEW_DESC1 {
                Format: plane_format,
                ViewDimension: D3D_SRV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC1_0 {
                    Texture2D: D3D11_TEX2D_SRV1 {
                        MostDetailedMip: 0,
                        MipLevels: 1,
                        PlaneSlice: index as u32,
                    },
                },
            };
            let mut view = None;
            // SAFETY: the plane index and view format match the corresponding plane of input.
            unsafe { device.CreateShaderResourceView1(&input, Some(&desc), Some(&mut view)) }?;
            planes[index] = Some(view.context("D3D11 returned no plane view")?.cast()?);
        }
        let output = make_texture(
            key.output_width,
            key.output_height,
            DXGI_FORMAT_R8G8B8A8_UNORM,
            D3D11_BIND_UNORDERED_ACCESS.0 as u32,
        )?;
        let mut target = None;
        // SAFETY: output has unordered access binding and a typed single subresource.
        unsafe { device.CreateUnorderedAccessView(&output, None, Some(&mut target)) }?;
        Ok(Self {
            key,
            input,
            planes,
            output,
            target: target.context("D3D11 returned no filter target")?,
        })
    }
}
