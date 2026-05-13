#![warn(clippy::undocumented_unsafe_blocks)]

mod error;
mod muxer;

use std::ffi::{CStr, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};

use player_plugin::{
    CompletedDownloadInfo, PostDownloadProcessor, ProcessorError,
    VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3, VesperPluginBytes, VesperPluginDescriptor,
    VesperPluginKind, VesperPluginProcessResult, VesperPluginProgressCallbacks,
    VesperPluginResultStatus, VesperPostDownloadProcessorApi,
};

pub use muxer::FfmpegRemuxProcessor;

static PLUGIN_NAME: &[u8] = b"player-remux-ffmpeg\0";

struct PluginBundle {
    api: VesperPostDownloadProcessorApi,
    descriptor: VesperPluginDescriptor,
}

#[unsafe(no_mangle)]
pub extern "C" fn vesper_plugin_entry() -> *const VesperPluginDescriptor {
    catch_unwind(AssertUnwindSafe(vesper_plugin_entry_impl)).unwrap_or(std::ptr::null())
}

fn vesper_plugin_entry_impl() -> *const VesperPluginDescriptor {
    let processor = Box::new(FfmpegRemuxProcessor::new());
    let processor = Box::into_raw(processor);

    let mut bundle = Box::new(PluginBundle {
        api: VesperPostDownloadProcessorApi {
            context: processor.cast::<c_void>(),
            destroy: Some(destroy_processor),
            name: Some(processor_name),
            capabilities_json: Some(processor_capabilities_json),
            free_bytes: Some(free_plugin_bytes),
            process_json: Some(processor_process_json),
            assemble_json: Some(processor_assemble_json),
        },
        descriptor: VesperPluginDescriptor {
            abi_version: VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
            plugin_kind: VesperPluginKind::PostDownloadProcessor,
            plugin_name: PLUGIN_NAME.as_ptr().cast::<c_char>(),
            api: std::ptr::null(),
        },
    });
    bundle.descriptor.api = (&bundle.api as *const VesperPostDownloadProcessorApi).cast::<c_void>();
    let bundle = Box::leak(bundle);
    &bundle.descriptor
}

unsafe extern "C" fn destroy_processor(context: *mut c_void) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        destroy_processor_impl(context);
    }));
}

fn destroy_processor_impl(context: *mut c_void) {
    if context.is_null() {
        return;
    }

    let processor = context.cast::<FfmpegRemuxProcessor>();
    // SAFETY: `context` was created by `vesper_plugin_entry_impl` from
    // `Box<FfmpegRemuxProcessor>` and is destroyed at most once by the host.
    let _ = unsafe { Box::from_raw(processor) };
}

unsafe extern "C" fn processor_name(_context: *mut c_void) -> *const c_char {
    PLUGIN_NAME.as_ptr().cast::<c_char>()
}

unsafe extern "C" fn processor_capabilities_json(context: *mut c_void) -> VesperPluginBytes {
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `context` is the processor pointer stored in the exported API
        // table and remains valid until the host invokes `destroy`.
        let processor = unsafe { &*(context.cast::<FfmpegRemuxProcessor>()) };
        serialize_payload(&processor.capabilities())
    }))
    .unwrap_or_else(|_| serialize_payload(&plugin_panic_error()))
}

unsafe extern "C" fn free_plugin_bytes(_context: *mut c_void, payload: VesperPluginBytes) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the payload was produced by this dynamic library and has not
        // been reclaimed yet.
        let _ = unsafe { payload.into_vec() };
    }));
}

unsafe extern "C" fn processor_process_json(
    context: *mut c_void,
    input_json: *const u8,
    input_json_len: usize,
    output_path: *const c_char,
    progress: VesperPluginProgressCallbacks,
) -> VesperPluginProcessResult {
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `context` is the processor pointer stored in the exported API
        // table and remains valid until the host invokes `destroy`.
        let processor = unsafe { &*(context.cast::<FfmpegRemuxProcessor>()) };
        let result = decode_input(input_json, input_json_len).and_then(|input| {
            let output_path = decode_output_path(output_path)?;
            let progress = CallbackProgress {
                callbacks: progress,
            };
            processor.process(&input, std::path::Path::new(&output_path), &progress)
        });

        encode_processor_result(result)
    }))
    .unwrap_or_else(|_| encode_processor_result(Err(plugin_panic_error())))
}

unsafe extern "C" fn processor_assemble_json(
    context: *mut c_void,
    input_json: *const u8,
    input_json_len: usize,
    output_path: *const c_char,
    progress: VesperPluginProgressCallbacks,
) -> VesperPluginProcessResult {
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `context` is the processor pointer stored in the exported API
        // table and remains valid until the host invokes `destroy`.
        let processor = unsafe { &*(context.cast::<FfmpegRemuxProcessor>()) };
        let result = decode_input(input_json, input_json_len).and_then(|input| {
            let output_path = decode_output_path(output_path)?;
            let progress = CallbackProgress {
                callbacks: progress,
            };
            processor.assemble(&input, std::path::Path::new(&output_path), &progress)
        });

        encode_processor_result(result)
    }))
    .unwrap_or_else(|_| encode_processor_result(Err(plugin_panic_error())))
}

fn decode_output_path(output_path: *const c_char) -> Result<String, ProcessorError> {
    if output_path.is_null() {
        return Err(ProcessorError::AbiViolation(
            "plugin output path pointer must not be null".to_owned(),
        ));
    }
    // SAFETY: `output_path` has been checked for null and the plugin ABI
    // requires a NUL-terminated UTF-8 path for this synchronous callback.
    unsafe { CStr::from_ptr(output_path) }
        .to_str()
        .map(str::to_owned)
        .map_err(|error| ProcessorError::AbiViolation(error.to_string()))
}

fn encode_processor_result(
    result: Result<player_plugin::ProcessorOutput, ProcessorError>,
) -> VesperPluginProcessResult {
    match result {
        Ok(output) => VesperPluginProcessResult {
            status: VesperPluginResultStatus::Success,
            payload: serialize_payload(&output),
        },
        Err(error) => VesperPluginProcessResult {
            status: VesperPluginResultStatus::Failure,
            payload: serialize_payload(&error),
        },
    }
}

fn decode_input(
    input_json: *const u8,
    input_json_len: usize,
) -> Result<CompletedDownloadInfo, ProcessorError> {
    if input_json.is_null() {
        return Err(ProcessorError::AbiViolation(
            "plugin input JSON pointer must not be null".to_owned(),
        ));
    }

    // SAFETY: `input_json` has been checked for null and the ABI caller keeps
    // the byte range alive for this synchronous callback.
    let payload = unsafe { std::slice::from_raw_parts(input_json, input_json_len) };
    serde_json::from_slice(payload).map_err(|error| ProcessorError::PayloadCodec(error.to_string()))
}

fn serialize_payload<T: serde::Serialize>(value: &T) -> VesperPluginBytes {
    match serde_json::to_vec(value) {
        Ok(payload) => VesperPluginBytes::from_vec(payload),
        Err(error) => VesperPluginBytes::from_vec(error.to_string().into_bytes()),
    }
}

fn plugin_panic_error() -> ProcessorError {
    ProcessorError::AbiViolation("plugin callback panicked".to_owned())
}

struct CallbackProgress {
    callbacks: VesperPluginProgressCallbacks,
}

impl player_plugin::ProcessorProgress for CallbackProgress {
    fn on_progress(&self, ratio: f32) {
        if let Some(on_progress) = self.callbacks.on_progress {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                // SAFETY: callback pointers and context are borrowed from the
                // host for the duration of this synchronous plugin invocation.
                unsafe { on_progress(self.callbacks.context, ratio) };
            }));
        }
    }

    fn is_cancelled(&self) -> bool {
        self.callbacks.is_cancelled.is_some_and(|is_cancelled| {
            catch_unwind(AssertUnwindSafe(|| {
                // SAFETY: callback pointers and context are borrowed from the
                // host for the duration of this synchronous plugin invocation.
                unsafe { is_cancelled(self.callbacks.context) }
            }))
            .unwrap_or(true)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_input, vesper_plugin_entry};
    use player_plugin::{
        ProcessorError, VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3, VesperPluginKind,
    };

    #[test]
    fn exported_descriptor_matches_expected_plugin_metadata() {
        // SAFETY: the remux entry point returns a process-lifetime descriptor
        // pointer or null; this test immediately borrows it.
        let descriptor = unsafe { vesper_plugin_entry().as_ref() }.expect("descriptor");

        assert_eq!(
            descriptor.abi_version,
            VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3
        );
        assert_eq!(
            descriptor.plugin_kind,
            VesperPluginKind::PostDownloadProcessor
        );
        assert!(!descriptor.api.is_null());
        assert!(!descriptor.plugin_name.is_null());
    }

    #[test]
    fn decode_input_rejects_null_pointer_as_abi_violation() {
        let error = decode_input(std::ptr::null(), 0).expect_err("null input should fail");

        assert!(matches!(error, ProcessorError::AbiViolation(_)));
    }

    #[test]
    fn decode_input_rejects_invalid_json_as_payload_codec_error() {
        let error = decode_input(b"not-json".as_ptr(), b"not-json".len())
            .expect_err("invalid json should fail");

        assert!(matches!(error, ProcessorError::PayloadCodec(_)));
    }
}
