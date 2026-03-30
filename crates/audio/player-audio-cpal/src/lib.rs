use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::time::Instant;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    FromSample, OutputCallbackInfo, Sample, SampleFormat, SizedSample, Stream, StreamConfig,
};

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

#[derive(Debug)]
struct SharedPlaybackState {
    timeline: Mutex<PlaybackTimelineState>,
    paused: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}

#[derive(Debug)]
struct PlaybackTimelineState {
    generation: u64,
    media_start: Duration,
    playback_rate: f32,
    base_sample_offset: usize,
    samples: Vec<f32>,
    cursor: usize,
    played_cursor: usize,
    generation_complete: bool,
    scheduled_buffers: VecDeque<ScheduledBuffer>,
}

#[derive(Debug, Clone)]
struct ScheduledBuffer {
    generation: u64,
    start_sample: usize,
    end_sample: usize,
    playback_start_wall: Instant,
    playback_end_wall: Instant,
}

enum ReservedOutput {
    Pending,
    Ready(ReservedOutputChunk),
    Finished,
}

struct ReservedOutputChunk {
    samples: Vec<f32>,
    end_reached: bool,
}

pub fn detect_default_output() -> AudioOutputDescriptor {
    let default_output_device = default_output_device_name();
    let default_output_config = default_output_config().ok();

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
        let state = Arc::new(SharedPlaybackState {
            timeline: Mutex::new(PlaybackTimelineState {
                generation: 0,
                media_start,
                playback_rate: sanitize_playback_rate(playback_rate),
                base_sample_offset: 0,
                samples: Vec::new(),
                cursor: 0,
                played_cursor: 0,
                generation_complete: false,
                scheduled_buffers: VecDeque::new(),
            }),
            paused: paused.clone(),
            finished: finished.clone(),
        });

        let stream = build_output_stream(&device, &output_config, state.clone())?;
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

        self.state
            .freeze_played_position(self.sample_rate, self.channels);
        self.state.paused.store(true, Ordering::SeqCst);
        let _ = self._stream.pause();
    }

    pub fn play(&mut self) {
        if !self.state.paused.load(Ordering::SeqCst) {
            return;
        }
        self.state.clear_scheduled_buffers();
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
        if samples.len() % channels != 0 {
            anyhow::bail!(
                "audio sample buffer length {} is not divisible by channel count {}",
                samples.len(),
                self.channels
            );
        }

        Ok(self.state.append_samples(generation, samples))
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

fn default_output_device_name() -> Option<String> {
    let host = cpal::default_host();
    host.default_output_device()
        .and_then(|device| device.description().ok())
        .map(|description| description.name().to_owned())
}

fn build_output_stream(
    device: &cpal::Device,
    output_config: &AudioOutputConfig,
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
                        write_output_data(data, &state, sample_rate, channels, info)
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
                        write_output_data(data, &state, sample_rate, channels, info)
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
                    write_output_data(data, &state, sample_rate, channels, info)
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

    match state.reserve_output(data.len(), sample_rate, channels, info) {
        ReservedOutput::Pending => {
            fill_silence(data);
        }
        ReservedOutput::Finished => {
            fill_silence(data);
            state.finished.store(true, Ordering::SeqCst);
        }
        ReservedOutput::Ready(chunk) => {
            for (output, sample) in data.iter_mut().zip(chunk.samples.iter()) {
                *output = T::from_sample(*sample);
            }

            for output in &mut data[chunk.samples.len()..] {
                *output = T::EQUILIBRIUM;
            }

            if chunk.end_reached {
                state.finished.store(true, Ordering::SeqCst);
            }
        }
    }
}

impl SharedPlaybackState {
    fn begin_generation(&self, channels: u16, media_start: Duration, playback_rate: f32) -> u64 {
        let mut generation = 0u64;
        if let Ok(mut timeline) = self.timeline.lock() {
            timeline.generation = timeline.generation.saturating_add(1);
            timeline.media_start = media_start;
            timeline.playback_rate = sanitize_playback_rate(playback_rate);
            timeline.base_sample_offset = 0;
            timeline.samples.clear();
            timeline.cursor = 0;
            timeline.played_cursor = 0;
            timeline.generation_complete = false;
            timeline.scheduled_buffers.clear();
            generation = timeline.generation;
        }

        let _ = channels;
        self.finished.store(false, Ordering::SeqCst);
        generation
    }

    fn append_samples(&self, generation: u64, samples: Vec<f32>) -> bool {
        if let Ok(mut timeline) = self.timeline.lock() {
            if timeline.generation != generation {
                return false;
            }
            timeline.samples.extend(samples);
            self.finished.store(false, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    fn finish_generation(&self, generation: u64) {
        if let Ok(mut timeline) = self.timeline.lock() {
            if timeline.generation == generation {
                timeline.generation_complete = true;
                let end_sample = timeline.end_sample_offset();
                if timeline.cursor >= end_sample && timeline.played_cursor >= end_sample {
                    self.finished.store(true, Ordering::SeqCst);
                }
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

            Some(timeline.end_sample_offset().saturating_sub(timeline.cursor))
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
        let Ok(mut timeline) = self.timeline.lock() else {
            return Duration::ZERO;
        };
        let end_sample = timeline.end_sample_offset();
        let cursor = timeline.cursor.min(end_sample);
        let mut played_cursor = timeline.played_cursor.min(cursor);

        if !self.paused.load(Ordering::SeqCst) {
            let generation = timeline.generation;
            while timeline
                .scheduled_buffers
                .front()
                .map(|buffer| buffer.generation != generation)
                .unwrap_or(false)
            {
                timeline.scheduled_buffers.pop_front();
            }

            let now = Instant::now();
            while let Some(front) = timeline.scheduled_buffers.front() {
                if now < front.playback_end_wall {
                    break;
                }

                played_cursor = front.end_sample.min(cursor);
                timeline.scheduled_buffers.pop_front();
            }

            if let Some(front) = timeline.scheduled_buffers.front() {
                if now > front.playback_start_wall {
                    let elapsed = now.saturating_duration_since(front.playback_start_wall);
                    let elapsed_frames =
                        (elapsed.as_secs_f64() * f64::from(sample_rate)).floor() as usize;
                    let buffer_frames =
                        (front.end_sample.saturating_sub(front.start_sample)) / channels.max(1);
                    let played_frames = elapsed_frames.min(buffer_frames);
                    let interpolated = front
                        .start_sample
                        .saturating_add(played_frames.saturating_mul(channels.max(1)));
                    played_cursor = interpolated.min(front.end_sample).min(cursor);
                }
            } else if timeline.generation_complete && cursor >= end_sample {
                played_cursor = cursor;
            }
        }

        timeline.played_cursor = played_cursor;
        trim_consumed_prefix(&mut timeline, channels, sample_rate);
        media_time_for_sample_offset(
            timeline.media_start,
            timeline.playback_rate,
            sample_rate,
            channels,
            played_cursor,
        )
    }

    fn reserve_output(
        &self,
        requested_samples: usize,
        sample_rate: u32,
        channels: u16,
        info: &OutputCallbackInfo,
    ) -> ReservedOutput {
        let channels = usize::from(channels.max(1));
        let Ok(mut timeline) = self.timeline.lock() else {
            return ReservedOutput::Pending;
        };
        let end_sample = timeline.end_sample_offset();
        let start_sample = timeline.cursor.min(end_sample);
        let available_samples = end_sample
            .saturating_sub(start_sample)
            .min(requested_samples);

        if available_samples == 0 {
            return if timeline.generation_complete {
                ReservedOutput::Finished
            } else {
                ReservedOutput::Pending
            };
        }

        let chunk_end = start_sample.saturating_add(available_samples);
        let local_start = start_sample.saturating_sub(timeline.base_sample_offset);
        let local_end = chunk_end.saturating_sub(timeline.base_sample_offset);
        let copied_samples = timeline.samples[local_start..local_end].to_vec();
        let generation = timeline.generation;
        timeline.cursor = chunk_end;

        let buffer_frames = available_samples / channels.max(1);
        if buffer_frames > 0 {
            let timestamp = info.timestamp();
            let playback_delay = timestamp
                .playback
                .duration_since(&timestamp.callback)
                .unwrap_or(Duration::ZERO);
            let playback_start_wall = Instant::now() + playback_delay;
            let playback_end_wall =
                playback_start_wall + duration_from_frames(buffer_frames as u64, sample_rate);
            timeline.scheduled_buffers.push_back(ScheduledBuffer {
                generation,
                start_sample,
                end_sample: chunk_end,
                playback_start_wall,
                playback_end_wall,
            });
        }

        ReservedOutput::Ready(ReservedOutputChunk {
            samples: copied_samples,
            end_reached: timeline.generation_complete && chunk_end >= end_sample,
        })
    }

    fn freeze_played_position(&self, sample_rate: u32, channels: u16) {
        let played_position = self.playback_position(sample_rate, channels);
        if let Ok(mut timeline) = self.timeline.lock() {
            let sample_offset = sample_offset_for_media_time(
                timeline.media_start,
                timeline.playback_rate,
                sample_rate,
                usize::from(channels.max(1)),
                played_position,
            );
            timeline.played_cursor = sample_offset.min(timeline.end_sample_offset());
            timeline.scheduled_buffers.clear();
            trim_consumed_prefix(&mut timeline, usize::from(channels.max(1)), sample_rate);
        }
    }

    fn clear_scheduled_buffers(&self) {
        if let Ok(mut timeline) = self.timeline.lock() {
            timeline.scheduled_buffers.clear();
        }
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

fn sample_offset_for_media_time(
    media_start: Duration,
    playback_rate: f32,
    sample_rate: u32,
    channels: usize,
    position: Duration,
) -> usize {
    if position <= media_start {
        return 0;
    }

    let relative = position.saturating_sub(media_start);
    let frame_offset = (relative.as_secs_f64() / f64::from(playback_rate.max(f32::EPSILON))
        * f64::from(sample_rate))
    .floor() as usize;
    frame_offset.saturating_mul(channels.max(1))
}

fn sanitize_playback_rate(playback_rate: f32) -> f32 {
    if playback_rate.is_finite() && playback_rate > 0.0 {
        playback_rate
    } else {
        1.0
    }
}

impl PlaybackTimelineState {
    fn end_sample_offset(&self) -> usize {
        self.base_sample_offset.saturating_add(self.samples.len())
    }
}

fn trim_consumed_prefix(timeline: &mut PlaybackTimelineState, channels: usize, sample_rate: u32) {
    let trim_frames_threshold = sample_rate.max(1) as usize;
    let trim_samples_threshold = trim_frames_threshold.saturating_mul(channels.max(1));
    let consumed_samples = timeline
        .played_cursor
        .saturating_sub(timeline.base_sample_offset);
    let trim_samples = consumed_samples - (consumed_samples % channels.max(1));

    if trim_samples < trim_samples_threshold {
        return;
    }

    timeline.samples.drain(..trim_samples);
    timeline.base_sample_offset = timeline.base_sample_offset.saturating_add(trim_samples);
}
