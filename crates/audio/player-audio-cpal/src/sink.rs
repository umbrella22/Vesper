use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use cpal::Stream;
use cpal::traits::{HostTrait, StreamTrait};
use rtrb::RingBuffer;

use crate::ring::{AudioRingSample, audio_ring_capacity_samples};
use crate::stream::build_output_stream;
use crate::timeline::{SharedPlaybackState, sanitize_playback_rate};
use crate::types::AudioOutputConfig;

pub struct AudioSink {
    _stream: Stream,
    sample_rate: u32,
    channels: u16,
    state: Arc<SharedPlaybackState>,
}

#[derive(Debug, Clone)]
pub struct AudioSinkController {
    state: Arc<SharedPlaybackState>,
    channels: u16,
}

impl AudioSink {
    pub fn new_default(
        output_config: AudioOutputConfig,
        media_start: Duration,
        playback_rate: f32,
        start_paused: bool,
    ) -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .context("no default audio output device available")?;
        let channels = usize::from(output_config.channels);

        if channels == 0 {
            anyhow::bail!("audio output channel count must be greater than zero");
        }

        let ring_capacity = audio_ring_capacity_samples(output_config.sample_rate, channels);
        let (producer, consumer) = RingBuffer::<AudioRingSample>::new(ring_capacity);
        let state = Arc::new(SharedPlaybackState::new(
            producer,
            media_start,
            sanitize_playback_rate(playback_rate),
            start_paused,
        ));

        let stream = build_output_stream(&device, &output_config, consumer, state.clone())?;
        if !start_paused {
            stream
                .play()
                .context("failed to start default audio output stream")?;
        }

        Ok(Self {
            _stream: stream,
            sample_rate: output_config.sample_rate,
            channels: output_config.channels,
            state,
        })
    }

    pub fn controller(&self) -> AudioSinkController {
        AudioSinkController {
            state: self.state.clone(),
            channels: self.channels,
        }
    }

    pub fn pause(&mut self) {
        if self.state.is_paused() {
            return;
        }

        self.state.set_paused(true);
        let _ = self._stream.pause();
    }

    pub fn play(&mut self) {
        if !self.state.is_paused() {
            return;
        }
        self.state.set_paused(false);
        let _ = self._stream.play();
    }

    pub fn is_finished(&self) -> bool {
        self.state.is_finished()
    }

    pub fn playback_position(&self) -> Duration {
        self.state
            .playback_position(self.sample_rate, self.channels)
    }

    pub fn playback_rate(&self) -> f32 {
        self.state.playback_rate()
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }
}

impl AudioSinkController {
    pub fn begin_generation(&self, media_start: Duration, playback_rate: f32) -> u64 {
        self.state
            .begin_generation(self.channels, media_start, playback_rate)
    }

    pub fn append_samples(&self, generation: u64, samples: Vec<f32>) -> Result<bool> {
        if samples.is_empty() {
            return Ok(self.is_generation_active(generation));
        }

        let channels = usize::from(self.channels.max(1));
        if !samples.len().is_multiple_of(channels) {
            anyhow::bail!(
                "audio sample buffer length {} is not divisible by channel count {}",
                samples.len(),
                self.channels
            );
        }

        self.state.append_samples(generation, samples)
    }

    pub fn finish_generation(&self, generation: u64) {
        self.state.finish_generation(generation);
    }

    pub fn is_generation_active(&self, generation: u64) -> bool {
        self.state.is_generation_active(generation)
    }

    pub fn buffered_samples(&self, generation: u64) -> Option<usize> {
        self.state.buffered_samples(generation)
    }
}
