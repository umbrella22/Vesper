use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use player_plugin::{
    DecoderCapabilities, DecoderError, DecoderNativeFrame, DecoderNativeRequirements,
    DecoderOperationStatus, DecoderPacket, DecoderPacketResult, DecoderPcmFrame,
    DecoderReceiveFrameStatus, DecoderReceiveNativeFrameMetadata, DecoderReceiveNativeFrameOutput,
    DecoderReceivePcmFrameMetadata, DecoderReceivePcmFrameOutput, DecoderSessionConfig,
    DecoderSessionInfo, NativeDecoderPluginFactory, NativeDecoderSession, NativeFrameLeaseToken,
};
use player_plugin_abi::{
    VESPER_MAX_LEASES_PER_SESSION, VESPER_RELEASE_DISCARDED, VESPER_RELEASE_PRESENTED,
    VesperByteSlice, VesperJsonOut, VesperNativeDecoder, VesperNativeFrameOut, VesperPcmFrameOut,
    VesperSessionId, VesperStatus, status,
};

use super::PluginOwner;
use super::runtime::{
    ActiveSessionError, ActiveSessionRegistry, InterfaceRuntime, JsonCallResult,
    NativeAbiBoundaryError, OPEN_FAILURE_CLOSE_ATTEMPTS, OpenCallResult, borrowed_bytes,
};

const DECODER_FAILURE_STATUSES: &[VesperStatus] = &[
    status::FAILURE,
    status::INVALID_ARGUMENT,
    status::UNSUPPORTED,
    status::EXHAUSTED,
];

type DecoderSendPacketFn = unsafe extern "C" fn(
    context: *mut c_void,
    session_id: VesperSessionId,
    packet_json: VesperByteSlice,
    packet_data: VesperByteSlice,
    out: *mut VesperJsonOut,
) -> VesperStatus;
type DecoderReceiveNativeFrameFn = unsafe extern "C" fn(
    context: *mut c_void,
    session_id: VesperSessionId,
    out: *mut VesperNativeFrameOut,
) -> VesperStatus;
type DecoderReleaseNativeFrameFn = unsafe extern "C" fn(
    context: *mut c_void,
    session_id: VesperSessionId,
    lease_id: u64,
    disposition: u32,
    out: *mut VesperJsonOut,
) -> VesperStatus;
type DecoderReceivePcmFrameFn = unsafe extern "C" fn(
    context: *mut c_void,
    session_id: VesperSessionId,
    out: *mut VesperPcmFrameOut,
) -> VesperStatus;

#[derive(Debug)]
struct NativeAbiDecoderFactoryInner {
    runtime: Arc<InterfaceRuntime>,
    name: String,
    capabilities: DecoderCapabilities,
    native_requirements: DecoderNativeRequirements,
    open_session: player_plugin_abi::VesperOpenSessionFn,
    send_packet: DecoderSendPacketFn,
    receive_native_frame: DecoderReceiveNativeFrameFn,
    release_native_frame: DecoderReleaseNativeFrameFn,
    flush_session: player_plugin_abi::VesperSessionOperationFn,
    close_session: player_plugin_abi::VesperSessionOperationFn,
    receive_pcm_frame: Option<DecoderReceivePcmFrameFn>,
    next_session_token: AtomicU64,
    active_sessions: ActiveSessionRegistry,
}

#[derive(Debug, Clone)]
pub(crate) struct NativeAbiDecoderPluginFactory {
    inner: Arc<NativeAbiDecoderFactoryInner>,
}

impl NativeAbiDecoderPluginFactory {
    pub(super) fn new(
        plugin_id: &str,
        plugin_name: String,
        instance_id: &str,
        owner: Arc<PluginOwner>,
        table: VesperNativeDecoder,
    ) -> Result<Self, NativeAbiBoundaryError> {
        let runtime = Arc::new(InterfaceRuntime::new(
            owner,
            table.header.context,
            plugin_id,
            instance_id,
        )?);
        let capabilities_json =
            required_callback(&runtime, "capabilities_json", table.capabilities_json)?;
        let native_requirements_json = required_callback(
            &runtime,
            "native_requirements_json",
            table.native_requirements_json,
        )?;
        let open_session =
            required_callback(&runtime, "open_session_json", table.open_session_json)?;
        let send_packet = required_callback(&runtime, "send_packet", table.send_packet)?;
        let receive_native_frame =
            required_callback(&runtime, "receive_native_frame", table.receive_native_frame)?;
        let release_native_frame =
            required_callback(&runtime, "release_native_frame", table.release_native_frame)?;
        let flush_session = required_callback(&runtime, "flush_session", table.flush_session)?;
        let close_session = required_callback(&runtime, "close_session", table.close_session)?;

        let capabilities = load_decoder_value::<DecoderCapabilities>(
            &runtime,
            "capabilities_json",
            capabilities_json,
        )?;
        let native_requirements = load_decoder_value::<DecoderNativeRequirements>(
            &runtime,
            "native_requirements_json",
            native_requirements_json,
        )?;
        if capabilities.supports_pcm_frames && table.receive_pcm_frame.is_none() {
            return Err(runtime.contract_violation(
                "construct_wrapper",
                "decoder advertises PCM output without receive_pcm_frame",
            ));
        }
        Ok(Self {
            inner: Arc::new(NativeAbiDecoderFactoryInner {
                runtime,
                name: plugin_name,
                capabilities,
                native_requirements,
                open_session,
                send_packet,
                receive_native_frame,
                release_native_frame,
                flush_session,
                close_session,
                receive_pcm_frame: table.receive_pcm_frame,
                next_session_token: AtomicU64::new(1),
                active_sessions: ActiveSessionRegistry::default(),
            }),
        })
    }
}

impl NativeAbiDecoderFactoryInner {
    fn allocate_session_token(&self) -> Result<u64, DecoderError> {
        self.next_session_token
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| DecoderError::internal("host decoder session token space is exhausted"))
    }
}

impl NativeDecoderPluginFactory for NativeAbiDecoderPluginFactory {
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
            DecoderError::payload_codec(format!("serialize decoder session config failed: {error}"))
        })?;
        let open_reservation =
            self.inner
                .active_sessions
                .reserve_open()
                .map_err(|error| match error {
                    ActiveSessionError::Exhausted => DecoderError::internal(
                        "host decoder interface reached its active session limit",
                    ),
                    ActiveSessionError::Duplicate { .. } => DecoderError::internal(
                        "host decoder session reservation failed unexpectedly",
                    ),
                })?;
        let result = self
            .inner
            .runtime
            .invoke_open(
                "open_session_json",
                DECODER_FAILURE_STATUSES,
                |out| {
                    // SAFETY: callback/context are validated and config/output are
                    // borrowed for this synchronous call only.
                    unsafe {
                        (self.inner.open_session)(
                            self.inner.runtime.context(),
                            borrowed_bytes(&config_json),
                            out,
                        )
                    }
                },
                |session_id| {
                    cleanup_decoder_session(
                        &self.inner.runtime,
                        self.inner.close_session,
                        session_id,
                    );
                },
            )
            .map_err(map_decoder_boundary)?;
        match result {
            OpenCallResult::Success {
                session_id,
                payload,
            } => {
                let session_info = match self
                    .inner
                    .runtime
                    .decode_json::<DecoderSessionInfo>("open_session_json", &payload)
                {
                    Ok(session_info) => session_info,
                    Err(error) => {
                        cleanup_decoder_session(
                            &self.inner.runtime,
                            self.inner.close_session,
                            session_id,
                        );
                        return Err(map_decoder_boundary(error));
                    }
                };
                let session_token = match self.inner.allocate_session_token() {
                    Ok(session_token) => session_token,
                    Err(error) => {
                        cleanup_decoder_session(
                            &self.inner.runtime,
                            self.inner.close_session,
                            session_id,
                        );
                        return Err(error);
                    }
                };
                if let Err(error) = open_reservation.register(session_id) {
                    return Err(match error {
                        ActiveSessionError::Duplicate { session_id } => {
                            map_decoder_boundary(self.inner.runtime.contract_violation(
                                "open_session_json",
                                format!("plugin reused active decoder session id {session_id}"),
                            ))
                        }
                        ActiveSessionError::Exhausted => DecoderError::internal(
                            "host decoder interface reached its active session limit",
                        ),
                    });
                }
                Ok(Box::new(NativeAbiDecoderSession {
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
            OpenCallResult::Failure { status, payload } => Err(decode_decoder_failure(
                &self.inner.runtime,
                "open_session_json",
                status,
                &payload,
            )),
        }
    }
}

struct NativeAbiDecoderSession {
    factory: Arc<NativeAbiDecoderFactoryInner>,
    session_id: u64,
    session_token: u64,
    session_info: DecoderSessionInfo,
    active_leases: HashMap<u64, u64>,
    active_abi_leases: HashSet<u64>,
    next_lease_token: u64,
    closing: bool,
    closed: bool,
}

impl std::fmt::Debug for NativeAbiDecoderSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeAbiDecoderSession")
            .field("session_id", &self.session_id)
            .field("session_token", &self.session_token)
            .field("active_lease_count", &self.active_leases.len())
            .field("closed", &self.closed)
            .finish()
    }
}

impl NativeAbiDecoderSession {
    fn ensure_open(&self) -> Result<(), DecoderError> {
        if self.closed || self.closing {
            Err(DecoderError::NotConfigured)
        } else {
            Ok(())
        }
    }

    fn decode_result<T>(
        &self,
        operation: &'static str,
        result: JsonCallResult,
    ) -> Result<T, DecoderError>
    where
        T: serde::de::DeserializeOwned,
    {
        match result {
            JsonCallResult::Success(payload) => self
                .factory
                .runtime
                .decode_json::<T>(operation, &payload)
                .map_err(map_decoder_boundary),
            JsonCallResult::Failure { status, payload } => Err(decode_decoder_failure(
                &self.factory.runtime,
                operation,
                status,
                &payload,
            )),
        }
    }

    fn release_abi_lease(&mut self, lease_id: u64, disposition: u32) -> Result<(), DecoderError> {
        let result = self
            .factory
            .runtime
            .invoke_cleanup_json("release_native_frame", DECODER_FAILURE_STATUSES, |out| {
                // SAFETY: callback/context/session are validated and output
                // is borrowed for this synchronous cleanup call.
                unsafe {
                    (self.factory.release_native_frame)(
                        self.factory.runtime.context(),
                        self.session_id,
                        lease_id,
                        disposition,
                        out,
                    )
                }
            })
            .map_err(map_decoder_boundary)?;
        let status =
            self.decode_result::<DecoderOperationStatus>("release_native_frame", result)?;
        require_decoder_completed(&self.factory.runtime, "release_native_frame", status)
    }

    fn release_frame(
        &mut self,
        frame: DecoderNativeFrame,
        disposition: u32,
    ) -> Result<(), DecoderError> {
        self.ensure_open()?;
        let token = frame.lease_token.ok_or_else(|| {
            DecoderError::abi_violation("native frame does not carry a plugin lease token")
        })?;
        let (interface_token, session_token, lease_token) = token.host_lease_parts();
        if interface_token != self.factory.runtime.interface_token()
            || session_token != self.session_token
        {
            return Err(DecoderError::abi_violation(
                "native frame lease belongs to a different interface or session",
            ));
        }
        let Some(lease_id) = self.active_leases.remove(&lease_token) else {
            return Err(DecoderError::abi_violation(
                "native frame lease is stale or was already released",
            ));
        };
        self.active_abi_leases.remove(&lease_id);
        self.release_abi_lease(lease_id, disposition)
    }

    fn discard_malformed_lease(&mut self, lease_id: u64) {
        if lease_id == 0 {
            return;
        }
        if self.active_abi_leases.contains(&lease_id) {
            let _ = self.factory.runtime.contract_violation(
                "receive_native_frame",
                "malformed output reused an active native-frame lease id",
            );
            self.drain_session_after_lease_violation();
            return;
        }
        if self
            .release_abi_lease(lease_id, VESPER_RELEASE_DISCARDED)
            .is_err()
        {
            self.drain_session_after_lease_violation();
        }
    }

    fn drain_session_after_lease_violation(&mut self) {
        if self.flush().is_err() {
            let _ = self.close();
        }
    }
}

impl NativeDecoderSession for NativeAbiDecoderSession {
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
            DecoderError::payload_codec(format!("serialize decoder packet failed: {error}"))
        })?;
        let result = self
            .factory
            .runtime
            .invoke_json("send_packet", DECODER_FAILURE_STATUSES, |out| {
                // SAFETY: callback/context/session are validated and packet
                // metadata/data/output are borrowed for this synchronous call.
                unsafe {
                    (self.factory.send_packet)(
                        self.factory.runtime.context(),
                        self.session_id,
                        borrowed_bytes(&packet_json),
                        borrowed_bytes(data),
                        out,
                    )
                }
            })
            .map_err(map_decoder_boundary)?;
        self.decode_result("send_packet", result)
    }

    fn receive_native_frame(&mut self) -> Result<DecoderReceiveNativeFrameOutput, DecoderError> {
        self.ensure_open()?;
        if self.active_leases.len() >= VESPER_MAX_LEASES_PER_SESSION {
            return Err(DecoderError::internal(format!(
                "decoder session reached the {VESPER_MAX_LEASES_PER_SESSION}-frame lease limit"
            )));
        }
        if self.next_lease_token == u64::MAX {
            return Err(DecoderError::internal(
                "host decoder lease token space is exhausted",
            ));
        }
        let mut out = VesperNativeFrameOut::default();
        let call = self
            .factory
            .runtime
            .invoke_callback("receive_native_frame", false, || {
                // SAFETY: callback/context/session are validated and output is
                // host-owned for this synchronous call.
                unsafe {
                    (self.factory.receive_native_frame)(
                        self.factory.runtime.context(),
                        self.session_id,
                        &mut out,
                    )
                }
            });
        let metadata_payload = self
            .factory
            .runtime
            .capture_owned_bytes("receive_native_frame", out.metadata);
        let output_check = self.factory.runtime.validate_out_prefix(
            "receive_native_frame",
            out.struct_size,
            0,
            size_of::<VesperNativeFrameOut>() as u32,
        );
        let raw_status = match call {
            Ok(raw_status) => raw_status,
            Err(error) => {
                self.discard_malformed_lease(out.lease_id);
                return Err(map_decoder_boundary(error));
            }
        };
        let metadata_payload = match metadata_payload {
            Ok(payload) => payload,
            Err(error) => {
                self.discard_malformed_lease(out.lease_id);
                return Err(map_decoder_boundary(error));
            }
        };
        if let Err(error) = output_check {
            self.discard_malformed_lease(out.lease_id);
            return Err(map_decoder_boundary(error));
        }
        let result = match self.factory.runtime.classify_json_status(
            "receive_native_frame",
            raw_status,
            DECODER_FAILURE_STATUSES,
            metadata_payload,
        ) {
            Ok(result) => result,
            Err(error) => {
                self.discard_malformed_lease(out.lease_id);
                return Err(map_decoder_boundary(error));
            }
        };
        let payload = match result {
            JsonCallResult::Success(payload) => payload,
            JsonCallResult::Failure { status, payload } => {
                if out.native_handle != 0 || out.lease_id != 0 || out.requires_release != 0 {
                    self.discard_malformed_lease(out.lease_id);
                    return Err(map_decoder_boundary(
                        self.factory.runtime.contract_violation(
                            "receive_native_frame",
                            "failed frame receive returned native resources",
                        ),
                    ));
                }
                return Err(decode_decoder_failure(
                    &self.factory.runtime,
                    "receive_native_frame",
                    status,
                    &payload,
                ));
            }
        };
        let metadata = match self
            .factory
            .runtime
            .decode_json::<DecoderReceiveNativeFrameMetadata>("receive_native_frame", &payload)
        {
            Ok(metadata) => metadata,
            Err(error) => {
                self.discard_malformed_lease(out.lease_id);
                return Err(map_decoder_boundary(error));
            }
        };
        if out.requires_release > 1 {
            self.discard_malformed_lease(out.lease_id);
            return Err(map_decoder_boundary(
                self.factory.runtime.contract_violation(
                    "receive_native_frame",
                    format!(
                        "requires_release must be 0 or 1, got {}",
                        out.requires_release
                    ),
                ),
            ));
        }
        match metadata.status {
            DecoderReceiveFrameStatus::Frame => {
                let Some(frame_metadata) = metadata.frame else {
                    self.discard_malformed_lease(out.lease_id);
                    return Err(map_decoder_boundary(
                        self.factory.runtime.contract_violation(
                            "receive_native_frame",
                            "frame status is missing frame metadata",
                        ),
                    ));
                };
                let handle = usize::try_from(out.native_handle).map_err(|_| {
                    self.discard_malformed_lease(out.lease_id);
                    map_decoder_boundary(self.factory.runtime.contract_violation(
                        "receive_native_frame",
                        "native handle does not fit this process",
                    ))
                })?;
                if handle == 0 {
                    self.discard_malformed_lease(out.lease_id);
                    return Err(map_decoder_boundary(
                        self.factory.runtime.contract_violation(
                            "receive_native_frame",
                            "frame status returned a zero native handle",
                        ),
                    ));
                }
                let requires_release = out.requires_release == 1;
                if requires_release != (out.lease_id != 0) {
                    self.discard_malformed_lease(out.lease_id);
                    return Err(map_decoder_boundary(
                        self.factory.runtime.contract_violation(
                            "receive_native_frame",
                            "requires_release and lease_id disagree",
                        ),
                    ));
                }
                let lease_token = if requires_release {
                    if self.active_abi_leases.contains(&out.lease_id) {
                        let error = self.factory.runtime.contract_violation(
                            "receive_native_frame",
                            "plugin reused an active native-frame lease id",
                        );
                        // The duplicate ID cannot identify which resource a
                        // direct release would target. Flush the whole session
                        // through the cleanup path and invalidate every token.
                        self.drain_session_after_lease_violation();
                        return Err(map_decoder_boundary(error));
                    }
                    let lease_token = self.next_lease_token;
                    self.next_lease_token += 1;
                    self.active_leases.insert(lease_token, out.lease_id);
                    self.active_abi_leases.insert(out.lease_id);
                    Some(NativeFrameLeaseToken::from_host_lease(
                        self.factory.runtime.interface_token(),
                        self.session_token,
                        lease_token,
                    ))
                } else {
                    None
                };
                Ok(DecoderReceiveNativeFrameOutput::Frame(DecoderNativeFrame {
                    metadata: frame_metadata,
                    handle,
                    lease_token,
                }))
            }
            DecoderReceiveFrameStatus::NeedMoreInput | DecoderReceiveFrameStatus::Eof => {
                if metadata.frame.is_some()
                    || out.native_handle != 0
                    || out.lease_id != 0
                    || out.requires_release != 0
                {
                    self.discard_malformed_lease(out.lease_id);
                    return Err(map_decoder_boundary(
                        self.factory.runtime.contract_violation(
                            "receive_native_frame",
                            "non-frame status returned frame metadata or native resources",
                        ),
                    ));
                }
                if metadata.status == DecoderReceiveFrameStatus::NeedMoreInput {
                    Ok(DecoderReceiveNativeFrameOutput::NeedMoreInput)
                } else {
                    Ok(DecoderReceiveNativeFrameOutput::Eof)
                }
            }
        }
    }

    fn receive_pcm_frame(&mut self) -> Result<DecoderReceivePcmFrameOutput, DecoderError> {
        self.ensure_open()?;
        let Some(receive_pcm_frame) = self.factory.receive_pcm_frame else {
            return Err(DecoderError::UnsupportedCapability {
                capability: "audio-pcm-output".to_owned(),
            });
        };
        let mut out = VesperPcmFrameOut::default();
        let call = self
            .factory
            .runtime
            .invoke_callback("receive_pcm_frame", false, || {
                // SAFETY: callback/context/session are validated and output is
                // host-owned for this synchronous call.
                unsafe {
                    receive_pcm_frame(self.factory.runtime.context(), self.session_id, &mut out)
                }
            });
        let metadata = self
            .factory
            .runtime
            .capture_owned_bytes("receive_pcm_frame", out.metadata);
        let data = self
            .factory
            .runtime
            .capture_owned_bytes("receive_pcm_frame", out.data);
        self.factory
            .runtime
            .validate_out_prefix(
                "receive_pcm_frame",
                out.struct_size,
                out.reserved,
                size_of::<VesperPcmFrameOut>() as u32,
            )
            .map_err(map_decoder_boundary)?;
        let raw_status = call.map_err(map_decoder_boundary)?;
        let metadata = metadata.map_err(map_decoder_boundary)?;
        let data = data.map_err(map_decoder_boundary)?;
        let result = self
            .factory
            .runtime
            .classify_json_status(
                "receive_pcm_frame",
                raw_status,
                DECODER_FAILURE_STATUSES,
                metadata,
            )
            .map_err(map_decoder_boundary)?;
        let payload = match result {
            JsonCallResult::Success(payload) => payload,
            JsonCallResult::Failure { status, payload } => {
                if !data.is_empty() {
                    return Err(map_decoder_boundary(
                        self.factory.runtime.contract_violation(
                            "receive_pcm_frame",
                            "failed PCM receive returned sample bytes",
                        ),
                    ));
                }
                return Err(decode_decoder_failure(
                    &self.factory.runtime,
                    "receive_pcm_frame",
                    status,
                    &payload,
                ));
            }
        };
        let metadata = self
            .factory
            .runtime
            .decode_json::<DecoderReceivePcmFrameMetadata>("receive_pcm_frame", &payload)
            .map_err(map_decoder_boundary)?;
        match metadata.status {
            DecoderReceiveFrameStatus::Frame => {
                let Some(frame_metadata) = metadata.frame else {
                    return Err(map_decoder_boundary(
                        self.factory.runtime.contract_violation(
                            "receive_pcm_frame",
                            "PCM frame status is missing frame metadata",
                        ),
                    ));
                };
                Ok(DecoderReceivePcmFrameOutput::Frame(DecoderPcmFrame {
                    metadata: frame_metadata,
                    data,
                }))
            }
            DecoderReceiveFrameStatus::NeedMoreInput | DecoderReceiveFrameStatus::Eof => {
                if metadata.frame.is_some() || !data.is_empty() {
                    return Err(map_decoder_boundary(
                        self.factory.runtime.contract_violation(
                            "receive_pcm_frame",
                            "non-frame PCM status returned metadata or sample bytes",
                        ),
                    ));
                }
                if metadata.status == DecoderReceiveFrameStatus::NeedMoreInput {
                    Ok(DecoderReceivePcmFrameOutput::NeedMoreInput)
                } else {
                    Ok(DecoderReceivePcmFrameOutput::Eof)
                }
            }
        }
    }

    fn release_native_frame(&mut self, frame: DecoderNativeFrame) -> Result<(), DecoderError> {
        self.release_frame(frame, VESPER_RELEASE_DISCARDED)
    }

    fn release_native_frame_with_presentation(
        &mut self,
        frame: DecoderNativeFrame,
        presented: bool,
    ) -> Result<(), DecoderError> {
        self.release_frame(
            frame,
            if presented {
                VESPER_RELEASE_PRESENTED
            } else {
                VESPER_RELEASE_DISCARDED
            },
        )
    }

    fn flush(&mut self) -> Result<(), DecoderError> {
        self.ensure_open()?;
        self.active_leases.clear();
        self.active_abi_leases.clear();
        let result = self
            .factory
            .runtime
            .invoke_cleanup_json("flush_session", DECODER_FAILURE_STATUSES, |out| {
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
            .map_err(map_decoder_boundary)?;
        let status = self.decode_result::<DecoderOperationStatus>("flush_session", result)?;
        require_decoder_completed(&self.factory.runtime, "flush_session", status)
    }

    fn close(&mut self) -> Result<(), DecoderError> {
        if self.closed {
            return Ok(());
        }
        self.closing = true;
        self.active_leases.clear();
        self.active_abi_leases.clear();
        let result = self
            .factory
            .runtime
            .invoke_cleanup_json("close_session", DECODER_FAILURE_STATUSES, |out| {
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
            .map_err(map_decoder_boundary)?;
        let status = self.decode_result::<DecoderOperationStatus>("close_session", result)?;
        require_decoder_completed(&self.factory.runtime, "close_session", status)?;
        self.factory.active_sessions.remove(self.session_id);
        self.closed = true;
        Ok(())
    }
}

impl Drop for NativeAbiDecoderSession {
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

fn load_decoder_value<T>(
    runtime: &InterfaceRuntime,
    operation: &'static str,
    callback: player_plugin_abi::VesperGetJsonFn,
) -> Result<T, NativeAbiBoundaryError>
where
    T: serde::de::DeserializeOwned,
{
    let result = runtime.invoke_json(operation, DECODER_FAILURE_STATUSES, |out| {
        // SAFETY: callback/context are validated and output is host-owned for
        // this synchronous call.
        unsafe { callback(runtime.context(), out) }
    })?;
    match result {
        JsonCallResult::Success(payload) => runtime.decode_json(operation, &payload),
        JsonCallResult::Failure {
            status: raw_status,
            payload,
        } => {
            let error = decode_decoder_failure(runtime, operation, raw_status, &payload);
            if matches!(&error, DecoderError::AbiViolation { .. }) {
                return Err(runtime.contract_violation(
                    operation,
                    "decoder metadata callback returned an incompatible failure payload",
                ));
            }
            Err(runtime.reported_failure(operation, raw_status, error.to_string()))
        }
    }
}

fn decode_decoder_failure(
    runtime: &InterfaceRuntime,
    operation: &'static str,
    raw_status: VesperStatus,
    payload: &[u8],
) -> DecoderError {
    let error = if raw_status == status::EXHAUSTED && payload.is_empty() {
        DecoderError::internal("decoder interface resource limit exhausted")
    } else {
        match runtime.decode_json::<DecoderError>(operation, payload) {
            Ok(error) => error,
            Err(error) => return map_decoder_boundary(error),
        }
    };
    if decoder_status_matches(raw_status, &error) {
        error
    } else {
        map_decoder_boundary(runtime.contract_violation(
            operation,
            format!("status {raw_status} is inconsistent with decoder error `{error}`"),
        ))
    }
}

fn decoder_status_matches(raw_status: VesperStatus, error: &DecoderError) -> bool {
    match raw_status {
        status::FAILURE => matches!(
            error,
            DecoderError::Internal { .. }
                | DecoderError::NotConfigured
                | DecoderError::NeedMoreInput
                | DecoderError::Eof
        ),
        status::INVALID_ARGUMENT => {
            matches!(
                error,
                DecoderError::InvalidPacket { .. } | DecoderError::PayloadCodec { .. }
            )
        }
        status::UNSUPPORTED => matches!(
            error,
            DecoderError::UnsupportedCodec { .. } | DecoderError::UnsupportedCapability { .. }
        ),
        status::EXHAUSTED => matches!(error, DecoderError::Internal { .. }),
        _ => false,
    }
}

fn require_decoder_completed(
    runtime: &InterfaceRuntime,
    operation: &'static str,
    status: DecoderOperationStatus,
) -> Result<(), DecoderError> {
    if status.completed {
        Ok(())
    } else {
        Err(map_decoder_boundary(runtime.contract_violation(
            operation,
            "successful cleanup reported completed=false",
        )))
    }
}

fn cleanup_decoder_session(
    runtime: &InterfaceRuntime,
    close: player_plugin_abi::VesperSessionOperationFn,
    session_id: u64,
) {
    for _ in 0..OPEN_FAILURE_CLOSE_ATTEMPTS {
        let result = runtime.invoke_cleanup_json(
            "close_session_after_open_failure",
            DECODER_FAILURE_STATUSES,
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
            runtime.decode_json::<DecoderOperationStatus>(
                "close_session_after_open_failure",
                &payload,
            ),
            Ok(DecoderOperationStatus { completed: true })
        ) {
            return;
        }
        let _ = runtime.contract_violation(
            "close_session_after_open_failure",
            "successful orphan close reported completed=false",
        );
    }
}

fn map_decoder_boundary(error: NativeAbiBoundaryError) -> DecoderError {
    DecoderError::abi_violation(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::ptr::NonNull;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use player_plugin::{
        DecoderCapabilities, DecoderFrameFormat, DecoderMediaKind, DecoderNativeFrameMetadata,
        DecoderNativeHandleKind, DecoderPcmFrame, DecoderPcmFrameMetadata, DecoderPcmSampleLayout,
        Plugin, PluginBuilder,
    };
    use player_plugin_abi::{
        NATIVE_DECODER_INTERFACE_ID, VESPER_INTERFACE_MAJOR, VESPER_INTERFACE_MINOR,
        VesperInterfaceHeader, VesperOpenSessionOut, VesperOwnedBytes,
    };
    use serde::Serialize;

    use super::*;
    use crate::native_abi::{CheckedInterfaceTable, CheckedPluginRoot};

    const DECODER_INSTANCE: &str = "dev.vesper.fixture.decoder";

    thread_local! {
        static FIXTURE_PLUGIN_CONFIG: RefCell<Option<FixturePluginConfig>> = const {
            RefCell::new(None)
        };
    }

    #[derive(Default)]
    struct DecoderCounters {
        releases: AtomicUsize,
        flushes: AtomicUsize,
        closes: AtomicUsize,
    }

    #[derive(Clone)]
    struct FixtureDecoderFactory {
        counters: Arc<DecoderCounters>,
        fail_send: bool,
    }

    #[derive(Clone)]
    struct FixturePluginConfig {
        count: usize,
        counters: Arc<DecoderCounters>,
        fail_send: bool,
    }

    impl NativeDecoderPluginFactory for FixtureDecoderFactory {
        fn name(&self) -> &str {
            "fixture decoder"
        }

        fn capabilities(&self) -> DecoderCapabilities {
            DecoderCapabilities {
                supports_pcm_frames: true,
                ..DecoderCapabilities::default()
            }
        }

        fn native_requirements(&self) -> DecoderNativeRequirements {
            DecoderNativeRequirements::default()
        }

        fn open_native_session(
            &self,
            _config: &DecoderSessionConfig,
        ) -> Result<Box<dyn NativeDecoderSession>, DecoderError> {
            Ok(Box::new(FixtureDecoderSession {
                counters: self.counters.clone(),
                fail_send: self.fail_send,
            }))
        }
    }

    struct FixtureDecoderSession {
        counters: Arc<DecoderCounters>,
        fail_send: bool,
    }

    impl NativeDecoderSession for FixtureDecoderSession {
        fn session_info(&self) -> DecoderSessionInfo {
            DecoderSessionInfo::default()
        }

        fn send_packet(
            &mut self,
            _packet: &DecoderPacket,
            _data: &[u8],
        ) -> Result<DecoderPacketResult, DecoderError> {
            if self.fail_send {
                Err(DecoderError::abi_violation("fixture poison"))
            } else {
                Ok(DecoderPacketResult::default())
            }
        }

        fn receive_native_frame(
            &mut self,
        ) -> Result<DecoderReceiveNativeFrameOutput, DecoderError> {
            Ok(DecoderReceiveNativeFrameOutput::Frame(DecoderNativeFrame {
                metadata: native_frame_metadata(),
                handle: 0xfeed,
                lease_token: None,
            }))
        }

        fn receive_pcm_frame(&mut self) -> Result<DecoderReceivePcmFrameOutput, DecoderError> {
            Ok(DecoderReceivePcmFrameOutput::Frame(DecoderPcmFrame {
                metadata: pcm_frame_metadata(),
                data: vec![1, 2, 3, 4],
            }))
        }

        fn release_native_frame(&mut self, _frame: DecoderNativeFrame) -> Result<(), DecoderError> {
            self.counters.releases.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn flush(&mut self) -> Result<(), DecoderError> {
            self.counters.flushes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn close(&mut self) -> Result<(), DecoderError> {
            self.counters.closes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn native_frame_metadata() -> DecoderNativeFrameMetadata {
        DecoderNativeFrameMetadata {
            media_kind: DecoderMediaKind::Video,
            format: DecoderFrameFormat::Nv12,
            codec: "h264".to_owned(),
            pts_us: Some(1),
            duration_us: Some(2),
            width: 16,
            height: 16,
            coded_width: None,
            coded_height: None,
            visible_rect: None,
            handle_kind: DecoderNativeHandleKind::D3D11Texture2D,
            pipeline_profile: None,
            color_space: None,
            hdr_metadata: None,
            color: None,
            hdr: None,
            sync_info: None,
            transform: None,
            frame_id: Some(7),
            release_tracking: None,
        }
    }

    fn pcm_frame_metadata() -> DecoderPcmFrameMetadata {
        DecoderPcmFrameMetadata::audio(
            "aac",
            DecoderFrameFormat::F32,
            48_000,
            2,
            DecoderPcmSampleLayout::Interleaved,
            1,
        )
    }

    fn generated_decoder_wrappers(
        count: usize,
        counters: Arc<DecoderCounters>,
        fail_send: bool,
    ) -> Vec<NativeAbiDecoderPluginFactory> {
        FIXTURE_PLUGIN_CONFIG.with(|config| {
            *config.borrow_mut() = Some(FixturePluginConfig {
                count,
                counters,
                fail_send,
            });
        });
        let root_ptr = player_plugin::__private::export_plugin(fixture_plugin);
        FIXTURE_PLUGIN_CONFIG.with(|config| {
            let _ = config.borrow_mut().take();
        });
        let root =
            // SAFETY: the generated root transfers ownership into the checked loader.
            unsafe { CheckedPluginRoot::from_raw(root_ptr, None) }.expect("checked decoder root");
        root.interfaces
            .iter()
            .filter_map(|interface| {
                let CheckedInterfaceTable::NativeDecoder(table) = interface.table else {
                    return None;
                };
                Some(
                    NativeAbiDecoderPluginFactory::new(
                        &root.plugin_id,
                        root.plugin_name.clone(),
                        &interface.descriptor.instance_id,
                        root.owner.clone(),
                        table,
                    )
                    .expect("checked decoder wrapper"),
                )
            })
            .collect()
    }

    fn fixture_plugin() -> Plugin {
        let config = FIXTURE_PLUGIN_CONFIG
            .with(|config| config.borrow().clone().expect("fixture plugin config"));
        let mut builder = PluginBuilder::new("dev.vesper.decoder-fixture", "Decoder fixture")
            .expect("plugin builder");
        for index in 0..config.count {
            builder = builder
                .with_native_decoder(
                    format!("{DECODER_INSTANCE}.instance{index}"),
                    FixtureDecoderFactory {
                        counters: config.counters.clone(),
                        fail_send: config.fail_send,
                    },
                )
                .expect("decoder interface");
        }
        builder.build().expect("decoder fixture plugin")
    }

    fn open_decoder_session(
        factory: &NativeAbiDecoderPluginFactory,
    ) -> Box<dyn NativeDecoderSession> {
        factory
            .open_native_session(&DecoderSessionConfig::default())
            .expect("decoder session")
    }

    fn receive_frame(session: &mut dyn NativeDecoderSession) -> DecoderNativeFrame {
        let output = session.receive_native_frame().expect("native frame");
        let DecoderReceiveNativeFrameOutput::Frame(frame) = output else {
            panic!("expected native frame")
        };
        frame
    }

    #[test]
    fn identical_native_handles_have_distinct_lease_tokens() {
        let counters = Arc::new(DecoderCounters::default());
        let factory = generated_decoder_wrappers(1, counters.clone(), false)
            .pop()
            .expect("factory");
        let mut session = open_decoder_session(&factory);
        let first = receive_frame(session.as_mut());
        let second = receive_frame(session.as_mut());
        assert_eq!(first.handle, second.handle);
        assert_ne!(first.lease_token, second.lease_token);
        session.release_native_frame(first).expect("release first");
        session
            .release_native_frame(second)
            .expect("release second");
        assert_eq!(counters.releases.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn decoder_lease_tokens_reject_wrong_session_interface_and_reuse() {
        let counters = Arc::new(DecoderCounters::default());
        let factories = generated_decoder_wrappers(2, counters.clone(), false);
        let mut first_session = open_decoder_session(&factories[0]);
        let mut second_session = open_decoder_session(&factories[0]);
        let mut other_interface_session = open_decoder_session(&factories[1]);

        let first = receive_frame(first_session.as_mut());
        assert!(matches!(
            second_session.release_native_frame(first.clone()),
            Err(DecoderError::AbiViolation { .. })
        ));
        assert!(matches!(
            other_interface_session.release_native_frame(first.clone()),
            Err(DecoderError::AbiViolation { .. })
        ));
        first_session
            .release_native_frame(first.clone())
            .expect("producing session releases frame");
        assert!(matches!(
            first_session.release_native_frame(first),
            Err(DecoderError::AbiViolation { .. })
        ));
        assert_eq!(counters.releases.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn decoder_lease_limit_and_flush_drain_all_frames() {
        let counters = Arc::new(DecoderCounters::default());
        let factory = generated_decoder_wrappers(1, counters.clone(), false)
            .pop()
            .expect("factory");
        let mut session = open_decoder_session(&factory);
        for _ in 0..VESPER_MAX_LEASES_PER_SESSION {
            let _frame = receive_frame(session.as_mut());
        }
        assert!(matches!(
            session.receive_native_frame(),
            Err(DecoderError::Internal { .. })
        ));
        session.flush().expect("flush leases");
        assert_eq!(
            counters.releases.load(Ordering::SeqCst),
            VESPER_MAX_LEASES_PER_SESSION
        );
        assert_eq!(counters.flushes.load(Ordering::SeqCst), 1);
        let frame = receive_frame(session.as_mut());
        session
            .release_native_frame(frame)
            .expect("post-flush release");
    }

    #[test]
    fn decoder_cleanup_remains_callable_after_interface_poison() {
        let counters = Arc::new(DecoderCounters::default());
        let factory = generated_decoder_wrappers(1, counters.clone(), true)
            .pop()
            .expect("factory");
        let mut session = open_decoder_session(&factory);
        assert!(matches!(
            session.send_packet(&DecoderPacket::default(), &[]),
            Err(DecoderError::AbiViolation { .. })
        ));
        session.flush().expect("cleanup flush after poison");
        session.close().expect("cleanup close after poison");
        session.close().expect("idempotent close");
        assert_eq!(counters.flushes.load(Ordering::SeqCst), 1);
        assert_eq!(counters.closes.load(Ordering::SeqCst), 1);
    }

    const RAW_MODE_MALFORMED_METADATA: usize = 1;
    const RAW_MODE_DUPLICATE_LEASE: usize = 2;
    const RAW_MODE_PCM: usize = 3;

    #[derive(Default)]
    struct RawDecoderCounters {
        freed: AtomicUsize,
        released: AtomicUsize,
        flushed: AtomicUsize,
        closed: AtomicUsize,
    }

    struct RawDecoderContext {
        mode: usize,
        counters: Arc<RawDecoderCounters>,
    }

    unsafe fn raw_context<'a>(context: *mut c_void) -> Option<&'a RawDecoderContext> {
        // SAFETY: every raw fixture callback receives the live context installed
        // in the interface header and owned by `PluginOwner`.
        unsafe { context.cast::<RawDecoderContext>().as_ref() }
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
        if let Some(context) =
            // SAFETY: the owner pointer is the raw fixture context.
            unsafe { raw_context(owner) }
        {
            context.counters.freed.fetch_add(1, Ordering::SeqCst);
        }
        // SAFETY: fixture outputs allocate every owned byte sequence with
        // `VesperOwnedBytes::from_vec` and transfer it back exactly once.
        drop(unsafe { bytes.into_vec() });
    }

    unsafe extern "C" fn raw_destroy(owner: *mut c_void) {
        if !owner.is_null() {
            // SAFETY: `raw_decoder_factory` transfers one boxed context to the owner.
            drop(unsafe { Box::from_raw(owner.cast::<RawDecoderContext>()) });
        }
    }

    unsafe extern "C" fn raw_capabilities(
        context: *mut c_void,
        out: *mut VesperJsonOut,
    ) -> VesperStatus {
        let supports_pcm_frames =
            // SAFETY: callback context follows the fixture table contract.
            unsafe { raw_context(context) }
                .is_some_and(|context| context.mode == RAW_MODE_PCM);
        // SAFETY: forwarded host output retains the callback contract.
        unsafe {
            write_json(
                out,
                &DecoderCapabilities {
                    supports_pcm_frames,
                    ..DecoderCapabilities::default()
                },
            )
        }
    }

    unsafe extern "C" fn raw_native_requirements(
        _context: *mut c_void,
        out: *mut VesperJsonOut,
    ) -> VesperStatus {
        // SAFETY: forwarded host output retains the callback contract.
        unsafe { write_json(out, &DecoderNativeRequirements::default()) }
    }

    unsafe extern "C" fn raw_open(
        _context: *mut c_void,
        _config: VesperByteSlice,
        out: *mut VesperOpenSessionOut,
    ) -> VesperStatus {
        let Some(out) =
            // SAFETY: the host passes a writable, initialized output.
            (unsafe { out.as_mut() })
        else {
            return status::INVALID_ARGUMENT;
        };
        let payload = match serde_json::to_vec(&DecoderSessionInfo::default()) {
            Ok(payload) => payload,
            Err(_) => return status::FAILURE,
        };
        out.session_id = 1;
        out.payload = VesperOwnedBytes::from_vec(payload);
        status::OK
    }

    unsafe extern "C" fn raw_send_packet(
        _context: *mut c_void,
        _session_id: u64,
        _packet_json: VesperByteSlice,
        _packet_data: VesperByteSlice,
        out: *mut VesperJsonOut,
    ) -> VesperStatus {
        // SAFETY: forwarded host output retains the callback contract.
        unsafe { write_json(out, &DecoderPacketResult::default()) }
    }

    unsafe extern "C" fn raw_receive_native_frame(
        context: *mut c_void,
        _session_id: u64,
        out: *mut VesperNativeFrameOut,
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
        let metadata = if context.mode == RAW_MODE_MALFORMED_METADATA {
            b"{".to_vec()
        } else {
            match serde_json::to_vec(&DecoderReceiveNativeFrameMetadata::frame(
                native_frame_metadata(),
            )) {
                Ok(metadata) => metadata,
                Err(_) => return status::FAILURE,
            }
        };
        out.metadata = VesperOwnedBytes::from_vec(metadata);
        out.native_handle = 0xfeed;
        out.lease_id = 9;
        out.requires_release = 1;
        status::OK
    }

    unsafe extern "C" fn raw_release(
        context: *mut c_void,
        _session_id: u64,
        _lease_id: u64,
        _disposition: u32,
        out: *mut VesperJsonOut,
    ) -> VesperStatus {
        let Some(context) =
            // SAFETY: callback context follows the fixture table contract.
            (unsafe { raw_context(context) })
        else {
            return status::INVALID_ARGUMENT;
        };
        context.counters.released.fetch_add(1, Ordering::SeqCst);
        // SAFETY: forwarded host output retains the callback contract.
        unsafe { write_json(out, &DecoderOperationStatus { completed: true }) }
    }

    unsafe extern "C" fn raw_flush(
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
        context.counters.flushed.fetch_add(1, Ordering::SeqCst);
        // SAFETY: forwarded host output retains the callback contract.
        unsafe { write_json(out, &DecoderOperationStatus { completed: true }) }
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
        context.counters.closed.fetch_add(1, Ordering::SeqCst);
        // SAFETY: forwarded host output retains the callback contract.
        unsafe { write_json(out, &DecoderOperationStatus { completed: true }) }
    }

    unsafe extern "C" fn raw_receive_pcm(
        _context: *mut c_void,
        _session_id: u64,
        out: *mut VesperPcmFrameOut,
    ) -> VesperStatus {
        let Some(out) =
            // SAFETY: the host passes a writable, initialized output.
            (unsafe { out.as_mut() })
        else {
            return status::INVALID_ARGUMENT;
        };
        let metadata = match serde_json::to_vec(&DecoderReceivePcmFrameMetadata::frame(
            pcm_frame_metadata(),
        )) {
            Ok(metadata) => metadata,
            Err(_) => return status::FAILURE,
        };
        out.metadata = VesperOwnedBytes::from_vec(metadata);
        out.data = VesperOwnedBytes::from_vec(vec![1, 2, 3, 4]);
        status::OK
    }

    fn raw_decoder_factory(
        mode: usize,
    ) -> (NativeAbiDecoderPluginFactory, Arc<RawDecoderCounters>) {
        let counters = Arc::new(RawDecoderCounters::default());
        let context = Box::new(RawDecoderContext {
            mode,
            counters: counters.clone(),
        });
        let context =
            NonNull::new(Box::into_raw(context).cast::<c_void>()).expect("raw decoder context");
        let owner = Arc::new(PluginOwner {
            owner: context,
            free_bytes: raw_free,
            destroy_owner: raw_destroy,
            library: None,
        });
        let table = VesperNativeDecoder {
            header: VesperInterfaceHeader::new(
                size_of::<VesperNativeDecoder>() as u32,
                NATIVE_DECODER_INTERFACE_ID,
                VESPER_INTERFACE_MAJOR,
                VESPER_INTERFACE_MINOR,
                context.as_ptr(),
            ),
            capabilities_json: Some(raw_capabilities),
            native_requirements_json: Some(raw_native_requirements),
            open_session_json: Some(raw_open),
            send_packet: Some(raw_send_packet),
            receive_native_frame: Some(raw_receive_native_frame),
            release_native_frame: Some(raw_release),
            flush_session: Some(raw_flush),
            close_session: Some(raw_close),
            receive_pcm_frame: Some(raw_receive_pcm),
        };
        let factory = NativeAbiDecoderPluginFactory::new(
            "dev.vesper.raw-decoder",
            "Raw decoder".to_owned(),
            DECODER_INSTANCE,
            owner,
            table,
        )
        .expect("raw decoder wrapper");
        (factory, counters)
    }

    #[test]
    fn malformed_native_frame_metadata_releases_returned_lease() {
        let (factory, counters) = raw_decoder_factory(RAW_MODE_MALFORMED_METADATA);
        let mut session = open_decoder_session(&factory);
        assert!(matches!(
            session.receive_native_frame(),
            Err(DecoderError::AbiViolation { .. })
        ));
        assert_eq!(counters.released.load(Ordering::SeqCst), 1);
        session.close().expect("close after poison");
        assert_eq!(counters.closed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn duplicate_active_lease_flushes_without_ambiguous_direct_release() {
        let (factory, counters) = raw_decoder_factory(RAW_MODE_DUPLICATE_LEASE);
        let mut session = open_decoder_session(&factory);
        let frame = receive_frame(session.as_mut());
        assert!(matches!(
            session.receive_native_frame(),
            Err(DecoderError::AbiViolation { .. })
        ));
        assert_eq!(counters.released.load(Ordering::SeqCst), 0);
        assert_eq!(counters.flushed.load(Ordering::SeqCst), 1);
        assert!(matches!(
            session.release_native_frame(frame),
            Err(DecoderError::AbiViolation { .. })
        ));
    }

    #[test]
    fn pcm_metadata_and_data_are_reclaimed_exactly_once() {
        let (factory, counters) = raw_decoder_factory(RAW_MODE_PCM);
        let mut session = open_decoder_session(&factory);
        let baseline = counters.freed.load(Ordering::SeqCst);
        let output = session.receive_pcm_frame().expect("PCM frame");
        assert!(matches!(
            output,
            DecoderReceivePcmFrameOutput::Frame(DecoderPcmFrame { data, .. }) if data == [1, 2, 3, 4]
        ));
        assert_eq!(counters.freed.load(Ordering::SeqCst), baseline + 2);
    }

    #[test]
    fn duplicate_active_raw_session_id_is_rejected_without_ambiguous_close() {
        let (factory, counters) = raw_decoder_factory(RAW_MODE_DUPLICATE_LEASE);
        let mut first = open_decoder_session(&factory);
        assert!(matches!(
            factory.open_native_session(&DecoderSessionConfig::default()),
            Err(DecoderError::AbiViolation { .. })
        ));
        assert_eq!(counters.closed.load(Ordering::SeqCst), 0);
        first
            .close()
            .expect("cleanup remains callable after poison");
        assert_eq!(counters.closed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn reused_raw_session_and_lease_ids_receive_new_host_tokens() {
        let (factory, counters) = raw_decoder_factory(RAW_MODE_DUPLICATE_LEASE);
        let mut first = open_decoder_session(&factory);
        let stale = receive_frame(first.as_mut());
        first.close().expect("close first raw session");

        let mut second = open_decoder_session(&factory);
        let current = receive_frame(second.as_mut());
        assert_ne!(stale.lease_token, current.lease_token);
        assert!(matches!(
            second.release_native_frame(stale),
            Err(DecoderError::AbiViolation { .. })
        ));
        second
            .release_native_frame(current)
            .expect("release current raw lease");
        assert_eq!(counters.released.load(Ordering::SeqCst), 1);
    }
}
