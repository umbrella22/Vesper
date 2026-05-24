use super::*;

#[derive(Debug)]
pub(crate) struct DynamicPostDownloadProcessorInner {
    #[allow(dead_code)]
    library: Option<Arc<LibraryHolder>>,
    name: String,
    api: CheckedPostDownloadProcessorApi,
    capabilities: ProcessorCapabilities,
}

impl Drop for DynamicPostDownloadProcessorInner {
    fn drop(&mut self) {
        if let Some(destroy) = self.api.destroy {
            // SAFETY: `destroy` and `context` come from the validated plugin ABI
            // table and are only invoked once when this wrapper is dropped.
            unsafe { destroy(self.api.context) };
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DynamicPostDownloadProcessor {
    inner: Arc<DynamicPostDownloadProcessorInner>,
}

impl DynamicPostDownloadProcessor {
    pub(crate) fn new(
        library: Option<Arc<LibraryHolder>>,
        fallback_name: String,
        api: CheckedPostDownloadProcessorApi,
    ) -> Result<Self, PluginLoadError> {
        let name = if let Some(name_fn) = api.name {
            // SAFETY: the plugin ABI declares `name_fn` with `api.context`, and
            // the returned pointer is interpreted immediately as an optional
            // NUL-terminated UTF-8 string.
            let name_ptr = unsafe { name_fn(api.context) };
            if name_ptr.is_null() {
                fallback_name
            } else {
                c_string_field(name_ptr, "processor_name")?
            }
        } else {
            fallback_name
        };
        let capabilities = decode_plugin_bytes::<ProcessorCapabilities>(
            // SAFETY: the validated API guarantees `capabilities_json` and
            // `free_bytes` are present and use the shared `VesperPluginBytes`
            // ownership contract documented in `player-plugin`.
            unsafe { (api.capabilities_json)(api.context) },
            api.free_bytes,
            api.context,
        )
        .map_err(map_capabilities_payload_error)?;

        Ok(Self {
            inner: Arc::new(DynamicPostDownloadProcessorInner {
                library,
                name,
                api,
                capabilities,
            }),
        })
    }

    fn call_json_entry(
        &self,
        entry: ProcessJsonFn,
        input: &CompletedDownloadInfo,
        output_path: &Path,
        progress: &dyn ProcessorProgress,
    ) -> Result<ProcessorOutput, ProcessorError> {
        let input_json = serde_json::to_vec(input).map_err(|error| {
            ProcessorError::PayloadCodec(format!(
                "serialize dynamic plugin input for `{}` failed: {error}",
                self.inner.name
            ))
        })?;
        let output_path = CString::new(output_path.to_string_lossy().as_bytes()).map_err(|_| {
            ProcessorError::OutputPath(format!(
                "output path for plugin `{}` contains interior NUL",
                self.inner.name
            ))
        })?;

        let mut adapter = ProgressAdapter { progress };
        let callbacks = VesperPluginProgressCallbacks {
            context: (&mut adapter as *mut ProgressAdapter<'_>).cast::<c_void>(),
            on_progress: Some(progress_on_progress),
            is_cancelled: Some(progress_is_cancelled),
        };

        // SAFETY: the validated plugin API guarantees the JSON entry is present.
        // `input_json` and `output_path` live for the duration of the call, and
        // the ABI contract documents that `callbacks.context` is only valid
        // during this synchronous invocation.
        let result = unsafe {
            entry(
                self.inner.api.context,
                input_json.as_ptr(),
                input_json.len(),
                output_path.as_ptr(),
                callbacks,
            )
        };

        match result.status {
            VesperPluginResultStatus::Success => decode_plugin_bytes::<ProcessorOutput>(
                result.payload,
                self.inner.api.free_bytes,
                self.inner.api.context,
            )
            .map_err(|error| map_plugin_payload_error(&self.inner.name, "success", error)),
            VesperPluginResultStatus::Failure => decode_plugin_bytes::<ProcessorError>(
                result.payload,
                self.inner.api.free_bytes,
                self.inner.api.context,
            )
            .map_err(|error| map_plugin_payload_error(&self.inner.name, "error", error))
            .and_then(Err),
        }
    }
}

impl PostDownloadProcessor for DynamicPostDownloadProcessor {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn supported_input_formats(&self) -> &[player_plugin::ContentFormatKind] {
        &self.inner.capabilities.supported_input_formats
    }

    fn capabilities(&self) -> ProcessorCapabilities {
        self.inner.capabilities.clone()
    }

    fn process(
        &self,
        input: &CompletedDownloadInfo,
        output_path: &Path,
        progress: &dyn ProcessorProgress,
    ) -> Result<ProcessorOutput, ProcessorError> {
        self.call_json_entry(self.inner.api.process_json, input, output_path, progress)
    }

    fn assemble(
        &self,
        input: &CompletedDownloadInfo,
        output_path: &Path,
        progress: &dyn ProcessorProgress,
    ) -> Result<ProcessorOutput, ProcessorError> {
        self.call_json_entry(self.inner.api.assemble_json, input, output_path, progress)
    }
}

struct ProgressAdapter<'a> {
    progress: &'a dyn ProcessorProgress,
}

unsafe extern "C" fn progress_on_progress(context: *mut c_void, ratio: f32) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `context` is created from `ProgressAdapter` immediately before the
        // synchronous `process_json` call and remains valid until that call returns.
        let adapter = unsafe { &*(context.cast::<ProgressAdapter<'_>>()) };
        adapter.progress.on_progress(ratio);
    }));
}

unsafe extern "C" fn progress_is_cancelled(context: *mut c_void) -> bool {
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `context` is created from `ProgressAdapter` immediately before the
        // synchronous `process_json` call and remains valid until that call returns.
        let adapter = unsafe { &*(context.cast::<ProgressAdapter<'_>>()) };
        adapter.progress.is_cancelled()
    }))
    .unwrap_or(true)
}
