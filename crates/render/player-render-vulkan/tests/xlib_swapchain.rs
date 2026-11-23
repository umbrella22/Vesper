#![cfg(target_os = "linux")]
#![warn(clippy::undocumented_unsafe_blocks)]

use ash::{Entry, vk};
use player_plugin_abi::{VESPER_PIXEL_NV12, VesperColorInfo, VesperVideoFormat};
use player_render_vulkan::{
    VulkanColorFrame, VulkanColorInput, VulkanDeviceContext, VulkanFrameLease, VulkanGpuBackend,
    VulkanPresentStatus, VulkanSwapchainFrame, VulkanUploadPlane,
};
use std::ffi::{c_char, c_int, c_uint, c_ulong};
use std::io::Read;
use std::sync::{Arc, mpsc};
use std::time::Duration;

#[link(name = "X11")]
unsafe extern "C" {
    fn XInitThreads() -> c_int;
    fn XOpenDisplay(name: *const c_char) -> *mut vk::Display;
    fn XDefaultRootWindow(display: *mut vk::Display) -> vk::Window;
    fn XCreateSimpleWindow(
        display: *mut vk::Display,
        parent: vk::Window,
        x: c_int,
        y: c_int,
        width: c_uint,
        height: c_uint,
        border_width: c_uint,
        border: c_ulong,
        background: c_ulong,
    ) -> vk::Window;
    fn XMapWindow(display: *mut vk::Display, window: vk::Window) -> c_int;
    fn XSync(display: *mut vk::Display, discard: c_int) -> c_int;
    fn XDestroyWindow(display: *mut vk::Display, window: vk::Window) -> c_int;
    fn XCloseDisplay(display: *mut vk::Display) -> c_int;
}

struct Window {
    display: *mut vk::Display,
    window: vk::Window,
}

// SAFETY: XInitThreads enables concurrent Xlib access before the display opens.
unsafe impl Send for Window {}
// SAFETY: the display and window are destroyed only by their unique owner.
unsafe impl Sync for Window {}

impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: this owner retains both Xlib handles until Vulkan surface destruction.
        unsafe {
            XDestroyWindow(self.display, self.window);
            XCloseDisplay(self.display);
        }
    }
}

struct Host {
    _window: Window,
    entry: Entry,
    instance: ash::Instance,
    surface_api: ash::khr::surface::Instance,
    surface: vk::SurfaceKHR,
    device: ash::Device,
    swapchain_api: ash::khr::swapchain::Device,
    swapchain: vk::SwapchainKHR,
    semaphores: Vec<vk::Semaphore>,
}

impl Drop for Host {
    fn drop(&mut self) {
        // SAFETY: the test drains graphics and presentation before normal drop.
        // The idle wait also covers already-submitted work when an assertion unwinds.
        unsafe {
            let _ = self.device.device_wait_idle();
            for semaphore in self.semaphores.drain(..) {
                self.device.destroy_semaphore(semaphore, None);
            }
            self.swapchain_api.destroy_swapchain(self.swapchain, None);
            self.device.destroy_device(None);
            self.surface_api.destroy_surface(self.surface, None);
            self.instance.destroy_instance(None);
        }
    }
}

struct Lease(Arc<Host>, mpsc::Sender<bool>);

impl VulkanFrameLease for Lease {
    fn complete(self, presented: bool) {
        let _ = self.1.send(presented);
        drop(self.0);
    }
}

#[test]
#[ignore = "requires X11, a Vulkan 1.2 presentation driver and VESPER_VULKAN_COLOR_FILTER_SPIRV"]
fn presents_filtered_nv12_through_a_linux_swapchain() {
    let size = vk::Extent2D {
        width: 1920,
        height: 1080,
    };
    let shader = std::env::var_os("VESPER_VULKAN_COLOR_FILTER_SPIRV").expect("Slang SPIR-V path");
    let shader = ash::util::read_spv(&mut std::fs::File::open(shader).expect("read SPIR-V"))
        .expect("SPIR-V words");
    // SAFETY: this test serially initializes Xlib, Vulkan and their live handle graph.
    let (host, context, images, extent, format) = unsafe {
        assert_ne!(XInitThreads(), 0);
        let display = XOpenDisplay(std::ptr::null());
        assert!(!display.is_null(), "set DISPLAY to an available X11 server");
        let window = Window {
            display,
            window: XCreateSimpleWindow(
                display,
                XDefaultRootWindow(display),
                0,
                0,
                size.width,
                size.height,
                0,
                0,
                0,
            ),
        };
        assert_ne!(window.window, 0);
        XMapWindow(display, window.window);
        XSync(display, 0);
        let entry = Entry::load().expect("load Vulkan");
        let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_2);
        let surface_extensions = [
            ash::khr::surface::NAME.as_ptr(),
            ash::khr::xlib_surface::NAME.as_ptr(),
        ];
        let instance = entry
            .create_instance(
                &vk::InstanceCreateInfo::default()
                    .application_info(&app)
                    .enabled_extension_names(&surface_extensions),
                None,
            )
            .expect("create X11 Vulkan instance");
        let surface_api = ash::khr::surface::Instance::new(&entry, &instance);
        let surface = ash::khr::xlib_surface::Instance::new(&entry, &instance)
            .create_xlib_surface(
                &vk::XlibSurfaceCreateInfoKHR::default()
                    .dpy(display)
                    .window(window.window),
                None,
            )
            .expect("create Vulkan Xlib surface");
        let (physical, family) = instance
            .enumerate_physical_devices()
            .expect("physical devices")
            .into_iter()
            .find_map(|physical| {
                if instance
                    .get_physical_device_properties(physical)
                    .api_version
                    < vk::API_VERSION_1_2
                {
                    return None;
                }
                let mut timeline = vk::PhysicalDeviceTimelineSemaphoreFeatures::default();
                instance.get_physical_device_features2(
                    physical,
                    &mut vk::PhysicalDeviceFeatures2::default().push_next(&mut timeline),
                );
                if timeline.timeline_semaphore != vk::TRUE {
                    return None;
                }
                instance
                    .get_physical_device_queue_family_properties(physical)
                    .iter()
                    .enumerate()
                    .find_map(|(index, props)| {
                        let family = index as u32;
                        (props.queue_count > 0
                            && props
                                .queue_flags
                                .contains(vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE)
                            && surface_api
                                .get_physical_device_surface_support(physical, family, surface)
                                .unwrap_or(false))
                        .then_some((physical, family))
                    })
            })
            .expect("graphics, compute and X11 presentation queue with timeline semaphores");
        let priorities = [1.0];
        let queues = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(family)
            .queue_priorities(&priorities)];
        let extensions = [ash::khr::swapchain::NAME.as_ptr()];
        let mut timeline =
            vk::PhysicalDeviceTimelineSemaphoreFeatures::default().timeline_semaphore(true);
        let device = instance
            .create_device(
                physical,
                &vk::DeviceCreateInfo::default()
                    .queue_create_infos(&queues)
                    .enabled_extension_names(&extensions)
                    .push_next(&mut timeline),
                None,
            )
            .expect("create device");
        let queue = device.get_device_queue(family, 0);
        let swapchain_api = ash::khr::swapchain::Device::new(&instance, &device);
        let caps = surface_api
            .get_physical_device_surface_capabilities(physical, surface)
            .expect("surface capabilities");
        assert!(
            caps.supported_usage_flags
                .contains(vk::ImageUsageFlags::TRANSFER_DST)
        );
        let format = surface_api
            .get_physical_device_surface_formats(physical, surface)
            .expect("surface formats")
            .into_iter()
            .find(|entry| {
                matches!(
                    entry.format,
                    vk::Format::B8G8R8A8_SRGB | vk::Format::R8G8B8A8_SRGB
                )
            })
            .expect("sRGB surface format");
        let extent = caps.current_extent;
        assert_eq!(extent, size);
        let swapchain = swapchain_api
            .create_swapchain(
                &vk::SwapchainCreateInfoKHR::default()
                    .surface(surface)
                    .min_image_count(caps.min_image_count)
                    .image_format(format.format)
                    .image_color_space(format.color_space)
                    .image_extent(extent)
                    .image_array_layers(1)
                    .image_usage(vk::ImageUsageFlags::TRANSFER_DST)
                    .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .pre_transform(caps.current_transform)
                    .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
                    .present_mode(vk::PresentModeKHR::FIFO)
                    .clipped(true),
                None,
            )
            .expect("create swapchain");
        let images = swapchain_api
            .get_swapchain_images(swapchain)
            .expect("swapchain images");
        let semaphores = (0..=images.len())
            .map(|_| {
                device
                    .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
                    .expect("binary semaphore")
            })
            .collect();
        let host = Arc::new(Host {
            _window: window,
            entry,
            instance,
            surface_api,
            surface,
            device,
            swapchain_api,
            swapchain,
            semaphores,
        });
        let context = VulkanDeviceContext::from_host(
            host.device.clone(),
            host.instance.handle(),
            physical,
            queue,
            family,
            host.entry.static_fn().get_instance_proc_addr,
            host.clone(),
        );
        (host, context, images, extent, format.format)
    };
    let mut backend = VulkanGpuBackend::new(context).expect("Vulkan frame backend");
    // SAFETY: the shader is the build-time Slang reference and the device supports compute.
    unsafe { backend.configure_color_filter(&shader) }.expect("reference filter");
    let format_input = VesperVideoFormat {
        pixel_format: VESPER_PIXEL_NV12,
        width: size.width,
        height: size.height,
        color: VesperColorInfo {
            primaries: 1,
            transfer: 1,
            matrix: 1,
            range: 1,
            bit_depth: 8,
            ..Default::default()
        },
        ..Default::default()
    };
    let stride = size.width as usize;
    let pixel_count = stride * size.height as usize;
    let mut luma = vec![195; pixel_count];
    let mut chroma = vec![128; pixel_count / 2];
    // Optional input is 60 decoded 1920x1080 NV12 frames, without headers.
    let mut fixture = std::env::var_os("VESPER_VULKAN_XLIB_NV12_FIXTURE")
        .map(|path| std::fs::File::open(path).expect("open decoded NV12 fixture"));
    let (frame_count, frames_per_epoch) = if fixture.is_some() { (60, 20) } else { (6, 2) };
    let (send, receive) = mpsc::channel();
    let started = std::time::Instant::now();
    for frame_index in 0..frame_count {
        if let Some(fixture) = &mut fixture {
            fixture.read_exact(&mut luma).expect("read NV12 luma");
            fixture.read_exact(&mut chroma).expect("read NV12 chroma");
        }
        let epoch = frame_index / frames_per_epoch + 1;
        if frame_index % frames_per_epoch == 0 {
            backend.begin_epoch(epoch).expect("presentation epoch");
        }
        let acquired = host.semaphores[0];
        // SAFETY: the prior frame has drained its acquire wait. The per-image
        // presentation semaphore is reused only after reacquiring that image.
        let status = unsafe {
            let (index, _) = host
                .swapchain_api
                .acquire_next_image(host.swapchain, 2_000_000_000, acquired, vk::Fence::null())
                .expect("acquire swapchain image");
            backend
                .submit_color_to_swapchain(
                    epoch,
                    Lease(host.clone(), send.clone()),
                    VulkanColorFrame {
                        format: format_input,
                        gain: 0.5,
                        input: VulkanColorInput::Staged([
                            VulkanUploadPlane {
                                bytes: &luma,
                                row_stride: stride,
                            },
                            VulkanUploadPlane {
                                bytes: &chroma,
                                row_stride: stride,
                            },
                        ]),
                    },
                    VulkanSwapchainFrame {
                        loader: &host.swapchain_api,
                        swapchain: host.swapchain,
                        image_index: index,
                        image: images[index as usize],
                        extent,
                        format,
                        acquired,
                        presentation_ready: host.semaphores[1 + index as usize],
                    },
                )
                .expect("submit the filter and present")
        };
        assert!(matches!(
            status,
            VulkanPresentStatus::Presented { .. } | VulkanPresentStatus::Suboptimal { .. }
        ));
        backend
            .drain(Duration::from_secs(2))
            .expect("retire filtered frame");
        assert_eq!(receive.recv_timeout(Duration::from_secs(1)), Ok(true));
    }
    eprintln!(
        "X11 Vulkan presented and retired {frame_count} filtered 1920x1080 frames in {:?}",
        started.elapsed()
    );
}
