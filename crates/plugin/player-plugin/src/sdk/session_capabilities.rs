#![deny(unsafe_code)]

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use player_plugin_abi::export::{
    ExportCallEffects, ExportFailure, ExportInterface as RawExportInterface, ExportInterfaceKind,
    ExportInvocation, ExportOperation,
};
use player_plugin_abi::{
    VESPER_MAX_LEASES_PER_SESSION, VESPER_MAX_PACKET_BYTES, VESPER_MAX_PCM_BYTES,
    VESPER_RELEASE_DISCARDED, VESPER_RELEASE_PRESENTED, status,
};

use super::session::{SessionGuard, SessionRegistry, SessionRegistryError};
use super::{decode, encode, failure, json_invocation, unexpected_operation};
use crate::{
    AudioPlaybackPolicy, AudioProcessorError, AudioProcessorOperationStatus,
    AudioProcessorPluginFactory, AudioProcessorSession, AudioProcessorSessionConfig, DecoderError,
    DecoderNativeFrame, DecoderOperationStatus, DecoderPcmFrame, DecoderReceiveNativeFrameMetadata,
    DecoderReceiveNativeFrameOutput, DecoderReceivePcmFrameMetadata, DecoderReceivePcmFrameOutput,
    DecoderSessionConfig, FrameProcessorError, FrameProcessorInputFrame,
    FrameProcessorOperationStatus, FrameProcessorPluginFactory, FrameProcessorReceiveFrameMetadata,
    FrameProcessorReceiveOutput, FrameProcessorSession, FrameProcessorSessionConfig,
    FrameProcessorSubmitFrame, NativeDecoderPluginFactory, NativeDecoderSession, NativeFrame,
    SourceNormalizerError, SourceNormalizerOperationStatus, SourceNormalizerPacketPluginFactory,
    SourceNormalizerPacketSeek, SourceNormalizerPacketSession, SourceNormalizerPacketSessionConfig,
    SourceNormalizerReadPacketStatus, SourceNormalizerResourcePluginFactory,
    SourceNormalizerResourceSession, SourceNormalizerResourceSessionConfig,
    validate_source_normalizer_plugin_input,
};

struct LeaseSequence {
    next: AtomicU64,
}

impl Default for LeaseSequence {
    fn default() -> Self {
        Self {
            next: AtomicU64::new(1),
        }
    }
}

impl LeaseSequence {
    fn allocate(&self) -> Result<u64, ExportFailure> {
        self.next
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| ExportFailure::with_status(status::EXHAUSTED, Vec::new()))
    }
}

struct DecoderSessionState {
    session: Box<dyn NativeDecoderSession>,
    frames: HashMap<u64, DecoderNativeFrame>,
}

pub(super) struct NativeDecoderAdapter<F> {
    instance_id: String,
    factory: F,
    presentation_release: bool,
    sessions: Mutex<SessionRegistry<DecoderSessionState>>,
    lease_sequence: LeaseSequence,
}

impl<F> NativeDecoderAdapter<F>
where
    F: NativeDecoderPluginFactory,
{
    pub(super) fn new(instance_id: String, factory: F) -> Self {
        let presentation_release = factory.supports_native_frame_presentation_release();
        Self {
            instance_id,
            factory,
            presentation_release,
            sessions: Mutex::new(SessionRegistry::default()),
            lease_sequence: LeaseSequence::default(),
        }
    }

    fn acquire(&self, session_id: u64) -> Result<SessionGuard<DecoderSessionState>, ExportFailure> {
        lock_registry(&self.sessions)
            .acquire(session_id)
            .map_err(registry_failure)
    }

    fn close(&self, session_id: u64) -> Result<ExportInvocation, ExportFailure> {
        let closing = lock_registry(&self.sessions)
            .begin_close(session_id)
            .map_err(registry_failure)?;
        let Some(mut closing) = closing else {
            return json_invocation(&DecoderOperationStatus { completed: true });
        };
        let (release_error, close_result) = {
            let state = closing.value_mut().ok_or_else(contract_failure)?;
            let release_error = release_decoder_frames(state, self.presentation_release);
            let close_result = state.session.close();
            (release_error, close_result)
        };
        match close_result {
            Ok(()) => {
                closing.commit();
                if let Some(error) = release_error {
                    return Err(decoder_failure(error));
                }
                json_invocation(&DecoderOperationStatus { completed: true })
            }
            Err(error) => {
                if let Some(release_error) = release_error {
                    return Err(decoder_failure(release_error));
                }
                Err(decoder_failure(error))
            }
        }
    }
}

impl<F> RawExportInterface for NativeDecoderAdapter<F>
where
    F: NativeDecoderPluginFactory,
{
    fn kind(&self) -> ExportInterfaceKind {
        ExportInterfaceKind::NativeDecoder
    }

    fn instance_id(&self) -> &str {
        &self.instance_id
    }

    fn invoke(&self, operation: ExportOperation<'_>) -> Result<ExportInvocation, ExportFailure> {
        match operation {
            ExportOperation::Capabilities => json_invocation(&self.factory.capabilities()),
            ExportOperation::NativeRequirements => {
                json_invocation(&self.factory.native_requirements())
            }
            ExportOperation::OpenSession { config_json } => {
                let config = decode::<DecoderSessionConfig>(config_json)
                    .map_err(|message| decoder_failure(DecoderError::payload_codec(message)))?;
                let mut session = self
                    .factory
                    .open_native_session(&config)
                    .map_err(decoder_failure)?;
                let session_info = match catch_unwind(AssertUnwindSafe(|| session.session_info())) {
                    Ok(session_info) => session_info,
                    Err(panic) => {
                        attempt_open_failure_cleanup(|| {
                            let _ = session.close();
                        });
                        resume_unwind(panic);
                    }
                };
                let payload = match encode(&session_info) {
                    Ok(payload) => payload,
                    Err(error) => {
                        attempt_open_failure_cleanup(|| {
                            let _ = session.close();
                        });
                        return Err(error);
                    }
                };
                let state = DecoderSessionState {
                    session,
                    frames: HashMap::new(),
                };
                let session_id = match lock_registry(&self.sessions).insert(state) {
                    Ok(session_id) => session_id,
                    Err((error, mut state)) => {
                        let _ = release_decoder_frames(&mut state, self.presentation_release);
                        let _ = state.session.close();
                        return Err(registry_failure(error));
                    }
                };
                Ok(ExportInvocation::OpenSession {
                    session_id,
                    payload,
                })
            }
            ExportOperation::DecoderSendPacket {
                session_id,
                packet_json,
                packet_data,
            } => {
                if packet_data.len() as u64 > VESPER_MAX_PACKET_BYTES {
                    return Err(decoder_failure(DecoderError::InvalidPacket {
                        message: format!(
                            "packet payload exceeds the {VESPER_MAX_PACKET_BYTES}-byte protocol limit"
                        ),
                    }));
                }
                let packet = decode(packet_json)
                    .map_err(|message| decoder_failure(DecoderError::payload_codec(message)))?;
                let mut session = self.acquire(session_id)?;
                let state = session.value_mut().ok_or_else(contract_failure)?;
                let result = state
                    .session
                    .send_packet(&packet, packet_data)
                    .map_err(decoder_failure)?;
                json_invocation(&result)
            }
            ExportOperation::DecoderReceiveNativeFrame { session_id } => {
                let mut session = self.acquire(session_id)?;
                let state = session.value_mut().ok_or_else(contract_failure)?;
                if state.frames.len() >= VESPER_MAX_LEASES_PER_SESSION {
                    return Err(ExportFailure::with_status(status::EXHAUSTED, Vec::new()));
                }
                match state
                    .session
                    .receive_native_frame()
                    .map_err(decoder_failure)?
                {
                    DecoderReceiveNativeFrameOutput::Frame(frame) => {
                        let metadata = match encode(&DecoderReceiveNativeFrameMetadata::frame(
                            frame.metadata.clone(),
                        )) {
                            Ok(metadata) => metadata,
                            Err(failure) => {
                                return Err(release_decoder_frame_after_failure(
                                    state,
                                    frame,
                                    self.presentation_release,
                                    failure,
                                ));
                            }
                        };
                        let native_handle = match u64::try_from(frame.handle) {
                            Ok(native_handle) => native_handle,
                            Err(_) => {
                                let failure = decoder_failure(DecoderError::abi_violation(
                                    "native frame handle does not fit the native ABI wire type",
                                ));
                                return Err(release_decoder_frame_after_failure(
                                    state,
                                    frame,
                                    self.presentation_release,
                                    failure,
                                ));
                            }
                        };
                        let lease_id = match self.lease_sequence.allocate() {
                            Ok(lease_id) => lease_id,
                            Err(failure) => {
                                return Err(release_decoder_frame_after_failure(
                                    state,
                                    frame,
                                    self.presentation_release,
                                    failure,
                                ));
                            }
                        };
                        state.frames.insert(lease_id, frame);
                        Ok(ExportInvocation::NativeFrame {
                            metadata,
                            native_handle,
                            lease_id,
                            requires_release: true,
                        })
                    }
                    DecoderReceiveNativeFrameOutput::NeedMoreInput => {
                        Ok(ExportInvocation::NativeFrame {
                            metadata: encode(&DecoderReceiveNativeFrameMetadata::need_more_input())?,
                            native_handle: 0,
                            lease_id: 0,
                            requires_release: false,
                        })
                    }
                    DecoderReceiveNativeFrameOutput::Eof => Ok(ExportInvocation::NativeFrame {
                        metadata: encode(&DecoderReceiveNativeFrameMetadata::eof())?,
                        native_handle: 0,
                        lease_id: 0,
                        requires_release: false,
                    }),
                }
            }
            ExportOperation::DecoderReceivePcmFrame { session_id } => {
                let mut session = self.acquire(session_id)?;
                let state = session.value_mut().ok_or_else(contract_failure)?;
                match state.session.receive_pcm_frame().map_err(decoder_failure)? {
                    DecoderReceivePcmFrameOutput::Frame(frame) => {
                        frame.validate().map_err(decoder_failure)?;
                        Ok(ExportInvocation::PcmFrame {
                            metadata: encode(&DecoderReceivePcmFrameMetadata::frame(
                                frame.metadata,
                            ))?,
                            data: frame.data,
                        })
                    }
                    DecoderReceivePcmFrameOutput::NeedMoreInput => Ok(ExportInvocation::PcmFrame {
                        metadata: encode(&DecoderReceivePcmFrameMetadata::need_more_input())?,
                        data: Vec::new(),
                    }),
                    DecoderReceivePcmFrameOutput::Eof => Ok(ExportInvocation::PcmFrame {
                        metadata: encode(&DecoderReceivePcmFrameMetadata::eof())?,
                        data: Vec::new(),
                    }),
                }
            }
            ExportOperation::DecoderReleaseNativeFrame {
                session_id,
                lease_id,
                disposition,
            } => {
                if disposition != VESPER_RELEASE_DISCARDED
                    && disposition != VESPER_RELEASE_PRESENTED
                {
                    return Err(ExportFailure::with_status(
                        status::INVALID_ARGUMENT,
                        Vec::new(),
                    ));
                }
                let mut session = self.acquire(session_id)?;
                let state = session.value_mut().ok_or_else(contract_failure)?;
                let frame = state.frames.remove(&lease_id).ok_or_else(stale_failure)?;
                let result = if self.presentation_release {
                    state.session.release_native_frame_with_presentation(
                        frame,
                        disposition == VESPER_RELEASE_PRESENTED,
                    )
                } else {
                    state.session.release_native_frame(frame)
                };
                result.map_err(decoder_failure)?;
                json_invocation(&DecoderOperationStatus { completed: true })
            }
            ExportOperation::SessionFlush { session_id, .. } => {
                let mut session = self.acquire(session_id)?;
                let state = session.value_mut().ok_or_else(contract_failure)?;
                let release_error = release_decoder_frames(state, self.presentation_release);
                let flush_result = state.session.flush();
                if let Some(error) = release_error {
                    return Err(decoder_failure(error));
                }
                flush_result.map_err(decoder_failure)?;
                json_invocation(&DecoderOperationStatus { completed: true })
            }
            ExportOperation::SessionClose { session_id, .. } => self.close(session_id),
            _ => Err(unexpected_operation("native decoder")),
        }
    }
}

struct FrameProcessorSessionState {
    session: Box<dyn FrameProcessorSession>,
    frames: HashMap<u64, NativeFrame>,
}

pub(super) struct FrameProcessorAdapter<F> {
    instance_id: String,
    factory: F,
    sessions: Mutex<SessionRegistry<FrameProcessorSessionState>>,
    lease_sequence: LeaseSequence,
}

impl<F> FrameProcessorAdapter<F>
where
    F: FrameProcessorPluginFactory,
{
    pub(super) fn new(instance_id: String, factory: F) -> Self {
        Self {
            instance_id,
            factory,
            sessions: Mutex::new(SessionRegistry::default()),
            lease_sequence: LeaseSequence::default(),
        }
    }

    fn acquire(
        &self,
        session_id: u64,
    ) -> Result<SessionGuard<FrameProcessorSessionState>, ExportFailure> {
        lock_registry(&self.sessions)
            .acquire(session_id)
            .map_err(registry_failure)
    }

    fn close(&self, session_id: u64) -> Result<ExportInvocation, ExportFailure> {
        let closing = lock_registry(&self.sessions)
            .begin_close(session_id)
            .map_err(registry_failure)?;
        let Some(mut closing) = closing else {
            return json_invocation(&FrameProcessorOperationStatus { completed: true });
        };
        let (release_error, close_result) = {
            let state = closing.value_mut().ok_or_else(contract_failure)?;
            let release_error = release_processed_frames(state);
            let close_result = state.session.close();
            (release_error, close_result)
        };
        match close_result {
            Ok(()) => {
                closing.commit();
                if let Some(error) = release_error {
                    return Err(frame_processor_failure(error));
                }
                json_invocation(&FrameProcessorOperationStatus { completed: true })
            }
            Err(error) => {
                if let Some(release_error) = release_error {
                    return Err(frame_processor_failure(release_error));
                }
                Err(frame_processor_failure(error))
            }
        }
    }
}

impl<F> RawExportInterface for FrameProcessorAdapter<F>
where
    F: FrameProcessorPluginFactory,
{
    fn kind(&self) -> ExportInterfaceKind {
        ExportInterfaceKind::FrameProcessor
    }

    fn instance_id(&self) -> &str {
        &self.instance_id
    }

    fn invoke(&self, operation: ExportOperation<'_>) -> Result<ExportInvocation, ExportFailure> {
        match operation {
            ExportOperation::Capabilities => json_invocation(&self.factory.capabilities()),
            ExportOperation::OpenSession { config_json } => {
                let config =
                    decode::<FrameProcessorSessionConfig>(config_json).map_err(|message| {
                        frame_processor_failure(FrameProcessorError::payload_codec(message))
                    })?;
                let mut session = self
                    .factory
                    .open_session(&config)
                    .map_err(frame_processor_failure)?;
                let session_info = match catch_unwind(AssertUnwindSafe(|| session.session_info())) {
                    Ok(session_info) => session_info,
                    Err(panic) => {
                        attempt_open_failure_cleanup(|| {
                            let _ = session.close();
                        });
                        resume_unwind(panic);
                    }
                };
                let payload = match encode(&session_info) {
                    Ok(payload) => payload,
                    Err(error) => {
                        attempt_open_failure_cleanup(|| {
                            let _ = session.close();
                        });
                        return Err(error);
                    }
                };
                let state = FrameProcessorSessionState {
                    session,
                    frames: HashMap::new(),
                };
                let session_id = match lock_registry(&self.sessions).insert(state) {
                    Ok(session_id) => session_id,
                    Err((error, mut state)) => {
                        let _ = release_processed_frames(&mut state);
                        let _ = state.session.close();
                        return Err(registry_failure(error));
                    }
                };
                Ok(ExportInvocation::OpenSession {
                    session_id,
                    payload,
                })
            }
            ExportOperation::FrameSubmit {
                session_id,
                submit_json,
                native_handle,
            } => {
                let submit =
                    decode::<FrameProcessorSubmitFrame>(submit_json).map_err(|message| {
                        frame_processor_failure(FrameProcessorError::payload_codec(message))
                    })?;
                let handle = usize::try_from(native_handle).map_err(|_| {
                    frame_processor_failure(FrameProcessorError::abi_violation(
                        "native frame handle does not fit this process",
                    ))
                })?;
                let mut session = self.acquire(session_id)?;
                let state = session.value_mut().ok_or_else(contract_failure)?;
                let result = state
                    .session
                    .submit_frame(
                        FrameProcessorInputFrame::from_abi(&submit.metadata, handle),
                        &submit,
                    )
                    .map_err(frame_processor_failure)?;
                json_invocation(&result)
            }
            ExportOperation::FrameReceive { session_id } => {
                let mut session = self.acquire(session_id)?;
                let state = session.value_mut().ok_or_else(contract_failure)?;
                if state.frames.len() >= VESPER_MAX_LEASES_PER_SESSION {
                    return Err(ExportFailure::with_status(status::EXHAUSTED, Vec::new()));
                }
                match state
                    .session
                    .receive_frame()
                    .map_err(frame_processor_failure)?
                {
                    FrameProcessorReceiveOutput::Frame(output) => {
                        let requires_release =
                            frame_processor_output_requires_release(&output.frame);
                        let mut metadata = FrameProcessorReceiveFrameMetadata::frame(
                            output.frame.metadata.clone(),
                        );
                        metadata.timings = output.timings;
                        metadata.source_frame_id = output.source_frame_id;
                        metadata.message = output.message;
                        let metadata = match encode(&metadata) {
                            Ok(metadata) => metadata,
                            Err(failure) => {
                                return Err(release_processed_frame_after_failure(
                                    state,
                                    output.frame,
                                    failure,
                                ));
                            }
                        };
                        let native_handle = match u64::try_from(output.frame.handle) {
                            Ok(native_handle) => native_handle,
                            Err(_) => {
                                let failure =
                                    frame_processor_failure(FrameProcessorError::abi_violation(
                                        "native frame handle does not fit the native ABI wire type",
                                    ));
                                return Err(release_processed_frame_after_failure(
                                    state,
                                    output.frame,
                                    failure,
                                ));
                            }
                        };
                        let lease_id = if requires_release {
                            match self.lease_sequence.allocate() {
                                Ok(lease_id) => lease_id,
                                Err(failure) => {
                                    return Err(release_processed_frame_after_failure(
                                        state,
                                        output.frame,
                                        failure,
                                    ));
                                }
                            }
                        } else {
                            0
                        };
                        if requires_release {
                            state.frames.insert(lease_id, output.frame);
                        }
                        Ok(ExportInvocation::NativeFrame {
                            metadata,
                            native_handle,
                            lease_id,
                            requires_release,
                        })
                    }
                    FrameProcessorReceiveOutput::Pending => Ok(ExportInvocation::NativeFrame {
                        metadata: encode(&FrameProcessorReceiveFrameMetadata::pending())?,
                        native_handle: 0,
                        lease_id: 0,
                        requires_release: false,
                    }),
                    FrameProcessorReceiveOutput::EndOfStream => Ok(ExportInvocation::NativeFrame {
                        metadata: encode(&FrameProcessorReceiveFrameMetadata::end_of_stream())?,
                        native_handle: 0,
                        lease_id: 0,
                        requires_release: false,
                    }),
                }
            }
            ExportOperation::FrameRelease {
                session_id,
                lease_id,
            } => {
                let mut session = self.acquire(session_id)?;
                let state = session.value_mut().ok_or_else(contract_failure)?;
                let frame = state.frames.remove(&lease_id).ok_or_else(stale_failure)?;
                state
                    .session
                    .release_frame(frame)
                    .map_err(frame_processor_failure)?;
                json_invocation(&FrameProcessorOperationStatus { completed: true })
            }
            ExportOperation::SessionFlush { session_id, .. } => {
                let mut session = self.acquire(session_id)?;
                let state = session.value_mut().ok_or_else(contract_failure)?;
                let release_error = release_processed_frames(state);
                let flush_result = state.session.flush();
                if let Some(error) = release_error {
                    return Err(frame_processor_failure(error));
                }
                flush_result.map_err(frame_processor_failure)?;
                json_invocation(&FrameProcessorOperationStatus { completed: true })
            }
            ExportOperation::SessionClose { session_id, .. } => self.close(session_id),
            _ => Err(unexpected_operation("frame processor")),
        }
    }
}

fn release_processed_frames(state: &mut FrameProcessorSessionState) -> Option<FrameProcessorError> {
    let frames = std::mem::take(&mut state.frames);
    let mut first_error = None;
    for (_, frame) in frames {
        if let Err(error) = state.session.release_frame(frame)
            && first_error.is_none()
        {
            first_error = Some(error);
        }
    }
    first_error
}

fn release_processed_frame_after_failure(
    state: &mut FrameProcessorSessionState,
    frame: NativeFrame,
    failure: ExportFailure,
) -> ExportFailure {
    if !frame_processor_output_requires_release(&frame) {
        return failure;
    }
    match state.session.release_frame(frame) {
        Ok(()) => failure,
        Err(cleanup_error) => frame_processor_failure(FrameProcessorError::abi_violation(format!(
            "frame result was rejected and its lease cleanup failed: {cleanup_error}"
        ))),
    }
}

fn frame_processor_output_requires_release(frame: &NativeFrame) -> bool {
    frame
        .metadata
        .release_tracking
        .as_ref()
        .is_none_or(|tracking| tracking.requires_release)
}

fn frame_processor_failure(error: FrameProcessorError) -> ExportFailure {
    let raw_status = match error {
        FrameProcessorError::UnsupportedHandle { .. } => status::UNSUPPORTED,
        FrameProcessorError::PayloadCodec { .. } => status::INVALID_ARGUMENT,
        FrameProcessorError::AbiViolation { .. } => status::ABI_VIOLATION,
        FrameProcessorError::Backpressure { .. } => status::EXHAUSTED,
        FrameProcessorError::Timeout { .. } => status::TIMEOUT,
        _ => status::FAILURE,
    };
    failure(raw_status, &error)
}

pub(super) struct AudioProcessorAdapter<F> {
    instance_id: String,
    factory: F,
    sessions: Mutex<SessionRegistry<Box<dyn AudioProcessorSession>>>,
}

impl<F> AudioProcessorAdapter<F>
where
    F: AudioProcessorPluginFactory,
{
    pub(super) fn new(instance_id: String, factory: F) -> Self {
        Self {
            instance_id,
            factory,
            sessions: Mutex::new(SessionRegistry::default()),
        }
    }

    fn acquire(
        &self,
        session_id: u64,
    ) -> Result<SessionGuard<Box<dyn AudioProcessorSession>>, ExportFailure> {
        lock_registry(&self.sessions)
            .acquire(session_id)
            .map_err(registry_failure)
    }

    fn close(&self, session_id: u64) -> Result<ExportInvocation, ExportFailure> {
        let closing = lock_registry(&self.sessions)
            .begin_close(session_id)
            .map_err(registry_failure)?;
        let Some(mut closing) = closing else {
            return json_invocation(&AudioProcessorOperationStatus { completed: true });
        };
        let close_result = closing.value_mut().ok_or_else(contract_failure)?.close();
        close_result.map_err(audio_processor_failure)?;
        closing.commit();
        json_invocation(&AudioProcessorOperationStatus { completed: true })
    }
}

impl<F> RawExportInterface for AudioProcessorAdapter<F>
where
    F: AudioProcessorPluginFactory,
{
    fn kind(&self) -> ExportInterfaceKind {
        ExportInterfaceKind::AudioProcessor
    }

    fn instance_id(&self) -> &str {
        &self.instance_id
    }

    fn invoke(&self, operation: ExportOperation<'_>) -> Result<ExportInvocation, ExportFailure> {
        match operation {
            ExportOperation::Capabilities => json_invocation(&self.factory.capabilities()),
            ExportOperation::OpenSession { config_json } => {
                let config =
                    decode::<AudioProcessorSessionConfig>(config_json).map_err(|message| {
                        audio_processor_failure(AudioProcessorError::payload_codec(message))
                    })?;
                validate_audio_session_config(&self.factory.capabilities(), &config)?;
                let mut session = self
                    .factory
                    .open_session(&config)
                    .map_err(audio_processor_failure)?;
                if !session
                    .capabilities()
                    .supports_input_format(&config.input_metadata.format)
                    || !session
                        .capabilities()
                        .supports_playback_policy(config.playback_policy)
                {
                    attempt_open_failure_cleanup(|| {
                        let _ = session.close();
                    });
                    return Err(audio_processor_failure(
                        AudioProcessorError::UnsupportedPlaybackPolicy,
                    ));
                }
                match catch_unwind(AssertUnwindSafe(|| {
                    session.configure(config.playback_policy)
                })) {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        attempt_open_failure_cleanup(|| {
                            let _ = session.close();
                        });
                        return Err(audio_processor_failure(error));
                    }
                    Err(panic) => {
                        attempt_open_failure_cleanup(|| {
                            let _ = session.close();
                        });
                        resume_unwind(panic);
                    }
                }
                let session_info = match catch_unwind(AssertUnwindSafe(|| session.session_info())) {
                    Ok(session_info) => session_info,
                    Err(panic) => {
                        attempt_open_failure_cleanup(|| {
                            let _ = session.close();
                        });
                        resume_unwind(panic);
                    }
                };
                let payload = match encode(&session_info) {
                    Ok(payload) => payload,
                    Err(error) => {
                        attempt_open_failure_cleanup(|| {
                            let _ = session.close();
                        });
                        return Err(error);
                    }
                };
                let session_id = match lock_registry(&self.sessions).insert(session) {
                    Ok(session_id) => session_id,
                    Err((error, mut session)) => {
                        let _ = session.close();
                        return Err(registry_failure(error));
                    }
                };
                Ok(ExportInvocation::OpenSession {
                    session_id,
                    payload,
                })
            }
            ExportOperation::AudioConfigure {
                session_id,
                policy_json,
            } => {
                let policy = decode::<AudioPlaybackPolicy>(policy_json).map_err(|message| {
                    audio_processor_failure(AudioProcessorError::payload_codec(message))
                })?;
                policy.validate().map_err(audio_processor_failure)?;
                let mut session = self.acquire(session_id)?;
                let session = session.value_mut().ok_or_else(contract_failure)?;
                if !session.capabilities().supports_playback_policy(policy) {
                    return Err(audio_processor_failure(
                        AudioProcessorError::UnsupportedPlaybackPolicy,
                    ));
                }
                session.configure(policy).map_err(audio_processor_failure)?;
                json_invocation(&AudioProcessorOperationStatus { completed: true })
            }
            ExportOperation::AudioProcess {
                session_id,
                metadata_json,
                pcm_data,
            } => {
                if pcm_data.len() as u64 > VESPER_MAX_PCM_BYTES {
                    return Err(audio_processor_failure(AudioProcessorError::InvalidPcm(
                        format!(
                            "PCM payload exceeds the {VESPER_MAX_PCM_BYTES}-byte protocol limit"
                        ),
                    )));
                }
                let metadata = decode(metadata_json).map_err(|message| {
                    audio_processor_failure(AudioProcessorError::payload_codec(message))
                })?;
                let frame = DecoderPcmFrame {
                    metadata,
                    data: pcm_data.to_vec(),
                };
                frame.validate().map_err(|error| {
                    audio_processor_failure(AudioProcessorError::InvalidPcm(error.to_string()))
                })?;
                let mut session = self.acquire(session_id)?;
                let output = session
                    .value_mut()
                    .ok_or_else(contract_failure)?
                    .process(frame)
                    .map_err(audio_processor_failure)?;
                output.validate().map_err(|error| {
                    audio_processor_failure(AudioProcessorError::abi_violation(format!(
                        "processor returned invalid PCM: {error}"
                    )))
                })?;
                if output.data.len() as u64 > VESPER_MAX_PCM_BYTES {
                    return Err(audio_processor_failure(AudioProcessorError::abi_violation(
                        format!(
                            "processor output exceeds the {VESPER_MAX_PCM_BYTES}-byte protocol limit"
                        ),
                    )));
                }
                Ok(ExportInvocation::PcmFrame {
                    metadata: encode(&output.metadata)?,
                    data: output.data,
                })
            }
            ExportOperation::SessionFlush { session_id, .. } => {
                let mut session = self.acquire(session_id)?;
                session
                    .value_mut()
                    .ok_or_else(contract_failure)?
                    .flush()
                    .map_err(audio_processor_failure)?;
                json_invocation(&AudioProcessorOperationStatus { completed: true })
            }
            ExportOperation::SessionClose { session_id, .. } => self.close(session_id),
            _ => Err(unexpected_operation("audio processor")),
        }
    }
}

fn validate_audio_session_config(
    capabilities: &crate::AudioProcessorCapabilities,
    config: &AudioProcessorSessionConfig,
) -> Result<(), ExportFailure> {
    config.input_metadata.validate().map_err(|error| {
        audio_processor_failure(AudioProcessorError::InvalidPcm(error.to_string()))
    })?;
    config
        .playback_policy
        .validate()
        .map_err(audio_processor_failure)?;
    if !capabilities.supports_input_format(&config.input_metadata.format)
        || !capabilities.supports_playback_policy(config.playback_policy)
    {
        return Err(audio_processor_failure(
            AudioProcessorError::UnsupportedPlaybackPolicy,
        ));
    }
    Ok(())
}

fn audio_processor_failure(error: AudioProcessorError) -> ExportFailure {
    let raw_status = match error {
        AudioProcessorError::InvalidCapacity
        | AudioProcessorError::InvalidPcm(_)
        | AudioProcessorError::InvalidPlaybackRate { .. }
        | AudioProcessorError::PayloadCodec(_) => status::INVALID_ARGUMENT,
        AudioProcessorError::UnsupportedPlaybackPolicy => status::UNSUPPORTED,
        AudioProcessorError::AbiViolation(_) => status::ABI_VIOLATION,
        AudioProcessorError::Backpressure(_) => status::EXHAUSTED,
        AudioProcessorError::Timeout(_) => status::TIMEOUT,
        AudioProcessorError::Closed | AudioProcessorError::Processor(_) => status::FAILURE,
    };
    failure(raw_status, &error)
}

struct SourcePacketSessionState {
    session: Box<dyn SourceNormalizerPacketSession>,
    packet_handles: HashMap<u64, usize>,
}

pub(super) struct SourceNormalizerPacketAdapter<F> {
    instance_id: String,
    factory: F,
    sessions: Mutex<SessionRegistry<SourcePacketSessionState>>,
    lease_sequence: LeaseSequence,
}

impl<F> SourceNormalizerPacketAdapter<F>
where
    F: SourceNormalizerPacketPluginFactory,
{
    pub(super) fn new(instance_id: String, factory: F) -> Self {
        Self {
            instance_id,
            factory,
            sessions: Mutex::new(SessionRegistry::default()),
            lease_sequence: LeaseSequence::default(),
        }
    }

    fn acquire(
        &self,
        session_id: u64,
    ) -> Result<SessionGuard<SourcePacketSessionState>, ExportFailure> {
        lock_registry(&self.sessions)
            .acquire(session_id)
            .map_err(registry_failure)
    }

    fn close(
        &self,
        session_id: u64,
        effects: &ExportCallEffects,
    ) -> Result<ExportInvocation, ExportFailure> {
        let closing = lock_registry(&self.sessions)
            .begin_close(session_id)
            .map_err(registry_failure)?;
        let Some(mut closing) = closing else {
            return json_invocation(&completed_source_operation());
        };
        let (release_error, close_result) = {
            let state = closing.value_mut().ok_or_else(contract_failure)?;
            effects.mark_packet_lease_state_changed();
            let release_error = release_source_packets(state);
            let close_result = state.session.close();
            (release_error, close_result)
        };
        match close_result {
            Ok(()) => {
                closing.commit();
                if let Some(error) = release_error {
                    return Err(source_failure(error));
                }
                json_invocation(&completed_source_operation())
            }
            Err(error) => {
                if let Some(release_error) = release_error {
                    return Err(source_failure(release_error));
                }
                Err(source_failure(error))
            }
        }
    }
}

impl<F> RawExportInterface for SourceNormalizerPacketAdapter<F>
where
    F: SourceNormalizerPacketPluginFactory,
{
    fn kind(&self) -> ExportInterfaceKind {
        ExportInterfaceKind::SourceNormalizerPacket
    }

    fn instance_id(&self) -> &str {
        &self.instance_id
    }

    fn invoke(&self, operation: ExportOperation<'_>) -> Result<ExportInvocation, ExportFailure> {
        match operation {
            ExportOperation::Capabilities => json_invocation(&self.factory.packet_capabilities()),
            ExportOperation::OpenSession { config_json } => {
                let config = decode::<SourceNormalizerPacketSessionConfig>(config_json).map_err(
                    |message| source_failure(SourceNormalizerError::payload_codec(message)),
                )?;
                validate_source_input(&config.input, &config.headers)?;
                let mut session = self
                    .factory
                    .open_packet_session(&config)
                    .map_err(source_failure)?;
                let stream_info = match catch_unwind(AssertUnwindSafe(|| session.stream_info())) {
                    Ok(stream_info) => stream_info,
                    Err(panic) => {
                        attempt_open_failure_cleanup(|| {
                            let _ = session.close();
                        });
                        resume_unwind(panic);
                    }
                };
                let payload = match encode(&stream_info) {
                    Ok(payload) => payload,
                    Err(error) => {
                        attempt_open_failure_cleanup(|| {
                            let _ = session.close();
                        });
                        return Err(error);
                    }
                };
                let state = SourcePacketSessionState {
                    session,
                    packet_handles: HashMap::new(),
                };
                let session_id = match lock_registry(&self.sessions).insert(state) {
                    Ok(session_id) => session_id,
                    Err((error, mut state)) => {
                        let _ = release_source_packets(&mut state);
                        let _ = state.session.close();
                        return Err(registry_failure(error));
                    }
                };
                Ok(ExportInvocation::OpenSession {
                    session_id,
                    payload,
                })
            }
            ExportOperation::PacketRead { session_id } => {
                let mut session = self.acquire(session_id)?;
                let state = session.value_mut().ok_or_else(contract_failure)?;
                if state.packet_handles.len() >= VESPER_MAX_LEASES_PER_SESSION {
                    return Err(ExportFailure::with_status(status::EXHAUSTED, Vec::new()));
                }
                let (metadata, data, packet_handle) = {
                    let packet = state.session.read_packet().map_err(source_failure)?;
                    let data = if packet.data.len() as u64 > VESPER_MAX_PACKET_BYTES {
                        None
                    } else {
                        Some(packet.data.to_vec())
                    };
                    (packet.metadata.clone(), data, packet.handle)
                };
                let Some(data) = data else {
                    let failure = source_failure(SourceNormalizerError::abi_violation(format!(
                        "packet payload exceeds the {VESPER_MAX_PACKET_BYTES}-byte protocol limit"
                    )));
                    return Err(release_source_packet_after_failure(
                        state,
                        packet_handle,
                        failure,
                    ));
                };
                match metadata.status {
                    SourceNormalizerReadPacketStatus::Packet => {
                        if metadata.packet.is_none() {
                            let failure = source_failure(SourceNormalizerError::abi_violation(
                                "packet status requires packet metadata",
                            ));
                            return Err(release_source_packet_after_failure(
                                state,
                                packet_handle,
                                failure,
                            ));
                        }
                        let metadata_json = match encode(&metadata) {
                            Ok(metadata_json) => metadata_json,
                            Err(failure) => {
                                return Err(release_source_packet_after_failure(
                                    state,
                                    packet_handle,
                                    failure,
                                ));
                            }
                        };
                        let lease_id = match self.lease_sequence.allocate() {
                            Ok(lease_id) => lease_id,
                            Err(failure) => {
                                return Err(release_source_packet_after_failure(
                                    state,
                                    packet_handle,
                                    failure,
                                ));
                            }
                        };
                        state.packet_handles.insert(lease_id, packet_handle);
                        Ok(ExportInvocation::Packet {
                            metadata: metadata_json,
                            data,
                            lease_id,
                        })
                    }
                    SourceNormalizerReadPacketStatus::NeedMoreData
                    | SourceNormalizerReadPacketStatus::EndOfStream => {
                        if metadata.packet.is_some() || !data.is_empty() || packet_handle != 0 {
                            let failure = source_failure(SourceNormalizerError::abi_violation(
                                "non-packet status cannot carry packet data or a packet handle",
                            ));
                            return Err(if packet_handle == 0 {
                                failure
                            } else {
                                release_source_packet_after_failure(state, packet_handle, failure)
                            });
                        }
                        Ok(ExportInvocation::Packet {
                            metadata: encode(&metadata)?,
                            data: Vec::new(),
                            lease_id: 0,
                        })
                    }
                }
            }
            ExportOperation::PacketRelease {
                session_id,
                lease_id,
                effects,
            } => {
                let mut session = self.acquire(session_id)?;
                let state = session.value_mut().ok_or_else(contract_failure)?;
                let packet_handle = state
                    .packet_handles
                    .remove(&lease_id)
                    .ok_or_else(stale_failure)?;
                effects.mark_packet_lease_state_changed();
                state
                    .session
                    .release_packet(packet_handle)
                    .map_err(source_failure)?;
                json_invocation(&completed_source_operation())
            }
            ExportOperation::PacketSeek {
                session_id,
                seek_json,
                effects,
            } => {
                let seek = decode::<SourceNormalizerPacketSeek>(seek_json).map_err(|message| {
                    source_failure(SourceNormalizerError::payload_codec(message))
                })?;
                let mut session = self.acquire(session_id)?;
                let state = session.value_mut().ok_or_else(contract_failure)?;
                effects.mark_packet_lease_state_changed();
                let release_error = release_source_packets(state);
                let seek_result = state.session.seek(&seek);
                if let Some(error) = release_error {
                    return Err(source_failure(error));
                }
                json_invocation(&seek_result.map_err(source_failure)?)
            }
            ExportOperation::SessionFlush {
                session_id,
                effects,
            } => {
                let mut session = self.acquire(session_id)?;
                let state = session.value_mut().ok_or_else(contract_failure)?;
                effects.mark_packet_lease_state_changed();
                let release_error = release_source_packets(state);
                let flush_result = state.session.flush();
                if let Some(error) = release_error {
                    return Err(source_failure(error));
                }
                json_invocation(&flush_result.map_err(source_failure)?)
            }
            ExportOperation::SessionClose {
                session_id,
                effects,
            } => self.close(session_id, effects),
            _ => Err(unexpected_operation("source normalizer packet interface")),
        }
    }
}

struct SourceResourceSessionState {
    session: Box<dyn SourceNormalizerResourceSession>,
}

pub(super) struct SourceNormalizerResourceAdapter<F> {
    instance_id: String,
    factory: F,
    sessions: Mutex<SessionRegistry<SourceResourceSessionState>>,
}

impl<F> SourceNormalizerResourceAdapter<F>
where
    F: SourceNormalizerResourcePluginFactory,
{
    pub(super) fn new(instance_id: String, factory: F) -> Self {
        Self {
            instance_id,
            factory,
            sessions: Mutex::new(SessionRegistry::default()),
        }
    }

    fn acquire(
        &self,
        session_id: u64,
    ) -> Result<SessionGuard<SourceResourceSessionState>, ExportFailure> {
        lock_registry(&self.sessions)
            .acquire(session_id)
            .map_err(registry_failure)
    }

    fn close(&self, session_id: u64) -> Result<ExportInvocation, ExportFailure> {
        let closing = lock_registry(&self.sessions)
            .begin_close(session_id)
            .map_err(registry_failure)?;
        let Some(mut closing) = closing else {
            return json_invocation(&completed_source_operation());
        };
        let close_result = closing
            .value_mut()
            .ok_or_else(contract_failure)?
            .session
            .close();
        match close_result {
            Ok(()) => {
                closing.commit();
                json_invocation(&completed_source_operation())
            }
            Err(error) => Err(source_failure(error)),
        }
    }
}

impl<F> RawExportInterface for SourceNormalizerResourceAdapter<F>
where
    F: SourceNormalizerResourcePluginFactory,
{
    fn kind(&self) -> ExportInterfaceKind {
        ExportInterfaceKind::SourceNormalizerResource
    }

    fn instance_id(&self) -> &str {
        &self.instance_id
    }

    fn invoke(&self, operation: ExportOperation<'_>) -> Result<ExportInvocation, ExportFailure> {
        match operation {
            ExportOperation::Capabilities => json_invocation(&self.factory.resource_capabilities()),
            ExportOperation::OpenSession { config_json } => {
                let config = decode::<SourceNormalizerResourceSessionConfig>(config_json).map_err(
                    |message| source_failure(SourceNormalizerError::payload_codec(message)),
                )?;
                validate_source_input(&config.input, &config.headers)?;
                let mut session = self
                    .factory
                    .open_resource_session(&config)
                    .map_err(source_failure)?;
                let session_info = match catch_unwind(AssertUnwindSafe(|| session.session_info())) {
                    Ok(session_info) => session_info,
                    Err(panic) => {
                        attempt_open_failure_cleanup(|| {
                            let _ = session.close();
                        });
                        resume_unwind(panic);
                    }
                };
                let payload = match encode(&session_info) {
                    Ok(payload) => payload,
                    Err(error) => {
                        attempt_open_failure_cleanup(|| {
                            let _ = session.close();
                        });
                        return Err(error);
                    }
                };
                let state = SourceResourceSessionState { session };
                let session_id = match lock_registry(&self.sessions).insert(state) {
                    Ok(session_id) => session_id,
                    Err((error, mut state)) => {
                        let _ = state.session.close();
                        return Err(registry_failure(error));
                    }
                };
                Ok(ExportInvocation::OpenSession {
                    session_id,
                    payload,
                })
            }
            ExportOperation::ResourcePoll { session_id } => {
                let mut session = self.acquire(session_id)?;
                let state = session.value_mut().ok_or_else(contract_failure)?;
                let result = state.session.poll().map_err(source_failure)?;
                json_invocation(&result)
            }
            ExportOperation::ResourceWait {
                session_id,
                timeout_ms,
            } => {
                let mut session = self.acquire(session_id)?;
                let state = session.value_mut().ok_or_else(contract_failure)?;
                let result = state
                    .session
                    .wait_for_update(Duration::from_millis(timeout_ms))
                    .map_err(source_failure)?;
                json_invocation(&result)
            }
            ExportOperation::ResourceCancel { session_id } => {
                let mut session = self.acquire(session_id)?;
                let state = session.value_mut().ok_or_else(contract_failure)?;
                let result = state.session.cancel().map_err(source_failure)?;
                json_invocation(&result)
            }
            ExportOperation::SessionClose { session_id, .. } => self.close(session_id),
            _ => Err(unexpected_operation("source normalizer resource interface")),
        }
    }
}

fn release_source_packets(state: &mut SourcePacketSessionState) -> Option<SourceNormalizerError> {
    let packet_handles = std::mem::take(&mut state.packet_handles);
    let mut first_error = None;
    for (_, packet_handle) in packet_handles {
        if let Err(error) = state.session.release_packet(packet_handle)
            && first_error.is_none()
        {
            first_error = Some(error);
        }
    }
    first_error
}

fn release_source_packet_after_failure(
    state: &mut SourcePacketSessionState,
    packet_handle: usize,
    failure: ExportFailure,
) -> ExportFailure {
    match state.session.release_packet(packet_handle) {
        Ok(()) => failure,
        Err(cleanup_error) => source_failure(SourceNormalizerError::abi_violation(format!(
            "packet result was rejected and its lease cleanup failed: {cleanup_error}"
        ))),
    }
}

fn completed_source_operation() -> SourceNormalizerOperationStatus {
    SourceNormalizerOperationStatus {
        completed: true,
        message: None,
    }
}

fn attempt_open_failure_cleanup(cleanup: impl FnOnce()) {
    let _ = catch_unwind(AssertUnwindSafe(cleanup));
}

fn validate_source_input(input: &str, headers: &[(String, String)]) -> Result<(), ExportFailure> {
    validate_source_normalizer_plugin_input(input, headers).map_err(source_failure)
}

fn source_failure(error: SourceNormalizerError) -> ExportFailure {
    let raw_status = match error {
        SourceNormalizerError::UnsupportedRuntimeProfile { .. }
        | SourceNormalizerError::UnsupportedOperation { .. } => status::UNSUPPORTED,
        SourceNormalizerError::InvalidInput { .. }
        | SourceNormalizerError::PayloadCodec { .. }
        | SourceNormalizerError::Configuration { .. } => status::INVALID_ARGUMENT,
        SourceNormalizerError::AbiViolation { .. } => status::ABI_VIOLATION,
        SourceNormalizerError::Timeout { .. } => status::TIMEOUT,
        SourceNormalizerError::ResourceExhausted { .. } => status::EXHAUSTED,
        _ => status::FAILURE,
    };
    failure(raw_status, &error)
}

fn release_decoder_frames(
    state: &mut DecoderSessionState,
    presentation_release: bool,
) -> Option<DecoderError> {
    let frames = std::mem::take(&mut state.frames);
    let mut first_error = None;
    for (_, frame) in frames {
        let result = if presentation_release {
            state
                .session
                .release_native_frame_with_presentation(frame, false)
        } else {
            state.session.release_native_frame(frame)
        };
        if let Err(error) = result
            && first_error.is_none()
        {
            first_error = Some(error);
        }
    }
    first_error
}

fn release_decoder_frame_after_failure(
    state: &mut DecoderSessionState,
    frame: DecoderNativeFrame,
    presentation_release: bool,
    failure: ExportFailure,
) -> ExportFailure {
    let result = if presentation_release {
        state
            .session
            .release_native_frame_with_presentation(frame, false)
    } else {
        state.session.release_native_frame(frame)
    };
    match result {
        Ok(()) => failure,
        Err(cleanup_error) => decoder_failure(DecoderError::abi_violation(format!(
            "decoder frame result was rejected and its lease cleanup failed: {cleanup_error}"
        ))),
    }
}

fn decoder_failure(error: DecoderError) -> ExportFailure {
    let raw_status = match error {
        DecoderError::UnsupportedCodec { .. } | DecoderError::UnsupportedCapability { .. } => {
            status::UNSUPPORTED
        }
        DecoderError::InvalidPacket { .. } | DecoderError::PayloadCodec { .. } => {
            status::INVALID_ARGUMENT
        }
        DecoderError::AbiViolation { .. } => status::ABI_VIOLATION,
        _ => status::FAILURE,
    };
    failure(raw_status, &error)
}

fn lock_registry<S>(registry: &Mutex<SessionRegistry<S>>) -> MutexGuard<'_, SessionRegistry<S>> {
    registry.lock().unwrap_or_else(|error| error.into_inner())
}

fn registry_failure(error: SessionRegistryError) -> ExportFailure {
    let raw_status = match error {
        SessionRegistryError::Stale => status::STALE_HANDLE,
        SessionRegistryError::Busy | SessionRegistryError::Exhausted => status::EXHAUSTED,
    };
    ExportFailure::with_status(raw_status, Vec::new())
}

fn stale_failure() -> ExportFailure {
    ExportFailure::with_status(status::STALE_HANDLE, Vec::new())
}

fn contract_failure() -> ExportFailure {
    ExportFailure::with_status(status::ABI_VIOLATION, Vec::new())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    use super::*;
    use crate::{
        DecoderCapabilities, DecoderFrameFormat, DecoderMediaKind, DecoderNativeFrame,
        DecoderNativeRequirements, DecoderPacket, DecoderPacketResult,
        DecoderReceiveNativeFrameOutput, DecoderSessionInfo, FrameProcessorCapabilities,
        FrameProcessorFrameTimings, FrameProcessorOutputFrame, FrameProcessorReceiveOutput,
        FrameProcessorSessionInfo, FrameProcessorSubmitResult, NativeFrameMetadata,
        NativeHandleKind, SourceNormalizerOutputRoute, SourceNormalizerPacket,
        SourceNormalizerPacketCapabilities, SourceNormalizerPacketLease,
        SourceNormalizerPacketStreamInfo, SourceNormalizerReadPacketMetadata,
        SourceNormalizerResourceCapabilities,
    };

    #[derive(Default)]
    struct SessionCounters {
        opens: AtomicUsize,
        releases: AtomicUsize,
        flushes: AtomicUsize,
        closes: AtomicUsize,
    }

    struct FixtureDecoderFactory {
        closes: Arc<AtomicUsize>,
        fail_first_close: bool,
        panic_session_info: bool,
    }

    impl NativeDecoderPluginFactory for FixtureDecoderFactory {
        fn name(&self) -> &str {
            "fixture-decoder"
        }

        fn capabilities(&self) -> DecoderCapabilities {
            DecoderCapabilities::default()
        }

        fn native_requirements(&self) -> DecoderNativeRequirements {
            DecoderNativeRequirements::default()
        }

        fn open_native_session(
            &self,
            _config: &DecoderSessionConfig,
        ) -> Result<Box<dyn NativeDecoderSession>, DecoderError> {
            Ok(Box::new(FixtureDecoderSession {
                closes: self.closes.clone(),
                fail_first_close: self.fail_first_close,
                panic_session_info: self.panic_session_info,
            }))
        }
    }

    struct FixtureDecoderSession {
        closes: Arc<AtomicUsize>,
        fail_first_close: bool,
        panic_session_info: bool,
    }

    impl NativeDecoderSession for FixtureDecoderSession {
        fn session_info(&self) -> DecoderSessionInfo {
            assert!(!self.panic_session_info, "fixture session-info panic");
            DecoderSessionInfo::default()
        }

        fn send_packet(
            &mut self,
            _packet: &DecoderPacket,
            _data: &[u8],
        ) -> Result<DecoderPacketResult, DecoderError> {
            Ok(DecoderPacketResult::default())
        }

        fn receive_native_frame(
            &mut self,
        ) -> Result<DecoderReceiveNativeFrameOutput, DecoderError> {
            Ok(DecoderReceiveNativeFrameOutput::NeedMoreInput)
        }

        fn release_native_frame(&mut self, _frame: DecoderNativeFrame) -> Result<(), DecoderError> {
            Ok(())
        }

        fn flush(&mut self) -> Result<(), DecoderError> {
            Ok(())
        }

        fn close(&mut self) -> Result<(), DecoderError> {
            let previous = self.closes.fetch_add(1, Ordering::Relaxed);
            if self.fail_first_close && previous == 0 {
                return Err(DecoderError::internal("fixture close failure"));
            }
            Ok(())
        }
    }

    fn open_session(adapter: &NativeDecoderAdapter<FixtureDecoderFactory>) -> u64 {
        let config = serde_json::to_vec(&DecoderSessionConfig::default()).expect("config JSON");
        let invocation = adapter
            .invoke(ExportOperation::OpenSession {
                config_json: &config,
            })
            .expect("open session");
        let ExportInvocation::OpenSession { session_id, .. } = invocation else {
            panic!("expected open output");
        };
        session_id
    }

    #[test]
    fn decoder_sessions_use_generation_tokens_and_idempotent_close() {
        let closes = Arc::new(AtomicUsize::new(0));
        let adapter = NativeDecoderAdapter::new(
            "dev.vesper.fixture.decoder".to_owned(),
            FixtureDecoderFactory {
                closes: closes.clone(),
                fail_first_close: false,
                panic_session_info: false,
            },
        );
        let first = open_session(&adapter);
        let effects = ExportCallEffects::default();
        assert_ne!(first, 0);
        adapter
            .invoke(ExportOperation::SessionClose {
                session_id: first,
                effects: &effects,
            })
            .expect("close first");
        adapter
            .invoke(ExportOperation::SessionClose {
                session_id: first,
                effects: &effects,
            })
            .expect("close is idempotent");
        assert_eq!(closes.load(Ordering::Relaxed), 1);

        let second = open_session(&adapter);
        assert_ne!(second, first);
        let packet = serde_json::to_vec(&DecoderPacket::default()).expect("packet JSON");
        let error = adapter
            .invoke(ExportOperation::DecoderSendPacket {
                session_id: first,
                packet_json: &packet,
                packet_data: &[],
            })
            .expect_err("old generation is stale");
        assert_eq!(error.status(), status::STALE_HANDLE);
        adapter
            .invoke(ExportOperation::SessionClose {
                session_id: second,
                effects: &effects,
            })
            .expect("close second");
        assert_eq!(closes.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn decoder_adapter_rejects_oversized_packet_before_calling_the_session() {
        let closes = Arc::new(AtomicUsize::new(0));
        let adapter = NativeDecoderAdapter::new(
            "dev.vesper.fixture.decoder".to_owned(),
            FixtureDecoderFactory {
                closes,
                fail_first_close: false,
                panic_session_info: false,
            },
        );
        let session_id = open_session(&adapter);
        let packet = serde_json::to_vec(&DecoderPacket::default()).expect("packet JSON");
        let oversized =
            vec![0; usize::try_from(VESPER_MAX_PACKET_BYTES).expect("packet limit fits usize") + 1];

        let error = adapter
            .invoke(ExportOperation::DecoderSendPacket {
                session_id,
                packet_json: &packet,
                packet_data: &oversized,
            })
            .expect_err("oversized decoder packet must be rejected");
        assert_eq!(error.status(), status::INVALID_ARGUMENT);
    }

    #[test]
    fn decoder_close_failure_restores_session_for_retry() {
        let closes = Arc::new(AtomicUsize::new(0));
        let adapter = NativeDecoderAdapter::new(
            "dev.vesper.fixture.decoder".to_owned(),
            FixtureDecoderFactory {
                closes: closes.clone(),
                fail_first_close: true,
                panic_session_info: false,
            },
        );
        let session_id = open_session(&adapter);
        let effects = ExportCallEffects::default();
        let first = adapter
            .invoke(ExportOperation::SessionClose {
                session_id,
                effects: &effects,
            })
            .expect_err("first close fails");
        assert_eq!(first.status(), status::FAILURE);
        adapter
            .invoke(ExportOperation::SessionClose {
                session_id,
                effects: &effects,
            })
            .expect("second close retries the author session");
        adapter
            .invoke(ExportOperation::SessionClose {
                session_id,
                effects: &effects,
            })
            .expect("successful close is idempotent");
        assert_eq!(closes.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn decoder_session_info_panic_closes_unregistered_session() {
        let closes = Arc::new(AtomicUsize::new(0));
        let adapter = NativeDecoderAdapter::new(
            "dev.vesper.fixture.decoder".to_owned(),
            FixtureDecoderFactory {
                closes: closes.clone(),
                fail_first_close: false,
                panic_session_info: true,
            },
        );
        let config = serde_json::to_vec(&DecoderSessionConfig::default()).expect("config JSON");
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _ = adapter.invoke(ExportOperation::OpenSession {
                config_json: &config,
            });
        }));
        assert!(result.is_err());
        assert_eq!(closes.load(Ordering::Relaxed), 1);
    }

    struct FixtureFrameProcessorFactory {
        counters: Arc<SessionCounters>,
        requires_release: bool,
    }

    impl FrameProcessorPluginFactory for FixtureFrameProcessorFactory {
        fn name(&self) -> &str {
            "fixture-frame-processor"
        }

        fn capabilities(&self) -> FrameProcessorCapabilities {
            FrameProcessorCapabilities::default()
        }

        fn open_session(
            &self,
            _config: &FrameProcessorSessionConfig,
        ) -> Result<Box<dyn FrameProcessorSession>, FrameProcessorError> {
            self.counters.opens.fetch_add(1, Ordering::Relaxed);
            Ok(Box::new(FixtureFrameProcessorSession {
                counters: self.counters.clone(),
                requires_release: self.requires_release,
            }))
        }
    }

    struct FixtureFrameProcessorSession {
        counters: Arc<SessionCounters>,
        requires_release: bool,
    }

    impl FrameProcessorSession for FixtureFrameProcessorSession {
        fn session_info(&self) -> FrameProcessorSessionInfo {
            FrameProcessorSessionInfo::default()
        }

        fn submit_frame(
            &mut self,
            _frame: FrameProcessorInputFrame<'_>,
            _submit: &FrameProcessorSubmitFrame,
        ) -> Result<FrameProcessorSubmitResult, FrameProcessorError> {
            Ok(FrameProcessorSubmitResult::default())
        }

        fn receive_frame(&mut self) -> Result<FrameProcessorReceiveOutput, FrameProcessorError> {
            let mut frame = fixture_native_frame();
            frame.metadata.release_tracking = Some(crate::NativeFrameReleaseTracking {
                frame_id: frame.metadata.frame_id,
                requires_release: self.requires_release,
            });
            Ok(FrameProcessorReceiveOutput::Frame(
                FrameProcessorOutputFrame {
                    frame,
                    timings: FrameProcessorFrameTimings::default(),
                    source_frame_id: None,
                    message: None,
                },
            ))
        }

        fn release_frame(&mut self, _frame: NativeFrame) -> Result<(), FrameProcessorError> {
            self.counters.releases.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn flush(&mut self) -> Result<(), FrameProcessorError> {
            self.counters.flushes.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn close(&mut self) -> Result<(), FrameProcessorError> {
            self.counters.closes.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    fn fixture_native_frame() -> NativeFrame {
        NativeFrame {
            metadata: NativeFrameMetadata {
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
                handle_kind: NativeHandleKind::CvPixelBuffer,
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
            handle: 7,
            lease_token: None,
        }
    }

    fn open_frame_session(adapter: &FrameProcessorAdapter<FixtureFrameProcessorFactory>) -> u64 {
        let config = FrameProcessorSessionConfig {
            processor_index: 0,
            input_metadata: fixture_native_frame().metadata,
            max_in_flight_frames: None,
        };
        let config = serde_json::to_vec(&config).expect("frame config JSON");
        let invocation = adapter
            .invoke(ExportOperation::OpenSession {
                config_json: &config,
            })
            .expect("open frame session");
        let ExportInvocation::OpenSession { session_id, .. } = invocation else {
            panic!("expected open output");
        };
        session_id
    }

    fn receive_frame_lease(
        adapter: &FrameProcessorAdapter<FixtureFrameProcessorFactory>,
        session_id: u64,
    ) -> u64 {
        let invocation = adapter
            .invoke(ExportOperation::FrameReceive { session_id })
            .expect("receive frame");
        let ExportInvocation::NativeFrame { lease_id, .. } = invocation else {
            panic!("expected native frame output");
        };
        lease_id
    }

    #[test]
    fn frame_processor_leases_are_session_scoped_bounded_and_drained() {
        let counters = Arc::new(SessionCounters::default());
        let adapter = FrameProcessorAdapter::new(
            "dev.vesper.fixture.frame-processor".to_owned(),
            FixtureFrameProcessorFactory {
                counters: counters.clone(),
                requires_release: true,
            },
        );
        let first = open_frame_session(&adapter);
        let second = open_frame_session(&adapter);
        let effects = ExportCallEffects::default();

        let first_lease = receive_frame_lease(&adapter, first);
        let wrong_session = adapter
            .invoke(ExportOperation::FrameRelease {
                session_id: second,
                lease_id: first_lease,
            })
            .expect_err("a lease cannot be released through another session");
        assert_eq!(wrong_session.status(), status::STALE_HANDLE);

        adapter
            .invoke(ExportOperation::SessionFlush {
                session_id: first,
                effects: &effects,
            })
            .expect("flush drains the first lease");
        assert_eq!(counters.releases.load(Ordering::Relaxed), 1);
        assert_eq!(counters.flushes.load(Ordering::Relaxed), 1);

        for _ in 0..VESPER_MAX_LEASES_PER_SESSION {
            let lease_id = receive_frame_lease(&adapter, first);
            assert_ne!(lease_id, 0);
        }
        let exhausted = adapter
            .invoke(ExportOperation::FrameReceive { session_id: first })
            .expect_err("the per-session lease cap must be enforced");
        assert_eq!(exhausted.status(), status::EXHAUSTED);

        adapter
            .invoke(ExportOperation::SessionClose {
                session_id: first,
                effects: &effects,
            })
            .expect("close drains outstanding leases");
        adapter
            .invoke(ExportOperation::SessionClose {
                session_id: first,
                effects: &effects,
            })
            .expect("close remains idempotent");
        adapter
            .invoke(ExportOperation::SessionClose {
                session_id: second,
                effects: &effects,
            })
            .expect("close second session");
        assert_eq!(counters.releases.load(Ordering::Relaxed), 65);
        assert_eq!(counters.closes.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn frame_processor_borrowed_passthrough_does_not_create_or_release_a_lease() {
        let counters = Arc::new(SessionCounters::default());
        let adapter = FrameProcessorAdapter::new(
            "dev.vesper.fixture.borrowed-frame-processor".to_owned(),
            FixtureFrameProcessorFactory {
                counters: counters.clone(),
                requires_release: false,
            },
        );
        let session_id = open_frame_session(&adapter);
        let effects = ExportCallEffects::default();

        let invocation = adapter
            .invoke(ExportOperation::FrameReceive { session_id })
            .expect("receive borrowed frame");
        let ExportInvocation::NativeFrame {
            lease_id,
            requires_release,
            ..
        } = invocation
        else {
            panic!("expected native frame output");
        };
        assert_eq!(lease_id, 0);
        assert!(!requires_release);

        adapter
            .invoke(ExportOperation::SessionFlush {
                session_id,
                effects: &effects,
            })
            .expect("flush borrowed output");
        adapter
            .invoke(ExportOperation::SessionClose {
                session_id,
                effects: &effects,
            })
            .expect("close borrowed output");
        assert_eq!(counters.releases.load(Ordering::Relaxed), 0);
    }

    struct FixturePacketFactory {
        counters: Arc<SessionCounters>,
        malformed: bool,
        packet_size: usize,
    }

    impl SourceNormalizerPacketPluginFactory for FixturePacketFactory {
        fn name(&self) -> &str {
            "fixture-packet-normalizer"
        }

        fn packet_capabilities(&self) -> SourceNormalizerPacketCapabilities {
            SourceNormalizerPacketCapabilities::default()
        }

        fn open_packet_session(
            &self,
            _config: &SourceNormalizerPacketSessionConfig,
        ) -> Result<Box<dyn SourceNormalizerPacketSession>, SourceNormalizerError> {
            self.counters.opens.fetch_add(1, Ordering::Relaxed);
            Ok(Box::new(FixturePacketSession {
                counters: self.counters.clone(),
                data: vec![1; self.packet_size],
                malformed: self.malformed,
            }))
        }
    }

    struct FixturePacketSession {
        counters: Arc<SessionCounters>,
        data: Vec<u8>,
        malformed: bool,
    }

    impl SourceNormalizerPacketSession for FixturePacketSession {
        fn stream_info(&self) -> SourceNormalizerPacketStreamInfo {
            SourceNormalizerPacketStreamInfo::default()
        }

        fn read_packet(
            &mut self,
        ) -> Result<SourceNormalizerPacketLease<'_>, SourceNormalizerError> {
            Ok(SourceNormalizerPacketLease {
                metadata: if self.malformed {
                    SourceNormalizerReadPacketMetadata {
                        status: SourceNormalizerReadPacketStatus::Packet,
                        packet: None,
                        message: None,
                    }
                } else {
                    SourceNormalizerReadPacketMetadata::packet(SourceNormalizerPacket::default())
                },
                data: &self.data,
                handle: 11,
            })
        }

        fn release_packet(&mut self, _packet_handle: usize) -> Result<(), SourceNormalizerError> {
            self.counters.releases.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn seek(
            &mut self,
            _seek: &SourceNormalizerPacketSeek,
        ) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
            Ok(completed_source_operation())
        }

        fn flush(&mut self) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
            self.counters.flushes.fetch_add(1, Ordering::Relaxed);
            Ok(completed_source_operation())
        }

        fn close(&mut self) -> Result<(), SourceNormalizerError> {
            self.counters.closes.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    fn packet_config(input: &str) -> SourceNormalizerPacketSessionConfig {
        SourceNormalizerPacketSessionConfig {
            runtime_profile: "fixture".to_owned(),
            input: input.to_owned(),
            headers: Vec::new(),
            startup_timeout_ms: None,
            session_timeout_ms: None,
            preferred_media_kind: Default::default(),
        }
    }

    fn open_packet_session(adapter: &SourceNormalizerPacketAdapter<FixturePacketFactory>) -> u64 {
        let config = serde_json::to_vec(&packet_config("file:///tmp/input.mp4"))
            .expect("packet config JSON");
        let invocation = adapter
            .invoke(ExportOperation::OpenSession {
                config_json: &config,
            })
            .expect("open packet session");
        let ExportInvocation::OpenSession { session_id, .. } = invocation else {
            panic!("expected open output");
        };
        session_id
    }

    fn read_packet_lease(
        adapter: &SourceNormalizerPacketAdapter<FixturePacketFactory>,
        session_id: u64,
    ) -> u64 {
        let invocation = adapter
            .invoke(ExportOperation::PacketRead { session_id })
            .expect("read packet");
        let ExportInvocation::Packet { lease_id, .. } = invocation else {
            panic!("expected packet output");
        };
        lease_id
    }

    #[test]
    fn source_packet_leases_are_session_scoped_bounded_and_drained() {
        let counters = Arc::new(SessionCounters::default());
        let adapter = SourceNormalizerPacketAdapter::new(
            "dev.vesper.fixture.packet".to_owned(),
            FixturePacketFactory {
                counters: counters.clone(),
                malformed: false,
                packet_size: 3,
            },
        );
        let first = open_packet_session(&adapter);
        let second = open_packet_session(&adapter);
        let effects = ExportCallEffects::default();

        let first_lease = read_packet_lease(&adapter, first);
        let wrong_session = adapter
            .invoke(ExportOperation::PacketRelease {
                session_id: second,
                lease_id: first_lease,
                effects: &effects,
            })
            .expect_err("a packet lease cannot be released through another session");
        assert_eq!(wrong_session.status(), status::STALE_HANDLE);

        adapter
            .invoke(ExportOperation::SessionFlush {
                session_id: first,
                effects: &effects,
            })
            .expect("flush drains the first packet lease");
        assert_eq!(counters.releases.load(Ordering::Relaxed), 1);
        assert_eq!(counters.flushes.load(Ordering::Relaxed), 1);

        for _ in 0..VESPER_MAX_LEASES_PER_SESSION {
            assert_ne!(read_packet_lease(&adapter, first), 0);
        }
        let exhausted = adapter
            .invoke(ExportOperation::PacketRead { session_id: first })
            .expect_err("the packet lease cap must be enforced");
        assert_eq!(exhausted.status(), status::EXHAUSTED);

        adapter
            .invoke(ExportOperation::SessionClose {
                session_id: first,
                effects: &effects,
            })
            .expect("close drains outstanding packet leases");
        adapter
            .invoke(ExportOperation::SessionClose {
                session_id: first,
                effects: &effects,
            })
            .expect("close remains idempotent");
        adapter
            .invoke(ExportOperation::SessionClose {
                session_id: second,
                effects: &effects,
            })
            .expect("close second session");
        assert_eq!(counters.releases.load(Ordering::Relaxed), 65);
        assert_eq!(counters.closes.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn malformed_packet_seek_keeps_the_existing_lease_releasable() {
        let counters = Arc::new(SessionCounters::default());
        let adapter = SourceNormalizerPacketAdapter::new(
            "dev.vesper.fixture.packet".to_owned(),
            FixturePacketFactory {
                counters: counters.clone(),
                malformed: false,
                packet_size: 3,
            },
        );
        let session_id = open_packet_session(&adapter);
        let lease_id = read_packet_lease(&adapter, session_id);
        let seek_effects = ExportCallEffects::default();

        let error = adapter
            .invoke(ExportOperation::PacketSeek {
                session_id,
                seek_json: b"{",
                effects: &seek_effects,
            })
            .expect_err("malformed seek JSON must fail before lease invalidation");
        assert_eq!(error.status(), status::INVALID_ARGUMENT);

        let release_effects = ExportCallEffects::default();
        adapter
            .invoke(ExportOperation::PacketRelease {
                session_id,
                lease_id,
                effects: &release_effects,
            })
            .expect("the existing lease remains releasable");
        assert_eq!(counters.releases.load(Ordering::Relaxed), 1);
        assert_ne!(read_packet_lease(&adapter, session_id), 0);

        let close_effects = ExportCallEffects::default();
        adapter
            .invoke(ExportOperation::SessionClose {
                session_id,
                effects: &close_effects,
            })
            .expect("close drains the final packet lease");
        assert_eq!(counters.releases.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn malformed_packet_results_release_the_plugin_lease_before_failing() {
        let counters = Arc::new(SessionCounters::default());
        let adapter = SourceNormalizerPacketAdapter::new(
            "dev.vesper.fixture.packet".to_owned(),
            FixturePacketFactory {
                counters: counters.clone(),
                malformed: true,
                packet_size: 3,
            },
        );
        let session_id = open_packet_session(&adapter);
        let effects = ExportCallEffects::default();

        let error = adapter
            .invoke(ExportOperation::PacketRead { session_id })
            .expect_err("malformed packet metadata must fail");
        assert_eq!(error.status(), status::ABI_VIOLATION);
        assert_eq!(counters.releases.load(Ordering::Relaxed), 1);

        adapter
            .invoke(ExportOperation::SessionClose {
                session_id,
                effects: &effects,
            })
            .expect("close malformed packet session");
        assert_eq!(counters.releases.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn oversized_packet_is_released_before_crossing_the_abi() {
        let counters = Arc::new(SessionCounters::default());
        let packet_size =
            usize::try_from(VESPER_MAX_PACKET_BYTES).expect("packet limit fits usize") + 1;
        let adapter = SourceNormalizerPacketAdapter::new(
            "dev.vesper.fixture.packet".to_owned(),
            FixturePacketFactory {
                counters: counters.clone(),
                malformed: false,
                packet_size,
            },
        );
        let session_id = open_packet_session(&adapter);
        let error = adapter
            .invoke(ExportOperation::PacketRead { session_id })
            .expect_err("oversized packet must fail before export");
        assert_eq!(error.status(), status::ABI_VIOLATION);
        assert_eq!(counters.releases.load(Ordering::Relaxed), 1);
    }

    struct RejectingResourceFactory {
        opens: Arc<AtomicUsize>,
    }

    impl SourceNormalizerResourcePluginFactory for RejectingResourceFactory {
        fn name(&self) -> &str {
            "fixture-resource-normalizer"
        }

        fn resource_capabilities(&self) -> SourceNormalizerResourceCapabilities {
            SourceNormalizerResourceCapabilities::default()
        }

        fn open_resource_session(
            &self,
            _config: &SourceNormalizerResourceSessionConfig,
        ) -> Result<Box<dyn SourceNormalizerResourceSession>, SourceNormalizerError> {
            self.opens.fetch_add(1, Ordering::Relaxed);
            Err(SourceNormalizerError::internal(
                "fixture resource factory must not be called",
            ))
        }
    }

    #[test]
    fn source_adapters_reject_sensitive_inputs_before_opening_sessions() {
        let packet_counters = Arc::new(SessionCounters::default());
        let packet_adapter = SourceNormalizerPacketAdapter::new(
            "dev.vesper.fixture.packet".to_owned(),
            FixturePacketFactory {
                counters: packet_counters.clone(),
                malformed: false,
                packet_size: 3,
            },
        );
        let resource_opens = Arc::new(AtomicUsize::new(0));
        let resource_adapter = SourceNormalizerResourceAdapter::new(
            "dev.vesper.fixture.resource".to_owned(),
            RejectingResourceFactory {
                opens: resource_opens.clone(),
            },
        );

        for input in [
            "https://user:secret@example.com/video.mp4",
            "https:user:secret@example.com/video.mp4?token=secret",
            "https://example.com/video.mp4?token=secret",
            "https://example.com/video.mp4#fragment",
        ] {
            let packet = serde_json::to_vec(&packet_config(input)).expect("packet config JSON");
            let error = packet_adapter
                .invoke(ExportOperation::OpenSession {
                    config_json: &packet,
                })
                .expect_err("sensitive packet input must be rejected");
            assert_eq!(error.status(), status::INVALID_ARGUMENT);
            let payload = std::str::from_utf8(error.payload()).expect("UTF-8 error payload");
            assert!(!payload.contains("secret"));

            let resource = SourceNormalizerResourceSessionConfig {
                runtime_profile: "fixture".to_owned(),
                input: input.to_owned(),
                headers: Vec::new(),
                output_root: "/tmp/output".to_owned(),
                cache_policy: Default::default(),
                preferred_route: Some(SourceNormalizerOutputRoute::Fmp4LocalStream),
                startup_timeout_ms: None,
                read_idle_timeout_ms: None,
            };
            let resource = serde_json::to_vec(&resource).expect("resource config JSON");
            let error = resource_adapter
                .invoke(ExportOperation::OpenSession {
                    config_json: &resource,
                })
                .expect_err("sensitive resource input must be rejected");
            assert_eq!(error.status(), status::INVALID_ARGUMENT);
        }

        let mut header_config = packet_config("https://example.com/video.mp4");
        header_config
            .headers
            .push(("Authorization".to_owned(), "Bearer secret".to_owned()));
        let header_config = serde_json::to_vec(&header_config).expect("header config JSON");
        let error = packet_adapter
            .invoke(ExportOperation::OpenSession {
                config_json: &header_config,
            })
            .expect_err("headers must be rejected");
        assert_eq!(error.status(), status::INVALID_ARGUMENT);

        let resource_with_headers = SourceNormalizerResourceSessionConfig {
            runtime_profile: "fixture".to_owned(),
            input: "https://example.com/video.mp4".to_owned(),
            headers: vec![("Authorization".to_owned(), "Bearer secret".to_owned())],
            output_root: "/tmp/output".to_owned(),
            cache_policy: Default::default(),
            preferred_route: Some(SourceNormalizerOutputRoute::Fmp4LocalStream),
            startup_timeout_ms: None,
            read_idle_timeout_ms: None,
        };
        let resource_with_headers =
            serde_json::to_vec(&resource_with_headers).expect("resource header config JSON");
        let error = resource_adapter
            .invoke(ExportOperation::OpenSession {
                config_json: &resource_with_headers,
            })
            .expect_err("resource headers must be rejected");
        assert_eq!(error.status(), status::INVALID_ARGUMENT);
        let payload = std::str::from_utf8(error.payload()).expect("UTF-8 error payload");
        assert!(!payload.contains("secret"));

        for local_path in [
            "/tmp/video?literal#name.mp4",
            "./fixtures/video @ local?.mp4",
            r"C:\media\clip#one.mp4",
            "D:/media/clip?one.mp4",
        ] {
            assert!(validate_source_input(local_path, &[]).is_ok());
        }
        assert_eq!(packet_counters.opens.load(Ordering::Relaxed), 0);
        assert_eq!(resource_opens.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn one_plugin_root_can_export_packet_and_resource_interfaces() {
        let packet_counters = Arc::new(SessionCounters::default());
        let resource_opens = Arc::new(AtomicUsize::new(0));
        let plugin = super::super::PluginBuilder::new("dev.vesper.fixture", "Fixture")
            .and_then(|builder| {
                builder.with_source_normalizer_packet(
                    "dev.vesper.fixture.normalizer",
                    FixturePacketFactory {
                        counters: packet_counters,
                        malformed: false,
                        packet_size: 3,
                    },
                )
            })
            .and_then(|builder| {
                builder.with_source_normalizer_resource(
                    "dev.vesper.fixture.normalizer",
                    RejectingResourceFactory {
                        opens: resource_opens,
                    },
                )
            })
            .and_then(super::super::PluginBuilder::build)
            .expect("dual-interface plugin");

        let kinds: Vec<_> = plugin
            .interfaces
            .iter()
            .map(|interface| interface.kind())
            .collect();
        assert_eq!(
            kinds,
            vec![
                ExportInterfaceKind::SourceNormalizerPacket,
                ExportInterfaceKind::SourceNormalizerResource,
            ]
        );
    }
}
