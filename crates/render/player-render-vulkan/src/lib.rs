//! Vulkan submission and retirement for native video filters.
#![warn(clippy::undocumented_unsafe_blocks)]

#[cfg(target_os = "android")]
mod android_buffer;
#[cfg(target_os = "android")]
mod android_surface;
mod color_filter;
mod compat;
#[cfg(target_os = "linux")]
mod dma_buf;
#[cfg(target_os = "linux")]
pub use dma_buf::{DmaBufVideoPlane, ImportedDmaBufVideoImage};
mod filter_images;
mod swapchain;
mod upload;
#[cfg(target_os = "windows")]
mod win32_memory;
#[cfg(target_os = "windows")]
pub use win32_memory::{ImportedWin32VideoImage, Win32TextureHandleKind};
#[cfg(any(target_os = "linux", target_os = "windows"))]
mod desktop_surface;

#[cfg(target_os = "android")]
pub use android_buffer::{
    AndroidVulkanImport, AndroidVulkanImportSupport, ImportedAndroidVideoBuffer,
};
#[cfg(target_os = "android")]
pub use android_surface::{AndroidVulkanPresenter, probe_android_vulkan_support};
pub use color_filter::{VulkanColorFrame, VulkanColorInput};
pub use upload::VulkanUploadPlane;

#[cfg(target_os = "linux")]
pub use desktop_surface::DesktopVulkanPresenter as XlibVulkanPresenter;
#[cfg(target_os = "windows")]
pub use desktop_surface::DesktopVulkanPresenter as Win32VulkanPresenter;
pub use filter_images::{VulkanFilterImage, VulkanFilterImages};
pub use swapchain::{VulkanPresentStatus, VulkanSwapchainFrame};

use ash::vk::Handle;
use ash::{Device, vk};
use player_plugin_abi::{
    VESPER_GPU_VULKAN, VesperGpuContext, VesperGpuContextData, VesperVulkanContext,
};
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

/// Reads the words of a SPIR-V artifact, including byte order and alignment.
pub fn read_shader_words(
    read: &mut (impl std::io::Read + std::io::Seek),
) -> std::io::Result<Box<[u32]>> {
    ash::util::read_spv(read).map(Vec::into_boxed_slice)
}

const FRAME_SLOTS: usize = 3;
// Every live or retired queue reserves a permit. A stuck GPU cannot create
// an unbounded backlog of retired imports and decoder leases.
const MAX_QUEUES: usize = 8;
static LIVE_QUEUES: AtomicUsize = AtomicUsize::new(0);
static RETIRED: Mutex<Vec<Box<dyn RetiredQueue>>> = Mutex::new(Vec::new());
static RETIRED_READY: Condvar = Condvar::new();
static RETIREMENT_WORKER: OnceLock<Result<(), std::io::Error>> = OnceLock::new();
const RETIREMENT_POLL_INTERVAL: Duration = Duration::from_millis(8);

#[derive(Debug, thiserror::Error)]
pub enum VulkanError {
    #[error("Vulkan returned {0:?}")]
    Vulkan(#[from] vk::Result),
    #[error("all Vulkan frame slots are in use")]
    Backpressure,
    #[error("Vulkan queue capacity exhausted")]
    Exhausted,
    #[error("could not start the Vulkan retirement worker: {0}")]
    RetirementWorker(String),
    #[error("Vulkan filter images require a nonzero extent")]
    InvalidFilterExtent,
    #[error("no device-local Vulkan memory type is available for filter images")]
    FilterImageMemoryUnavailable,
    #[error("the Vulkan presentation epoch is stale")]
    StaleEpoch,
    #[error("no Vulkan memory type meets the resource requirements")]
    MemoryTypeUnavailable,
    #[error("the reference Vulkan color filter has not been configured")]
    ColorFilterUnavailable,
    #[error("the reference Vulkan color filter requires uncropped NV12 or P010")]
    UnsupportedColorInput,
    #[cfg(target_os = "linux")]
    #[error("could not duplicate the decoded DMA-BUF descriptor: {0}")]
    DuplicateDmaBuf(#[source] std::io::Error),
    #[error("Vulkan upload plane is shorter than its negotiated dimensions and stride")]
    InvalidUploadPlane,
    #[error("the reference Vulkan presenter requires an acquired sRGB transfer destination")]
    UnsupportedSwapchainTarget,
    #[error("Android Vulkan video import is unsupported: {0}")]
    UnsupportedAndroidImport(&'static str),
    #[error("the Android Vulkan surface is unavailable: {0}")]
    UnsupportedAndroidSurface(&'static str),
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[error("the desktop Vulkan surface is unavailable: {0}")]
    UnsupportedDesktopSurface(&'static str),
}

/// Ownership transferred with command submission. Completion can happen after seek or drop.
pub trait VulkanFrameLease: Send + 'static {
    fn complete(self, accepted: bool);
}

/// Optional Vulkan device capabilities enabled by the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VulkanDeviceFeatures {
    pub timeline_semaphores: bool,
    pub foreign_queue_ownership: bool,
}

impl Default for VulkanDeviceFeatures {
    fn default() -> Self {
        Self {
            timeline_semaphores: true,
            foreign_queue_ownership: true,
        }
    }
}

#[derive(Clone)]
pub struct VulkanDeviceContext {
    device: Device,
    instance: vk::Instance,
    physical_device: vk::PhysicalDevice,
    queue: vk::Queue,
    family: u32,
    get_instance_proc_addr: vk::PFN_vkGetInstanceProcAddr,
    features: VulkanDeviceFeatures,
    #[cfg(target_os = "linux")]
    dma_buf_enabled: bool,
    _owner: Arc<dyn Send + Sync>,
}

impl VulkanDeviceContext {
    /// # Safety
    /// `owner` keeps the loader, instance, device and queue alive through all
    /// cloned contexts. The queue belongs to `family` and timeline semaphores
    /// were enabled through Vulkan 1.2. The host uses the queue exclusively
    /// through the backend or guarantees external synchronization.
    pub unsafe fn from_host(
        device: Device,
        instance: vk::Instance,
        physical_device: vk::PhysicalDevice,
        queue: vk::Queue,
        family: u32,
        get_instance_proc_addr: vk::PFN_vkGetInstanceProcAddr,
        owner: Arc<dyn Send + Sync>,
    ) -> Self {
        // SAFETY: the caller certifies Vulkan 1.2 timeline semaphores and queue ownership.
        unsafe {
            Self::from_host_with_features(
                device,
                instance,
                physical_device,
                queue,
                family,
                get_instance_proc_addr,
                owner,
                VulkanDeviceFeatures::default(),
            )
        }
    }

    /// # Safety
    /// `owner` keeps the loader, instance, device and queue alive through all
    /// cloned contexts. The queue belongs to `family`. When `features.timeline_semaphores`
    /// is true, Vulkan 1.2 timeline semaphores must be enabled on `device`; when false,
    /// per-slot fences are used and Vulkan 1.1+ is sufficient. When
    /// `features.foreign_queue_ownership` is true, `VK_EXT_queue_family_foreign` must
    /// be enabled on `device`; otherwise external queue barriers use `VK_QUEUE_FAMILY_EXTERNAL`.
    /// The host uses the queue exclusively through the backend or guarantees external
    /// synchronization.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn from_host_with_features(
        device: Device,
        instance: vk::Instance,
        physical_device: vk::PhysicalDevice,
        queue: vk::Queue,
        family: u32,
        get_instance_proc_addr: vk::PFN_vkGetInstanceProcAddr,
        owner: Arc<dyn Send + Sync>,
        features: VulkanDeviceFeatures,
    ) -> Self {
        Self {
            device,
            instance,
            physical_device,
            queue,
            family,
            get_instance_proc_addr,
            features,
            #[cfg(target_os = "linux")]
            dma_buf_enabled: false,
            _owner: owner,
        }
    }

    pub fn device(&self) -> &Device {
        &self.device
    }

    #[cfg(any(target_os = "android", target_os = "linux"))]
    fn instance_dispatch(&self) -> ash::Instance {
        let loader = ash::StaticFn {
            get_instance_proc_addr: self.get_instance_proc_addr,
        };
        // SAFETY: the context owner keeps the instance and its loader live.
        unsafe { ash::Instance::load(&loader, self.instance) }
    }

    fn physical_limits(&self) -> vk::PhysicalDeviceLimits {
        let loader = ash::StaticFn {
            get_instance_proc_addr: self.get_instance_proc_addr,
        };
        // SAFETY: the context owner keeps the physical device and its entry live.
        unsafe {
            ash::Instance::load(&loader, self.instance)
                .get_physical_device_properties(self.physical_device)
                .limits
        }
    }

    fn allocate_memory(
        &self,
        requirements: vk::MemoryRequirements,
        flags: vk::MemoryPropertyFlags,
    ) -> Result<vk::DeviceMemory, VulkanError> {
        let loader = ash::StaticFn {
            get_instance_proc_addr: self.get_instance_proc_addr,
        };
        // SAFETY: the context owner retains the loader, instance, physical device and logical device.
        let memory = unsafe {
            ash::Instance::load(&loader, self.instance)
                .get_physical_device_memory_properties(self.physical_device)
        };
        let index = (0..memory.memory_type_count)
            .find(|&index| {
                requirements.memory_type_bits & (1 << index) != 0
                    && memory.memory_types[index as usize]
                        .property_flags
                        .contains(flags)
            })
            .ok_or(VulkanError::MemoryTypeUnavailable)?;
        // SAFETY: this index was selected from the allocation's allowed types.
        Ok(unsafe {
            self.device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(requirements.size)
                    .memory_type_index(index),
                None,
            )
        }?)
    }

    fn gpu_context(&self, command_buffer: vk::CommandBuffer) -> VesperGpuContext {
        fn pointer<H: Handle>(handle: H) -> *mut c_void {
            std::ptr::with_exposed_provenance_mut(handle.as_raw() as usize)
        }
        VesperGpuContext {
            struct_size: size_of::<VesperGpuContext>() as u32,
            backend: VESPER_GPU_VULKAN,
            data: VesperGpuContextData {
                vulkan: VesperVulkanContext {
                    instance: pointer(self.instance),
                    physical_device: pointer(self.physical_device),
                    device: pointer(self.device.handle()),
                    queue: pointer(self.queue),
                    command_buffer: pointer(command_buffer),
                    get_instance_proc_addr: self.get_instance_proc_addr as *const c_void,
                    queue_family: self.family,
                },
            },
        }
    }
}

struct Permit;

impl Permit {
    fn acquire() -> Result<Self, VulkanError> {
        LIVE_QUEUES
            .try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_QUEUES).then_some(count + 1)
            })
            .map(|_| Self)
            .map_err(|_| VulkanError::Exhausted)
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        LIVE_QUEUES.fetch_sub(1, Ordering::Release);
    }
}

struct Pending<T> {
    ready_value: u64,
    epoch: u64,
    resource: T,
}

enum QueueSync {
    Timeline(vk::Semaphore),
    Fences([vk::Fence; FRAME_SLOTS]),
}

struct Queue<T: VulkanFrameLease> {
    context: VulkanDeviceContext,
    pool: vk::CommandPool,
    commands: [vk::CommandBuffer; FRAME_SLOTS],
    sync: QueueSync,
    slots: [Option<Pending<T>>; FRAME_SLOTS],
    filter_images: [Option<VulkanFilterImages>; FRAME_SLOTS],
    color_filter: Option<color_filter::ColorFilter>,
    last_submitted: u64,
    epoch: u64,
    _permit: Permit,
}

impl<T: VulkanFrameLease> Queue<T> {
    fn poll(&mut self) -> Result<(), VulkanError> {
        match self.sync {
            QueueSync::Timeline(timeline) => {
                // SAFETY: the timeline belongs to this live device and is queryable in flight.
                let value = unsafe { self.context.device.get_semaphore_counter_value(timeline) };
                match value {
                    Ok(value) => {
                        for slot in &mut self.slots {
                            if slot.as_ref().is_some_and(|work| work.ready_value <= value)
                                && let Some(work) = slot.take()
                            {
                                complete(work.resource, work.epoch == self.epoch);
                            }
                        }
                        Ok(())
                    }
                    Err(error) => {
                        if error == vk::Result::ERROR_DEVICE_LOST {
                            self.cancel_all();
                        }
                        Err(error.into())
                    }
                }
            }
            QueueSync::Fences(fences) => {
                for (index, fence) in fences.into_iter().enumerate() {
                    if self.slots[index].is_none() {
                        continue;
                    }
                    // SAFETY: the slot fence belongs to this live device and tracks one submission.
                    match unsafe { self.context.device.get_fence_status(fence) } {
                        Ok(false) => {}
                        Ok(true) => {
                            // SAFETY: the fence has signaled and this backend owns its reset cycle.
                            let reset = unsafe { self.context.device.reset_fences(&[fence]) };
                            if let Err(error) = reset {
                                if error == vk::Result::ERROR_DEVICE_LOST {
                                    self.cancel_all();
                                }
                                return Err(error.into());
                            }
                            if let Some(work) = self.slots[index].take() {
                                complete(work.resource, work.epoch == self.epoch);
                            }
                        }
                        Err(error) => {
                            if error == vk::Result::ERROR_DEVICE_LOST {
                                self.cancel_all();
                            }
                            return Err(error.into());
                        }
                    }
                }
                Ok(())
            }
        }
    }

    fn cancel_all(&mut self) {
        for slot in &mut self.slots {
            if let Some(work) = slot.take() {
                complete(work.resource, false);
            }
        }
    }

    fn is_empty(&self) -> bool {
        self.slots.iter().all(Option::is_none)
    }
}

fn complete<T: VulkanFrameLease>(resource: T, accepted: bool) {
    // A host callback may unwind. Never let it destroy a queue whose other
    // images and command buffers are still in flight.
    if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        resource.complete(accepted);
    })) {
        // A custom panic payload can itself panic on drop.
        std::mem::forget(panic);
    }
}

impl<T: VulkanFrameLease> Drop for Queue<T> {
    fn drop(&mut self) {
        // SAFETY: ownership reaches Drop only after recorded work has retired
        // or the device reported loss. The context keeps this device resident.
        unsafe {
            match self.sync {
                QueueSync::Timeline(timeline) => {
                    self.context.device.destroy_semaphore(timeline, None);
                }
                QueueSync::Fences(fences) => {
                    for fence in fences {
                        self.context.device.destroy_fence(fence, None);
                    }
                }
            }
            self.context.device.destroy_command_pool(self.pool, None);
        }
    }
}

trait RetiredQueue: Send {
    fn poll(&mut self) -> bool;
}

impl<T: VulkanFrameLease> RetiredQueue for Queue<T> {
    fn poll(&mut self) -> bool {
        let _ = Queue::poll(self);
        self.is_empty()
    }
}

fn ensure_retirement_worker() -> Result<(), VulkanError> {
    RETIREMENT_WORKER
        .get_or_init(|| {
            std::thread::Builder::new()
                .name("vesper-vulkan-retirement".into())
                .spawn(retire_queues)
                .map(drop)
        })
        .as_ref()
        .copied()
        .map_err(|error| VulkanError::RetirementWorker(error.to_string()))
}

fn retire_queues() {
    let mut pending = Vec::<Box<dyn RetiredQueue>>::with_capacity(MAX_QUEUES);
    loop {
        {
            let mut ready = RETIRED.lock().unwrap_or_else(|e| e.into_inner());
            if pending.is_empty() {
                ready = RETIRED_READY
                    .wait_while(ready, |ready| ready.is_empty())
                    .unwrap_or_else(|e| e.into_inner());
            } else if ready.is_empty() {
                (ready, _) = RETIRED_READY
                    .wait_timeout(ready, RETIREMENT_POLL_INTERVAL)
                    .unwrap_or_else(|e| e.into_inner());
            }
            pending.append(&mut ready);
        }
        pending.retain_mut(|queue| !queue.poll());
    }
}

trait CommandRecording<T: VulkanFrameLease> {
    fn record(self, queue: &mut Queue<T>, index: usize, resource: &T) -> Result<(), VulkanError>;
}

struct PlainRecording<F>(F);

impl<T, F> CommandRecording<T> for PlainRecording<F>
where
    T: VulkanFrameLease,
    F: FnOnce(
        &VulkanDeviceContext,
        vk::CommandBuffer,
        &VesperGpuContext,
        &T,
    ) -> Result<(), VulkanError>,
{
    fn record(self, queue: &mut Queue<T>, index: usize, resource: &T) -> Result<(), VulkanError> {
        let command = queue.commands[index];
        (self.0)(
            &queue.context,
            command,
            &queue.context.gpu_context(command),
            resource,
        )
    }
}

struct FilteredRecording<F> {
    extent: vk::Extent2D,
    encode: F,
}

impl<T, F> CommandRecording<T> for FilteredRecording<F>
where
    T: VulkanFrameLease,
    F: FnOnce(
        &VulkanDeviceContext,
        vk::CommandBuffer,
        &VesperGpuContext,
        &T,
        &VulkanFilterImages,
    ) -> Result<(), VulkanError>,
{
    fn record(self, queue: &mut Queue<T>, index: usize, resource: &T) -> Result<(), VulkanError> {
        let command = queue.commands[index];
        let slot = &mut queue.filter_images[index];
        if slot
            .as_ref()
            .is_none_or(|images| images.extent() != self.extent)
        {
            *slot = Some(VulkanFilterImages::new(&queue.context, self.extent)?);
        }
        let images = slot.as_ref().ok_or(VulkanError::Exhausted)?;
        // SAFETY: the backend selected a free slot with a recording command.
        unsafe { images.prepare(command) };
        (self.encode)(
            &queue.context,
            command,
            &queue.context.gpu_context(command),
            resource,
            images,
        )
    }
}

struct ColorRecording<'a, F> {
    frame: VulkanColorFrame<'a>,
    present: F,
}

impl<T, F> CommandRecording<T> for ColorRecording<'_, F>
where
    T: VulkanFrameLease,
    F: FnOnce(
        &VulkanDeviceContext,
        vk::CommandBuffer,
        &T,
        &VulkanFilterImage,
    ) -> Result<(), VulkanError>,
{
    fn record(self, queue: &mut Queue<T>, index: usize, resource: &T) -> Result<(), VulkanError> {
        let extent = self.frame.extent()?;
        let command = queue.commands[index];
        let filter = queue
            .color_filter
            .as_mut()
            .ok_or(VulkanError::ColorFilterUnavailable)?;
        let slot = &mut queue.filter_images[index];
        if slot.as_ref().is_none_or(|images| images.extent() != extent) {
            *slot = Some(VulkanFilterImages::new(&queue.context, extent)?);
        }
        let images = slot.as_ref().ok_or(VulkanError::Exhausted)?;
        // SAFETY: the owned slot has retired and the command is recording.
        unsafe { images.prepare(command) };
        let output = &images.targets()[0];
        filter.encode(index, &queue.context, command, self.frame, output)?;
        (self.present)(&queue.context, command, resource, output)
    }
}

/// Owns a reusable set of Vulkan command buffers and orders releases by timeline value.
pub struct VulkanGpuBackend<T: VulkanFrameLease> {
    queue: Option<Queue<T>>,
}

impl<T: VulkanFrameLease> VulkanGpuBackend<T> {
    pub fn new(context: VulkanDeviceContext) -> Result<Self, VulkanError> {
        ensure_retirement_worker()?;
        let permit = Permit::acquire()?;
        let device = &context.device;
        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(context.family)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        // SAFETY: the context certifies the queue family belongs to this live device.
        let pool = unsafe { device.create_command_pool(&pool_info, None) }?;
        let sync = if context.features.timeline_semaphores {
            let mut semaphore_type =
                vk::SemaphoreTypeCreateInfo::default().semaphore_type(vk::SemaphoreType::TIMELINE);
            let sem_info = vk::SemaphoreCreateInfo::default().push_next(&mut semaphore_type);
            // SAFETY: timeline semaphores are enabled at device creation.
            match unsafe { device.create_semaphore(&sem_info, None) } {
                Ok(timeline) => QueueSync::Timeline(timeline),
                Err(error) => {
                    // SAFETY: the empty pool is owned by this constructor.
                    unsafe { device.destroy_command_pool(pool, None) };
                    return Err(error.into());
                }
            }
        } else {
            let mut fences = [vk::Fence::null(); FRAME_SLOTS];
            let fence_info = vk::FenceCreateInfo::default();
            for slot in &mut fences {
                // SAFETY: device is live and owns each newly created unsignaled fence.
                match unsafe { device.create_fence(&fence_info, None) } {
                    Ok(fence) => *slot = fence,
                    Err(error) => {
                        for created in fences {
                            if created != vk::Fence::null() {
                                // SAFETY: each partial fence belongs to this live constructor.
                                unsafe { device.destroy_fence(created, None) };
                            }
                        }
                        // SAFETY: the empty pool is owned by this constructor.
                        unsafe { device.destroy_command_pool(pool, None) };
                        return Err(error.into());
                    }
                }
            }
            QueueSync::Fences(fences)
        };
        let mut queue = Queue {
            context,
            pool,
            sync,
            commands: [vk::CommandBuffer::null(); FRAME_SLOTS],
            slots: std::array::from_fn(|_| None),
            filter_images: std::array::from_fn(|_| None),
            color_filter: None,
            last_submitted: 0,
            epoch: 0,
            _permit: permit,
        };
        let command_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(FRAME_SLOTS as u32);
        // SAFETY: queue owns the pool and the requested command count is bounded.
        let allocated = unsafe { queue.context.device.allocate_command_buffers(&command_info) }?;
        queue.commands.copy_from_slice(&allocated);
        Ok(Self { queue: Some(queue) })
    }

    pub fn begin_epoch(&mut self, epoch: u64) -> Result<(), VulkanError> {
        let queue = self.queue.as_mut().ok_or(VulkanError::Exhausted)?;
        if epoch <= queue.epoch {
            return Err(VulkanError::StaleEpoch);
        }
        queue.epoch = epoch;
        queue.poll()
    }

    pub fn poll(&mut self) -> Result<(), VulkanError> {
        self.queue.as_mut().ok_or(VulkanError::Exhausted)?.poll()
    }

    /// Records plugin and presentation work into one primary command buffer.
    /// The resource encloses every referenced import and output image.
    ///
    /// # Safety
    /// `encode` must record valid Vulkan commands and keep all referenced
    /// resources live through `resource`. The recorder cannot submit or end
    /// the command buffer. Binary dependencies must be externally synchronized
    /// and live through completion.
    pub unsafe fn submit(
        &mut self,
        epoch: u64,
        resource: T,
        acquire: Option<vk::Semaphore>,
        presentation_ready: Option<vk::Semaphore>,
        encode: impl FnOnce(
            &VulkanDeviceContext,
            vk::CommandBuffer,
            &VesperGpuContext,
            &T,
        ) -> Result<(), VulkanError>,
    ) -> Result<u64, VulkanError> {
        self.submit_inner(
            epoch,
            resource,
            acquire,
            presentation_ready,
            PlainRecording(encode),
        )
    }

    /// Submits through a command slot with two reusable `GENERAL` RGBA16F images.
    ///
    /// # Safety
    /// The same rules as [`Self::submit`] apply, and the images are borrowed only
    /// for this submission. They may be reused once its timeline value retires.
    pub unsafe fn submit_filtered(
        &mut self,
        epoch: u64,
        resource: T,
        acquire: Option<vk::Semaphore>,
        presentation_ready: Option<vk::Semaphore>,
        extent: vk::Extent2D,
        encode: impl FnOnce(
            &VulkanDeviceContext,
            vk::CommandBuffer,
            &VesperGpuContext,
            &T,
            &VulkanFilterImages,
        ) -> Result<(), VulkanError>,
    ) -> Result<u64, VulkanError> {
        self.submit_inner(
            epoch,
            resource,
            acquire,
            presentation_ready,
            FilteredRecording { extent, encode },
        )
    }

    /// Creates per-slot bindings for the build-time reference shader.
    ///
    /// # Safety
    /// `shader` is the trusted SPIR-V build of `color-filter.slang`. The host
    /// enabled its required Vulkan features and selected a compute-capable queue.
    pub unsafe fn configure_color_filter(&mut self, shader: &[u32]) -> Result<(), VulkanError> {
        let queue = self.queue.as_mut().ok_or(VulkanError::Exhausted)?;
        queue.poll()?;
        if !queue.is_empty() {
            return Err(VulkanError::Backpressure);
        }
        queue.color_filter = Some(color_filter::ColorFilter::new(&queue.context, shader)?);
        Ok(())
    }

    /// Configures the Android build-time shader for one combined YCbCr input.
    /// The first imported format supplies the immutable sampler for the layout.
    ///
    /// # Safety
    /// `shader` is the trusted combined-420 build of `color-filter.slang`. The
    /// device enabled sampler YCbCr conversion and the Android import features.
    #[cfg(target_os = "android")]
    pub unsafe fn configure_android_color_filter(
        &mut self,
        shader: Box<[u32]>,
    ) -> Result<(), VulkanError> {
        let queue = self.queue.as_mut().ok_or(VulkanError::Exhausted)?;
        queue.poll()?;
        if !queue.is_empty() {
            return Err(VulkanError::Backpressure);
        }
        queue.color_filter = Some(color_filter::ColorFilter::android(shader));
        Ok(())
    }

    /// Runs the build-time reference shader and hands its `GENERAL` RGBA16F output
    /// to the presentation recorder on the same command buffer.
    ///
    /// # Safety
    /// The caller satisfies [`Self::submit`]. Imported views must be readable in
    /// `SHADER_READ_ONLY_OPTIMAL`, match the negotiated uncropped format and stay
    /// alive through submission retirement. Android inputs must already have a
    /// signaled producer fence and `resource` must retain the buffer and producer
    /// lease until retirement. Staged bytes are consumed before return.
    pub unsafe fn submit_color_filtered(
        &mut self,
        epoch: u64,
        resource: T,
        acquire: Option<vk::Semaphore>,
        presentation_ready: Option<vk::Semaphore>,
        frame: VulkanColorFrame<'_>,
        present: impl FnOnce(
            &VulkanDeviceContext,
            vk::CommandBuffer,
            &T,
            &VulkanFilterImage,
        ) -> Result<(), VulkanError>,
    ) -> Result<u64, VulkanError> {
        self.submit_inner(
            epoch,
            resource,
            acquire,
            presentation_ready,
            ColorRecording { frame, present },
        )
    }

    fn submit_inner<F>(
        &mut self,
        epoch: u64,
        resource: T,
        acquire: Option<vk::Semaphore>,
        presentation_ready: Option<vk::Semaphore>,
        recording: F,
    ) -> Result<u64, VulkanError>
    where
        F: CommandRecording<T>,
    {
        let queue = self.queue.as_mut().ok_or(VulkanError::Exhausted)?;
        if epoch != queue.epoch {
            return Err(VulkanError::StaleEpoch);
        }
        queue.poll()?;
        let index = queue
            .slots
            .iter()
            .position(Option::is_none)
            .ok_or(VulkanError::Backpressure)?;
        let value = queue
            .last_submitted
            .checked_add(1)
            .ok_or(VulkanError::Exhausted)?;
        let command = queue.commands[index];
        let device = &queue.context.device;
        // SAFETY: polling established this slot's previous command completed.
        // The backend exclusively owns and records this primary command buffer.
        unsafe {
            device.reset_command_buffer(command, vk::CommandBufferResetFlags::empty())?;
            device.begin_command_buffer(
                command,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
        }
        if let Err(error) = recording.record(queue, index, &resource) {
            // SAFETY: the command is still private and was never submitted.
            let _ = unsafe {
                queue
                    .context
                    .device
                    .reset_command_buffer(command, vk::CommandBufferResetFlags::empty())
            };
            return Err(error);
        }
        let device = &queue.context.device;
        // SAFETY: the encode callback records commands and leaves the buffer open.
        unsafe { device.end_command_buffer(command) }?;
        let waits = acquire.as_slice();
        let wait_stages = [vk::PipelineStageFlags::ALL_COMMANDS];
        let commands = [command];
        match queue.sync {
            QueueSync::Timeline(timeline_handle) => {
                let wait_values = [0];
                let signals = [timeline_handle, presentation_ready.unwrap_or_default()];
                let signal_values = [value, 0];
                let signal_count = 1 + usize::from(presentation_ready.is_some());
                let mut timeline = vk::TimelineSemaphoreSubmitInfo::default()
                    .wait_semaphore_values(&wait_values[..waits.len()])
                    .signal_semaphore_values(&signal_values[..signal_count]);
                let submit = vk::SubmitInfo::default()
                    .push_next(&mut timeline)
                    .command_buffers(&commands)
                    .wait_semaphores(waits)
                    .wait_dst_stage_mask(&wait_stages[..waits.len()])
                    .signal_semaphores(&signals[..signal_count]);
                // SAFETY: the caller certifies external queue synchronization and valid
                // binary dependencies. This submission owns a recorded command slot.
                unsafe { device.queue_submit(queue.context.queue, &[submit], vk::Fence::null()) }?;
            }
            QueueSync::Fences(fences) => {
                let signals = presentation_ready.as_slice();
                let submit = vk::SubmitInfo::default()
                    .command_buffers(&commands)
                    .wait_semaphores(waits)
                    .wait_dst_stage_mask(&wait_stages[..waits.len()])
                    .signal_semaphores(signals);
                // SAFETY: the caller certifies external queue synchronization, valid binary
                // dependencies, and this slot's fence was reset when it became free.
                unsafe { device.queue_submit(queue.context.queue, &[submit], fences[index]) }?;
            }
        }
        queue.last_submitted = value;
        queue.slots[index] = Some(Pending {
            ready_value: value,
            epoch,
            resource,
        });
        Ok(value)
    }

    /// Waits up to the supplied deadline for all currently submitted work.
    pub fn drain(&mut self, timeout: Duration) -> Result<(), VulkanError> {
        let queue = self.queue.as_mut().ok_or(VulkanError::Exhausted)?;
        if queue.is_empty() {
            return Ok(());
        }
        let nanos = u64::try_from(timeout.as_nanos())
            .unwrap_or(u64::MAX - 1)
            .min(u64::MAX - 1);
        let status = match queue.sync {
            QueueSync::Timeline(timeline) => {
                let semaphores = [timeline];
                let values = [queue.last_submitted];
                let info = vk::SemaphoreWaitInfo::default()
                    .semaphores(&semaphores)
                    .values(&values);
                // SAFETY: the timeline belongs to the live device and the wait is bounded.
                unsafe { queue.context.device.wait_semaphores(&info, nanos) }
            }
            QueueSync::Fences(fences) => {
                let mut active = [vk::Fence::null(); FRAME_SLOTS];
                let mut count = 0;
                for (slot, fence) in queue.slots.iter().zip(fences) {
                    if slot.is_some() {
                        active[count] = fence;
                        count += 1;
                    }
                }
                if count == 0 {
                    return Ok(());
                }
                // SAFETY: every collected fence is live and tracks an in-flight submission.
                unsafe {
                    queue
                        .context
                        .device
                        .wait_for_fences(&active[..count], true, nanos)
                }
            }
        };
        if status == Err(vk::Result::ERROR_DEVICE_LOST) {
            queue.cancel_all();
        }
        status?;
        queue.poll()
    }
}

impl<T: VulkanFrameLease> Drop for VulkanGpuBackend<T> {
    fn drop(&mut self) {
        let Some(mut queue) = self.queue.take() else {
            return;
        };
        // Closing invalidates every outstanding presentation, including the
        // last possible epoch, without reusing its resources before completion.
        for work in queue.slots.iter_mut().flatten() {
            work.epoch = queue.epoch ^ 1;
        }
        let _ = queue.poll();
        if !queue.is_empty() {
            RETIRED
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(Box::new(queue));
            RETIRED_READY.notify_one();
        }
    }
}
