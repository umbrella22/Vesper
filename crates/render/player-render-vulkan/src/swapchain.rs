//! Presentation into a host-owned, sRGB swapchain.

use ash::vk;

use crate::{
    VulkanColorFrame, VulkanDeviceContext, VulkanError, VulkanFilterImage, VulkanFrameLease,
    VulkanGpuBackend,
};

/// A swapchain image acquired by the window adapter. Its binary presentation
/// semaphore is associated with the image index and reused on reacquisition.
pub struct VulkanSwapchainFrame<'a> {
    pub loader: &'a ash::khr::swapchain::Device,
    pub swapchain: vk::SwapchainKHR,
    pub image_index: u32,
    pub image: vk::Image,
    pub extent: vk::Extent2D,
    pub format: vk::Format,
    pub acquired: vk::Semaphore,
    pub presentation_ready: vk::Semaphore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VulkanPresentStatus {
    Presented { timeline_value: u64 },
    Suboptimal { timeline_value: u64 },
    OutOfDate { timeline_value: u64 },
}

impl<T: VulkanFrameLease> VulkanGpuBackend<T> {
    /// Converts the linear filter result to the acquired sRGB image and presents
    /// it on the same queue. A resize can invalidate the window independently of
    /// the submitted video's timeline retirement.
    ///
    /// # Safety
    /// In addition to [`Self::submit_color_filtered`], the supplied handles must
    /// describe an acquired swapchain image belonging to the backend's queue
    /// family. The swapchain authorizes `TRANSFER_DST` and both formats support
    /// the respective blit operations. The resource keeps the incoming frame,
    /// swapchain and acquire semaphore live through render retirement. The
    /// caller owns the presentation semaphore per image and keeps it live until
    /// that image is reacquired or presentation has retired. An error before or
    /// during presentation requires the caller to retire the acquired image.
    pub unsafe fn submit_color_to_swapchain(
        &mut self,
        epoch: u64,
        resource: T,
        color_frame: VulkanColorFrame<'_>,
        target: VulkanSwapchainFrame<'_>,
    ) -> Result<VulkanPresentStatus, VulkanError> {
        let input_size = color_frame.extent()?;
        if !matches!(
            target.format,
            vk::Format::B8G8R8A8_SRGB | vk::Format::R8G8B8A8_SRGB
        ) || target.swapchain == vk::SwapchainKHR::null()
            || target.image == vk::Image::null()
            || target.acquired == vk::Semaphore::null()
            || target.presentation_ready == vk::Semaphore::null()
            || target.acquired == target.presentation_ready
        {
            return Err(VulkanError::UnsupportedSwapchainTarget);
        }
        let src_offset = blit_corner(input_size)?;
        let dst_offset = blit_corner(target.extent)?;
        // SAFETY: the caller has acquired the target image, maintains the
        // handles, and selected the queue and blit capabilities specified above.
        let timeline_value = unsafe {
            self.submit_color_filtered(
                epoch,
                resource,
                Some(target.acquired),
                Some(target.presentation_ready),
                color_frame,
                |context, command, _, output| {
                    encode_blit(
                        context,
                        command,
                        output,
                        target.image,
                        src_offset,
                        dst_offset,
                        vk::Filter::NEAREST,
                    );
                    Ok(())
                },
            )
        }?;
        let image_indices = [target.image_index];
        let swapchains = [target.swapchain];
        let waits = [target.presentation_ready];
        let info = vk::PresentInfoKHR::default()
            .image_indices(&image_indices)
            .swapchains(&swapchains)
            .wait_semaphores(&waits);
        let queue = self
            .queue
            .as_ref()
            .ok_or(VulkanError::Exhausted)?
            .context
            .queue;
        // SAFETY: this is the host's externally synchronized queue and the
        // submitted blit signals the per-image semaphore for this presentation.
        match unsafe { target.loader.queue_present(queue, &info) } {
            Ok(false) => Ok(VulkanPresentStatus::Presented { timeline_value }),
            Ok(true) => Ok(VulkanPresentStatus::Suboptimal { timeline_value }),
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                Ok(VulkanPresentStatus::OutOfDate { timeline_value })
            }
            Err(error) => Err(error.into()),
        }
    }
}

pub(crate) fn blit_corner(extent: vk::Extent2D) -> Result<vk::Offset3D, VulkanError> {
    if extent.width == 0 || extent.height == 0 {
        return Err(VulkanError::UnsupportedSwapchainTarget);
    }
    Ok(vk::Offset3D {
        x: i32::try_from(extent.width).map_err(|_| VulkanError::UnsupportedSwapchainTarget)?,
        y: i32::try_from(extent.height).map_err(|_| VulkanError::UnsupportedSwapchainTarget)?,
        z: 1,
    })
}

pub(crate) unsafe fn encode_blit(
    context: &VulkanDeviceContext,
    command: vk::CommandBuffer,
    output: &VulkanFilterImage,
    target: vk::Image,
    src_offset: vk::Offset3D,
    dst_offset: vk::Offset3D,
    filter: vk::Filter,
) {
    let color = vk::ImageAspectFlags::COLOR;
    let layers = vk::ImageSubresourceLayers::default()
        .aspect_mask(color)
        .layer_count(1);
    let range = vk::ImageSubresourceRange::default()
        .aspect_mask(color)
        .level_count(1)
        .layer_count(1);
    let change = vk::ImageMemoryBarrier::default()
        .image(target)
        .subresource_range(range)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED);
    let for_blit = change
        .old_layout(vk::ImageLayout::UNDEFINED)
        .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE);
    let for_present = change
        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE);
    let region = vk::ImageBlit::default()
        .src_subresource(layers)
        .src_offsets([vk::Offset3D::default(), src_offset])
        .dst_subresource(layers)
        .dst_offsets([vk::Offset3D::default(), dst_offset]);
    let device = context.device();
    // SAFETY: the caller validates image acquisition, bounds, layout and blit capabilities.
    // The reference filter established the source-write dependency before this call.
    unsafe {
        device.cmd_pipeline_barrier(
            command,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[for_blit],
        );
        device.cmd_blit_image(
            command,
            output.image(),
            vk::ImageLayout::GENERAL,
            target,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &[region],
            filter,
        );
        device.cmd_pipeline_barrier(
            command,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[for_present],
        );
    }
}
