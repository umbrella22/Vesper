#![deny(unsafe_code)]

use std::mem::size_of;

use player_plugin::{
    AudioPitchMode, AudioPlaybackPolicy, AudioProcessorCapabilities, AudioProcessorChain,
    AudioProcessorError, AudioProcessorPluginFactory, AudioProcessorSession,
    AudioProcessorSessionConfig, AudioProcessorSessionInfo, DecoderFrameFormat, DecoderPcmFrame,
    DecoderPcmSampleLayout, Plugin, PluginBuildError,
};

#[cfg(test)]
use player_plugin::DecoderPcmFrameMetadata;

const PLUGIN_ID: &str = "dev.vesper.audio-processor-diagnostic";
const INSTANCE_ID: &str = "dev.vesper.audio-processor-diagnostic.audio";
const PLUGIN_NAME: &str = "Vesper Audio Processor Diagnostic";

/// Builds a bounded diagnostic chain with one deterministic PCM processor.
pub fn diagnostic_chain() -> Result<AudioProcessorChain, AudioProcessorError> {
    AudioProcessorChain::with_processors(2, vec![Box::new(GainProcessor::default())])
}

#[derive(Debug, Default)]
struct DiagnosticAudioProcessorFactory;

impl AudioProcessorPluginFactory for DiagnosticAudioProcessorFactory {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn capabilities(&self) -> AudioProcessorCapabilities {
        diagnostic_capabilities()
    }

    fn open_session(
        &self,
        config: &AudioProcessorSessionConfig,
    ) -> Result<Box<dyn AudioProcessorSession>, AudioProcessorError> {
        if !diagnostic_capabilities().supports_input_format(&config.input_metadata.format)
            || config.input_metadata.sample_layout != DecoderPcmSampleLayout::Interleaved
            || !diagnostic_capabilities().supports_playback_policy(config.playback_policy)
        {
            return Err(AudioProcessorError::UnsupportedPlaybackPolicy);
        }
        Ok(Box::new(GainProcessor {
            policy: config.playback_policy,
        }))
    }
}

struct GainProcessor {
    policy: AudioPlaybackPolicy,
}

impl Default for GainProcessor {
    fn default() -> Self {
        Self {
            policy: AudioPlaybackPolicy::normal(),
        }
    }
}

impl AudioProcessorSession for GainProcessor {
    fn name(&self) -> &str {
        "vesper-audio-processor-diagnostic.gain"
    }

    fn capabilities(&self) -> AudioProcessorCapabilities {
        diagnostic_capabilities()
    }

    fn session_info(&self) -> AudioProcessorSessionInfo {
        AudioProcessorSessionInfo {
            processor_name: Some("vesper-audio-processor-diagnostic.gain".to_owned()),
            selected_backend: Some(match self.policy.pitch_mode {
                AudioPitchMode::PreservePitch => "wsola".to_owned(),
                AudioPitchMode::FollowRate => "linear-resample".to_owned(),
            }),
            output_format: Some(DecoderFrameFormat::F32),
            max_in_flight_frames: Some(2),
        }
    }

    fn configure(&mut self, policy: AudioPlaybackPolicy) -> Result<(), AudioProcessorError> {
        policy.validate()?;
        if !diagnostic_capabilities().supports_playback_policy(policy) {
            return Err(AudioProcessorError::UnsupportedPlaybackPolicy);
        }
        self.policy = policy;
        Ok(())
    }

    fn process(
        &mut self,
        mut frame: DecoderPcmFrame,
    ) -> Result<DecoderPcmFrame, AudioProcessorError> {
        frame
            .validate()
            .map_err(|error| AudioProcessorError::InvalidPcm(error.to_string()))?;
        if frame.metadata.format != DecoderFrameFormat::F32
            || frame.metadata.sample_layout != DecoderPcmSampleLayout::Interleaved
        {
            return Err(AudioProcessorError::InvalidPcm(
                "diagnostic processor requires interleaved F32 PCM".to_owned(),
            ));
        }
        let samples = decode_f32(&frame.data);
        let mut output = match self.policy.pitch_mode {
            AudioPitchMode::PreservePitch => wsola::stretch(
                &samples,
                frame.metadata.sample_rate,
                frame.metadata.channels,
                self.policy.playback_rate,
            )
            .map_err(|error| AudioProcessorError::Processor(error.to_string()))?,
            AudioPitchMode::FollowRate => {
                resample_follow_rate(&samples, frame.metadata.channels, self.policy.playback_rate)?
            }
        };
        let input_frames = usize::try_from(frame.metadata.frame_count).map_err(|_| {
            AudioProcessorError::Processor("input frame count does not fit usize".to_owned())
        })?;
        let target_frames =
            ((input_frames as f64 / f64::from(self.policy.playback_rate)).round() as usize).max(1);
        let target_samples = target_frames
            .checked_mul(usize::from(frame.metadata.channels))
            .ok_or_else(|| {
                AudioProcessorError::Processor("processed sample count overflows usize".to_owned())
            })?;
        output.resize(target_samples, 0.0);
        output.truncate(target_samples);
        for sample in &mut output {
            *sample *= 0.5;
        }
        let channels = usize::from(frame.metadata.channels);
        let output_frames = output.len() / channels;
        frame.metadata.frame_count = u32::try_from(output_frames).map_err(|_| {
            AudioProcessorError::Processor("processed frame count exceeds u32".to_owned())
        })?;
        if frame.metadata.frame_count == 0 {
            return Err(AudioProcessorError::Processor(
                "processor produced an empty PCM frame".to_owned(),
            ));
        }
        frame.metadata.duration_us = i64::try_from(output_frames)
            .ok()
            .and_then(|frames| frames.checked_mul(1_000_000))
            .map(|micros| micros / i64::from(frame.metadata.sample_rate));
        frame.data = encode_f32(&output);
        Ok(frame)
    }

    fn flush(&mut self) -> Result<(), AudioProcessorError> {
        Ok(())
    }

    fn close(&mut self) -> Result<(), AudioProcessorError> {
        Ok(())
    }
}

fn diagnostic_capabilities() -> AudioProcessorCapabilities {
    AudioProcessorCapabilities {
        accepted_formats: vec![DecoderFrameFormat::F32],
        output_format: Some(DecoderFrameFormat::F32),
        supports_flush: true,
        max_in_flight_frames: Some(2),
        playback_rate_min: Some(0.5),
        playback_rate_max: Some(3.0),
        pitch_modes: vec![AudioPitchMode::PreservePitch, AudioPitchMode::FollowRate],
    }
}

fn decode_f32(data: &[u8]) -> Vec<f32> {
    let (samples, remainder) = data.as_chunks::<{ size_of::<f32>() }>();
    debug_assert!(remainder.is_empty());
    samples
        .iter()
        .map(|sample| f32::from_le_bytes(*sample))
        .collect()
}

fn encode_f32(samples: &[f32]) -> Vec<u8> {
    let mut data = Vec::with_capacity(std::mem::size_of_val(samples));
    for sample in samples {
        data.extend_from_slice(&sample.to_le_bytes());
    }
    data
}

fn resample_follow_rate(
    samples: &[f32],
    channels: u16,
    rate: f32,
) -> Result<Vec<f32>, AudioProcessorError> {
    let channels = usize::from(channels);
    if channels == 0 || !samples.len().is_multiple_of(channels) {
        return Err(AudioProcessorError::InvalidPcm(
            "interleaved PCM sample count is not channel-aligned".to_owned(),
        ));
    }
    let input_frames = samples.len() / channels;
    let output_frames = ((input_frames as f64 / f64::from(rate)).round() as usize).max(1);
    let mut output = Vec::with_capacity(output_frames * channels);
    for output_index in 0..output_frames {
        let source_position = output_index as f64 * f64::from(rate);
        let left = (source_position.floor() as usize).min(input_frames - 1);
        let right = left.saturating_add(1).min(input_frames - 1);
        let fraction = (source_position - left as f64) as f32;
        for channel in 0..channels {
            let left_sample = samples[left * channels + channel];
            let right_sample = samples[right * channels + channel];
            output.push(left_sample + (right_sample - left_sample) * fraction);
        }
    }
    Ok(output)
}

#[player_plugin::export]
fn diagnostic_audio_processor_plugin() -> Result<Plugin, PluginBuildError> {
    Plugin::builder(PLUGIN_ID, PLUGIN_NAME)?
        .with_audio_processor(INSTANCE_ID, DiagnosticAudioProcessorFactory)?
        .build()
}

#[cfg(test)]
fn fixture_frame() -> DecoderPcmFrame {
    let metadata = DecoderPcmFrameMetadata::audio(
        "diagnostic-pcm",
        DecoderFrameFormat::F32,
        48_000,
        2,
        DecoderPcmSampleLayout::Interleaved,
        2,
    );
    let mut data = Vec::with_capacity(16);
    for value in [1.0_f32, -1.0, 0.5, -0.5] {
        data.extend_from_slice(&value.to_le_bytes());
    }
    DecoderPcmFrame { metadata, data }
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use super::*;
    use player_plugin::{AudioPlaybackPolicy, AudioProcessorSubmitStatus};

    const SAMPLE_RATE: u32 = 48_000;
    const TEST_FREQUENCY_HZ: f32 = 440.0;

    fn sine_frame(frame_count: u32) -> DecoderPcmFrame {
        let metadata = DecoderPcmFrameMetadata::audio(
            "diagnostic-sine",
            DecoderFrameFormat::F32,
            SAMPLE_RATE,
            1,
            DecoderPcmSampleLayout::Interleaved,
            frame_count,
        );
        let mut data = Vec::with_capacity(frame_count as usize * size_of::<f32>());
        for index in 0..frame_count {
            let phase =
                std::f32::consts::TAU * TEST_FREQUENCY_HZ * index as f32 / SAMPLE_RATE as f32;
            data.extend_from_slice(&phase.sin().to_le_bytes());
        }
        DecoderPcmFrame { metadata, data }
    }

    fn f32_samples(frame: &DecoderPcmFrame) -> Vec<f32> {
        frame
            .data
            .as_chunks::<{ size_of::<f32>() }>()
            .0
            .iter()
            .map(|sample| f32::from_le_bytes(*sample))
            .collect()
    }

    fn positive_zero_crossing_frequency(samples: &[f32], sample_rate: u32) -> f32 {
        let crossings = samples
            .windows(2)
            .filter(|window| window[0] <= 0.0 && window[1] > 0.0)
            .count();
        crossings as f32 * sample_rate as f32 / samples.len() as f32
    }

    fn process_sine(mode: AudioPitchMode, rate: f32) -> DecoderPcmFrame {
        let mut chain = diagnostic_chain().expect("diagnostic chain");
        chain
            .set_playback_policy(AudioPlaybackPolicy {
                playback_rate: rate,
                pitch_mode: mode,
            })
            .expect("supported playback policy");
        chain
            .submit(sine_frame(SAMPLE_RATE))
            .expect("submit sine PCM");
        chain
            .receive()
            .expect("receive PCM")
            .expect("processed PCM")
    }

    #[test]
    fn bounded_chain_processes_valid_pcm_and_preserves_timing_metadata() {
        let mut chain = diagnostic_chain().expect("diagnostic chain");
        chain
            .set_playback_policy(AudioPlaybackPolicy {
                playback_rate: 1.25,
                pitch_mode: AudioPitchMode::FollowRate,
            })
            .expect("supported playback policy");

        let mut input = fixture_frame();
        input.metadata.pts_us = Some(1_000);
        input.metadata.duration_us = Some(42);
        input.metadata.discontinuity = true;
        assert_eq!(
            chain.submit(input).expect("submit PCM"),
            AudioProcessorSubmitStatus::Accepted
        );

        let output = chain
            .receive()
            .expect("receive PCM")
            .expect("processed PCM");
        assert_eq!(output.metadata.pts_us, Some(1_000));
        assert!(
            output
                .metadata
                .duration_us
                .is_some_and(|duration| duration > 0)
        );
        assert!(output.metadata.discontinuity);
        assert_eq!(
            f32::from_le_bytes(output.data[0..4].try_into().expect("sample")),
            0.5
        );
    }

    #[test]
    fn policy_bounds_and_queue_backpressure_are_typed() {
        let mut chain = diagnostic_chain().expect("diagnostic chain");
        let unsupported = chain.set_playback_policy(AudioPlaybackPolicy {
            playback_rate: 4.0,
            pitch_mode: AudioPitchMode::FollowRate,
        });
        assert_eq!(
            unsupported,
            Err(AudioProcessorError::UnsupportedPlaybackPolicy)
        );

        assert_eq!(
            chain.submit(fixture_frame()).expect("first submit"),
            AudioProcessorSubmitStatus::Accepted
        );
        assert_eq!(
            chain.submit(fixture_frame()).expect("second submit"),
            AudioProcessorSubmitStatus::Accepted
        );
        assert_eq!(
            chain.submit(fixture_frame()).expect("bounded submit"),
            AudioProcessorSubmitStatus::Backpressure
        );
    }

    #[test]
    fn exports_plugin_entry() {
        assert!(!vesper_plugin_entry().is_null());
    }

    #[test]
    fn flush_and_close_are_bounded_and_idempotent() {
        let mut chain = diagnostic_chain().expect("diagnostic chain");
        chain.submit(fixture_frame()).expect("submit PCM");
        chain.flush().expect("flush chain");
        assert_eq!(chain.queue_depth(), 0);
        chain.close().expect("close chain");
        chain.close().expect("idempotent close");
        assert_eq!(
            chain.submit(fixture_frame()),
            Err(AudioProcessorError::Closed)
        );
    }

    #[test]
    fn rewrite_red_preserve_pitch_two_x_halves_duration_without_shifting_fundamental() {
        let output = process_sine(AudioPitchMode::PreservePitch, 2.0);
        let samples = f32_samples(&output);
        assert!(
            (23_760..=24_240).contains(&samples.len()),
            "2x PreservePitch must return about 24000 frames, got {}",
            samples.len()
        );
        let frequency = positive_zero_crossing_frequency(&samples, SAMPLE_RATE);
        assert!(
            (420.0..=460.0).contains(&frequency),
            "2x PreservePitch must keep the 440 Hz fundamental, got {frequency:.2} Hz"
        );
    }

    #[test]
    fn rewrite_red_follow_rate_two_x_halves_duration_and_doubles_fundamental() {
        let output = process_sine(AudioPitchMode::FollowRate, 2.0);
        let samples = f32_samples(&output);
        assert!(
            (23_760..=24_240).contains(&samples.len()),
            "2x FollowRate must return about 24000 frames, got {}",
            samples.len()
        );
        let frequency = positive_zero_crossing_frequency(&samples, SAMPLE_RATE);
        assert!(
            (840.0..=920.0).contains(&frequency),
            "2x FollowRate must shift the 440 Hz fundamental to about 880 Hz, got {frequency:.2} Hz"
        );
    }
}
