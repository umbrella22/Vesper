use super::*;

#[derive(Debug)]
pub(crate) struct DynamicNativeDecoderPluginFactoryInner {
    #[allow(dead_code)]
    library: Option<Arc<LibraryHolder>>,
    name: String,
    api: CheckedNativeDecoderPluginApi,
    capabilities: DecoderCapabilities,
    native_requirements: DecoderNativeRequirements,
}

impl Drop for DynamicNativeDecoderPluginFactoryInner {
    fn drop(&mut self) {
        if let Some(destroy) = self.api.destroy {
            // SAFETY: `destroy` and `context` come from the validated plugin ABI
            // table and are only invoked once when this wrapper is dropped.
            unsafe { destroy(self.api.context) };
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DynamicNativeDecoderPluginFactory {
    inner: Arc<DynamicNativeDecoderPluginFactoryInner>,
}

impl DynamicNativeDecoderPluginFactory {
    pub(crate) fn new(
        library: Option<Arc<LibraryHolder>>,
        fallback_name: String,
        api: CheckedNativeDecoderPluginApi,
    ) -> Result<Self, PluginLoadError> {
        let name = if let Some(name_fn) = api.name {
            // SAFETY: the plugin ABI declares `name_fn` with `api.context`, and
            // the returned pointer is interpreted immediately as an optional
            // NUL-terminated UTF-8 string.
            let name_ptr = unsafe { name_fn(api.context) };
            if name_ptr.is_null() {
                fallback_name
            } else {
                c_string_field(name_ptr, "decoder_name")?
            }
        } else {
            fallback_name
        };
        let capabilities = decode_plugin_bytes::<DecoderCapabilities>(
            // SAFETY: the validated API guarantees `capabilities_json` and
            // `free_bytes` are present and use the shared `VesperPluginBytes`
            // ownership contract documented in `player-plugin`.
            unsafe { (api.capabilities_json)(api.context) },
            api.free_bytes,
            api.context,
        )
        .map_err(map_capabilities_payload_error)?;
        let native_requirements = decode_plugin_bytes::<DecoderNativeRequirements>(
            // SAFETY: the validated API guarantees `native_requirements_json`
            // and `free_bytes` are present and use the shared bytes ownership
            // contract documented in `player-plugin`.
            unsafe { (api.native_requirements_json)(api.context) },
            api.free_bytes,
            api.context,
        )
        .map_err(map_capabilities_payload_error)?;

        Ok(Self {
            inner: Arc::new(DynamicNativeDecoderPluginFactoryInner {
                library,
                name,
                api,
                capabilities,
                native_requirements,
            }),
        })
    }
}

impl NativeDecoderPluginFactory for DynamicNativeDecoderPluginFactory {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn capabilities(&self) -> DecoderCapabilities {
        self.inner.capabilities.clone()
    }

    fn native_requirements(&self) -> DecoderNativeRequirements {
        self.inner.native_requirements.clone()
    }

    fn open_native_session(
        &self,
        config: &DecoderSessionConfig,
    ) -> Result<Box<dyn NativeDecoderSession>, DecoderError> {
        let config_json = serde_json::to_vec(config).map_err(|error| {
            DecoderError::payload_codec(format!(
                "serialize native decoder config for `{}` failed: {error}",
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
                    return Err(DecoderError::abi_violation(format!(
                        "native decoder plugin `{}` returned a null session pointer",
                        self.inner.name
                    )));
                }
                let session_info = decode_plugin_bytes_or_default::<DecoderSessionInfo>(
                    result.payload,
                    self.inner.api.free_bytes,
                    self.inner.api.context,
                )
                .map_err(|error| {
                    map_decoder_payload_error(&self.inner.name, "open_native", error)
                })?;
                Ok(Box::new(DynamicNativeDecoderSession {
                    factory: self.inner.clone(),
                    session: result.session,
                    session_info,
                    closed: false,
                    outstanding_frames: Vec::new(),
                }))
            }
            VesperPluginResultStatus::Failure => {
                let error = decode_decoder_error_payload(
                    result.payload,
                    self.inner.api.free_bytes,
                    self.inner.api.context,
                    &self.inner.name,
                    "open_native",
                );
                Err(error)
            }
        }
    }
}

#[derive(Debug)]
struct DynamicNativeDecoderSession {
    factory: Arc<DynamicNativeDecoderPluginFactoryInner>,
    session: *mut c_void,
    session_info: DecoderSessionInfo,
    closed: bool,
    outstanding_frames: Vec<DecoderNativeFrame>,
}

// SAFETY: the dynamic native decoder session is only exposed through
// `NativeDecoderSession: Send`; the plugin ABI requires the opaque session
// pointer to be safe to move across threads when exported through this API.
unsafe impl Send for DynamicNativeDecoderSession {}

impl DynamicNativeDecoderSession {
    fn ensure_open(&self) -> Result<(), DecoderError> {
        if self.closed || self.session.is_null() {
            Err(DecoderError::NotConfigured)
        } else {
            Ok(())
        }
    }

    fn decode_operation_result(
        &self,
        result: VesperPluginProcessResult,
        operation: &'static str,
    ) -> Result<(), DecoderError> {
        match result.status {
            VesperPluginResultStatus::Success => {
                let _ = decode_plugin_bytes_or_default::<DecoderOperationStatus>(
                    result.payload,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                )
                .map_err(|error| map_decoder_payload_error(&self.factory.name, operation, error))?;
                Ok(())
            }
            VesperPluginResultStatus::Failure => Err(decode_decoder_error_payload(
                result.payload,
                self.factory.api.free_bytes,
                self.factory.api.context,
                &self.factory.name,
                operation,
            )),
        }
    }

    fn take_outstanding_native_frame(
        &mut self,
        frame: &DecoderNativeFrame,
    ) -> Result<DecoderNativeFrame, DecoderError> {
        let index = self
            .outstanding_frames
            .iter()
            .position(|candidate| candidate.handle == frame.handle)
            .ok_or_else(|| {
                DecoderError::abi_violation(format!(
                    "native decoder plugin `{}` was asked to release an untracked native frame handle",
                    self.factory.name
                ))
            })?;
        Ok(self.outstanding_frames.swap_remove(index))
    }

    fn release_tracked_native_frame(
        &mut self,
        frame: DecoderNativeFrame,
        operation: &'static str,
    ) -> Result<(), DecoderError> {
        let handle_kind =
            native_handle_kind_code(&NativeHandleKind::from(frame.metadata.handle_kind.clone()))
                .map_err(DecoderError::abi_violation)?;
        // SAFETY: the validated plugin API guarantees `release_native_frame` is
        // present. The frame handle was previously returned by this same plugin
        // session and tracked by the loader.
        let result = unsafe {
            (self.factory.api.release_native_frame)(
                self.factory.api.context,
                self.session,
                handle_kind,
                frame.handle,
            )
        };
        self.decode_operation_result(result, operation)
    }

    fn release_outstanding_native_frames(
        &mut self,
        operation: &'static str,
    ) -> Result<(), DecoderError> {
        let mut first_error = None;
        while let Some(frame) = self.outstanding_frames.pop() {
            let release_result = self.release_tracked_native_frame(frame.clone(), operation);
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

impl NativeDecoderSession for DynamicNativeDecoderSession {
    fn session_info(&self) -> DecoderSessionInfo {
        self.session_info.clone()
    }

    fn send_packet(
        &mut self,
        packet: &DecoderPacket,
        data: &[u8],
    ) -> Result<DecoderPacketResult, DecoderError> {
        self.ensure_open()?;
        let packet_json = serde_json::to_vec(packet).map_err(|error| {
            DecoderError::payload_codec(format!(
                "serialize native decoder packet for `{}` failed: {error}",
                self.factory.name
            ))
        })?;
        let data_ptr = if data.is_empty() {
            std::ptr::null()
        } else {
            data.as_ptr()
        };

        // SAFETY: the validated plugin API guarantees `send_packet` is present.
        // The JSON and packet data buffers remain alive for this synchronous call.
        let result = unsafe {
            (self.factory.api.send_packet)(
                self.factory.api.context,
                self.session,
                packet_json.as_ptr(),
                packet_json.len(),
                data_ptr,
                data.len(),
            )
        };

        match result.status {
            VesperPluginResultStatus::Success => decode_plugin_bytes_or_default::<
                DecoderPacketResult,
            >(
                result.payload,
                self.factory.api.free_bytes,
                self.factory.api.context,
            )
            .map_err(|error| map_decoder_payload_error(&self.factory.name, "send_packet", error)),
            VesperPluginResultStatus::Failure => Err(decode_decoder_error_payload(
                result.payload,
                self.factory.api.free_bytes,
                self.factory.api.context,
                &self.factory.name,
                "send_packet",
            )),
        }
    }

    fn receive_native_frame(&mut self) -> Result<DecoderReceiveNativeFrameOutput, DecoderError> {
        self.ensure_open()?;
        // SAFETY: the validated plugin API guarantees `receive_native_frame` is
        // present and returns plugin-owned byte buffers reclaimed below.
        let result = unsafe {
            (self.factory.api.receive_native_frame)(self.factory.api.context, self.session)
        };

        match result.status {
            VesperPluginResultStatus::Success => {
                let metadata = decode_plugin_bytes::<DecoderReceiveNativeFrameMetadata>(
                    result.metadata,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                )
                .map_err(|error| {
                    map_decoder_payload_error(&self.factory.name, "receive_native_frame", error)
                })?;
                match metadata.status {
                    DecoderReceiveFrameStatus::Frame => {
                        if result.handle == 0 {
                            return Err(DecoderError::abi_violation(format!(
                                "native decoder plugin `{}` returned frame status with a null handle",
                                self.factory.name
                            )));
                        }
                        let frame = metadata.frame.ok_or_else(|| {
                            DecoderError::abi_violation(format!(
                                "native decoder plugin `{}` returned frame status without frame metadata",
                                self.factory.name
                            ))
                        })?;
                        let frame = DecoderNativeFrame {
                            metadata: frame,
                            handle: result.handle,
                        };
                        self.outstanding_frames.push(frame.clone());
                        Ok(DecoderReceiveNativeFrameOutput::Frame(frame))
                    }
                    DecoderReceiveFrameStatus::NeedMoreInput => {
                        Ok(DecoderReceiveNativeFrameOutput::NeedMoreInput)
                    }
                    DecoderReceiveFrameStatus::Eof => Ok(DecoderReceiveNativeFrameOutput::Eof),
                }
            }
            VesperPluginResultStatus::Failure => Err(decode_decoder_error_payload(
                result.metadata,
                self.factory.api.free_bytes,
                self.factory.api.context,
                &self.factory.name,
                "receive_native_frame",
            )),
        }
    }

    fn release_native_frame(&mut self, frame: DecoderNativeFrame) -> Result<(), DecoderError> {
        self.ensure_open()?;
        let frame = self.take_outstanding_native_frame(&frame)?;
        self.release_tracked_native_frame(frame, "release_native_frame")
    }

    fn flush(&mut self) -> Result<(), DecoderError> {
        self.ensure_open()?;
        // SAFETY: the validated plugin API guarantees `flush_session` is present.
        let result =
            unsafe { (self.factory.api.flush_session)(self.factory.api.context, self.session) };
        self.decode_operation_result(result, "flush")
    }

    fn close(&mut self) -> Result<(), DecoderError> {
        if self.closed || self.session.is_null() {
            return Ok(());
        }
        let release_result =
            self.release_outstanding_native_frames("release_native_frame_on_close");
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

impl Drop for DynamicNativeDecoderSession {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            tracing::error!(
                plugin = %self.factory.name,
                error = %error,
                "native decoder plugin session close failed during drop"
            );
        }
    }
}
