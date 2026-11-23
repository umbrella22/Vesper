use ash::{Instance, StaticFn, khr, vk};
use player_plugin_abi::VESPER_PIXEL_NV12;

use crate::{VulkanDeviceContext, VulkanError};

/// D3D11 texture sharing interfaces whose handles may be imported into Vulkan.
#[derive(Clone, Copy)]
pub enum Win32TextureHandleKind {
    Nt,
    Kmt,
}

impl Win32TextureHandleKind {
    fn memory_type(self) -> vk::ExternalMemoryHandleTypeFlags {
        match self {
            Self::Nt => vk::ExternalMemoryHandleTypeFlags::D3D11_TEXTURE,
            Self::Kmt => vk::ExternalMemoryHandleTypeFlags::D3D11_TEXTURE_KMT,
        }
    }
}

/// Vulkan views of an SDR NV12 texture that retains its D3D11 producer allocation.
pub struct ImportedWin32VideoImage {
    context: VulkanDeviceContext,
    image: vk::Image,
    memory: vk::DeviceMemory,
    views: [vk::ImageView; 2],
    layer: u32,
    extent: vk::Extent2D,
}

impl ImportedWin32VideoImage {
    /// # Safety
    /// The device enables `VK_KHR_external_memory_win32`. `handle` is the specified D3D11 handle kind for an unprotected, exclusively
    /// borrowed layer of an NV12 texture on this context's adapter. Its producer has
    /// finished writing that layer before Vulkan acquires it. The caller keeps the D3D11 allocation
    /// and this imported image alive until GPU retirement. The caller retains
    /// ownership of the original Win32 handle.
    pub unsafe fn import_nv12(
        context: &VulkanDeviceContext,
        handle: vk::HANDLE,
        kind: Win32TextureHandleKind,
        width: u32,
        height: u32,
        layers: u32,
        layer: u32,
    ) -> Result<Self, VulkanError> {
        if handle == 0 || width == 0 || height == 0 || layers == 0 || layer >= layers {
            return Err(VulkanError::UnsupportedColorInput);
        }
        let extent = vk::Extent2D { width, height };
        let handle_type = kind.memory_type();
        let image_format = vk::Format::G8_B8R8_2PLANE_420_UNORM;
        let usage = vk::ImageUsageFlags::SAMPLED;
        let flags = vk::ImageCreateFlags::MUTABLE_FORMAT;
        let device = context.device();
        // SAFETY: the context retains the instance and the loader that resolves its functions.
        let instance = unsafe {
            Instance::load(
                &StaticFn {
                    get_instance_proc_addr: context.get_instance_proc_addr,
                },
                context.instance,
            )
        };
        let mut sharing =
            vk::PhysicalDeviceExternalImageFormatInfo::default().handle_type(handle_type);
        let info = vk::PhysicalDeviceImageFormatInfo2::default()
            .format(image_format)
            .ty(vk::ImageType::TYPE_2D)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(usage)
            .flags(flags)
            .push_next(&mut sharing);
        let mut external = vk::ExternalImageFormatProperties::default();
        let mut properties = vk::ImageFormatProperties2::default().push_next(&mut external);
        // SAFETY: the structures describe the planned image, and the instance owns the adapter.
        unsafe {
            instance.get_physical_device_image_format_properties2(
                context.physical_device,
                &info,
                &mut properties,
            )
        }?;
        let limits = properties.image_format_properties;
        if !external
            .external_memory_properties
            .external_memory_features
            .contains(vk::ExternalMemoryFeatureFlags::IMPORTABLE)
            || width > limits.max_extent.width
            || height > limits.max_extent.height
            || layers > limits.max_array_layers
        {
            return Err(VulkanError::UnsupportedColorInput);
        }
        let mut sharing = vk::ExternalMemoryImageCreateInfo::default().handle_types(handle_type);
        let image_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(image_format)
            .extent(vk::Extent3D {
                width,
                height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(layers)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(usage)
            .flags(flags)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .push_next(&mut sharing);
        let mut imported = Self {
            context: context.clone(),
            // SAFETY: the format and limits above cover this external image descriptor.
            image: unsafe { device.create_image(&image_info, None) }?,
            memory: vk::DeviceMemory::null(),
            views: [vk::ImageView::null(); 2],
            layer,
            extent,
        };
        let api = khr::external_memory_win32::Device::new(&instance, device);
        let mut handle_properties = vk::MemoryWin32HandlePropertiesKHR::default();
        // SAFETY: the caller owns a valid D3D11 handle and the context enabled its extension.
        let requirements = unsafe {
            api.get_memory_win32_handle_properties(handle_type, handle, &mut handle_properties)?;
            device.get_image_memory_requirements(imported.image)
        };
        let compatible_types = requirements.memory_type_bits & handle_properties.memory_type_bits;
        if compatible_types == 0 {
            return Err(VulkanError::UnsupportedColorInput);
        }
        let mut imported_handle = vk::ImportMemoryWin32HandleInfoKHR::default()
            .handle_type(handle_type)
            .handle(handle);
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(imported.image);
        let allocate = vk::MemoryAllocateInfo::default()
            .allocation_size(requirements.size)
            .memory_type_index(compatible_types.trailing_zeros())
            .push_next(&mut dedicated)
            .push_next(&mut imported_handle);
        // SAFETY: this is a dedicated import of the matching D3D11 texture. Vulkan retains
        // its own reference to NT imports and leaves the Win32 handle with the caller.
        unsafe {
            imported.memory = device.allocate_memory(&allocate, None)?;
            device.bind_image_memory(imported.image, imported.memory, 0)?;
            for (index, (aspect, format)) in [
                (vk::ImageAspectFlags::PLANE_0, vk::Format::R8_UNORM),
                (vk::ImageAspectFlags::PLANE_1, vk::Format::R8G8_UNORM),
            ]
            .into_iter()
            .enumerate()
            {
                imported.views[index] = device.create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(imported.image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(format)
                        .subresource_range(
                            vk::ImageSubresourceRange::default()
                                .aspect_mask(aspect)
                                .level_count(1)
                                .base_array_layer(layer)
                                .layer_count(1),
                        ),
                    None,
                )?;
            }
        }
        Ok(imported)
    }

    pub(crate) fn views(&self) -> [vk::ImageView; 2] {
        self.views
    }

    pub(crate) fn check_frame(
        &self,
        context: &VulkanDeviceContext,
        extent: vk::Extent2D,
        format: u32,
    ) -> Result<(), VulkanError> {
        if extent.width > self.extent.width
            || extent.height > self.extent.height
            || format != VESPER_PIXEL_NV12
            || self.context.device.handle() != context.device.handle()
        {
            return Err(VulkanError::UnsupportedColorInput);
        }
        Ok(())
    }

    pub(crate) fn encode_ownership(&self, command: vk::CommandBuffer, acquire: bool) {
        let family = self.context.family;
        // The decoder can use other layers of the same allocation concurrently.
        // Transfer only the subresource covered by this frame's lease.
        let range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .base_array_layer(self.layer)
            .layer_count(1);
        let barrier = vk::ImageMemoryBarrier::default()
            .image(self.image)
            .subresource_range(range);
        let (src, dst, barrier) = if acquire {
            (
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                barrier
                    .src_queue_family_index(vk::QUEUE_FAMILY_EXTERNAL)
                    .dst_queue_family_index(family)
                    .old_layout(vk::ImageLayout::GENERAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ),
            )
        } else {
            (
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                barrier
                    .src_queue_family_index(family)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_EXTERNAL)
                    .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .new_layout(vk::ImageLayout::GENERAL)
                    .src_access_mask(vk::AccessFlags::SHADER_READ),
            )
        };
        // SAFETY: the filter commands use the live imported image only between the ownership transfers.
        unsafe {
            self.context.device().cmd_pipeline_barrier(
                command,
                src,
                dst,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            )
        };
    }
}

impl Drop for ImportedWin32VideoImage {
    fn drop(&mut self) {
        // SAFETY: the submitter retains the image until retirement. Partial imports have no work.
        unsafe {
            let device = self.context.device();
            for view in self.views {
                if view != vk::ImageView::null() {
                    device.destroy_image_view(view, None);
                }
            }
            device.destroy_image(self.image, None);
            if self.memory != vk::DeviceMemory::null() {
                device.free_memory(self.memory, None);
            }
        }
    }
}
