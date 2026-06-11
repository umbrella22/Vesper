use std::time::Duration;

use super::*;

#[derive(Debug)]
pub(crate) struct DynamicSourceNormalizerPacketPluginFactoryInner {
    // Held so the dynamic library stays loaded for the whole factory/session lifetime.
    _library_holder: Option<Arc<LibraryHolder>>,
    name: String,
    api: CheckedSourceNormalizerPacketPluginApi,
    capabilities: SourceNormalizerPacketCapabilities,
}

impl Drop for DynamicSourceNormalizerPacketPluginFactoryInner {
    fn drop(&mut self) {
        if let Some(destroy) = self.api.destroy {
            // SAFETY: `destroy` and `context` come from the validated plugin ABI
            // table and are only invoked once when this wrapper is dropped.
            let _ = catch_source_normalizer_plugin_call(&self.name, "destroy", || unsafe {
                destroy(self.api.context)
            });
        }
    }
}

#[derive(Debug)]
pub(crate) struct DynamicSourceNormalizerResourcePluginFactoryInner {
    // Held so the dynamic library stays loaded for the whole factory/session lifetime.
    _library_holder: Option<Arc<LibraryHolder>>,
    name: String,
    api: CheckedSourceNormalizerResourcePluginApi,
    capabilities: SourceNormalizerResourceCapabilities,
}

impl Drop for DynamicSourceNormalizerResourcePluginFactoryInner {
    fn drop(&mut self) {
        if let Some(destroy) = self.api.destroy {
            // SAFETY: `destroy` and `context` come from the validated plugin ABI
            // table and are only invoked once when this wrapper is dropped.
            let _ = catch_source_normalizer_plugin_call(&self.name, "destroy", || unsafe {
                destroy(self.api.context)
            });
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DynamicSourceNormalizerPacketPluginFactory {
    inner: Arc<DynamicSourceNormalizerPacketPluginFactoryInner>,
}

#[derive(Debug, Clone)]
pub(crate) struct DynamicSourceNormalizerResourcePluginFactory {
    inner: Arc<DynamicSourceNormalizerResourcePluginFactoryInner>,
}

impl DynamicSourceNormalizerPacketPluginFactory {
    pub(crate) fn new(
        library: Option<Arc<LibraryHolder>>,
        fallback_name: String,
        api: CheckedSourceNormalizerPacketPluginApi,
    ) -> Result<Self, PluginLoadError> {
        let name = if let Some(name_fn) = api.name {
            // SAFETY: the plugin ABI declares `name_fn` with `api.context`, and
            // the returned pointer is interpreted immediately as an optional
            // NUL-terminated UTF-8 string.
            let name_ptr = unsafe { name_fn(api.context) };
            if name_ptr.is_null() {
                fallback_name
            } else {
                c_string_field(name_ptr, "source_normalizer_packet_name")?
            }
        } else {
            fallback_name
        };
        let capabilities = decode_plugin_bytes::<SourceNormalizerPacketCapabilities>(
            // SAFETY: the validated API guarantees `packet_capabilities_json`
            // and `free_bytes` are present.
            catch_source_normalizer_plugin_call(&name, "packet_capabilities_json", || unsafe {
                (api.packet_capabilities_json)(api.context)
            })
            .map_err(|error| PluginLoadError::CapabilitiesAbiViolation(error.to_string()))?,
            api.free_bytes,
            api.context,
        )
        .map_err(map_capabilities_payload_error)?;

        Ok(Self {
            inner: Arc::new(DynamicSourceNormalizerPacketPluginFactoryInner {
                _library_holder: library,
                name,
                api,
                capabilities,
            }),
        })
    }
}

impl DynamicSourceNormalizerResourcePluginFactory {
    pub(crate) fn new(
        library: Option<Arc<LibraryHolder>>,
        fallback_name: String,
        api: CheckedSourceNormalizerResourcePluginApi,
    ) -> Result<Self, PluginLoadError> {
        let name = if let Some(name_fn) = api.name {
            // SAFETY: the plugin ABI declares `name_fn` with `api.context`, and
            // the returned pointer is interpreted immediately as an optional
            // NUL-terminated UTF-8 string.
            let name_ptr = unsafe { name_fn(api.context) };
            if name_ptr.is_null() {
                fallback_name
            } else {
                c_string_field(name_ptr, "source_normalizer_resource_name")?
            }
        } else {
            fallback_name
        };
        let capabilities = decode_plugin_bytes::<SourceNormalizerResourceCapabilities>(
            // SAFETY: the validated API guarantees `resource_capabilities_json`
            // and `free_bytes` are present.
            catch_source_normalizer_plugin_call(&name, "resource_capabilities_json", || unsafe {
                (api.resource_capabilities_json)(api.context)
            })
            .map_err(|error| PluginLoadError::CapabilitiesAbiViolation(error.to_string()))?,
            api.free_bytes,
            api.context,
        )
        .map_err(map_capabilities_payload_error)?;

        Ok(Self {
            inner: Arc::new(DynamicSourceNormalizerResourcePluginFactoryInner {
                _library_holder: library,
                name,
                api,
                capabilities,
            }),
        })
    }
}

impl SourceNormalizerPacketPluginFactory for DynamicSourceNormalizerPacketPluginFactory {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn packet_capabilities(&self) -> SourceNormalizerPacketCapabilities {
        self.inner.capabilities.clone()
    }

    fn open_packet_session(
        &self,
        config: &SourceNormalizerPacketSessionConfig,
    ) -> Result<Box<dyn SourceNormalizerPacketSession>, SourceNormalizerError> {
        let config_json = serde_json::to_vec(config).map_err(|error| {
            SourceNormalizerError::payload_codec(format!(
                "serialize source normalizer packet config for `{}` failed: {error}",
                self.inner.name
            ))
        })?;

        // SAFETY: the validated plugin API guarantees
        // `open_packet_session_json` is present, and `config_json` remains
        // alive for the duration of this synchronous callback.
        let result = catch_source_normalizer_plugin_call(
            &self.inner.name,
            "open_packet_session",
            || unsafe {
                (self.inner.api.open_packet_session_json)(
                    self.inner.api.context,
                    config_json.as_ptr(),
                    config_json.len(),
                )
            },
        )?;

        match result.status {
            VesperPluginResultStatus::Success => {
                if result.session.is_null() {
                    reclaim_plugin_payload(
                        result.payload,
                        self.inner.api.free_bytes,
                        self.inner.api.context,
                    );
                    return Err(SourceNormalizerError::abi_violation(format!(
                        "source normalizer packet plugin `{}` returned a null session pointer",
                        self.inner.name
                    )));
                }
                let stream_info = decode_plugin_bytes::<SourceNormalizerPacketStreamInfo>(
                    result.payload,
                    self.inner.api.free_bytes,
                    self.inner.api.context,
                )
                .map_err(|error| {
                    map_source_normalizer_payload_error(
                        &self.inner.name,
                        "open_packet_session",
                        error,
                    )
                })?;
                Ok(Box::new(DynamicSourceNormalizerPacketSession {
                    factory: self.inner.clone(),
                    session: result.session,
                    stream_info,
                    outstanding_packet: None,
                    closed: false,
                }))
            }
            VesperPluginResultStatus::Failure => Err(decode_source_normalizer_error_payload(
                result.payload,
                self.inner.api.free_bytes,
                self.inner.api.context,
                &self.inner.name,
                "open_packet_session",
            )),
        }
    }
}

impl SourceNormalizerResourcePluginFactory for DynamicSourceNormalizerResourcePluginFactory {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn resource_capabilities(&self) -> SourceNormalizerResourceCapabilities {
        self.inner.capabilities.clone()
    }

    fn open_resource_session(
        &self,
        config: &SourceNormalizerResourceSessionConfig,
    ) -> Result<Box<dyn SourceNormalizerResourceSession>, SourceNormalizerError> {
        let config_json = serde_json::to_vec(config).map_err(|error| {
            SourceNormalizerError::payload_codec(format!(
                "serialize source normalizer resource config for `{}` failed: {error}",
                self.inner.name
            ))
        })?;

        // SAFETY: the validated plugin API guarantees
        // `open_resource_session_json` is present, and `config_json` remains
        // alive for the duration of this synchronous callback.
        let result = catch_source_normalizer_plugin_call(
            &self.inner.name,
            "open_resource_session",
            || unsafe {
                (self.inner.api.open_resource_session_json)(
                    self.inner.api.context,
                    config_json.as_ptr(),
                    config_json.len(),
                )
            },
        )?;

        match result.status {
            VesperPluginResultStatus::Success => {
                if result.session.is_null() {
                    reclaim_plugin_payload(
                        result.payload,
                        self.inner.api.free_bytes,
                        self.inner.api.context,
                    );
                    return Err(SourceNormalizerError::abi_violation(format!(
                        "source normalizer resource plugin `{}` returned a null session pointer",
                        self.inner.name
                    )));
                }
                let session_info = decode_plugin_bytes::<SourceNormalizerResourceSessionInfo>(
                    result.payload,
                    self.inner.api.free_bytes,
                    self.inner.api.context,
                )
                .map_err(|error| {
                    map_source_normalizer_payload_error(
                        &self.inner.name,
                        "open_resource_session",
                        error,
                    )
                })?;
                Ok(Box::new(DynamicSourceNormalizerResourceSession {
                    factory: self.inner.clone(),
                    session: result.session,
                    session_info,
                    closed: false,
                }))
            }
            VesperPluginResultStatus::Failure => Err(decode_source_normalizer_error_payload(
                result.payload,
                self.inner.api.free_bytes,
                self.inner.api.context,
                &self.inner.name,
                "open_resource_session",
            )),
        }
    }
}

#[derive(Debug)]
struct DynamicSourceNormalizerPacketSession {
    factory: Arc<DynamicSourceNormalizerPacketPluginFactoryInner>,
    session: *mut c_void,
    stream_info: SourceNormalizerPacketStreamInfo,
    outstanding_packet: Option<usize>,
    closed: bool,
}

// SAFETY: the dynamic source normalizer packet session is only exposed through
// `SourceNormalizerPacketSession: Send`; the plugin ABI requires the opaque
// session pointer to be safe to move across threads when exported through this
// API.
unsafe impl Send for DynamicSourceNormalizerPacketSession {}

#[derive(Debug)]
struct DynamicSourceNormalizerResourceSession {
    factory: Arc<DynamicSourceNormalizerResourcePluginFactoryInner>,
    session: *mut c_void,
    session_info: SourceNormalizerResourceSessionInfo,
    closed: bool,
}

// SAFETY: the dynamic source normalizer resource session is only exposed
// through `SourceNormalizerResourceSession: Send`; the plugin ABI requires the
// opaque session pointer to be safe to move across threads when exported
// through this API.
unsafe impl Send for DynamicSourceNormalizerResourceSession {}

impl DynamicSourceNormalizerPacketSession {
    fn ensure_open(&self) -> Result<(), SourceNormalizerError> {
        if self.closed || self.session.is_null() {
            Err(SourceNormalizerError::NotConfigured)
        } else {
            Ok(())
        }
    }

    fn decode_operation_result(
        &self,
        result: VesperPluginProcessResult,
        operation: &'static str,
    ) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        match result.status {
            VesperPluginResultStatus::Success => {
                decode_plugin_bytes_or_default::<SourceNormalizerOperationStatus>(
                    result.payload,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                )
                .map_err(|error| {
                    map_source_normalizer_payload_error(&self.factory.name, operation, error)
                })
            }
            VesperPluginResultStatus::Failure => Err(decode_source_normalizer_error_payload(
                result.payload,
                self.factory.api.free_bytes,
                self.factory.api.context,
                &self.factory.name,
                operation,
            )),
        }
    }

    fn release_outstanding_packet(
        &mut self,
        operation: &'static str,
    ) -> Result<(), SourceNormalizerError> {
        let Some(packet_handle) = self.outstanding_packet.take() else {
            return Ok(());
        };

        // SAFETY: `release_packet` is present in the validated v2 API and the
        // handle was returned by this same session from a successful read.
        let result =
            match catch_source_normalizer_plugin_call(&self.factory.name, operation, || unsafe {
                (self.factory.api.release_packet)(
                    self.factory.api.context,
                    self.session,
                    packet_handle,
                )
            }) {
                Ok(result) => result,
                Err(error) => {
                    self.closed = true;
                    return Err(error);
                }
            };
        self.decode_operation_result(result, operation).map(|_| ())
    }

    fn release_packet_result(
        &self,
        packet_handle: usize,
    ) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        // SAFETY: `release_packet` is present in the validated v2 API and this
        // method is only called for a handle currently tracked by this session.
        let result = match catch_source_normalizer_plugin_call(
            &self.factory.name,
            "release_packet",
            || unsafe {
                (self.factory.api.release_packet)(
                    self.factory.api.context,
                    self.session,
                    packet_handle,
                )
            },
        ) {
            Ok(result) => result,
            Err(error) => return Err(error),
        };
        self.decode_operation_result(result, "release_packet")
    }

    fn reclaim_unexpected_packet_handle(&self, packet_handle: usize) {
        if packet_handle == 0 || self.session.is_null() {
            return;
        }
        // SAFETY: this is best-effort cleanup for an ABI-violating result that
        // still returned a plugin-owned handle.
        let result = catch_source_normalizer_plugin_call(
            &self.factory.name,
            "release_unexpected_packet",
            || unsafe {
                (self.factory.api.release_packet)(
                    self.factory.api.context,
                    self.session,
                    packet_handle,
                )
            },
        );
        let Ok(result) = result else {
            return;
        };
        reclaim_plugin_payload(
            result.payload,
            self.factory.api.free_bytes,
            self.factory.api.context,
        );
    }
}

impl DynamicSourceNormalizerResourceSession {
    fn ensure_open(&self) -> Result<(), SourceNormalizerError> {
        if self.closed || self.session.is_null() {
            Err(SourceNormalizerError::NotConfigured)
        } else {
            Ok(())
        }
    }

    fn decode_operation_result(
        &self,
        result: VesperPluginProcessResult,
        operation: &'static str,
    ) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        match result.status {
            VesperPluginResultStatus::Success => {
                decode_plugin_bytes_or_default::<SourceNormalizerOperationStatus>(
                    result.payload,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                )
                .map_err(|error| {
                    map_source_normalizer_payload_error(&self.factory.name, operation, error)
                })
            }
            VesperPluginResultStatus::Failure => Err(decode_source_normalizer_error_payload(
                result.payload,
                self.factory.api.free_bytes,
                self.factory.api.context,
                &self.factory.name,
                operation,
            )),
        }
    }
}

impl SourceNormalizerPacketSession for DynamicSourceNormalizerPacketSession {
    fn stream_info(&self) -> SourceNormalizerPacketStreamInfo {
        self.stream_info.clone()
    }

    fn read_packet(&mut self) -> Result<SourceNormalizerPacketLease<'_>, SourceNormalizerError> {
        self.ensure_open()?;
        if let Some(packet_handle) = self.outstanding_packet {
            return Err(SourceNormalizerError::abi_violation(format!(
                "source normalizer packet plugin `{}` still has unreleased packet handle {}",
                self.factory.name, packet_handle
            )));
        }

        // SAFETY: the validated plugin API guarantees `read_packet` is present
        // and returns metadata bytes reclaimed below. Packet bytes stay valid
        // until `release_packet` is called for the returned handle.
        let result = match catch_source_normalizer_plugin_call(
            &self.factory.name,
            "read_packet",
            || unsafe { (self.factory.api.read_packet)(self.factory.api.context, self.session) },
        ) {
            Ok(result) => result,
            Err(error) => {
                self.closed = true;
                return Err(error);
            }
        };

        match result.status {
            VesperPluginResultStatus::Success => {
                let metadata = decode_plugin_bytes::<SourceNormalizerReadPacketMetadata>(
                    result.metadata,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                )
                .map_err(|error| {
                    map_source_normalizer_payload_error(&self.factory.name, "read_packet", error)
                })?;

                if metadata.status != SourceNormalizerReadPacketStatus::Packet {
                    if !result.data.is_null() || result.data_len != 0 || result.packet_handle != 0 {
                        self.reclaim_unexpected_packet_handle(result.packet_handle);
                        return Err(SourceNormalizerError::abi_violation(format!(
                            "source normalizer packet plugin `{}` returned packet bytes for {:?}",
                            self.factory.name, metadata.status
                        )));
                    }
                    return Ok(SourceNormalizerPacketLease {
                        metadata,
                        data: &[],
                        handle: 0,
                    });
                }

                if metadata.packet.is_none() {
                    self.reclaim_unexpected_packet_handle(result.packet_handle);
                    return Err(SourceNormalizerError::abi_violation(format!(
                        "source normalizer packet plugin `{}` returned Packet status without packet metadata",
                        self.factory.name
                    )));
                }
                if result.packet_handle == 0 {
                    return Err(SourceNormalizerError::abi_violation(format!(
                        "source normalizer packet plugin `{}` returned Packet status without a packet handle",
                        self.factory.name
                    )));
                }
                if result.data.is_null() && result.data_len > 0 {
                    self.reclaim_unexpected_packet_handle(result.packet_handle);
                    return Err(SourceNormalizerError::abi_violation(format!(
                        "source normalizer packet plugin `{}` returned null packet data with len {}",
                        self.factory.name, result.data_len
                    )));
                }

                self.outstanding_packet = Some(result.packet_handle);
                let data = if result.data_len == 0 {
                    &[]
                } else {
                    // SAFETY: the plugin returned a successful packet lease. The
                    // byte range remains valid until this loader calls
                    // `release_packet` for `result.packet_handle`.
                    unsafe { std::slice::from_raw_parts(result.data, result.data_len) }
                };
                Ok(SourceNormalizerPacketLease {
                    metadata,
                    data,
                    handle: result.packet_handle,
                })
            }
            VesperPluginResultStatus::Failure => {
                if result.packet_handle != 0 {
                    self.reclaim_unexpected_packet_handle(result.packet_handle);
                }
                Err(decode_source_normalizer_error_payload(
                    result.metadata,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                    &self.factory.name,
                    "read_packet",
                ))
            }
        }
    }

    fn release_packet(&mut self, packet_handle: usize) -> Result<(), SourceNormalizerError> {
        self.ensure_open()?;
        match self.outstanding_packet {
            Some(outstanding) if outstanding == packet_handle => {
                self.release_packet_result(packet_handle)?;
                self.outstanding_packet = None;
                Ok(())
            }
            Some(outstanding) => Err(SourceNormalizerError::abi_violation(format!(
                "source normalizer packet plugin `{}` tried to release packet handle {}, but {} is outstanding",
                self.factory.name, packet_handle, outstanding
            ))),
            None => Err(SourceNormalizerError::abi_violation(format!(
                "source normalizer packet plugin `{}` has no outstanding packet handle to release",
                self.factory.name
            ))),
        }
    }

    fn seek(
        &mut self,
        seek: &SourceNormalizerPacketSeek,
    ) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        self.ensure_open()?;
        self.release_outstanding_packet("release_packet_on_seek")?;
        let Some(seek_packet_session_json) = self.factory.api.seek_packet_session_json else {
            return Err(SourceNormalizerError::unsupported_operation("seek"));
        };
        let seek_json = serde_json::to_vec(seek).map_err(|error| {
            SourceNormalizerError::payload_codec(format!(
                "serialize source normalizer packet seek for `{}` failed: {error}",
                self.factory.name
            ))
        })?;

        // SAFETY: the optional seek callback comes from the validated v2 API and
        // the JSON buffer remains alive for the synchronous call.
        let result = match catch_source_normalizer_plugin_call(
            &self.factory.name,
            "seek_packet",
            || unsafe {
                seek_packet_session_json(
                    self.factory.api.context,
                    self.session,
                    seek_json.as_ptr(),
                    seek_json.len(),
                )
            },
        ) {
            Ok(result) => result,
            Err(error) => {
                self.closed = true;
                return Err(error);
            }
        };
        self.decode_operation_result(result, "seek_packet")
    }

    fn flush(&mut self) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        self.ensure_open()?;
        self.release_outstanding_packet("release_packet_on_flush")?;
        // SAFETY: the validated plugin API guarantees `flush_packet_session` is
        // present for packet v2 sessions.
        let result = match catch_source_normalizer_plugin_call(
            &self.factory.name,
            "flush_packet",
            || unsafe {
                (self.factory.api.flush_packet_session)(self.factory.api.context, self.session)
            },
        ) {
            Ok(result) => result,
            Err(error) => {
                self.closed = true;
                return Err(error);
            }
        };
        self.decode_operation_result(result, "flush_packet")
    }

    fn close(&mut self) -> Result<(), SourceNormalizerError> {
        if self.closed || self.session.is_null() {
            return Ok(());
        }
        let release_result = self.release_outstanding_packet("release_packet_on_close");
        // SAFETY: the validated plugin API guarantees `close_packet_session` is
        // present and consumes or releases the opaque session pointer exactly
        // once.
        let result = match catch_source_normalizer_plugin_call(
            &self.factory.name,
            "close_packet",
            || unsafe {
                (self.factory.api.close_packet_session)(self.factory.api.context, self.session)
            },
        ) {
            Ok(result) => result,
            Err(error) => {
                self.closed = true;
                self.session = std::ptr::null_mut();
                return release_result.and(Err(error));
            }
        };
        self.closed = true;
        self.session = std::ptr::null_mut();
        let close_result = self
            .decode_operation_result(result, "close_packet")
            .map(|_| ());
        release_result.and(close_result)
    }
}

impl SourceNormalizerResourceSession for DynamicSourceNormalizerResourceSession {
    fn session_info(&self) -> SourceNormalizerResourceSessionInfo {
        self.session_info.clone()
    }

    fn poll(&mut self) -> Result<SourceNormalizerResourceSessionStatus, SourceNormalizerError> {
        self.ensure_open()?;
        // SAFETY: the validated plugin API guarantees `poll_resource_session`
        // is present and returns a JSON status payload reclaimed below.
        let result = match catch_source_normalizer_plugin_call(
            &self.factory.name,
            "poll_resource",
            || unsafe {
                (self.factory.api.poll_resource_session)(self.factory.api.context, self.session)
            },
        ) {
            Ok(result) => result,
            Err(error) => {
                self.closed = true;
                return Err(error);
            }
        };
        match result.status {
            VesperPluginResultStatus::Success => {
                decode_plugin_bytes::<SourceNormalizerResourceSessionStatus>(
                    result.payload,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                )
                .map_err(|error| {
                    map_source_normalizer_payload_error(&self.factory.name, "poll_resource", error)
                })
            }
            VesperPluginResultStatus::Failure => Err(decode_source_normalizer_error_payload(
                result.payload,
                self.factory.api.free_bytes,
                self.factory.api.context,
                &self.factory.name,
                "poll_resource",
            )),
        }
    }

    fn wait_for_update(
        &mut self,
        timeout: Duration,
    ) -> Result<SourceNormalizerResourceSessionWaitStatus, SourceNormalizerError> {
        self.ensure_open()?;
        let timeout_ms = duration_to_plugin_timeout_millis(timeout);
        // SAFETY: the validated v4 plugin API guarantees
        // `wait_resource_session_update` is present for resource sessions and
        // returns a JSON wait-status payload reclaimed below.
        let result = match catch_source_normalizer_plugin_call(
            &self.factory.name,
            "wait_resource_update",
            || unsafe {
                (self.factory.api.wait_resource_session_update)(
                    self.factory.api.context,
                    self.session,
                    timeout_ms,
                )
            },
        ) {
            Ok(result) => result,
            Err(error) => {
                self.closed = true;
                return Err(error);
            }
        };
        match result.status {
            VesperPluginResultStatus::Success => {
                decode_plugin_bytes::<SourceNormalizerResourceSessionWaitStatus>(
                    result.payload,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                )
                .map_err(|error| {
                    map_source_normalizer_payload_error(
                        &self.factory.name,
                        "wait_resource_update",
                        error,
                    )
                })
            }
            VesperPluginResultStatus::Failure => Err(decode_source_normalizer_error_payload(
                result.payload,
                self.factory.api.free_bytes,
                self.factory.api.context,
                &self.factory.name,
                "wait_resource_update",
            )),
        }
    }

    fn cancel(&mut self) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        self.ensure_open()?;
        // SAFETY: the validated plugin API guarantees `cancel_resource_session`
        // is present for resource sessions.
        let result = match catch_source_normalizer_plugin_call(
            &self.factory.name,
            "cancel_resource",
            || unsafe {
                (self.factory.api.cancel_resource_session)(self.factory.api.context, self.session)
            },
        ) {
            Ok(result) => result,
            Err(error) => {
                self.closed = true;
                return Err(error);
            }
        };
        self.decode_operation_result(result, "cancel_resource")
    }

    fn close(&mut self) -> Result<(), SourceNormalizerError> {
        if self.closed || self.session.is_null() {
            return Ok(());
        }
        // SAFETY: the validated plugin API guarantees `close_resource_session`
        // is present and consumes or releases the opaque session pointer exactly
        // once.
        let result = match catch_source_normalizer_plugin_call(
            &self.factory.name,
            "close_resource",
            || unsafe {
                (self.factory.api.close_resource_session)(self.factory.api.context, self.session)
            },
        ) {
            Ok(result) => result,
            Err(error) => {
                self.closed = true;
                self.session = std::ptr::null_mut();
                return Err(error);
            }
        };
        self.closed = true;
        self.session = std::ptr::null_mut();
        self.decode_operation_result(result, "close_resource")
            .map(|_| ())
    }
}

impl Drop for DynamicSourceNormalizerPacketSession {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            tracing::error!(
                plugin = %self.factory.name,
                error = %error,
                "source normalizer packet plugin session close failed during drop"
            );
        }
    }
}

fn duration_to_plugin_timeout_millis(timeout: Duration) -> u64 {
    if timeout.is_zero() {
        return 0;
    }
    let millis = timeout.as_millis();
    if millis == 0 {
        return 1;
    }
    u64::try_from(millis).unwrap_or(u64::MAX)
}

impl Drop for DynamicSourceNormalizerResourceSession {
    fn drop(&mut self) {
        // Normal close drops the plugin-owned session handle. Drop also covers
        // panic or early-return host paths so resource workers are not left alive.
        if let Err(error) = self.close() {
            tracing::error!(
                plugin = %self.factory.name,
                error = %error,
                "source normalizer resource plugin session close failed during drop"
            );
        }
    }
}
