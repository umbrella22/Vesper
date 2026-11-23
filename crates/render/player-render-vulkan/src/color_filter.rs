use std::mem::size_of;
use std::sync::Arc;

use ash::vk;
use player_plugin_abi::{VESPER_PIXEL_NV12, VESPER_PIXEL_P010, VesperVideoFormat};

#[cfg(target_os = "android")]
use crate::android_buffer::{Android420Sampler, ImportedAndroidVideoBuffer};
use crate::upload::{Upload420, VulkanUploadPlane};
use crate::{FRAME_SLOTS, VulkanDeviceContext, VulkanError, VulkanFilterImage};

const PARAM_SIZE: usize = size_of::<[u32; 8]>();

pub enum VulkanColorInput<'a> {
    /// The caller owns both views and supplies the shader-read transition.
    Imported([vk::ImageView; 2]),
    /// An SDR NV12 texture shared by a same-adapter D3D11 producer.
    #[cfg(target_os = "windows")]
    Win32(&'a crate::ImportedWin32VideoImage),
    /// Luma and chroma from a synchronized Linux DMA-BUF frame.
    #[cfg(target_os = "linux")]
    DmaBuf(&'a crate::ImportedDmaBufVideoImage),
    /// Packed or strided software planes copied into Vulkan-owned staging memory.
    Staged([VulkanUploadPlane<'a>; 2]),
    /// A clear Android hardware buffer using its immutable combined sampler.
    #[cfg(target_os = "android")]
    Android(&'a ImportedAndroidVideoBuffer),
}

pub struct VulkanColorFrame<'a> {
    pub format: VesperVideoFormat,
    pub gain: f32,
    pub input: VulkanColorInput<'a>,
}

impl VulkanColorFrame<'_> {
    pub(crate) fn extent(&self) -> Result<vk::Extent2D, VulkanError> {
        let bit_depth = match self.format.pixel_format {
            VESPER_PIXEL_NV12 => 8,
            VESPER_PIXEL_P010 => 10,
            _ => return Err(VulkanError::UnsupportedColorInput),
        };
        let format = &self.format;
        if !self.gain.is_finite()
            || format.width == 0
            || format.height == 0
            || format.color.bit_depth != bit_depth
            || format.rotation_deg != 0
            || format.mirror_flags != 0
            || format.crop_x != 0
            || format.crop_y != 0
            || (format.crop_w != 0 && format.crop_w != format.width)
            || (format.crop_h != 0 && format.crop_h != format.height)
        {
            return Err(VulkanError::UnsupportedColorInput);
        }
        Ok(vk::Extent2D {
            width: format.width,
            height: format.height,
        })
    }

    fn parameters(&self) -> [u8; PARAM_SIZE] {
        let color = self.format.color;
        let values = [
            self.format.width,
            self.format.height,
            color.bit_depth.into(),
            color.range.into(),
            color.primaries.into(),
            color.matrix.into(),
            color.transfer.into(),
            self.gain.to_bits(),
        ];
        let mut data = [0; PARAM_SIZE];
        let (words, _) = data.as_chunks_mut::<{ size_of::<u32>() }>();
        for (bytes, value) in words.iter_mut().zip(values) {
            *bytes = value.to_ne_bytes();
        }
        data
    }
}

pub(crate) struct ColorFilter {
    pipeline: Option<Arc<ColorPipeline>>,
    slots: [Option<ColorFilterSlot>; FRAME_SLOTS],
    #[cfg(target_os = "android")]
    combined_shader: Option<Box<[u32]>>,
}

impl ColorFilter {
    pub(crate) fn new(context: &VulkanDeviceContext, shader: &[u32]) -> Result<Self, VulkanError> {
        let pipeline = Arc::new(ColorPipeline::new(
            context,
            shader,
            ColorInputLayout::Planes,
        )?);
        Ok(Self {
            pipeline: Some(pipeline),
            slots: std::array::from_fn(|_| None),
            #[cfg(target_os = "android")]
            combined_shader: None,
        })
    }

    #[cfg(target_os = "android")]
    pub(crate) fn android(shader: Box<[u32]>) -> Self {
        Self {
            pipeline: None,
            slots: std::array::from_fn(|_| None),
            combined_shader: Some(shader),
        }
    }

    pub(crate) fn encode(
        &mut self,
        slot: usize,
        context: &VulkanDeviceContext,
        command: vk::CommandBuffer,
        frame: VulkanColorFrame<'_>,
        output: &VulkanFilterImage,
    ) -> Result<(), VulkanError> {
        let extent = frame.extent()?;
        #[cfg(target_os = "linux")]
        let dma_buf = if let VulkanColorInput::DmaBuf(image) = &frame.input {
            image.check_frame(context, extent, frame.format.pixel_format)?;
            Some(*image)
        } else {
            None
        };
        #[cfg(target_os = "windows")]
        let win32 = if let VulkanColorInput::Win32(image) = &frame.input {
            image.check_frame(context, extent, frame.format.pixel_format)?;
            Some(*image)
        } else {
            None
        };
        #[cfg(target_os = "android")]
        let android = match frame.input {
            VulkanColorInput::Android(image) => {
                if image.extent() != extent {
                    return Err(VulkanError::UnsupportedColorInput);
                }
                let shader = self
                    .combined_shader
                    .as_deref()
                    .ok_or(VulkanError::ColorFilterUnavailable)?;
                let sampler = image.sampler_owner();
                if !matches!(self.pipeline.as_deref().map(|pipeline| &pipeline.input), Some(ColorInputLayout::Android(current)) if Arc::ptr_eq(current, sampler))
                {
                    self.pipeline = Some(Arc::new(ColorPipeline::new(
                        context,
                        shader,
                        ColorInputLayout::Android(Arc::clone(sampler)),
                    )?));
                }
                Some(image)
            }
            _ if self.combined_shader.is_some() => return Err(VulkanError::UnsupportedColorInput),
            _ => None,
        };
        let pipeline = self
            .pipeline
            .as_ref()
            .ok_or(VulkanError::ColorFilterUnavailable)?;
        let slot = &mut self.slots[slot];
        if slot
            .as_ref()
            .is_none_or(|binding| !Arc::ptr_eq(&binding.pipeline, pipeline))
        {
            *slot = Some(ColorFilterSlot::new(pipeline)?);
        }
        let binding = slot.as_mut().ok_or(VulkanError::Exhausted)?;
        let parameters = frame.parameters();
        let inputs = match frame.input {
            VulkanColorInput::Imported(views) => views,
            #[cfg(target_os = "windows")]
            VulkanColorInput::Win32(image) => image.views(),
            #[cfg(target_os = "linux")]
            VulkanColorInput::DmaBuf(image) => image.views(),
            VulkanColorInput::Staged(planes) => {
                if binding
                    .upload
                    .as_ref()
                    .is_none_or(|upload| !upload.matches(extent, frame.format.pixel_format))
                {
                    binding.upload =
                        Some(Upload420::new(context, extent, frame.format.pixel_format)?);
                }
                let upload = binding.upload.as_mut().ok_or(VulkanError::Exhausted)?;
                upload.record(command, planes)?
            }
            #[cfg(target_os = "android")]
            VulkanColorInput::Android(image) => [image.view(), vk::ImageView::null()],
        };
        let (input_type, input_count, _) = pipeline.input.descriptors();
        if inputs[..input_count].contains(&vk::ImageView::null()) {
            return Err(VulkanError::UnsupportedColorInput);
        }
        let device = context.device();
        let input_images = inputs.map(|view| {
            [vk::DescriptorImageInfo::default()
                .image_view(view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)]
        });
        let output_image = [vk::DescriptorImageInfo::default()
            .image_view(output.view())
            .image_layout(vk::ImageLayout::GENERAL)];
        let params_binding = [vk::DescriptorBufferInfo::default()
            .buffer(binding.params)
            .range(PARAM_SIZE as u64)];
        let inputs = std::array::from_fn::<_, 2, _>(|index| {
            vk::WriteDescriptorSet::default()
                .dst_set(binding.descriptor)
                .dst_binding(index as u32)
                .descriptor_type(input_type)
                .image_info(&input_images[index])
        });
        let outputs = [
            vk::WriteDescriptorSet::default()
                .dst_set(binding.descriptor)
                .dst_binding(2)
                .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                .image_info(&output_image),
            vk::WriteDescriptorSet::default()
                .dst_set(binding.descriptor)
                .dst_binding(3)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .buffer_info(&params_binding),
        ];
        let uniforms_ready = vk::BufferMemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::UNIFORM_READ)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .buffer(binding.params)
            .size(PARAM_SIZE as u64);
        let output_ready = vk::MemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::SHADER_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ | vk::AccessFlags::TRANSFER_READ);
        // SAFETY: the command slot is retired and exclusively held by the caller.
        // The output and descriptors remain alive for its next submission.
        unsafe {
            #[cfg(target_os = "android")]
            if let Some(image) = android {
                image.acquire_for_sampling(command);
            }
            device.update_descriptor_sets(&inputs[..input_count], &[]);
            device.update_descriptor_sets(&outputs, &[]);
            device.cmd_update_buffer(command, binding.params, 0, &parameters);
            device.cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[uniforms_ready],
                &[],
            );
            device.cmd_bind_pipeline(
                command,
                vk::PipelineBindPoint::COMPUTE,
                binding.pipeline.pipeline,
            );
            device.cmd_bind_descriptor_sets(
                command,
                vk::PipelineBindPoint::COMPUTE,
                binding.pipeline.layout,
                0,
                &[binding.descriptor],
                &[],
            );
            #[cfg(target_os = "windows")]
            if let Some(image) = win32 {
                image.encode_ownership(command, true);
            }
            #[cfg(target_os = "linux")]
            if let Some(image) = dma_buf {
                image.encode_ownership(command, true);
            }
            device.cmd_dispatch(
                command,
                extent.width.div_ceil(16),
                extent.height.div_ceil(8),
                1,
            );
            #[cfg(target_os = "windows")]
            if let Some(image) = win32 {
                image.encode_ownership(command, false);
            }
            #[cfg(target_os = "linux")]
            if let Some(image) = dma_buf {
                image.encode_ownership(command, false);
            }
            #[cfg(target_os = "android")]
            if let Some(image) = android {
                image.release_to_producer(command);
            }
            device.cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::DependencyFlags::empty(),
                &[output_ready],
                &[],
                &[],
            );
        }
        Ok(())
    }
}

enum ColorInputLayout {
    Planes,
    #[cfg(target_os = "android")]
    Android(Arc<Android420Sampler>),
}

impl ColorInputLayout {
    fn descriptors(&self) -> (vk::DescriptorType, usize, u32) {
        match self {
            Self::Planes => (vk::DescriptorType::SAMPLED_IMAGE, 2, 2),
            #[cfg(target_os = "android")]
            Self::Android(sampler) => (
                vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                1,
                sampler.descriptor_count,
            ),
        }
    }

    fn immutable_samplers(&self) -> &[vk::Sampler] {
        match self {
            Self::Planes => &[],
            #[cfg(target_os = "android")]
            Self::Android(sampler) => std::slice::from_ref(&sampler.handle),
        }
    }
}

struct ColorPipeline {
    context: VulkanDeviceContext,
    input: ColorInputLayout,
    descriptor_layout: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
}

impl ColorPipeline {
    fn new(
        context: &VulkanDeviceContext,
        shader: &[u32],
        input: ColorInputLayout,
    ) -> Result<Self, VulkanError> {
        let device = context.device();
        let mut state = Self {
            context: context.clone(),
            input,
            descriptor_layout: vk::DescriptorSetLayout::null(),
            layout: vk::PipelineLayout::null(),
            pipeline: vk::Pipeline::null(),
        };
        let (input_type, input_count, _) = state.input.descriptors();
        let binding = |index, ty| {
            vk::DescriptorSetLayoutBinding::default()
                .binding(index)
                .descriptor_type(ty)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
        };
        let mut inputs = [binding(0, input_type), binding(1, input_type)];
        let samplers = state.input.immutable_samplers();
        if !samplers.is_empty() {
            inputs[0] = inputs[0].immutable_samplers(samplers);
        }
        let outputs = [
            binding(2, vk::DescriptorType::STORAGE_IMAGE),
            binding(3, vk::DescriptorType::UNIFORM_BUFFER),
        ];
        let bindings = [&inputs[..input_count], &outputs].concat();
        // SAFETY: the descriptor layout satisfies the reference shader contract.
        unsafe {
            state.descriptor_layout = device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                None,
            )?;
            state.layout = device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default().set_layouts(&[state.descriptor_layout]),
                None,
            )?;
        }
        // SAFETY: shader comes from an explicitly selected trusted build-time artifact.
        let module = unsafe {
            device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(shader), None)
        }?;
        let stage = vk::PipelineShaderStageCreateInfo::default()
            .module(module)
            .name(c"main")
            .stage(vk::ShaderStageFlags::COMPUTE);
        let pipeline_info = vk::ComputePipelineCreateInfo::default()
            .stage(stage)
            .layout(state.layout);
        // SAFETY: module and layout belong to this device and are live during creation.
        let result = unsafe {
            let result =
                device.create_compute_pipelines(vk::PipelineCache::null(), &[pipeline_info], None);
            device.destroy_shader_module(module, None);
            result
        };
        match result {
            Ok(pipelines) => state.pipeline = pipelines[0],
            Err((pipelines, error)) => {
                for pipeline in pipelines {
                    // SAFETY: a partial result owns these unused pipelines. Vulkan accepts null.
                    unsafe { device.destroy_pipeline(pipeline, None) };
                }
                return Err(error.into());
            }
        }
        Ok(state)
    }
}

impl Drop for ColorPipeline {
    fn drop(&mut self) {
        // SAFETY: queue slots release their owners only once GPU work has retired.
        // Vulkan accepts null handles if construction failed before allocation.
        unsafe {
            let device = self.context.device();
            device.destroy_pipeline(self.pipeline, None);
            device.destroy_pipeline_layout(self.layout, None);
            device.destroy_descriptor_set_layout(self.descriptor_layout, None);
        }
    }
}

struct ColorFilterSlot {
    pipeline: Arc<ColorPipeline>,
    pool: vk::DescriptorPool,
    descriptor: vk::DescriptorSet,
    params: vk::Buffer,
    memory: vk::DeviceMemory,
    upload: Option<Upload420>,
}

impl ColorFilterSlot {
    fn new(pipeline: &Arc<ColorPipeline>) -> Result<Self, VulkanError> {
        let context = &pipeline.context;
        let device = context.device();
        let mut slot = Self {
            pipeline: Arc::clone(pipeline),
            pool: vk::DescriptorPool::null(),
            descriptor: vk::DescriptorSet::null(),
            params: vk::Buffer::null(),
            memory: vk::DeviceMemory::null(),
            upload: None,
        };
        let (input_type, _, descriptor_count) = pipeline.input.descriptors();
        let descriptors = [
            vk::DescriptorPoolSize {
                ty: input_type,
                descriptor_count,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::STORAGE_IMAGE,
                descriptor_count: 1,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::UNIFORM_BUFFER,
                descriptor_count: 1,
            },
        ];
        // SAFETY: the pool owns exactly one set for this slot and the uniform is private.
        unsafe {
            slot.pool = device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(1)
                    .pool_sizes(&descriptors),
                None,
            )?;
            slot.descriptor = device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(slot.pool)
                    .set_layouts(&[pipeline.descriptor_layout]),
            )?[0];
            slot.params = device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(PARAM_SIZE as u64)
                    .usage(
                        vk::BufferUsageFlags::UNIFORM_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
                    ),
                None,
            )?;
            let requirements = device.get_buffer_memory_requirements(slot.params);
            slot.memory =
                context.allocate_memory(requirements, vk::MemoryPropertyFlags::DEVICE_LOCAL)?;
            device.bind_buffer_memory(slot.params, slot.memory, 0)?;
        }
        Ok(slot)
    }
}

impl Drop for ColorFilterSlot {
    fn drop(&mut self) {
        // SAFETY: no GPU work remains by the time this slot is dropped.
        unsafe {
            let device = self.pipeline.context.device();
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_buffer(self.params, None);
            device.free_memory(self.memory, None);
        }
    }
}
