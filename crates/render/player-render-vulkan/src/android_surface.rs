//! Owns a clear Android Surface, its swapchain, and a bounded presentation flight.

use std::ffi::{CStr, c_void};
use std::sync::Arc;

use ash::{Device, Entry, Instance, android, ext, khr, vk};
use player_plugin_abi::VesperVideoFormat;

use crate::compat::has_extensions;
use crate::swapchain::{blit_corner, encode_blit};
use crate::{
    AndroidVulkanImport, AndroidVulkanImportSupport, ImportedAndroidVideoBuffer, Permit, RETIRED,
    RETIRED_READY, RetiredQueue, VulkanColorFrame, VulkanColorInput, VulkanDeviceContext,
    VulkanDeviceFeatures, VulkanError, VulkanFrameLease, VulkanGpuBackend,
    ensure_retirement_worker,
};

const MAX_SWAPCHAIN_IMAGES: usize = 8;
const TRANSFER_DESTINATION: vk::ImageUsageFlags = vk::ImageUsageFlags::TRANSFER_DST;

#[link(name = "android")]
unsafe extern "C" {
    fn ANativeWindow_acquire(window: *mut vk::ANativeWindow);
    fn ANativeWindow_release(window: *mut vk::ANativeWindow);
    fn ANativeWindow_getWidth(window: *mut vk::ANativeWindow) -> i32;
    fn ANativeWindow_getHeight(window: *mut vk::ANativeWindow) -> i32;
}

struct InstanceOwner {
    entry: Entry,
    instance: Instance,
    api_version: u32,
    surface_maintenance1: bool,
}

impl InstanceOwner {
    fn open() -> Result<Self, VulkanError> {
        // SAFETY: Android provides the Vulkan loader as a system library.
        let entry = unsafe { Entry::load_from("libvulkan.so") }
            .map_err(|_| unsupported("system Vulkan loader is unavailable"))?;
        // SAFETY: the entry keeps the system loader live.
        let version =
            unsafe { entry.try_enumerate_instance_version() }?.unwrap_or(vk::API_VERSION_1_0);
        let api_version =
            crate::compat::select_instance_api_version(version).map_err(unsupported)?;
        let required_extensions = [khr::surface::NAME, khr::android_surface::NAME];
        // SAFETY: the entry keeps the system loader live.
        let available = unsafe { entry.enumerate_instance_extension_properties(None) }?;
        if !has_extensions(&available, &required_extensions) {
            return Err(unsupported(
                "Android Vulkan surface extensions are unavailable",
            ));
        }
        let surface_maintenance1 = has_extensions(
            &available,
            &[
                khr::get_surface_capabilities2::NAME,
                ext::surface_maintenance1::NAME,
            ],
        );
        let mut extension_names = Vec::with_capacity(4);
        for name in required_extensions {
            extension_names.push(name.as_ptr());
        }
        if surface_maintenance1 {
            extension_names.push(khr::get_surface_capabilities2::NAME.as_ptr());
            extension_names.push(ext::surface_maintenance1::NAME.as_ptr());
        }
        let app = vk::ApplicationInfo::default().api_version(api_version);
        let info = vk::InstanceCreateInfo::default()
            .application_info(&app)
            .enabled_extension_names(&extension_names);
        // SAFETY: the loader is live and the extension list is supported.
        let instance = unsafe { entry.create_instance(&info, None) }?;
        Ok(Self {
            entry,
            instance,
            api_version,
            surface_maintenance1,
        })
    }
}

impl Drop for InstanceOwner {
    fn drop(&mut self) {
        // SAFETY: all child objects have been destroyed before the instance.
        unsafe { self.instance.destroy_instance(None) };
    }
}

struct SurfaceOwner {
    vulkan: InstanceOwner,
    loader: khr::surface::Instance,
    surface: vk::SurfaceKHR,
    window: usize,
}

impl SurfaceOwner {
    unsafe fn open(window: *mut vk::ANativeWindow) -> Result<Arc<Self>, VulkanError> {
        if window.is_null() {
            return Err(unsupported("missing native window"));
        }
        let vulkan = InstanceOwner::open()?;
        let loader = khr::surface::Instance::new(&vulkan.entry, &vulkan.instance);
        let android_surface = khr::android_surface::Instance::new(&vulkan.entry, &vulkan.instance);
        // SAFETY: the caller has a live window; this owner takes an independent reference.
        unsafe { ANativeWindow_acquire(window) };
        let mut owner = Self {
            vulkan,
            loader,
            surface: vk::SurfaceKHR::null(),
            window: window as usize,
        };
        // SAFETY: the owner retains both the native window and its Vulkan instance.
        owner.surface = unsafe {
            android_surface.create_android_surface(
                &vk::AndroidSurfaceCreateInfoKHR::default().window(window),
                None,
            )
        }?;
        Ok(Arc::new(owner))
    }
}

impl Drop for SurfaceOwner {
    fn drop(&mut self) {
        // SAFETY: the last dependent device and swapchain have retired.
        unsafe {
            self.loader.destroy_surface(self.surface, None);
            ANativeWindow_release(self.window as *mut vk::ANativeWindow);
        }
    }
}

struct DeviceOwner {
    device: Device,
    _surface: Arc<SurfaceOwner>,
}

impl Drop for DeviceOwner {
    fn drop(&mut self) {
        // SAFETY: the Vulkan context is retained by every live submission and swapchain.
        unsafe { self.device.destroy_device(None) };
    }
}

fn unsupported(reason: &'static str) -> VulkanError {
    VulkanError::UnsupportedAndroidSurface(reason)
}

const REQUIRED_DEVICE_EXTENSIONS: [&CStr; 2] = [
    android::external_memory_android_hardware_buffer::NAME,
    khr::swapchain::NAME,
];

struct SelectedDevice {
    physical: vk::PhysicalDevice,
    family: u32,
    timeline_semaphores: bool,
    foreign_queue_ownership: bool,
    swapchain_maintenance1: bool,
}

/// Checks device-wide presenter requirements before opening a decoder. Window
/// presentation support and swapchain format are checked again on attachment.
pub fn probe_android_vulkan_support() -> Result<(), VulkanError> {
    let vulkan = InstanceOwner::open()?;
    select_device(
        &vulkan.instance,
        vulkan.api_version,
        vulkan.surface_maintenance1,
        |_, _| Ok(true),
    )
    .map(|_| ())
}

fn select_device(
    instance: &Instance,
    instance_api_version: u32,
    surface_maintenance1: bool,
    mut supports_window: impl FnMut(vk::PhysicalDevice, u32) -> Result<bool, VulkanError>,
) -> Result<SelectedDevice, VulkanError> {
    // SAFETY: the instance owns each physical device returned by the enumeration.
    let physical_devices = unsafe { instance.enumerate_physical_devices() }?;
    let mut rejection = unsupported("no Vulkan physical device is available");
    for physical in physical_devices {
        // SAFETY: each physical device belongs to this live Vulkan 1.1+ instance.
        let support = unsafe { AndroidVulkanImportSupport::query(instance, physical) }?;
        if let Err(error) = support.require() {
            rejection = error;
            continue;
        }
        // SAFETY: physical belongs to this instance.
        let extensions = unsafe { instance.enumerate_device_extension_properties(physical) }?;
        if !has_extensions(&extensions, &REQUIRED_DEVICE_EXTENSIONS) {
            rejection =
                unsupported("Android hardware buffer or swapchain extensions are unavailable");
            continue;
        }
        let timeline_semaphores = instance_api_version >= vk::API_VERSION_1_2
            && support.vulkan_1_2
            && support.timeline_semaphore;
        let swapchain_maintenance1 = if surface_maintenance1
            && has_extensions(&extensions, &[ext::swapchain_maintenance1::NAME])
        {
            let mut present_fences = vk::PhysicalDeviceSwapchainMaintenance1FeaturesEXT::default();
            // SAFETY: physical advertises this extension and the feature chain is writable.
            unsafe {
                instance.get_physical_device_features2(
                    physical,
                    &mut vk::PhysicalDeviceFeatures2::default().push_next(&mut present_fences),
                )
            };
            present_fences.swapchain_maintenance1 == vk::TRUE
        } else {
            false
        };
        rejection = unsupported("no graphics and compute queue can present to the Surface");
        // SAFETY: physical belongs to this instance.
        let families = unsafe { instance.get_physical_device_queue_family_properties(physical) };
        for (index, family) in families.iter().enumerate() {
            let index = index as u32;
            if family.queue_count != 0
                && family
                    .queue_flags
                    .contains(vk::QueueFlags::COMPUTE | vk::QueueFlags::GRAPHICS)
                && supports_window(physical, index)?
            {
                return Ok(SelectedDevice {
                    physical,
                    family: index,
                    timeline_semaphores,
                    foreign_queue_ownership: support.foreign_queue_ownership,
                    swapchain_maintenance1,
                });
            }
        }
    }
    Err(rejection)
}

fn open_device(surface: &Arc<SurfaceOwner>) -> Result<(VulkanDeviceContext, bool), VulkanError> {
    let instance = &surface.vulkan.instance;
    let selected = select_device(
        instance,
        surface.vulkan.api_version,
        surface.vulkan.surface_maintenance1,
        |physical, index| {
            // SAFETY: the physical device, family index and surface belong to the same instance.
            Ok(unsafe {
                surface
                    .loader
                    .get_physical_device_surface_support(physical, index, surface.surface)
            }?)
        },
    )?;
    let queues = [vk::DeviceQueueCreateInfo::default()
        .queue_family_index(selected.family)
        .queue_priorities(&[1.0])];
    let mut extension_names = Vec::with_capacity(4);
    for name in REQUIRED_DEVICE_EXTENSIONS {
        extension_names.push(name.as_ptr());
    }
    if selected.foreign_queue_ownership {
        extension_names.push(ext::queue_family_foreign::NAME.as_ptr());
    }
    if selected.swapchain_maintenance1 {
        extension_names.push(ext::swapchain_maintenance1::NAME.as_ptr());
    }
    let mut present_fences =
        vk::PhysicalDeviceSwapchainMaintenance1FeaturesEXT::default().swapchain_maintenance1(true);
    let mut timeline =
        vk::PhysicalDeviceTimelineSemaphoreFeatures::default().timeline_semaphore(true);
    let mut ycbcr =
        vk::PhysicalDeviceSamplerYcbcrConversionFeatures::default().sampler_ycbcr_conversion(true);
    let mut info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queues)
        .enabled_extension_names(&extension_names)
        .push_next(&mut ycbcr);
    if selected.timeline_semaphores {
        info = info.push_next(&mut timeline);
    }
    if selected.swapchain_maintenance1 {
        info = info.push_next(&mut present_fences);
    }
    // SAFETY: one queue in the supported family and the queried features are enabled.
    let device = unsafe { instance.create_device(selected.physical, &info, None) }?;
    // SAFETY: the device was created with one queue from selected.family.
    let queue = unsafe { device.get_device_queue(selected.family, 0) };
    let owner = Arc::new(DeviceOwner {
        device: device.clone(),
        _surface: Arc::clone(surface),
    });
    let features = VulkanDeviceFeatures {
        timeline_semaphores: selected.timeline_semaphores,
        foreign_queue_ownership: selected.foreign_queue_ownership,
    };
    // SAFETY: owner retains the loader and both Vulkan parents. The presenter owns queue submission
    // and checked each optional feature flag in `features` above.
    let context = unsafe {
        VulkanDeviceContext::from_host_with_features(
            device,
            instance.handle(),
            selected.physical,
            queue,
            selected.family,
            surface.vulkan.entry.static_fn().get_instance_proc_addr,
            owner,
            features,
        )
    };
    Ok((context, selected.swapchain_maintenance1))
}

struct Swapchain {
    context: VulkanDeviceContext,
    loader: khr::swapchain::Device,
    handle: vk::SwapchainKHR,
    extent: vk::Extent2D,
    blit_filter: vk::Filter,
    present_fences: bool,
    images: Vec<vk::Image>,
    acquired: vk::Fence,
    presented: vk::Fence,
    ready: Vec<vk::Semaphore>,
    _permit: Permit,
}

impl Swapchain {
    fn open(
        surface: &SurfaceOwner,
        context: &VulkanDeviceContext,
        present_fences: bool,
    ) -> Result<Arc<Self>, VulkanError> {
        // SAFETY: surface and physical device are alive and share the same instance.
        let (caps, formats) = unsafe {
            (
                surface.loader.get_physical_device_surface_capabilities(
                    context.physical_device,
                    surface.surface,
                )?,
                surface.loader.get_physical_device_surface_formats(
                    context.physical_device,
                    surface.surface,
                )?,
            )
        };
        if !caps.supported_usage_flags.contains(TRANSFER_DESTINATION) {
            return Err(unsupported("the Surface does not accept direct blits"));
        }
        // SAFETY: physical belongs to the same live instance as this Surface.
        let source_features = unsafe {
            surface
                .vulkan
                .instance
                .get_physical_device_format_properties(
                    context.physical_device,
                    vk::Format::R16G16B16A16_SFLOAT,
                )
        }
        .optimal_tiling_features;
        if !source_features
            .contains(vk::FormatFeatureFlags::BLIT_SRC | vk::FormatFeatureFlags::STORAGE_IMAGE)
        {
            return Err(unsupported(
                "the driver cannot blit the linear filter output",
            ));
        }
        let format = formats
            .into_iter()
            .find(|format| {
                if format.color_space != vk::ColorSpaceKHR::SRGB_NONLINEAR
                    || !matches!(
                        format.format,
                        vk::Format::R8G8B8A8_SRGB | vk::Format::B8G8R8A8_SRGB
                    )
                {
                    return false;
                }
                // SAFETY: context's physical device and supported format come from this instance.
                unsafe {
                    surface
                        .vulkan
                        .instance
                        .get_physical_device_format_properties(
                            context.physical_device,
                            format.format,
                        )
                }
                .optimal_tiling_features
                .contains(vk::FormatFeatureFlags::BLIT_DST)
            })
            .ok_or_else(|| unsupported("the Surface offers no blit-compatible sRGB format"))?;
        let extent = if caps.current_extent.width != u32::MAX {
            caps.current_extent
        } else {
            // SAFETY: SurfaceOwner retains the ANativeWindow through both queries.
            let (width, height) = unsafe {
                let window = surface.window as *mut vk::ANativeWindow;
                (
                    ANativeWindow_getWidth(window),
                    ANativeWindow_getHeight(window),
                )
            };
            if width <= 0 || height <= 0 {
                return Err(unsupported("the Surface has no visible extent"));
            }
            vk::Extent2D {
                width: (width as u32)
                    .clamp(caps.min_image_extent.width, caps.max_image_extent.width),
                height: (height as u32)
                    .clamp(caps.min_image_extent.height, caps.max_image_extent.height),
            }
        };
        let _ = blit_corner(extent)?;
        let count = caps
            .min_image_count
            .saturating_add(1)
            .min(if caps.max_image_count == 0 {
                MAX_SWAPCHAIN_IMAGES as u32
            } else {
                caps.max_image_count.min(MAX_SWAPCHAIN_IMAGES as u32)
            });
        if count < caps.min_image_count {
            return Err(unsupported("the Surface exceeds the swapchain image limit"));
        }
        let alpha = [
            vk::CompositeAlphaFlagsKHR::OPAQUE,
            vk::CompositeAlphaFlagsKHR::INHERIT,
        ]
        .into_iter()
        .find(|mode| caps.supported_composite_alpha.contains(*mode))
        .ok_or_else(|| unsupported("the Surface offers no opaque presentation mode"))?;
        let device = context.device();
        let permit = Permit::acquire()?;
        let mut swapchain = Self {
            context: context.clone(),
            loader: khr::swapchain::Device::new(&surface.vulkan.instance, device),
            handle: vk::SwapchainKHR::null(),
            extent,
            blit_filter: if source_features
                .contains(vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR)
            {
                vk::Filter::LINEAR
            } else {
                vk::Filter::NEAREST
            },
            present_fences,
            images: Vec::new(),
            acquired: vk::Fence::null(),
            presented: vk::Fence::null(),
            ready: Vec::new(),
            _permit: permit,
        };
        let info = vk::SwapchainCreateInfoKHR::default()
            .surface(surface.surface)
            .min_image_count(count)
            .image_format(format.format)
            .image_color_space(format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(TRANSFER_DESTINATION)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(caps.current_transform)
            .composite_alpha(alpha)
            .present_mode(vk::PresentModeKHR::FIFO)
            .clipped(true);
        // SAFETY: the live device supports presentation and all Surface-selected fields.
        unsafe {
            swapchain.handle = swapchain.loader.create_swapchain(&info, None)?;
            swapchain.images = swapchain.loader.get_swapchain_images(swapchain.handle)?;
            if swapchain.images.len() > MAX_SWAPCHAIN_IMAGES {
                return Err(unsupported(
                    "the allocated swapchain exceeds the image limit",
                ));
            }
            swapchain.acquired = device.create_fence(&vk::FenceCreateInfo::default(), None)?;
            if present_fences {
                swapchain.presented = device.create_fence(&vk::FenceCreateInfo::default(), None)?;
            }
            swapchain.ready.reserve_exact(swapchain.images.len());
            for _ in 0..swapchain.images.len() {
                swapchain
                    .ready
                    .push(device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None)?);
            }
        }
        Ok(Arc::new(swapchain))
    }

    fn fence_status(&self, fence: vk::Fence) -> Result<bool, vk::Result> {
        // SAFETY: this owner keeps the fence live. This query does not wait.
        unsafe { self.context.device().get_fence_status(fence) }
    }
}

impl Drop for Swapchain {
    fn drop(&mut self) {
        // SAFETY: both flights have retired and all GPU slots dropped their reference.
        unsafe {
            for semaphore in self.ready.drain(..) {
                self.context.device().destroy_semaphore(semaphore, None);
            }
            self.context.device().destroy_fence(self.presented, None);
            self.context.device().destroy_fence(self.acquired, None);
            self.loader.destroy_swapchain(self.handle, None);
        }
    }
}

#[derive(Clone, Copy)]
enum Flight {
    Idle,
    Acquiring(u32),
    Presenting,
}

enum RetiringSwapchain {
    Fence {
        swapchain: Arc<Swapchain>,
        fence: vk::Fence,
        wait_idle_after: bool,
    },
    WaitIdle {
        swapchain: Arc<Swapchain>,
    },
}

impl RetiredQueue for RetiringSwapchain {
    fn poll(&mut self) -> bool {
        match self {
            Self::Fence {
                swapchain,
                fence,
                wait_idle_after,
            } => match swapchain.fence_status(*fence) {
                Ok(false) => false,
                Err(vk::Result::ERROR_DEVICE_LOST) => true,
                Ok(true) | Err(_) => {
                    if *wait_idle_after {
                        *self = Self::WaitIdle {
                            swapchain: Arc::clone(swapchain),
                        };
                        self.poll()
                    } else {
                        true
                    }
                }
            },
            Self::WaitIdle { swapchain } => {
                if Arc::strong_count(swapchain) > 1 {
                    return false;
                }
                // SAFETY: the presenter and every GPU queue submission retaining `swapchain`
                // have already dropped, so no concurrent queue or present call can run.
                let _ = unsafe { swapchain.context.device().device_wait_idle() };
                true
            }
        }
    }
}

struct PresentWork<T> {
    frame: T,
    import: Arc<ImportedAndroidVideoBuffer>,
    _swapchain: Arc<Swapchain>,
}

impl<T: VulkanFrameLease> VulkanFrameLease for PresentWork<T> {
    fn complete(self, accepted: bool) {
        drop(self.import);
        self.frame.complete(accepted);
    }
}

/// Presents clear Android decoder buffers through the reference Slang filter.
/// The caller serializes Surface lifecycle and playback calls on its runtime thread.
pub struct AndroidVulkanPresenter<T: VulkanFrameLease> {
    backend: VulkanGpuBackend<PresentWork<T>>,
    importer: AndroidVulkanImport,
    swapchain: Arc<Swapchain>,
    flight: Flight,
    has_presented: bool,
    failed: bool,
}

impl<T: VulkanFrameLease> AndroidVulkanPresenter<T> {
    /// # Safety
    /// `window` is a live ANativeWindow with no competing producer. The window
    /// is retained for retirement. `shader` is the trusted combined-420 SPIR-V.
    pub unsafe fn open(window: *mut c_void, shader: Box<[u32]>) -> Result<Self, VulkanError> {
        ensure_retirement_worker()?;
        // SAFETY: the caller supplies a live native window with no competing producer.
        let surface = unsafe { SurfaceOwner::open(window.cast()) }?;
        let (context, present_fences) = open_device(&surface)?;
        let swapchain = Swapchain::open(&surface, &context, present_fences)?;
        // SAFETY: device creation enabled the Android import extensions and YCbCr conversion.
        let importer = unsafe { AndroidVulkanImport::new(context.clone()) }?;
        let mut backend = VulkanGpuBackend::new(context)?;
        // SAFETY: the caller provided the built-in combined-420 shader.
        unsafe { backend.configure_android_color_filter(shader) }?;
        Ok(Self {
            backend,
            importer,
            swapchain,
            flight: Flight::Idle,
            has_presented: false,
            failed: false,
        })
    }

    /// Polls the existing flight and acquires an image with a zero timeout.
    pub fn prepare(&mut self) -> Result<bool, VulkanError> {
        if self.failed {
            return Err(unsupported("the Surface requires reattachment"));
        }
        self.backend.poll()?;
        let swapchain = &self.swapchain;
        let device = swapchain.context.device();
        if swapchain.present_fences && matches!(self.flight, Flight::Presenting) {
            if !swapchain.fence_status(swapchain.presented)? {
                return Ok(false);
            }
            // SAFETY: the previous present operation released its fence and semaphore payloads.
            unsafe { device.reset_fences(&[swapchain.presented]) }?;
            self.flight = Flight::Idle;
        }
        if matches!(self.flight, Flight::Idle) {
            // SAFETY: acquire fence is unsignaled and not associated with another operation.
            match unsafe {
                swapchain.loader.acquire_next_image(
                    swapchain.handle,
                    0,
                    vk::Semaphore::null(),
                    swapchain.acquired,
                )
            } {
                Ok((index, _)) => self.flight = Flight::Acquiring(index),
                Err(vk::Result::NOT_READY | vk::Result::TIMEOUT) => return Ok(false),
                Err(error) => return Err(error.into()),
            }
        }
        Ok(swapchain.fence_status(swapchain.acquired)?)
    }

    pub fn begin_epoch(&mut self, epoch: u64) -> Result<(), VulkanError> {
        self.backend.begin_epoch(epoch)
    }

    /// # Safety
    /// `buffer` is a clear YUV 420 AHardwareBuffer obtained from `frame`, which
    /// retains the producer lease. The producer acquire fence has signaled.
    pub unsafe fn present(
        &mut self,
        epoch: u64,
        frame: T,
        buffer: *const c_void,
        format: VesperVideoFormat,
        gain: f32,
    ) -> Result<bool, VulkanError> {
        if !self.prepare()? {
            frame.complete(false);
            return Ok(false);
        }
        let Flight::Acquiring(index) = self.flight else {
            return Err(VulkanError::UnsupportedSwapchainTarget);
        };
        // SAFETY: caller retains the live clear decoder buffer through the frame lease.
        let import = Arc::new(unsafe { self.importer.import_420(buffer.cast()) }?);
        let swapchain = &self.swapchain;
        let target = *swapchain
            .images
            .get(index as usize)
            .ok_or(VulkanError::UnsupportedSwapchainTarget)?;
        let ready = *swapchain
            .ready
            .get(index as usize)
            .ok_or(VulkanError::UnsupportedSwapchainTarget)?;
        let color = VulkanColorFrame {
            format,
            gain,
            input: VulkanColorInput::Android(&import),
        };
        let source_size = blit_corner(color.extent()?)?;
        let target_size = blit_corner(swapchain.extent)?;
        let work = PresentWork {
            frame,
            import: Arc::clone(&import),
            _swapchain: Arc::clone(swapchain),
        };
        // SAFETY: the acquire fence completed. The slot retains the buffer lease and the swapchain.
        unsafe {
            self.backend.submit_color_filtered(
                epoch,
                work,
                None,
                Some(ready),
                color,
                |context, command, _, output| {
                    encode_blit(
                        context,
                        command,
                        output,
                        target,
                        source_size,
                        target_size,
                        swapchain.blit_filter,
                    );
                    Ok(())
                },
            )
        }?;
        // SAFETY: acquisition completed and this fence can be reused after the present flight.
        // Keep the fence state intact until queue_present has registered the next flight.
        let fences = [swapchain.presented];
        let mut fence = vk::SwapchainPresentFenceInfoEXT::default().fences(&fences);
        let indices = [index];
        let handles = [swapchain.handle];
        let waits = [ready];
        let mut present = vk::PresentInfoKHR::default()
            .image_indices(&indices)
            .swapchains(&handles)
            .wait_semaphores(&waits);
        if swapchain.present_fences {
            present = present.push_next(&mut fence);
        }
        // SAFETY: the graphics command signals the wait semaphore for this acquired image.
        let result = unsafe {
            swapchain
                .loader
                .queue_present(swapchain.context.queue, &present)
        };
        match result {
            Ok(_) | Err(vk::Result::ERROR_OUT_OF_DATE_KHR | vk::Result::ERROR_SURFACE_LOST_KHR) => {
                self.has_presented = true;
                self.flight = if swapchain.present_fences {
                    Flight::Presenting
                } else {
                    Flight::Idle
                };
                // SAFETY: the acquire fence already signaled and is no longer used by WSI.
                if let Err(error) = unsafe {
                    swapchain
                        .context
                        .device()
                        .reset_fences(&[swapchain.acquired])
                } {
                    self.failed = true;
                    return Err(error.into());
                }
            }
            Err(_) => {
                // No present was enqueued. The GPU slot retains the swapchain until its signal retires.
            }
        }
        if result.is_err() {
            self.failed = true;
        }
        Ok(result.map(|_| true)?)
    }
}

impl<T: VulkanFrameLease> Drop for AndroidVulkanPresenter<T> {
    fn drop(&mut self) {
        let wait_idle_after = !self.swapchain.present_fences && self.has_presented;
        let retirement = match self.flight {
            Flight::Idle => {
                if !wait_idle_after {
                    return;
                }
                RetiringSwapchain::WaitIdle {
                    swapchain: Arc::clone(&self.swapchain),
                }
            }
            Flight::Acquiring(_) => {
                let mut retirement = RetiringSwapchain::Fence {
                    swapchain: Arc::clone(&self.swapchain),
                    fence: self.swapchain.acquired,
                    wait_idle_after,
                };
                if !wait_idle_after && retirement.poll() {
                    return;
                }
                retirement
            }
            Flight::Presenting => {
                let mut retirement = RetiringSwapchain::Fence {
                    swapchain: Arc::clone(&self.swapchain),
                    fence: self.swapchain.presented,
                    wait_idle_after: false,
                };
                if retirement.poll() {
                    return;
                }
                retirement
            }
        };
        RETIRED
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(Box::new(retirement));
        RETIRED_READY.notify_one();
    }
}
