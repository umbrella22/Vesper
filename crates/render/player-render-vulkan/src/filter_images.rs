use crate::{VulkanDeviceContext, VulkanError};
use ash::vk;

/// The two RGBA16F intermediates used by the filter chain in one command slot.
/// A slot can only replace or reuse this pair once its timeline value is retired.
pub struct VulkanFilterImages {
    extent: vk::Extent2D,
    targets: [VulkanFilterImage; 2],
}

impl VulkanFilterImages {
    pub(crate) fn new(
        context: &VulkanDeviceContext,
        extent: vk::Extent2D,
    ) -> Result<Self, VulkanError> {
        const MAX_RGBA16F_BYTES: u64 = 256 * 1024 * 1024;
        let limit = context.physical_limits().max_image_dimension2_d;
        let pixels = u64::from(extent.width) * u64::from(extent.height);
        if pixels == 0
            || pixels > MAX_RGBA16F_BYTES / 8
            || extent.width > limit
            || extent.height > limit
        {
            return Err(VulkanError::InvalidFilterExtent);
        }
        let loader = ash::StaticFn {
            get_instance_proc_addr: context.get_instance_proc_addr,
        };
        // SAFETY: the cloned context keeps the Vulkan entry and instance live.
        let instance = unsafe { ash::Instance::load(&loader, context.instance) };
        // SAFETY: the physical device belongs to this instance.
        let memory =
            unsafe { instance.get_physical_device_memory_properties(context.physical_device) };
        let targets = [
            VulkanFilterImage::new(context, extent, &memory)?,
            VulkanFilterImage::new(context, extent, &memory)?,
        ];
        Ok(Self { extent, targets })
    }

    pub fn extent(&self) -> vk::Extent2D {
        self.extent
    }

    pub fn targets(&self) -> &[VulkanFilterImage; 2] {
        &self.targets
    }

    /// Starts both empty targets in `GENERAL` for storage-image writes.
    /// The filter chain is responsible for write-to-read barriers between passes.
    pub(crate) unsafe fn prepare(&self, command: vk::CommandBuffer) {
        let barriers = self.targets.each_ref().map(|target| {
            vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::GENERAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_access_mask(vk::AccessFlags::SHADER_READ | vk::AccessFlags::SHADER_WRITE)
                .image(target.image)
                .subresource_range(COLOR_SUBRESOURCE)
        });
        // SAFETY: the slot's previous submission completed and this command is recording.
        unsafe {
            self.targets[0].context.device().cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &barriers,
            );
        }
    }
}

const COLOR_SUBRESOURCE: vk::ImageSubresourceRange = vk::ImageSubresourceRange {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    base_mip_level: 0,
    level_count: 1,
    base_array_layer: 0,
    layer_count: 1,
};

pub struct VulkanFilterImage {
    context: VulkanDeviceContext,
    image: vk::Image,
    view: vk::ImageView,
    memory: vk::DeviceMemory,
}

impl VulkanFilterImage {
    pub const FORMAT: vk::Format = vk::Format::R16G16B16A16_SFLOAT;

    fn new(
        context: &VulkanDeviceContext,
        extent: vk::Extent2D,
        memory: &vk::PhysicalDeviceMemoryProperties,
    ) -> Result<Self, VulkanError> {
        let device = context.device();
        let image_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(Self::FORMAT)
            .extent(vk::Extent3D {
                width: extent.width,
                height: extent.height,
                depth: 1,
            })
            .array_layers(1)
            .mip_levels(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(
                vk::ImageUsageFlags::STORAGE
                    | vk::ImageUsageFlags::SAMPLED
                    | vk::ImageUsageFlags::TRANSFER_SRC,
            );
        // SAFETY: this 2D image is private to the command slot and starts unbound.
        let image = unsafe { device.create_image(&image_info, None) }?;
        let mut target = Self {
            context: context.clone(),
            image,
            view: vk::ImageView::null(),
            memory: vk::DeviceMemory::null(),
        };
        // SAFETY: the image belongs to this device and is kept alive by target.
        let requirements = unsafe { device.get_image_memory_requirements(image) };
        let index = memory.memory_types[..memory.memory_type_count as usize]
            .iter()
            .enumerate()
            .position(|(index, ty)| {
                requirements.memory_type_bits & (1 << index) != 0
                    && ty
                        .property_flags
                        .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
            })
            .ok_or(VulkanError::FilterImageMemoryUnavailable)?;
        let allocation = vk::MemoryAllocateInfo::default()
            .allocation_size(requirements.size)
            .memory_type_index(index as u32);
        // SAFETY: the allocation meets the just-created image's size and memory requirements.
        target.memory = unsafe { device.allocate_memory(&allocation, None) }?;
        // SAFETY: this new allocation is exclusive and is bound at aligned offset zero.
        unsafe { device.bind_image_memory(image, target.memory, 0) }?;
        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(Self::FORMAT)
            .subresource_range(COLOR_SUBRESOURCE);
        // SAFETY: the view covers the only mip and array layer of the bound image.
        target.view = unsafe { device.create_image_view(&view_info, None) }?;
        Ok(target)
    }

    pub fn image(&self) -> vk::Image {
        self.image
    }

    pub fn view(&self) -> vk::ImageView {
        self.view
    }
}

impl Drop for VulkanFilterImage {
    fn drop(&mut self) {
        let device = self.context.device();
        // SAFETY: the timeline retired all reads and writes, or construction failed
        // before submission. Vulkan accepts null handles for partial construction.
        unsafe {
            device.destroy_image_view(self.view, None);
            device.destroy_image(self.image, None);
            device.free_memory(self.memory, None);
        }
    }
}
