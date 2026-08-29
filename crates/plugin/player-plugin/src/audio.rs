use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{DecoderFrameFormat, DecoderPcmFrame, DecoderPcmFrameMetadata};

const MAX_AUDIO_PROCESSOR_QUEUE_FRAMES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioProcessorSubmitStatus {
    Accepted,
    Backpressure,
}

/// Pitch behavior requested from an audio processing chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioPitchMode {
    /// Preserve the original pitch while changing playback speed.
    PreservePitch,
    /// Let pitch follow the playback rate.
    FollowRate,
}

impl AudioPitchMode {
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::PreservePitch => "preservePitch",
            Self::FollowRate => "followRate",
        }
    }
}

/// Playback policy applied to an audio processor chain.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AudioPlaybackPolicy {
    pub playback_rate: f32,
    pub pitch_mode: AudioPitchMode,
}

impl AudioPlaybackPolicy {
    pub const fn normal() -> Self {
        Self {
            playback_rate: 1.0,
            pitch_mode: AudioPitchMode::FollowRate,
        }
    }

    pub fn validate(self) -> Result<(), AudioProcessorError> {
        if !self.playback_rate.is_finite() || self.playback_rate <= 0.0 {
            return Err(AudioProcessorError::InvalidPlaybackRate {
                rate: self.playback_rate,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AudioProcessorCapabilities {
    pub accepted_formats: Vec<DecoderFrameFormat>,
    pub output_format: Option<DecoderFrameFormat>,
    pub supports_flush: bool,
    pub max_in_flight_frames: Option<u32>,
    pub playback_rate_min: Option<f32>,
    pub playback_rate_max: Option<f32>,
    pub pitch_modes: Vec<AudioPitchMode>,
}

impl AudioProcessorCapabilities {
    pub fn supports_input_format(&self, format: &DecoderFrameFormat) -> bool {
        self.accepted_formats.is_empty() || self.accepted_formats.iter().any(|item| item == format)
    }

    pub fn supports_playback_policy(&self, policy: AudioPlaybackPolicy) -> bool {
        let rate_supported = self
            .playback_rate_min
            .is_none_or(|minimum| policy.playback_rate >= minimum)
            && self
                .playback_rate_max
                .is_none_or(|maximum| policy.playback_rate <= maximum);
        let pitch_supported =
            self.pitch_modes.is_empty() || self.pitch_modes.contains(&policy.pitch_mode);
        rate_supported && pitch_supported
    }
}

/// Configuration used to open one native audio processor session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioProcessorSessionConfig {
    pub processor_index: usize,
    pub input_metadata: DecoderPcmFrameMetadata,
    pub playback_policy: AudioPlaybackPolicy,
    #[serde(default)]
    pub max_in_flight_frames: Option<u32>,
}

/// Metadata returned after opening one audio processor session.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AudioProcessorSessionInfo {
    pub processor_name: Option<String>,
    pub selected_backend: Option<String>,
    pub output_format: Option<DecoderFrameFormat>,
    pub max_in_flight_frames: Option<u32>,
}

/// Empty success payload used by configure, flush, and close operations.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AudioProcessorOperationStatus {
    pub completed: bool,
}

#[derive(Debug, Error, Clone, PartialEq, Serialize, Deserialize)]
pub enum AudioProcessorError {
    #[error(
        "audio processor queue capacity must be between 1 and {MAX_AUDIO_PROCESSOR_QUEUE_FRAMES}"
    )]
    InvalidCapacity,
    #[error("audio processor chain is closed")]
    Closed,
    #[error("invalid PCM frame: {0}")]
    InvalidPcm(String),
    #[error("invalid playback rate: {rate}")]
    InvalidPlaybackRate { rate: f32 },
    #[error("audio processor does not support the requested playback policy")]
    UnsupportedPlaybackPolicy,
    #[error("audio processor payload codec error: {0}")]
    PayloadCodec(String),
    #[error("audio processor ABI violation: {0}")]
    AbiViolation(String),
    #[error("audio processor backpressure: {0}")]
    Backpressure(String),
    #[error("audio processor timeout: {0}")]
    Timeout(String),
    #[error("audio processor failure: {0}")]
    Processor(String),
}

impl AudioProcessorError {
    pub fn payload_codec(message: impl Into<String>) -> Self {
        Self::PayloadCodec(message.into())
    }

    pub fn abi_violation(message: impl Into<String>) -> Self {
        Self::AbiViolation(message.into())
    }
}

/// Safe factory exported by native Rust audio processor plugins.
pub trait AudioProcessorPluginFactory: Send + Sync {
    fn name(&self) -> &str;

    fn capabilities(&self) -> AudioProcessorCapabilities;

    fn open_session(
        &self,
        config: &AudioProcessorSessionConfig,
    ) -> Result<Box<dyn AudioProcessorSession>, AudioProcessorError>;
}

pub trait AudioProcessorSession: Send {
    fn name(&self) -> &str;

    fn capabilities(&self) -> AudioProcessorCapabilities;

    fn session_info(&self) -> AudioProcessorSessionInfo {
        let capabilities = self.capabilities();
        AudioProcessorSessionInfo {
            processor_name: Some(self.name().to_owned()),
            selected_backend: None,
            output_format: capabilities.output_format,
            max_in_flight_frames: capabilities.max_in_flight_frames,
        }
    }

    fn configure(&mut self, _policy: AudioPlaybackPolicy) -> Result<(), AudioProcessorError> {
        Ok(())
    }

    /// Processes one PCM frame while preserving the host-owned PTS and discontinuity marker.
    fn process(&mut self, frame: DecoderPcmFrame) -> Result<DecoderPcmFrame, AudioProcessorError>;

    fn flush(&mut self) -> Result<(), AudioProcessorError>;

    fn close(&mut self) -> Result<(), AudioProcessorError>;
}

pub struct AudioProcessorChain {
    capacity: usize,
    pending: VecDeque<DecoderPcmFrame>,
    processors: Vec<Box<dyn AudioProcessorSession>>,
    playback_policy: AudioPlaybackPolicy,
    closed: bool,
}

impl AudioProcessorChain {
    pub fn new(capacity: usize) -> Result<Self, AudioProcessorError> {
        if capacity == 0 || capacity > MAX_AUDIO_PROCESSOR_QUEUE_FRAMES {
            return Err(AudioProcessorError::InvalidCapacity);
        }
        Ok(Self {
            capacity,
            pending: VecDeque::with_capacity(capacity),
            processors: Vec::new(),
            playback_policy: AudioPlaybackPolicy::normal(),
            closed: false,
        })
    }

    pub fn with_processors(
        capacity: usize,
        processors: Vec<Box<dyn AudioProcessorSession>>,
    ) -> Result<Self, AudioProcessorError> {
        let mut chain = Self::new(capacity)?;
        chain.processors = processors;
        Ok(chain)
    }

    pub fn playback_policy(&self) -> AudioPlaybackPolicy {
        self.playback_policy
    }

    pub fn set_playback_policy(
        &mut self,
        policy: AudioPlaybackPolicy,
    ) -> Result<(), AudioProcessorError> {
        if self.closed {
            return Err(AudioProcessorError::Closed);
        }
        policy.validate()?;
        if self
            .processors
            .iter()
            .any(|processor| !processor.capabilities().supports_playback_policy(policy))
        {
            return Err(AudioProcessorError::UnsupportedPlaybackPolicy);
        }
        for processor in &mut self.processors {
            processor.configure(policy)?;
        }
        self.playback_policy = policy;
        Ok(())
    }

    pub fn submit(
        &mut self,
        frame: DecoderPcmFrame,
    ) -> Result<AudioProcessorSubmitStatus, AudioProcessorError> {
        if self.closed {
            return Err(AudioProcessorError::Closed);
        }
        frame
            .validate()
            .map_err(|error| AudioProcessorError::InvalidPcm(error.to_string()))?;
        if self.pending.len() >= self.capacity {
            return Ok(AudioProcessorSubmitStatus::Backpressure);
        }
        let mut processed = frame;
        for processor in &mut self.processors {
            let input_pts_us = processed.metadata.pts_us;
            let input_discontinuity = processed.metadata.discontinuity;
            let output = processor.process(processed)?;
            output.validate().map_err(|error| {
                AudioProcessorError::abi_violation(format!(
                    "processor returned invalid PCM: {error}"
                ))
            })?;
            if output.metadata.pts_us != input_pts_us {
                return Err(AudioProcessorError::abi_violation(
                    "processor changed the host-owned PCM presentation timestamp",
                ));
            }
            if output.metadata.discontinuity != input_discontinuity {
                return Err(AudioProcessorError::abi_violation(
                    "processor changed the host-owned PCM discontinuity marker",
                ));
            }
            processed = output;
        }
        self.pending.push_back(processed);
        Ok(AudioProcessorSubmitStatus::Accepted)
    }

    pub fn receive(&mut self) -> Result<Option<DecoderPcmFrame>, AudioProcessorError> {
        if self.closed {
            return Err(AudioProcessorError::Closed);
        }
        Ok(self.pending.pop_front())
    }

    pub fn flush(&mut self) -> Result<(), AudioProcessorError> {
        if self.closed {
            return Err(AudioProcessorError::Closed);
        }
        self.pending.clear();
        for processor in &mut self.processors {
            processor.flush()?;
        }
        Ok(())
    }

    pub fn close(&mut self) -> Result<(), AudioProcessorError> {
        if !self.closed {
            self.pending.clear();
            let mut first_error = None;
            for processor in self.processors.iter_mut().rev() {
                if let Err(error) = processor.close()
                    && first_error.is_none()
                {
                    first_error = Some(error);
                }
            }
            self.closed = true;
            if let Some(error) = first_error {
                return Err(error);
            }
        }
        Ok(())
    }

    pub fn queue_depth(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AudioPitchMode, AudioPlaybackPolicy, AudioProcessorCapabilities, AudioProcessorChain,
        AudioProcessorError, AudioProcessorSession, AudioProcessorSubmitStatus,
    };
    use crate::{
        DecoderFrameFormat, DecoderPcmFrame, DecoderPcmFrameMetadata, DecoderPcmSampleLayout,
    };

    fn frame() -> DecoderPcmFrame {
        let metadata = DecoderPcmFrameMetadata::audio(
            "aac",
            DecoderFrameFormat::F32,
            48_000,
            2,
            DecoderPcmSampleLayout::Interleaved,
            2,
        );
        DecoderPcmFrame {
            metadata,
            data: vec![0; 16],
        }
    }

    #[test]
    fn audio_chain_is_bounded_and_flushes_pending_output() {
        let mut chain = AudioProcessorChain::new(1).expect("bounded chain");
        assert_eq!(
            chain.submit(frame()).unwrap(),
            AudioProcessorSubmitStatus::Accepted
        );
        assert_eq!(
            chain.submit(frame()).unwrap(),
            AudioProcessorSubmitStatus::Backpressure
        );
        chain.flush().expect("flush chain");
        assert!(chain.receive().unwrap().is_none());
    }

    struct AddOneProcessor;

    impl AudioProcessorSession for AddOneProcessor {
        fn name(&self) -> &str {
            "add-one"
        }

        fn capabilities(&self) -> AudioProcessorCapabilities {
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

        fn process(
            &mut self,
            mut frame: DecoderPcmFrame,
        ) -> Result<DecoderPcmFrame, AudioProcessorError> {
            frame.data[0] = frame.data[0].saturating_add(1);
            Ok(frame)
        }

        fn flush(&mut self) -> Result<(), AudioProcessorError> {
            Ok(())
        }

        fn close(&mut self) -> Result<(), AudioProcessorError> {
            Ok(())
        }
    }

    #[test]
    fn audio_chain_applies_processors_in_linear_order() {
        let mut chain = AudioProcessorChain::with_processors(
            2,
            vec![Box::new(AddOneProcessor), Box::new(AddOneProcessor)],
        )
        .expect("processor chain");
        let mut input = frame();
        input.data[0] = 0;
        assert_eq!(
            chain.submit(input).unwrap(),
            AudioProcessorSubmitStatus::Accepted
        );
        assert_eq!(chain.receive().unwrap().expect("output").data[0], 2);
        chain.close().expect("close chain");
    }

    #[test]
    fn audio_chain_rejects_invalid_or_unsupported_playback_policy() {
        let mut chain = AudioProcessorChain::with_processors(2, vec![Box::new(AddOneProcessor)])
            .expect("processor chain");
        assert!(matches!(
            chain.set_playback_policy(AudioPlaybackPolicy {
                playback_rate: 0.0,
                pitch_mode: AudioPitchMode::FollowRate,
            }),
            Err(AudioProcessorError::InvalidPlaybackRate { .. })
        ));
        assert_eq!(chain.playback_policy(), AudioPlaybackPolicy::normal());

        let mut chain = AudioProcessorChain::new(2).expect("processor chain");
        let mut processor = AddOneProcessor;
        processor
            .configure(AudioPlaybackPolicy::normal())
            .expect("default policy");
        chain
            .set_playback_policy(AudioPlaybackPolicy {
                playback_rate: 1.5,
                pitch_mode: AudioPitchMode::PreservePitch,
            })
            .expect("empty chain accepts policy");
    }

    struct FollowRateOnlyProcessor;

    impl AudioProcessorSession for FollowRateOnlyProcessor {
        fn name(&self) -> &str {
            "follow-rate-only"
        }

        fn capabilities(&self) -> AudioProcessorCapabilities {
            AudioProcessorCapabilities {
                accepted_formats: vec![DecoderFrameFormat::F32],
                output_format: Some(DecoderFrameFormat::F32),
                supports_flush: true,
                max_in_flight_frames: Some(1),
                playback_rate_min: Some(0.5),
                playback_rate_max: Some(2.0),
                pitch_modes: vec![AudioPitchMode::FollowRate],
            }
        }

        fn process(
            &mut self,
            frame: DecoderPcmFrame,
        ) -> Result<DecoderPcmFrame, AudioProcessorError> {
            Ok(frame)
        }

        fn flush(&mut self) -> Result<(), AudioProcessorError> {
            Ok(())
        }

        fn close(&mut self) -> Result<(), AudioProcessorError> {
            Ok(())
        }
    }

    #[test]
    fn audio_chain_rejects_rate_and_pitch_modes_outside_processor_capabilities() {
        let mut chain =
            AudioProcessorChain::with_processors(2, vec![Box::new(FollowRateOnlyProcessor)])
                .expect("processor chain");
        assert_eq!(
            chain.set_playback_policy(AudioPlaybackPolicy {
                playback_rate: 2.5,
                pitch_mode: AudioPitchMode::FollowRate,
            }),
            Err(AudioProcessorError::UnsupportedPlaybackPolicy)
        );
        assert_eq!(
            chain.set_playback_policy(AudioPlaybackPolicy {
                playback_rate: 1.5,
                pitch_mode: AudioPitchMode::PreservePitch,
            }),
            Err(AudioProcessorError::UnsupportedPlaybackPolicy)
        );
    }

    struct TimestampMutatingProcessor {
        pts_us: Option<i64>,
    }

    impl AudioProcessorSession for TimestampMutatingProcessor {
        fn name(&self) -> &str {
            "timestamp-mutator"
        }

        fn capabilities(&self) -> AudioProcessorCapabilities {
            AudioProcessorCapabilities::default()
        }

        fn process(
            &mut self,
            mut frame: DecoderPcmFrame,
        ) -> Result<DecoderPcmFrame, AudioProcessorError> {
            frame.metadata.pts_us = self.pts_us;
            Ok(frame)
        }

        fn flush(&mut self) -> Result<(), AudioProcessorError> {
            Ok(())
        }

        fn close(&mut self) -> Result<(), AudioProcessorError> {
            Ok(())
        }
    }

    #[test]
    fn audio_chain_rejects_negative_or_mutated_host_owned_timestamps() {
        for mutated_pts in [Some(-1), Some(1_001)] {
            let mut input = frame();
            input.metadata.pts_us = Some(1_000);
            let mut chain = AudioProcessorChain::with_processors(
                1,
                vec![Box::new(TimestampMutatingProcessor {
                    pts_us: mutated_pts,
                })],
            )
            .expect("processor chain");

            assert!(matches!(
                chain.submit(input),
                Err(AudioProcessorError::AbiViolation(message))
                    if message.contains("presentation timestamp")
            ));
            assert_eq!(chain.queue_depth(), 0);
        }
    }
}
