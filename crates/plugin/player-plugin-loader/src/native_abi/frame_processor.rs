use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use player_plugin::{
    FrameProcessorCapabilities, FrameProcessorError, FrameProcessorInputFrame,
    FrameProcessorOperationStatus, FrameProcessorOutputFrame, FrameProcessorPluginFactory,
    FrameProcessorReceiveFrameMetadata, FrameProcessorReceiveOutput, FrameProcessorReceiveStatus,
    FrameProcessorSession, FrameProcessorSessionConfig, FrameProcessorSessionInfo,
    FrameProcessorSubmitFrame, FrameProcessorSubmitResult, NativeFrame, NativeFrameLeaseToken,
};
use player_plugin_abi::{
    VESPER_MAX_LEASES_PER_SESSION, VesperByteSlice, VesperFrameProcessor, VesperJsonOut,
    VesperNativeFrameOut, VesperSessionId, VesperStatus, status,
};

use super::PluginOwner;
use super::runtime::{
    ActiveSessionError, ActiveSessionRegistry, InterfaceRuntime, JsonCallResult,
    NativeAbiBoundaryError, OPEN_FAILURE_CLOSE_ATTEMPTS, OpenCallResult, borrowed_bytes,
};

const FRAME_PROCESSOR_FAILURE_STATUSES: &[VesperStatus] = &[
    status::FAILURE,
    status::INVALID_ARGUMENT,
    status::UNSUPPORTED,
    status::EXHAUSTED,
    status::TIMEOUT,
];

type FrameSubmitCall = unsafe extern "C" fn(
    context: *mut c_void,
    session_id: VesperSessionId,
    submit_json: VesperByteSlice,
    native_handle: u64,
    out: *mut VesperJsonOut,
) -> VesperStatus;
type FrameReceiveCall = unsafe extern "C" fn(
    context: *mut c_void,
    session_id: VesperSessionId,
    out: *mut VesperNativeFrameOut,
) -> VesperStatus;
type FrameReleaseCall = unsafe extern "C" fn(
    context: *mut c_void,
    session_id: VesperSessionId,
    lease_id: u64,
    out: *mut VesperJsonOut,
) -> VesperStatus;

#[derive(Debug)]
struct NativeAbiFrameProcessorFactoryInner {
    runtime: Arc<InterfaceRuntime>,
    name: String,
    capabilities: FrameProcessorCapabilities,
    open_session: player_plugin_abi::VesperOpenSessionFn,
    submit_frame: FrameSubmitCall,
    receive_frame: FrameReceiveCall,
    release_frame: FrameReleaseCall,
    flush_session: player_plugin_abi::VesperSessionOperationFn,
    close_session: player_plugin_abi::VesperSessionOperationFn,
    next_session_token: AtomicU64,
    active_sessions: ActiveSessionRegistry,
}

#[derive(Debug, Clone)]
pub(crate) struct NativeAbiFrameProcessorPluginFactory {
    inner: Arc<NativeAbiFrameProcessorFactoryInner>,
}

impl NativeAbiFrameProcessorPluginFactory {
    pub(super) fn new(
        plugin_id: &str,
        plugin_name: String,
        instance_id: &str,
        owner: Arc<PluginOwner>,
        table: VesperFrameProcessor,
    ) -> Result<Self, NativeAbiBoundaryError> {
        let runtime = Arc::new(InterfaceRuntime::new(
            owner,
            table.header.context,
            plugin_id,
            instance_id,
        )?);
        let capabilities_json =
            required_callback(&runtime, "capabilities_json", table.capabilities_json)?;
        let open_session =
            required_callback(&runtime, "open_session_json", table.open_session_json)?;
        let submit_frame =
            required_callback(&runtime, "submit_frame_json", table.submit_frame_json)?;
        let receive_frame = required_callback(&runtime, "receive_frame", table.receive_frame)?;
        let release_frame = required_callback(&runtime, "release_frame", table.release_frame)?;
        let flush_session = required_callback(&runtime, "flush_session", table.flush_session)?;
        let close_session = required_callback(&runtime, "close_session", table.close_session)?;
        let capabilities = load_frame_processor_value::<FrameProcessorCapabilities>(
            &runtime,
            "capabilities_json",
            capabilities_json,
        )?;
        Ok(Self {
            inner: Arc::new(NativeAbiFrameProcessorFactoryInner {
                runtime,
                name: plugin_name,
                capabilities,
                open_session,
                submit_frame,
                receive_frame,
                release_frame,
                flush_session,
                close_session,
                next_session_token: AtomicU64::new(1),
                active_sessions: ActiveSessionRegistry::default(),
            }),
        })
    }
}

impl FrameProcessorPluginFactory for NativeAbiFrameProcessorPluginFactory {
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
                "serialize frame processor config failed: {error}"
            ))
        })?;
        let open_reservation =
            self.inner
                .active_sessions
                .reserve_open()
                .map_err(|error| match error {
                    ActiveSessionError::Exhausted => FrameProcessorError::Backpressure {
                        message: "host frame processor interface reached its active session limit"
                            .to_owned(),
                    },
                    ActiveSessionError::Duplicate { .. } => FrameProcessorError::internal(
                        "host frame processor session reservation failed unexpectedly",
                    ),
                })?;
        let result = self
            .inner
            .runtime
            .invoke_open(
                "open_session_json",
                FRAME_PROCESSOR_FAILURE_STATUSES,
                |out| {
                    // SAFETY: callback/context are validated and the borrowed
                    // config/output remain live for this synchronous call.
                    unsafe {
                        (self.inner.open_session)(
                            self.inner.runtime.context(),
                            borrowed_bytes(&config_json),
                            out,
                        )
                    }
                },
                |session_id| {
                    cleanup_frame_processor_session(
                        &self.inner.runtime,
                        self.inner.close_session,
                        session_id,
                    );
                },
            )
            .map_err(map_frame_processor_boundary)?;
        match result {
            OpenCallResult::Success {
                session_id,
                payload,
            } => {
                let session_info = match self
                    .inner
                    .runtime
                    .decode_json::<FrameProcessorSessionInfo>("open_session_json", &payload)
                {
                    Ok(session_info) => session_info,
                    Err(error) => {
                        cleanup_frame_processor_session(
                            &self.inner.runtime,
                            self.inner.close_session,
                            session_id,
                        );
                        return Err(map_frame_processor_boundary(error));
                    }
                };
                let session_token = match self.inner.next_session_token.fetch_update(
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                    |current| current.checked_add(1),
                ) {
                    Ok(session_token) => session_token,
                    Err(_) => {
                        cleanup_frame_processor_session(
                            &self.inner.runtime,
                            self.inner.close_session,
                            session_id,
                        );
                        return Err(FrameProcessorError::internal(
                            "host frame processor session token space is exhausted",
                        ));
                    }
                };
                if let Err(error) = open_reservation.register(session_id) {
                    return Err(match error {
                        ActiveSessionError::Duplicate { session_id } => {
                            map_frame_processor_boundary(self.inner.runtime.contract_violation(
                                "open_session_json",
                                format!(
                                    "plugin reused active frame processor session id {session_id}"
                                ),
                            ))
                        }
                        ActiveSessionError::Exhausted => FrameProcessorError::Backpressure {
                            message:
                                "host frame processor interface reached its active session limit"
                                    .to_owned(),
                        },
                    });
                }
                Ok(Box::new(NativeAbiFrameProcessorSession {
                    factory: self.inner.clone(),
                    session_id,
                    session_token,
                    session_info,
                    active_leases: HashMap::new(),
                    active_abi_leases: HashSet::new(),
                    next_lease_token: 1,
                    closing: false,
                    closed: false,
                }))
            }
            OpenCallResult::Failure { status, payload } => Err(decode_frame_processor_failure(
                &self.inner.runtime,
                "open_session_json",
                status,
                &payload,
            )),
        }
    }
}

struct NativeAbiFrameProcessorSession {
    factory: Arc<NativeAbiFrameProcessorFactoryInner>,
    session_id: u64,
    session_token: u64,
    session_info: FrameProcessorSessionInfo,
    active_leases: HashMap<u64, u64>,
    active_abi_leases: HashSet<u64>,
    next_lease_token: u64,
    closing: bool,
    closed: bool,
}

impl std::fmt::Debug for NativeAbiFrameProcessorSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeAbiFrameProcessorSession")
            .field("session_id", &self.session_id)
            .field("session_token", &self.session_token)
            .field("active_lease_count", &self.active_leases.len())
            .field("closed", &self.closed)
            .finish()
    }
}

impl NativeAbiFrameProcessorSession {
    fn ensure_open(&self) -> Result<(), FrameProcessorError> {
        if self.closed || self.closing {
            Err(FrameProcessorError::NotConfigured)
        } else {
            Ok(())
        }
    }

    fn decode_result<T>(
        &self,
        operation: &'static str,
        result: JsonCallResult,
    ) -> Result<T, FrameProcessorError>
    where
        T: serde::de::DeserializeOwned,
    {
        match result {
            JsonCallResult::Success(payload) => self
                .factory
                .runtime
                .decode_json::<T>(operation, &payload)
                .map_err(map_frame_processor_boundary),
            JsonCallResult::Failure { status, payload } => Err(decode_frame_processor_failure(
                &self.factory.runtime,
                operation,
                status,
                &payload,
            )),
        }
    }

    fn release_abi_lease(&mut self, lease_id: u64) -> Result<(), FrameProcessorError> {
        let result = self
            .factory
            .runtime
            .invoke_cleanup_json("release_frame", FRAME_PROCESSOR_FAILURE_STATUSES, |out| {
                // SAFETY: callback/context/session are validated and output is
                // borrowed for this synchronous cleanup call.
                unsafe {
                    (self.factory.release_frame)(
                        self.factory.runtime.context(),
                        self.session_id,
                        lease_id,
                        out,
                    )
                }
            })
            .map_err(map_frame_processor_boundary)?;
        let status =
            self.decode_result::<FrameProcessorOperationStatus>("release_frame", result)?;
        require_frame_processor_completed(&self.factory.runtime, "release_frame", status)
    }

    fn release_output(&mut self, frame: NativeFrame) -> Result<(), FrameProcessorError> {
        self.ensure_open()?;
        let Some(token) = frame.lease_token else {
            if frame
                .metadata
                .release_tracking
                .as_ref()
                .is_some_and(|tracking| tracking.requires_release)
            {
                return Err(FrameProcessorError::abi_violation(
                    "native frame requires release but has no plugin lease token",
                ));
            }
            return Ok(());
        };
        let (interface_token, session_token, lease_token) = token.host_lease_parts();
        if interface_token != self.factory.runtime.interface_token()
            || session_token != self.session_token
        {
            return Err(FrameProcessorError::abi_violation(
                "native frame lease belongs to a different interface or session",
            ));
        }
        let Some(lease_id) = self.active_leases.remove(&lease_token) else {
            return Err(FrameProcessorError::abi_violation(
                "native frame lease is stale or was already released",
            ));
        };
        self.active_abi_leases.remove(&lease_id);
        self.release_abi_lease(lease_id)
    }

    fn discard_malformed_lease(&mut self, lease_id: u64) {
        if lease_id == 0 {
            return;
        }
        if self.active_abi_leases.contains(&lease_id) {
            let _ = self.factory.runtime.contract_violation(
                "receive_frame",
                "malformed output reused an active frame lease id",
            );
            self.drain_after_lease_violation();
        } else if self.release_abi_lease(lease_id).is_err() {
            self.drain_after_lease_violation();
        }
    }

    fn drain_after_lease_violation(&mut self) {
        if self.flush().is_err() {
            let _ = self.close();
        }
    }
}

impl FrameProcessorSession for NativeAbiFrameProcessorSession {
    fn session_info(&self) -> FrameProcessorSessionInfo {
        self.session_info.clone()
    }

    fn submit_frame(
        &mut self,
        frame: FrameProcessorInputFrame<'_>,
        submit: &FrameProcessorSubmitFrame,
    ) -> Result<FrameProcessorSubmitResult, FrameProcessorError> {
        self.ensure_open()?;
        let submit_json = serde_json::to_vec(submit).map_err(|error| {
            FrameProcessorError::payload_codec(format!(
                "serialize frame processor submit failed: {error}"
            ))
        })?;
        let native_handle = u64::try_from(frame.native_handle()).map_err(|_| {
            FrameProcessorError::abi_violation(
                "native frame handle does not fit the native ABI wire type",
            )
        })?;
        let result = self
            .factory
            .runtime
            .invoke_json(
                "submit_frame_json",
                FRAME_PROCESSOR_FAILURE_STATUSES,
                |out| {
                    // SAFETY: callback/context, borrowed submit JSON, input handle,
                    // and output remain valid for this synchronous call.
                    unsafe {
                        (self.factory.submit_frame)(
                            self.factory.runtime.context(),
                            self.session_id,
                            borrowed_bytes(&submit_json),
                            native_handle,
                            out,
                        )
                    }
                },
            )
            .map_err(map_frame_processor_boundary)?;
        self.decode_result("submit_frame_json", result)
    }

    fn receive_frame(&mut self) -> Result<FrameProcessorReceiveOutput, FrameProcessorError> {
        self.ensure_open()?;
        if self.active_leases.len() >= VESPER_MAX_LEASES_PER_SESSION {
            return Err(FrameProcessorError::Backpressure {
                message: format!(
                    "frame processor session reached the {VESPER_MAX_LEASES_PER_SESSION}-frame lease limit"
                ),
            });
        }
        if self.next_lease_token == u64::MAX {
            return Err(FrameProcessorError::internal(
                "host frame processor lease token space is exhausted",
            ));
        }
        let mut out = VesperNativeFrameOut::default();
        let call = self
            .factory
            .runtime
            .invoke_callback("receive_frame", false, || {
                // SAFETY: callback/context/session are validated and output is
                // host-owned for this synchronous call.
                unsafe {
                    (self.factory.receive_frame)(
                        self.factory.runtime.context(),
                        self.session_id,
                        &mut out,
                    )
                }
            });
        let metadata_payload = self
            .factory
            .runtime
            .capture_owned_bytes("receive_frame", out.metadata);
        let output_check = self.factory.runtime.validate_out_prefix(
            "receive_frame",
            out.struct_size,
            0,
            size_of::<VesperNativeFrameOut>() as u32,
        );
        let raw_status = match call {
            Ok(raw_status) => raw_status,
            Err(error) => {
                self.discard_malformed_lease(out.lease_id);
                return Err(map_frame_processor_boundary(error));
            }
        };
        let metadata_payload = match metadata_payload {
            Ok(payload) => payload,
            Err(error) => {
                self.discard_malformed_lease(out.lease_id);
                return Err(map_frame_processor_boundary(error));
            }
        };
        if let Err(error) = output_check {
            self.discard_malformed_lease(out.lease_id);
            return Err(map_frame_processor_boundary(error));
        }
        let result = match self.factory.runtime.classify_json_status(
            "receive_frame",
            raw_status,
            FRAME_PROCESSOR_FAILURE_STATUSES,
            metadata_payload,
        ) {
            Ok(result) => result,
            Err(error) => {
                self.discard_malformed_lease(out.lease_id);
                return Err(map_frame_processor_boundary(error));
            }
        };
        let payload = match result {
            JsonCallResult::Success(payload) => payload,
            JsonCallResult::Failure { status, payload } => {
                if out.native_handle != 0 || out.lease_id != 0 || out.requires_release != 0 {
                    self.discard_malformed_lease(out.lease_id);
                    return Err(map_frame_processor_boundary(
                        self.factory.runtime.contract_violation(
                            "receive_frame",
                            "failed frame receive returned native resources",
                        ),
                    ));
                }
                return Err(decode_frame_processor_failure(
                    &self.factory.runtime,
                    "receive_frame",
                    status,
                    &payload,
                ));
            }
        };
        let metadata = match self
            .factory
            .runtime
            .decode_json::<FrameProcessorReceiveFrameMetadata>("receive_frame", &payload)
        {
            Ok(metadata) => metadata,
            Err(error) => {
                self.discard_malformed_lease(out.lease_id);
                return Err(map_frame_processor_boundary(error));
            }
        };
        if out.requires_release > 1 {
            self.discard_malformed_lease(out.lease_id);
            return Err(map_frame_processor_boundary(
                self.factory.runtime.contract_violation(
                    "receive_frame",
                    format!(
                        "requires_release must be 0 or 1, got {}",
                        out.requires_release
                    ),
                ),
            ));
        }
        match metadata.status {
            FrameProcessorReceiveStatus::Frame => {
                let Some(frame_metadata) = metadata.frame else {
                    self.discard_malformed_lease(out.lease_id);
                    return Err(map_frame_processor_boundary(
                        self.factory.runtime.contract_violation(
                            "receive_frame",
                            "frame status is missing frame metadata",
                        ),
                    ));
                };
                let handle = usize::try_from(out.native_handle).map_err(|_| {
                    self.discard_malformed_lease(out.lease_id);
                    map_frame_processor_boundary(self.factory.runtime.contract_violation(
                        "receive_frame",
                        "native handle does not fit this process",
                    ))
                })?;
                if handle == 0 {
                    self.discard_malformed_lease(out.lease_id);
                    return Err(map_frame_processor_boundary(
                        self.factory.runtime.contract_violation(
                            "receive_frame",
                            "frame status returned a zero native handle",
                        ),
                    ));
                }
                let requires_release = out.requires_release == 1;
                if requires_release != (out.lease_id != 0) {
                    self.discard_malformed_lease(out.lease_id);
                    return Err(map_frame_processor_boundary(
                        self.factory.runtime.contract_violation(
                            "receive_frame",
                            "requires_release and lease_id disagree",
                        ),
                    ));
                }
                let lease_token = if requires_release {
                    if self.active_abi_leases.contains(&out.lease_id) {
                        let error = self.factory.runtime.contract_violation(
                            "receive_frame",
                            "plugin reused an active frame lease id",
                        );
                        self.drain_after_lease_violation();
                        return Err(map_frame_processor_boundary(error));
                    }
                    let host_lease = self.next_lease_token;
                    self.next_lease_token += 1;
                    self.active_leases.insert(host_lease, out.lease_id);
                    self.active_abi_leases.insert(out.lease_id);
                    Some(NativeFrameLeaseToken::from_host_lease(
                        self.factory.runtime.interface_token(),
                        self.session_token,
                        host_lease,
                    ))
                } else {
                    None
                };
                Ok(FrameProcessorReceiveOutput::Frame(
                    FrameProcessorOutputFrame {
                        frame: NativeFrame {
                            metadata: frame_metadata,
                            handle,
                            lease_token,
                        },
                        timings: metadata.timings,
                        source_frame_id: metadata.source_frame_id,
                        message: metadata.message,
                    },
                ))
            }
            FrameProcessorReceiveStatus::Pending | FrameProcessorReceiveStatus::EndOfStream => {
                if metadata.frame.is_some()
                    || out.native_handle != 0
                    || out.lease_id != 0
                    || out.requires_release != 0
                {
                    self.discard_malformed_lease(out.lease_id);
                    return Err(map_frame_processor_boundary(
                        self.factory.runtime.contract_violation(
                            "receive_frame",
                            "non-frame status returned frame metadata or native resources",
                        ),
                    ));
                }
                if metadata.status == FrameProcessorReceiveStatus::Pending {
                    Ok(FrameProcessorReceiveOutput::Pending)
                } else {
                    Ok(FrameProcessorReceiveOutput::EndOfStream)
                }
            }
        }
    }

    fn release_frame(&mut self, frame: NativeFrame) -> Result<(), FrameProcessorError> {
        self.release_output(frame)
    }

    fn flush(&mut self) -> Result<(), FrameProcessorError> {
        self.ensure_open()?;
        self.active_leases.clear();
        self.active_abi_leases.clear();
        let result = self
            .factory
            .runtime
            .invoke_cleanup_json("flush_session", FRAME_PROCESSOR_FAILURE_STATUSES, |out| {
                // SAFETY: callback/context/session are validated and output is
                // borrowed for this synchronous cleanup call.
                unsafe {
                    (self.factory.flush_session)(
                        self.factory.runtime.context(),
                        self.session_id,
                        out,
                    )
                }
            })
            .map_err(map_frame_processor_boundary)?;
        let status =
            self.decode_result::<FrameProcessorOperationStatus>("flush_session", result)?;
        require_frame_processor_completed(&self.factory.runtime, "flush_session", status)
    }

    fn close(&mut self) -> Result<(), FrameProcessorError> {
        if self.closed {
            return Ok(());
        }
        self.closing = true;
        self.active_leases.clear();
        self.active_abi_leases.clear();
        let result = self
            .factory
            .runtime
            .invoke_cleanup_json("close_session", FRAME_PROCESSOR_FAILURE_STATUSES, |out| {
                // SAFETY: callback/context/session are validated and output is
                // borrowed for this synchronous cleanup call.
                unsafe {
                    (self.factory.close_session)(
                        self.factory.runtime.context(),
                        self.session_id,
                        out,
                    )
                }
            })
            .map_err(map_frame_processor_boundary)?;
        let status =
            self.decode_result::<FrameProcessorOperationStatus>("close_session", result)?;
        require_frame_processor_completed(&self.factory.runtime, "close_session", status)?;
        self.factory.active_sessions.remove(self.session_id);
        self.closed = true;
        Ok(())
    }
}

impl Drop for NativeAbiFrameProcessorSession {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn required_callback<T: Copy>(
    runtime: &InterfaceRuntime,
    callback_name: &'static str,
    callback: Option<T>,
) -> Result<T, NativeAbiBoundaryError> {
    callback.ok_or_else(|| {
        runtime.contract_violation(
            "construct_wrapper",
            format!("{callback_name} callback is missing after validation"),
        )
    })
}

fn load_frame_processor_value<T>(
    runtime: &InterfaceRuntime,
    operation: &'static str,
    callback: player_plugin_abi::VesperGetJsonFn,
) -> Result<T, NativeAbiBoundaryError>
where
    T: serde::de::DeserializeOwned,
{
    let result = runtime.invoke_json(operation, FRAME_PROCESSOR_FAILURE_STATUSES, |out| {
        // SAFETY: callback/context are validated and output is host-owned for
        // this synchronous call.
        unsafe { callback(runtime.context(), out) }
    })?;
    match result {
        JsonCallResult::Success(payload) => runtime.decode_json(operation, &payload),
        JsonCallResult::Failure { status, payload } => {
            let error = decode_frame_processor_failure(runtime, operation, status, &payload);
            Err(runtime.reported_failure(operation, status, error.to_string()))
        }
    }
}

fn decode_frame_processor_failure(
    runtime: &InterfaceRuntime,
    operation: &'static str,
    raw_status: VesperStatus,
    payload: &[u8],
) -> FrameProcessorError {
    let error = if raw_status == status::EXHAUSTED && payload.is_empty() {
        FrameProcessorError::Backpressure {
            message: "frame processor resource limit exhausted".to_owned(),
        }
    } else if raw_status == status::TIMEOUT && payload.is_empty() {
        FrameProcessorError::Timeout {
            message: "frame processor operation timed out".to_owned(),
        }
    } else {
        match runtime.decode_json::<FrameProcessorError>(operation, payload) {
            Ok(error) => error,
            Err(error) => return map_frame_processor_boundary(error),
        }
    };
    if frame_processor_status_matches(raw_status, &error) {
        error
    } else {
        map_frame_processor_boundary(runtime.contract_violation(
            operation,
            format!("status {raw_status} is inconsistent with frame processor error `{error}`"),
        ))
    }
}

fn frame_processor_status_matches(raw_status: VesperStatus, error: &FrameProcessorError) -> bool {
    match raw_status {
        status::FAILURE => matches!(
            error,
            FrameProcessorError::Internal { .. } | FrameProcessorError::NotConfigured
        ),
        status::INVALID_ARGUMENT => matches!(error, FrameProcessorError::PayloadCodec { .. }),
        status::UNSUPPORTED => matches!(error, FrameProcessorError::UnsupportedHandle { .. }),
        status::EXHAUSTED => matches!(error, FrameProcessorError::Backpressure { .. }),
        status::TIMEOUT => matches!(error, FrameProcessorError::Timeout { .. }),
        _ => false,
    }
}

fn require_frame_processor_completed(
    runtime: &InterfaceRuntime,
    operation: &'static str,
    status: FrameProcessorOperationStatus,
) -> Result<(), FrameProcessorError> {
    if status.completed {
        Ok(())
    } else {
        Err(map_frame_processor_boundary(runtime.contract_violation(
            operation,
            "successful cleanup reported completed=false",
        )))
    }
}

fn cleanup_frame_processor_session(
    runtime: &InterfaceRuntime,
    close: player_plugin_abi::VesperSessionOperationFn,
    session_id: u64,
) {
    for _ in 0..OPEN_FAILURE_CLOSE_ATTEMPTS {
        let result = runtime.invoke_cleanup_json(
            "close_session_after_open_failure",
            FRAME_PROCESSOR_FAILURE_STATUSES,
            |out| {
                // SAFETY: callback/context/session came from the successful
                // open and output is host-owned for this synchronous cleanup.
                unsafe { close(runtime.context(), session_id, out) }
            },
        );
        let Ok(JsonCallResult::Success(payload)) = result else {
            continue;
        };
        if matches!(
            runtime.decode_json::<FrameProcessorOperationStatus>(
                "close_session_after_open_failure",
                &payload,
            ),
            Ok(FrameProcessorOperationStatus { completed: true })
        ) {
            return;
        }
        let _ = runtime.contract_violation(
            "close_session_after_open_failure",
            "successful orphan close reported completed=false",
        );
    }
}

fn map_frame_processor_boundary(error: NativeAbiBoundaryError) -> FrameProcessorError {
    FrameProcessorError::abi_violation(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use player_plugin::{
        DecoderFrameFormat, DecoderMediaKind, FrameProcessorReceiveFrameMetadata,
        FrameProcessorSubmitResult, NativeFrameMetadata, NativeHandleKind,
    };
    use player_plugin_abi::{
        FRAME_PROCESSOR_INTERFACE_ID, VESPER_INTERFACE_MAJOR, VESPER_INTERFACE_MINOR,
        VesperInterfaceHeader, VesperOpenSessionOut, VesperOwnedBytes,
    };
    use serde::Serialize;

    use super::*;

    const FRAME_PROCESSOR_INSTANCE: &str = "dev.vesper.fixture.frame-processor";

    #[derive(Clone, Copy)]
    enum OpenPayload {
        Valid,
        Malformed,
    }

    #[derive(Default)]
    struct RawFrameProcessorCounters {
        closes: AtomicUsize,
    }

    struct RawFrameProcessorContext {
        open_payload: OpenPayload,
        fail_first_close: bool,
        counters: Arc<RawFrameProcessorCounters>,
    }

    unsafe fn raw_context<'a>(context: *mut c_void) -> Option<&'a RawFrameProcessorContext> {
        // SAFETY: every fixture callback receives the live context installed
        // in the interface header and owned by `PluginOwner`.
        unsafe { context.cast::<RawFrameProcessorContext>().as_ref() }
    }

    unsafe fn write_json<T: Serialize>(out: *mut VesperJsonOut, value: &T) -> VesperStatus {
        let Some(out) =
            // SAFETY: the host passes a writable, initialized output.
            (unsafe { out.as_mut() })
        else {
            return status::INVALID_ARGUMENT;
        };
        match serde_json::to_vec(value) {
            Ok(payload) => {
                out.payload = VesperOwnedBytes::from_vec(payload);
                status::OK
            }
            Err(_) => status::FAILURE,
        }
    }

    unsafe extern "C" fn raw_free(owner: *mut c_void, bytes: VesperOwnedBytes) {
        let _ = owner;
        // SAFETY: fixture outputs allocate every owned byte sequence with
        // `VesperOwnedBytes::from_vec` and transfer it back exactly once.
        drop(unsafe { bytes.into_vec() });
    }

    unsafe extern "C" fn raw_destroy(owner: *mut c_void) {
        if !owner.is_null() {
            // SAFETY: `raw_factory` transfers one boxed context to the owner.
            drop(unsafe { Box::from_raw(owner.cast::<RawFrameProcessorContext>()) });
        }
    }

    unsafe extern "C" fn raw_capabilities(
        _context: *mut c_void,
        out: *mut VesperJsonOut,
    ) -> VesperStatus {
        // SAFETY: forwarded host output retains the callback contract.
        unsafe { write_json(out, &FrameProcessorCapabilities::default()) }
    }

    unsafe extern "C" fn raw_open(
        context: *mut c_void,
        _config: VesperByteSlice,
        out: *mut VesperOpenSessionOut,
    ) -> VesperStatus {
        let Some(context) =
            // SAFETY: callback context follows the fixture table contract.
            (unsafe { raw_context(context) })
        else {
            return status::INVALID_ARGUMENT;
        };
        let Some(out) =
            // SAFETY: the host passes a writable, initialized output.
            (unsafe { out.as_mut() })
        else {
            return status::INVALID_ARGUMENT;
        };
        let payload = match context.open_payload {
            OpenPayload::Valid => match serde_json::to_vec(&FrameProcessorSessionInfo::default()) {
                Ok(payload) => payload,
                Err(_) => return status::FAILURE,
            },
            OpenPayload::Malformed => b"{".to_vec(),
        };
        out.session_id = 41;
        out.payload = VesperOwnedBytes::from_vec(payload);
        status::OK
    }

    unsafe extern "C" fn raw_submit(
        _context: *mut c_void,
        _session_id: u64,
        _submit: VesperByteSlice,
        _native_handle: u64,
        out: *mut VesperJsonOut,
    ) -> VesperStatus {
        // SAFETY: forwarded host output retains the callback contract.
        unsafe { write_json(out, &FrameProcessorSubmitResult::default()) }
    }

    unsafe extern "C" fn raw_receive(
        _context: *mut c_void,
        _session_id: u64,
        out: *mut VesperNativeFrameOut,
    ) -> VesperStatus {
        let Some(out) =
            // SAFETY: the host passes a writable, initialized output.
            (unsafe { out.as_mut() })
        else {
            return status::INVALID_ARGUMENT;
        };
        let payload = match serde_json::to_vec(&FrameProcessorReceiveFrameMetadata::pending()) {
            Ok(payload) => payload,
            Err(_) => return status::FAILURE,
        };
        out.metadata = VesperOwnedBytes::from_vec(payload);
        status::OK
    }

    unsafe extern "C" fn raw_release(
        _context: *mut c_void,
        _session_id: u64,
        _lease_id: u64,
        out: *mut VesperJsonOut,
    ) -> VesperStatus {
        // SAFETY: forwarded host output retains the callback contract.
        unsafe { write_json(out, &FrameProcessorOperationStatus { completed: true }) }
    }

    unsafe extern "C" fn raw_flush(
        _context: *mut c_void,
        _session_id: u64,
        out: *mut VesperJsonOut,
    ) -> VesperStatus {
        // SAFETY: forwarded host output retains the callback contract.
        unsafe { write_json(out, &FrameProcessorOperationStatus { completed: true }) }
    }

    unsafe extern "C" fn raw_close(
        context: *mut c_void,
        _session_id: u64,
        out: *mut VesperJsonOut,
    ) -> VesperStatus {
        let Some(context) =
            // SAFETY: callback context follows the fixture table contract.
            (unsafe { raw_context(context) })
        else {
            return status::INVALID_ARGUMENT;
        };
        context.counters.closes.fetch_add(1, Ordering::SeqCst);
        if context.fail_first_close && context.counters.closes.load(Ordering::SeqCst) == 1 {
            // SAFETY: forwarded host output retains the callback contract.
            let _ = unsafe {
                write_json(
                    out,
                    &FrameProcessorError::Timeout {
                        message: "fixture close timeout".to_owned(),
                    },
                )
            };
            return status::TIMEOUT;
        }
        // SAFETY: forwarded host output retains the callback contract.
        unsafe { write_json(out, &FrameProcessorOperationStatus { completed: true }) }
    }

    fn raw_factory(
        open_payload: OpenPayload,
    ) -> (
        NativeAbiFrameProcessorPluginFactory,
        Arc<RawFrameProcessorCounters>,
    ) {
        raw_factory_with_close_failure(open_payload, false)
    }

    fn raw_factory_with_close_failure(
        open_payload: OpenPayload,
        fail_first_close: bool,
    ) -> (
        NativeAbiFrameProcessorPluginFactory,
        Arc<RawFrameProcessorCounters>,
    ) {
        let counters = Arc::new(RawFrameProcessorCounters::default());
        let context = Box::new(RawFrameProcessorContext {
            open_payload,
            fail_first_close,
            counters: counters.clone(),
        });
        let context = NonNull::new(Box::into_raw(context).cast::<c_void>())
            .expect("raw frame processor context");
        let owner = Arc::new(PluginOwner {
            owner: context,
            free_bytes: raw_free,
            destroy_owner: raw_destroy,
            library: None,
        });
        let table = VesperFrameProcessor {
            header: VesperInterfaceHeader::new(
                size_of::<VesperFrameProcessor>() as u32,
                FRAME_PROCESSOR_INTERFACE_ID,
                VESPER_INTERFACE_MAJOR,
                VESPER_INTERFACE_MINOR,
                context.as_ptr(),
            ),
            capabilities_json: Some(raw_capabilities),
            open_session_json: Some(raw_open),
            submit_frame_json: Some(raw_submit),
            receive_frame: Some(raw_receive),
            release_frame: Some(raw_release),
            flush_session: Some(raw_flush),
            close_session: Some(raw_close),
        };
        let factory = NativeAbiFrameProcessorPluginFactory::new(
            "dev.vesper.raw-frame-processor",
            "Raw frame processor".to_owned(),
            FRAME_PROCESSOR_INSTANCE,
            owner,
            table,
        )
        .expect("raw frame processor wrapper");
        (factory, counters)
    }

    fn session_config() -> FrameProcessorSessionConfig {
        FrameProcessorSessionConfig {
            processor_index: 0,
            input_metadata: NativeFrameMetadata {
                media_kind: DecoderMediaKind::Video,
                format: DecoderFrameFormat::Nv12,
                codec: "h264".to_owned(),
                pts_us: None,
                duration_us: None,
                width: 16,
                height: 16,
                coded_width: None,
                coded_height: None,
                visible_rect: None,
                handle_kind: NativeHandleKind::D3D11Texture2D,
                pipeline_profile: None,
                color_space: None,
                hdr_metadata: None,
                color: None,
                hdr: None,
                sync_info: None,
                transform: None,
                frame_id: None,
                release_tracking: None,
            },
            max_in_flight_frames: None,
        }
    }

    #[test]
    fn malformed_open_session_info_closes_created_session() {
        let (factory, counters) = raw_factory(OpenPayload::Malformed);
        assert!(matches!(
            factory.open_session(&session_config()),
            Err(FrameProcessorError::AbiViolation { .. })
        ));
        assert_eq!(counters.closes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn exhausted_host_session_token_closes_created_session() {
        let (factory, counters) = raw_factory(OpenPayload::Valid);
        factory
            .inner
            .next_session_token
            .store(u64::MAX, Ordering::Relaxed);
        assert!(matches!(
            factory.open_session(&session_config()),
            Err(FrameProcessorError::Internal { .. })
        ));
        assert_eq!(counters.closes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn orphan_open_cleanup_retries_a_failed_close_once() {
        let (factory, counters) = raw_factory_with_close_failure(OpenPayload::Malformed, true);
        assert!(matches!(
            factory.open_session(&session_config()),
            Err(FrameProcessorError::AbiViolation { .. })
        ));
        assert_eq!(counters.closes.load(Ordering::SeqCst), 2);
    }
}
