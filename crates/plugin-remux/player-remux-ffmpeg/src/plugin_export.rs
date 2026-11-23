use std::ffi::c_void;
use std::mem::size_of;
use std::path::PathBuf;
use std::ptr;
use std::sync::OnceLock;

use player_plugin::{CompletedDownloadInfo, PostDownloadProcessor, ProcessorProgress};
use player_plugin_abi::checked::{MAX_JSON_BYTES, bytes, read_bytes, read_sized, sized_pointer};
use player_plugin_abi::*;

use crate::FfmpegRemuxProcessor;

const ID: &str = "io.github.umbrella22.vesper.remux-ffmpeg";
const NAME: &str = "player-remux-ffmpeg";

fn guard(call: impl FnOnce() -> VesperStatus) -> VesperStatus {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(call)).unwrap_or_else(|payload| {
        std::mem::forget(payload);
        VESPER_ERR_PANIC
    })
}

unsafe extern "C" fn capabilities(_ctx: *mut c_void, out: *mut VesperBytes) -> VesperStatus {
    static JSON: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    guard(|| {
        if !sized_pointer(out) {
            return VESPER_ERR_INVALID_ARG;
        }
        // SAFETY: out is a writable VesperBytes supplied by the caller.
        unsafe { out.write(VesperBytes::default()) };
        let Some(json) = JSON
            .get_or_init(|| serde_json::to_vec(&FfmpegRemuxProcessor::new().capabilities()).ok())
        else {
            return VESPER_ERR_INTERNAL;
        };
        // SAFETY: this immutable static buffer is live through every root call.
        unsafe { out.write(bytes(json)) };
        VESPER_OK
    })
}

#[derive(serde::Deserialize)]
struct Request {
    input: CompletedDownloadInfo,
    output_path: PathBuf,
}

struct Progress(Option<VesperProgressCallbacks>);

// SAFETY: native callbacks permit use from operation workers that finish before return.
unsafe impl Send for Progress {}
// SAFETY: the borrowed callback context remains live and thread-safe for the call.
unsafe impl Sync for Progress {}

impl ProcessorProgress for Progress {
    fn on_progress(&self, ratio: f32) {
        // Preserve sub-percent progress when translating the Rust ratio to ABI units.
        const TOTAL: u64 = 1_000_000;
        if let Some(callbacks) = &self.0
            && let Some(update) = callbacks.progress
        {
            // SAFETY: the callbacks and ctx are live through this synchronous operation.
            unsafe {
                update(
                    callbacks.ctx,
                    (ratio.clamp(0.0, 1.0) * TOTAL as f32) as u64,
                    TOTAL,
                )
            };
        }
    }

    fn is_cancelled(&self) -> bool {
        self.0.as_ref().is_some_and(|callbacks| {
            callbacks.is_cancelled.is_some_and(|check| {
                // SAFETY: the callback and ctx are live through the operation.
                unsafe { check(callbacks.ctx) != 0 }
            })
        })
    }
}

unsafe fn run(
    request: VesperBytes,
    progress: *const VesperProgressCallbacks,
    out: *mut VesperBytes,
    assemble: bool,
) -> VesperStatus {
    guard(|| {
        if !sized_pointer(out) {
            return VESPER_ERR_INVALID_ARG;
        }
        // SAFETY: the host provides writable storage for the result lease.
        unsafe { out.write(VesperBytes::default()) };
        let progress = if progress.is_null() {
            Progress(None)
        } else {
            // SAFETY: a non-null progress table is borrowed through this call.
            let Ok(callbacks) = (unsafe { read_sized(progress) }) else {
                return VESPER_ERR_INVALID_ARG;
            };
            Progress(Some(callbacks))
        };
        // SAFETY: the host lends the input JSON until the operation returns.
        let Ok(request) = (unsafe { read_bytes(request, MAX_JSON_BYTES) }) else {
            return VESPER_ERR_INVALID_ARG;
        };
        let Ok(Request { input, output_path }) = serde_json::from_slice(request) else {
            return VESPER_ERR_INVALID_ARG;
        };
        let processor = FfmpegRemuxProcessor::new();
        let result = if assemble {
            processor.assemble(&input, &output_path, &progress)
        } else {
            processor.process(&input, &output_path, &progress)
        };
        let Ok(serialized) = serde_json::to_vec(&result) else {
            return VESPER_ERR_INTERNAL;
        };
        let serialized = serialized.into_boxed_slice();
        let lease = bytes(&serialized);
        let _ = Box::into_raw(serialized);
        // SAFETY: ownership of the boxed byte slice transfers to the host.
        unsafe { out.write(lease) };
        VESPER_OK
    })
}

unsafe extern "C" fn process(
    _ctx: *mut c_void,
    request: VesperBytes,
    progress: *const VesperProgressCallbacks,
    out: *mut VesperBytes,
) -> VesperStatus {
    // SAFETY: input, callbacks and output meet the synchronous operation contract.
    unsafe { run(request, progress, out, false) }
}

unsafe extern "C" fn assemble(
    _ctx: *mut c_void,
    request: VesperBytes,
    progress: *const VesperProgressCallbacks,
    out: *mut VesperBytes,
) -> VesperStatus {
    // SAFETY: input, callbacks and output meet the synchronous operation contract.
    unsafe { run(request, progress, out, true) }
}

unsafe extern "C" fn release(_ctx: *mut c_void, result: VesperBytes) {
    if !result.data.is_null() {
        // SAFETY: the host returns the original pointer and length exactly once.
        unsafe {
            drop(Box::from_raw(ptr::slice_from_raw_parts_mut(
                result.data.cast_mut(),
                result.len as usize,
            )));
        }
    }
}

static TABLE: VesperPostDownloadProcessorVtable = VesperPostDownloadProcessorVtable {
    struct_size: size_of::<VesperPostDownloadProcessorVtable>() as u32,
    capabilities_json: Some(capabilities),
    process_json: Some(process),
    assemble_json: Some(assemble),
    release_result: Some(release),
};

unsafe extern "C" fn interface_at(
    _ctx: *mut c_void,
    index: u32,
    out: *mut VesperInterfaceId,
) -> VesperStatus {
    if !sized_pointer(out) || index != 0 {
        return VESPER_ERR_INVALID_ARG;
    }
    // SAFETY: the host provides writable interface ID storage.
    unsafe { out.write(VESPER_POST_DOWNLOAD_PROCESSOR_ID) };
    VESPER_OK
}

unsafe extern "C" fn query(
    _ctx: *mut c_void,
    id: *const VesperInterfaceId,
    out: *mut *const c_void,
) -> VesperStatus {
    if !sized_pointer(id) || !sized_pointer(out) {
        return VESPER_ERR_INVALID_ARG;
    }
    // SAFETY: the host passes a readable ID and a writable pointer output.
    unsafe {
        out.write(ptr::null());
        if id.read() != VESPER_POST_DOWNLOAD_PROCESSOR_ID {
            return VESPER_ERR_UNSUPPORTED;
        }
        out.write(ptr::from_ref(&TABLE).cast());
    }
    VESPER_OK
}

#[unsafe(no_mangle)]
pub extern "C" fn vesper_native_plugin_entry() -> *const VesperPluginRoot {
    &const {
        VesperPluginRoot {
            struct_size: size_of::<VesperPluginRoot>() as u32,
            abi_major: VESPER_PLUGIN_ABI_MAJOR,
            abi_minor: VESPER_PLUGIN_ABI_MINOR,
            owner: ptr::null_mut(),
            plugin_id: VesperBytes {
                data: ID.as_ptr(),
                len: ID.len() as u64,
            },
            plugin_name: VesperBytes {
                data: NAME.as_ptr(),
                len: NAME.len() as u64,
            },
            interface_count: 1,
            interface_at: Some(interface_at),
            query_interface: Some(query),
            destroy_owner: None,
        }
    }
}
