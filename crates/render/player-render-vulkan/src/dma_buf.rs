use std::os::fd::{AsRawFd, BorrowedFd, IntoRawFd};

use ash::{ext, khr, vk};
use player_plugin_abi::{VESPER_PIXEL_NV12, VESPER_PIXEL_P010};

use crate::{VulkanDeviceContext, VulkanError};

pub(crate) const EXTENSIONS: [&std::ffi::CStr; 4] = [
    khr::external_memory_fd::NAME,
    ext::external_memory_dma_buf::NAME,
    ext::image_drm_format_modifier::NAME,
    ext::queue_family_foreign::NAME,
];

/// One single-plane DRM layer and the object containing its storage.
pub struct DmaBufVideoPlane<'a> {
    pub fd: BorrowedFd<'a>,
    pub object_size: u64,
    pub modifier: u64,
    pub offset: u64,
    pub pitch: u64,
}

/// Vulkan sample views of separate luma and chroma DMA-BUF layers.
pub struct ImportedDmaBufVideoImage {
    planes: [ImportedPlane; 2],
    extent: vk::Extent2D,
    pixel_format: u32,
}

impl ImportedDmaBufVideoImage {
    /// Imports a synchronized NV12 or P010 frame without copying its pixels.
    ///
    /// # Safety
    /// The producer exported the planes with matching formats, extents and layouts.
    /// It has completed its writes and will not reuse the frame until both planes
    /// have retired from Vulkan. Keep the producer's frame lease until retirement.
    pub unsafe fn import(
        context: &VulkanDeviceContext,
        width: u32,
        height: u32,
        pixel_format: u32,
        [luma, chroma]: [DmaBufVideoPlane<'_>; 2],
    ) -> Result<Self, VulkanError> {
        if !context.dma_buf_enabled || width == 0 || height == 0 {
            return Err(VulkanError::UnsupportedColorInput);
        }
        let extent = vk::Extent2D { width, height };
        let chroma_extent = vk::Extent2D {
            width: width.div_ceil(2),
            height: height.div_ceil(2),
        };
        let [luma_format, chroma_format] = match pixel_format {
            VESPER_PIXEL_NV12 => [vk::Format::R8_UNORM, vk::Format::R8G8_UNORM],
            VESPER_PIXEL_P010 => [vk::Format::R16_UNORM, vk::Format::R16G16_UNORM],
            _ => return Err(VulkanError::UnsupportedColorInput),
        };
        let instance = context.instance_dispatch();
        // SAFETY: the caller supplies matching DRM layers from a live producer.
        let planes = unsafe {
            [
                ImportedPlane::new(context, &instance, extent, luma_format, luma)?,
                ImportedPlane::new(context, &instance, chroma_extent, chroma_format, chroma)?,
            ]
        };
        Ok(Self {
            planes,
            extent,
            pixel_format,
        })
    }

    pub(crate) fn check_frame(
        &self,
        context: &VulkanDeviceContext,
        extent: vk::Extent2D,
        pixel_format: u32,
    ) -> Result<(), VulkanError> {
        if self.extent != extent
            || self.pixel_format != pixel_format
            || self.planes[0].context.device().handle() != context.device().handle()
        {
            return Err(VulkanError::UnsupportedColorInput);
        }
        Ok(())
    }

    pub(crate) fn views(&self) -> [vk::ImageView; 2] {
        [self.planes[0].view, self.planes[1].view]
    }

    pub(crate) fn encode_ownership(&self, command: vk::CommandBuffer, acquire: bool) {
        let context = &self.planes[0].context;
        let (src, dst, from, to, old_layout, new_layout, read_from, read_to) = if acquire {
            (
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::QUEUE_FAMILY_FOREIGN_EXT,
                context.family,
                vk::ImageLayout::GENERAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                vk::AccessFlags::empty(),
                vk::AccessFlags::SHADER_READ,
            )
        } else {
            (
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                context.family,
                vk::QUEUE_FAMILY_FOREIGN_EXT,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                vk::ImageLayout::GENERAL,
                vk::AccessFlags::SHADER_READ,
                vk::AccessFlags::empty(),
            )
        };
        let barriers = self.planes.each_ref().map(|plane| {
            vk::ImageMemoryBarrier::default()
                .image(plane.image)
                .subresource_range(color_range())
                .src_queue_family_index(from)
                .dst_queue_family_index(to)
                .old_layout(old_layout)
                .new_layout(new_layout)
                .src_access_mask(read_from)
                .dst_access_mask(read_to)
        });
        // SAFETY: command recording owns the slot, and the frame lease keeps the
        // producer and these images alive across both ownership transfers.
        unsafe {
            context.device().cmd_pipeline_barrier(
                command,
                src,
                dst,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &barriers,
            )
        };
    }
}

struct ImportedPlane {
    context: VulkanDeviceContext,
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
}

impl ImportedPlane {
    unsafe fn new(
        context: &VulkanDeviceContext,
        instance: &ash::Instance,
        extent: vk::Extent2D,
        format: vk::Format,
        plane: DmaBufVideoPlane<'_>,
    ) -> Result<Self, VulkanError> {
        let DmaBufVideoPlane {
            fd,
            object_size,
            modifier,
            offset,
            pitch,
        } = plane;
        if pitch == 0 || offset >= object_size {
            return Err(VulkanError::UnsupportedColorInput);
        }
        // SAFETY: capability queries use the live adapter owned by context.
        unsafe { check_modifier(context, instance, extent, format, modifier) }?;
        let handle_type = vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT;
        let layouts = [vk::SubresourceLayout {
            offset,
            row_pitch: pitch,
            ..Default::default()
        }];
        let mut drm_layout = vk::ImageDrmFormatModifierExplicitCreateInfoEXT::default()
            .drm_format_modifier(modifier)
            .plane_layouts(&layouts);
        let mut external = vk::ExternalMemoryImageCreateInfo::default().handle_types(handle_type);
        let image_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(vk::Extent3D {
                width: extent.width,
                height: extent.height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
            .usage(vk::ImageUsageFlags::SAMPLED)
            .push_next(&mut drm_layout)
            .push_next(&mut external);
        let device = context.device();
        let mut image = Self {
            context: context.clone(),
            // SAFETY: check_modifier verified single-plane import for this format.
            image: unsafe { device.create_image(&image_info, None) }?,
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
        };
        let fd_api = khr::external_memory_fd::Device::new(instance, device);
        let mut fd_properties = vk::MemoryFdPropertiesKHR::default();
        // SAFETY: the producer owns the exported DMA-BUF descriptor for this frame.
        let requirements = unsafe {
            fd_api.get_memory_fd_properties(handle_type, fd.as_raw_fd(), &mut fd_properties)?;
            device.get_image_memory_requirements(image.image)
        };
        let compatible = fd_properties.memory_type_bits & requirements.memory_type_bits;
        if compatible == 0 || requirements.size > object_size {
            return Err(VulkanError::UnsupportedColorInput);
        }
        // Vulkan consumes the duplicated descriptor only after successful allocation.
        let fd = fd
            .try_clone_to_owned()
            .map_err(VulkanError::DuplicateDmaBuf)?;
        let mut import = vk::ImportMemoryFdInfoKHR::default()
            .handle_type(handle_type)
            .fd(fd.as_raw_fd());
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image.image);
        let info = vk::MemoryAllocateInfo::default()
            .allocation_size(object_size)
            .memory_type_index(compatible.trailing_zeros())
            .push_next(&mut dedicated)
            .push_next(&mut import);
        // SAFETY: the dedicated import uses the matching allocation and image layout.
        image.memory = unsafe { device.allocate_memory(&info, None) }?;
        let _ = fd.into_raw_fd();
        // SAFETY: image and imported memory are compatible and uniquely owned.
        unsafe {
            device.bind_image_memory(image.image, image.memory, 0)?;
            image.view = device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image.image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format)
                    .subresource_range(color_range()),
                None,
            )?;
        }
        Ok(image)
    }
}

impl Drop for ImportedPlane {
    fn drop(&mut self) {
        // SAFETY: partial imports have no GPU work. Submitted imports are retired first.
        unsafe {
            let device = self.context.device();
            if self.view != vk::ImageView::null() {
                device.destroy_image_view(self.view, None);
            }
            device.destroy_image(self.image, None);
            if self.memory != vk::DeviceMemory::null() {
                device.free_memory(self.memory, None);
            }
        }
    }
}

fn color_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .level_count(1)
        .layer_count(1)
}

unsafe fn check_modifier(
    context: &VulkanDeviceContext,
    instance: &ash::Instance,
    extent: vk::Extent2D,
    format: vk::Format,
    modifier: u64,
) -> Result<(), VulkanError> {
    let physical = context.physical_device;
    let mut list = vk::DrmFormatModifierPropertiesListEXT::default();
    // SAFETY: the instance and physical device are live and the list requests a count.
    unsafe {
        instance.get_physical_device_format_properties2(
            physical,
            format,
            &mut vk::FormatProperties2::default().push_next(&mut list),
        )
    };
    let mut modifiers = vec![
        vk::DrmFormatModifierPropertiesEXT::default();
        list.drm_format_modifier_count as usize
    ];
    let mut list = vk::DrmFormatModifierPropertiesListEXT::default()
        .drm_format_modifier_properties(&mut modifiers);
    // SAFETY: the list borrows space for all modifiers advertised by the adapter.
    unsafe {
        instance.get_physical_device_format_properties2(
            physical,
            format,
            &mut vk::FormatProperties2::default().push_next(&mut list),
        )
    };
    if !modifiers.iter().any(|info| {
        info.drm_format_modifier == modifier
            && info.drm_format_modifier_plane_count == 1
            && info
                .drm_format_modifier_tiling_features
                .contains(vk::FormatFeatureFlags::SAMPLED_IMAGE)
    }) {
        return Err(VulkanError::UnsupportedColorInput);
    }
    let mut drm = vk::PhysicalDeviceImageDrmFormatModifierInfoEXT::default()
        .drm_format_modifier(modifier)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    let mut external = vk::PhysicalDeviceExternalImageFormatInfo::default()
        .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
    let info = vk::PhysicalDeviceImageFormatInfo2::default()
        .format(format)
        .ty(vk::ImageType::TYPE_2D)
        .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
        .usage(vk::ImageUsageFlags::SAMPLED)
        .push_next(&mut drm)
        .push_next(&mut external);
    let mut memory = vk::ExternalImageFormatProperties::default();
    let mut properties = vk::ImageFormatProperties2::default().push_next(&mut memory);
    // SAFETY: the chain describes the exact image format and external handle type.
    unsafe {
        instance.get_physical_device_image_format_properties2(physical, &info, &mut properties)
    }?;
    let max = properties.image_format_properties.max_extent;
    if extent.width > max.width
        || extent.height > max.height
        || !memory
            .external_memory_properties
            .external_memory_features
            .contains(vk::ExternalMemoryFeatureFlags::IMPORTABLE)
    {
        return Err(VulkanError::UnsupportedColorInput);
    }
    Ok(())
}
