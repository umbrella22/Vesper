use ash::{Entry, vk};
use player_plugin_abi::{VESPER_PIXEL_NV12, VESPER_PIXEL_P010, VesperColorInfo, VesperVideoFormat};
use player_render_vulkan::{
    VulkanColorFrame, VulkanColorInput, VulkanDeviceContext, VulkanError, VulkanFrameLease,
    VulkanGpuBackend, VulkanUploadPlane,
};
use std::sync::{Arc, mpsc};
use std::time::Duration;

struct DeviceOwner {
    _entry: Entry,
    instance: ash::Instance,
    device: ash::Device,
    queue: vk::Queue,
    gate: vk::Semaphore,
}

impl Drop for DeviceOwner {
    fn drop(&mut self) {
        // SAFETY: this owner outlives the backend and its retired command buffers.
        unsafe {
            self.device.destroy_semaphore(self.gate, None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

struct Lease(u32, mpsc::Sender<(u32, bool)>);

impl VulkanFrameLease for Lease {
    fn complete(self, accepted: bool) {
        self.1.send((self.0, accepted)).expect("live test observer");
    }
}

fn test_device() -> (Arc<DeviceOwner>, VulkanDeviceContext, vk::PhysicalDevice) {
    // SAFETY: loads the platform Vulkan loader for an explicitly requested test.
    let entry = unsafe { Entry::load() }.expect("load Vulkan");
    // SAFETY: loader is resident and the call does not require an instance.
    let extensions = unsafe { entry.enumerate_instance_extension_properties(None) }
        .expect("instance extensions");
    let portability = extensions.iter().any(|extension| {
        extension.extension_name_as_c_str() == Ok(ash::khr::portability_enumeration::NAME)
    });
    let extension_names = [ash::khr::portability_enumeration::NAME.as_ptr()];
    let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_2);
    let mut info = vk::InstanceCreateInfo::default().application_info(&app);
    if portability {
        info = info
            .flags(vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR)
            .enabled_extension_names(&extension_names);
    }
    // SAFETY: instance parameters and enabled extension names are valid.
    let instance = unsafe { entry.create_instance(&info, None) }.expect("Vulkan instance");
    // SAFETY: instance is live, owns the physical devices, and outlives each query.
    let (physical, family) = unsafe { instance.enumerate_physical_devices() }
        .expect("physical devices")
        .into_iter()
        .find_map(|physical| {
            let mut timeline = vk::PhysicalDeviceTimelineSemaphoreFeatures::default();
            let mut features = vk::PhysicalDeviceFeatures2::default().push_next(&mut timeline);
            // SAFETY: the selected handles and outputs belong to this instance.
            let families = unsafe {
                let properties = instance.get_physical_device_properties(physical);
                if properties.api_version < vk::API_VERSION_1_2 {
                    return None;
                }
                instance.get_physical_device_features2(physical, &mut features);
                instance.get_physical_device_queue_family_properties(physical)
            };
            if timeline.timeline_semaphore != vk::TRUE {
                return None;
            }
            let family = families.iter().position(|family| {
                family.queue_count > 0 && family.queue_flags.contains(vk::QueueFlags::COMPUTE)
            })?;
            Some((physical, family as u32))
        })
        .expect("device supporting timeline semaphores");
    let portability_subset = c"VK_KHR_portability_subset";
    // SAFETY: physical belongs to the live instance.
    let subset = unsafe { instance.enumerate_device_extension_properties(physical) }
        .expect("device extensions")
        .iter()
        .any(|extension| extension.extension_name_as_c_str() == Ok(portability_subset));
    let priorities = [1.0];
    let queue_info = [vk::DeviceQueueCreateInfo::default()
        .queue_family_index(family)
        .queue_priorities(&priorities)];
    let mut timeline_features =
        vk::PhysicalDeviceTimelineSemaphoreFeatures::default().timeline_semaphore(true);
    let mut device_info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queue_info)
        .push_next(&mut timeline_features);
    let extensions = [portability_subset.as_ptr()];
    if subset {
        device_info = device_info.enabled_extension_names(&extensions);
    }
    // SAFETY: all requested features and queues were checked on this physical device.
    let device = unsafe { instance.create_device(physical, &device_info, None) }.expect("device");
    // SAFETY: device creation requested queue 0 for this family.
    let queue = unsafe { device.get_device_queue(family, 0) };
    let mut semaphore_type =
        vk::SemaphoreTypeCreateInfo::default().semaphore_type(vk::SemaphoreType::TIMELINE);
    // SAFETY: timeline semaphores are enabled on the device.
    let gate = unsafe {
        device.create_semaphore(
            &vk::SemaphoreCreateInfo::default().push_next(&mut semaphore_type),
            None,
        )
    }
    .expect("gate");
    let owner = Arc::new(DeviceOwner {
        _entry: entry,
        instance,
        device,
        queue,
        gate,
    });
    // SAFETY: owner retains every handle and this test serializes submissions.
    let context = unsafe {
        VulkanDeviceContext::from_host(
            owner.device.clone(),
            owner.instance.handle(),
            physical,
            queue,
            family,
            owner._entry.static_fn().get_instance_proc_addr,
            owner.clone(),
        )
    };
    (owner, context, physical)
}

#[test]
#[ignore = "requires a Vulkan 1.2 driver with timeline semaphore support"]
fn closed_session_retires_after_gpu_completion_without_another_poll() {
    let (owner, context, _) = test_device();
    let (queue, gate) = (owner.queue, owner.gate);
    let mut backend = VulkanGpuBackend::new(context).expect("frame backend");
    let extent = vk::Extent2D {
        width: 64,
        height: 32,
    };
    let (send, receive) = mpsc::channel();
    let mut reused = None;
    for index in 0..2 {
        // SAFETY: this records only the owned image pair's initial barriers.
        unsafe {
            backend.submit_filtered(
                0,
                Lease(index, send.clone()),
                None,
                None,
                extent,
                |_, _, _, _, images| {
                    assert_eq!(images.extent(), extent);
                    let handles = images.targets().each_ref().map(|target| target.image());
                    assert_ne!(handles[0], handles[1]);
                    if let Some(previous) = reused {
                        assert_eq!(
                            handles, previous,
                            "a retired slot must reuse its image pair"
                        );
                    }
                    reused = Some(handles);
                    Ok(())
                },
            )
        }
        .expect("submit reusable image pair");
        backend
            .drain(Duration::from_secs(3))
            .expect("retire image pair");
        assert_eq!(
            receive.recv_timeout(Duration::from_secs(1)),
            Ok((index, true))
        );
    }

    let wait_values = [1];
    let wait = [gate];
    let stages = [vk::PipelineStageFlags::ALL_COMMANDS];
    let mut timeline =
        vk::TimelineSemaphoreSubmitInfo::default().wait_semaphore_values(&wait_values);
    let queued_gate = vk::SubmitInfo::default()
        .wait_semaphores(&wait)
        .wait_dst_stage_mask(&stages)
        .push_next(&mut timeline);
    // SAFETY: this timeline may wait for a later host signal. The test owns the queue.
    unsafe {
        owner
            .device
            .queue_submit(queue, &[queued_gate], vk::Fence::null())
    }
    .expect("queue gate");

    let mut in_flight = Vec::new();
    for index in 0..3 {
        // SAFETY: the recorder only observes handles. The backend owns the
        // targets and retains them while the queued GPU barriers are in flight.
        unsafe {
            backend.submit_filtered(
                0,
                Lease(index, send.clone()),
                None,
                None,
                extent,
                |_, _, _, _, images| {
                    for target in images.targets() {
                        assert!(
                            !in_flight.contains(&target.image()),
                            "an in-flight image cannot be reused"
                        );
                        in_flight.push(target.image());
                    }
                    Ok(())
                },
            )
        }
        .expect("submit one of three frame slots");
    }
    // SAFETY: as above. The backend must reject the fourth submission before recording.
    let fourth = unsafe { backend.submit(0, Lease(3, send), None, None, |_, _, _, _| Ok(())) };
    assert!(matches!(fourth, Err(VulkanError::Backpressure)));
    backend
        .begin_epoch(1)
        .expect("invalidate epoch during seek");
    drop(backend);

    let early = receive.recv_timeout(Duration::from_millis(30));
    // SAFETY: gate stays live and the single host signal releases the queued wait.
    unsafe {
        owner
            .device
            .signal_semaphore(&vk::SemaphoreSignalInfo::default().semaphore(gate).value(1))
    }
    .expect("release gate");
    assert_eq!(early, Err(mpsc::RecvTimeoutError::Timeout));
    for index in 0..3 {
        assert_eq!(
            receive.recv_timeout(Duration::from_secs(3)),
            Ok((index, false))
        );
    }
    assert_eq!(
        receive.recv_timeout(Duration::from_secs(3)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    );
}

struct Readback {
    owner: Arc<DeviceOwner>,
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
}

impl Drop for Readback {
    fn drop(&mut self) {
        // SAFETY: the test only consumes the readback after the timeline completes.
        unsafe {
            self.owner.device.destroy_buffer(self.buffer, None);
            self.owner.device.free_memory(self.memory, None);
        }
    }
}

#[test]
#[ignore = "requires a Vulkan 1.2 driver and VESPER_VULKAN_COLOR_FILTER_SPIRV"]
fn slanged_nv12_and_p010_filter_reads_strided_planes_and_writes_linear_rgb() {
    let shader_path = std::env::var_os("VESPER_VULKAN_COLOR_FILTER_SPIRV")
        .expect("build the shader with scripts/plugin-shader-build.sh vulkan");
    let shader = ash::util::read_spv(&mut std::fs::File::open(shader_path).expect("shader binary"))
        .expect("SPIR-V words");
    let (owner, context, physical) = test_device();
    let mut backend = VulkanGpuBackend::<Lease>::new(context).expect("frame backend");
    // SAFETY: this is the build-time Slang shader and the test owns a compute queue.
    unsafe { backend.configure_color_filter(&shader) }.expect("configure color filter");
    let extent = vk::Extent3D {
        width: 7,
        height: 5,
        depth: 1,
    };
    let image_size = (extent.width * extent.height * 8) as u64;
    let device = &owner.device;
    // SAFETY: the buffer and allocation belong to the selected live device, with a matching memory type.
    let readback = unsafe {
        let buffer = device
            .create_buffer(
                &vk::BufferCreateInfo::default()
                    .usage(vk::BufferUsageFlags::TRANSFER_DST)
                    .size(image_size),
                None,
            )
            .expect("readback buffer");
        let requirements = device.get_buffer_memory_requirements(buffer);
        let properties = owner
            .instance
            .get_physical_device_memory_properties(physical);
        let flags = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
        let index = (0..properties.memory_type_count)
            .find(|index| {
                requirements.memory_type_bits & (1 << index) != 0
                    && properties.memory_types[*index as usize]
                        .property_flags
                        .contains(flags)
            })
            .expect("compatible coherent memory type");
        let memory = device
            .allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(requirements.size)
                    .memory_type_index(index),
                None,
            )
            .expect("readback memory");
        device
            .bind_buffer_memory(buffer, memory, 0)
            .expect("bind readback memory");
        Readback {
            owner: owner.clone(),
            buffer,
            memory,
        }
    };
    let (send, receive) = mpsc::channel();
    for (i, (pixel_format, color, y_code, uv_code)) in [
        (
            VESPER_PIXEL_NV12,
            VesperColorInfo {
                primaries: 1,
                transfer: 1,
                matrix: 1,
                range: 1,
                bit_depth: 8,
                ..Default::default()
            },
            235u16,
            128u16,
        ),
        // Limited-range P010 code 509 represents approximately 100 nits.
        (
            VESPER_PIXEL_P010,
            VesperColorInfo {
                primaries: 9,
                transfer: 16,
                matrix: 9,
                range: 1,
                bit_depth: 10,
                ..Default::default()
            },
            509,
            512,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let sample_bytes = if color.bit_depth == 10 { 2 } else { 1 };
        let format = VesperVideoFormat {
            pixel_format,
            width: extent.width,
            height: extent.height,
            color,
            ..Default::default()
        };
        let luma_stride = extent.width as usize * sample_bytes + 3;
        let uv_samples = extent.width.div_ceil(2) as usize * 2;
        let chroma_stride = uv_samples * sample_bytes + 5;
        let fill_plane = |stride: usize, rows: u32, samples: usize, code: u16| {
            let mut bytes = vec![0x5a; stride * rows as usize];
            let code = if sample_bytes == 1 { code } else { code << 6 }.to_le_bytes();
            for row in bytes.chunks_exact_mut(stride) {
                for sample in row[..samples * sample_bytes].chunks_exact_mut(sample_bytes) {
                    sample.copy_from_slice(&code[..sample_bytes]);
                }
            }
            bytes
        };
        let luma = fill_plane(luma_stride, extent.height, extent.width as usize, y_code);
        let chroma = fill_plane(
            chroma_stride,
            extent.height.div_ceil(2),
            uv_samples,
            uv_code,
        );
        let input = VulkanColorInput::Staged([
            VulkanUploadPlane {
                bytes: &luma,
                row_stride: luma_stride,
            },
            VulkanUploadPlane {
                bytes: &chroma,
                row_stride: chroma_stride,
            },
        ]);
        // SAFETY: the owned readback is held through drain. The shader output
        // stays in GENERAL, and has the transfer-source usage and correct extent.
        unsafe {
            backend.submit_color_filtered(
                0,
                Lease(i as u32, send.clone()),
                None,
                None,
                VulkanColorFrame {
                    format,
                    gain: 0.5,
                    input,
                },
                |context, command, _, image| {
                    let regions = [vk::BufferImageCopy::default()
                        .image_extent(extent)
                        .image_subresource(
                            vk::ImageSubresourceLayers::default()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .layer_count(1),
                        )];
                    context.device().cmd_copy_image_to_buffer(
                        command,
                        image.image(),
                        vk::ImageLayout::GENERAL,
                        readback.buffer,
                        &regions,
                    );
                    let readable = [vk::MemoryBarrier::default()
                        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                        .dst_access_mask(vk::AccessFlags::HOST_READ)];
                    context.device().cmd_pipeline_barrier(
                        command,
                        vk::PipelineStageFlags::TRANSFER,
                        vk::PipelineStageFlags::HOST,
                        vk::DependencyFlags::empty(),
                        &readable,
                        &[],
                        &[],
                    );
                    Ok(())
                },
            )
        }
        .expect("submit staged filter");
        backend
            .drain(Duration::from_secs(2))
            .expect("retire the frame");
        assert_eq!(
            receive.recv_timeout(Duration::from_secs(1)).expect("lease"),
            (i as u32, true)
        );
        // SAFETY: the barrier and timeline wait completed before mapping coherent memory.
        let pixels = unsafe {
            let mapped = device
                .map_memory(readback.memory, 0, image_size, vk::MemoryMapFlags::empty())
                .expect("map result");
            let pixels =
                std::slice::from_raw_parts(mapped.cast::<u16>(), image_size as usize / 2).to_vec();
            device.unmap_memory(readback.memory);
            pixels
        };
        for pixel in pixels.as_chunks::<4>().0 {
            for &component in &pixel[..3] {
                // Half-float 0.5 is 0x3800. Allow normal PQ quantization error.
                assert!(
                    component.abs_diff(0x3800) < 48,
                    "format {pixel_format} gave {pixel:04x?}"
                );
            }
            assert_eq!(pixel[3], 0x3c00, "opaque target");
        }
    }
}
