use super::*;

#[derive(Debug)]
pub(crate) struct DynamicFrameProcessorPluginFactoryInner {
    #[allow(dead_code)]
    library: Option<Arc<LibraryHolder>>,
    name: String,
    api: CheckedFrameProcessorPluginApi,
    capabilities: FrameProcessorCapabilities,
}

impl Drop for DynamicFrameProcessorPluginFactoryInner {
    fn drop(&mut self) {
        if let Some(destroy) = self.api.destroy {
            // SAFETY: `destroy` and `context` come from the validated plugin ABI
            // table and are only invoked once when this wrapper is dropped.
            unsafe { destroy(self.api.context) };
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DynamicFrameProcessorPluginFactory {
    inner: Arc<DynamicFrameProcessorPluginFactoryInner>,
}

impl DynamicFrameProcessorPluginFactory {
    pub(crate) fn new(
        library: Option<Arc<LibraryHolder>>,
        fallback_name: String,
        api: CheckedFrameProcessorPluginApi,
    ) -> Result<Self, PluginLoadError> {
        let name = if let Some(name_fn) = api.name {
            // SAFETY: the plugin ABI declares `name_fn` with `api.context`, and
            // the returned pointer is interpreted immediately as an optional
            // NUL-terminated UTF-8 string.
            let name_ptr = unsafe { name_fn(api.context) };
            if name_ptr.is_null() {
                fallback_name
            } else {
                c_string_field(name_ptr, "frame_processor_name")?
            }
        } else {
            fallback_name
        };
        let capabilities = decode_plugin_bytes::<FrameProcessorCapabilities>(
            // SAFETY: the validated API guarantees `capabilities_json` and
            // `free_bytes` are present and use the shared `VesperPluginBytes`
            // ownership contract documented in `player-plugin`.
            unsafe { (api.capabilities_json)(api.context) },
            api.free_bytes,
            api.context,
        )
        .map_err(map_capabilities_payload_error)?;

        Ok(Self {
            inner: Arc::new(DynamicFrameProcessorPluginFactoryInner {
                library,
                name,
                api,
                capabilities,
            }),
        })
    }
}

impl FrameProcessorPluginFactory for DynamicFrameProcessorPluginFactory {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn capabilities(&self) -> FrameProcessorCapabilities {
        self.inner.capabilities.clone()
    }

    fn open_session(
        &self,
        config: &FrameProcessorSessionConfig,
    ) -> Result<Box<dyn FrameProcessorSession>, FrameProcessorError> {
        let config_json = serde_json::to_vec(config).map_err(|error| {
            FrameProcessorError::payload_codec(format!(
                "serialize frame processor config for `{}` failed: {error}",
                self.inner.name
            ))
        })?;

        // SAFETY: the validated plugin API guarantees `open_session_json` is
        // present, and `config_json` remains alive for the duration of this
        // synchronous callback.
        let result = unsafe {
            (self.inner.api.open_session_json)(
                self.inner.api.context,
                config_json.as_ptr(),
                config_json.len(),
            )
        };

        match result.status {
            VesperPluginResultStatus::Success => {
                if result.session.is_null() {
                    reclaim_plugin_payload(
                        result.payload,
                        self.inner.api.free_bytes,
                        self.inner.api.context,
                    );
                    return Err(FrameProcessorError::abi_violation(format!(
                        "frame processor plugin `{}` returned a null session pointer",
                        self.inner.name
                    )));
                }
                let session_info = decode_plugin_bytes_or_default::<FrameProcessorSessionInfo>(
                    result.payload,
                    self.inner.api.free_bytes,
                    self.inner.api.context,
                )
                .map_err(|error| {
                    map_frame_processor_payload_error(&self.inner.name, "open_session", error)
                })?;
                Ok(Box::new(DynamicFrameProcessorSession {
                    factory: self.inner.clone(),
                    session: result.session,
                    session_info,
                    closed: false,
                    outstanding_frames: Vec::new(),
                }))
            }
            VesperPluginResultStatus::Failure => {
                let error = decode_frame_processor_error_payload(
                    result.payload,
                    self.inner.api.free_bytes,
                    self.inner.api.context,
                    &self.inner.name,
                    "open_session",
                );
                Err(error)
            }
        }
    }
}

#[derive(Debug)]
struct DynamicFrameProcessorSession {
    factory: Arc<DynamicFrameProcessorPluginFactoryInner>,
    session: *mut c_void,
    session_info: FrameProcessorSessionInfo,
    closed: bool,
    outstanding_frames: Vec<NativeFrame>,
}

// SAFETY: the dynamic frame processor session is only exposed through
// `FrameProcessorSession: Send`; the plugin ABI requires the opaque session
// pointer to be safe to move across threads when exported through this API.
unsafe impl Send for DynamicFrameProcessorSession {}

impl DynamicFrameProcessorSession {
    fn ensure_open(&self) -> Result<(), FrameProcessorError> {
        if self.closed || self.session.is_null() {
            Err(FrameProcessorError::NotConfigured)
        } else {
            Ok(())
        }
    }

    fn decode_operation_result(
        &self,
        result: VesperPluginProcessResult,
        operation: &'static str,
    ) -> Result<(), FrameProcessorError> {
        match result.status {
            VesperPluginResultStatus::Success => {
                let _ = decode_plugin_bytes_or_default::<FrameProcessorOperationStatus>(
                    result.payload,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                )
                .map_err(|error| {
                    map_frame_processor_payload_error(&self.factory.name, operation, error)
                })?;
                Ok(())
            }
            VesperPluginResultStatus::Failure => Err(decode_frame_processor_error_payload(
                result.payload,
                self.factory.api.free_bytes,
                self.factory.api.context,
                &self.factory.name,
                operation,
            )),
        }
    }

    fn take_outstanding_frame(
        &mut self,
        frame: &NativeFrame,
    ) -> Result<NativeFrame, FrameProcessorError> {
        let index = self
            .outstanding_frames
            .iter()
            .position(|candidate| candidate.handle == frame.handle)
            .ok_or_else(|| {
                FrameProcessorError::abi_violation(format!(
                    "frame processor plugin `{}` was asked to release an untracked output frame handle",
                    self.factory.name
                ))
            })?;
        Ok(self.outstanding_frames.swap_remove(index))
    }

    fn release_tracked_frame(
        &mut self,
        frame: NativeFrame,
        operation: &'static str,
    ) -> Result<(), FrameProcessorError> {
        let handle_kind = native_handle_kind_code(&frame.metadata.handle_kind)
            .map_err(FrameProcessorError::abi_violation)?;
        // SAFETY: the validated plugin API guarantees `release_frame` is
        // present. The frame handle was previously returned by this same plugin
        // session and tracked by the loader.
        let result = unsafe {
            (self.factory.api.release_frame)(
                self.factory.api.context,
                self.session,
                handle_kind,
                frame.handle,
            )
        };
        self.decode_operation_result(result, operation)
    }

    fn release_outstanding_frames(
        &mut self,
        operation: &'static str,
    ) -> Result<(), FrameProcessorError> {
        let mut first_error = None;
        while let Some(frame) = self.outstanding_frames.pop() {
            let release_result = self.release_tracked_frame(frame.clone(), operation);
            if release_result.is_err() {
                self.outstanding_frames.push(frame);
            }
            if let Err(error) = release_result
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl FrameProcessorSession for DynamicFrameProcessorSession {
    fn session_info(&self) -> FrameProcessorSessionInfo {
        self.session_info.clone()
    }

    fn submit_frame(
        &mut self,
        frame: &NativeFrame,
        submit: &FrameProcessorSubmitFrame,
    ) -> Result<FrameProcessorSubmitResult, FrameProcessorError> {
        self.ensure_open()?;
        let submit_json = serde_json::to_vec(submit).map_err(|error| {
            FrameProcessorError::payload_codec(format!(
                "serialize frame processor submit payload for `{}` failed: {error}",
                self.factory.name
            ))
        })?;

        // SAFETY: the validated plugin API guarantees `submit_frame_json` is
        // present. The JSON buffer remains alive for this synchronous call, and
        // the input frame handle is borrowed only for the duration of the call.
        let result = unsafe {
            (self.factory.api.submit_frame_json)(
                self.factory.api.context,
                self.session,
                submit_json.as_ptr(),
                submit_json.len(),
                frame.handle,
            )
        };

        match result.status {
            VesperPluginResultStatus::Success => {
                decode_plugin_bytes_or_default::<FrameProcessorSubmitResult>(
                    result.payload,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                )
                .map_err(|error| {
                    map_frame_processor_payload_error(&self.factory.name, "submit_frame", error)
                })
            }
            VesperPluginResultStatus::Failure => Err(decode_frame_processor_error_payload(
                result.payload,
                self.factory.api.free_bytes,
                self.factory.api.context,
                &self.factory.name,
                "submit_frame",
            )),
        }
    }

    fn receive_frame(&mut self) -> Result<FrameProcessorReceiveOutput, FrameProcessorError> {
        self.ensure_open()?;
        // SAFETY: the validated plugin API guarantees `receive_frame` is
        // present and returns plugin-owned byte buffers reclaimed below.
        let result =
            unsafe { (self.factory.api.receive_frame)(self.factory.api.context, self.session) };

        match result.status {
            VesperPluginResultStatus::Success => {
                let metadata = decode_plugin_bytes::<FrameProcessorReceiveFrameMetadata>(
                    result.metadata,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                )
                .map_err(|error| {
                    map_frame_processor_payload_error(&self.factory.name, "receive_frame", error)
                })?;
                match metadata.status {
                    FrameProcessorReceiveStatus::Frame => {
                        if result.handle == 0 {
                            return Err(FrameProcessorError::abi_violation(format!(
                                "frame processor plugin `{}` returned frame status with a null handle",
                                self.factory.name
                            )));
                        }
                        let frame_metadata = metadata.frame.ok_or_else(|| {
                            FrameProcessorError::abi_violation(format!(
                                "frame processor plugin `{}` returned frame status without frame metadata",
                                self.factory.name
                            ))
                        })?;
                        let frame = NativeFrame {
                            metadata: frame_metadata,
                            handle: result.handle,
                        };
                        if frame_processor_output_requires_release(&frame) {
                            self.outstanding_frames.push(frame.clone());
                        }
                        Ok(FrameProcessorReceiveOutput::Frame(
                            FrameProcessorOutputFrame {
                                frame,
                                timings: metadata.timings,
                                source_frame_id: metadata.source_frame_id,
                            },
                        ))
                    }
                    FrameProcessorReceiveStatus::Pending => {
                        Ok(FrameProcessorReceiveOutput::Pending)
                    }
                    FrameProcessorReceiveStatus::EndOfStream => {
                        Ok(FrameProcessorReceiveOutput::EndOfStream)
                    }
                }
            }
            VesperPluginResultStatus::Failure => Err(decode_frame_processor_error_payload(
                result.metadata,
                self.factory.api.free_bytes,
                self.factory.api.context,
                &self.factory.name,
                "receive_frame",
            )),
        }
    }

    fn release_frame(&mut self, frame: NativeFrame) -> Result<(), FrameProcessorError> {
        self.ensure_open()?;
        let frame = self.take_outstanding_frame(&frame)?;
        self.release_tracked_frame(frame, "release_frame")
    }

    fn flush(&mut self) -> Result<(), FrameProcessorError> {
        self.ensure_open()?;
        let release_result = self.release_outstanding_frames("release_frame_on_flush");
        // SAFETY: the validated plugin API guarantees `flush_session` is present.
        let result =
            unsafe { (self.factory.api.flush_session)(self.factory.api.context, self.session) };
        let flush_result = self.decode_operation_result(result, "flush");
        release_result.and(flush_result)
    }

    fn close(&mut self) -> Result<(), FrameProcessorError> {
        if self.closed || self.session.is_null() {
            return Ok(());
        }
        let release_result = self.release_outstanding_frames("release_frame_on_close");
        // SAFETY: the validated plugin API guarantees `close_session` is present
        // and consumes or releases the opaque session pointer exactly once.
        let result =
            unsafe { (self.factory.api.close_session)(self.factory.api.context, self.session) };
        self.closed = true;
        self.session = std::ptr::null_mut();
        let close_result = self.decode_operation_result(result, "close");
        release_result.and(close_result)
    }
}

impl Drop for DynamicFrameProcessorSession {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            tracing::error!(
                plugin = %self.factory.name,
                error = %error,
                "frame processor plugin session close failed during drop"
            );
        }
    }
}

fn frame_processor_output_requires_release(frame: &NativeFrame) -> bool {
    frame
        .metadata
        .release_tracking
        .as_ref()
        .is_none_or(|tracking| tracking.requires_release)
}
