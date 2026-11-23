use std::sync::Arc;

use ash::{Instance, android::external_memory_android_hardware_buffer, ext, khr, vk};

use crate::{VulkanDeviceContext, VulkanError};

const HARDWARE_BUFFER_USAGE_GPU_SAMPLED_IMAGE: u64 = 1 << 8;
const HARDWARE_BUFFER_USAGE_PROTECTED_CONTENT: u64 = 1 << 14;

#[repr(C)]
#[derive(Default)]
struct HardwareBufferDescription {
    width: u32,
    height: u32,
    layers: u32,
    format: u32,
    usage: u64,
    stride: u32,
    reserved0: u32,
    reserved1: u64,
}

#[link(name = "android")]
unsafe extern "C" {
    #[link_name = "AHardwareBuffer_describe"]
    fn describe(buffer: *const vk::AHardwareBuffer, description: *mut HardwareBufferDescription);
}

/// Features needed by the shared Android renderer and AHardwareBuffer importer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AndroidVulkanImportSupport {
    pub vulkan_1_1: bool,
    pub vulkan_1_2: bool,
    pub timeline_semaphore: bool,
    pub hardware_buffer_import: bool,
    pub foreign_queue_ownership: bool,
    pub sampler_ycbcr_conversion: bool,
}

impl AndroidVulkanImportSupport {
    /// # Safety
    /// A live Vulkan 1.1 or later instance owns `physical`.
    pub unsafe fn query(
        instance: &Instance,
        physical: vk::PhysicalDevice,
    ) -> Result<Self, VulkanError> {
        // SAFETY: the live instance and physical device are supplied by the caller.
        let properties = unsafe { instance.get_physical_device_properties(physical) };
        // SAFETY: physical belongs to instance and remains live for enumeration.
        let extensions = unsafe { instance.enumerate_device_extension_properties(physical) }?;
        let vulkan_1_1 = properties.api_version >= vk::API_VERSION_1_1;
        let vulkan_1_2 = properties.api_version >= vk::API_VERSION_1_2;
        let mut timeline = vk::PhysicalDeviceTimelineSemaphoreFeatures::default();
        let mut ycbcr = vk::PhysicalDeviceSamplerYcbcrConversionFeatures::default();
        if vulkan_1_1 {
            let mut features = vk::PhysicalDeviceFeatures2::default().push_next(&mut ycbcr);
            if vulkan_1_2 {
                features = features.push_next(&mut timeline);
            }
            // SAFETY: features and its typed pNext are writable, and the device is live.
            unsafe { instance.get_physical_device_features2(physical, &mut features) };
        }
        let has_extension = |name: &std::ffi::CStr| {
            extensions
                .iter()
                .any(|extension| extension.extension_name_as_c_str() == Ok(name))
        };
        Ok(Self {
            vulkan_1_1,
            vulkan_1_2,
            timeline_semaphore: timeline.timeline_semaphore == vk::TRUE,
            hardware_buffer_import: has_extension(external_memory_android_hardware_buffer::NAME),
            foreign_queue_ownership: has_extension(ext::queue_family_foreign::NAME),
            sampler_ycbcr_conversion: ycbcr.sampler_ycbcr_conversion == vk::TRUE,
        })
    }

    pub fn require(self) -> Result<(), VulkanError> {
        crate::compat::validate_android_import_requirements(
            self.vulkan_1_1,
            self.hardware_buffer_import,
            self.sampler_ycbcr_conversion,
        )
        .map_err(VulkanError::UnsupportedAndroidImport)
    }
}

pub struct AndroidVulkanImport {
    context: VulkanDeviceContext,
    extension: external_memory_android_hardware_buffer::Device,
    external_sampler_descriptors: u32,
    sampler: Option<Arc<Android420Sampler>>,
}

impl AndroidVulkanImport {
    /// # Safety
    /// The context's live device enables the Android hardware-buffer external
    /// memory extension, foreign queue family ownership and sampler YCbCr conversion.
    pub unsafe fn new(context: VulkanDeviceContext) -> Result<Self, VulkanError> {
        let instance = context.instance_dispatch();
        // SAFETY: instance owns the physical device retained by context.
        unsafe { AndroidVulkanImportSupport::query(&instance, context.physical_device) }?
            .require()?;
        let extension =
            external_memory_android_hardware_buffer::Device::new(&instance, context.device());
        // SAFETY: this context retains the instance and physical device.
        let extensions =
            unsafe { instance.enumerate_device_extension_properties(context.physical_device) }?;
        // SAFETY: the query reads properties of the same live physical device.
        let api_version =
            unsafe { instance.get_physical_device_properties(context.physical_device) }.api_version;
        let mut maintenance = vk::PhysicalDeviceMaintenance6PropertiesKHR::default();
        if api_version >= crate::compat::VULKAN_1_4
            || extensions
                .iter()
                .any(|extension| extension.extension_name_as_c_str() == Ok(khr::maintenance6::NAME))
        {
            let mut properties =
                vk::PhysicalDeviceProperties2::default().push_next(&mut maintenance);
            // SAFETY: the driver advertises this structure and the output chain is writable.
            unsafe {
                instance.get_physical_device_properties2(context.physical_device, &mut properties)
            };
        }
        Ok(Self {
            context,
            extension,
            external_sampler_descriptors: maintenance.max_combined_image_sampler_descriptor_count,
            sampler: None,
        })
    }

    /// Imports a clear YUV 420 buffer for YCbCr sampling.
    ///
    /// # Safety
    /// `buffer` is a live YUV 420 hardware buffer for this call. Opaque
    /// formats require the producer to attest that the buffer contains YUV
    /// 420. The caller keeps the producer lease alive and waits for its
    /// acquire fence before issuing reads. GPU retirement precedes release.
    pub unsafe fn import_420(
        &mut self,
        buffer: *const vk::AHardwareBuffer,
    ) -> Result<ImportedAndroidVideoBuffer, VulkanError> {
        if buffer.is_null() {
            return Err(VulkanError::UnsupportedAndroidImport(
                "null hardware buffer",
            ));
        }
        let mut description = HardwareBufferDescription::default();
        // SAFETY: the caller retains the described native buffer through the import.
        unsafe { describe(buffer, &mut description) };
        if description.usage & HARDWARE_BUFFER_USAGE_PROTECTED_CONTENT != 0 {
            return Err(VulkanError::UnsupportedAndroidImport(
                "protected hardware buffer",
            ));
        }
        if description.usage & HARDWARE_BUFFER_USAGE_GPU_SAMPLED_IMAGE == 0
            || description.width == 0
            || description.height == 0
            || description.layers != 1
        {
            return Err(VulkanError::UnsupportedAndroidImport(
                "a single sampled image layer is required",
            ));
        }
        let mut format = vk::AndroidHardwareBufferFormatPropertiesANDROID::default();
        let mut properties =
            vk::AndroidHardwareBufferPropertiesANDROID::default().push_next(&mut format);
        // SAFETY: buffer and the enabled extension are live. The output chain is writable.
        unsafe {
            self.extension
                .get_android_hardware_buffer_properties(buffer, &mut properties)
        }?;
        let allocation_size = properties.allocation_size;
        let memory_types = properties.memory_type_bits;
        // The NDK flexible YUV/P010 formats can map to vendor external formats.
        // Treat the driver response as the format authority for image creation.
        let external_format = match format.format {
            vk::Format::G8_B8R8_2PLANE_420_UNORM
            | vk::Format::G8_B8_R8_3PLANE_420_UNORM
            | vk::Format::G10X6_B10X6R10X6_2PLANE_420_UNORM_3PACK16
            | vk::Format::G10X6_B10X6_R10X6_3PLANE_420_UNORM_3PACK16 => 0,
            vk::Format::UNDEFINED if format.external_format != 0 => format.external_format,
            _ => {
                return Err(VulkanError::UnsupportedAndroidImport(
                    "hardware buffer does not declare a supported YUV 420 format",
                ));
            }
        };
        if !format
            .format_features
            .contains(vk::FormatFeatureFlags::SAMPLED_IMAGE)
            || memory_types == 0
            || allocation_size == 0
        {
            return Err(VulkanError::UnsupportedAndroidImport(
                "hardware buffer cannot be allocated as a sampled image",
            ));
        }
        let extent = vk::Extent2D {
            width: description.width,
            height: description.height,
        };
        let sampler = self.sampler_for(&format, external_format)?;
        let mut external = vk::ExternalMemoryImageCreateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::ANDROID_HARDWARE_BUFFER_ANDROID);
        let mut android_format =
            vk::ExternalFormatANDROID::default().external_format(external_format);
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format.format)
            .extent(vk::Extent3D {
                width: extent.width,
                height: extent.height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::SAMPLED)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .push_next(&mut external)
            .push_next(&mut android_format);
        let device = self.context.device();
        // SAFETY: device is live and the image description borrows live extension structs.
        let image = unsafe { device.create_image(&info, None) }?;
        let mut imported = ImportedAndroidVideoBuffer {
            context: self.context.clone(),
            image,
            extent,
            format: format.format,
            external_format,
            view: vk::ImageView::null(),
            sampler,
            memory: vk::DeviceMemory::null(),
        };
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
        let mut import =
            vk::ImportAndroidHardwareBufferInfoANDROID::default().buffer(buffer.cast_mut());
        let allocation = vk::MemoryAllocateInfo::default()
            .allocation_size(allocation_size)
            .memory_type_index(memory_types.trailing_zeros())
            .push_next(&mut dedicated)
            .push_next(&mut import);
        // SAFETY: dedicated and import refer to live compatible objects. Vulkan retains
        // its own hardware buffer reference when the allocation succeeds.
        imported.memory = unsafe { device.allocate_memory(&allocation, None) }?;
        // SAFETY: the imported dedicated allocation is bound once at offset zero.
        unsafe { device.bind_image_memory(image, imported.memory, 0) }?;
        let mut conversion_info =
            vk::SamplerYcbcrConversionInfo::default().conversion(imported.sampler.conversion);
        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(format.format)
            .subresource_range(range(vk::ImageAspectFlags::COLOR))
            .push_next(&mut conversion_info);
        // SAFETY: the image is bound and the view references its matching conversion.
        imported.view = unsafe { device.create_image_view(&view_info, None) }?;
        Ok(imported)
    }

    fn sampler_for(
        &mut self,
        format: &vk::AndroidHardwareBufferFormatPropertiesANDROID<'_>,
        external_format: u64,
    ) -> Result<Arc<Android420Sampler>, VulkanError> {
        let components = if external_format == 0 {
            vk::ComponentMapping::default()
        } else {
            format.sampler_ycbcr_conversion_components
        };
        let key = SamplerKey {
            format: format.format,
            external_format,
            swizzle: [components.r, components.g, components.b, components.a],
            x: format.suggested_x_chroma_offset,
            y: format.suggested_y_chroma_offset,
        };
        if let Some(sampler) = self.sampler.as_ref().filter(|sampler| sampler.key == key) {
            return Ok(Arc::clone(sampler));
        }
        // Query VK_KHR_maintenance6 / Vulkan 1.4 descriptor budgets when available,
        // otherwise fall back to a conservative Vulkan 1.1-1.3 descriptor count.
        let descriptor_count = crate::compat::resolve_external_sampler_descriptors(
            external_format,
            self.external_sampler_descriptors,
        );
        let device = self.context.device();
        let mut conversion_format =
            vk::ExternalFormatANDROID::default().external_format(external_format);
        let conversion_info = vk::SamplerYcbcrConversionCreateInfo::default()
            .format(key.format)
            .components(components)
            // Preserve the coded range and channels for the shader's CICP transform.
            .ycbcr_model(vk::SamplerYcbcrModelConversion::RGB_IDENTITY)
            .x_chroma_offset(key.x)
            .y_chroma_offset(key.y)
            .chroma_filter(vk::Filter::NEAREST)
            .push_next(&mut conversion_format);
        // SAFETY: the live device enabled conversion and the layout comes from the driver.
        let conversion = unsafe { device.create_sampler_ycbcr_conversion(&conversion_info, None) }?;
        let mut sampler = Android420Sampler {
            context: self.context.clone(),
            key,
            conversion,
            handle: vk::Sampler::null(),
            descriptor_count,
        };
        let mut conversion_info = vk::SamplerYcbcrConversionInfo::default().conversion(conversion);
        let sampler_info = vk::SamplerCreateInfo::default()
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .push_next(&mut conversion_info);
        // SAFETY: the sampler uses the same live conversion object as the view.
        sampler.handle = unsafe { device.create_sampler(&sampler_info, None) }?;
        let sampler = Arc::new(sampler);
        self.sampler = Some(Arc::clone(&sampler));
        Ok(sampler)
    }
}

#[derive(PartialEq, Eq)]
struct SamplerKey {
    format: vk::Format,
    external_format: u64,
    swizzle: [vk::ComponentSwizzle; 4],
    x: vk::ChromaLocation,
    y: vk::ChromaLocation,
}

pub(crate) struct Android420Sampler {
    context: VulkanDeviceContext,
    key: SamplerKey,
    conversion: vk::SamplerYcbcrConversion,
    pub(crate) handle: vk::Sampler,
    pub(crate) descriptor_count: u32,
}

impl Drop for Android420Sampler {
    fn drop(&mut self) {
        // SAFETY: every dependent image view and layout retains the owner.
        unsafe {
            self.context.device().destroy_sampler(self.handle, None);
            self.context
                .device()
                .destroy_sampler_ycbcr_conversion(self.conversion, None);
        }
    }
}

/// The image, conversion and imported memory retire together after GPU completion.
pub struct ImportedAndroidVideoBuffer {
    context: VulkanDeviceContext,
    image: vk::Image,
    extent: vk::Extent2D,
    format: vk::Format,
    external_format: u64,
    view: vk::ImageView,
    sampler: Arc<Android420Sampler>,
    memory: vk::DeviceMemory,
}

impl ImportedAndroidVideoBuffer {
    pub fn extent(&self) -> vk::Extent2D {
        self.extent
    }
    pub fn format(&self) -> vk::Format {
        self.format
    }
    pub fn external_format(&self) -> u64 {
        self.external_format
    }
    pub fn view(&self) -> vk::ImageView {
        self.view
    }
    /// A YCbCr conversion sampler must be bound as an immutable combined image sampler.
    pub fn sampler(&self) -> vk::Sampler {
        self.sampler.handle
    }

    pub(crate) fn sampler_owner(&self) -> &Arc<Android420Sampler> {
        &self.sampler
    }

    fn external_queue_family(&self) -> u32 {
        if self.context.features.foreign_queue_ownership {
            vk::QUEUE_FAMILY_FOREIGN_EXT
        } else {
            vk::QUEUE_FAMILY_EXTERNAL
        }
    }

    /// # Safety
    /// `command` belongs to this context's queue family and is recording. The
    /// producer has made the buffer available in GENERAL layout through its fence.
    pub unsafe fn acquire_for_sampling(&self, command: vk::CommandBuffer) {
        let barrier = vk::ImageMemoryBarrier::default()
            .image(self.image)
            .old_layout(vk::ImageLayout::GENERAL)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .src_queue_family_index(self.external_queue_family())
            .dst_queue_family_index(self.context.family)
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
            .subresource_range(range(vk::ImageAspectFlags::COLOR));
        // SAFETY: the caller guarantees producer completion and an active command buffer.
        unsafe {
            self.context.device().cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            )
        };
    }

    /// # Safety
    /// Encode after the final compute read. Keep this image and the producer
    /// lease live until the submission retires, even if the session closes.
    pub unsafe fn release_to_producer(&self, command: vk::CommandBuffer) {
        let barrier = vk::ImageMemoryBarrier::default()
            .image(self.image)
            .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .new_layout(vk::ImageLayout::GENERAL)
            .src_queue_family_index(self.context.family)
            .dst_queue_family_index(self.external_queue_family())
            .src_access_mask(vk::AccessFlags::SHADER_READ)
            .subresource_range(range(vk::ImageAspectFlags::COLOR));
        // SAFETY: command is active and the caller keeps the image live through completion.
        unsafe {
            self.context.device().cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            )
        };
    }
}

impl Drop for ImportedAndroidVideoBuffer {
    fn drop(&mut self) {
        // SAFETY: the owner retains the context and the submitter retires all GPU
        // access before drop. Null handles also cover partial construction failure.
        unsafe {
            let device = self.context.device();
            device.destroy_image_view(self.view, None);
            device.destroy_image(self.image, None);
            device.free_memory(self.memory, None);
        }
    }
}

fn range(aspect: vk::ImageAspectFlags) -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange::default()
        .aspect_mask(aspect)
        .level_count(1)
        .layer_count(1)
}
