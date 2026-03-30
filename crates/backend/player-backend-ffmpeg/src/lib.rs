mod buffered;

use std::mem::size_of;
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;
use std::ffi::CString;

use anyhow::{Context, Result};
use ffmpeg::codec;
use ffmpeg::filter;
use ffmpeg::format::Pixel;
use ffmpeg::format::sample::{Sample, Type as SampleType};
use ffmpeg::software::scaling::{context::Context as ScalingContext, flag::Flags};
use ffmpeg::util::frame::audio::Audio;
use ffmpeg::util::frame::video::Video;
use ffmpeg_next as ffmpeg;
use player_core::{MediaSource, MediaSourceProtocol};

pub use buffered::{BufferedFramePoll, BufferedVideoSource, BufferedVideoSourceBootstrap};
pub use player_core::{DecodedVideoFrame, VideoPixelFormat};

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
    input: ffmpeg::format::context::Input,
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

enum VideoFrameOutput {
    DirectYuv420p,
    Rgba(ScalingContext),
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
        let input = ffmpeg::format::input(&source.uri())
            .with_context(|| format!("failed to open media source: {}", source.uri()))?;
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
            source,
            duration,
            bit_rate,
            audio_streams,
            video_streams,
            best_video,
            best_audio,
        })
    }

    pub fn open_video_source(&self, source: MediaSource) -> Result<VideoFrameSource> {
        let input = ffmpeg::format::input(&source.uri())
            .with_context(|| format!("failed to open media source: {}", source.uri()))?;
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
                source.uri()
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

    pub fn decode_audio_track(
        &self,
        source: MediaSource,
        output_rate: u32,
        output_channels: u16,
    ) -> Result<DecodedAudioTrack> {
        self.decode_audio_track_with_playback_rate(source, output_rate, output_channels, 1.0)
    }

    pub fn decode_audio_track_with_playback_rate(
        &self,
        source: MediaSource,
        output_rate: u32,
        output_channels: u16,
        playback_rate: f32,
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

        let mut input = ffmpeg::format::input(&source.uri())
            .with_context(|| format!("failed to open media source: {}", source.uri()))?;
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

impl VideoFrameSource {
    pub fn decode_info(&self) -> &VideoDecodeInfo {
        &self.decode_info
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
    use super::{DecodedAudioTrack, playback_rate_filter_chain, supports_input_format};
    use std::sync::Arc;
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
}
