use std::ffi::CStr;
use std::sync::Arc;
use std::time::Duration;

use ash::{Entry, Instance, khr, vk};

use crate::compat::{has_extensions, select_instance_api_version};
use crate::{
    Permit, RETIRED, RETIRED_READY, RetiredQueue, VulkanColorFrame, VulkanDeviceContext,
    VulkanDeviceFeatures, VulkanError, VulkanFrameLease, VulkanGpuBackend, VulkanPresentStatus,
    swapchain::{blit_corner, encode_blit},
};

const FORMAT_FEATURES: vk::FormatFeatureFlags = vk::FormatFeatureFlags::BLIT_DST;

struct Surface {
    entry: Entry,
    instance: Instance,
    api: khr::surface::Instance,
    handle: vk::SurfaceKHR,
    _window_owner: Arc<dyn Send + Sync>,
}

impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: the surface and its window outlive all dependent devices and swapchains.
        unsafe {
            if self.handle != vk::SurfaceKHR::null() {
                self.api.destroy_surface(self.handle, None);
            }
            self.instance.destroy_instance(None);
        }
    }
}

struct Device {
    device: ash::Device,
    _surface: Arc<Surface>,
}

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: every resource retains the device through VulkanDeviceContext.
        unsafe { self.device.destroy_device(None) };
    }
}

struct Swapchain {
    context: VulkanDeviceContext,
    api: khr::swapchain::Device,
    handle: vk::SwapchainKHR,
    extent: vk::Extent2D,
    present_fences: bool,
    images: Vec<vk::Image>,
    acquired: vk::Fence,
    presented: vk::Fence,
    ready: Vec<vk::Semaphore>,
    _permit: Permit,
}

impl Drop for Swapchain {
    fn drop(&mut self) {
        // SAFETY: construction failure has no submissions. The presentation owner otherwise
        // defers its last reference until WSI and the graphics queue have retired.
        unsafe {
            let device = self.context.device();
            for semaphore in self.ready.drain(..) {
                device.destroy_semaphore(semaphore, None);
            }
            device.destroy_fence(self.acquired, None);
            device.destroy_fence(self.presented, None);
            self.api.destroy_swapchain(self.handle, None);
        }
    }
}

struct PresentWork<T> {
    frame: T,
    _swapchain: Arc<Swapchain>,
}

impl<T: VulkanFrameLease> VulkanFrameLease for PresentWork<T> {
    fn complete(self, accepted: bool) {
        self.frame.complete(accepted);
    }
}

/// Desktop video filtering and WSI on one Vulkan queue.
pub struct DesktopVulkanPresenter<T: VulkanFrameLease> {
    backend: VulkanGpuBackend<PresentWork<T>>,
    swapchain: Arc<Swapchain>,
    flight: Flight,
    has_presented: bool,
    failed: bool,
}

impl<T: VulkanFrameLease> DesktopVulkanPresenter<T> {
    #[cfg(target_os = "linux")]
    /// # Safety
    /// The thread-safe owner keeps the Xlib display and mapped window alive. The
    /// caller uses this presenter exclusively for drawing to the window until
    /// it is destroyed. `shader` is the compiled Vesper separate-plane filter.
    pub unsafe fn open(
        display: *mut vk::Display,
        window: vk::Window,
        window_owner: Arc<dyn Send + Sync>,
        shader: &[u32],
    ) -> Result<Self, VulkanError> {
        if display.is_null() || window == 0 {
            return Err(unsupported("missing Xlib window or display"));
        }
        Self::open_surface(
            khr::xlib_surface::NAME,
            window_owner,
            shader,
            None,
            |entry, instance| {
                let api = khr::xlib_surface::Instance::new(entry, instance);
                // SAFETY: the supplied owner retains the Xlib display and window.
                unsafe {
                    api.create_xlib_surface(
                        &vk::XlibSurfaceCreateInfoKHR::default()
                            .dpy(display)
                            .window(window),
                        None,
                    )
                }
            },
        )
    }

    /// # Safety
    /// The host supplies a live HWND and its HINSTANCE and only this presenter
    /// draws to the HWND while active. The owner retains any host-side resources
    /// needed through presentation retirement.
    /// `luid` identifies the adapter used by the producer of imported textures.
    /// `shader` satisfies the separate-plane color-filter shader contract.
    #[cfg(target_os = "windows")]
    pub unsafe fn open_win32(
        hwnd: vk::HWND,
        hinstance: vk::HINSTANCE,
        luid: [u8; vk::LUID_SIZE],
        window_owner: Arc<dyn Send + Sync>,
        shader: &[u32],
    ) -> Result<Self, VulkanError> {
        if hwnd == 0 || hinstance == 0 {
            return Err(unsupported("missing Win32 HWND or HINSTANCE"));
        }
        Self::open_surface(
            khr::win32_surface::NAME,
            window_owner,
            shader,
            Some(luid),
            |entry, instance| {
                let api = khr::win32_surface::Instance::new(entry, instance);
                // SAFETY: the supplied owner retains the Win32 surface handles.
                unsafe {
                    api.create_win32_surface(
                        &vk::Win32SurfaceCreateInfoKHR::default()
                            .hwnd(hwnd)
                            .hinstance(hinstance),
                        None,
                    )
                }
            },
        )
    }

    fn open_surface(
        window_extension: &CStr,
        window_owner: Arc<dyn Send + Sync>,
        shader: &[u32],
        adapter_luid: Option<[u8; vk::LUID_SIZE]>,
        create_surface: impl FnOnce(&Entry, &Instance) -> Result<vk::SurfaceKHR, vk::Result>,
    ) -> Result<Self, VulkanError> {
        // SAFETY: Surface retains the system loader until the Vulkan instance is destroyed.
        let entry = unsafe { Entry::load() }.map_err(|_| unsupported("missing Vulkan loader"))?;
        // SAFETY: `entry` owns the live Vulkan loader.
        let version =
            unsafe { entry.try_enumerate_instance_version() }?.unwrap_or(vk::API_VERSION_1_0);
        let api_version = select_instance_api_version(version).map_err(unsupported)?;
        // SAFETY: `entry` owns the live Vulkan loader.
        let available = unsafe { entry.enumerate_instance_extension_properties(None) }?;
        let required_extensions = [khr::surface::NAME, window_extension];
        if !has_extensions(&available, &required_extensions) {
            return Err(unsupported(
                "Vulkan window surface extensions are unavailable",
            ));
        }
        let surface_maintenance1 = has_extensions(
            &available,
            &[
                khr::get_surface_capabilities2::NAME,
                ash::ext::surface_maintenance1::NAME,
            ],
        );
        let mut extensions = Vec::with_capacity(4);
        for name in required_extensions {
            extensions.push(name.as_ptr());
        }
        if surface_maintenance1 {
            extensions.push(khr::get_surface_capabilities2::NAME.as_ptr());
            extensions.push(ash::ext::surface_maintenance1::NAME.as_ptr());
        }
        let app = vk::ApplicationInfo::default().api_version(api_version);
        // SAFETY: the extension names and application info are live for instance creation.
        let instance = unsafe {
            entry.create_instance(
                &vk::InstanceCreateInfo::default()
                    .application_info(&app)
                    .enabled_extension_names(&extensions),
                None,
            )
        }?;
        let mut surface = Surface {
            api: khr::surface::Instance::new(&entry, &instance),
            entry,
            instance,
            handle: vk::SurfaceKHR::null(),
            _window_owner: window_owner,
        };
        surface.handle = create_surface(&surface.entry, &surface.instance)?;
        let surface = Arc::new(surface);
        let (context, present_fences) =
            open_device(&surface, api_version, surface_maintenance1, adapter_luid)?;
        let swapchain = Arc::new(open_swapchain(&surface, &context, present_fences)?);
        let mut backend = VulkanGpuBackend::new(context)?;
        // SAFETY: the shader contract and selected graphics+compute queue cover filter creation.
        unsafe { backend.configure_color_filter(shader) }?;
        Ok(Self {
            backend,
            swapchain,
            flight: Flight::Idle,
            has_presented: false,
            failed: false,
        })
    }

    pub fn device_context(&self) -> &VulkanDeviceContext {
        &self.swapchain.context
    }

    #[cfg(target_os = "linux")]
    pub fn supports_dma_buf(&self) -> bool {
        self.swapchain.context.dma_buf_enabled
    }

    pub fn begin_epoch(&mut self, epoch: u64) -> Result<(), VulkanError> {
        self.backend.begin_epoch(epoch)
    }

    pub fn drain(&mut self, timeout: Duration) -> Result<(), VulkanError> {
        self.backend.drain(timeout)
    }

    #[cfg(target_os = "windows")]
    pub fn drain_presentation(&mut self, timeout: Duration) -> Result<(), VulkanError> {
        self.backend.drain(timeout)?;
        let fence = match self.flight {
            Flight::Idle => {
                if !self.swapchain.present_fences && self.has_presented {
                    // SAFETY: backend submissions drained above and this presenter owns the queue.
                    unsafe { self.swapchain.context.device().device_wait_idle() }?;
                }
                return Ok(());
            }
            Flight::Acquiring(_) => self.swapchain.acquired,
            Flight::Presenting => self.swapchain.presented,
        };
        // SAFETY: the selected fence is live and only this presenter can reset it.
        unsafe {
            self.swapchain.context.device().wait_for_fences(
                &[fence],
                true,
                timeout.as_nanos().min(u64::MAX as u128) as u64,
            )
        }?;
        Ok(())
    }

    /// Submits an upload or import on the presenter's queue. A busy queue does
    /// not acquire another image. The caller may drop or retry the input.
    ///
    /// # Safety
    /// The caller satisfies the selected [`VulkanColorFrame`] input contract.
    pub unsafe fn present(
        &mut self,
        epoch: u64,
        lease: T,
        frame: VulkanColorFrame<'_>,
    ) -> Result<Option<VulkanPresentStatus>, VulkanError> {
        if self.failed {
            lease.complete(false);
            return Err(unsupported("presentation surface has been invalidated"));
        }
        let src = blit_corner(frame.extent()?)?;
        let swapchain = &self.swapchain;
        let dst = blit_corner(swapchain.extent)?;
        let device = swapchain.context.device();
        self.backend.poll()?;
        if swapchain.present_fences && matches!(self.flight, Flight::Presenting) {
            // SAFETY: the live fence was enqueued by the preceding presentation.
            if !unsafe { device.get_fence_status(swapchain.presented) }? {
                lease.complete(false);
                return Ok(None);
            }
            // SAFETY: the signaled present fence released the presentation resources.
            unsafe { device.reset_fences(&[swapchain.presented]) }?;
            self.flight = Flight::Idle;
        }
        if matches!(self.flight, Flight::Idle) {
            // SAFETY: this fence is unsignaled and has no pending acquire operation.
            match unsafe {
                swapchain.api.acquire_next_image(
                    swapchain.handle,
                    0,
                    vk::Semaphore::null(),
                    swapchain.acquired,
                )
            } {
                Ok((index, _)) => self.flight = Flight::Acquiring(index),
                Err(vk::Result::NOT_READY | vk::Result::TIMEOUT) => {
                    lease.complete(false);
                    return Ok(None);
                }
                Err(error) => {
                    self.failed = true;
                    lease.complete(false);
                    return Err(error.into());
                }
            }
        }
        let Flight::Acquiring(index) = self.flight else {
            lease.complete(false);
            return Ok(None);
        };
        // SAFETY: this fence is live and signaled by the pending image acquisition.
        if !unsafe { device.get_fence_status(swapchain.acquired) }? {
            lease.complete(false);
            return Ok(None);
        }
        let target = *swapchain
            .images
            .get(index as usize)
            .ok_or(VulkanError::UnsupportedSwapchainTarget)?;
        let ready = *swapchain
            .ready
            .get(index as usize)
            .ok_or(VulkanError::UnsupportedSwapchainTarget)?;
        // SAFETY: the acquire fence signaled, the frame owner satisfies the input contract,
        // and the submitted work retains the swapchain across GPU completion.
        let value = unsafe {
            self.backend.submit_color_filtered(
                epoch,
                PresentWork {
                    frame: lease,
                    _swapchain: Arc::clone(swapchain),
                },
                None,
                Some(ready),
                frame,
                |context, command, _, output| {
                    encode_blit(
                        context,
                        command,
                        output,
                        target,
                        src,
                        dst,
                        vk::Filter::LINEAR,
                    );
                    Ok(())
                },
            )
        }?;
        let fences = [swapchain.presented];
        let mut fence = vk::SwapchainPresentFenceInfoEXT::default().fences(&fences);
        let indices = [index];
        let handles = [swapchain.handle];
        let waits = [ready];
        let mut request = vk::PresentInfoKHR::default()
            .image_indices(&indices)
            .swapchains(&handles)
            .wait_semaphores(&waits);
        if swapchain.present_fences {
            request = request.push_next(&mut fence);
        }
        // SAFETY: the graphics command signals this wait semaphore for this acquired image.
        let result = unsafe {
            swapchain
                .api
                .queue_present(swapchain.context.queue, &request)
        };
        match result {
            Ok(_) | Err(vk::Result::ERROR_OUT_OF_DATE_KHR | vk::Result::ERROR_SURFACE_LOST_KHR) => {
                self.has_presented = true;
                self.flight = if swapchain.present_fences {
                    Flight::Presenting
                } else {
                    Flight::Idle
                };
                // SAFETY: the acquisition completed before submission and no longer uses the fence.
                if let Err(error) = unsafe { device.reset_fences(&[swapchain.acquired]) } {
                    self.failed = true;
                    return Err(error.into());
                }
            }
            Err(_) => {
                // No present was enqueued. The timeline retains the graphics submission.
            }
        }
        self.failed = result.is_err();
        match result {
            Ok(false) => Ok(Some(VulkanPresentStatus::Presented {
                timeline_value: value,
            })),
            Ok(true) => Ok(Some(VulkanPresentStatus::Suboptimal {
                timeline_value: value,
            })),
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => Ok(Some(VulkanPresentStatus::OutOfDate {
                timeline_value: value,
            })),
            Err(error) => Err(error.into()),
        }
    }
}

#[derive(Clone, Copy)]
enum Flight {
    Idle,
    Acquiring(u32),
    Presenting,
}

enum RetiredSwapchain {
    Fence {
        swapchain: Arc<Swapchain>,
        fence: vk::Fence,
        wait_idle_after: bool,
    },
    WaitIdle {
        swapchain: Arc<Swapchain>,
    },
}

impl RetiredQueue for RetiredSwapchain {
    fn poll(&mut self) -> bool {
        match self {
            Self::Fence {
                swapchain,
                fence,
                wait_idle_after,
            } => {
                // SAFETY: the owner retains the fence and never resets it after retirement.
                match unsafe { swapchain.context.device().get_fence_status(*fence) } {
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
                }
            }
            Self::WaitIdle { swapchain } => {
                if Arc::strong_count(swapchain) > 1 {
                    return false;
                }
                // SAFETY: the presenter and all PresentWork queue slots retaining `swapchain`
                // have dropped, so no concurrent queue or present call remains active.
                let _ = unsafe { swapchain.context.device().device_wait_idle() };
                true
            }
        }
    }
}

impl<T: VulkanFrameLease> Drop for DesktopVulkanPresenter<T> {
    fn drop(&mut self) {
        let wait_idle_after = !self.swapchain.present_fences && self.has_presented;
        let retirement = match self.flight {
            Flight::Idle => {
                if !wait_idle_after {
                    return;
                }
                RetiredSwapchain::WaitIdle {
                    swapchain: Arc::clone(&self.swapchain),
                }
            }
            Flight::Acquiring(_) => {
                let mut retirement = RetiredSwapchain::Fence {
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
                let mut retirement = RetiredSwapchain::Fence {
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

fn unsupported(reason: &'static str) -> VulkanError {
    VulkanError::UnsupportedDesktopSurface(reason)
}

fn open_device(
    surface: &Arc<Surface>,
    instance_api_version: u32,
    surface_maintenance1: bool,
    adapter_luid: Option<[u8; vk::LUID_SIZE]>,
) -> Result<(VulkanDeviceContext, bool), VulkanError> {
    let instance = &surface.instance;
    // SAFETY: surface and instance remain live through selection and device creation.
    unsafe {
        for physical in instance.enumerate_physical_devices()? {
            let physical_api_version = instance
                .get_physical_device_properties(physical)
                .api_version;
            if physical_api_version < vk::API_VERSION_1_1 {
                continue;
            }
            if let Some(expected) = adapter_luid {
                let mut identity = vk::PhysicalDeviceIDProperties::default();
                instance.get_physical_device_properties2(
                    physical,
                    &mut vk::PhysicalDeviceProperties2::default().push_next(&mut identity),
                );
                if identity.device_luid_valid != vk::TRUE || identity.device_luid != expected {
                    continue;
                }
            }
            let required = [
                khr::swapchain::NAME,
                #[cfg(target_os = "windows")]
                khr::external_memory_win32::NAME,
            ];
            let available = instance.enumerate_device_extension_properties(physical)?;
            if !has_extensions(&available, &required) {
                continue;
            }
            let physical_1_2 = physical_api_version >= vk::API_VERSION_1_2;
            let has_swapchain_maintenance = surface_maintenance1
                && has_extensions(&available, &[ash::ext::swapchain_maintenance1::NAME]);
            let mut timeline = vk::PhysicalDeviceTimelineSemaphoreFeatures::default();
            let mut maintenance = vk::PhysicalDeviceSwapchainMaintenance1FeaturesEXT::default();
            #[cfg(target_os = "windows")]
            let mut ycbcr = vk::PhysicalDeviceSamplerYcbcrConversionFeatures::default();
            if physical_1_2 || has_swapchain_maintenance || cfg!(target_os = "windows") {
                let mut features = vk::PhysicalDeviceFeatures2::default();
                if physical_1_2 {
                    features = features.push_next(&mut timeline);
                }
                if has_swapchain_maintenance {
                    features = features.push_next(&mut maintenance);
                }
                #[cfg(target_os = "windows")]
                {
                    features = features.push_next(&mut ycbcr);
                }
                instance.get_physical_device_features2(physical, &mut features);
            }
            #[cfg(target_os = "windows")]
            if ycbcr.sampler_ycbcr_conversion != vk::TRUE {
                continue;
            }
            let timeline_semaphores = instance_api_version >= vk::API_VERSION_1_2
                && physical_1_2
                && timeline.timeline_semaphore == vk::TRUE;
            let swapchain_maintenance1 =
                has_swapchain_maintenance && maintenance.swapchain_maintenance1 == vk::TRUE;
            for (index, props) in instance
                .get_physical_device_queue_family_properties(physical)
                .iter()
                .enumerate()
            {
                let family = index as u32;
                if props.queue_count == 0
                    || !props
                        .queue_flags
                        .contains(vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE)
                    || !surface.api.get_physical_device_surface_support(
                        physical,
                        family,
                        surface.handle,
                    )?
                {
                    continue;
                }
                let priorities = [1.0];
                let queues = [vk::DeviceQueueCreateInfo::default()
                    .queue_family_index(family)
                    .queue_priorities(&priorities)];
                let mut extensions: Vec<*const std::ffi::c_char> =
                    required.iter().map(|name| name.as_ptr()).collect();
                if swapchain_maintenance1 {
                    extensions.push(ash::ext::swapchain_maintenance1::NAME.as_ptr());
                }
                #[cfg(target_os = "linux")]
                let dma_buf_enabled = has_extensions(&available, &crate::dma_buf::EXTENSIONS);
                #[cfg(target_os = "linux")]
                if dma_buf_enabled {
                    for name in crate::dma_buf::EXTENSIONS {
                        extensions.push(name.as_ptr());
                    }
                }
                // Feature queries link their nodes in place. Device creation needs a fresh chain.
                let mut enable_timeline =
                    vk::PhysicalDeviceTimelineSemaphoreFeatures::default().timeline_semaphore(true);
                let mut enable_maintenance =
                    vk::PhysicalDeviceSwapchainMaintenance1FeaturesEXT::default()
                        .swapchain_maintenance1(true);
                #[cfg(target_os = "windows")]
                let mut enable_ycbcr = vk::PhysicalDeviceSamplerYcbcrConversionFeatures::default()
                    .sampler_ycbcr_conversion(true);
                let mut create_info = vk::DeviceCreateInfo::default()
                    .queue_create_infos(&queues)
                    .enabled_extension_names(&extensions);
                if timeline_semaphores {
                    create_info = create_info.push_next(&mut enable_timeline);
                }
                if swapchain_maintenance1 {
                    create_info = create_info.push_next(&mut enable_maintenance);
                }
                #[cfg(target_os = "windows")]
                {
                    create_info = create_info.push_next(&mut enable_ycbcr);
                }
                let device = instance.create_device(physical, &create_info, None)?;
                let queue = device.get_device_queue(family, 0);
                let owner = Arc::new(Device {
                    device: device.clone(),
                    _surface: Arc::clone(surface),
                });
                let features = VulkanDeviceFeatures {
                    timeline_semaphores,
                    foreign_queue_ownership: true,
                };
                let context = VulkanDeviceContext::from_host_with_features(
                    device,
                    instance.handle(),
                    physical,
                    queue,
                    family,
                    surface.entry.static_fn().get_instance_proc_addr,
                    owner,
                    features,
                );
                #[cfg(target_os = "linux")]
                let context = VulkanDeviceContext {
                    dma_buf_enabled,
                    ..context
                };
                return Ok((context, swapchain_maintenance1));
            }
        }
    }
    Err(unsupported(
        "no Vulkan 1.1+ graphics and compute presentation queue is available",
    ))
}

fn open_swapchain(
    surface: &Surface,
    context: &VulkanDeviceContext,
    present_fences: bool,
) -> Result<Swapchain, VulkanError> {
    let permit = Permit::acquire()?;
    let physical = context.physical_device;
    let device = context.device();
    let api = khr::swapchain::Device::new(&surface.instance, device);
    // SAFETY: the live device's queue family supports presentation to this surface.
    unsafe {
        let caps = surface
            .api
            .get_physical_device_surface_capabilities(physical, surface.handle)?;
        if !caps
            .supported_usage_flags
            .contains(vk::ImageUsageFlags::TRANSFER_DST)
            || !caps
                .supported_composite_alpha
                .contains(vk::CompositeAlphaFlagsKHR::OPAQUE)
            || caps.current_extent.width == 0
            || caps.current_extent.height == 0
        {
            return Err(unsupported("missing a visible opaque transfer destination"));
        }
        let format = surface
            .api
            .get_physical_device_surface_formats(physical, surface.handle)?
            .into_iter()
            .find(|format| {
                matches!(
                    format.format,
                    vk::Format::B8G8R8A8_SRGB | vk::Format::R8G8B8A8_SRGB
                ) && format.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
                    && surface
                        .instance
                        .get_physical_device_format_properties(physical, format.format)
                        .optimal_tiling_features
                        .contains(FORMAT_FEATURES)
            })
            .ok_or_else(|| unsupported("missing an sRGB blit destination"))?;
        let source_features = surface
            .instance
            .get_physical_device_format_properties(physical, vk::Format::R16G16B16A16_SFLOAT);
        if !source_features.optimal_tiling_features.contains(
            vk::FormatFeatureFlags::BLIT_SRC | vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR,
        ) {
            return Err(unsupported("the filter output cannot be linearly blitted"));
        }
        let handle = api.create_swapchain(
            &vk::SwapchainCreateInfoKHR::default()
                .surface(surface.handle)
                .min_image_count(caps.min_image_count)
                .image_format(format.format)
                .image_color_space(format.color_space)
                .image_extent(caps.current_extent)
                .image_array_layers(1)
                .image_usage(vk::ImageUsageFlags::TRANSFER_DST)
                .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
                .pre_transform(caps.current_transform)
                .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
                .present_mode(vk::PresentModeKHR::FIFO)
                .clipped(true),
            None,
        )?;
        let mut swapchain = Swapchain {
            context: context.clone(),
            api,
            handle,
            extent: caps.current_extent,
            present_fences,
            images: Vec::new(),
            acquired: vk::Fence::null(),
            presented: vk::Fence::null(),
            ready: Vec::new(),
            _permit: permit,
        };
        swapchain.images = swapchain.api.get_swapchain_images(handle)?;
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
        Ok(swapchain)
    }
}
