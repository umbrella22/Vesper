#![allow(clippy::new_ret_no_self, clippy::too_many_arguments)]
#![warn(clippy::undocumented_unsafe_blocks)]

mod audio;
mod buffered;
mod clock;
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
pub use clock::{AudioMasterClock, MasterClock};
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

/// A bounded, timestamped PCM chunk emitted by the FFmpeg audio route.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedAudioChunk {
    /// Media presentation time for the first frame in this chunk.
    pub presentation_time: Option<Duration>,
    /// Media duration represented by this chunk at the requested playback rate.
    pub duration: Duration,
    /// Output sample rate in samples per second.
    pub sample_rate: u32,
    /// Number of interleaved output channels.
    pub channels: u16,
    /// Interleaved F32 PCM samples.
    pub samples: Vec<f32>,
    /// Whether this chunk starts a new decode/seek discontinuity.
    pub discontinuity: bool,
}

impl DecodedAudioChunk {
    /// Validates the backend-to-sink PCM handoff contract.
    pub fn validate(&self) -> Result<()> {
        if self.sample_rate == 0 {
            anyhow::bail!("decoded audio chunk sample rate must be greater than zero");
        }
        if self.channels == 0 {
            anyhow::bail!("decoded audio chunk channel count must be greater than zero");
        }
        if !self
            .samples
            .len()
            .is_multiple_of(usize::from(self.channels))
        {
            anyhow::bail!(
                "decoded audio chunk sample count {} is not divisible by channel count {}",
                self.samples.len(),
                self.channels
            );
        }
        if self.duration.is_zero() && !self.samples.is_empty() {
            anyhow::bail!("decoded audio chunk with samples must have a non-zero duration");
        }
        Ok(())
    }
}

/// Typed failure category for the FFmpeg streaming audio boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioStreamError {
    InvalidOutputRate,
    InvalidOutputChannels,
    InvalidPlaybackRate,
    InvalidChunk {
        message: String,
    },
    Backend {
        operation: &'static str,
        message: String,
    },
    Processor {
        message: String,
    },
    Consumer {
        message: String,
    },
}

impl std::fmt::Display for AudioStreamError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidOutputRate => {
                formatter.write_str("audio output sample rate must be greater than zero")
            }
            Self::InvalidOutputChannels => {
                formatter.write_str("audio output channel count must be greater than zero")
            }
            Self::InvalidPlaybackRate => {
                formatter.write_str("audio playback rate must be a finite value greater than zero")
            }
            Self::InvalidChunk { message } => {
                write!(formatter, "decoded audio chunk is invalid: {message}")
            }
            Self::Backend { operation, message } => {
                write!(
                    formatter,
                    "FFmpeg audio backend failed during {operation}: {message}"
                )
            }
            Self::Processor { message } => {
                write!(formatter, "Native audio processor failed: {message}")
            }
            Self::Consumer { message } => {
                write!(
                    formatter,
                    "audio stream consumer callback failed: {message}"
                )
            }
        }
    }
}

impl std::error::Error for AudioStreamError {}

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
    pub reorder_depth: Option<u32>,
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
    use super::audio::{audio_filter_spec, follow_rate_filter_chain, playback_rate_filter_chain};
    use super::hls::{
        MAX_REMOTE_HLS_MANIFEST_BYTES, append_remote_hls_manifest_chunk, parse_hls_master_manifest,
        resolve_hls_master_manifest_sources, resolve_uri_relative_to,
        select_hls_audio_rendition_uri, select_hls_video_variant_uri,
    };
    use super::input::{
        FfmpegInputInterrupt, InputOpenProfile, InputOpenPurpose, ffmpeg_interrupt_callback,
        input_open_profile_for_source, input_open_tuning_options, input_open_tuning_summary,
        supports_input_format,
    };
    use super::{AudioStreamError, DecodedAudioChunk, DecodedAudioTrack, FfmpegBackend};
    use player_model::MediaSource;
    use player_plugin::{AudioPitchMode, AudioPlaybackPolicy};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    const SYNTHETIC_SAMPLE_RATE: u32 = 48_000;
    const SYNTHETIC_FREQUENCY_HZ: f32 = 440.0;

    fn synthetic_sine_track() -> DecodedAudioTrack {
        let samples = (0..SYNTHETIC_SAMPLE_RATE)
            .map(|index| {
                let phase = std::f32::consts::TAU * SYNTHETIC_FREQUENCY_HZ * index as f32
                    / SYNTHETIC_SAMPLE_RATE as f32;
                phase.sin()
            })
            .collect::<Vec<_>>();
        DecodedAudioTrack {
            presentation_time: Duration::ZERO,
            sample_rate: SYNTHETIC_SAMPLE_RATE,
            channels: 1,
            playback_rate: 1.0,
            samples: Arc::from(samples),
        }
    }

    fn positive_zero_crossing_frequency(samples: &[f32]) -> f32 {
        let crossings = samples
            .windows(2)
            .filter(|window| window[0] <= 0.0 && window[1] > 0.0)
            .count();
        crossings as f32 * SYNTHETIC_SAMPLE_RATE as f32 / samples.len() as f32
    }

    #[test]
    fn playback_rate_filter_spec_chains_high_rates() {
        assert_eq!(playback_rate_filter_chain(0.5), "atempo=0.500000");
        assert_eq!(playback_rate_filter_chain(1.0), "anull");
        assert_eq!(playback_rate_filter_chain(1.25), "atempo=1.250000");
        assert_eq!(playback_rate_filter_chain(2.0), "atempo=2.000000");
        assert_eq!(
            playback_rate_filter_chain(3.0),
            "atempo=2.000000,atempo=1.500000"
        );
    }

    #[test]
    fn pitch_mode_filter_specs_keep_one_final_resample_stage() {
        assert_eq!(
            follow_rate_filter_chain(48_000, 0.5),
            "asetrate=48000*0.500000"
        );
        assert_eq!(follow_rate_filter_chain(48_000, 1.0), "anull");
        assert_eq!(
            follow_rate_filter_chain(48_000, 1.25),
            "asetrate=48000*1.250000"
        );
        assert_eq!(
            follow_rate_filter_chain(48_000, 2.0),
            "asetrate=48000*2.000000"
        );

        let output_layout = ffmpeg_next::ChannelLayout::STEREO;
        let preserve = audio_filter_spec(
            AudioPlaybackPolicy {
                playback_rate: 2.0,
                pitch_mode: AudioPitchMode::PreservePitch,
            },
            44_100,
            48_000,
            output_layout,
        );
        assert!(preserve.starts_with("atempo=2.000000,aresample=48000,"));
        assert!(!preserve.contains("asetrate="));
        assert_eq!(preserve.matches("aresample=").count(), 1);

        let follow = audio_filter_spec(
            AudioPlaybackPolicy {
                playback_rate: 2.0,
                pitch_mode: AudioPitchMode::FollowRate,
            },
            44_100,
            48_000,
            output_layout,
        );
        assert!(follow.starts_with("asetrate=44100*2.000000,aresample=48000,"));
        assert!(!follow.contains("atempo="));
        assert_eq!(follow.matches("aresample=").count(), 1);
    }

    #[test]
    fn rewrite_red_follow_rate_two_x_halves_duration_and_doubles_fundamental() {
        let source = synthetic_sine_track();

        let output = FfmpegBackend::new()
            .expect("initialize FFmpeg")
            .retime_audio_track_with_playback_policy(
                &source,
                AudioPlaybackPolicy {
                    playback_rate: 2.0,
                    pitch_mode: AudioPitchMode::FollowRate,
                },
            )
            .expect("retime synthetic PCM");
        let frequency = positive_zero_crossing_frequency(&output.samples);
        assert!(
            (840.0..=920.0).contains(&frequency),
            "2x FollowRate must shift the 440 Hz fundamental to about 880 Hz, got {frequency:.2} Hz"
        );
        assert!(
            (23_000..=25_000).contains(&output.samples.len()),
            "2x FollowRate must return about 24000 frames, got {}",
            output.samples.len()
        );
    }

    #[test]
    fn playback_policy_matrix_matches_duration_and_pitch() {
        let backend = FfmpegBackend::new().expect("initialize FFmpeg");
        let source = synthetic_sine_track();
        for pitch_mode in [AudioPitchMode::PreservePitch, AudioPitchMode::FollowRate] {
            for playback_rate in [0.5_f32, 1.0, 1.25, 2.0] {
                let output = backend
                    .retime_audio_track_with_playback_policy(
                        &source,
                        AudioPlaybackPolicy {
                            playback_rate,
                            pitch_mode,
                        },
                    )
                    .expect("retime synthetic PCM for policy matrix");
                let expected_frames = SYNTHETIC_SAMPLE_RATE as f32 / playback_rate;
                let frame_tolerance = (expected_frames * 0.03).max(512.0);
                assert!(
                    (output.samples.len() as f32 - expected_frames).abs() <= frame_tolerance,
                    "{pitch_mode:?} at {playback_rate}x returned {} frames, expected about {expected_frames}",
                    output.samples.len()
                );

                let expected_frequency = match pitch_mode {
                    AudioPitchMode::PreservePitch => SYNTHETIC_FREQUENCY_HZ,
                    AudioPitchMode::FollowRate => SYNTHETIC_FREQUENCY_HZ * playback_rate,
                };
                let frequency = positive_zero_crossing_frequency(&output.samples);
                let frequency_tolerance = (expected_frequency * 0.06).max(15.0);
                assert!(
                    (frequency - expected_frequency).abs() <= frequency_tolerance,
                    "{pitch_mode:?} at {playback_rate}x produced {frequency:.2} Hz, expected about {expected_frequency:.2} Hz"
                );
            }
        }
    }

    #[test]
    fn legacy_retime_api_keeps_preserve_pitch_semantics() {
        let output = FfmpegBackend::new()
            .expect("initialize FFmpeg")
            .retime_audio_track(&synthetic_sine_track(), 2.0)
            .expect("retime synthetic PCM through legacy API");
        let frequency = positive_zero_crossing_frequency(&output.samples);
        assert!(
            (420.0..=460.0).contains(&frequency),
            "legacy retime API must preserve the 440 Hz fundamental, got {frequency:.2} Hz"
        );
    }

    #[test]
    fn decoded_audio_chunk_rejects_invalid_sink_handoff_metadata() {
        let valid_chunk = DecodedAudioChunk {
            presentation_time: Some(Duration::from_secs(1)),
            duration: Duration::from_millis(10),
            sample_rate: 48_000,
            channels: 2,
            samples: vec![0.0; 960],
            discontinuity: true,
        };
        assert!(valid_chunk.validate().is_ok());

        let mut zero_rate = valid_chunk.clone();
        zero_rate.sample_rate = 0;
        assert!(zero_rate.validate().is_err());

        let mut zero_channels = valid_chunk.clone();
        zero_channels.channels = 0;
        assert!(zero_channels.validate().is_err());

        let mut unaligned_samples = valid_chunk.clone();
        unaligned_samples.samples.pop();
        assert!(unaligned_samples.validate().is_err());

        let mut zero_duration = valid_chunk;
        zero_duration.duration = Duration::ZERO;
        assert!(zero_duration.validate().is_err());
    }

    #[test]
    fn streaming_audio_rejects_invalid_playback_rate_before_opening_source() {
        let backend = FfmpegBackend { initialized: false };
        let error = backend
            .stream_audio_source_with_playback_rate_and_interrupt(
                MediaSource::new("/a/source-that-need-not-exist.m4a"),
                48_000,
                2,
                0.0,
                Duration::ZERO,
                None,
                |_| Ok(()),
                |_| Ok(true),
            )
            .expect_err("invalid playback rate must be rejected before source opening");

        assert_eq!(error, AudioStreamError::InvalidPlaybackRate);
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
        assert!(audio_options.contains(&("protocol_whitelist", "http,https,tcp,tls,crypto")));
        assert!(audio_options.contains(&("protocol_blacklist", "file,concat,subfile")));
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
            select_hls_audio_rendition_uri("https://example.com/live/master.m3u8", manifest)
                .expect("valid remote HLS audio rendition");

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
            select_hls_video_variant_uri("https://example.com/live/master.m3u8", manifest)
                .expect("valid remote HLS video variant");

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
            resolve_hls_master_manifest_sources("https://example.com/live/master.m3u8", manifest)
                .expect("valid remote HLS sources");

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

    #[test]
    fn remote_hls_uri_resolution_rejects_non_http_initial_and_derived_uris() {
        assert_eq!(
            resolve_uri_relative_to("ftp://example.com/live/master.m3u8", "video.m3u8"),
            None
        );
        assert_eq!(
            resolve_uri_relative_to("https://example.com/live/master.m3u8", "file:///etc/passwd"),
            None
        );
        assert_eq!(
            resolve_uri_relative_to(
                "https://example.com/live/master.m3u8",
                "tcp://127.0.0.1:9000"
            ),
            None
        );
    }

    #[test]
    fn remote_hls_manifest_limit_is_checked_before_append() {
        let mut bytes = vec![b'x'; MAX_REMOTE_HLS_MANIFEST_BYTES - 1];
        append_remote_hls_manifest_chunk(&mut bytes, b"y")
            .expect("manifest at the byte limit should be accepted");
        assert_eq!(bytes.len(), MAX_REMOTE_HLS_MANIFEST_BYTES);

        let error = append_remote_hls_manifest_chunk(&mut bytes, b"z")
            .expect_err("manifest above the byte limit should be rejected");

        assert_eq!(bytes.len(), MAX_REMOTE_HLS_MANIFEST_BYTES);
        assert!(error.to_string().contains("524288-byte limit"));
    }
}
