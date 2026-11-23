use super::*;
use player_plugin::{
    CompletedDownloadInfo, ContentFormatKind, PostDownloadProcessor, ProcessorCapabilities,
    ProcessorError, ProcessorOutput, ProcessorProgress,
};
use std::sync::atomic::{AtomicBool, Ordering};

impl NativePlugin {
    /// Adapts a native post-download processor to the Rust download API.
    pub fn post_download_processor(&self) -> PluginResult<Arc<dyn PostDownloadProcessor>> {
        let raw = self
            .query(&VESPER_POST_DOWNLOAD_PROCESSOR_ID)?
            .cast::<VesperPostDownloadProcessorVtable>();
        // SAFETY: the interface UUID identifies the entire native post-download table.
        let table = unsafe { read_vtable(raw, size_of::<VesperPostDownloadProcessorVtable>()) }?;
        let (Some(capabilities), Some(process), Some(release)) = (
            table.capabilities_json,
            table.process_json,
            table.release_result,
        ) else {
            return Err(PluginError::Contract(
                "incomplete post-download processor table",
            ));
        };
        let mut out = VesperBytes::default();
        // SAFETY: the owner is live and out addresses writable storage.
        check_status(unsafe { capabilities(self.root.0.owner, &mut out) })?;
        // SAFETY: capabilities are root-owned immutable bytes until destroy_owner.
        let capabilities: ProcessorCapabilities =
            serde_json::from_slice(unsafe { read_bytes(out, MAX_JSON_BYTES) }?)
                .map_err(|_| PluginError::Contract("invalid post-download capabilities JSON"))?;
        if capabilities.supports_assembly && table.assemble_json.is_none() {
            return Err(PluginError::Contract(
                "assembly advertised without an operation",
            ));
        }
        Ok(Arc::new(NativePostDownload {
            root: Arc::clone(&self.root),
            name: self.name.clone(),
            capabilities,
            process: Some(process),
            assemble: table.assemble_json,
            release,
        }))
    }
}

struct NativePostDownload {
    root: Arc<Root>,
    name: String,
    capabilities: ProcessorCapabilities,
    process: VesperDownloadOperation,
    assemble: VesperDownloadOperation,
    release: unsafe extern "C" fn(*mut c_void, VesperBytes),
}

struct OperationProgress<'a> {
    observer: &'a dyn ProcessorProgress,
    panicked: AtomicBool,
}

fn invoke_progress<R: Copy>(
    ctx: *mut c_void,
    fallback: R,
    call: impl FnOnce(&OperationProgress<'_>) -> R,
) -> R {
    // SAFETY: only passed to the plugin for synchronous use within invoke.
    let state = unsafe { &*ctx.cast::<OperationProgress<'_>>() };
    if state.panicked.load(Ordering::Relaxed) {
        return fallback;
    }
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| call(state))).unwrap_or_else(
        |payload| {
            state.panicked.store(true, Ordering::Relaxed);
            std::mem::forget(payload);
            fallback
        },
    )
}

unsafe extern "C" fn on_progress(ctx: *mut c_void, processed: u64, total: u64) {
    if total != 0 {
        invoke_progress(ctx, (), |state| {
            state
                .observer
                .on_progress(processed.min(total) as f32 / total as f32);
        });
    }
}

unsafe extern "C" fn is_cancelled(ctx: *mut c_void) -> u8 {
    invoke_progress(ctx, 1, |state| u8::from(state.observer.is_cancelled()))
}

struct ResultLease<'a> {
    processor: &'a NativePostDownload,
    result: VesperBytes,
}

impl Drop for ResultLease<'_> {
    fn drop(&mut self) {
        if !self.result.data.is_null() {
            // SAFETY: the operation transferred the result once and the root is live.
            unsafe { (self.processor.release)(self.processor.root.0.owner, self.result) };
        }
    }
}

impl NativePostDownload {
    fn invoke(
        &self,
        operation: VesperDownloadOperation,
        input: &CompletedDownloadInfo,
        output_path: &Path,
        progress: &dyn ProcessorProgress,
    ) -> Result<ProcessorOutput, ProcessorError> {
        #[derive(serde::Serialize)]
        struct Request<'a> {
            input: &'a CompletedDownloadInfo,
            output_path: &'a Path,
        }
        let operation = operation
            .ok_or_else(|| ProcessorError::UnsupportedFormat(input.content_format.kind()))?;
        let encode_error =
            |error: serde_json::Error| ProcessorError::PayloadCodec(error.to_string());
        let request = serde_json::to_vec(&Request { input, output_path }).map_err(encode_error)?;
        if request.len() > MAX_JSON_BYTES {
            return Err(ProcessorError::PayloadCodec(
                "post-download request exceeds ABI JSON limit".into(),
            ));
        }
        let state = OperationProgress {
            observer: progress,
            panicked: AtomicBool::new(false),
        };
        let callbacks = VesperProgressCallbacks {
            struct_size: size_of::<VesperProgressCallbacks>() as u32,
            ctx: ptr::from_ref(&state).cast_mut().cast(),
            progress: Some(on_progress),
            is_cancelled: Some(is_cancelled),
        };
        let mut result = ResultLease {
            processor: self,
            result: VesperBytes::default(),
        };
        // SAFETY: the request, result storage, and callbacks are live through the call.
        let status = unsafe {
            operation(
                self.root.0.owner,
                bytes(&request),
                &callbacks,
                &mut result.result,
            )
        };
        if state.panicked.load(Ordering::Relaxed) {
            return Err(ProcessorError::AbiViolation(
                "post-download observer panicked".into(),
            ));
        }
        let abi_error = |error: PluginError| ProcessorError::AbiViolation(error.to_string());
        check_status(status).map_err(abi_error)?;
        // SAFETY: the result lease owns these bytes until after deserialization.
        let payload = unsafe { read_bytes(result.result, MAX_JSON_BYTES) }.map_err(abi_error)?;
        serde_json::from_slice::<Result<ProcessorOutput, ProcessorError>>(payload)
            .map_err(encode_error)?
    }
}

impl PostDownloadProcessor for NativePostDownload {
    fn name(&self) -> &str {
        &self.name
    }
    fn supported_input_formats(&self) -> &[ContentFormatKind] {
        &self.capabilities.supported_input_formats
    }
    fn capabilities(&self) -> ProcessorCapabilities {
        self.capabilities.clone()
    }
    fn process(
        &self,
        input: &CompletedDownloadInfo,
        output_path: &Path,
        progress: &dyn ProcessorProgress,
    ) -> Result<ProcessorOutput, ProcessorError> {
        self.invoke(self.process, input, output_path, progress)
    }
    fn assemble(
        &self,
        input: &CompletedDownloadInfo,
        output_path: &Path,
        progress: &dyn ProcessorProgress,
    ) -> Result<ProcessorOutput, ProcessorError> {
        self.invoke(self.assemble, input, output_path, progress)
    }
}
