use std::{
    ffi::{CString, c_char, c_void},
    os::unix::ffi::OsStrExt,
    path::Path,
    ptr::NonNull,
    sync::OnceLock,
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use libloading::Library;
use player_platform_native_frame::NativeFramePipelineFrame;

use crate::abi::*;

type Submit = unsafe extern "C" fn(*mut c_void, *const VesperProbeVideoFrame, u32) -> u32;
type Stats = unsafe extern "C" fn(*mut c_void, *mut VesperProbeMetalStats);
type Command = unsafe extern "C" fn(*mut c_void);

pub struct Metal {
    context: NonNull<c_void>,
    submit: Submit,
    stats: Stats,
    retire: Command,
    signal: Command,
    close: Command,
    wait: unsafe extern "C" fn(*mut c_void, u32) -> u32,
    check_avplayer: unsafe extern "C" fn(*const c_char) -> u32,
}

impl Metal {
    pub unsafe fn open(path: &Path) -> Result<Self> {
        // The runner supplies one backend. Its completion handlers can run
        // after a session closes, so retain this mapping through process exit.
        static LIBRARY: OnceLock<Result<Library, libloading::Error>> = OnceLock::new();
        let library = LIBRARY
            .get_or_init(|| {
                // SAFETY: the caller supplies the trusted fixture built by the runner.
                unsafe { Library::new(path) }
            })
            .as_ref()
            .map_err(|error| anyhow::anyhow!("load Metal backend: {error}"))?;
        // SAFETY: these signatures match the exports in the probe's metal.mm.
        let (open, submit, stats, retire, signal, close, wait, check_avplayer) = unsafe {
            (
                *library
                    .get::<unsafe extern "C" fn() -> *mut c_void>(b"vesper_probe_metal_open\0")?,
                *library.get::<Submit>(b"vesper_probe_metal_submit\0")?,
                *library.get::<Stats>(b"vesper_probe_metal_stats\0")?,
                *library.get::<Command>(b"vesper_probe_metal_retire\0")?,
                *library.get::<Command>(b"vesper_probe_metal_signal\0")?,
                *library.get::<Command>(b"vesper_probe_metal_close\0")?,
                *library.get::<unsafe extern "C" fn(*mut c_void, u32) -> u32>(
                    b"vesper_probe_metal_wait\0",
                )?,
                *library.get::<unsafe extern "C" fn(*const c_char) -> u32>(
                    b"vesper_probe_avplayer_check\0",
                )?,
            )
        };
        // SAFETY: the functions are mapped and `open` has no lifetimed arguments.
        let context = NonNull::new(unsafe { open() }).context("initialize Metal probe")?;
        Ok(Self {
            context,
            submit,
            stats,
            retire,
            signal,
            close,
            wait,
            check_avplayer,
        })
    }

    /// The frame must carry a live CVPixelBuffer lease borrowed from the decoder.
    pub unsafe fn submit(
        &mut self,
        frame: &NativeFramePipelineFrame,
        lease_id: u64,
        gated: bool,
    ) -> Result<bool> {
        let pixel_buffer =
            NonNull::new(frame.handle as *mut c_void).context("null decoded frame")?;
        // SAFETY: the decoder's lease is still held by NativeFramePipelineCore.
        unsafe { CFRetain(pixel_buffer.as_ptr()) };
        let lease = RetainedPixelBuffer(pixel_buffer);
        let pod = VesperProbeVideoFrame {
            struct_size: size_of::<VesperProbeVideoFrame>() as u32,
            width: frame.width,
            height: frame.height,
            pts_us: frame.presentation_time_us,
            pixel_buffer: frame.handle as u64,
            lease_id,
            release_ctx: pixel_buffer.as_ptr(),
            release_fn: Some(release_pixel_buffer),
        };
        // SAFETY: the backend borrows the descriptor for this call and inherits
        // our retain on OK. Its completion function balances the retain once.
        let status = unsafe { (self.submit)(self.context.as_ptr(), &pod, u32::from(gated)) };
        match status {
            VESPER_PROBE_OK => {
                std::mem::forget(lease);
                Ok(true)
            }
            VESPER_PROBE_TRY_AGAIN => Ok(false),
            VESPER_PROBE_INVALID => bail!("unsupported decoded layout or non-IOSurface frame"),
            VESPER_PROBE_INTERNAL => bail!("could not import or encode the Metal frame"),
            _ => bail!("unknown Metal frame status {status}"),
        }
    }

    pub fn stats(&mut self) -> VesperProbeMetalStats {
        let mut stats = VesperProbeMetalStats::default();
        // SAFETY: `context` is alive and the output descriptor is writable.
        unsafe { (self.stats)(self.context.as_ptr(), &mut stats) };
        stats
    }

    pub fn retire(&mut self) {
        // SAFETY: the backend owns its pool and the unique host context is alive.
        unsafe { (self.retire)(self.context.as_ptr()) };
    }

    pub fn signal(&mut self) {
        // SAFETY: this context owns the shared event that gates submitted work.
        unsafe { (self.signal)(self.context.as_ptr()) };
    }

    pub fn check_avplayer_resource(&self, path: &Path) -> Result<()> {
        let path = CString::new(path.as_os_str().as_bytes())?;
        // SAFETY: `path` is a valid read-only C string for the duration of the call.
        let status = unsafe { (self.check_avplayer)(path.as_ptr()) };
        ensure!(
            status == VESPER_PROBE_OK,
            "AVPlayer resource did not become ready (status {status})"
        );
        Ok(())
    }

    pub fn wait_idle(&mut self, timeout: Duration) -> Result<VesperProbeMetalStats> {
        let timeout_ms = u32::try_from(timeout.as_millis())?;
        // SAFETY: the fixture owns the live context and waits for at most timeout_ms.
        let status = unsafe { (self.wait)(self.context.as_ptr(), timeout_ms) };
        let stats = self.stats();
        ensure!(
            status == VESPER_PROBE_OK,
            "Metal completion wait returned {status}: {stats:?}"
        );
        ensure!(stats.failed == 0, "Metal command buffer failed {stats:?}");
        ensure!(
            stats.released == stats.submitted && stats.active == 0 && stats.retired == 0,
            "Metal completion did not retire every submitted slot {stats:?}"
        );
        Ok(stats)
    }
}

impl Drop for Metal {
    fn drop(&mut self) {
        // SAFETY: the host owns this context. Close unblocks the GPU and leaves
        // the completion handlers' independent resource owners alive.
        unsafe { (self.close)(self.context.as_ptr()) };
    }
}

struct RetainedPixelBuffer(NonNull<c_void>);

impl Drop for RetainedPixelBuffer {
    fn drop(&mut self) {
        // SAFETY: this owner balances exactly one CFRetain.
        unsafe { CFRelease(self.0.as_ptr()) };
    }
}

unsafe extern "C" fn release_pixel_buffer(context: *mut c_void, _lease_id: u64) {
    // SAFETY: the backend calls this once for a previously transferred retain.
    unsafe { CFRelease(context) };
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRetain(value: *const c_void) -> *const c_void;
    fn CFRelease(value: *const c_void);
}
