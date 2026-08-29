use std::ffi::c_void;
use std::mem::size_of;
use std::sync::Arc;

use player_plugin::{
    AudioPlaybackPolicy, AudioProcessorCapabilities, AudioProcessorError,
    AudioProcessorOperationStatus, AudioProcessorPluginFactory, AudioProcessorSession,
    AudioProcessorSessionConfig, AudioProcessorSessionInfo, DecoderPcmFrame,
};
use player_plugin_abi::{
    VesperAudioProcessor, VesperByteSlice, VesperJsonOut, VesperPcmFrameOut, VesperSessionId,
    VesperStatus, status,
};

use super::PluginOwner;
use super::runtime::{
    ActiveSessionError, ActiveSessionRegistry, InterfaceRuntime, JsonCallResult,
    NativeAbiBoundaryError, OPEN_FAILURE_CLOSE_ATTEMPTS, OpenCallResult, borrowed_bytes,
};

const AUDIO_PROCESSOR_FAILURE_STATUSES: &[VesperStatus] = &[
    status::FAILURE,
    status::INVALID_ARGUMENT,
    status::UNSUPPORTED,
    status::EXHAUSTED,
    status::TIMEOUT,
];

type AudioConfigureCall = unsafe extern "C" fn(
    context: *mut c_void,
    session_id: VesperSessionId,
    policy_json: VesperByteSlice,
    out: *mut VesperJsonOut,
) -> VesperStatus;
type AudioProcessCall = unsafe extern "C" fn(
    context: *mut c_void,
    session_id: VesperSessionId,
    metadata_json: VesperByteSlice,
    pcm_data: VesperByteSlice,
    out: *mut VesperPcmFrameOut,
) -> VesperStatus;

#[derive(Debug)]
struct NativeAbiAudioProcessorFactoryInner {
    runtime: Arc<InterfaceRuntime>,
    name: String,
    capabilities: AudioProcessorCapabilities,
    open_session: player_plugin_abi::VesperOpenSessionFn,
    configure_session: AudioConfigureCall,
    process_pcm_frame: AudioProcessCall,
    flush_session: player_plugin_abi::VesperSessionOperationFn,
    close_session: player_plugin_abi::VesperSessionOperationFn,
    active_sessions: ActiveSessionRegistry,
}

#[derive(Debug, Clone)]
pub(crate) struct NativeAbiAudioProcessorPluginFactory {
    inner: Arc<NativeAbiAudioProcessorFactoryInner>,
}

impl NativeAbiAudioProcessorPluginFactory {
    pub(super) fn new(
        plugin_id: &str,
        plugin_name: String,
        instance_id: &str,
        owner: Arc<PluginOwner>,
        table: VesperAudioProcessor,
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
        let configure_session = required_callback(
            &runtime,
            "configure_session_json",
            table.configure_session_json,
        )?;
        let process_pcm_frame =
            required_callback(&runtime, "process_pcm_frame", table.process_pcm_frame)?;
        let flush_session = required_callback(&runtime, "flush_session", table.flush_session)?;
        let close_session = required_callback(&runtime, "close_session", table.close_session)?;
        let capabilities = load_audio_processor_value::<AudioProcessorCapabilities>(
            &runtime,
            "capabilities_json",
            capabilities_json,
        )?;
        validate_capabilities(&runtime, &capabilities)?;
        Ok(Self {
            inner: Arc::new(NativeAbiAudioProcessorFactoryInner {
                runtime,
                name: plugin_name,
                capabilities,
                open_session,
                configure_session,
                process_pcm_frame,
                flush_session,
                close_session,
                active_sessions: ActiveSessionRegistry::default(),
            }),
        })
    }
}

impl AudioProcessorPluginFactory for NativeAbiAudioProcessorPluginFactory {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn capabilities(&self) -> AudioProcessorCapabilities {
        self.inner.capabilities.clone()
    }

    fn open_session(
        &self,
        config: &AudioProcessorSessionConfig,
    ) -> Result<Box<dyn AudioProcessorSession>, AudioProcessorError> {
        let config_json = serde_json::to_vec(config).map_err(|error| {
            AudioProcessorError::payload_codec(format!(
                "serialize audio processor config failed: {error}"
            ))
        })?;
        let reservation = self
            .inner
            .active_sessions
            .reserve_open()
            .map_err(map_active_session_error)?;
        let result = self
            .inner
            .runtime
            .invoke_open(
                "open_session_json",
                AUDIO_PROCESSOR_FAILURE_STATUSES,
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
                    cleanup_audio_processor_session(
                        &self.inner.runtime,
                        self.inner.close_session,
                        session_id,
                    );
                },
            )
            .map_err(map_audio_boundary)?;
        match result {
            OpenCallResult::Success {
                session_id,
                payload,
            } => {
                let session_info = match self
                    .inner
                    .runtime
                    .decode_json::<AudioProcessorSessionInfo>("open_session_json", &payload)
                {
                    Ok(session_info) => session_info,
                    Err(error) => {
                        cleanup_audio_processor_session(
                            &self.inner.runtime,
                            self.inner.close_session,
                            session_id,
                        );
                        return Err(map_audio_boundary(error));
                    }
                };
                if let Err(error) = reservation.register(session_id) {
                    return Err(match error {
                        ActiveSessionError::Duplicate { session_id } => {
                            map_audio_boundary(self.inner.runtime.contract_violation(
                                "open_session_json",
                                format!(
                                    "plugin reused active audio processor session id {session_id}"
                                ),
                            ))
                        }
                        ActiveSessionError::Exhausted => AudioProcessorError::Backpressure(
                            "host audio processor interface reached its active session limit"
                                .to_owned(),
                        ),
                    });
                }
                Ok(Box::new(NativeAbiAudioProcessorSession {
                    factory: self.inner.clone(),
                    session_id,
                    session_info,
                    closed: false,
                }))
            }
            OpenCallResult::Failure { status, payload } => Err(decode_audio_failure(
                &self.inner.runtime,
                "open_session_json",
                status,
                &payload,
            )),
        }
    }
}

struct NativeAbiAudioProcessorSession {
    factory: Arc<NativeAbiAudioProcessorFactoryInner>,
    session_id: u64,
    session_info: AudioProcessorSessionInfo,
    closed: bool,
}

impl std::fmt::Debug for NativeAbiAudioProcessorSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeAbiAudioProcessorSession")
            .field("session_id", &self.session_id)
            .field("closed", &self.closed)
            .finish()
    }
}

impl NativeAbiAudioProcessorSession {
    fn ensure_open(&self) -> Result<(), AudioProcessorError> {
        if self.closed {
            Err(AudioProcessorError::Closed)
        } else {
            Ok(())
        }
    }

    fn decode_result<T>(
        &self,
        operation: &'static str,
        result: JsonCallResult,
    ) -> Result<T, AudioProcessorError>
    where
        T: serde::de::DeserializeOwned,
    {
        match result {
            JsonCallResult::Success(payload) => self
                .factory
                .runtime
                .decode_json(operation, &payload)
                .map_err(map_audio_boundary),
            JsonCallResult::Failure { status, payload } => Err(decode_audio_failure(
                &self.factory.runtime,
                operation,
                status,
                &payload,
            )),
        }
    }

    fn require_completed(
        &self,
        operation: &'static str,
        result: JsonCallResult,
    ) -> Result<(), AudioProcessorError> {
        let status = self.decode_result::<AudioProcessorOperationStatus>(operation, result)?;
        if status.completed {
            Ok(())
        } else {
            Err(map_audio_boundary(self.factory.runtime.contract_violation(
                operation,
                "successful operation reported completed=false",
            )))
        }
    }
}

impl AudioProcessorSession for NativeAbiAudioProcessorSession {
    fn name(&self) -> &str {
        self.session_info
            .processor_name
            .as_deref()
            .unwrap_or(&self.factory.name)
    }

    fn capabilities(&self) -> AudioProcessorCapabilities {
        self.factory.capabilities.clone()
    }

    fn session_info(&self) -> AudioProcessorSessionInfo {
        self.session_info.clone()
    }

    fn configure(&mut self, policy: AudioPlaybackPolicy) -> Result<(), AudioProcessorError> {
        self.ensure_open()?;
        policy.validate()?;
        if !self.factory.capabilities.supports_playback_policy(policy) {
            return Err(AudioProcessorError::UnsupportedPlaybackPolicy);
        }
        let policy_json = serde_json::to_vec(&policy).map_err(|error| {
            AudioProcessorError::payload_codec(format!(
                "serialize audio playback policy failed: {error}"
            ))
        })?;
        let result = self
            .factory
            .runtime
            .invoke_json(
                "configure_session_json",
                AUDIO_PROCESSOR_FAILURE_STATUSES,
                |out| {
                    // SAFETY: callback/context/session are validated and the
                    // borrowed policy/output live for this synchronous call.
                    unsafe {
                        (self.factory.configure_session)(
                            self.factory.runtime.context(),
                            self.session_id,
                            borrowed_bytes(&policy_json),
                            out,
                        )
                    }
                },
            )
            .map_err(map_audio_boundary)?;
        self.require_completed("configure_session_json", result)
    }

    fn process(&mut self, frame: DecoderPcmFrame) -> Result<DecoderPcmFrame, AudioProcessorError> {
        self.ensure_open()?;
        frame
            .validate()
            .map_err(|error| AudioProcessorError::InvalidPcm(error.to_string()))?;
        let input_pts_us = frame.metadata.pts_us;
        let input_discontinuity = frame.metadata.discontinuity;
        if !self
            .factory
            .capabilities
            .supports_input_format(&frame.metadata.format)
        {
            return Err(AudioProcessorError::Processor(format!(
                "unsupported PCM input format {:?}",
                frame.metadata.format
            )));
        }
        let metadata_json = serde_json::to_vec(&frame.metadata).map_err(|error| {
            AudioProcessorError::payload_codec(format!(
                "serialize PCM frame metadata failed: {error}"
            ))
        })?;
        let mut out = VesperPcmFrameOut::default();
        let call = self
            .factory
            .runtime
            .invoke_callback("process_pcm_frame", false, || {
                // SAFETY: callback/context/session are validated and all
                // borrowed inputs/output live for this synchronous call.
                unsafe {
                    (self.factory.process_pcm_frame)(
                        self.factory.runtime.context(),
                        self.session_id,
                        borrowed_bytes(&metadata_json),
                        borrowed_bytes(&frame.data),
                        &mut out,
                    )
                }
            });
        let metadata_payload = self
            .factory
            .runtime
            .capture_owned_bytes("process_pcm_frame", out.metadata);
        let pcm_payload = self
            .factory
            .runtime
            .capture_owned_bytes("process_pcm_frame", out.data);
        let output_check = self.factory.runtime.validate_out_prefix(
            "process_pcm_frame",
            out.struct_size,
            out.reserved,
            size_of::<VesperPcmFrameOut>() as u32,
        );
        let raw_status = call.map_err(map_audio_boundary)?;
        let metadata_payload = metadata_payload.map_err(map_audio_boundary)?;
        let pcm_payload = pcm_payload.map_err(map_audio_boundary)?;
        output_check.map_err(map_audio_boundary)?;
        let result = self
            .factory
            .runtime
            .classify_json_status(
                "process_pcm_frame",
                raw_status,
                AUDIO_PROCESSOR_FAILURE_STATUSES,
                metadata_payload,
            )
            .map_err(map_audio_boundary)?;
        match result {
            JsonCallResult::Success(metadata_payload) => {
                let metadata = self
                    .factory
                    .runtime
                    .decode_json("process_pcm_frame", &metadata_payload)
                    .map_err(map_audio_boundary)?;
                let output = DecoderPcmFrame {
                    metadata,
                    data: pcm_payload,
                };
                output.validate().map_err(|error| {
                    map_audio_boundary(self.factory.runtime.contract_violation(
                        "process_pcm_frame",
                        format!("returned invalid PCM output: {error}"),
                    ))
                })?;
                if output.metadata.pts_us != input_pts_us {
                    return Err(map_audio_boundary(self.factory.runtime.contract_violation(
                        "process_pcm_frame",
                        "changed the host-owned PCM presentation timestamp",
                    )));
                }
                if output.metadata.discontinuity != input_discontinuity {
                    return Err(map_audio_boundary(self.factory.runtime.contract_violation(
                        "process_pcm_frame",
                        "changed the host-owned PCM discontinuity marker",
                    )));
                }
                Ok(output)
            }
            JsonCallResult::Failure { status, payload } => {
                if !pcm_payload.is_empty() {
                    return Err(map_audio_boundary(self.factory.runtime.contract_violation(
                        "process_pcm_frame",
                        "failed processing call returned PCM data",
                    )));
                }
                Err(decode_audio_failure(
                    &self.factory.runtime,
                    "process_pcm_frame",
                    status,
                    &payload,
                ))
            }
        }
    }

    fn flush(&mut self) -> Result<(), AudioProcessorError> {
        self.ensure_open()?;
        let result = self
            .factory
            .runtime
            .invoke_cleanup_json("flush_session", AUDIO_PROCESSOR_FAILURE_STATUSES, |out| {
                // SAFETY: callback/context/session are validated and the
                // output is borrowed for this synchronous cleanup call.
                unsafe {
                    (self.factory.flush_session)(
                        self.factory.runtime.context(),
                        self.session_id,
                        out,
                    )
                }
            })
            .map_err(map_audio_boundary)?;
        self.require_completed("flush_session", result)
    }

    fn close(&mut self) -> Result<(), AudioProcessorError> {
        if self.closed {
            return Ok(());
        }
        let result = self
            .factory
            .runtime
            .invoke_cleanup_json("close_session", AUDIO_PROCESSOR_FAILURE_STATUSES, |out| {
                // SAFETY: callback/context/session are validated and the
                // output is borrowed for this synchronous cleanup call.
                unsafe {
                    (self.factory.close_session)(
                        self.factory.runtime.context(),
                        self.session_id,
                        out,
                    )
                }
            })
            .map_err(map_audio_boundary)?;
        self.require_completed("close_session", result)?;
        self.factory.active_sessions.remove(self.session_id);
        self.closed = true;
        Ok(())
    }
}

impl Drop for NativeAbiAudioProcessorSession {
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

fn load_audio_processor_value<T>(
    runtime: &InterfaceRuntime,
    operation: &'static str,
    callback: player_plugin_abi::VesperGetJsonFn,
) -> Result<T, NativeAbiBoundaryError>
where
    T: serde::de::DeserializeOwned,
{
    let result = runtime.invoke_json(operation, AUDIO_PROCESSOR_FAILURE_STATUSES, |out| {
        // SAFETY: callback/context are validated and output is host-owned for
        // this synchronous call.
        unsafe { callback(runtime.context(), out) }
    })?;
    match result {
        JsonCallResult::Success(payload) => runtime.decode_json(operation, &payload),
        JsonCallResult::Failure { status, payload } => {
            let error = decode_audio_failure(runtime, operation, status, &payload);
            Err(runtime.reported_failure(operation, status, error.to_string()))
        }
    }
}

fn validate_capabilities(
    runtime: &InterfaceRuntime,
    capabilities: &AudioProcessorCapabilities,
) -> Result<(), NativeAbiBoundaryError> {
    if capabilities
        .playback_rate_min
        .is_some_and(|rate| !rate.is_finite() || rate <= 0.0)
        || capabilities
            .playback_rate_max
            .is_some_and(|rate| !rate.is_finite() || rate <= 0.0)
        || matches!(
            (
                capabilities.playback_rate_min,
                capabilities.playback_rate_max
            ),
            (Some(minimum), Some(maximum)) if minimum > maximum
        )
    {
        return Err(runtime.contract_violation(
            "capabilities_json",
            "audio processor returned invalid playback-rate bounds",
        ));
    }
    if capabilities.max_in_flight_frames == Some(0) {
        return Err(runtime.contract_violation(
            "capabilities_json",
            "audio processor returned max_in_flight_frames=0",
        ));
    }
    Ok(())
}

fn decode_audio_failure(
    runtime: &InterfaceRuntime,
    operation: &'static str,
    raw_status: VesperStatus,
    payload: &[u8],
) -> AudioProcessorError {
    let error = if raw_status == status::EXHAUSTED && payload.is_empty() {
        AudioProcessorError::Backpressure("audio processor resource limit exhausted".to_owned())
    } else if raw_status == status::TIMEOUT && payload.is_empty() {
        AudioProcessorError::Timeout("audio processor operation timed out".to_owned())
    } else {
        match runtime.decode_json::<AudioProcessorError>(operation, payload) {
            Ok(error) => error,
            Err(error) => return map_audio_boundary(error),
        }
    };
    if audio_status_matches(raw_status, &error) {
        error
    } else {
        map_audio_boundary(runtime.contract_violation(
            operation,
            format!("status {raw_status} is inconsistent with audio processor error `{error}`"),
        ))
    }
}

fn audio_status_matches(raw_status: VesperStatus, error: &AudioProcessorError) -> bool {
    match raw_status {
        status::FAILURE => matches!(
            error,
            AudioProcessorError::Closed | AudioProcessorError::Processor(_)
        ),
        status::INVALID_ARGUMENT => matches!(
            error,
            AudioProcessorError::InvalidCapacity
                | AudioProcessorError::InvalidPcm(_)
                | AudioProcessorError::InvalidPlaybackRate { .. }
                | AudioProcessorError::PayloadCodec(_)
        ),
        status::UNSUPPORTED => matches!(error, AudioProcessorError::UnsupportedPlaybackPolicy),
        status::EXHAUSTED => matches!(error, AudioProcessorError::Backpressure(_)),
        status::TIMEOUT => matches!(error, AudioProcessorError::Timeout(_)),
        _ => false,
    }
}

fn cleanup_audio_processor_session(
    runtime: &InterfaceRuntime,
    close: player_plugin_abi::VesperSessionOperationFn,
    session_id: u64,
) {
    for _ in 0..OPEN_FAILURE_CLOSE_ATTEMPTS {
        let result = runtime.invoke_cleanup_json(
            "close_session_after_open_failure",
            AUDIO_PROCESSOR_FAILURE_STATUSES,
            |out| {
                // SAFETY: callback/context/session came from the successful
                // open and output is host-owned for this cleanup call.
                unsafe { close(runtime.context(), session_id, out) }
            },
        );
        let Ok(JsonCallResult::Success(payload)) = result else {
            continue;
        };
        if matches!(
            runtime.decode_json::<AudioProcessorOperationStatus>(
                "close_session_after_open_failure",
                &payload,
            ),
            Ok(AudioProcessorOperationStatus { completed: true })
        ) {
            return;
        }
        let _ = runtime.contract_violation(
            "close_session_after_open_failure",
            "successful orphan close reported completed=false",
        );
    }
}

fn map_active_session_error(error: ActiveSessionError) -> AudioProcessorError {
    match error {
        ActiveSessionError::Exhausted => AudioProcessorError::Backpressure(
            "host audio processor interface reached its active session limit".to_owned(),
        ),
        ActiveSessionError::Duplicate { .. } => AudioProcessorError::Processor(
            "host audio processor session reservation failed unexpectedly".to_owned(),
        ),
    }
}

fn map_audio_boundary(error: NativeAbiBoundaryError) -> AudioProcessorError {
    AudioProcessorError::abi_violation(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use player_plugin::{
        AudioPitchMode, DecoderFrameFormat, DecoderPcmFrameMetadata, DecoderPcmSampleLayout,
    };
    use player_plugin_abi::{
        AUDIO_PROCESSOR_INTERFACE_ID, VESPER_INTERFACE_MAJOR, VESPER_INTERFACE_MINOR,
        VesperInterfaceHeader, VesperOpenSessionOut, VesperOwnedBytes,
    };
    use serde::Serialize;

    use super::*;

    const AUDIO_INSTANCE: &str = "dev.vesper.fixture.audio-processor";

    #[derive(Clone, Copy)]
    enum RawMode {
        Valid,
        MalformedOpen,
        MalformedPcm,
        MutatedPts,
        MutatedDiscontinuity,
        UnknownStatus,
    }

    struct RawAudioContext {
        mode: RawMode,
        closes: Arc<AtomicUsize>,
    }

    unsafe fn raw_context<'a>(context: *mut c_void) -> Option<&'a RawAudioContext> {
        // SAFETY: every fixture callback receives the live context installed
        // in the interface header and owned by `PluginOwner`.
        unsafe { context.cast::<RawAudioContext>().as_ref() }
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

    unsafe extern "C" fn raw_free(_owner: *mut c_void, bytes: VesperOwnedBytes) {
        // SAFETY: fixture outputs allocate every owned byte sequence with
        // `VesperOwnedBytes::from_vec` and transfer it back exactly once.
        drop(unsafe { bytes.into_vec() });
    }

    unsafe extern "C" fn raw_destroy(owner: *mut c_void) {
        if !owner.is_null() {
            // SAFETY: `raw_factory` transfers one boxed context to the owner.
            drop(unsafe { Box::from_raw(owner.cast::<RawAudioContext>()) });
        }
    }

    unsafe extern "C" fn raw_capabilities(
        _context: *mut c_void,
        out: *mut VesperJsonOut,
    ) -> VesperStatus {
        // SAFETY: forwarded host output retains the callback contract.
        unsafe { write_json(out, &capabilities()) }
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
        out.session_id = 41;
        out.payload = VesperOwnedBytes::from_vec(match context.mode {
            RawMode::MalformedOpen => b"{".to_vec(),
            _ => match serde_json::to_vec(&AudioProcessorSessionInfo::default()) {
                Ok(payload) => payload,
                Err(_) => return status::FAILURE,
            },
        });
        status::OK
    }

    unsafe extern "C" fn raw_configure(
        _context: *mut c_void,
        _session_id: u64,
        _policy: VesperByteSlice,
        out: *mut VesperJsonOut,
    ) -> VesperStatus {
        // SAFETY: forwarded host output retains the callback contract.
        unsafe { write_json(out, &AudioProcessorOperationStatus { completed: true }) }
    }

    unsafe extern "C" fn raw_process(
        context: *mut c_void,
        _session_id: u64,
        metadata: VesperByteSlice,
        pcm_data: VesperByteSlice,
        out: *mut VesperPcmFrameOut,
    ) -> VesperStatus {
        let Some(context) =
            // SAFETY: callback context follows the fixture table contract.
            (unsafe { raw_context(context) })
        else {
            return status::INVALID_ARGUMENT;
        };
        if matches!(context.mode, RawMode::UnknownStatus) {
            return 0xffff_ff00;
        }
        let Some(out) =
            // SAFETY: the host passes a writable, initialized output.
            (unsafe { out.as_mut() })
        else {
            return status::INVALID_ARGUMENT;
        };
        let metadata_len = match usize::try_from(metadata.len) {
            Ok(len) => len,
            Err(_) => return status::INVALID_ARGUMENT,
        };
        let pcm_len = match usize::try_from(pcm_data.len) {
            Ok(len) => len,
            Err(_) => return status::INVALID_ARGUMENT,
        };
        if (metadata_len != 0 && metadata.data.is_null())
            || (pcm_len != 0 && pcm_data.data.is_null())
        {
            return status::INVALID_ARGUMENT;
        }
        // SAFETY: callback inputs are borrowed readable ranges for this call.
        let metadata = unsafe { std::slice::from_raw_parts(metadata.data, metadata_len) };
        // SAFETY: same borrowed input contract as metadata.
        let pcm_data = unsafe { std::slice::from_raw_parts(pcm_data.data, pcm_len) };
        let metadata = match context.mode {
            RawMode::MutatedPts | RawMode::MutatedDiscontinuity => {
                let mut metadata = match serde_json::from_slice::<DecoderPcmFrameMetadata>(metadata)
                {
                    Ok(metadata) => metadata,
                    Err(_) => return status::INVALID_ARGUMENT,
                };
                match context.mode {
                    RawMode::MutatedPts => metadata.pts_us = Some(-1),
                    RawMode::MutatedDiscontinuity => {
                        metadata.discontinuity = !metadata.discontinuity;
                    }
                    _ => {}
                }
                match serde_json::to_vec(&metadata) {
                    Ok(metadata) => metadata,
                    Err(_) => return status::FAILURE,
                }
            }
            _ => metadata.to_vec(),
        };
        out.metadata = VesperOwnedBytes::from_vec(metadata);
        out.data = VesperOwnedBytes::from_vec(if matches!(context.mode, RawMode::MalformedPcm) {
            vec![0]
        } else {
            pcm_data.to_vec()
        });
        status::OK
    }

    unsafe extern "C" fn raw_flush(
        _context: *mut c_void,
        _session_id: u64,
        out: *mut VesperJsonOut,
    ) -> VesperStatus {
        // SAFETY: forwarded host output retains the callback contract.
        unsafe { write_json(out, &AudioProcessorOperationStatus { completed: true }) }
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
        context.closes.fetch_add(1, Ordering::SeqCst);
        // SAFETY: forwarded host output retains the callback contract.
        unsafe { write_json(out, &AudioProcessorOperationStatus { completed: true }) }
    }

    fn capabilities() -> AudioProcessorCapabilities {
        AudioProcessorCapabilities {
            accepted_formats: vec![DecoderFrameFormat::F32],
            output_format: Some(DecoderFrameFormat::F32),
            supports_flush: true,
            max_in_flight_frames: Some(1),
            playback_rate_min: Some(0.5),
            playback_rate_max: Some(2.0),
            pitch_modes: vec![AudioPitchMode::PreservePitch, AudioPitchMode::FollowRate],
        }
    }

    fn raw_factory(mode: RawMode) -> (NativeAbiAudioProcessorPluginFactory, Arc<AtomicUsize>) {
        let closes = Arc::new(AtomicUsize::new(0));
        let context = Box::new(RawAudioContext {
            mode,
            closes: closes.clone(),
        });
        let context = NonNull::new(Box::into_raw(context).cast::<c_void>())
            .expect("raw audio processor context");
        let owner = Arc::new(PluginOwner {
            owner: context,
            free_bytes: raw_free,
            destroy_owner: raw_destroy,
            library: None,
        });
        let table = VesperAudioProcessor {
            header: VesperInterfaceHeader::new(
                size_of::<VesperAudioProcessor>() as u32,
                AUDIO_PROCESSOR_INTERFACE_ID,
                VESPER_INTERFACE_MAJOR,
                VESPER_INTERFACE_MINOR,
                context.as_ptr(),
            ),
            capabilities_json: Some(raw_capabilities),
            open_session_json: Some(raw_open),
            configure_session_json: Some(raw_configure),
            process_pcm_frame: Some(raw_process),
            flush_session: Some(raw_flush),
            close_session: Some(raw_close),
        };
        let factory = NativeAbiAudioProcessorPluginFactory::new(
            "dev.vesper.raw-audio-processor",
            "Raw audio processor".to_owned(),
            AUDIO_INSTANCE,
            owner,
            table,
        )
        .expect("raw audio processor wrapper");
        (factory, closes)
    }

    fn config() -> AudioProcessorSessionConfig {
        AudioProcessorSessionConfig {
            processor_index: 0,
            input_metadata: metadata(),
            playback_policy: AudioPlaybackPolicy::normal(),
            max_in_flight_frames: Some(1),
        }
    }

    fn metadata() -> DecoderPcmFrameMetadata {
        DecoderPcmFrameMetadata::audio(
            "fixture-pcm",
            DecoderFrameFormat::F32,
            48_000,
            1,
            DecoderPcmSampleLayout::Interleaved,
            4,
        )
    }

    fn frame() -> DecoderPcmFrame {
        DecoderPcmFrame {
            metadata: metadata(),
            data: vec![0; 4 * size_of::<f32>()],
        }
    }

    #[test]
    fn malformed_open_payload_closes_created_session() {
        let (factory, closes) = raw_factory(RawMode::MalformedOpen);
        assert!(matches!(
            factory.open_session(&config()),
            Err(AudioProcessorError::AbiViolation(_))
        ));
        assert_eq!(closes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn malformed_pcm_output_poisons_processing_but_close_still_runs() {
        let (factory, closes) = raw_factory(RawMode::MalformedPcm);
        let mut session = factory.open_session(&config()).expect("open session");
        assert!(matches!(
            session.process(frame()),
            Err(AudioProcessorError::AbiViolation(_))
        ));
        assert!(matches!(
            session.process(frame()),
            Err(AudioProcessorError::AbiViolation(_))
        ));
        session.close().expect("cleanup poisoned session");
        session.close().expect("idempotent close");
        assert_eq!(closes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn unknown_status_poisons_processing_but_close_still_runs() {
        let (factory, closes) = raw_factory(RawMode::UnknownStatus);
        let mut session = factory.open_session(&config()).expect("open session");
        assert!(matches!(
            session.process(frame()),
            Err(AudioProcessorError::AbiViolation(_))
        ));
        assert!(matches!(
            session.configure(AudioPlaybackPolicy::normal()),
            Err(AudioProcessorError::AbiViolation(_))
        ));
        session.close().expect("cleanup poisoned session");
        assert_eq!(closes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn host_owned_timing_mutation_poisons_processing_but_close_still_runs() {
        for (mode, expected_message) in [
            (RawMode::MutatedPts, "presentation timestamp"),
            (RawMode::MutatedDiscontinuity, "discontinuity marker"),
        ] {
            let (factory, closes) = raw_factory(mode);
            let mut session = factory.open_session(&config()).expect("open session");
            assert!(matches!(
                session.process(frame()),
                Err(AudioProcessorError::AbiViolation(message))
                    if message.contains(expected_message)
            ));
            assert!(matches!(
                session.process(frame()),
                Err(AudioProcessorError::AbiViolation(_))
            ));
            session.close().expect("cleanup poisoned session");
            assert_eq!(closes.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn valid_checked_wrapper_round_trips_pcm_and_cleanup() {
        let (factory, closes) = raw_factory(RawMode::Valid);
        let mut session = factory.open_session(&config()).expect("open session");
        assert_eq!(session.process(frame()).expect("process frame"), frame());
        session.flush().expect("flush session");
        session.close().expect("close session");
        assert_eq!(closes.load(Ordering::SeqCst), 1);
    }
}
