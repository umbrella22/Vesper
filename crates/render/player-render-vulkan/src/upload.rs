use ash::vk;
use player_plugin_abi::{VESPER_PIXEL_NV12, VESPER_PIXEL_P010};

use crate::{VulkanDeviceContext, VulkanError};

const MAX_PLANE_BYTES: usize = 128 * 1024 * 1024;
const COLOR: vk::ImageSubresourceRange = vk::ImageSubresourceRange {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    base_mip_level: 0,
    level_count: 1,
    base_array_layer: 0,
    layer_count: 1,
};

#[derive(Clone, Copy)]
pub struct VulkanUploadPlane<'a> {
    pub bytes: &'a [u8],
    pub row_stride: usize,
}

pub(crate) struct Upload420 {
    extent: vk::Extent2D,
    format: u32,
    planes: [UploadPlane; 2],
}

impl Upload420 {
    pub(crate) fn new(
        context: &VulkanDeviceContext,
        extent: vk::Extent2D,
        format: u32,
    ) -> Result<Self, VulkanError> {
        let (luma, chroma, bytes) = match format {
            VESPER_PIXEL_NV12 => (vk::Format::R8_UNORM, vk::Format::R8G8_UNORM, 1),
            VESPER_PIXEL_P010 => (vk::Format::R16_UNORM, vk::Format::R16G16_UNORM, 2),
            _ => return Err(VulkanError::UnsupportedColorInput),
        };
        let chroma_extent = vk::Extent2D {
            width: extent.width.div_ceil(2),
            height: extent.height.div_ceil(2),
        };
        Ok(Self {
            extent,
            format,
            planes: [
                UploadPlane::new(context, extent, luma, bytes)?,
                UploadPlane::new(context, chroma_extent, chroma, bytes * 2)?,
            ],
        })
    }

    pub(crate) fn matches(&self, extent: vk::Extent2D, format: u32) -> bool {
        self.extent == extent && self.format == format
    }

    pub(crate) fn record(
        &mut self,
        command: vk::CommandBuffer,
        planes: [VulkanUploadPlane<'_>; 2],
    ) -> Result<[vk::ImageView; 2], VulkanError> {
        for (plane, input) in self.planes.iter().zip(planes) {
            plane.stage(input)?;
            plane.record(command);
        }
        Ok(self.planes.each_ref().map(|plane| plane.view))
    }
}

struct UploadPlane {
    context: VulkanDeviceContext,
    extent: vk::Extent3D,
    row_bytes: usize,
    size: usize,
    staging: vk::Buffer,
    staging_memory: vk::DeviceMemory,
    image: vk::Image,
    image_memory: vk::DeviceMemory,
    view: vk::ImageView,
}

impl UploadPlane {
    fn new(
        context: &VulkanDeviceContext,
        extent: vk::Extent2D,
        format: vk::Format,
        pixel_bytes: usize,
    ) -> Result<Self, VulkanError> {
        let row_bytes = (extent.width as usize)
            .checked_mul(pixel_bytes)
            .ok_or(VulkanError::InvalidFilterExtent)?;
        let size = row_bytes
            .checked_mul(extent.height as usize)
            .filter(|size| (1..=MAX_PLANE_BYTES).contains(size))
            .ok_or(VulkanError::InvalidFilterExtent)?;
        let extent = vk::Extent3D {
            width: extent.width,
            height: extent.height,
            depth: 1,
        };
        let mut plane = Self {
            context: context.clone(),
            extent,
            row_bytes,
            size,
            staging: vk::Buffer::null(),
            staging_memory: vk::DeviceMemory::null(),
            image: vk::Image::null(),
            image_memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
        };
        let device = context.device();
        // SAFETY: these private handles belong to the live context and RAII covers failures.
        unsafe {
            plane.staging = device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .usage(vk::BufferUsageFlags::TRANSFER_SRC)
                    .size(size as u64),
                None,
            )?;
            plane.staging_memory = context.allocate_memory(
                device.get_buffer_memory_requirements(plane.staging),
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            )?;
            device.bind_buffer_memory(plane.staging, plane.staging_memory, 0)?;
            plane.image = device.create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(format)
                    .extent(extent)
                    .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED)
                    .array_layers(1)
                    .mip_levels(1)
                    .samples(vk::SampleCountFlags::TYPE_1),
                None,
            )?;
            plane.image_memory = context.allocate_memory(
                device.get_image_memory_requirements(plane.image),
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
            )?;
            device.bind_image_memory(plane.image, plane.image_memory, 0)?;
            plane.view = device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(plane.image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format)
                    .subresource_range(COLOR),
                None,
            )?;
        }
        Ok(plane)
    }

    fn stage(&self, input: VulkanUploadPlane<'_>) -> Result<(), VulkanError> {
        let height = self.extent.height as usize;
        let required = (height - 1)
            .checked_mul(input.row_stride)
            .and_then(|bytes| bytes.checked_add(self.row_bytes))
            .ok_or(VulkanError::InvalidUploadPlane)?;
        if input.row_stride < self.row_bytes || input.bytes.len() < required {
            return Err(VulkanError::InvalidUploadPlane);
        }
        let device = self.context.device();
        // SAFETY: the slot is idle and the allocation is host-visible and coherent.
        let allocation = unsafe {
            device.map_memory(
                self.staging_memory,
                0,
                self.size as u64,
                vk::MemoryMapFlags::empty(),
            )?
        };
        for row in 0..height {
            // SAFETY: checked source stride and validated packed size bound both intervals.
            // The host slice cannot alias this private mapped allocation.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    input.bytes.as_ptr().add(row * input.row_stride),
                    allocation.cast::<u8>().add(row * self.row_bytes),
                    self.row_bytes,
                );
            }
        }
        // SAFETY: the staging allocation was mapped above and no references escape.
        unsafe { device.unmap_memory(self.staging_memory) };
        Ok(())
    }

    fn record(&self, command: vk::CommandBuffer) {
        let device = self.context.device();
        let image_barrier = vk::ImageMemoryBarrier::default()
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .subresource_range(COLOR)
            .image(self.image);
        let upload = image_barrier
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE);
        let sample = image_barrier
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ);
        let region = vk::BufferImageCopy::default()
            .image_extent(self.extent)
            .image_subresource(
                vk::ImageSubresourceLayers::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .layer_count(1),
            );
        // SAFETY: recording occurs in the slot after its previous submission has completed.
        // The packed coherent buffer is populated before submission and lives with its image.
        unsafe {
            device.cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[upload],
            );
            device.cmd_copy_buffer_to_image(
                command,
                self.staging,
                self.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[region],
            );
            device.cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[sample],
            );
        }
    }
}

impl Drop for UploadPlane {
    fn drop(&mut self) {
        // SAFETY: the owning slot waits for GPU retirement before dropping its buffers and images.
        unsafe {
            let device = self.context.device();
            device.destroy_image_view(self.view, None);
            device.destroy_image(self.image, None);
            device.free_memory(self.image_memory, None);
            device.destroy_buffer(self.staging, None);
            device.free_memory(self.staging_memory, None);
        }
    }
}
