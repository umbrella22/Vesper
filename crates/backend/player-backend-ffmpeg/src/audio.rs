use std::mem::size_of;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use anyhow::{Context, Result};
use ffmpeg::filter;
use ffmpeg::format::sample::{Sample, Type as SampleType};
use ffmpeg::util::frame::audio::Audio;
use ffmpeg_next as ffmpeg;
use player_model::MediaSource;
use player_plugin::{AudioPitchMode, AudioPlaybackPolicy};
use tracing::warn;

use crate::hls::resolve_audio_decode_source;
use crate::input::{InputOpenPurpose, open_media_input};
use crate::probe::media_probe_from_input;
use crate::time::{duration_to_av_timestamp, timestamp_to_duration};
use crate::{AudioStreamError, DecodedAudioChunk, DecodedAudioTrack, FfmpegBackend, MediaProbe};

#[derive(Debug)]
struct EmittedAudioTimeline {
    start_position: Duration,
    playback_rate: f32,
    next_presentation_time: Option<Duration>,
}

impl EmittedAudioTimeline {
    fn new(start_position: Duration, playback_rate: f32) -> Self {
        Self {
            start_position,
            playback_rate,
            next_presentation_time: None,
        }
    }

    fn next_chunk_timing(
        &mut self,
        filtered_presentation_time: Option<Duration>,
        frame_count: usize,
        sample_rate: u32,
    ) -> (Duration, Duration) {
        let duration = Duration::from_secs_f64(
            (frame_count as f64 / f64::from(sample_rate.max(1))) * f64::from(self.playback_rate),
        );
        let presentation_time = self.next_presentation_time.unwrap_or_else(|| {
            filtered_presentation_time
                .unwrap_or(self.start_position)
                .max(self.start_position)
        });
        self.next_presentation_time = Some(presentation_time.saturating_add(duration));
        (presentation_time, duration)
    }
}

impl DecodedAudioTrack {
    pub fn duration(&self) -> Duration {
        let sample_frames = self.samples.len() / usize::from(self.channels.max(1));
        Duration::from_secs_f64(
            (sample_frames as f64 / f64::from(self.sample_rate.max(1)))
                * f64::from(self.playback_rate.max(f32::EPSILON)),
        )
    }

    pub fn sample_offset_for_position(&self, position: Duration) -> usize {
        if position <= self.presentation_time {
            return 0;
        }

        let offset = position.saturating_sub(self.presentation_time);
        let frame_offset = (offset.as_secs_f64() / f64::from(self.playback_rate.max(f32::EPSILON))
            * f64::from(self.sample_rate))
        .floor() as usize;
        let sample_offset = frame_offset.saturating_mul(usize::from(self.channels.max(1)));

        sample_offset.min(self.samples.len())
    }

    pub fn media_time_for_sample_offset(&self, sample_offset: usize) -> Duration {
        let aligned_offset = sample_offset - (sample_offset % usize::from(self.channels.max(1)));
        let frame_offset = aligned_offset / usize::from(self.channels.max(1));

        self.presentation_time
            + Duration::from_secs_f64(
                (frame_offset as f64 / f64::from(self.sample_rate.max(1)))
                    * f64::from(self.playback_rate.max(f32::EPSILON)),
            )
    }
}

impl FfmpegBackend {
    pub fn decode_audio_track(
        &self,
        source: MediaSource,
        output_rate: u32,
        output_channels: u16,
    ) -> Result<DecodedAudioTrack> {
        self.decode_audio_track_with_interrupt(source, output_rate, output_channels, None)
    }

    pub fn decode_audio_track_with_interrupt(
        &self,
        source: MediaSource,
        output_rate: u32,
        output_channels: u16,
        interrupt_flag: Option<Arc<AtomicBool>>,
    ) -> Result<DecodedAudioTrack> {
        self.decode_audio_track_with_playback_rate_and_interrupt(
            source,
            output_rate,
            output_channels,
            1.0,
            interrupt_flag,
        )
    }

    pub fn decode_audio_track_with_playback_rate(
        &self,
        source: MediaSource,
        output_rate: u32,
        output_channels: u16,
        playback_rate: f32,
    ) -> Result<DecodedAudioTrack> {
        self.decode_audio_track_with_playback_rate_and_interrupt(
            source,
            output_rate,
            output_channels,
            playback_rate,
            None,
        )
    }

    pub fn decode_audio_track_with_playback_rate_and_interrupt(
        &self,
        source: MediaSource,
        output_rate: u32,
        output_channels: u16,
        playback_rate: f32,
        interrupt_flag: Option<Arc<AtomicBool>>,
    ) -> Result<DecodedAudioTrack> {
        self.decode_audio_track_with_playback_policy_and_interrupt(
            source,
            output_rate,
            output_channels,
            AudioPlaybackPolicy {
                playback_rate,
                pitch_mode: AudioPitchMode::PreservePitch,
            },
            interrupt_flag,
        )
    }

    pub fn decode_audio_track_with_playback_policy(
        &self,
        source: MediaSource,
        output_rate: u32,
        output_channels: u16,
        playback_policy: AudioPlaybackPolicy,
    ) -> Result<DecodedAudioTrack> {
        self.decode_audio_track_with_playback_policy_and_interrupt(
            source,
            output_rate,
            output_channels,
            playback_policy,
            None,
        )
    }

    pub fn decode_audio_track_with_playback_policy_and_interrupt(
        &self,
        source: MediaSource,
        output_rate: u32,
        output_channels: u16,
        playback_policy: AudioPlaybackPolicy,
        interrupt_flag: Option<Arc<AtomicBool>>,
    ) -> Result<DecodedAudioTrack> {
        if output_rate == 0 {
            anyhow::bail!("audio output sample rate must be greater than zero");
        }

        if output_channels == 0 {
            anyhow::bail!("audio output channel count must be greater than zero");
        }

        if !playback_policy.playback_rate.is_finite() || playback_policy.playback_rate <= 0.0 {
            anyhow::bail!("audio playback rate must be a finite value greater than zero");
        }

        let audio_source = resolve_audio_decode_source(&source, interrupt_flag.clone())
            .unwrap_or_else(|error| {
                warn!(
                    source = source.uri(),
                    error = %error,
                    "failed to resolve remote HLS audio rendition playlist; falling back to the original source"
                );
                source.clone()
            });
        let mut input =
            open_media_input(&audio_source, InputOpenPurpose::AudioDecode, interrupt_flag)
                .with_context(|| format!("failed to open media source: {}", audio_source.uri()))?;
        let stream = input
            .streams()
            .best(ffmpeg::media::Type::Audio)
            .context("no audio stream found in media source")?;
        let stream_index = stream.index();
        let time_base = stream.time_base();

        let context_decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
            .context("failed to create decoder context for audio stream")?;
        let mut decoder = context_decoder
            .decoder()
            .audio()
            .context("failed to open audio decoder")?;

        let output_layout = ffmpeg::ChannelLayout::default(i32::from(output_channels));
        let mut filter_graph = build_audio_filter_graph(
            &decoder,
            time_base,
            output_rate,
            output_layout,
            playback_policy,
        )?;

        let mut first_presentation_time = None;
        let mut samples = Vec::new();

        for (stream, packet) in input.packets() {
            if stream.index() != stream_index {
                continue;
            }

            decoder
                .send_packet(&packet)
                .context("failed to send audio packet to decoder")?;
            drain_audio_frames(
                &mut decoder,
                &mut filter_graph,
                time_base,
                output_rate,
                output_channels,
                &mut first_presentation_time,
                &mut samples,
            )?;
        }

        decoder
            .send_eof()
            .context("failed to flush audio decoder")?;
        drain_audio_frames(
            &mut decoder,
            &mut filter_graph,
            time_base,
            output_rate,
            output_channels,
            &mut first_presentation_time,
            &mut samples,
        )?;
        flush_audio_filter(
            &mut filter_graph,
            time_base,
            output_rate,
            output_channels,
            &mut samples,
        )?;

        Ok(DecodedAudioTrack {
            presentation_time: first_presentation_time.unwrap_or(Duration::ZERO),
            sample_rate: output_rate,
            channels: output_channels,
            playback_rate: playback_policy.playback_rate,
            samples: Arc::from(samples),
        })
    }

    pub fn stream_audio_source_with_playback_rate_and_interrupt<P, F>(
        &self,
        source: MediaSource,
        output_rate: u32,
        output_channels: u16,
        playback_rate: f32,
        start_position: Duration,
        interrupt_flag: Option<Arc<AtomicBool>>,
        on_probe: P,
        on_chunk: F,
    ) -> std::result::Result<(), AudioStreamError>
    where
        P: FnMut(MediaProbe) -> std::result::Result<(), AudioStreamError>,
        F: FnMut(DecodedAudioChunk) -> std::result::Result<bool, AudioStreamError>,
    {
        self.stream_audio_source_with_playback_policy_and_interrupt(
            source,
            output_rate,
            output_channels,
            AudioPlaybackPolicy {
                playback_rate,
                pitch_mode: AudioPitchMode::PreservePitch,
            },
            start_position,
            interrupt_flag,
            on_probe,
            on_chunk,
        )
    }

    pub fn stream_audio_source_with_playback_policy_and_interrupt<P, F>(
        &self,
        source: MediaSource,
        output_rate: u32,
        output_channels: u16,
        playback_policy: AudioPlaybackPolicy,
        start_position: Duration,
        interrupt_flag: Option<Arc<AtomicBool>>,
        on_probe: P,
        on_chunk: F,
    ) -> std::result::Result<(), AudioStreamError>
    where
        P: FnMut(MediaProbe) -> std::result::Result<(), AudioStreamError>,
        F: FnMut(DecodedAudioChunk) -> std::result::Result<bool, AudioStreamError>,
    {
        if output_rate == 0 {
            return Err(AudioStreamError::InvalidOutputRate);
        }

        if output_channels == 0 {
            return Err(AudioStreamError::InvalidOutputChannels);
        }

        if !playback_policy.playback_rate.is_finite() || playback_policy.playback_rate <= 0.0 {
            return Err(AudioStreamError::InvalidPlaybackRate);
        }

        self.stream_audio_source_with_playback_policy_and_interrupt_impl(
            source,
            output_rate,
            output_channels,
            playback_policy,
            start_position,
            interrupt_flag,
            on_probe,
            on_chunk,
        )
        .map_err(|error| {
            error
                .downcast_ref::<AudioStreamError>()
                .cloned()
                .unwrap_or_else(|| AudioStreamError::Backend {
                    operation: "decode",
                    message: error.to_string(),
                })
        })
    }

    fn stream_audio_source_with_playback_policy_and_interrupt_impl<P, F>(
        &self,
        source: MediaSource,
        output_rate: u32,
        output_channels: u16,
        playback_policy: AudioPlaybackPolicy,
        start_position: Duration,
        interrupt_flag: Option<Arc<AtomicBool>>,
        mut on_probe: P,
        mut on_chunk: F,
    ) -> Result<()>
    where
        P: FnMut(MediaProbe) -> std::result::Result<(), AudioStreamError>,
        F: FnMut(DecodedAudioChunk) -> std::result::Result<bool, AudioStreamError>,
    {
        let audio_source = resolve_audio_decode_source(&source, interrupt_flag.clone())
            .unwrap_or_else(|error| {
                warn!(
                    source = source.uri(),
                    error = %error,
                    "failed to resolve remote HLS audio rendition playlist; falling back to the original source"
                );
                source.clone()
            });
        let mut input =
            open_media_input(&audio_source, InputOpenPurpose::AudioDecode, interrupt_flag)
                .with_context(|| format!("failed to open media source: {}", audio_source.uri()))?;
        let stream = input
            .streams()
            .best(ffmpeg::media::Type::Audio)
            .context("no audio stream found in media source")?;
        let stream_index = stream.index();
        let time_base = stream.time_base();
        let stream_parameters = stream.parameters();
        on_probe(media_probe_from_input(&input, &source)?)
            .map_err(|error| anyhow::anyhow!(error))?;

        if !start_position.is_zero() {
            let timestamp = duration_to_av_timestamp(start_position);
            input.seek(timestamp, ..timestamp).with_context(|| {
                format!(
                    "failed to seek audio source {} to {:.3}s",
                    audio_source.uri(),
                    start_position.as_secs_f64()
                )
            })?;
        }

        let context_decoder = ffmpeg::codec::context::Context::from_parameters(stream_parameters)
            .context("failed to create decoder context for audio stream")?;
        let mut decoder = context_decoder
            .decoder()
            .audio()
            .context("failed to open audio decoder")?;

        let output_layout = ffmpeg::ChannelLayout::default(i32::from(output_channels));
        let mut filter_graph = build_audio_filter_graph(
            &decoder,
            time_base,
            output_rate,
            output_layout,
            playback_policy,
        )?;

        let mut discontinuity = true;
        let mut emitted_timeline =
            EmittedAudioTimeline::new(start_position, playback_policy.playback_rate);
        for (stream, packet) in input.packets() {
            if stream.index() != stream_index {
                continue;
            }

            decoder
                .send_packet(&packet)
                .context("failed to send audio packet to decoder")?;
            if !drain_audio_frames_with_emitter(
                &mut decoder,
                &mut filter_graph,
                time_base,
                output_rate,
                output_channels,
                &mut discontinuity,
                &mut emitted_timeline,
                &mut on_chunk,
            )? {
                return Ok(());
            }
        }

        decoder
            .send_eof()
            .context("failed to flush audio decoder")?;
        if !drain_audio_frames_with_emitter(
            &mut decoder,
            &mut filter_graph,
            time_base,
            output_rate,
            output_channels,
            &mut discontinuity,
            &mut emitted_timeline,
            &mut on_chunk,
        )? {
            return Ok(());
        }
        flush_typed_audio_filter_with_emitter(
            &mut filter_graph,
            time_base,
            output_rate,
            output_channels,
            &mut discontinuity,
            &mut emitted_timeline,
            &mut on_chunk,
        )?;

        Ok(())
    }

    pub fn retime_audio_track(
        &self,
        source_track: &DecodedAudioTrack,
        playback_rate: f32,
    ) -> Result<DecodedAudioTrack> {
        self.retime_audio_track_with_playback_policy(
            source_track,
            AudioPlaybackPolicy {
                playback_rate,
                pitch_mode: AudioPitchMode::PreservePitch,
            },
        )
    }

    pub fn retime_audio_track_with_playback_policy(
        &self,
        source_track: &DecodedAudioTrack,
        playback_policy: AudioPlaybackPolicy,
    ) -> Result<DecodedAudioTrack> {
        self.retime_audio_track_range_with_playback_policy(
            source_track,
            playback_policy,
            0..source_track.samples.len(),
        )
    }

    pub fn retime_audio_track_range(
        &self,
        source_track: &DecodedAudioTrack,
        playback_rate: f32,
        sample_range: Range<usize>,
    ) -> Result<DecodedAudioTrack> {
        self.retime_audio_track_range_with_playback_policy(
            source_track,
            AudioPlaybackPolicy {
                playback_rate,
                pitch_mode: AudioPitchMode::PreservePitch,
            },
            sample_range,
        )
    }

    pub fn retime_audio_track_range_with_playback_policy(
        &self,
        source_track: &DecodedAudioTrack,
        playback_policy: AudioPlaybackPolicy,
        sample_range: Range<usize>,
    ) -> Result<DecodedAudioTrack> {
        if !playback_policy.playback_rate.is_finite() || playback_policy.playback_rate <= 0.0 {
            anyhow::bail!("audio playback rate must be a finite value greater than zero");
        }

        let channels = usize::from(source_track.channels.max(1));
        let start_sample =
            align_audio_sample_offset(sample_range.start, channels).min(source_track.samples.len());
        let end_sample =
            align_audio_sample_offset(sample_range.end, channels).min(source_track.samples.len());

        if end_sample <= start_sample {
            return Ok(DecodedAudioTrack {
                presentation_time: source_track.media_time_for_sample_offset(start_sample),
                sample_rate: source_track.sample_rate,
                channels: source_track.channels,
                playback_rate: playback_policy.playback_rate,
                samples: Arc::from(Vec::<f32>::new()),
            });
        }

        if (source_track.playback_rate - playback_policy.playback_rate).abs() < 0.000_001
            && start_sample == 0
            && end_sample == source_track.samples.len()
        {
            return Ok(source_track.clone());
        }

        let mut samples = Vec::new();
        self.stream_retime_audio_track_range_with_playback_policy(
            source_track,
            playback_policy,
            start_sample..end_sample,
            |chunk| {
                samples.extend(chunk);
                Ok(true)
            },
        )?;

        Ok(DecodedAudioTrack {
            presentation_time: source_track.media_time_for_sample_offset(start_sample),
            sample_rate: source_track.sample_rate,
            channels: source_track.channels,
            playback_rate: playback_policy.playback_rate,
            samples: Arc::from(samples),
        })
    }

    pub fn stream_retime_audio_track_range<F>(
        &self,
        source_track: &DecodedAudioTrack,
        playback_rate: f32,
        sample_range: Range<usize>,
        on_chunk: F,
    ) -> Result<()>
    where
        F: FnMut(Vec<f32>) -> Result<bool>,
    {
        self.stream_retime_audio_track_range_with_playback_policy(
            source_track,
            AudioPlaybackPolicy {
                playback_rate,
                pitch_mode: AudioPitchMode::PreservePitch,
            },
            sample_range,
            on_chunk,
        )
    }

    pub fn stream_retime_audio_track_range_with_playback_policy<F>(
        &self,
        source_track: &DecodedAudioTrack,
        playback_policy: AudioPlaybackPolicy,
        sample_range: Range<usize>,
        mut on_chunk: F,
    ) -> Result<()>
    where
        F: FnMut(Vec<f32>) -> Result<bool>,
    {
        if !playback_policy.playback_rate.is_finite() || playback_policy.playback_rate <= 0.0 {
            anyhow::bail!("audio playback rate must be a finite value greater than zero");
        }

        let channels = usize::from(source_track.channels.max(1));
        let start_sample =
            align_audio_sample_offset(sample_range.start, channels).min(source_track.samples.len());
        let end_sample =
            align_audio_sample_offset(sample_range.end, channels).min(source_track.samples.len());

        if end_sample <= start_sample {
            return Ok(());
        }

        let input_layout = ffmpeg::ChannelLayout::default(i32::from(source_track.channels));
        let mut filter_graph = build_audio_filter_graph_for_spec(
            Sample::F32(SampleType::Packed),
            ffmpeg::Rational(1, source_track.sample_rate.max(1) as i32),
            source_track.sample_rate,
            input_layout,
            source_track.sample_rate,
            input_layout,
            playback_policy,
        )?;
        let chunk_frames = 2_048usize;
        let start_frame = start_sample / channels;
        let end_frame = end_sample / channels;
        let total_frames = end_frame.saturating_sub(start_frame);

        for relative_frame_index in (0..total_frames).step_by(chunk_frames) {
            let frame_index = start_frame.saturating_add(relative_frame_index);
            let frames = (total_frames - relative_frame_index).min(chunk_frames);
            let sample_start = frame_index.saturating_mul(channels);
            let sample_end = sample_start + frames.saturating_mul(channels);
            let mut frame = Audio::new(Sample::F32(SampleType::Packed), frames, input_layout);
            frame.set_rate(source_track.sample_rate);
            frame.set_pts(Some(relative_frame_index as i64));
            copy_f32_samples_into_audio_frame(
                &mut frame,
                &source_track.samples[sample_start..sample_end],
            )?;
            filter_graph
                .get("in")
                .context("audio filter graph did not expose an input node")?
                .source()
                .add(&frame)
                .context("failed to push retimed audio frame into the filter graph")?;
            if !emit_filtered_audio_frames_raw(
                &mut filter_graph,
                ffmpeg::Rational(1, source_track.sample_rate.max(1) as i32),
                source_track.sample_rate,
                source_track.channels,
                &mut on_chunk,
            )? {
                return Ok(());
            }
        }

        flush_audio_filter_with_raw_emitter(
            &mut filter_graph,
            ffmpeg::Rational(1, source_track.sample_rate.max(1) as i32),
            source_track.sample_rate,
            source_track.channels,
            &mut on_chunk,
        )?;
        Ok(())
    }
}

fn drain_audio_frames(
    decoder: &mut ffmpeg::decoder::Audio,
    filter_graph: &mut filter::Graph,
    time_base: ffmpeg::Rational,
    output_rate: u32,
    output_channels: u16,
    first_presentation_time: &mut Option<Duration>,
    samples: &mut Vec<f32>,
) -> Result<()> {
    loop {
        let mut decoded = Audio::empty();
        if decoder.receive_frame(&mut decoded).is_err() {
            return Ok(());
        }

        if first_presentation_time.is_none() {
            *first_presentation_time = decoded
                .timestamp()
                .or(decoded.pts())
                .and_then(|timestamp| timestamp_to_duration(timestamp, time_base));
        }

        let presentation_timestamp = decoded.timestamp().or(decoded.pts());
        decoded.set_pts(presentation_timestamp);
        filter_graph
            .get("in")
            .context("audio filter graph did not expose an input node")?
            .source()
            .add(&decoded)
            .context("failed to push decoded audio frame into the filter graph")?;
        collect_filtered_audio_frames(
            filter_graph,
            time_base,
            output_rate,
            output_channels,
            samples,
        )?;
    }
}

fn drain_audio_frames_with_emitter<F>(
    decoder: &mut ffmpeg::decoder::Audio,
    filter_graph: &mut filter::Graph,
    time_base: ffmpeg::Rational,
    output_rate: u32,
    output_channels: u16,
    discontinuity: &mut bool,
    emitted_timeline: &mut EmittedAudioTimeline,
    emit: &mut F,
) -> Result<bool>
where
    F: FnMut(DecodedAudioChunk) -> std::result::Result<bool, AudioStreamError>,
{
    loop {
        let mut decoded = Audio::empty();
        if decoder.receive_frame(&mut decoded).is_err() {
            return Ok(true);
        }

        let presentation_timestamp = decoded.timestamp().or(decoded.pts());
        decoded.set_pts(presentation_timestamp);
        filter_graph
            .get("in")
            .context("audio filter graph did not expose an input node")?
            .source()
            .add(&decoded)
            .context("failed to push decoded audio frame into the filter graph")?;
        if !emit_typed_filtered_audio_frames(
            filter_graph,
            time_base,
            output_rate,
            output_channels,
            discontinuity,
            emitted_timeline,
            emit,
        )? {
            return Ok(false);
        }
    }
}

fn collect_filtered_audio_frames(
    filter_graph: &mut filter::Graph,
    time_base: ffmpeg::Rational,
    output_rate: u32,
    output_channels: u16,
    samples: &mut Vec<f32>,
) -> Result<()> {
    let mut discontinuity = false;
    let mut emitted_timeline = EmittedAudioTimeline::new(Duration::ZERO, 1.0);
    emit_typed_filtered_audio_frames(
        filter_graph,
        time_base,
        output_rate,
        output_channels,
        &mut discontinuity,
        &mut emitted_timeline,
        &mut |chunk| {
            samples.extend(chunk.samples);
            Ok(true)
        },
    )
    .map(|_| ())
}

fn flush_audio_filter(
    filter_graph: &mut filter::Graph,
    time_base: ffmpeg::Rational,
    output_rate: u32,
    output_channels: u16,
    samples: &mut Vec<f32>,
) -> Result<()> {
    let mut discontinuity = false;
    let mut emitted_timeline = EmittedAudioTimeline::new(Duration::ZERO, 1.0);
    flush_typed_audio_filter_with_emitter(
        filter_graph,
        time_base,
        output_rate,
        output_channels,
        &mut discontinuity,
        &mut emitted_timeline,
        &mut |chunk| {
            samples.extend(chunk.samples);
            Ok(true)
        },
    )
}

fn flush_typed_audio_filter_with_emitter<F>(
    filter_graph: &mut filter::Graph,
    time_base: ffmpeg::Rational,
    output_rate: u32,
    output_channels: u16,
    discontinuity: &mut bool,
    emitted_timeline: &mut EmittedAudioTimeline,
    emit: &mut F,
) -> Result<()>
where
    F: FnMut(DecodedAudioChunk) -> std::result::Result<bool, AudioStreamError>,
{
    filter_graph
        .get("in")
        .context("audio filter graph did not expose an input node")?
        .source()
        .flush()
        .context("failed to flush the audio filter graph")?;
    emit_typed_filtered_audio_frames(
        filter_graph,
        time_base,
        output_rate,
        output_channels,
        discontinuity,
        emitted_timeline,
        emit,
    )
    .map(|_| ())
}

fn emit_typed_filtered_audio_frames<F>(
    filter_graph: &mut filter::Graph,
    time_base: ffmpeg::Rational,
    output_rate: u32,
    output_channels: u16,
    discontinuity: &mut bool,
    emitted_timeline: &mut EmittedAudioTimeline,
    emit: &mut F,
) -> Result<bool>
where
    F: FnMut(DecodedAudioChunk) -> std::result::Result<bool, AudioStreamError>,
{
    let mut filtered = Audio::empty();
    while filter_graph
        .get("out")
        .context("audio filter graph did not expose an output node")?
        .sink()
        .frame(&mut filtered)
        .is_ok()
    {
        let samples = copy_interleaved_f32_samples(&filtered)?;
        let frame_count = filtered.samples();
        let filtered_presentation_time = filtered
            .timestamp()
            .or(filtered.pts())
            .and_then(|timestamp| timestamp_to_duration(timestamp, time_base));
        let (presentation_time, duration) = emitted_timeline.next_chunk_timing(
            filtered_presentation_time,
            frame_count,
            output_rate,
        );
        let chunk = DecodedAudioChunk {
            presentation_time: Some(presentation_time),
            duration,
            sample_rate: output_rate,
            channels: output_channels,
            samples,
            discontinuity: *discontinuity,
        };
        chunk.validate().map_err(|error| {
            anyhow::anyhow!(AudioStreamError::InvalidChunk {
                message: error.to_string(),
            })
        })?;
        *discontinuity = false;
        if !emit(chunk).map_err(|error| anyhow::anyhow!(error))? {
            return Ok(false);
        }
    }

    Ok(true)
}

fn emit_filtered_audio_frames_raw<F>(
    filter_graph: &mut filter::Graph,
    time_base: ffmpeg::Rational,
    output_rate: u32,
    output_channels: u16,
    emit: &mut F,
) -> Result<bool>
where
    F: FnMut(Vec<f32>) -> Result<bool>,
{
    let mut discontinuity = false;
    let mut emitted_timeline = EmittedAudioTimeline::new(Duration::ZERO, 1.0);
    emit_typed_filtered_audio_frames(
        filter_graph,
        time_base,
        output_rate,
        output_channels,
        &mut discontinuity,
        &mut emitted_timeline,
        &mut |chunk| {
            emit(chunk.samples).map_err(|error| AudioStreamError::Consumer {
                message: error.to_string(),
            })
        },
    )
}

fn flush_audio_filter_with_raw_emitter<F>(
    filter_graph: &mut filter::Graph,
    time_base: ffmpeg::Rational,
    output_rate: u32,
    output_channels: u16,
    emit: &mut F,
) -> Result<()>
where
    F: FnMut(Vec<f32>) -> Result<bool>,
{
    filter_graph
        .get("in")
        .context("audio filter graph did not expose an input node")?
        .source()
        .flush()
        .context("failed to flush the audio filter graph")?;
    emit_filtered_audio_frames_raw(filter_graph, time_base, output_rate, output_channels, emit)
        .map(|_| ())
}

fn normalized_channel_layout(
    channel_layout: ffmpeg::ChannelLayout,
    channels: u16,
) -> ffmpeg::ChannelLayout {
    if channel_layout.is_empty() {
        ffmpeg::ChannelLayout::default(i32::from(channels))
    } else {
        channel_layout
    }
}

fn copy_interleaved_f32_samples(frame: &Audio) -> Result<Vec<f32>> {
    let channels = frame.channels() as usize;
    let total_samples = frame.samples().saturating_mul(channels);
    if total_samples == 0 {
        return Ok(Vec::new());
    }

    let bytes = frame.data(0);
    let expected_bytes = total_samples * size_of::<f32>();
    let bytes = bytes.get(..expected_bytes).with_context(|| {
        format!(
            "resampled audio frame is smaller than expected: have {} bytes, need {}",
            bytes.len(),
            expected_bytes
        )
    })?;

    let mut samples = Vec::with_capacity(total_samples);
    let (chunks, _) = bytes.as_chunks::<{ size_of::<f32>() }>();
    for &[a, b, c, d] in chunks {
        let sample = f32::from_ne_bytes([a, b, c, d]);
        samples.push(sample);
    }

    Ok(samples)
}

fn copy_f32_samples_into_audio_frame(frame: &mut Audio, samples: &[f32]) -> Result<()> {
    let expected_bytes = samples.len().saturating_mul(size_of::<f32>());
    let frame_bytes = frame.data_mut(0);
    let frame_len = frame_bytes.len();
    let target = frame_bytes.get_mut(..expected_bytes).with_context(|| {
        format!(
            "audio frame buffer is smaller than expected: have {} bytes, need {}",
            frame_len, expected_bytes
        )
    })?;

    let (chunks, _) = target.as_chunks_mut::<{ size_of::<f32>() }>();
    for (chunk, sample) in chunks.iter_mut().zip(samples.iter()) {
        chunk.copy_from_slice(&sample.to_ne_bytes());
    }

    Ok(())
}

fn align_audio_sample_offset(sample_offset: usize, channels: usize) -> usize {
    if channels == 0 {
        return sample_offset;
    }

    sample_offset - (sample_offset % channels)
}

fn build_audio_filter_graph(
    decoder: &ffmpeg::decoder::Audio,
    time_base: ffmpeg::Rational,
    output_rate: u32,
    output_layout: ffmpeg::ChannelLayout,
    playback_policy: AudioPlaybackPolicy,
) -> Result<filter::Graph> {
    let input_layout = normalized_channel_layout(decoder.channel_layout(), decoder.channels());
    build_audio_filter_graph_for_spec(
        decoder.format(),
        time_base,
        decoder.rate(),
        input_layout,
        output_rate,
        output_layout,
        playback_policy,
    )
}

fn build_audio_filter_graph_for_spec(
    input_format: Sample,
    time_base: ffmpeg::Rational,
    input_rate: u32,
    input_layout: ffmpeg::ChannelLayout,
    output_rate: u32,
    output_layout: ffmpeg::ChannelLayout,
    playback_policy: AudioPlaybackPolicy,
) -> Result<filter::Graph> {
    let mut filter_graph = filter::Graph::new();
    let args = format!(
        "time_base={}:sample_rate={}:sample_fmt={}:channel_layout=0x{:x}",
        time_base,
        input_rate,
        input_format.name(),
        input_layout.bits()
    );

    filter_graph
        .add(
            &filter::find("abuffer").context("failed to resolve FFmpeg abuffer filter")?,
            "in",
            &args,
        )
        .context("failed to create FFmpeg audio filter input")?;
    filter_graph
        .add(
            &filter::find("abuffersink").context("failed to resolve FFmpeg abuffersink filter")?,
            "out",
            "",
        )
        .context("failed to create FFmpeg audio filter output")?;

    let filter_spec = audio_filter_spec(playback_policy, input_rate, output_rate, output_layout);
    filter_graph
        .output("in", 0)
        .context("failed to wire FFmpeg audio filter input")?
        .input("out", 0)
        .context("failed to wire FFmpeg audio filter output")?
        .parse(&filter_spec)
        .with_context(|| format!("failed to parse FFmpeg audio filter spec: {filter_spec}"))?;
    filter_graph
        .validate()
        .context("failed to validate FFmpeg audio filter graph")?;

    Ok(filter_graph)
}

pub(crate) fn audio_filter_spec(
    playback_policy: AudioPlaybackPolicy,
    input_rate: u32,
    output_rate: u32,
    output_layout: ffmpeg::ChannelLayout,
) -> String {
    let sample_format = Sample::F32(SampleType::Packed).name();
    let rate_filter = match playback_policy.pitch_mode {
        AudioPitchMode::PreservePitch => playback_rate_filter_chain(playback_policy.playback_rate),
        AudioPitchMode::FollowRate => {
            follow_rate_filter_chain(input_rate, playback_policy.playback_rate)
        }
    };
    format!(
        "{},aresample={},aformat=sample_fmts={}:channel_layouts=0x{:x}",
        rate_filter,
        output_rate,
        sample_format,
        output_layout.bits(),
    )
}

pub(crate) fn follow_rate_filter_chain(input_rate: u32, playback_rate: f32) -> String {
    const EPSILON: f32 = 0.000_001;

    if (playback_rate - 1.0).abs() < EPSILON {
        return "anull".to_owned();
    }

    format!("asetrate={input_rate}*{playback_rate:.6}")
}

pub(crate) fn playback_rate_filter_chain(playback_rate: f32) -> String {
    const FILTER_MIN: f64 = 0.5;
    const FILTER_MAX: f64 = 2.0;
    const EPSILON: f64 = 0.000_001;

    let playback_rate = f64::from(playback_rate);
    if (playback_rate - 1.0).abs() < EPSILON {
        return "anull".to_owned();
    }

    let mut remaining = playback_rate;
    let mut stages = Vec::new();

    while remaining > FILTER_MAX + EPSILON {
        stages.push(FILTER_MAX);
        remaining /= FILTER_MAX;
    }

    while remaining < FILTER_MIN - EPSILON {
        stages.push(FILTER_MIN);
        remaining /= FILTER_MIN;
    }

    stages.push(remaining.clamp(FILTER_MIN, FILTER_MAX));
    stages
        .into_iter()
        .map(|stage| format!("atempo={stage:.6}"))
        .collect::<Vec<_>>()
        .join(",")
}
