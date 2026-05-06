use anyhow::{Context, Result};
use ffmpeg_next as ffmpeg;
use player_model::MediaSource;

use crate::time::{duration_from_micros, rational_to_f64};
use crate::{AudioStreamProbe, MediaProbe, VideoPacketStreamInfo, VideoStreamProbe};

pub(crate) fn video_packet_stream_info(
    stream: &ffmpeg::Stream<'_>,
) -> Result<VideoPacketStreamInfo> {
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

pub(crate) fn media_probe_from_input(
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
