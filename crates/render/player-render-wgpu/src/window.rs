use winit::dpi::LogicalSize;
use winit::window::{Window, WindowAttributes};

use crate::types::RenderSurfaceConfig;

pub fn default_window_attributes(config: RenderSurfaceConfig) -> WindowAttributes {
    Window::default_attributes()
        .with_title("Vesper basic player")
        .with_inner_size(LogicalSize::new(config.width, config.height))
}

pub fn preferred_backends() -> wgpu::Backends {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        wgpu::Backends::METAL
    }

    #[cfg(target_os = "windows")]
    {
        wgpu::Backends::DX12 | wgpu::Backends::VULKAN
    }

    #[cfg(target_os = "linux")]
    {
        wgpu::Backends::VULKAN | wgpu::Backends::GL
    }

    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "linux",
    )))]
    {
        wgpu::Backends::PRIMARY
    }
}

pub(crate) fn preferred_backend_label() -> &'static str {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        "Metal"
    }

    #[cfg(target_os = "windows")]
    {
        "DirectX 12 / Vulkan"
    }

    #[cfg(target_os = "linux")]
    {
        "Vulkan / OpenGL"
    }

    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "linux",
    )))]
    {
        "platform-default"
    }
}

pub(crate) fn preferred_surface_format(
    formats: &[wgpu::TextureFormat],
) -> Option<wgpu::TextureFormat> {
    formats
        .iter()
        .copied()
        .find(wgpu::TextureFormat::is_srgb)
        .or_else(|| formats.first().copied())
}

#[cfg(test)]
mod tests {
    use super::{preferred_backend_label, preferred_backends, preferred_surface_format};

    #[test]
    fn preferred_backend_configuration_matches_platform() {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            assert_eq!(preferred_backends(), wgpu::Backends::METAL);
            assert_eq!(preferred_backend_label(), "Metal");
        }

        #[cfg(target_os = "windows")]
        {
            assert_eq!(
                preferred_backends(),
                wgpu::Backends::DX12 | wgpu::Backends::VULKAN
            );
            assert_eq!(preferred_backend_label(), "DirectX 12 / Vulkan");
        }

        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                preferred_backends(),
                wgpu::Backends::VULKAN | wgpu::Backends::GL
            );
            assert_eq!(preferred_backend_label(), "Vulkan / OpenGL");
        }
    }

    #[test]
    fn preferred_surface_format_uses_first_srgb_format() {
        let format = preferred_surface_format(&[
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Bgra8UnormSrgb,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        ]);

        assert_eq!(format, Some(wgpu::TextureFormat::Bgra8UnormSrgb));
    }

    #[test]
    fn preferred_surface_format_falls_back_to_first_supported_format() {
        let format = preferred_surface_format(&[
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Bgra8Unorm,
        ]);

        assert_eq!(format, Some(wgpu::TextureFormat::Rgba8Unorm));
    }
}
