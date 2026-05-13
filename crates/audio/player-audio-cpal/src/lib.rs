//! CPAL audio output and playback clock tracking for desktop runtime paths.
//!
//! This crate is an internal implementation detail of the desktop adapter. It
//! exposes audio sink primitives rather than a stable public SDK surface.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    FromSample, OutputCallbackInfo, Sample, SampleFormat, SizedSample, Stream, StreamConfig,
};
use rtrb::{Consumer, Producer, RingBuffer};

const AUDIO_RING_CAPACITY_SECONDS: usize = 8;
const AUDIO_RING_MIN_CAPACITY_SAMPLES: usize = 16_384;
const STALE_DRAIN_MULTIPLIER: usize = 4;

#[derive(Debug, Clone)]
pub struct AudioOutputConfig {
    pub channels: u16,
    pub sample_rate: u32,
    pub sample_format: SampleFormat,
    pub stream_config: StreamConfig,
}

#[derive(Debug, Clone)]
pub struct AudioOutputDescriptor {
    pub default_output_device: Option<String>,
    pub default_output_config: Option<AudioOutputConfig>,
}

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

struct SharedPlaybackState {
    timeline: Mutex<PlaybackTimelineState>,
    producer: Mutex<Producer<AudioRingSample>>,
    generation: AtomicU64,
    ring_generation: AtomicU32,
    completed_generation: AtomicU64,
    queued_samples: AtomicUsize,
    played_samples: AtomicUsize,
    paused: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}

impl std::fmt::Debug for SharedPlaybackState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedPlaybackState")
            .field("generation", &self.generation.load(Ordering::Relaxed))
            .field(
                "completed_generation",
                &self.completed_generation.load(Ordering::Relaxed),
            )
            .field(
                "queued_samples",
                &self.queued_samples.load(Ordering::Relaxed),
            )
            .field(
                "played_samples",
                &self.played_samples.load(Ordering::Relaxed),
            )
            .field("paused", &self.paused.load(Ordering::Relaxed))
            .field("finished", &self.finished.load(Ordering::Relaxed))
            .finish()
    }
}

#[derive(Debug)]
struct PlaybackTimelineState {
    generation: u64,
    media_start: Duration,
    playback_rate: f32,
}

#[derive(Debug, Clone, Copy)]
struct AudioRingSample {
    generation: u32,
    value: f32,
}

pub fn detect_default_output() -> AudioOutputDescriptor {
    let host = cpal::default_host();
    let Some(device) = host.default_output_device() else {
        return AudioOutputDescriptor {
            default_output_device: None,
            default_output_config: None,
        };
    };

    let default_output_config = device.default_output_config().ok().map(|config| {
        let sample_format = config.sample_format();
        let stream_config: StreamConfig = config.into();

        AudioOutputConfig {
            channels: stream_config.channels,
            sample_rate: stream_config.sample_rate,
            sample_format,
            stream_config,
        }
    });

    let default_output_device = default_output_config
        .as_ref()
        .and_then(|_| device.description().ok())
        .map(|description| description.name().to_owned());

    AudioOutputDescriptor {
        default_output_device,
        default_output_config,
    }
}

pub fn default_output_config() -> Result<AudioOutputConfig> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .context("no default audio output device available")?;
    let config = device
        .default_output_config()
        .context("failed to query default audio output configuration")?;
    let sample_format = config.sample_format();
    let stream_config: StreamConfig = config.into();

    Ok(AudioOutputConfig {
        channels: stream_config.channels,
        sample_rate: stream_config.sample_rate,
        sample_format,
        stream_config,
    })
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

        let paused = Arc::new(AtomicBool::new(start_paused));
        let finished = Arc::new(AtomicBool::new(false));
        let ring_capacity = audio_ring_capacity_samples(output_config.sample_rate, channels);
        let (producer, consumer) = RingBuffer::<AudioRingSample>::new(ring_capacity);
        let state = Arc::new(SharedPlaybackState {
            timeline: Mutex::new(PlaybackTimelineState {
                generation: 0,
                media_start,
                playback_rate: sanitize_playback_rate(playback_rate),
            }),
            producer: Mutex::new(producer),
            generation: AtomicU64::new(0),
            ring_generation: AtomicU32::new(0),
            completed_generation: AtomicU64::new(0),
            queued_samples: AtomicUsize::new(0),
            played_samples: AtomicUsize::new(0),
            paused: paused.clone(),
            finished: finished.clone(),
        });

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
        if self.state.paused.load(Ordering::SeqCst) {
            return;
        }

        self.state.paused.store(true, Ordering::SeqCst);
        let _ = self._stream.pause();
    }

    pub fn play(&mut self) {
        if !self.state.paused.load(Ordering::SeqCst) {
            return;
        }
        self.state.paused.store(false, Ordering::SeqCst);
        let _ = self._stream.play();
    }

    pub fn is_finished(&self) -> bool {
        self.state.finished.load(Ordering::SeqCst)
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

fn build_output_stream(
    device: &cpal::Device,
    output_config: &AudioOutputConfig,
    mut consumer: Consumer<AudioRingSample>,
    state: Arc<SharedPlaybackState>,
) -> Result<Stream> {
    let error_callback = |error| eprintln!("audio output stream error: {error}");
    let sample_rate = output_config.sample_rate;
    let channels = output_config.channels;

    match output_config.sample_format {
        SampleFormat::F32 => device
            .build_output_stream(
                &output_config.stream_config,
                {
                    let state = state.clone();
                    move |data: &mut [f32], info| {
                        write_output_data(data, &mut consumer, &state, sample_rate, channels, info)
                    }
                },
                error_callback,
                None,
            )
            .context("failed to build f32 audio output stream"),
        SampleFormat::I16 => device
            .build_output_stream(
                &output_config.stream_config,
                {
                    let state = state.clone();
                    move |data: &mut [i16], info| {
                        write_output_data(data, &mut consumer, &state, sample_rate, channels, info)
                    }
                },
                error_callback,
                None,
            )
            .context("failed to build i16 audio output stream"),
        SampleFormat::U16 => device
            .build_output_stream(
                &output_config.stream_config,
                move |data: &mut [u16], info| {
                    write_output_data(data, &mut consumer, &state, sample_rate, channels, info)
                },
                error_callback,
                None,
            )
            .context("failed to build u16 audio output stream"),
        sample_format => anyhow::bail!("unsupported default audio sample format: {sample_format}"),
    }
}

fn write_output_data<T>(
    data: &mut [T],
    consumer: &mut Consumer<AudioRingSample>,
    state: &SharedPlaybackState,
    sample_rate: u32,
    channels: u16,
    info: &OutputCallbackInfo,
) where
    T: Sample + SizedSample + FromSample<f32>,
{
    if state.paused.load(Ordering::SeqCst) {
        fill_silence(data);
        return;
    }

    let current_generation = state.ring_generation.load(Ordering::Acquire);
    let max_pops = data.len().saturating_mul(STALE_DRAIN_MULTIPLIER).max(1);
    let mut written = 0usize;
    let mut popped = 0usize;
    let mut played = 0usize;

    while written < data.len() && popped < max_pops {
        let Ok(sample) = consumer.pop() else {
            break;
        };
        popped = popped.saturating_add(1);
        if sample.generation != current_generation {
            continue;
        }

        data[written] = T::from_sample(sample.value);
        written = written.saturating_add(1);
        played = played.saturating_add(1);
    }

    if played > 0 {
        state.played_samples.fetch_add(played, Ordering::AcqRel);
        state.finished.store(false, Ordering::SeqCst);
    }

    for output in &mut data[written..] {
        *output = T::EQUILIBRIUM;
    }

    if state.is_current_generation_complete_and_drained() {
        state.finished.store(true, Ordering::SeqCst);
    }

    let _ = (sample_rate, channels, info);
}

impl SharedPlaybackState {
    fn begin_generation(&self, channels: u16, media_start: Duration, playback_rate: f32) -> u64 {
        let mut generation = 0u64;
        if let Ok(mut timeline) = self.timeline.lock() {
            timeline.generation = timeline.generation.saturating_add(1);
            timeline.media_start = media_start;
            timeline.playback_rate = sanitize_playback_rate(playback_rate);
            generation = timeline.generation;
            self.generation.store(generation, Ordering::Release);
            self.ring_generation
                .store(ring_generation(generation), Ordering::Release);
            self.completed_generation.store(0, Ordering::Release);
            self.queued_samples.store(0, Ordering::Release);
            self.played_samples.store(0, Ordering::Release);
        }

        let _ = channels;
        self.finished.store(false, Ordering::SeqCst);
        generation
    }

    fn append_samples(&self, generation: u64, samples: Vec<f32>) -> Result<bool> {
        let Ok(timeline) = self.timeline.lock() else {
            anyhow::bail!("audio playback timeline lock is poisoned");
        };
        if timeline.generation != generation {
            return Ok(false);
        }
        let ring_generation = ring_generation(generation);
        drop(timeline);

        if self.generation.load(Ordering::Acquire) != generation {
            return Ok(false);
        }

        let mut producer = self
            .producer
            .lock()
            .map_err(|_| anyhow::anyhow!("audio output ring producer lock is poisoned"))?;
        let available_slots = producer.slots();
        if available_slots < samples.len() {
            anyhow::bail!(
                "audio output ring is full: {} samples requested, {} slots available",
                samples.len(),
                available_slots
            );
        }

        let sample_count = samples.len();
        let chunk = producer
            .write_chunk_uninit(sample_count)
            .map_err(|error| anyhow::anyhow!("audio output ring write failed: {error}"))?;
        let written = chunk.fill_from_iter(samples.into_iter().map(|value| AudioRingSample {
            generation: ring_generation,
            value,
        }));
        if written != sample_count {
            anyhow::bail!(
                "audio output ring accepted {} of {} samples",
                written,
                sample_count
            );
        }

        if self.generation.load(Ordering::Acquire) != generation {
            return Ok(false);
        }

        self.queued_samples
            .fetch_add(sample_count, Ordering::AcqRel);
        self.finished.store(false, Ordering::SeqCst);
        Ok(true)
    }

    fn finish_generation(&self, generation: u64) {
        if let Ok(timeline) = self.timeline.lock()
            && timeline.generation == generation
        {
            self.completed_generation
                .store(generation, Ordering::Release);
            if self.is_current_generation_complete_and_drained() {
                self.finished.store(true, Ordering::SeqCst);
            }
        }
    }

    fn is_generation_active(&self, generation: u64) -> bool {
        self.timeline
            .lock()
            .map(|timeline| timeline.generation == generation)
            .unwrap_or(false)
    }

    fn buffered_samples(&self, generation: u64) -> Option<usize> {
        self.timeline.lock().ok().and_then(|timeline| {
            if timeline.generation != generation {
                return None;
            }

            Some(
                self.queued_samples
                    .load(Ordering::Acquire)
                    .saturating_sub(self.played_samples.load(Ordering::Acquire)),
            )
        })
    }

    fn playback_rate(&self) -> f32 {
        self.timeline
            .lock()
            .map(|timeline| timeline.playback_rate)
            .unwrap_or(1.0)
    }

    fn playback_position(&self, sample_rate: u32, channels: u16) -> Duration {
        let channels = usize::from(channels.max(1));
        let Ok(timeline) = self.timeline.lock() else {
            return Duration::ZERO;
        };
        media_time_for_sample_offset(
            timeline.media_start,
            timeline.playback_rate,
            sample_rate,
            channels,
            self.played_samples.load(Ordering::Acquire),
        )
    }

    fn is_current_generation_complete_and_drained(&self) -> bool {
        let generation = self.generation.load(Ordering::Acquire);
        generation != 0
            && self.completed_generation.load(Ordering::Acquire) == generation
            && self.played_samples.load(Ordering::Acquire)
                >= self.queued_samples.load(Ordering::Acquire)
    }
}

fn fill_silence<T>(data: &mut [T])
where
    T: Sample,
{
    for output in data {
        *output = T::EQUILIBRIUM;
    }
}

fn duration_from_frames(frames: u64, sample_rate: u32) -> Duration {
    if sample_rate == 0 {
        return Duration::ZERO;
    }

    Duration::from_secs_f64((frames as f64) / f64::from(sample_rate))
}

fn audio_ring_capacity_samples(sample_rate: u32, channels: usize) -> usize {
    (sample_rate as usize)
        .saturating_mul(channels.max(1))
        .saturating_mul(AUDIO_RING_CAPACITY_SECONDS)
        .max(AUDIO_RING_MIN_CAPACITY_SAMPLES)
}

fn ring_generation(generation: u64) -> u32 {
    generation as u32
}

fn media_time_for_sample_offset(
    media_start: Duration,
    playback_rate: f32,
    sample_rate: u32,
    channels: usize,
    sample_offset: usize,
) -> Duration {
    let frame_offset = sample_offset / channels.max(1);
    media_start
        + Duration::from_secs_f64(
            duration_from_frames(frame_offset as u64, sample_rate).as_secs_f64()
                * f64::from(playback_rate),
        )
}

fn sanitize_playback_rate(playback_rate: f32) -> f32 {
    if playback_rate.is_finite() && playback_rate > 0.0 {
        playback_rate
    } else {
        1.0
    }
}
