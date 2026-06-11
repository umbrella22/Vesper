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
            let _ = catch_decoder_plugin_call(&self.name, "destroy", || unsafe {
                destroy(self.api.context)
            });
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
        if decoder_capabilities_advertise_pcm_frames(&capabilities)
            && api.receive_pcm_frame.is_none()
        {
            return Err(PluginLoadError::CapabilitiesAbiViolation(format!(
                "decoder plugin `{name}` advertises PCM frame output but does not export receive_pcm_frame"
            )));
        }
        if capabilities.supports_presentation_release
            && api.release_native_frame_with_presentation.is_none()
        {
            return Err(PluginLoadError::CapabilitiesAbiViolation(format!(
                "decoder plugin `{name}` advertises presentation-aware native frame release but does not export release_native_frame2"
            )));
        }
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

fn decoder_capabilities_advertise_pcm_frames(capabilities: &DecoderCapabilities) -> bool {
    (capabilities.supports_pcm_frames || capabilities.supports_audio_frames)
        && capabilities.codecs.iter().any(|codec| {
            codec.media_kind == DecoderMediaKind::Audio
                && codec.output_formats.iter().any(|format| {
                    matches!(format, DecoderFrameFormat::F32 | DecoderFrameFormat::S16)
                })
        })
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

    fn supports_native_frame_presentation_release(&self) -> bool {
        self.inner.capabilities.supports_presentation_release
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
        let result = catch_decoder_plugin_call(&self.inner.name, "open_native", || unsafe {
            (self.inner.api.open_session_json)(
                self.inner.api.context,
                config_json.as_ptr(),
                config_json.len(),
            )
        })?;

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
        presented: bool,
    ) -> Result<(), DecoderError> {
        let handle_kind =
            native_handle_kind_code(&NativeHandleKind::from(frame.metadata.handle_kind.clone()))
                .map_err(DecoderError::abi_violation)?;
        let result = if self.factory.capabilities.supports_presentation_release {
            let release_native_frame_with_presentation = self
                .factory
                .api
                .release_native_frame_with_presentation
                .ok_or_else(|| {
                    DecoderError::abi_violation(format!(
                        "native decoder plugin `{}` advertises presentation-aware release but does not export release_native_frame2",
                        self.factory.name
                    ))
                })?;
            // SAFETY: the current decoder ABI callback follows the same synchronous
            // ownership contract as legacy `release_native_frame` and accepts
            // the render/discard decision as an extra ABI-safe bool.
            catch_decoder_plugin_call(&self.factory.name, operation, || unsafe {
                release_native_frame_with_presentation(
                    self.factory.api.context,
                    self.session,
                    handle_kind,
                    frame.handle,
                    presented,
                )
            })
        } else {
            // SAFETY: the validated plugin API guarantees `release_native_frame`
            // is present. The frame handle was returned by this same plugin
            // session and tracked by the loader.
            catch_decoder_plugin_call(&self.factory.name, operation, || unsafe {
                (self.factory.api.release_native_frame)(
                    self.factory.api.context,
                    self.session,
                    handle_kind,
                    frame.handle,
                )
            })
        };
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                self.closed = true;
                return Err(error);
            }
        };
        self.decode_operation_result(result, operation)
    }

    fn release_outstanding_native_frames(
        &mut self,
        operation: &'static str,
    ) -> Result<(), DecoderError> {
        let mut first_error = None;
        while let Some(frame) = self.outstanding_frames.pop() {
            let release_result = self.release_tracked_native_frame(frame.clone(), operation, false);
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
        let result = match catch_decoder_plugin_call(&self.factory.name, "send_packet", || unsafe {
            (self.factory.api.send_packet)(
                self.factory.api.context,
                self.session,
                packet_json.as_ptr(),
                packet_json.len(),
                data_ptr,
                data.len(),
            )
        }) {
            Ok(result) => result,
            Err(error) => {
                self.closed = true;
                return Err(error);
            }
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
        let result =
            match catch_decoder_plugin_call(&self.factory.name, "receive_native_frame", || unsafe {
                (self.factory.api.receive_native_frame)(self.factory.api.context, self.session)
            }) {
                Ok(result) => result,
                Err(error) => {
                    self.closed = true;
                    return Err(error);
                }
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

    fn receive_pcm_frame(&mut self) -> Result<DecoderReceivePcmFrameOutput, DecoderError> {
        self.ensure_open()?;
        let Some(receive_pcm_frame) = self.factory.api.receive_pcm_frame else {
            return Err(DecoderError::UnsupportedCapability {
                capability: "audio-pcm-output".to_owned(),
            });
        };
        // SAFETY: when present, the optional decoder PCM callback follows the
        // same synchronous ownership contract as `receive_native_frame`.
        let result =
            match catch_decoder_plugin_call(&self.factory.name, "receive_pcm_frame", || unsafe {
                receive_pcm_frame(self.factory.api.context, self.session)
            }) {
                Ok(result) => result,
                Err(error) => {
                    self.closed = true;
                    return Err(error);
                }
            };

        match result.status {
            VesperPluginResultStatus::Success => {
                let metadata = match decode_plugin_bytes::<DecoderReceivePcmFrameMetadata>(
                    result.metadata,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                ) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        // Reclaim the plugin-owned PCM buffer before surfacing the
                        // metadata decode failure, mirroring the success paths.
                        reclaim_plugin_payload(
                            result.data,
                            self.factory.api.free_bytes,
                            self.factory.api.context,
                        );
                        return Err(map_decoder_payload_error(
                            &self.factory.name,
                            "receive_pcm_frame",
                            error,
                        ));
                    }
                };
                match metadata.status {
                    DecoderReceiveFrameStatus::Frame => {
                        let frame_metadata = match metadata.frame {
                            Some(frame_metadata) => frame_metadata,
                            None => {
                                reclaim_plugin_payload(
                                    result.data,
                                    self.factory.api.free_bytes,
                                    self.factory.api.context,
                                );
                                return Err(DecoderError::abi_violation(format!(
                                    "native decoder plugin `{}` returned PCM frame status without frame metadata",
                                    self.factory.name
                                )));
                            }
                        };
                        let data = plugin_bytes_into_vec(
                            result.data,
                            self.factory.api.free_bytes,
                            self.factory.api.context,
                        )
                        .map_err(|error| {
                            map_decoder_payload_error(
                                &self.factory.name,
                                "receive_pcm_frame_data",
                                error,
                            )
                        })?;
                        Ok(DecoderReceivePcmFrameOutput::Frame(DecoderPcmFrame {
                            metadata: frame_metadata,
                            data,
                        }))
                    }
                    DecoderReceiveFrameStatus::NeedMoreInput => {
                        reclaim_plugin_payload(
                            result.data,
                            self.factory.api.free_bytes,
                            self.factory.api.context,
                        );
                        Ok(DecoderReceivePcmFrameOutput::NeedMoreInput)
                    }
                    DecoderReceiveFrameStatus::Eof => {
                        reclaim_plugin_payload(
                            result.data,
                            self.factory.api.free_bytes,
                            self.factory.api.context,
                        );
                        Ok(DecoderReceivePcmFrameOutput::Eof)
                    }
                }
            }
            VesperPluginResultStatus::Failure => {
                reclaim_plugin_payload(
                    result.data,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                );
                Err(decode_decoder_error_payload(
                    result.metadata,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                    &self.factory.name,
                    "receive_pcm_frame",
                ))
            }
        }
    }

    fn release_native_frame(&mut self, frame: DecoderNativeFrame) -> Result<(), DecoderError> {
        self.ensure_open()?;
        let frame = self.take_outstanding_native_frame(&frame)?;
        self.release_tracked_native_frame(frame, "release_native_frame", false)
    }

    fn release_native_frame_with_presentation(
        &mut self,
        frame: DecoderNativeFrame,
        presented: bool,
    ) -> Result<(), DecoderError> {
        self.ensure_open()?;
        let frame = self.take_outstanding_native_frame(&frame)?;
        self.release_tracked_native_frame(frame, "release_native_frame", presented)
    }

    fn flush(&mut self) -> Result<(), DecoderError> {
        self.ensure_open()?;
        // SAFETY: the validated plugin API guarantees `flush_session` is present.
        let result = match catch_decoder_plugin_call(&self.factory.name, "flush", || unsafe {
            (self.factory.api.flush_session)(self.factory.api.context, self.session)
        }) {
            Ok(result) => result,
            Err(error) => {
                self.closed = true;
                return Err(error);
            }
        };
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
        let result = match catch_decoder_plugin_call(&self.factory.name, "close", || unsafe {
            (self.factory.api.close_session)(self.factory.api.context, self.session)
        }) {
            Ok(result) => result,
            Err(error) => {
                self.closed = true;
                self.session = std::ptr::null_mut();
                return release_result.and(Err(error));
            }
        };
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
