use std::mem::size_of;
use std::num::NonZeroU16;
use std::sync::Arc;
use std::time::Duration;

use player_plugin::author::*;
use player_plugin::{AudioPitchMode, AudioPlaybackPolicy};
use player_plugin_loader::native::{DEFAULT_AUDIO_CALL_BUDGET, NativePlugin};

use super::{
    AudioStreamError, DecodedAudioChunk, PlayerError, PlayerErrorCode, PlayerResult,
    PlayerRuntimeOptions,
};

const MAX_BLOCK_FRAMES: u16 = 8192;
const MAX_DSP_STEPS: usize = 1024;

pub(super) fn load_factory(
    options: &PlayerRuntimeOptions,
) -> PlayerResult<Option<Arc<dyn AudioProcessorFactory>>> {
    if options.audio_processor_library_paths.is_empty() {
        return Ok(None);
    }
    let [path] = options.audio_processor_library_paths.as_slice() else {
        return Err(PlayerError::new(
            PlayerErrorCode::InvalidArgument,
            "the desktop PCM route accepts one timing-owning audio processor",
        ));
    };
    options
        .validate_native_plugin_loading_policy("desktop audio processor")
        .map_err(|error| PlayerError::new(PlayerErrorCode::InvalidArgument, error.to_string()))?;
    // SAFETY: the host explicitly authorized its local development plugin path.
    // The checked loader allows only the native entry point for audio.
    let plugin = unsafe { NativePlugin::load(path) }
        .map_err(|error| PlayerError::new(PlayerErrorCode::BackendFailure, error.to_string()))?;
    let factory = plugin
        .audio_processor(DEFAULT_AUDIO_CALL_BUDGET)
        .map_err(|error| PlayerError::new(PlayerErrorCode::Unsupported, error.to_string()))?;
    tracing::info!(
        plugin_id = plugin.id(),
        "desktop PCM route selected a native audio processor plugin"
    );
    Ok(Some(factory))
}

pub(super) struct DesktopAudioProcessor {
    processor: Box<dyn AudioProcessor>,
    input_limit: usize,
    channels: NonZeroU16,
    sample_rate: u32,
    /// The desktop worker owns one CPAL generation. On seek or rate change its
    /// replacement gets a new processor and stale writes are rejected by CPAL.
    epoch: u64,
    rate: f32,
    buffered: bool,
    scratch: Vec<f32>,
    next_pts: Duration,
}

impl DesktopAudioProcessor {
    pub(super) fn open(
        factory: &dyn AudioProcessorFactory,
        sample_rate: u32,
        channels: u16,
        policy: AudioPlaybackPolicy,
        epoch: u64,
        position: Duration,
    ) -> Result<Self, AudioStreamError> {
        let channels = NonZeroU16::new(channels).ok_or(AudioStreamError::InvalidOutputChannels)?;
        let input = VesperAudioFormat {
            struct_size: size_of::<VesperAudioFormat>() as u32,
            sample_format: VESPER_SAMPLE_F32_INTERLEAVED,
            format_id: 1,
            sample_rate,
            channels: channels.get(),
            max_block_frames: MAX_BLOCK_FRAMES,
            channel_layout: 0,
        };
        let OpenAudioProcessor {
            output_format,
            mut processor,
        } = factory.open(input, &[]).map_err(dsp_error)?;
        if output_format.sample_rate != sample_rate
            || output_format.channels != channels.get()
            || output_format.sample_format != input.sample_format
            || output_format.max_block_frames == 0
        {
            return Err(dsp_failure(
                "audio processor layout must match the CPAL output",
            ));
        }
        let modes = processor.capabilities();
        let buffered = modes.buffered && (policy.playback_rate != 1.0 || !modes.fixed_block);
        if !buffered && !modes.fixed_block {
            return Err(dsp_failure("audio processor has no usable PCM mode"));
        }
        if !buffered && output_format.format_id != input.format_id {
            return Err(dsp_failure(
                "in-place audio processing requires the input format identifier",
            ));
        }
        let pitch = match policy.pitch_mode {
            AudioPitchMode::PreservePitch => VESPER_PITCH_PRESERVE,
            _ => VESPER_PITCH_FOLLOW_RATE,
        };
        processor
            .set_playback_policy(
                if buffered { policy.playback_rate } else { 1.0 },
                pitch,
                epoch,
            )
            .map_err(dsp_error)?;
        tracing::debug!(
            buffered,
            epoch,
            playback_rate = policy.playback_rate,
            "opened desktop native audio session"
        );
        Ok(Self {
            processor,
            input_limit: usize::from(input.max_block_frames.min(output_format.max_block_frames)),
            channels,
            sample_rate,
            epoch,
            rate: policy.playback_rate,
            buffered,
            scratch: if buffered {
                vec![
                    0.0;
                    usize::from(output_format.max_block_frames.min(MAX_BLOCK_FRAMES))
                        * usize::from(channels.get())
                ]
            } else {
                Vec::new()
            },
            next_pts: position,
        })
    }

    pub(super) fn process(
        &mut self,
        mut chunk: DecodedAudioChunk,
        mut emit: impl FnMut(DecodedAudioChunk) -> Result<bool, AudioStreamError>,
    ) -> Result<bool, AudioStreamError> {
        chunk
            .validate()
            .map_err(|error| AudioStreamError::InvalidChunk {
                message: error.to_string(),
            })?;
        if chunk.sample_rate != self.sample_rate || chunk.channels != self.channels.get() {
            return Err(dsp_failure(
                "decoded PCM differs from the negotiated format",
            ));
        }
        if chunk.discontinuity {
            self.processor.flush().map_err(dsp_error)?;
            self.next_pts = chunk.presentation_time.unwrap_or(self.next_pts);
        }
        let origin = chunk.presentation_time.unwrap_or(self.next_pts);
        let channels = usize::from(self.channels.get());
        if !self.buffered {
            for (index, samples) in chunk
                .samples
                .chunks_mut(self.input_limit * channels)
                .enumerate()
            {
                let pts = origin.saturating_add(self.media_duration(index * self.input_limit));
                let time = self.time(pts)?;
                let audio =
                    AudioBufferMut::interleaved(self.channels, samples).map_err(dsp_error)?;
                if !samples_are_empty(&audio) {
                    self.processor
                        .process_block(AudioBlock::InPlace { audio, time })
                        .map_err(dsp_error)?;
                }
            }
            self.next_pts = origin.saturating_add(chunk.duration);
            return emit(chunk);
        }

        let mut offset = 0;
        for _ in 0..MAX_DSP_STEPS {
            let mut consumed = false;
            if offset < chunk.samples.len() {
                let end = chunk
                    .samples
                    .len()
                    .min(offset + self.input_limit * channels);
                let pts =
                    origin.saturating_add(frame_duration(offset / channels, self.sample_rate, 1.0));
                let input = AudioBufferView::interleaved(
                    self.channels,
                    &chunk.samples[offset..end],
                    self.time(pts)?,
                )
                .map_err(dsp_error)?;
                match self.processor.push(Some(input)).map_err(dsp_error)? {
                    AudioPush::Consumed(frames)
                        if frames > 0 && frames as usize <= input.frames() =>
                    {
                        offset += frames as usize * channels;
                        consumed = true;
                    }
                    AudioPush::Backpressure => {}
                    _ => return Err(dsp_failure("invalid buffered audio consumption")),
                }
            }
            match self.pull(&mut emit)? {
                Pull::Stopped => return Ok(false),
                Pull::Produced | Pull::TryAgain => {}
                Pull::NeedInput if offset == chunk.samples.len() => return Ok(true),
                Pull::NeedInput if consumed => {}
                Pull::NeedInput => return Err(dsp_failure("audio processor made no progress")),
                Pull::Eof => return Err(dsp_failure("audio processor signaled EOF before EOS")),
            }
        }
        Err(dsp_failure(
            "audio processor exceeded its bounded work per decoded block",
        ))
    }

    pub(super) fn finish(
        &mut self,
        mut emit: impl FnMut(DecodedAudioChunk) -> Result<bool, AudioStreamError>,
    ) -> Result<(), AudioStreamError> {
        if !self.buffered {
            return Ok(());
        }
        let mut accepted = false;
        for _ in 0..MAX_DSP_STEPS {
            if !accepted {
                accepted = match self.processor.push(None).map_err(dsp_error)? {
                    AudioPush::EosAccepted => true,
                    AudioPush::Backpressure => false,
                    _ => {
                        return Err(dsp_failure(
                            "audio processor returned an input count during EOS",
                        ));
                    }
                };
            }
            match self.pull(&mut emit)? {
                Pull::Eof if accepted => return Ok(()),
                Pull::Stopped => return Ok(()),
                Pull::Produced | Pull::TryAgain => {}
                _ => {
                    return Err(dsp_failure(
                        "audio processor stopped progressing during EOS",
                    ));
                }
            }
        }
        Err(dsp_failure("audio processor exceeded its bounded EOS work"))
    }

    fn pull(
        &mut self,
        emit: &mut impl FnMut(DecodedAudioChunk) -> Result<bool, AudioStreamError>,
    ) -> Result<Pull, AudioStreamError> {
        let output =
            AudioBufferMut::interleaved(self.channels, &mut self.scratch).map_err(dsp_error)?;
        match self.processor.pull(output).map_err(dsp_error)? {
            AudioPull::Produced { frames, time } => {
                let samples = (frames as usize).saturating_mul(usize::from(self.channels.get()));
                if samples == 0 || samples > self.scratch.len() || time.rate_epoch != self.epoch {
                    return Err(dsp_failure(
                        "audio processor returned invalid frames or a stale generation",
                    ));
                }
                let output_pts = u64::try_from(time.pts_us).ok().map(Duration::from_micros);
                let pts = output_pts.map_or(self.next_pts, |value| value.max(self.next_pts));
                let duration = self.media_duration(frames as usize);
                self.next_pts = pts.saturating_add(duration);
                let accepted = emit(DecodedAudioChunk {
                    presentation_time: Some(pts),
                    duration,
                    sample_rate: self.sample_rate,
                    channels: self.channels.get(),
                    samples: self.scratch[..samples].to_vec(),
                    discontinuity: false,
                })?;
                Ok(if accepted {
                    Pull::Produced
                } else {
                    Pull::Stopped
                })
            }
            AudioPull::Eof => Ok(Pull::Eof),
            AudioPull::NeedInput => Ok(Pull::NeedInput),
            AudioPull::TryAgain => Ok(Pull::TryAgain),
        }
    }

    fn time(&self, pts: Duration) -> Result<AudioTime, AudioStreamError> {
        let pts_us = i64::try_from(pts.as_micros())
            .map_err(|_| dsp_failure("audio PTS exceeds the ABI timestamp range"))?;
        Ok(AudioTime {
            pts_us,
            rate_epoch: self.epoch,
        })
    }

    fn media_duration(&self, frames: usize) -> Duration {
        frame_duration(frames, self.sample_rate, self.rate)
    }
}

fn frame_duration(frames: usize, sample_rate: u32, rate: f32) -> Duration {
    Duration::from_secs_f64(frames as f64 * f64::from(rate) / f64::from(sample_rate))
}

fn samples_are_empty(block: &AudioBufferMut<'_>) -> bool {
    block.frames() == 0
}

enum Pull {
    Produced,
    Stopped,
    NeedInput,
    TryAgain,
    Eof,
}

fn dsp_failure(message: &str) -> AudioStreamError {
    AudioStreamError::Processor {
        message: message.to_owned(),
    }
}
fn dsp_error(error: PluginError) -> AudioStreamError {
    AudioStreamError::Processor {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct PendingEos {
        eos_accepted: bool,
        polls: usize,
    }

    impl AudioProcessor for PendingEos {
        fn capabilities(&self) -> AudioProcessorCapabilities {
            AudioProcessorCapabilities {
                fixed_block: false,
                buffered: true,
            }
        }

        fn set_playback_policy(&mut self, _: f32, _: u32, _: u64) -> PluginResult<()> {
            Ok(())
        }

        fn push(&mut self, input: Option<AudioBufferView<'_>>) -> PluginResult<AudioPush> {
            assert!(input.is_none());
            assert!(!self.eos_accepted);
            self.eos_accepted = true;
            Ok(AudioPush::EosAccepted)
        }

        fn pull(&mut self, _: AudioBufferMut<'_>) -> PluginResult<AudioPull> {
            assert!(self.eos_accepted);
            self.polls += 1;
            Ok(if self.polls == 1 {
                AudioPull::TryAgain
            } else {
                AudioPull::Eof
            })
        }

        fn latency_frames(&self) -> u32 {
            0
        }

        fn flush(&mut self) -> PluginResult<()> {
            Ok(())
        }
    }

    #[test]
    fn buffered_eos_retries_a_pending_pull_until_eof() {
        let mut processor = DesktopAudioProcessor {
            processor: Box::new(PendingEos {
                eos_accepted: false,
                polls: 0,
            }),
            input_limit: 1,
            channels: NonZeroU16::MIN,
            sample_rate: 48000,
            epoch: 1,
            rate: 1.0,
            buffered: true,
            scratch: vec![0.0],
            next_pts: Duration::ZERO,
        };
        processor
            .finish(|_| panic!("no PCM output expected"))
            .expect("pending EOS must drain");
    }
}
