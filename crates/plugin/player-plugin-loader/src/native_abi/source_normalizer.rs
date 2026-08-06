use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use player_plugin::{
    SourceNormalizerError, SourceNormalizerOperationStatus, SourceNormalizerPacketCapabilities,
    SourceNormalizerPacketLease, SourceNormalizerPacketPluginFactory, SourceNormalizerPacketSeek,
    SourceNormalizerPacketSession, SourceNormalizerPacketSessionConfig,
    SourceNormalizerPacketStreamInfo, SourceNormalizerReadPacketMetadata,
    SourceNormalizerReadPacketStatus, SourceNormalizerResourceCapabilities,
    SourceNormalizerResourcePluginFactory, SourceNormalizerResourceSession,
    SourceNormalizerResourceSessionConfig, SourceNormalizerResourceSessionInfo,
    SourceNormalizerResourceSessionStatus, SourceNormalizerResourceSessionWaitStatus,
    validate_source_normalizer_plugin_input,
};
use player_plugin_abi::{
    VESPER_MAX_LEASES_PER_SESSION, VESPER_MAX_PACKET_BYTES, VesperByteSlice, VesperJsonOut,
    VesperPacketOut, VesperSessionId, VesperSourceNormalizerPacket, VesperSourceNormalizerResource,
    VesperStatus, status,
};

use super::PluginOwner;
use super::runtime::{
    ActiveSessionError, ActiveSessionRegistry, InterfaceRuntime, JsonCallResult,
    NativeAbiBoundaryError, OPEN_FAILURE_CLOSE_ATTEMPTS, OpenCallResult, borrowed_bytes,
};

const SOURCE_FAILURE_STATUSES: &[VesperStatus] = &[
    status::FAILURE,
    status::INVALID_ARGUMENT,
    status::UNSUPPORTED,
    status::TIMEOUT,
    status::EXHAUSTED,
];

static NEXT_HOST_PACKET_TOKEN: AtomicU64 = AtomicU64::new(1);

type PacketReadCall = unsafe extern "C" fn(
    context: *mut c_void,
    session_id: VesperSessionId,
    out: *mut VesperPacketOut,
) -> VesperStatus;
type PacketReleaseCall = unsafe extern "C" fn(
    context: *mut c_void,
    session_id: VesperSessionId,
    lease_id: u64,
    out: *mut VesperJsonOut,
) -> VesperStatus;
type PacketSeekCall = unsafe extern "C" fn(
    context: *mut c_void,
    session_id: VesperSessionId,
    seek_json: VesperByteSlice,
    out: *mut VesperJsonOut,
) -> VesperStatus;
type ResourceWaitCall = unsafe extern "C" fn(
    context: *mut c_void,
    session_id: VesperSessionId,
    timeout_ms: u64,
    out: *mut VesperJsonOut,
) -> VesperStatus;

#[derive(Debug)]
struct NativeAbiSourcePacketFactoryInner {
    runtime: Arc<InterfaceRuntime>,
    name: String,
    capabilities: SourceNormalizerPacketCapabilities,
    open_session: player_plugin_abi::VesperOpenSessionFn,
    read_packet: PacketReadCall,
    release_packet: PacketReleaseCall,
    flush_session: player_plugin_abi::VesperSessionOperationFn,
    close_session: player_plugin_abi::VesperSessionOperationFn,
    seek_session: Option<PacketSeekCall>,
    active_sessions: ActiveSessionRegistry,
}

#[derive(Debug, Clone)]
pub(crate) struct NativeAbiSourceNormalizerPacketPluginFactory {
    inner: Arc<NativeAbiSourcePacketFactoryInner>,
}

impl NativeAbiSourceNormalizerPacketPluginFactory {
    pub(super) fn new(
        plugin_id: &str,
        plugin_name: String,
        instance_id: &str,
        owner: Arc<PluginOwner>,
        table: VesperSourceNormalizerPacket,
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
        let read_packet = required_callback(&runtime, "read_packet", table.read_packet)?;
        let release_packet = required_callback(&runtime, "release_packet", table.release_packet)?;
        let flush_session = required_callback(&runtime, "flush_session", table.flush_session)?;
        let close_session = required_callback(&runtime, "close_session", table.close_session)?;
        let capabilities = load_source_value::<SourceNormalizerPacketCapabilities>(
            &runtime,
            "capabilities_json",
            capabilities_json,
        )?;
        if capabilities.supports_seek && table.seek_session_json.is_none() {
            return Err(runtime.contract_violation(
                "construct_wrapper",
                "packet normalizer advertises seek support without seek_session_json",
            ));
        }
        Ok(Self {
            inner: Arc::new(NativeAbiSourcePacketFactoryInner {
                runtime,
                name: plugin_name,
                capabilities,
                open_session,
                read_packet,
                release_packet,
                flush_session,
                close_session,
                seek_session: table.seek_session_json,
                active_sessions: ActiveSessionRegistry::default(),
            }),
        })
    }
}

impl SourceNormalizerPacketPluginFactory for NativeAbiSourceNormalizerPacketPluginFactory {
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
        validate_source_normalizer_plugin_input(&config.input, &config.headers)?;
        let config_json = serde_json::to_vec(config).map_err(|error| {
            SourceNormalizerError::payload_codec(format!(
                "serialize packet normalizer session config failed: {error}"
            ))
        })?;
        let reservation = reserve_source_session(&self.inner.active_sessions, "packet")?;
        let result = self
            .inner
            .runtime
            .invoke_open(
                "open_session_json",
                SOURCE_FAILURE_STATUSES,
                |out| {
                    // SAFETY: callback/context are validated and config/output
                    // are borrowed for this synchronous call.
                    unsafe {
                        (self.inner.open_session)(
                            self.inner.runtime.context(),
                            borrowed_bytes(&config_json),
                            out,
                        )
                    }
                },
                |session_id| {
                    cleanup_source_session(
                        &self.inner.runtime,
                        self.inner.close_session,
                        session_id,
                    );
                },
            )
            .map_err(map_source_boundary)?;
        match result {
            OpenCallResult::Success {
                session_id,
                payload,
            } => {
                let stream_info = match self
                    .inner
                    .runtime
                    .decode_json::<SourceNormalizerPacketStreamInfo>("open_session_json", &payload)
                {
                    Ok(stream_info) => stream_info,
                    Err(error) => {
                        cleanup_source_session(
                            &self.inner.runtime,
                            self.inner.close_session,
                            session_id,
                        );
                        return Err(map_source_boundary(error));
                    }
                };
                register_source_session(reservation, &self.inner.runtime, "packet", session_id)?;
                Ok(Box::new(NativeAbiSourceNormalizerPacketSession {
                    factory: self.inner.clone(),
                    session_id,
                    stream_info,
                    packets: HashMap::new(),
                    active_abi_leases: HashSet::new(),
                    closing: false,
                    closed: false,
                }))
            }
            OpenCallResult::Failure { status, payload } => Err(decode_source_failure(
                &self.inner.runtime,
                "open_session_json",
                status,
                &payload,
            )),
        }
    }
}

#[derive(Debug)]
struct PacketLeaseState {
    abi_lease_id: u64,
    data: Vec<u8>,
}

struct NativeAbiSourceNormalizerPacketSession {
    factory: Arc<NativeAbiSourcePacketFactoryInner>,
    session_id: u64,
    stream_info: SourceNormalizerPacketStreamInfo,
    packets: HashMap<usize, PacketLeaseState>,
    active_abi_leases: HashSet<u64>,
    closing: bool,
    closed: bool,
}

impl std::fmt::Debug for NativeAbiSourceNormalizerPacketSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeAbiSourceNormalizerPacketSession")
            .field("session_id", &self.session_id)
            .field("active_packet_count", &self.packets.len())
            .field("closed", &self.closed)
            .finish()
    }
}

impl NativeAbiSourceNormalizerPacketSession {
    fn ensure_open(&self) -> Result<(), SourceNormalizerError> {
        if self.closed || self.closing {
            Err(SourceNormalizerError::NotConfigured)
        } else {
            Ok(())
        }
    }

    fn release_abi_lease(&self, lease_id: u64) -> Result<(), SourceNormalizerError> {
        let result = self
            .factory
            .runtime
            .invoke_cleanup_json("release_packet", SOURCE_FAILURE_STATUSES, |out| {
                // SAFETY: callback/context/session are validated and output is
                // borrowed for this synchronous cleanup call.
                unsafe {
                    (self.factory.release_packet)(
                        self.factory.runtime.context(),
                        self.session_id,
                        lease_id,
                        out,
                    )
                }
            })
            .map_err(map_source_boundary)?;
        let status = decode_source_result::<SourceNormalizerOperationStatus>(
            &self.factory.runtime,
            "release_packet",
            result,
        )?;
        require_source_completed(&self.factory.runtime, "release_packet", status)
    }

    fn discard_malformed_lease(&mut self, lease_id: u64) {
        if lease_id == 0 {
            return;
        }
        if self.active_abi_leases.contains(&lease_id) {
            let _ = self.factory.runtime.contract_violation(
                "read_packet",
                "malformed output reused an active packet lease id",
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

    fn invalidate_packets(&mut self) {
        self.packets.clear();
        self.active_abi_leases.clear();
    }
}

impl SourceNormalizerPacketSession for NativeAbiSourceNormalizerPacketSession {
    fn stream_info(&self) -> SourceNormalizerPacketStreamInfo {
        self.stream_info.clone()
    }

    fn read_packet(&mut self) -> Result<SourceNormalizerPacketLease<'_>, SourceNormalizerError> {
        self.ensure_open()?;
        if self.packets.len() >= VESPER_MAX_LEASES_PER_SESSION {
            return Err(SourceNormalizerError::resource_exhausted(format!(
                "packet session reached the {VESPER_MAX_LEASES_PER_SESSION}-lease limit"
            )));
        }
        let mut out = VesperPacketOut::default();
        let call = self
            .factory
            .runtime
            .invoke_callback("read_packet", false, || {
                // SAFETY: callback/context/session are validated and output is
                // host-owned for this synchronous call.
                unsafe {
                    (self.factory.read_packet)(
                        self.factory.runtime.context(),
                        self.session_id,
                        &mut out,
                    )
                }
            });
        let metadata_payload = self
            .factory
            .runtime
            .capture_owned_bytes("read_packet", out.metadata);
        let output_check = self.factory.runtime.validate_out_prefix(
            "read_packet",
            out.struct_size,
            out.reserved,
            size_of::<VesperPacketOut>() as u32,
        );
        let raw_status = match call {
            Ok(status) => status,
            Err(error) => {
                self.discard_malformed_lease(out.lease_id);
                return Err(map_source_boundary(error));
            }
        };
        let metadata_payload = match metadata_payload {
            Ok(payload) => payload,
            Err(error) => {
                self.discard_malformed_lease(out.lease_id);
                return Err(map_source_boundary(error));
            }
        };
        if let Err(error) = output_check {
            self.discard_malformed_lease(out.lease_id);
            return Err(map_source_boundary(error));
        }
        let result = match self.factory.runtime.classify_json_status(
            "read_packet",
            raw_status,
            SOURCE_FAILURE_STATUSES,
            metadata_payload,
        ) {
            Ok(result) => result,
            Err(error) => {
                self.discard_malformed_lease(out.lease_id);
                return Err(map_source_boundary(error));
            }
        };
        let payload = match result {
            JsonCallResult::Success(payload) => payload,
            JsonCallResult::Failure { status, payload } => {
                if out.lease_id != 0 || out.data.len != 0 || !out.data.data.is_null() {
                    self.discard_malformed_lease(out.lease_id);
                    return Err(map_source_boundary(
                        self.factory.runtime.contract_violation(
                            "read_packet",
                            "failed packet read returned borrowed packet resources",
                        ),
                    ));
                }
                return Err(decode_source_failure(
                    &self.factory.runtime,
                    "read_packet",
                    status,
                    &payload,
                ));
            }
        };
        let metadata = match self
            .factory
            .runtime
            .decode_json::<SourceNormalizerReadPacketMetadata>("read_packet", &payload)
        {
            Ok(metadata) => metadata,
            Err(error) => {
                self.discard_malformed_lease(out.lease_id);
                return Err(map_source_boundary(error));
            }
        };
        match metadata.status {
            SourceNormalizerReadPacketStatus::Packet => {
                if metadata.packet.is_none() || out.lease_id == 0 {
                    self.discard_malformed_lease(out.lease_id);
                    return Err(map_source_boundary(
                        self.factory.runtime.contract_violation(
                            "read_packet",
                            "packet status requires packet metadata and a non-zero lease id",
                        ),
                    ));
                }
                if self.active_abi_leases.contains(&out.lease_id) {
                    let error = self.factory.runtime.contract_violation(
                        "read_packet",
                        "plugin reused an active packet lease id",
                    );
                    self.drain_after_lease_violation();
                    return Err(map_source_boundary(error));
                }
                let data = match copy_packet_data(&self.factory.runtime, out.data) {
                    Ok(data) => data,
                    Err(error) => {
                        self.discard_malformed_lease(out.lease_id);
                        return Err(map_source_boundary(error));
                    }
                };
                let host_token = match allocate_host_packet_token() {
                    Ok(token) => token,
                    Err(error) => {
                        self.discard_malformed_lease(out.lease_id);
                        return Err(error);
                    }
                };
                self.active_abi_leases.insert(out.lease_id);
                self.packets.insert(
                    host_token,
                    PacketLeaseState {
                        abi_lease_id: out.lease_id,
                        data,
                    },
                );
                let packet = self.packets.get(&host_token).ok_or_else(|| {
                    SourceNormalizerError::internal("host packet lease registration was lost")
                })?;
                Ok(SourceNormalizerPacketLease {
                    metadata,
                    data: &packet.data,
                    handle: host_token,
                })
            }
            SourceNormalizerReadPacketStatus::NeedMoreData
            | SourceNormalizerReadPacketStatus::EndOfStream => {
                if metadata.packet.is_some()
                    || out.lease_id != 0
                    || out.data.len != 0
                    || !out.data.data.is_null()
                {
                    self.discard_malformed_lease(out.lease_id);
                    return Err(map_source_boundary(
                        self.factory.runtime.contract_violation(
                            "read_packet",
                            "non-packet status returned packet metadata or borrowed resources",
                        ),
                    ));
                }
                Ok(SourceNormalizerPacketLease {
                    metadata,
                    data: &[],
                    handle: 0,
                })
            }
        }
    }

    fn release_packet(&mut self, packet_handle: usize) -> Result<(), SourceNormalizerError> {
        self.ensure_open()?;
        let packet = self.packets.remove(&packet_handle).ok_or_else(|| {
            SourceNormalizerError::abi_violation(
                "packet lease is stale or belongs to a different session or interface",
            )
        })?;
        self.active_abi_leases.remove(&packet.abi_lease_id);
        self.release_abi_lease(packet.abi_lease_id)
    }

    fn seek(
        &mut self,
        seek: &SourceNormalizerPacketSeek,
    ) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        self.ensure_open()?;
        let callback = self
            .factory
            .seek_session
            .ok_or_else(|| SourceNormalizerError::unsupported_operation("seek"))?;
        let seek_json = serde_json::to_vec(seek).map_err(|error| {
            SourceNormalizerError::payload_codec(format!(
                "serialize packet normalizer seek failed: {error}"
            ))
        })?;
        self.invalidate_packets();
        let result = self
            .factory
            .runtime
            .invoke_json("seek_session_json", SOURCE_FAILURE_STATUSES, |out| {
                // SAFETY: callback/context/session and borrowed JSON are valid
                // for this synchronous call.
                unsafe {
                    callback(
                        self.factory.runtime.context(),
                        self.session_id,
                        borrowed_bytes(&seek_json),
                        out,
                    )
                }
            })
            .map_err(map_source_boundary)?;
        let status: SourceNormalizerOperationStatus =
            decode_source_result(&self.factory.runtime, "seek_session_json", result)?;
        require_source_completed(&self.factory.runtime, "seek_session_json", status.clone())?;
        Ok(status)
    }

    fn flush(&mut self) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        self.ensure_open()?;
        self.invalidate_packets();
        let result = self
            .factory
            .runtime
            .invoke_cleanup_json("flush_session", SOURCE_FAILURE_STATUSES, |out| {
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
            .map_err(map_source_boundary)?;
        let status: SourceNormalizerOperationStatus =
            decode_source_result(&self.factory.runtime, "flush_session", result)?;
        require_source_completed(&self.factory.runtime, "flush_session", status.clone())?;
        Ok(status)
    }

    fn close(&mut self) -> Result<(), SourceNormalizerError> {
        if self.closed {
            return Ok(());
        }
        self.closing = true;
        self.invalidate_packets();
        let result = self
            .factory
            .runtime
            .invoke_cleanup_json("close_session", SOURCE_FAILURE_STATUSES, |out| {
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
            .map_err(map_source_boundary)?;
        let status = decode_source_result(&self.factory.runtime, "close_session", result)?;
        require_source_completed(&self.factory.runtime, "close_session", status)?;
        self.factory.active_sessions.remove(self.session_id);
        self.closed = true;
        Ok(())
    }
}

impl Drop for NativeAbiSourceNormalizerPacketSession {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[derive(Debug)]
struct NativeAbiSourceResourceFactoryInner {
    runtime: Arc<InterfaceRuntime>,
    name: String,
    capabilities: SourceNormalizerResourceCapabilities,
    open_session: player_plugin_abi::VesperOpenSessionFn,
    poll_session: player_plugin_abi::VesperSessionOperationFn,
    wait_session: ResourceWaitCall,
    cancel_session: player_plugin_abi::VesperSessionOperationFn,
    close_session: player_plugin_abi::VesperSessionOperationFn,
    active_sessions: ActiveSessionRegistry,
}

#[derive(Debug, Clone)]
pub(crate) struct NativeAbiSourceNormalizerResourcePluginFactory {
    inner: Arc<NativeAbiSourceResourceFactoryInner>,
}

impl NativeAbiSourceNormalizerResourcePluginFactory {
    pub(super) fn new(
        plugin_id: &str,
        plugin_name: String,
        instance_id: &str,
        owner: Arc<PluginOwner>,
        table: VesperSourceNormalizerResource,
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
        let poll_session = required_callback(&runtime, "poll_session", table.poll_session)?;
        let wait_session =
            required_callback(&runtime, "wait_session_update", table.wait_session_update)?;
        let cancel_session = required_callback(&runtime, "cancel_session", table.cancel_session)?;
        let close_session = required_callback(&runtime, "close_session", table.close_session)?;
        let capabilities = load_source_value::<SourceNormalizerResourceCapabilities>(
            &runtime,
            "capabilities_json",
            capabilities_json,
        )?;
        Ok(Self {
            inner: Arc::new(NativeAbiSourceResourceFactoryInner {
                runtime,
                name: plugin_name,
                capabilities,
                open_session,
                poll_session,
                wait_session,
                cancel_session,
                close_session,
                active_sessions: ActiveSessionRegistry::default(),
            }),
        })
    }
}

impl SourceNormalizerResourcePluginFactory for NativeAbiSourceNormalizerResourcePluginFactory {
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
        validate_source_normalizer_plugin_input(&config.input, &config.headers)?;
        let config_json = serde_json::to_vec(config).map_err(|error| {
            SourceNormalizerError::payload_codec(format!(
                "serialize resource normalizer session config failed: {error}"
            ))
        })?;
        let reservation = reserve_source_session(&self.inner.active_sessions, "resource")?;
        let result = self
            .inner
            .runtime
            .invoke_open(
                "open_session_json",
                SOURCE_FAILURE_STATUSES,
                |out| {
                    // SAFETY: callback/context are validated and config/output
                    // are borrowed for this synchronous call.
                    unsafe {
                        (self.inner.open_session)(
                            self.inner.runtime.context(),
                            borrowed_bytes(&config_json),
                            out,
                        )
                    }
                },
                |session_id| {
                    cleanup_source_session(
                        &self.inner.runtime,
                        self.inner.close_session,
                        session_id,
                    );
                },
            )
            .map_err(map_source_boundary)?;
        match result {
            OpenCallResult::Success {
                session_id,
                payload,
            } => {
                let session_info = match self
                    .inner
                    .runtime
                    .decode_json::<SourceNormalizerResourceSessionInfo>(
                        "open_session_json",
                        &payload,
                    ) {
                    Ok(session_info) => session_info,
                    Err(error) => {
                        cleanup_source_session(
                            &self.inner.runtime,
                            self.inner.close_session,
                            session_id,
                        );
                        return Err(map_source_boundary(error));
                    }
                };
                register_source_session(reservation, &self.inner.runtime, "resource", session_id)?;
                Ok(Box::new(NativeAbiSourceNormalizerResourceSession {
                    factory: self.inner.clone(),
                    session_id,
                    session_info,
                    closing: false,
                    closed: false,
                }))
            }
            OpenCallResult::Failure { status, payload } => Err(decode_source_failure(
                &self.inner.runtime,
                "open_session_json",
                status,
                &payload,
            )),
        }
    }
}

struct NativeAbiSourceNormalizerResourceSession {
    factory: Arc<NativeAbiSourceResourceFactoryInner>,
    session_id: u64,
    session_info: SourceNormalizerResourceSessionInfo,
    closing: bool,
    closed: bool,
}

impl std::fmt::Debug for NativeAbiSourceNormalizerResourceSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeAbiSourceNormalizerResourceSession")
            .field("session_id", &self.session_id)
            .field("closed", &self.closed)
            .finish()
    }
}

impl NativeAbiSourceNormalizerResourceSession {
    fn ensure_open(&self) -> Result<(), SourceNormalizerError> {
        if self.closed || self.closing {
            Err(SourceNormalizerError::NotConfigured)
        } else {
            Ok(())
        }
    }

    fn invoke<T>(
        &self,
        operation: &'static str,
        callback: impl FnOnce(*mut VesperJsonOut) -> VesperStatus,
    ) -> Result<T, SourceNormalizerError>
    where
        T: serde::de::DeserializeOwned,
    {
        self.ensure_open()?;
        let result = self
            .factory
            .runtime
            .invoke_json(operation, SOURCE_FAILURE_STATUSES, callback)
            .map_err(map_source_boundary)?;
        decode_source_result(&self.factory.runtime, operation, result)
    }
}

impl SourceNormalizerResourceSession for NativeAbiSourceNormalizerResourceSession {
    fn session_info(&self) -> SourceNormalizerResourceSessionInfo {
        self.session_info.clone()
    }

    fn poll(&mut self) -> Result<SourceNormalizerResourceSessionStatus, SourceNormalizerError> {
        self.invoke("poll_session", |out| {
            // SAFETY: callback/context/session are validated and output is
            // borrowed for this synchronous call.
            unsafe {
                (self.factory.poll_session)(self.factory.runtime.context(), self.session_id, out)
            }
        })
    }

    fn wait_for_update(
        &mut self,
        timeout: Duration,
    ) -> Result<SourceNormalizerResourceSessionWaitStatus, SourceNormalizerError> {
        let timeout_ms = duration_to_timeout_millis(timeout);
        self.invoke("wait_session_update", |out| {
            // SAFETY: callback/context/session are validated and output is
            // borrowed for this synchronous call.
            unsafe {
                (self.factory.wait_session)(
                    self.factory.runtime.context(),
                    self.session_id,
                    timeout_ms,
                    out,
                )
            }
        })
    }

    fn cancel(&mut self) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        self.ensure_open()?;
        let result = self
            .factory
            .runtime
            .invoke_cleanup_json("cancel_session", SOURCE_FAILURE_STATUSES, |out| {
                // SAFETY: callback/context/session are validated and output is
                // borrowed for this synchronous cleanup call.
                unsafe {
                    (self.factory.cancel_session)(
                        self.factory.runtime.context(),
                        self.session_id,
                        out,
                    )
                }
            })
            .map_err(map_source_boundary)?;
        let status: SourceNormalizerOperationStatus =
            decode_source_result(&self.factory.runtime, "cancel_session", result)?;
        Ok(status)
    }

    fn close(&mut self) -> Result<(), SourceNormalizerError> {
        if self.closed {
            return Ok(());
        }
        self.closing = true;
        let result = self
            .factory
            .runtime
            .invoke_cleanup_json("close_session", SOURCE_FAILURE_STATUSES, |out| {
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
            .map_err(map_source_boundary)?;
        let status = decode_source_result(&self.factory.runtime, "close_session", result)?;
        require_source_completed(&self.factory.runtime, "close_session", status)?;
        self.factory.active_sessions.remove(self.session_id);
        self.closed = true;
        Ok(())
    }
}

impl Drop for NativeAbiSourceNormalizerResourceSession {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn reserve_source_session<'a>(
    registry: &'a ActiveSessionRegistry,
    kind: &'static str,
) -> Result<super::runtime::ActiveSessionReservation<'a>, SourceNormalizerError> {
    registry.reserve_open().map_err(|error| match error {
        ActiveSessionError::Exhausted => SourceNormalizerError::resource_exhausted(format!(
            "host {kind} normalizer interface reached its active session limit"
        )),
        ActiveSessionError::Duplicate { .. } => SourceNormalizerError::internal(format!(
            "host {kind} normalizer session reservation failed unexpectedly"
        )),
    })
}

fn register_source_session(
    reservation: super::runtime::ActiveSessionReservation<'_>,
    runtime: &InterfaceRuntime,
    kind: &'static str,
    session_id: u64,
) -> Result<(), SourceNormalizerError> {
    reservation
        .register(session_id)
        .map_err(|error| match error {
            ActiveSessionError::Duplicate { session_id } => {
                map_source_boundary(runtime.contract_violation(
                    "open_session_json",
                    format!("plugin reused active {kind} normalizer session id {session_id}"),
                ))
            }
            ActiveSessionError::Exhausted => SourceNormalizerError::resource_exhausted(format!(
                "host {kind} normalizer interface reached its active session limit"
            )),
        })
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

fn load_source_value<T>(
    runtime: &InterfaceRuntime,
    operation: &'static str,
    callback: player_plugin_abi::VesperGetJsonFn,
) -> Result<T, NativeAbiBoundaryError>
where
    T: serde::de::DeserializeOwned,
{
    let result = runtime.invoke_json(operation, SOURCE_FAILURE_STATUSES, |out| {
        // SAFETY: callback/context are validated and output is host-owned for
        // this synchronous call.
        unsafe { callback(runtime.context(), out) }
    })?;
    match result {
        JsonCallResult::Success(payload) => runtime.decode_json(operation, &payload),
        JsonCallResult::Failure { status, payload } => {
            let error = decode_source_failure(runtime, operation, status, &payload);
            Err(runtime.reported_failure(operation, status, error.to_string()))
        }
    }
}

fn decode_source_result<T>(
    runtime: &InterfaceRuntime,
    operation: &'static str,
    result: JsonCallResult,
) -> Result<T, SourceNormalizerError>
where
    T: serde::de::DeserializeOwned,
{
    match result {
        JsonCallResult::Success(payload) => runtime
            .decode_json(operation, &payload)
            .map_err(map_source_boundary),
        JsonCallResult::Failure { status, payload } => {
            Err(decode_source_failure(runtime, operation, status, &payload))
        }
    }
}

fn decode_source_failure(
    runtime: &InterfaceRuntime,
    operation: &'static str,
    raw_status: VesperStatus,
    payload: &[u8],
) -> SourceNormalizerError {
    let error = if raw_status == status::EXHAUSTED && payload.is_empty() {
        SourceNormalizerError::resource_exhausted("source normalizer resource limit exhausted")
    } else if raw_status == status::TIMEOUT && payload.is_empty() {
        SourceNormalizerError::Timeout {
            message: "source normalizer operation timed out".to_owned(),
        }
    } else {
        match runtime.decode_json::<SourceNormalizerError>(operation, payload) {
            Ok(error) => error,
            Err(error) => return map_source_boundary(error),
        }
    };
    if source_status_matches(raw_status, &error) {
        error
    } else {
        map_source_boundary(runtime.contract_violation(
            operation,
            format!("status {raw_status} is inconsistent with source normalizer error `{error}`"),
        ))
    }
}

fn source_status_matches(raw_status: VesperStatus, error: &SourceNormalizerError) -> bool {
    match raw_status {
        status::FAILURE => matches!(
            error,
            SourceNormalizerError::Internal { .. } | SourceNormalizerError::NotConfigured
        ),
        status::INVALID_ARGUMENT => matches!(
            error,
            SourceNormalizerError::InvalidInput { .. }
                | SourceNormalizerError::PayloadCodec { .. }
                | SourceNormalizerError::Configuration { .. }
        ),
        status::UNSUPPORTED => matches!(
            error,
            SourceNormalizerError::UnsupportedRuntimeProfile { .. }
                | SourceNormalizerError::UnsupportedOperation { .. }
        ),
        status::TIMEOUT => matches!(error, SourceNormalizerError::Timeout { .. }),
        status::EXHAUSTED => matches!(error, SourceNormalizerError::ResourceExhausted { .. }),
        _ => false,
    }
}

fn require_source_completed(
    runtime: &InterfaceRuntime,
    operation: &'static str,
    status: SourceNormalizerOperationStatus,
) -> Result<(), SourceNormalizerError> {
    if status.completed {
        Ok(())
    } else {
        Err(map_source_boundary(runtime.contract_violation(
            operation,
            "successful cleanup reported completed=false",
        )))
    }
}

fn cleanup_source_session(
    runtime: &InterfaceRuntime,
    close: player_plugin_abi::VesperSessionOperationFn,
    session_id: u64,
) {
    for _ in 0..OPEN_FAILURE_CLOSE_ATTEMPTS {
        let result = runtime.invoke_cleanup_json(
            "close_session_after_open_failure",
            SOURCE_FAILURE_STATUSES,
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
            runtime.decode_json::<SourceNormalizerOperationStatus>(
                "close_session_after_open_failure",
                &payload,
            ),
            Ok(SourceNormalizerOperationStatus {
                completed: true,
                ..
            })
        ) {
            return;
        }
        let _ = runtime.contract_violation(
            "close_session_after_open_failure",
            "successful orphan close reported completed=false",
        );
    }
}

fn copy_packet_data(
    runtime: &InterfaceRuntime,
    data: VesperByteSlice,
) -> Result<Vec<u8>, NativeAbiBoundaryError> {
    if data.len > VESPER_MAX_PACKET_BYTES {
        return Err(runtime.contract_violation(
            "read_packet",
            format!(
                "packet length {} exceeds the {VESPER_MAX_PACKET_BYTES}-byte protocol limit",
                data.len
            ),
        ));
    }
    if data.len == 0 {
        return if data.data.is_null() {
            Ok(Vec::new())
        } else {
            Err(runtime.contract_violation(
                "read_packet",
                "packet returned a non-null pointer with zero length",
            ))
        };
    }
    if data.data.is_null() {
        return Err(runtime.contract_violation(
            "read_packet",
            format!("packet returned a null pointer with length {}", data.len),
        ));
    }
    let len = usize::try_from(data.len).map_err(|_| {
        runtime.contract_violation(
            "read_packet",
            format!("packet length {} does not fit usize", data.len),
        )
    })?;
    if len > isize::MAX as usize {
        return Err(runtime.contract_violation(
            "read_packet",
            format!("packet length {} exceeds isize::MAX", data.len),
        ));
    }
    // SAFETY: the checked native ABI promises this borrowed range remains
    // readable until the matching raw lease is released. Bounds and pointer
    // representation are validated before the copy.
    let bytes = unsafe { std::slice::from_raw_parts(data.data, len) };
    Ok(bytes.to_vec())
}

fn allocate_host_packet_token() -> Result<usize, SourceNormalizerError> {
    let token = NEXT_HOST_PACKET_TOKEN
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .map_err(|_| {
            SourceNormalizerError::resource_exhausted("host packet token space is exhausted")
        })?;
    usize::try_from(token).map_err(|_| {
        SourceNormalizerError::resource_exhausted(
            "host packet token does not fit this process address width",
        )
    })
}

fn duration_to_timeout_millis(timeout: Duration) -> u64 {
    if timeout.is_zero() {
        return 0;
    }
    let millis = timeout.as_millis();
    if millis == 0 {
        1
    } else {
        u64::try_from(millis).unwrap_or(u64::MAX)
    }
}

fn map_source_boundary(error: NativeAbiBoundaryError) -> SourceNormalizerError {
    SourceNormalizerError::abi_violation(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    use player_plugin::{
        Plugin, PluginBuilder, SourceNormalizerOutputRoute, SourceNormalizerPacket,
        SourceNormalizerPacketMediaKind, SourceNormalizerRequiredCapabilities,
        SourceNormalizerResourceCachePolicy, SourceNormalizerResourceSessionState,
    };

    use super::*;
    use crate::native_abi::{CheckedInterfaceTable, CheckedPluginRoot};

    const SOURCE_INSTANCE: &str = "dev.vesper.fixture.source";

    thread_local! {
        static FIXTURE_COUNTERS: RefCell<Option<Arc<SourceCounters>>> = const {
            RefCell::new(None)
        };
    }

    #[derive(Default)]
    struct SourceCounters {
        packet_opens: AtomicUsize,
        packet_releases: AtomicUsize,
        packet_flushes: AtomicUsize,
        packet_closes: AtomicUsize,
        resource_opens: AtomicUsize,
        resource_cancels: AtomicUsize,
        resource_closes: AtomicUsize,
        wait_timeout_ms: AtomicU64,
    }

    #[derive(Clone)]
    struct FixturePacketFactory {
        counters: Arc<SourceCounters>,
    }

    impl SourceNormalizerPacketPluginFactory for FixturePacketFactory {
        fn name(&self) -> &str {
            "fixture packet source"
        }

        fn packet_capabilities(&self) -> SourceNormalizerPacketCapabilities {
            SourceNormalizerPacketCapabilities {
                supported_runtime_profiles: vec!["fixture".to_owned()],
                media_kinds: vec![SourceNormalizerPacketMediaKind::Video],
                supports_seek: true,
                supports_flush: true,
                required_capabilities: SourceNormalizerRequiredCapabilities::default(),
                ..SourceNormalizerPacketCapabilities::default()
            }
        }

        fn open_packet_session(
            &self,
            _config: &SourceNormalizerPacketSessionConfig,
        ) -> Result<Box<dyn SourceNormalizerPacketSession>, SourceNormalizerError> {
            self.counters.packet_opens.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(FixturePacketSession {
                counters: self.counters.clone(),
                data: vec![1, 2, 3, 4],
            }))
        }
    }

    struct FixturePacketSession {
        counters: Arc<SourceCounters>,
        data: Vec<u8>,
    }

    impl SourceNormalizerPacketSession for FixturePacketSession {
        fn stream_info(&self) -> SourceNormalizerPacketStreamInfo {
            SourceNormalizerPacketStreamInfo {
                normalizer_name: Some("fixture packet source".to_owned()),
                ..SourceNormalizerPacketStreamInfo::default()
            }
        }

        fn read_packet(
            &mut self,
        ) -> Result<SourceNormalizerPacketLease<'_>, SourceNormalizerError> {
            Ok(SourceNormalizerPacketLease {
                metadata: SourceNormalizerReadPacketMetadata::packet(
                    SourceNormalizerPacket::default(),
                ),
                data: &self.data,
                handle: 7,
            })
        }

        fn release_packet(&mut self, _packet_handle: usize) -> Result<(), SourceNormalizerError> {
            self.counters.packet_releases.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn seek(
            &mut self,
            _seek: &SourceNormalizerPacketSeek,
        ) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
            Ok(completed_operation())
        }

        fn flush(&mut self) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
            self.counters.packet_flushes.fetch_add(1, Ordering::SeqCst);
            Ok(completed_operation())
        }

        fn close(&mut self) -> Result<(), SourceNormalizerError> {
            self.counters.packet_closes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[derive(Clone)]
    struct FixtureResourceFactory {
        counters: Arc<SourceCounters>,
    }

    impl SourceNormalizerResourcePluginFactory for FixtureResourceFactory {
        fn name(&self) -> &str {
            "fixture resource source"
        }

        fn resource_capabilities(&self) -> SourceNormalizerResourceCapabilities {
            SourceNormalizerResourceCapabilities {
                supported_runtime_profiles: vec!["fixture".to_owned()],
                supported_output_routes: vec![SourceNormalizerOutputRoute::Fmp4LocalStream],
                supports_cancel: true,
                required_capabilities: SourceNormalizerRequiredCapabilities::default(),
                cache_policy: SourceNormalizerResourceCachePolicy::default(),
                ..SourceNormalizerResourceCapabilities::default()
            }
        }

        fn open_resource_session(
            &self,
            _config: &SourceNormalizerResourceSessionConfig,
        ) -> Result<Box<dyn SourceNormalizerResourceSession>, SourceNormalizerError> {
            self.counters.resource_opens.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(FixtureResourceSession {
                counters: self.counters.clone(),
            }))
        }
    }

    struct FixtureResourceSession {
        counters: Arc<SourceCounters>,
    }

    impl SourceNormalizerResourceSession for FixtureResourceSession {
        fn session_info(&self) -> SourceNormalizerResourceSessionInfo {
            resource_session_info()
        }

        fn poll(&mut self) -> Result<SourceNormalizerResourceSessionStatus, SourceNormalizerError> {
            Ok(SourceNormalizerResourceSessionStatus {
                state: SourceNormalizerResourceSessionState::Ready,
                info: Some(resource_session_info()),
                message: None,
                disk_bytes_used: Some(4),
            })
        }

        fn wait_for_update(
            &mut self,
            timeout: Duration,
        ) -> Result<SourceNormalizerResourceSessionWaitStatus, SourceNormalizerError> {
            self.counters.wait_timeout_ms.store(
                u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
                Ordering::SeqCst,
            );
            Ok(SourceNormalizerResourceSessionWaitStatus { updated: true })
        }

        fn cancel(&mut self) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
            self.counters
                .resource_cancels
                .fetch_add(1, Ordering::SeqCst);
            Ok(SourceNormalizerOperationStatus {
                completed: false,
                message: Some("cancellation requested".to_owned()),
            })
        }

        fn close(&mut self) -> Result<(), SourceNormalizerError> {
            self.counters.resource_closes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn completed_operation() -> SourceNormalizerOperationStatus {
        SourceNormalizerOperationStatus {
            completed: true,
            message: None,
        }
    }

    fn resource_session_info() -> SourceNormalizerResourceSessionInfo {
        SourceNormalizerResourceSessionInfo {
            session_id: Some("fixture-resource".to_owned()),
            normalizer_name: Some("fixture resource source".to_owned()),
            runtime_profile: Some("fixture".to_owned()),
            selected_backend: None,
            output_route: SourceNormalizerOutputRoute::Fmp4LocalStream,
            container: "mp4".to_owned(),
            primary_resource_path: Some("/tmp/fixture/output.mp4".to_owned()),
            primary_content_type: Some("video/mp4".to_owned()),
            resources: Vec::new(),
            tracks: Vec::new(),
            duration_millis: None,
            seekable: false,
            disk_bytes_used: Some(4),
        }
    }

    fn fixture_plugin() -> Plugin {
        let counters = FIXTURE_COUNTERS.with(|counters| {
            counters
                .borrow()
                .clone()
                .expect("fixture counters are installed")
        });
        PluginBuilder::new("dev.vesper.source-fixture", "Source fixture")
            .and_then(|builder| {
                builder.with_source_normalizer_packet(
                    SOURCE_INSTANCE,
                    FixturePacketFactory {
                        counters: counters.clone(),
                    },
                )
            })
            .and_then(|builder| {
                builder.with_source_normalizer_resource(
                    SOURCE_INSTANCE,
                    FixtureResourceFactory { counters },
                )
            })
            .and_then(PluginBuilder::build)
            .expect("source fixture plugin")
    }

    fn generated_source_wrappers(
        counters: Arc<SourceCounters>,
    ) -> (
        NativeAbiSourceNormalizerPacketPluginFactory,
        NativeAbiSourceNormalizerResourcePluginFactory,
    ) {
        FIXTURE_COUNTERS.with(|slot| {
            *slot.borrow_mut() = Some(counters);
        });
        let root_ptr = player_plugin::__private::export_plugin(fixture_plugin);
        FIXTURE_COUNTERS.with(|slot| {
            let _ = slot.borrow_mut().take();
        });
        let root =
            // SAFETY: the generated root transfers ownership into the checked loader.
            unsafe { CheckedPluginRoot::from_raw(root_ptr, None) }.expect("checked source root");
        let mut packet = None;
        let mut resource = None;
        for interface in &root.interfaces {
            match interface.table {
                CheckedInterfaceTable::SourceNormalizerPacket(table) => {
                    packet = Some(
                        NativeAbiSourceNormalizerPacketPluginFactory::new(
                            &root.plugin_id,
                            root.plugin_name.clone(),
                            &interface.descriptor.instance_id,
                            root.owner.clone(),
                            table,
                        )
                        .expect("packet wrapper"),
                    );
                }
                CheckedInterfaceTable::SourceNormalizerResource(table) => {
                    resource = Some(
                        NativeAbiSourceNormalizerResourcePluginFactory::new(
                            &root.plugin_id,
                            root.plugin_name.clone(),
                            &interface.descriptor.instance_id,
                            root.owner.clone(),
                            table,
                        )
                        .expect("resource wrapper"),
                    );
                }
                _ => {}
            }
        }
        (
            packet.expect("packet interface"),
            resource.expect("resource interface"),
        )
    }

    fn packet_config(input: &str) -> SourceNormalizerPacketSessionConfig {
        SourceNormalizerPacketSessionConfig {
            runtime_profile: "fixture".to_owned(),
            input: input.to_owned(),
            headers: Vec::new(),
            startup_timeout_ms: None,
            session_timeout_ms: None,
            preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
        }
    }

    fn resource_config() -> SourceNormalizerResourceSessionConfig {
        SourceNormalizerResourceSessionConfig {
            runtime_profile: "fixture".to_owned(),
            input: "./fixtures/input ? literal#.mp4".to_owned(),
            headers: Vec::new(),
            output_root: "/tmp/fixture".to_owned(),
            cache_policy: SourceNormalizerResourceCachePolicy::default(),
            preferred_route: Some(SourceNormalizerOutputRoute::Fmp4LocalStream),
            startup_timeout_ms: None,
            read_idle_timeout_ms: None,
        }
    }

    #[test]
    fn generated_packet_and_resource_wrappers_round_trip_with_bounded_identity() {
        let counters = Arc::new(SourceCounters::default());
        let (packet_factory, resource_factory) = generated_source_wrappers(counters.clone());

        assert!(matches!(
            packet_factory
                .open_packet_session(&packet_config("https://example.com/video.mp4?token=secret")),
            Err(SourceNormalizerError::InvalidInput { .. })
        ));
        assert_eq!(counters.packet_opens.load(Ordering::SeqCst), 0);

        let mut first = packet_factory
            .open_packet_session(&packet_config("./fixtures/input ? literal#.mp4"))
            .expect("first packet session");
        let mut second = packet_factory
            .open_packet_session(&packet_config("file:///tmp/input.mp4"))
            .expect("second packet session");
        let first_handle = {
            let packet = first.read_packet().expect("first packet");
            assert_eq!(packet.data, [1, 2, 3, 4]);
            packet.handle
        };
        let second_handle = {
            let packet = second.read_packet().expect("second packet");
            assert_eq!(packet.data, [1, 2, 3, 4]);
            packet.handle
        };
        assert_ne!(first_handle, second_handle);
        assert!(matches!(
            second.release_packet(first_handle),
            Err(SourceNormalizerError::AbiViolation { .. })
        ));
        first
            .release_packet(first_handle)
            .expect("release first packet");
        second
            .release_packet(second_handle)
            .expect("release second packet");

        let stale_after_flush = {
            let packet = first.read_packet().expect("packet before flush");
            packet.handle
        };
        first.flush().expect("flush packet session");
        assert!(matches!(
            first.release_packet(stale_after_flush),
            Err(SourceNormalizerError::AbiViolation { .. })
        ));
        first.close().expect("close first packet session");
        second.close().expect("close second packet session");

        let mut resource = resource_factory
            .open_resource_session(&resource_config())
            .expect("resource session");
        assert_eq!(
            resource.poll().expect("poll resource").state,
            SourceNormalizerResourceSessionState::Ready
        );
        assert!(
            resource
                .wait_for_update(Duration::from_micros(500))
                .expect("wait resource")
                .updated
        );
        let cancel = resource.cancel().expect("cancel resource");
        assert!(!cancel.completed);
        assert_eq!(cancel.message.as_deref(), Some("cancellation requested"));
        resource.close().expect("close resource");
        resource.close().expect("idempotent resource close");

        assert_eq!(counters.packet_releases.load(Ordering::SeqCst), 3);
        assert_eq!(counters.packet_flushes.load(Ordering::SeqCst), 1);
        assert_eq!(counters.packet_closes.load(Ordering::SeqCst), 2);
        assert_eq!(counters.resource_opens.load(Ordering::SeqCst), 1);
        assert_eq!(counters.resource_cancels.load(Ordering::SeqCst), 1);
        assert_eq!(counters.resource_closes.load(Ordering::SeqCst), 1);
        assert_eq!(counters.wait_timeout_ms.load(Ordering::SeqCst), 1);
    }
}
