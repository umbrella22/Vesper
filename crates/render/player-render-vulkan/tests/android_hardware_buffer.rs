#![cfg(target_os = "android")]

use ash::{Device, Entry, Instance, android, ext, vk};
use player_platform_native_frame::{
    NativeMediaPipeline, NativeMediaVideoPoll, NormalizedVideoPackets,
};
use player_plugin::{SourceNormalizerPacketMediaKind, SourceNormalizerPacketSessionConfig};
use player_plugin_abi::{VESPER_HANDLE_AHB, VESPER_PIXEL_NV12};
use player_plugin_loader::native::NativeVideoFrame;
use player_plugin_loader::{NativePluginArtifact, PluginRegistry};
use player_render_vulkan::{
    AndroidVulkanImport, AndroidVulkanImportSupport, ImportedAndroidVideoBuffer, VulkanColorFrame,
    VulkanColorInput, VulkanDeviceContext, VulkanDeviceFeatures, VulkanError, VulkanFrameLease,
    VulkanGpuBackend,
};
use std::ptr;
use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};
use std::time::{Duration, Instant};

#[repr(C)]
struct BufferDesc {
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
    fn AHardwareBuffer_allocate(desc: *const BufferDesc, out: *mut *mut vk::AHardwareBuffer)
    -> i32;
    fn AHardwareBuffer_release(buffer: *mut vk::AHardwareBuffer);
}

struct DeviceOwner {
    device: Device,
    instance: Instance,
    _entry: Entry,
}

impl Drop for DeviceOwner {
    fn drop(&mut self) {
        // SAFETY: resources and retirement queues retain this owner through completion.
        unsafe {
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

#[test]
#[ignore = "requires an Android Vulkan driver and hardware buffer allocator"]
fn admits_hardware_buffer_import_or_reports_the_required_feature() {
    let expect =
        std::env::var("VESPER_TEST_ANDROID_VULKAN").expect("set the expected device capability");
    // SAFETY: load the device's system Vulkan loader from the default search path.
    let entry = unsafe { Entry::load_from("libvulkan.so") }.expect("system Vulkan loader");
    // SAFETY: entry owns the live Vulkan loader.
    let loader_version = unsafe { entry.try_enumerate_instance_version() }
        .expect("instance version query")
        .unwrap_or(vk::API_VERSION_1_0);
    assert!(
        loader_version >= vk::API_VERSION_1_1,
        "Vulkan 1.1+ is required"
    );
    let instance_version = loader_version.min(vk::make_api_version(0, 1, 4, 0));
    let app = vk::ApplicationInfo::default().api_version(instance_version);
    // SAFETY: app and instance creation info live through the call.
    let instance = unsafe {
        entry.create_instance(
            &vk::InstanceCreateInfo::default().application_info(&app),
            None,
        )
    }
    .expect("Vulkan instance");
    // SAFETY: the instance is live through this test.
    let devices =
        unsafe { instance.enumerate_physical_devices() }.expect("Vulkan physical devices");
    let physical = devices[0];
    // SAFETY: physical was obtained from this live instance.
    let support = unsafe { AndroidVulkanImportSupport::query(&instance, physical) }
        .expect("driver feature query");
    eprintln!("Android Vulkan features: {support:?}");
    if expect == "unsupported" {
        assert!(matches!(
            support.require(),
            Err(VulkanError::UnsupportedAndroidImport(_))
        ));
        // SAFETY: no logical device or child objects were created.
        unsafe { instance.destroy_instance(None) };
        return;
    }
    assert!(matches!(expect.as_str(), "import" | "playback"));
    support.require().expect("Vulkan import support");
    // SAFETY: the physical device belongs to this live instance.
    let family = unsafe { instance.get_physical_device_queue_family_properties(physical) }
        .iter()
        .position(|queue| queue.queue_flags.contains(vk::QueueFlags::COMPUTE))
        .expect("compute queue family") as u32;
    let queues = [vk::DeviceQueueCreateInfo::default()
        .queue_family_index(family)
        .queue_priorities(&[1.0])];
    let mut extensions = vec![android::external_memory_android_hardware_buffer::NAME.as_ptr()];
    if support.foreign_queue_ownership {
        extensions.push(ext::queue_family_foreign::NAME.as_ptr());
    }
    let timeline_semaphores =
        instance_version >= vk::API_VERSION_1_2 && support.vulkan_1_2 && support.timeline_semaphore;
    let mut timeline =
        vk::PhysicalDeviceTimelineSemaphoreFeatures::default().timeline_semaphore(true);
    let mut ycbcr =
        vk::PhysicalDeviceSamplerYcbcrConversionFeatures::default().sampler_ycbcr_conversion(true);
    let mut info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queues)
        .enabled_extension_names(&extensions)
        .push_next(&mut ycbcr);
    if timeline_semaphores {
        info = info.push_next(&mut timeline);
    }
    // SAFETY: this physical device advertised the enabled features and extensions.
    let device = unsafe { instance.create_device(physical, &info, None) }.expect("logical device");
    // SAFETY: this device was created with one queue in family.
    let queue = unsafe { device.get_device_queue(family, 0) };
    let raw_instance = instance.handle();
    let get_proc = entry.static_fn().get_instance_proc_addr;
    let owner = Arc::new(DeviceOwner {
        device: device.clone(),
        instance,
        _entry: entry,
    });
    let features = VulkanDeviceFeatures {
        timeline_semaphores,
        foreign_queue_ownership: support.foreign_queue_ownership,
    };
    // SAFETY: owner retains the complete Vulkan object graph and queue use is exclusive.
    let context = unsafe {
        VulkanDeviceContext::from_host_with_features(
            device,
            raw_instance,
            physical,
            queue,
            family,
            get_proc,
            owner.clone(),
            features,
        )
    };
    // SAFETY: the test enabled both Android import extensions when creating the device.
    let mut importer =
        unsafe { AndroidVulkanImport::new(context.clone()) }.expect("hardware buffer importer");
    if expect == "playback" {
        check_media_codec_playback(context, importer, owner, physical, queue);
        return;
    }

    let mut imported = 0;
    // These are the NDK YCbCr 420 and YCbCr P010 hardware buffer formats.
    for format in [0x23, 0x36] {
        let description = BufferDesc {
            width: 64,
            height: 64,
            layers: 1,
            format,
            usage: 1 << 8,
            stride: 0,
            reserved0: 0,
            reserved1: 0,
        };
        let mut buffer = ptr::null_mut();
        // SAFETY: out is writable and the constructor synchronously reads the description.
        let status = unsafe { AHardwareBuffer_allocate(&description, &mut buffer) };
        if status != 0 {
            eprintln!("Android allocator rejected {format:#x}: status={status}");
            continue;
        }
        // SAFETY: a successful allocation gives the test one live, clear buffer.
        let result = unsafe { importer.import_420(buffer) };
        // SAFETY: relinquish the test's reference. A successful Vulkan import owns its own.
        unsafe { AHardwareBuffer_release(buffer) };
        match result {
            Ok(image) => {
                eprintln!(
                    "Android buffer {format:#x} imported as {:?}, external={:#x}",
                    image.format(),
                    image.external_format(),
                );
                assert_eq!(
                    image.extent(),
                    vk::Extent2D {
                        width: 64,
                        height: 64
                    }
                );
                assert_ne!(image.view(), vk::ImageView::null());
                assert_ne!(image.sampler(), vk::Sampler::null());
                imported += 1;
            }
            Err(error @ VulkanError::UnsupportedAndroidImport(_)) => {
                eprintln!("Android buffer {format:#x}: {error}")
            }
            Err(error) => panic!("Android buffer {format:#x} failed Vulkan import: {error}"),
        }
    }
    assert!(
        imported > 0,
        "no clear 420 buffer imported as a sampleable Vulkan image"
    );
}

struct FrameWork {
    import: Arc<ImportedAndroidVideoBuffer>,
    frame: NativeVideoFrame,
    readback: Arc<PixelReadback>,
    _gate: Arc<GpuGate>,
    completed: Sender<bool>,
}

struct GpuGate {
    owner: Arc<DeviceOwner>,
    semaphore: vk::Semaphore,
}

impl GpuGate {
    fn enqueue(owner: Arc<DeviceOwner>, queue: vk::Queue) -> Arc<Self> {
        let device = &owner.device;
        let mut timeline =
            vk::SemaphoreTypeCreateInfo::default().semaphore_type(vk::SemaphoreType::TIMELINE);
        // SAFETY: the test device enabled timeline semaphores.
        let semaphore = unsafe {
            device.create_semaphore(
                &vk::SemaphoreCreateInfo::default().push_next(&mut timeline),
                None,
            )
        }
        .expect("test GPU gate");
        let gate = Arc::new(Self { owner, semaphore });
        let wait = [semaphore];
        let mut values = vk::TimelineSemaphoreSubmitInfo::default().wait_semaphore_values(&[1]);
        let submission = vk::SubmitInfo::default()
            .push_next(&mut values)
            .wait_semaphores(&wait)
            .wait_dst_stage_mask(&[vk::PipelineStageFlags::ALL_COMMANDS]);
        // SAFETY: this test exclusively owns the queue and retains the gate until retirement.
        unsafe {
            gate.owner
                .device
                .queue_submit(queue, &[submission], vk::Fence::null())
        }
        .expect("enqueue GPU gate");
        gate
    }

    fn signal(&self) {
        // SAFETY: one host signal to a live timeline releases the queued wait.
        unsafe {
            self.owner.device.signal_semaphore(
                &vk::SemaphoreSignalInfo::default()
                    .semaphore(self.semaphore)
                    .value(1),
            )
        }
        .expect("release GPU gate");
    }
}

impl Drop for GpuGate {
    fn drop(&mut self) {
        // SAFETY: the last submission has retired before releasing its gate reference.
        unsafe { self.owner.device.destroy_semaphore(self.semaphore, None) };
    }
}

const SAMPLE_BYTES: u64 = 8;

struct PixelReadback {
    owner: Arc<DeviceOwner>,
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
}

impl PixelReadback {
    fn new(owner: Arc<DeviceOwner>, physical: vk::PhysicalDevice) -> Self {
        let mut readback = Self {
            owner,
            buffer: vk::Buffer::null(),
            memory: vk::DeviceMemory::null(),
        };
        // SAFETY: this owner keeps the device live. The buffer has one coherent host allocation.
        unsafe {
            let device = &readback.owner.device;
            readback.buffer = device
                .create_buffer(
                    &vk::BufferCreateInfo::default()
                        .size(SAMPLE_BYTES)
                        .usage(vk::BufferUsageFlags::TRANSFER_DST),
                    None,
                )
                .expect("readback buffer");
            let requirements = device.get_buffer_memory_requirements(readback.buffer);
            let memory = readback
                .owner
                .instance
                .get_physical_device_memory_properties(physical);
            let flags =
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
            let index = (0..memory.memory_type_count)
                .find(|&index| {
                    requirements.memory_type_bits & (1 << index) != 0
                        && memory.memory_types[index as usize]
                            .property_flags
                            .contains(flags)
                })
                .expect("coherent readback type");
            readback.memory = device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(requirements.size)
                        .memory_type_index(index),
                    None,
                )
                .expect("readback memory");
            device
                .bind_buffer_memory(readback.buffer, readback.memory, 0)
                .expect("bind readback");
        }
        readback
    }

    fn sample(&self) -> [u16; 4] {
        // SAFETY: only called after the transfer-to-host barrier and timeline retirement.
        unsafe {
            let device = &self.owner.device;
            let mapped = device
                .map_memory(self.memory, 0, SAMPLE_BYTES, vk::MemoryMapFlags::empty())
                .expect("map test pixel");
            let sample = mapped.cast::<[u16; 4]>().read();
            device.unmap_memory(self.memory);
            sample
        }
    }
}

impl Drop for PixelReadback {
    fn drop(&mut self) {
        // SAFETY: both the test and the submitting slot own this object through retirement.
        unsafe {
            self.owner.device.destroy_buffer(self.buffer, None);
            self.owner.device.free_memory(self.memory, None);
        }
    }
}

impl VulkanFrameLease for FrameWork {
    fn complete(self, accepted: bool) {
        drop(self.import);
        self.frame.complete(accepted);
        let _ = self.completed.send(accepted);
    }
}

fn start_standalone_binder_pool() {
    use std::ffi::{c_char, c_void};

    #[link(name = "dl")]
    unsafe extern "C" {
        fn dlopen(path: *const c_char, flags: i32) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    // The shell process has no Android application runtime to start its Binder pool.
    // Resolve the API 29 symbol at runtime so allocator probes still link at API 26.
    // Keep the library loaded for the process lifetime because the pool outlives the test.
    const RTLD_NOW: i32 = 2;
    // SAFETY: the Android 29+ system library exports this exact no-argument C function.
    unsafe {
        let library = dlopen(c"libbinder_ndk.so".as_ptr(), RTLD_NOW);
        assert!(
            !library.is_null(),
            "standalone playback requires Android 29+"
        );
        let start = dlsym(library, c"ABinderProcess_startThreadPool".as_ptr());
        assert!(!start.is_null(), "Binder thread pool entry is unavailable");
        std::mem::transmute::<_, unsafe extern "C" fn()>(start)();
    }
    eprintln!("Standalone Binder thread pool started");
}

fn check_media_codec_playback(
    context: VulkanDeviceContext,
    mut importer: AndroidVulkanImport,
    owner: Arc<DeviceOwner>,
    physical: vk::PhysicalDevice,
    queue: vk::Queue,
) {
    start_standalone_binder_pool();
    let requested = |key| std::env::var(key).unwrap_or_else(|error| panic!("{key}: {error}"));
    let load = |id, key| {
        use player_plugin::{PluginReference, PluginTransport};

        // The device test runner stages its own builds in a private directory.
        let artifact = NativePluginArtifact::new(id, requested(key)).expect("fixture identity");
        let registry =
            PluginRegistry::load_native_artifacts([artifact]).expect("load staged plugin");
        let reference =
            PluginReference::new(id, None, PluginTransport::Native).expect("native reference");
        registry
            .native_artifact(&reference)
            .expect("admitted module")
            .native_plugin()
            .expect("native entry")
    };
    let normalizer = load(
        "io.github.umbrella22.vesper.source-normalizer-ffmpeg",
        "VESPER_TEST_NORMALIZER",
    );
    let decoder = load(
        "io.github.umbrella22.vesper.decoder-mediacodec",
        "VESPER_TEST_DECODER",
    );
    let stream = normalizer
        .open_packet_stream(&SourceNormalizerPacketSessionConfig {
            input: requested("VESPER_TEST_MEDIA"),
            runtime_profile: "generic-fallback".to_owned(),
            preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
            headers: vec![],
            startup_timeout_ms: Some(5000),
            session_timeout_ms: None,
        })
        .expect("open local AVC fixture");
    let track = stream
        .stream_info()
        .tracks
        .iter()
        .find(|track| track.media_kind == SourceNormalizerPacketMediaKind::Video)
        .expect("video track");
    let stream_index = track.stream_index;
    let options = serde_json::to_vec(&serde_json::json!({
        "width": track.width, "height": track.height, "pixelFormat": VESPER_PIXEL_NV12,
        "hardwareDecoderName": requested("VESPER_TEST_HARDWARE_DECODER"),
    }))
    .expect("serialize MediaCodec options");
    let decoder = decoder
        .open_video_decoder(
            u32::from_be_bytes(*b"avc1"),
            &track.extradata,
            &options,
            Duration::from_millis(700),
        )
        .expect("MediaCodec output to AImageReader");
    let mut pipeline =
        NativeMediaPipeline::new(NormalizedVideoPackets::new(stream, stream_index), decoder);
    let mut backend = VulkanGpuBackend::new(context).expect("timeline Vulkan backend");
    let shader_file =
        std::fs::File::open(requested("VESPER_TEST_SHADER")).expect("Slang shader binary");
    let shader =
        ash::util::read_spv(&mut std::io::BufReader::new(shader_file)).expect("SPIR-V words");
    // SAFETY: the test stages the trusted Slang combined-420 shader and enabled YCbCr conversion.
    unsafe { backend.configure_android_color_filter(shader.into_boxed_slice()) }
        .expect("configure combined YCbCr filtering");
    let (completed, callbacks) = channel();
    let mut readbacks = Vec::new();
    let gate = GpuGate::enqueue(Arc::clone(&owner), queue);

    let mut last_pts = i64::MIN;
    for epoch in 0..3 {
        let target = Duration::from_millis(250 * epoch);
        if epoch > 0 {
            backend
                .begin_epoch(epoch)
                .expect("begin presentation epoch");
            pipeline.seek(target).expect("seek codec and normalizer");
        }
        let deadline = Instant::now() + Duration::from_secs(6);
        let (frame, format) = loop {
            assert!(
                Instant::now() < deadline,
                "no MediaCodec frame reached the seek target"
            );
            match pipeline.advance().expect("advance shared media pipeline") {
                NativeMediaVideoPoll::Frame { lease, format } => {
                    assert!(format.width > 0 && format.height > 0);
                    if lease.descriptor().pts_us >= target.as_micros() as i64 {
                        break (lease, format);
                    }
                    lease.complete(false);
                }
                NativeMediaVideoPoll::Pending => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                NativeMediaVideoPoll::EndOfStream => panic!("fixture ended without a frame"),
            }
        };
        let descriptor = frame.descriptor();
        assert_eq!(descriptor.handle_kind, VESPER_HANDLE_AHB);
        let pts = descriptor.pts_us;
        // SAFETY: this clear AVC decoder returns a live 420 AHardwareBuffer lease only after its acquire fence.
        let import = Arc::new(
            unsafe { importer.import_420(descriptor.handles[0] as *const vk::AHardwareBuffer) }
                .expect("import MediaCodec hardware buffer"),
        );
        eprintln!(
            "MediaCodec -> Vulkan epoch={epoch} pts_us={pts} vk_format={:?} external={:#x}",
            import.format(),
            import.external_format()
        );
        assert!(pts > last_pts, "seek must advance the presented timestamp");
        last_pts = pts;
        let readback = Arc::new(PixelReadback::new(Arc::clone(&owner), physical));
        readbacks.push(Arc::clone(&readback));
        let work = FrameWork {
            import: Arc::clone(&import),
            frame,
            readback,
            _gate: Arc::clone(&gate),
            completed: completed.clone(),
        };
        // SAFETY: each slot retains the import, the producer lease and the test readback.
        // The output stays in GENERAL and has transfer-source usage.
        unsafe {
            backend.submit_color_filtered(
                epoch,
                work,
                None,
                None,
                VulkanColorFrame {
                    format,
                    gain: if epoch == 1 { 0.0 } else { 1.0 },
                    input: VulkanColorInput::Android(&import),
                },
                |context, command, work, output| {
                    let sample = [vk::BufferImageCopy::default()
                        .image_subresource(
                            vk::ImageSubresourceLayers::default()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .layer_count(1),
                        )
                        .image_offset(vk::Offset3D {
                            x: (format.width / 2) as i32,
                            y: (format.height / 8) as i32,
                            z: 0,
                        })
                        .image_extent(vk::Extent3D {
                            width: 1,
                            height: 1,
                            depth: 1,
                        })];
                    context.device().cmd_copy_image_to_buffer(
                        command,
                        output.image(),
                        vk::ImageLayout::GENERAL,
                        work.readback.buffer,
                        &sample,
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
        .expect("submit imported buffer through Slang filter");
    }
    assert!(last_pts >= 0);
    drop(pipeline);
    drop(backend);
    let early = callbacks.recv_timeout(Duration::from_millis(30));
    gate.signal();
    assert_eq!(early, Err(std::sync::mpsc::RecvTimeoutError::Timeout));
    let results: Vec<_> = (0..3)
        .map(|_| {
            callbacks
                .recv_timeout(Duration::from_secs(3))
                .expect("GPU completion reaches decoder lease after seek or close")
        })
        .collect();
    assert_eq!(
        results,
        [false, false, false],
        "obsolete epochs must retire as dropped"
    );
    for (epoch, readback) in readbacks.iter().enumerate() {
        let sample = readback.sample();
        eprintln!("Android Vulkan filtered pixel epoch={epoch}: {sample:04x?}");
        assert_eq!(sample[3], 0x3c00, "filter writes opaque linear RGBA");
        if epoch == 1 {
            assert_eq!(sample[..3], [0, 0, 0], "zero gain produces black");
        } else {
            assert!(
                sample[..3].iter().any(|component| component & 0x7fff != 0),
                "fixture must sample a nonblack pixel"
            );
        }
    }
}
