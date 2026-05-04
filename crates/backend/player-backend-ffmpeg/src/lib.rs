#![allow(clippy::new_ret_no_self, clippy::too_many_arguments)]

mod buffered;

use std::collections::HashMap;
use std::ffi::{CString, c_int, c_void};
use std::mem::size_of;
use std::ops::{Deref, DerefMut, Range};
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use ffmpeg::codec;
use ffmpeg::filter;
use ffmpeg::format::Pixel;
use ffmpeg::format::sample::{Sample, Type as SampleType};
use ffmpeg::software::scaling::{context::Context as ScalingContext, flag::Flags};
use ffmpeg::util::frame::audio::Audio;
use ffmpeg::util::frame::video::Video;
use ffmpeg_next as ffmpeg;
use player_core::{MediaSource, MediaSourceKind, MediaSourceProtocol};
use tracing::{info, warn};

pub use buffered::{BufferedFramePoll, BufferedVideoSource, BufferedVideoSourceBootstrap};
pub use player_core::{DecodedVideoFrame, VideoPixelFormat};

const MAX_RESOLVED_HLS_SOURCE_CACHE_ENTRIES: usize = 32;

#[derive(Debug, Clone, Copy)]
pub struct FfmpegBackend {
    initialized: bool,
}

#[derive(Debug, Clone)]
pub struct MediaProbe {
    pub source: MediaSource,
    pub duration: Option<Duration>,
    pub bit_rate: Option<u64>,
    pub audio_streams: usize,
    pub video_streams: usize,
    pub best_video: Option<VideoStreamProbe>,
    pub best_audio: Option<AudioStreamProbe>,
}

#[derive(Debug, Clone)]
pub struct VideoStreamProbe {
    pub index: usize,
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub frame_rate: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct AudioStreamProbe {
    pub index: usize,
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoDecoderMode {
    Software,
    Hardware,
}

#[derive(Debug, Clone)]
pub struct VideoDecodeInfo {
    pub selected_mode: VideoDecoderMode,
    pub hardware_available: bool,
    pub hardware_backend: Option<String>,
    pub decoder_name: String,
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DecodedAudioTrack {
    pub presentation_time: Duration,
    pub sample_rate: u32,
    pub channels: u16,
    pub playback_rate: f32,
    pub samples: Arc<[f32]>,
}

pub struct VideoFrameSource {
    input: FfmpegInput,
    stream_index: usize,
    time_base: ffmpeg::Rational,
    fallback_frame_interval: Duration,
    fallback_start_time: Duration,
    decoder: ffmpeg::decoder::Video,
    output: VideoFrameOutput,
    decode_info: VideoDecodeInfo,
    decoded_frame_index: u64,
    end_of_input_sent: bool,
}

#[derive(Debug, Clone)]
pub struct VideoPacketStreamInfo {
    pub stream_index: usize,
    pub codec: String,
    pub extradata: Vec<u8>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frame_rate: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct CompressedVideoPacket {
    pub pts_us: Option<i64>,
    pub dts_us: Option<i64>,
    pub duration_us: Option<i64>,
    pub stream_index: u32,
    pub key_frame: bool,
    pub discontinuity: bool,
    pub data: Vec<u8>,
}

pub struct VideoPacketSource {
    input: FfmpegInput,
    stream_index: usize,
    time_base: ffmpeg::Rational,
    stream_info: VideoPacketStreamInfo,
}

enum VideoFrameOutput {
    DirectYuv420p,
    Rgba(ScalingContext),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputOpenPurpose {
    Probe,
    VideoDecode,
    AudioDecode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputOpenProfile {
    Default,
    RemoteHls,
}

struct FfmpegInput {
    inner: ffmpeg::format::context::Input,
    _interrupt: Option<FfmpegInputInterrupt>,
}

impl FfmpegInput {
    fn new(inner: ffmpeg::format::context::Input) -> Self {
        Self {
            inner,
            _interrupt: None,
        }
    }

    fn with_interrupt(
        inner: ffmpeg::format::context::Input,
        interrupt: FfmpegInputInterrupt,
    ) -> Self {
        Self {
            inner,
            _interrupt: Some(interrupt),
        }
    }
}

impl Deref for FfmpegInput {
    type Target = ffmpeg::format::context::Input;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for FfmpegInput {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

struct FfmpegInputInterrupt {
    flag: Arc<AtomicBool>,
}

impl FfmpegInputInterrupt {
    fn new(flag: Arc<AtomicBool>) -> Self {
        Self { flag }
    }

    fn callback(&self) -> ffmpeg::ffi::AVIOInterruptCB {
        ffmpeg::ffi::AVIOInterruptCB {
            callback: Some(ffmpeg_interrupt_callback),
            opaque: Arc::as_ptr(&self.flag).cast_mut().cast::<c_void>(),
        }
    }
}

extern "C" fn ffmpeg_interrupt_callback(opaque: *mut c_void) -> c_int {
    if opaque.is_null() {
        return 0;
    }

    let flag = unsafe { &*(opaque.cast::<AtomicBool>()) };
    i32::from(flag.load(Ordering::SeqCst))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HlsAudioRendition {
    group_id: String,
    uri: String,
    is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HlsVariantInfo {
    audio_group_id: Option<String>,
    uri: String,
}

#[derive(Debug, Clone, Default)]
struct ResolvedRemoteHlsSources {
    audio_rendition_uri: Option<String>,
    video_variant_uri: Option<String>,
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
    pub fn new() -> Result<Self> {
        ffmpeg::init().context("failed to initialize FFmpeg")?;

        Ok(Self { initialized: true })
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    pub fn supports_source(&self, source: &MediaSource) -> bool {
        match source.protocol() {
            MediaSourceProtocol::Dash => supports_input_format("dash"),
            MediaSourceProtocol::Hls => supports_input_format("hls"),
            _ => true,
        }
    }

    pub fn unsupported_source_reason(&self, source: &MediaSource) -> Option<String> {
        match source.protocol() {
            MediaSourceProtocol::Dash if !self.supports_source(source) => Some(
                "linked FFmpeg does not include the 'dash' demuxer; MPEG-DASH playback is unavailable in this build"
                    .to_owned(),
            ),
            MediaSourceProtocol::Hls if !self.supports_source(source) => Some(
                "linked FFmpeg does not include the 'hls' demuxer; HLS playback is unavailable in this build"
                    .to_owned(),
            ),
            _ => None,
        }
    }

    pub fn probe(&self, source: MediaSource) -> Result<MediaProbe> {
        self.probe_with_interrupt(source, None)
    }

    pub fn probe_with_interrupt(
        &self,
        source: MediaSource,
        interrupt_flag: Option<Arc<AtomicBool>>,
    ) -> Result<MediaProbe> {
        let input = open_media_input(&source, InputOpenPurpose::Probe, interrupt_flag)
            .with_context(|| format!("failed to open media source: {}", source.uri()))?;
        media_probe_from_input(&input, &source)
    }

    pub fn probe_audio_decode_source_with_interrupt(
        &self,
        source: MediaSource,
        interrupt_flag: Option<Arc<AtomicBool>>,
    ) -> Result<MediaProbe> {
        let audio_source = resolve_audio_decode_source(&source, interrupt_flag.clone())
            .unwrap_or_else(|error| {
                warn!(
                    source = source.uri(),
                    error = %error,
                    "failed to resolve remote HLS audio rendition playlist for probing; falling back to the original source"
                );
                source.clone()
            });
        let probe = self
            .probe_with_interrupt(audio_source, interrupt_flag)
            .with_context(|| format!("failed to probe media source: {}", source.uri()))?;

        Ok(MediaProbe { source, ..probe })
    }

    pub fn open_video_source(&self, source: MediaSource) -> Result<VideoFrameSource> {
        self.open_video_source_with_interrupt(source, None)
    }

    pub fn open_video_source_with_interrupt(
        &self,
        source: MediaSource,
        interrupt_flag: Option<Arc<AtomicBool>>,
    ) -> Result<VideoFrameSource> {
        let video_source = resolve_video_decode_source(&source, interrupt_flag.clone())
            .unwrap_or_else(|error| {
                warn!(
                    source = source.uri(),
                    error = %error,
                    "failed to resolve remote HLS video variant playlist; falling back to the original source"
                );
                source.clone()
            });
        let input = open_media_input(&video_source, InputOpenPurpose::VideoDecode, interrupt_flag)
            .with_context(|| format!("failed to open media source: {}", video_source.uri()))?;
        let stream = input
            .streams()
            .best(ffmpeg::media::Type::Video)
            .context("no video stream found in media source")?;
        let stream_index = stream.index();
        let time_base = stream.time_base();
        let fallback_frame_interval = frame_interval_from_stream(&stream);
        let parameters = stream.parameters();
        let (decoder, decode_info) = open_video_decoder(&parameters).with_context(|| {
            format!(
                "failed to open video decoder for media source {}",
                video_source.uri()
            )
        })?;
        let output =
            create_video_frame_output(&decoder).context("failed to create video frame output")?;

        Ok(VideoFrameSource {
            input,
            stream_index,
            time_base,
            fallback_frame_interval,
            fallback_start_time: Duration::ZERO,
            decoder,
            output,
            decode_info,
            decoded_frame_index: 0,
            end_of_input_sent: false,
        })
    }

    pub fn open_video_packet_source(&self, source: MediaSource) -> Result<VideoPacketSource> {
        self.open_video_packet_source_with_interrupt(source, None)
    }

    pub fn open_video_packet_source_with_interrupt(
        &self,
        source: MediaSource,
        interrupt_flag: Option<Arc<AtomicBool>>,
    ) -> Result<VideoPacketSource> {
        let video_source = resolve_video_decode_source(&source, interrupt_flag.clone())
            .unwrap_or_else(|error| {
                warn!(
                    source = source.uri(),
                    error = %error,
                    "failed to resolve remote HLS video variant playlist for packet demux; falling back to the original source"
                );
                source.clone()
            });
        let input = open_media_input(&video_source, InputOpenPurpose::VideoDecode, interrupt_flag)
            .with_context(|| format!("failed to open media source: {}", video_source.uri()))?;
        let stream = input
            .streams()
            .best(ffmpeg::media::Type::Video)
            .context("no video stream found in media source")?;
        let stream_index = stream.index();
        let time_base = stream.time_base();
        let stream_info = video_packet_stream_info(&stream)
            .context("failed to inspect compressed video stream")?;

        Ok(VideoPacketSource {
            input,
            stream_index,
            time_base,
            stream_info,
        })
    }

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
        if output_rate == 0 {
            anyhow::bail!("audio output sample rate must be greater than zero");
        }

        if output_channels == 0 {
            anyhow::bail!("audio output channel count must be greater than zero");
        }

        if !playback_rate.is_finite() || playback_rate <= 0.0 {
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
            playback_rate,
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
            &mut first_presentation_time,
            &mut samples,
        )?;
        flush_audio_filter(&mut filter_graph, &mut samples)?;

        Ok(DecodedAudioTrack {
            presentation_time: first_presentation_time.unwrap_or(Duration::ZERO),
            sample_rate: output_rate,
            channels: output_channels,
            playback_rate,
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
        mut on_probe: P,
        mut on_chunk: F,
    ) -> Result<()>
    where
        P: FnMut(MediaProbe) -> Result<()>,
        F: FnMut(Vec<f32>) -> Result<bool>,
    {
        if output_rate == 0 {
            anyhow::bail!("audio output sample rate must be greater than zero");
        }

        if output_channels == 0 {
            anyhow::bail!("audio output channel count must be greater than zero");
        }

        if !playback_rate.is_finite() || playback_rate <= 0.0 {
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
        let stream_parameters = stream.parameters();
        on_probe(media_probe_from_input(&input, &source)?)?;

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
            playback_rate,
        )?;

        for (stream, packet) in input.packets() {
            if stream.index() != stream_index {
                continue;
            }

            decoder
                .send_packet(&packet)
                .context("failed to send audio packet to decoder")?;
            if !drain_audio_frames_with_emitter(&mut decoder, &mut filter_graph, &mut on_chunk)? {
                return Ok(());
            }
        }

        decoder
            .send_eof()
            .context("failed to flush audio decoder")?;
        if !drain_audio_frames_with_emitter(&mut decoder, &mut filter_graph, &mut on_chunk)? {
            return Ok(());
        }
        flush_audio_filter_with_emitter(&mut filter_graph, &mut on_chunk)?;

        Ok(())
    }

    pub fn retime_audio_track(
        &self,
        source_track: &DecodedAudioTrack,
        playback_rate: f32,
    ) -> Result<DecodedAudioTrack> {
        self.retime_audio_track_range(source_track, playback_rate, 0..source_track.samples.len())
    }

    pub fn retime_audio_track_range(
        &self,
        source_track: &DecodedAudioTrack,
        playback_rate: f32,
        sample_range: Range<usize>,
    ) -> Result<DecodedAudioTrack> {
        if !playback_rate.is_finite() || playback_rate <= 0.0 {
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
                playback_rate,
                samples: Arc::from(Vec::<f32>::new()),
            });
        }

        if (source_track.playback_rate - playback_rate).abs() < 0.000_001
            && start_sample == 0
            && end_sample == source_track.samples.len()
        {
            return Ok(source_track.clone());
        }

        let mut samples = Vec::new();
        self.stream_retime_audio_track_range(
            source_track,
            playback_rate,
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
            playback_rate,
            samples: Arc::from(samples),
        })
    }

    pub fn stream_retime_audio_track_range<F>(
        &self,
        source_track: &DecodedAudioTrack,
        playback_rate: f32,
        sample_range: Range<usize>,
        mut on_chunk: F,
    ) -> Result<()>
    where
        F: FnMut(Vec<f32>) -> Result<bool>,
    {
        if !playback_rate.is_finite() || playback_rate <= 0.0 {
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
            playback_rate,
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
            if !emit_filtered_audio_frames(&mut filter_graph, &mut on_chunk)? {
                return Ok(());
            }
        }

        flush_audio_filter_with_emitter(&mut filter_graph, &mut on_chunk)?;
        Ok(())
    }
}

fn supports_input_format(name: &str) -> bool {
    let Ok(name) = CString::new(name) else {
        return false;
    };

    unsafe { !ffmpeg::ffi::av_find_input_format(name.as_ptr()).is_null() }
}

fn resolve_audio_decode_source(
    source: &MediaSource,
    interrupt_flag: Option<Arc<AtomicBool>>,
) -> Result<MediaSource> {
    if source.kind() != MediaSourceKind::Remote || source.protocol() != MediaSourceProtocol::Hls {
        return Ok(source.clone());
    }

    let Some(audio_rendition_uri) =
        resolve_remote_hls_audio_rendition_uri(source.uri(), interrupt_flag)?
    else {
        return Ok(source.clone());
    };

    if audio_rendition_uri != source.uri() {
        info!(
            source = source.uri(),
            audio_rendition_uri, "resolved remote HLS audio rendition playlist"
        );
        return Ok(MediaSource::new(audio_rendition_uri));
    }

    Ok(source.clone())
}

fn resolve_video_decode_source(
    source: &MediaSource,
    interrupt_flag: Option<Arc<AtomicBool>>,
) -> Result<MediaSource> {
    if source.kind() != MediaSourceKind::Remote || source.protocol() != MediaSourceProtocol::Hls {
        return Ok(source.clone());
    }

    let Some(video_variant_uri) =
        resolve_remote_hls_video_variant_uri(source.uri(), interrupt_flag)?
    else {
        return Ok(source.clone());
    };

    if video_variant_uri != source.uri() {
        info!(
            source = source.uri(),
            video_variant_uri, "resolved remote HLS video variant playlist"
        );
        return Ok(MediaSource::new(video_variant_uri));
    }

    Ok(source.clone())
}

fn resolve_remote_hls_audio_rendition_uri(
    manifest_uri: &str,
    interrupt_flag: Option<Arc<AtomicBool>>,
) -> Result<Option<String>> {
    Ok(resolve_remote_hls_sources(manifest_uri, interrupt_flag)?.audio_rendition_uri)
}

fn resolve_remote_hls_video_variant_uri(
    manifest_uri: &str,
    interrupt_flag: Option<Arc<AtomicBool>>,
) -> Result<Option<String>> {
    Ok(resolve_remote_hls_sources(manifest_uri, interrupt_flag)?.video_variant_uri)
}

fn resolve_remote_hls_sources(
    manifest_uri: &str,
    interrupt_flag: Option<Arc<AtomicBool>>,
) -> Result<ResolvedRemoteHlsSources> {
    if let Some(cached) = resolved_hls_source_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(manifest_uri)
        .cloned()
    {
        return Ok(cached);
    }

    let manifest_text = fetch_text_resource_via_ffmpeg(manifest_uri, interrupt_flag)
        .with_context(|| format!("failed to fetch remote HLS manifest: {manifest_uri}"))?;
    let resolved = resolve_hls_master_manifest_sources(manifest_uri, &manifest_text);

    let mut cache = resolved_hls_source_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if !cache.contains_key(manifest_uri) && cache.len() >= MAX_RESOLVED_HLS_SOURCE_CACHE_ENTRIES {
        cache.clear();
    }
    cache.insert(manifest_uri.to_owned(), resolved.clone());

    Ok(resolved)
}

fn resolved_hls_source_cache() -> &'static Mutex<HashMap<String, ResolvedRemoteHlsSources>> {
    static CACHE: OnceLock<Mutex<HashMap<String, ResolvedRemoteHlsSources>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn resolve_hls_master_manifest_sources(
    manifest_uri: &str,
    manifest_text: &str,
) -> ResolvedRemoteHlsSources {
    ResolvedRemoteHlsSources {
        audio_rendition_uri: select_hls_audio_rendition_uri(manifest_uri, manifest_text),
        video_variant_uri: select_hls_video_variant_uri(manifest_uri, manifest_text),
    }
}

fn fetch_text_resource_via_ffmpeg(
    uri: &str,
    interrupt_flag: Option<Arc<AtomicBool>>,
) -> Result<String> {
    let uri_cstr = CString::new(uri).context("resource URI contained an interior NUL byte")?;
    let interrupt = interrupt_flag.map(FfmpegInputInterrupt::new);
    let interrupt_callback = interrupt.as_ref().map(FfmpegInputInterrupt::callback);
    let interrupt_ptr = interrupt_callback
        .as_ref()
        .map(|callback| callback as *const _)
        .unwrap_or(ptr::null());
    let mut io_context = ptr::null_mut();
    let mut options = ffmpeg::Dictionary::new();
    options.set("rw_timeout", "15000000");
    let mut raw_options = unsafe { options.disown() };

    unsafe {
        let open_result = ffmpeg::ffi::avio_open2(
            &mut io_context,
            uri_cstr.as_ptr(),
            ffmpeg::ffi::AVIO_FLAG_READ,
            interrupt_ptr,
            &mut raw_options,
        );
        ffmpeg::Dictionary::own(raw_options);

        if open_result < 0 {
            return Err(anyhow::Error::new(ffmpeg::Error::from(open_result))
                .context(format!("failed to open FFmpeg IO for {uri}")));
        }

        let mut bytes = Vec::new();
        let mut buffer = [0u8; 8 * 1024];

        loop {
            let read_result =
                ffmpeg::ffi::avio_read(io_context, buffer.as_mut_ptr().cast(), buffer.len() as i32);

            if read_result == 0 || read_result == ffmpeg::ffi::AVERROR_EOF {
                break;
            }

            if read_result < 0 {
                ffmpeg::ffi::avio_closep(&mut io_context);
                return Err(anyhow::Error::new(ffmpeg::Error::from(read_result))
                    .context(format!("failed to read FFmpeg IO resource {uri}")));
            }

            bytes.extend_from_slice(&buffer[..read_result as usize]);
        }

        ffmpeg::ffi::avio_closep(&mut io_context);
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

fn select_hls_audio_rendition_uri(manifest_uri: &str, manifest_text: &str) -> Option<String> {
    let (audio_renditions, variants) = parse_hls_master_manifest(manifest_text);
    if audio_renditions.is_empty() {
        return None;
    }

    let preferred_group = variants
        .first()
        .and_then(|variant| variant.audio_group_id.as_deref());
    let selected = preferred_group
        .and_then(|group_id| choose_hls_audio_rendition(&audio_renditions, Some(group_id)))
        .or_else(|| choose_hls_audio_rendition(&audio_renditions, None))?;

    resolve_uri_relative_to(manifest_uri, &selected.uri)
}

fn select_hls_video_variant_uri(manifest_uri: &str, manifest_text: &str) -> Option<String> {
    let (_, variants) = parse_hls_master_manifest(manifest_text);
    let selected = variants.first()?;
    resolve_uri_relative_to(manifest_uri, &selected.uri)
}

fn parse_hls_master_manifest(manifest_text: &str) -> (Vec<HlsAudioRendition>, Vec<HlsVariantInfo>) {
    let mut audio_renditions = Vec::new();
    let mut variants = Vec::new();
    let mut pending_variant = None;

    for raw_line in manifest_text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(attributes) = line.strip_prefix("#EXT-X-MEDIA:") {
            let attributes = parse_hls_attribute_list(attributes);
            let media_type = attributes
                .get("TYPE")
                .map(|value| value.eq_ignore_ascii_case("AUDIO"))
                .unwrap_or(false);
            let Some(group_id) = attributes.get("GROUP-ID") else {
                continue;
            };
            let Some(uri) = attributes.get("URI") else {
                continue;
            };
            if !media_type {
                continue;
            }

            let is_default = attributes
                .get("DEFAULT")
                .map(|value| value.eq_ignore_ascii_case("YES"))
                .unwrap_or(false);
            audio_renditions.push(HlsAudioRendition {
                group_id: group_id.clone(),
                uri: uri.clone(),
                is_default,
            });
            continue;
        }

        if let Some(attributes) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            let attributes = parse_hls_attribute_list(attributes);
            pending_variant = Some(HlsVariantInfo {
                audio_group_id: attributes.get("AUDIO").cloned(),
                uri: String::new(),
            });
            continue;
        }

        if let Some(mut variant) = pending_variant.take() {
            if line.starts_with('#') {
                pending_variant = Some(variant);
                continue;
            }
            variant.uri = line.to_owned();
            variants.push(variant);
        }
    }

    (audio_renditions, variants)
}

fn choose_hls_audio_rendition<'a>(
    renditions: &'a [HlsAudioRendition],
    group_id: Option<&str>,
) -> Option<&'a HlsAudioRendition> {
    let candidates = renditions
        .iter()
        .filter(|rendition| group_id.is_none_or(|group| rendition.group_id == group));

    candidates
        .clone()
        .find(|rendition| rendition.is_default)
        .or_else(|| candidates.into_iter().next())
}

fn parse_hls_attribute_list(attributes: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in attributes.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            ',' if !in_quotes => {
                parse_hls_attribute_entry(&current, &mut values);
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    parse_hls_attribute_entry(&current, &mut values);
    values
}

fn parse_hls_attribute_entry(entry: &str, values: &mut HashMap<String, String>) {
    let trimmed = entry.trim();
    if trimmed.is_empty() {
        return;
    }

    let Some((key, value)) = trimmed.split_once('=') else {
        return;
    };
    let value = value.trim().trim_matches('"');
    values.insert(key.trim().to_owned(), value.to_owned());
}

fn resolve_uri_relative_to(base_uri: &str, reference: &str) -> Option<String> {
    let reference = reference.trim();
    if reference.is_empty() {
        return None;
    }

    if reference.contains("://") {
        return Some(reference.to_owned());
    }

    if reference.starts_with("//") {
        let (scheme, _) = base_uri.split_once("://")?;
        return Some(format!("{scheme}:{reference}"));
    }

    let base_uri = base_uri
        .split_once('#')
        .map(|(value, _)| value)
        .unwrap_or(base_uri);
    let base_uri = base_uri
        .split_once('?')
        .map(|(value, _)| value)
        .unwrap_or(base_uri);
    let (scheme, rest) = base_uri.split_once("://")?;
    let (authority, raw_path) = rest.split_once('/').unwrap_or((rest, ""));
    let base_path = format!("/{}", raw_path);
    let joined_path = if reference.starts_with('/') {
        reference.to_owned()
    } else {
        let base_dir = base_path
            .rsplit_once('/')
            .map(|(dir, _)| dir)
            .filter(|dir| !dir.is_empty())
            .unwrap_or("/");
        if base_dir.ends_with('/') {
            format!("{base_dir}{reference}")
        } else {
            format!("{base_dir}/{reference}")
        }
    };
    let normalized_path = normalize_url_path(&joined_path);

    Some(format!("{scheme}://{authority}{normalized_path}"))
}

fn normalize_url_path(path: &str) -> String {
    let mut segments = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            _ => segments.push(segment),
        }
    }

    if segments.is_empty() {
        "/".to_owned()
    } else {
        format!("/{}", segments.join("/"))
    }
}

fn open_media_input(
    source: &MediaSource,
    purpose: InputOpenPurpose,
    interrupt_flag: Option<Arc<AtomicBool>>,
) -> Result<FfmpegInput> {
    let profile = input_open_profile_for_source(source);
    if profile == InputOpenProfile::Default && interrupt_flag.is_none() {
        return ffmpeg::format::input(&source.uri())
            .map(FfmpegInput::new)
            .with_context(|| format!("failed to open media source: {}", source.uri()));
    }

    open_media_input_with_profile(source, purpose, profile, interrupt_flag)
}

fn open_media_input_with_profile(
    source: &MediaSource,
    purpose: InputOpenPurpose,
    profile: InputOpenProfile,
    interrupt_flag: Option<Arc<AtomicBool>>,
) -> Result<FfmpegInput> {
    let source_uri = source.uri();
    let source_uri_cstr =
        CString::new(source_uri).context("media source URI contained an interior NUL byte")?;
    let interrupt = interrupt_flag.map(FfmpegInputInterrupt::new);
    let interrupt_state = interrupt.as_ref().map(|interrupt| interrupt.flag.clone());
    let options = input_open_dictionary(profile, purpose);

    unsafe {
        let mut format_context = if interrupt.is_some() {
            ffmpeg::ffi::avformat_alloc_context()
        } else {
            ptr::null_mut()
        };

        if interrupt.is_some() && format_context.is_null() {
            anyhow::bail!("failed to allocate FFmpeg format context");
        }

        if let Some(interrupt) = interrupt.as_ref() {
            (*format_context).interrupt_callback = interrupt.callback();
        }

        let mut raw_options = options.disown();
        let open_started_at = Instant::now();
        let open_result = ffmpeg::ffi::avformat_open_input(
            &mut format_context,
            source_uri_cstr.as_ptr(),
            ptr::null_mut(),
            &mut raw_options,
        );
        let open_duration = open_started_at.elapsed();
        ffmpeg::Dictionary::own(raw_options);

        if open_result < 0 {
            if !format_context.is_null() {
                ffmpeg::ffi::avformat_close_input(&mut format_context);
            }
            log_input_open_failure(
                source,
                purpose,
                profile,
                open_duration,
                Duration::ZERO,
                interrupt_state.as_deref(),
                "avformat_open_input",
                open_result,
            );
            return Err(anyhow::Error::new(ffmpeg::Error::from(open_result))
                .context(format!("failed to open media source: {source_uri}")));
        }

        let stream_info_started_at = Instant::now();
        let stream_info_result =
            ffmpeg::ffi::avformat_find_stream_info(format_context, ptr::null_mut());
        let stream_info_duration = stream_info_started_at.elapsed();

        if stream_info_result < 0 {
            ffmpeg::ffi::avformat_close_input(&mut format_context);
            log_input_open_failure(
                source,
                purpose,
                profile,
                open_duration,
                stream_info_duration,
                interrupt_state.as_deref(),
                "avformat_find_stream_info",
                stream_info_result,
            );
            return Err(anyhow::Error::new(ffmpeg::Error::from(stream_info_result))
                .context(format!("failed to inspect media streams: {source_uri}")));
        }

        log_input_open_success(
            source,
            purpose,
            profile,
            open_duration,
            stream_info_duration,
            interrupt_state.as_deref(),
        );
        let input = ffmpeg::format::context::Input::wrap(format_context);
        Ok(match interrupt {
            Some(interrupt) => FfmpegInput::with_interrupt(input, interrupt),
            None => FfmpegInput::new(input),
        })
    }
}

fn input_open_profile_for_source(source: &MediaSource) -> InputOpenProfile {
    if source.kind() == MediaSourceKind::Remote && source.protocol() == MediaSourceProtocol::Hls {
        InputOpenProfile::RemoteHls
    } else {
        InputOpenProfile::Default
    }
}

fn input_open_dictionary(
    profile: InputOpenProfile,
    purpose: InputOpenPurpose,
) -> ffmpeg::Dictionary<'static> {
    let mut options = ffmpeg::Dictionary::new();

    for (key, value) in input_open_tuning_options(profile, purpose) {
        options.set(key, value);
    }

    options
}

fn input_open_tuning_options(
    profile: InputOpenProfile,
    purpose: InputOpenPurpose,
) -> &'static [(&'static str, &'static str)] {
    match (profile, purpose) {
        (InputOpenProfile::Default, _) => &[],
        (InputOpenProfile::RemoteHls, InputOpenPurpose::AudioDecode) => &[
            ("http_multiple", "0"),
            ("probesize", "524288"),
            ("formatprobesize", "524288"),
            ("analyzeduration", "2000000"),
            ("fpsprobesize", "4"),
            ("rw_timeout", "15000000"),
            ("allowed_media_types", "audio"),
        ],
        (InputOpenProfile::RemoteHls, _) => &[
            ("http_multiple", "0"),
            ("probesize", "524288"),
            ("formatprobesize", "524288"),
            ("analyzeduration", "2000000"),
            ("fpsprobesize", "4"),
            ("rw_timeout", "15000000"),
        ],
    }
}

fn log_input_open_success(
    source: &MediaSource,
    purpose: InputOpenPurpose,
    profile: InputOpenProfile,
    open_duration: Duration,
    stream_info_duration: Duration,
    interrupt_flag: Option<&AtomicBool>,
) {
    if profile == InputOpenProfile::Default {
        return;
    }

    info!(
        source = source.uri(),
        purpose = purpose.label(),
        profile = profile.label(),
        tuning = input_open_tuning_summary(profile, purpose),
        interrupted = interrupt_flag.is_some_and(|flag| flag.load(Ordering::SeqCst)),
        open_input_ms = open_duration.as_millis(),
        find_stream_info_ms = stream_info_duration.as_millis(),
        total_ms = open_duration.as_millis() + stream_info_duration.as_millis(),
        "opened FFmpeg media input"
    );
}

fn log_input_open_failure(
    source: &MediaSource,
    purpose: InputOpenPurpose,
    profile: InputOpenProfile,
    open_duration: Duration,
    stream_info_duration: Duration,
    interrupt_flag: Option<&AtomicBool>,
    phase: &'static str,
    error_code: i32,
) {
    if profile == InputOpenProfile::Default
        && !interrupt_flag.is_some_and(|flag| flag.load(Ordering::SeqCst))
    {
        return;
    }

    warn!(
        source = source.uri(),
        purpose = purpose.label(),
        profile = profile.label(),
        tuning = input_open_tuning_summary(profile, purpose),
        phase,
        interrupted = interrupt_flag.is_some_and(|flag| flag.load(Ordering::SeqCst)),
        open_input_ms = open_duration.as_millis(),
        find_stream_info_ms = stream_info_duration.as_millis(),
        error_code,
        error = %ffmpeg::Error::from(error_code),
        "failed to open FFmpeg media input"
    );
}

impl InputOpenPurpose {
    fn label(self) -> &'static str {
        match self {
            Self::Probe => "probe",
            Self::VideoDecode => "video_decode",
            Self::AudioDecode => "audio_decode",
        }
    }
}

impl InputOpenProfile {
    fn label(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::RemoteHls => "remote_hls",
        }
    }
}

fn input_open_tuning_summary(profile: InputOpenProfile, purpose: InputOpenPurpose) -> &'static str {
    match (profile, purpose) {
        (InputOpenProfile::Default, _) => "default",
        (InputOpenProfile::RemoteHls, InputOpenPurpose::AudioDecode) => {
            "http_multiple=0,probesize=524288,formatprobesize=524288,analyzeduration=2000000,fpsprobesize=4,rw_timeout=15000000,allowed_media_types=audio"
        }
        (InputOpenProfile::RemoteHls, _) => {
            "http_multiple=0,probesize=524288,formatprobesize=524288,analyzeduration=2000000,fpsprobesize=4,rw_timeout=15000000"
        }
    }
}

impl VideoFrameSource {
    pub fn decode_info(&self) -> &VideoDecodeInfo {
        &self.decode_info
    }

    pub fn media_probe(&self, source: &MediaSource) -> Result<MediaProbe> {
        media_probe_from_input(&self.input, source)
    }

    pub fn next_frame(&mut self) -> Result<Option<DecodedVideoFrame>> {
        loop {
            if let Some(frame) = self.try_receive_frame()? {
                return Ok(Some(frame));
            }

            if self.feed_next_packet()? {
                continue;
            }

            if self.end_of_input_sent {
                return Ok(None);
            }

            self.decoder
                .send_eof()
                .context("failed to flush video decoder")?;
            self.end_of_input_sent = true;
        }
    }

    pub fn seek_to(&mut self, position: Duration) -> Result<Option<DecodedVideoFrame>> {
        let timestamp = duration_to_av_timestamp(position);
        self.input.seek(timestamp, ..timestamp).with_context(|| {
            format!(
                "failed to seek video source to {:.3}s",
                position.as_secs_f64()
            )
        })?;
        self.decoder.flush();
        self.decoded_frame_index = 0;
        self.end_of_input_sent = false;
        self.fallback_start_time = position;

        loop {
            let Some(frame) = self.next_frame()? else {
                return Ok(None);
            };

            if frame
                .presentation_time
                .saturating_add(self.fallback_frame_interval)
                < position
            {
                continue;
            }

            return Ok(Some(frame));
        }
    }

    fn try_receive_frame(&mut self) -> Result<Option<DecodedVideoFrame>> {
        let mut decoded = Video::empty();
        if self.decoder.receive_frame(&mut decoded).is_err() {
            return Ok(None);
        }

        let presentation_time = decoded
            .timestamp()
            .or(decoded.pts())
            .and_then(|timestamp| timestamp_to_duration(timestamp, self.time_base))
            .unwrap_or_else(|| self.fallback_timestamp());
        self.decoded_frame_index += 1;

        let (pixel_format, width, height, bytes_per_row, bytes) = match &mut self.output {
            VideoFrameOutput::DirectYuv420p => (
                VideoPixelFormat::Yuv420p,
                decoded.width(),
                decoded.height(),
                decoded.width(),
                copy_yuv420p_bytes(&decoded),
            ),
            VideoFrameOutput::Rgba(scaler) => {
                let mut rgba_frame = Video::empty();
                scaler
                    .run(&decoded, &mut rgba_frame)
                    .context("failed to convert decoded frame to RGBA")?;
                (
                    VideoPixelFormat::Rgba8888,
                    rgba_frame.width(),
                    rgba_frame.height(),
                    rgba_frame.width().saturating_mul(4),
                    copy_rgba_bytes(&rgba_frame),
                )
            }
        };

        Ok(Some(DecodedVideoFrame {
            presentation_time,
            width,
            height,
            bytes_per_row,
            pixel_format,
            bytes,
        }))
    }

    fn feed_next_packet(&mut self) -> Result<bool> {
        for (stream, packet) in self.input.packets() {
            if stream.index() != self.stream_index {
                continue;
            }

            self.decoder
                .send_packet(&packet)
                .context("failed to send video packet to decoder")?;
            return Ok(true);
        }

        Ok(false)
    }

    fn fallback_timestamp(&self) -> Duration {
        self.fallback_start_time
            + self
                .fallback_frame_interval
                .saturating_mul(self.decoded_frame_index as u32)
    }
}

impl VideoPacketSource {
    pub fn stream_info(&self) -> &VideoPacketStreamInfo {
        &self.stream_info
    }

    pub fn next_packet(&mut self) -> Result<Option<CompressedVideoPacket>> {
        for (stream, packet) in self.input.packets() {
            if stream.index() != self.stream_index {
                continue;
            }

            let data = packet.data().map(<[u8]>::to_vec).unwrap_or_default();
            let stream_index = u32::try_from(self.stream_index).unwrap_or(u32::MAX);
            return Ok(Some(CompressedVideoPacket {
                pts_us: packet
                    .pts()
                    .and_then(|timestamp| timestamp_to_micros(timestamp, self.time_base)),
                dts_us: packet
                    .dts()
                    .and_then(|timestamp| timestamp_to_micros(timestamp, self.time_base)),
                duration_us: timestamp_to_micros(packet.duration(), self.time_base)
                    .filter(|duration| *duration > 0),
                stream_index,
                key_frame: packet.is_key(),
                discontinuity: false,
                data,
            }));
        }

        Ok(None)
    }

    pub fn seek_to(&mut self, position: Duration) -> Result<()> {
        let timestamp = duration_to_av_timestamp(position);
        self.input.seek(timestamp, ..timestamp).with_context(|| {
            format!(
                "failed to seek video packet source to {:.3}s",
                position.as_secs_f64()
            )
        })
    }
}

fn open_video_decoder(
    parameters: &ffmpeg::codec::Parameters,
) -> Result<(ffmpeg::decoder::Video, VideoDecodeInfo)> {
    let decoder = open_video_decoder_as(
        parameters,
        codec::decoder::find(parameters.id()),
        "default software decoder",
    )
    .context("failed to open software video decoder")?;
    let decode_info = software_video_decode_info(parameters, &decoder);
    Ok((decoder, decode_info))
}

fn open_video_decoder_as<D>(
    parameters: &ffmpeg::codec::Parameters,
    codec: D,
    decoder_label: &str,
) -> Result<ffmpeg::decoder::Video>
where
    D: codec::traits::Decoder,
{
    let context_decoder = ffmpeg::codec::context::Context::from_parameters(parameters.clone())
        .with_context(|| format!("failed to create codec context for {decoder_label}"))?;
    context_decoder
        .decoder()
        .open_as(codec)
        .and_then(|opened| opened.video())
        .with_context(|| format!("failed to open {decoder_label}"))
}

fn software_video_decode_info(
    parameters: &ffmpeg::codec::Parameters,
    decoder: &ffmpeg::decoder::Video,
) -> VideoDecodeInfo {
    VideoDecodeInfo {
        selected_mode: VideoDecoderMode::Software,
        hardware_available: false,
        hardware_backend: None,
        decoder_name: decoder
            .codec()
            .map(|codec| codec.name().to_owned())
            .unwrap_or_else(|| parameters.id().name().to_owned()),
        fallback_reason: None,
    }
}

fn create_video_frame_output(decoder: &ffmpeg::decoder::Video) -> Result<VideoFrameOutput> {
    if decoder.format() == Pixel::YUV420P {
        return Ok(VideoFrameOutput::DirectYuv420p);
    }

    Ok(VideoFrameOutput::Rgba(
        ScalingContext::get(
            decoder.format(),
            decoder.width(),
            decoder.height(),
            Pixel::RGBA,
            decoder.width(),
            decoder.height(),
            Flags::BILINEAR,
        )
        .context("failed to create RGBA scaler")?,
    ))
}

fn duration_from_micros(duration: i64) -> Option<Duration> {
    if duration <= 0 {
        return None;
    }

    Some(Duration::from_secs_f64(
        duration as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE),
    ))
}

fn duration_to_av_timestamp(duration: Duration) -> i64 {
    duration
        .as_micros()
        .min(i64::MAX as u128)
        .try_into()
        .expect("value clamped to i64")
}

fn video_stream_probe(stream: ffmpeg::Stream<'_>) -> Result<VideoStreamProbe> {
    let codec = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
        .context("failed to create decoder context for best video stream")?;
    let codec_id = format!("{:?}", codec.id());
    let decoder = codec
        .decoder()
        .video()
        .context("failed to inspect best video stream")?;

    Ok(VideoStreamProbe {
        index: stream.index(),
        codec: codec_id,
        width: decoder.width(),
        height: decoder.height(),
        frame_rate: rational_to_f64(stream.avg_frame_rate())
            .or_else(|| rational_to_f64(stream.rate())),
    })
}

fn video_packet_stream_info(stream: &ffmpeg::Stream<'_>) -> Result<VideoPacketStreamInfo> {
    let parameters = stream.parameters();
    let codec = ffmpeg::codec::context::Context::from_parameters(parameters.clone())
        .context("failed to create decoder context for compressed video stream")?;
    let codec_id = format!("{:?}", codec.id());
    let decoder = codec
        .decoder()
        .video()
        .context("failed to inspect compressed video stream")?;

    Ok(VideoPacketStreamInfo {
        stream_index: stream.index(),
        codec: codec_id,
        extradata: codec_parameters_extradata(&parameters),
        width: Some(decoder.width()).filter(|width| *width > 0),
        height: Some(decoder.height()).filter(|height| *height > 0),
        frame_rate: rational_to_f64(stream.avg_frame_rate())
            .or_else(|| rational_to_f64(stream.rate())),
    })
}

fn codec_parameters_extradata(parameters: &ffmpeg::codec::Parameters) -> Vec<u8> {
    // SAFETY: `parameters` is owned by FFmpeg and remains valid for this call;
    // extradata is copied into an owned Vec before returning.
    unsafe {
        let parameters = parameters.as_ptr();
        if parameters.is_null()
            || (*parameters).extradata.is_null()
            || (*parameters).extradata_size <= 0
        {
            return Vec::new();
        }
        let len = usize::try_from((*parameters).extradata_size).unwrap_or_default();
        std::slice::from_raw_parts((*parameters).extradata, len).to_vec()
    }
}

fn audio_stream_probe(stream: ffmpeg::Stream<'_>) -> Result<AudioStreamProbe> {
    let codec = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
        .context("failed to create decoder context for best audio stream")?;
    let codec_id = format!("{:?}", codec.id());
    let decoder = codec
        .decoder()
        .audio()
        .context("failed to inspect best audio stream")?;

    Ok(AudioStreamProbe {
        index: stream.index(),
        codec: codec_id,
        sample_rate: decoder.rate(),
        channels: decoder.channels(),
    })
}

fn media_probe_from_input(
    input: &ffmpeg::format::context::Input,
    source: &MediaSource,
) -> Result<MediaProbe> {
    let duration = duration_from_micros(input.duration());
    let bit_rate = u64::try_from(input.bit_rate())
        .ok()
        .filter(|bit_rate| *bit_rate > 0);

    let mut audio_streams = 0usize;
    let mut video_streams = 0usize;
    for stream in input.streams() {
        match stream.parameters().medium() {
            ffmpeg::media::Type::Audio => audio_streams += 1,
            ffmpeg::media::Type::Video => video_streams += 1,
            _ => {}
        }
    }

    let best_video = input
        .streams()
        .best(ffmpeg::media::Type::Video)
        .map(video_stream_probe)
        .transpose()?;
    let best_audio = input
        .streams()
        .best(ffmpeg::media::Type::Audio)
        .map(audio_stream_probe)
        .transpose()?;

    Ok(MediaProbe {
        source: source.clone(),
        duration,
        bit_rate,
        audio_streams,
        video_streams,
        best_video,
        best_audio,
    })
}

fn rational_to_f64(value: ffmpeg::Rational) -> Option<f64> {
    if value.numerator() <= 0 || value.denominator() <= 0 {
        return None;
    }

    Some(f64::from(value))
}

fn timestamp_to_duration(timestamp: i64, time_base: ffmpeg::Rational) -> Option<Duration> {
    let seconds = (timestamp as f64) * f64::from(time_base);
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }

    Some(Duration::from_secs_f64(seconds))
}

fn timestamp_to_micros(timestamp: i64, time_base: ffmpeg::Rational) -> Option<i64> {
    let numerator = i128::from(time_base.numerator());
    let denominator = i128::from(time_base.denominator());
    if denominator <= 0 {
        return None;
    }
    let value = i128::from(timestamp)
        .saturating_mul(numerator)
        .saturating_mul(1_000_000)
        / denominator;
    Some(value.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64)
}

fn frame_interval_from_stream(stream: &ffmpeg::Stream<'_>) -> Duration {
    let frame_rate = rational_to_f64(stream.avg_frame_rate())
        .or_else(|| rational_to_f64(stream.rate()))
        .filter(|value| *value > 0.0)
        .unwrap_or(30.0);

    Duration::from_secs_f64(1.0 / frame_rate)
}

fn copy_rgba_bytes(frame: &Video) -> Vec<u8> {
    let row_bytes = (frame.width() * 4) as usize;
    let stride = frame.stride(0);
    let height = frame.height() as usize;
    let data = frame.data(0);
    let mut bytes = Vec::with_capacity(row_bytes * height);

    for row in 0..height {
        let offset = row * stride;
        bytes.extend_from_slice(&data[offset..offset + row_bytes]);
    }

    bytes
}

fn copy_yuv420p_bytes(frame: &Video) -> Vec<u8> {
    let width = frame.width() as usize;
    let height = frame.height() as usize;
    let chroma_width = width.div_ceil(2);
    let chroma_height = height.div_ceil(2);
    let mut bytes = Vec::with_capacity(
        width
            .saturating_mul(height)
            .saturating_add(chroma_width.saturating_mul(chroma_height).saturating_mul(2)),
    );

    copy_plane_bytes(frame.data(0), frame.stride(0), width, height, &mut bytes);
    copy_plane_bytes(
        frame.data(1),
        frame.stride(1),
        chroma_width,
        chroma_height,
        &mut bytes,
    );
    copy_plane_bytes(
        frame.data(2),
        frame.stride(2),
        chroma_width,
        chroma_height,
        &mut bytes,
    );

    bytes
}

fn copy_plane_bytes(
    data: &[u8],
    stride: usize,
    row_bytes: usize,
    height: usize,
    out: &mut Vec<u8>,
) {
    for row in 0..height {
        let offset = row.saturating_mul(stride);
        out.extend_from_slice(&data[offset..offset + row_bytes]);
    }
}

fn drain_audio_frames(
    decoder: &mut ffmpeg::decoder::Audio,
    filter_graph: &mut filter::Graph,
    time_base: ffmpeg::Rational,
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
        collect_filtered_audio_frames(filter_graph, samples)?;
    }
}

fn drain_audio_frames_with_emitter<F>(
    decoder: &mut ffmpeg::decoder::Audio,
    filter_graph: &mut filter::Graph,
    emit: &mut F,
) -> Result<bool>
where
    F: FnMut(Vec<f32>) -> Result<bool>,
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
        if !emit_filtered_audio_frames(filter_graph, emit)? {
            return Ok(false);
        }
    }
}

fn collect_filtered_audio_frames(
    filter_graph: &mut filter::Graph,
    samples: &mut Vec<f32>,
) -> Result<()> {
    emit_filtered_audio_frames(filter_graph, &mut |chunk| {
        samples.extend(chunk);
        Ok(true)
    })
    .map(|_| ())
}

fn flush_audio_filter(filter_graph: &mut filter::Graph, samples: &mut Vec<f32>) -> Result<()> {
    flush_audio_filter_with_emitter(filter_graph, |chunk| {
        samples.extend(chunk);
        Ok(true)
    })
}

fn flush_audio_filter_with_emitter<F>(filter_graph: &mut filter::Graph, mut emit: F) -> Result<()>
where
    F: FnMut(Vec<f32>) -> Result<bool>,
{
    filter_graph
        .get("in")
        .context("audio filter graph did not expose an input node")?
        .source()
        .flush()
        .context("failed to flush the audio filter graph")?;
    emit_filtered_audio_frames(filter_graph, &mut emit).map(|_| ())
}

fn emit_filtered_audio_frames<F>(filter_graph: &mut filter::Graph, emit: &mut F) -> Result<bool>
where
    F: FnMut(Vec<f32>) -> Result<bool>,
{
    let mut filtered = Audio::empty();
    while filter_graph
        .get("out")
        .context("audio filter graph did not expose an output node")?
        .sink()
        .frame(&mut filtered)
        .is_ok()
    {
        if !emit(copy_interleaved_f32_samples(&filtered)?)? {
            return Ok(false);
        }
    }

    Ok(true)
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
    for chunk in bytes.chunks_exact(size_of::<f32>()) {
        let sample = f32::from_ne_bytes(
            chunk
                .try_into()
                .expect("chunks_exact always yields 4-byte slices"),
        );
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

    for (chunk, sample) in target
        .chunks_exact_mut(size_of::<f32>())
        .zip(samples.iter())
    {
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
    playback_rate: f32,
) -> Result<filter::Graph> {
    let input_layout = normalized_channel_layout(decoder.channel_layout(), decoder.channels());
    build_audio_filter_graph_for_spec(
        decoder.format(),
        time_base,
        decoder.rate(),
        input_layout,
        output_rate,
        output_layout,
        playback_rate,
    )
}

fn build_audio_filter_graph_for_spec(
    input_format: Sample,
    time_base: ffmpeg::Rational,
    input_rate: u32,
    input_layout: ffmpeg::ChannelLayout,
    output_rate: u32,
    output_layout: ffmpeg::ChannelLayout,
    playback_rate: f32,
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

    let filter_spec = audio_filter_spec(playback_rate, output_rate, output_layout);
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

fn audio_filter_spec(
    playback_rate: f32,
    output_rate: u32,
    output_layout: ffmpeg::ChannelLayout,
) -> String {
    let sample_format = Sample::F32(SampleType::Packed).name();
    format!(
        "{},aresample={},aformat=sample_fmts={}:channel_layouts=0x{:x}",
        playback_rate_filter_chain(playback_rate),
        output_rate,
        sample_format,
        output_layout.bits(),
    )
}

fn playback_rate_filter_chain(playback_rate: f32) -> String {
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

#[cfg(test)]
mod tests {
    use super::{
        DecodedAudioTrack, FfmpegInputInterrupt, InputOpenProfile, InputOpenPurpose,
        ffmpeg_interrupt_callback, input_open_profile_for_source, input_open_tuning_options,
        input_open_tuning_summary, parse_hls_master_manifest, playback_rate_filter_chain,
        resolve_hls_master_manifest_sources, resolve_uri_relative_to,
        select_hls_audio_rendition_uri, select_hls_video_variant_uri, supports_input_format,
    };
    use player_core::MediaSource;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    #[test]
    fn playback_rate_filter_spec_chains_high_rates() {
        assert_eq!(
            playback_rate_filter_chain(3.0),
            "atempo=2.000000,atempo=1.500000"
        );
    }

    #[test]
    fn decoded_audio_track_maps_media_time_across_playback_rates() {
        let track = DecodedAudioTrack {
            presentation_time: Duration::from_secs(2),
            sample_rate: 48_000,
            channels: 2,
            playback_rate: 2.0,
            samples: Arc::from(vec![0.0; 48_000 * 2 * 4]),
        };

        let offset = track.sample_offset_for_position(Duration::from_secs(6));
        assert_eq!(offset, 48_000 * 2 * 2);
        assert_eq!(
            track.media_time_for_sample_offset(offset),
            Duration::from_secs(6)
        );
    }

    #[test]
    fn supports_input_format_reports_known_and_unknown_demuxers() {
        assert!(supports_input_format("mov"));
        assert!(!supports_input_format("vesper-not-a-real-demuxer"));
    }

    #[test]
    fn remote_hls_sources_use_tuned_input_profile() {
        assert_eq!(
            input_open_profile_for_source(&MediaSource::new(
                "https://example.com/live/master.m3u8"
            )),
            InputOpenProfile::RemoteHls
        );
        assert_eq!(
            input_open_profile_for_source(&MediaSource::new("https://example.com/video.mp4")),
            InputOpenProfile::Default
        );
        assert_eq!(
            input_open_profile_for_source(&MediaSource::new("/tmp/video.mp4")),
            InputOpenProfile::Default
        );
    }

    #[test]
    fn remote_hls_audio_decode_tuning_is_audio_only() {
        assert!(
            input_open_tuning_summary(InputOpenProfile::RemoteHls, InputOpenPurpose::AudioDecode)
                .contains("allowed_media_types=audio")
        );
        assert!(
            !input_open_tuning_summary(InputOpenProfile::RemoteHls, InputOpenPurpose::VideoDecode,)
                .contains("allowed_media_types=audio")
        );
    }

    #[test]
    fn remote_hls_tuning_options_keep_audio_only_on_audio_decode() {
        let audio_options =
            input_open_tuning_options(InputOpenProfile::RemoteHls, InputOpenPurpose::AudioDecode);
        let video_options =
            input_open_tuning_options(InputOpenProfile::RemoteHls, InputOpenPurpose::VideoDecode);

        assert!(audio_options.contains(&("allowed_media_types", "audio")));
        assert!(!video_options.contains(&("allowed_media_types", "audio")));
        assert!(video_options.contains(&("rw_timeout", "15000000")));
        assert!(
            input_open_tuning_options(InputOpenProfile::Default, InputOpenPurpose::Probe)
                .is_empty()
        );
    }

    #[test]
    fn ffmpeg_interrupt_callback_observes_shared_cancel_flag() {
        let flag = Arc::new(AtomicBool::new(false));
        let interrupt = FfmpegInputInterrupt::new(flag.clone());
        let callback = interrupt.callback();
        let opaque = callback.opaque;

        assert_eq!(ffmpeg_interrupt_callback(opaque), 0);
        flag.store(true, Ordering::SeqCst);
        assert_eq!(ffmpeg_interrupt_callback(opaque), 1);
    }

    #[test]
    fn hls_master_parser_extracts_audio_renditions_and_variant_groups() {
        let manifest = r#"
#EXTM3U
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud-main",NAME="English",DEFAULT=YES,URI="a1/prog_index.m3u8"
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud-main",NAME="Dolby",URI="a2/prog_index.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=2400000,AUDIO="aud-main"
v1/prog_index.m3u8
"#;

        let (audio_renditions, variants) = parse_hls_master_manifest(manifest);
        assert_eq!(audio_renditions.len(), 2);
        assert_eq!(variants.len(), 1);
        assert_eq!(variants[0].audio_group_id.as_deref(), Some("aud-main"));
        assert_eq!(variants[0].uri, "v1/prog_index.m3u8");
        assert!(audio_renditions[0].is_default);
        assert_eq!(audio_renditions[0].uri, "a1/prog_index.m3u8");
    }

    #[test]
    fn hls_audio_rendition_selection_resolves_relative_uri_against_master_manifest() {
        let manifest = r#"
#EXTM3U
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud-main",NAME="English",DEFAULT=YES,URI="a1/prog_index.m3u8"
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud-main",NAME="Dolby",URI="a2/prog_index.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=2400000,AUDIO="aud-main"
v1/prog_index.m3u8
"#;

        let selected =
            select_hls_audio_rendition_uri("https://example.com/live/master.m3u8", manifest);

        assert_eq!(
            selected.as_deref(),
            Some("https://example.com/live/a1/prog_index.m3u8")
        );
    }

    #[test]
    fn hls_video_variant_selection_resolves_relative_uri_against_master_manifest() {
        let manifest = r#"
#EXTM3U
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud-main",NAME="English",DEFAULT=YES,URI="a1/prog_index.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=2400000,AUDIO="aud-main"
v1/prog_index.m3u8
"#;

        let selected =
            select_hls_video_variant_uri("https://example.com/live/master.m3u8", manifest);

        assert_eq!(
            selected.as_deref(),
            Some("https://example.com/live/v1/prog_index.m3u8")
        );
    }

    #[test]
    fn hls_master_resolution_computes_audio_and_video_sources_once() {
        let manifest = r#"
#EXTM3U
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud-main",NAME="English",DEFAULT=YES,URI="a1/prog_index.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=2400000,AUDIO="aud-main"
v1/prog_index.m3u8
"#;

        let resolved =
            resolve_hls_master_manifest_sources("https://example.com/live/master.m3u8", manifest);

        assert_eq!(
            resolved.audio_rendition_uri.as_deref(),
            Some("https://example.com/live/a1/prog_index.m3u8")
        );
        assert_eq!(
            resolved.video_variant_uri.as_deref(),
            Some("https://example.com/live/v1/prog_index.m3u8")
        );
    }

    #[test]
    fn relative_uri_resolver_normalizes_parent_segments() {
        let resolved = resolve_uri_relative_to(
            "https://example.com/live/master/master.m3u8",
            "../audio/a1/prog_index.m3u8",
        );

        assert_eq!(
            resolved.as_deref(),
            Some("https://example.com/live/audio/a1/prog_index.m3u8")
        );
    }
}
