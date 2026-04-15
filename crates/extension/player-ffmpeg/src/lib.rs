mod error;
mod muxer;

use std::ffi::{CStr, c_char, c_void};

use player_plugin::{
    CompletedDownloadInfo, PostDownloadProcessor, ProcessorError, VESPER_PLUGIN_ABI_VERSION,
    VesperPluginBytes, VesperPluginDescriptor, VesperPluginKind, VesperPluginProcessResult,
    VesperPluginProgressCallbacks, VesperPluginResultStatus, VesperPostDownloadProcessorApi,
};

pub use muxer::FfmpegPostDownloadProcessor;

static PLUGIN_NAME: &[u8] = b"player-ffmpeg\0";

struct PluginBundle {
    api: VesperPostDownloadProcessorApi,
    descriptor: VesperPluginDescriptor,
}

#[unsafe(no_mangle)]
pub extern "C" fn vesper_plugin_entry() -> *const VesperPluginDescriptor {
    let processor = Box::new(FfmpegPostDownloadProcessor::new());
    let processor = Box::into_raw(processor);

    let mut bundle = Box::new(PluginBundle {
        api: VesperPostDownloadProcessorApi {
            context: processor.cast::<c_void>(),
            destroy: Some(destroy_processor),
            name: Some(processor_name),
            capabilities_json: Some(processor_capabilities_json),
            free_bytes: Some(free_plugin_bytes),
            process_json: Some(processor_process_json),
        },
        descriptor: VesperPluginDescriptor {
            abi_version: VESPER_PLUGIN_ABI_VERSION,
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
    if context.is_null() {
        return;
    }

    let processor = context.cast::<FfmpegPostDownloadProcessor>();
    let _ = unsafe { Box::from_raw(processor) };
}

unsafe extern "C" fn processor_name(_context: *mut c_void) -> *const c_char {
    PLUGIN_NAME.as_ptr().cast::<c_char>()
}

unsafe extern "C" fn processor_capabilities_json(context: *mut c_void) -> VesperPluginBytes {
    let processor = unsafe { &*(context.cast::<FfmpegPostDownloadProcessor>()) };
    serialize_payload(&processor.capabilities())
}

unsafe extern "C" fn free_plugin_bytes(_context: *mut c_void, payload: VesperPluginBytes) {
    let _ = unsafe { payload.into_vec() };
}

unsafe extern "C" fn processor_process_json(
    context: *mut c_void,
    input_json: *const u8,
    input_json_len: usize,
    output_path: *const c_char,
    progress: VesperPluginProgressCallbacks,
) -> VesperPluginProcessResult {
    let processor = unsafe { &*(context.cast::<FfmpegPostDownloadProcessor>()) };
    let result = decode_input(input_json, input_json_len).and_then(|input| {
        if output_path.is_null() {
            return Err(ProcessorError::OutputPath(
                "plugin output path pointer must not be null".to_owned(),
            ));
        }
        let output_path = unsafe { CStr::from_ptr(output_path) }
            .to_str()
            .map_err(|error| ProcessorError::OutputPath(error.to_string()))?;
        let progress = CallbackProgress {
            callbacks: progress,
        };
        processor.process(&input, std::path::Path::new(output_path), &progress)
    });

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
        return Err(ProcessorError::MuxFailed(
            "plugin input JSON pointer must not be null".to_owned(),
        ));
    }

    let payload = unsafe { std::slice::from_raw_parts(input_json, input_json_len) };
    serde_json::from_slice(payload).map_err(|error| ProcessorError::MuxFailed(error.to_string()))
}

fn serialize_payload<T: serde::Serialize>(value: &T) -> VesperPluginBytes {
    match serde_json::to_vec(value) {
        Ok(payload) => VesperPluginBytes::from_vec(payload),
        Err(error) => VesperPluginBytes::from_vec(error.to_string().into_bytes()),
    }
}

struct CallbackProgress {
    callbacks: VesperPluginProgressCallbacks,
}

impl player_plugin::ProcessorProgress for CallbackProgress {
    fn on_progress(&self, ratio: f32) {
        if let Some(on_progress) = self.callbacks.on_progress {
            unsafe { on_progress(self.callbacks.context, ratio) };
        }
    }

    fn is_cancelled(&self) -> bool {
        self.callbacks
            .is_cancelled
            .map(|is_cancelled| unsafe { is_cancelled(self.callbacks.context) })
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::vesper_plugin_entry;
    use player_plugin::{VESPER_PLUGIN_ABI_VERSION, VesperPluginKind};

    #[test]
    fn exported_descriptor_matches_expected_plugin_metadata() {
        let descriptor = unsafe { vesper_plugin_entry().as_ref() }.expect("descriptor");

        assert_eq!(descriptor.abi_version, VESPER_PLUGIN_ABI_VERSION);
        assert_eq!(
            descriptor.plugin_kind,
            VesperPluginKind::PostDownloadProcessor
        );
        assert!(!descriptor.api.is_null());
        assert!(!descriptor.plugin_name.is_null());
    }
}
