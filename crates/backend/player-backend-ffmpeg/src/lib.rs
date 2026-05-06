#![allow(clippy::new_ret_no_self, clippy::too_many_arguments)]

mod audio;
mod buffered;
mod hls;
mod input;
mod packet;
mod probe;
mod time;
mod video;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use anyhow::{Context, Result};
use ffmpeg_next as ffmpeg;
use hls::{resolve_audio_decode_source, resolve_video_decode_source};
use input::{FfmpegInput, InputOpenPurpose, open_media_input, supports_input_format};
use player_model::{MediaSource, MediaSourceProtocol};
use probe::{media_probe_from_input, video_packet_stream_info};
use time::frame_interval_from_stream;
use tracing::warn;
use video::{VideoFrameOutput, create_video_frame_output, open_video_decoder};

pub use buffered::{BufferedFramePoll, BufferedVideoSource, BufferedVideoSourceBootstrap};
pub use player_model::{DecodedVideoFrame, VideoPixelFormat};

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
    pub(crate) input: FfmpegInput,
    pub(crate) stream_index: usize,
    pub(crate) time_base: ffmpeg::Rational,
    pub(crate) fallback_frame_interval: Duration,
    pub(crate) fallback_start_time: Duration,
    pub(crate) decoder: ffmpeg::decoder::Video,
    pub(crate) output: VideoFrameOutput,
    pub(crate) decode_info: VideoDecodeInfo,
    pub(crate) decoded_frame_index: u64,
    pub(crate) end_of_input_sent: bool,
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
    pub(crate) input: FfmpegInput,
    pub(crate) stream_index: usize,
    pub(crate) time_base: ffmpeg::Rational,
    pub(crate) stream_info: VideoPacketStreamInfo,
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
}

#[cfg(test)]
mod tests {
    use super::DecodedAudioTrack;
    use super::audio::playback_rate_filter_chain;
    use super::hls::{
        parse_hls_master_manifest, resolve_hls_master_manifest_sources, resolve_uri_relative_to,
        select_hls_audio_rendition_uri, select_hls_video_variant_uri,
    };
    use super::input::{
        FfmpegInputInterrupt, InputOpenProfile, InputOpenPurpose, ffmpeg_interrupt_callback,
        input_open_profile_for_source, input_open_tuning_options, input_open_tuning_summary,
        supports_input_format,
    };
    use player_model::MediaSource;
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
