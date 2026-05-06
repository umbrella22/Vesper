use std::time::Duration;

use anyhow::{Context, Result};
use ffmpeg::codec;
use ffmpeg::format::Pixel;
use ffmpeg::software::scaling::{context::Context as ScalingContext, flag::Flags};
use ffmpeg::util::frame::video::Video;
use ffmpeg_next as ffmpeg;
use player_model::MediaSource;

use crate::probe::media_probe_from_input;
use crate::time::{duration_to_av_timestamp, timestamp_to_duration};
use crate::{
    DecodedVideoFrame, MediaProbe, VideoDecodeInfo, VideoDecoderMode, VideoFrameSource,
    VideoPixelFormat,
};

pub(crate) enum VideoFrameOutput {
    DirectYuv420p,
    Rgba(ScalingContext),
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

pub(crate) fn open_video_decoder(
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

pub(crate) fn create_video_frame_output(
    decoder: &ffmpeg::decoder::Video,
) -> Result<VideoFrameOutput> {
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
