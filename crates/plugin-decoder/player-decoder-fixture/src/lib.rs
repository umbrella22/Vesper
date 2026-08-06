#![deny(unsafe_code)]

use std::collections::HashMap;

use player_plugin::{
    DecoderBitstreamFormat, DecoderCapabilities, DecoderCodecCapability, DecoderError,
    DecoderFrameFormat, DecoderMediaKind, DecoderNativeFrame, DecoderNativeFrameMetadata,
    DecoderNativeFrameReleaseTracking, DecoderNativeHandleKind, DecoderNativeRequirements,
    DecoderPacket, DecoderPacketResult, DecoderPcmFrame, DecoderPcmFrameMetadata,
    DecoderPcmSampleLayout, DecoderReceiveNativeFrameOutput, DecoderReceivePcmFrameOutput,
    DecoderSessionConfig, DecoderSessionInfo, NativeDecoderPluginFactory, NativeDecoderSession,
    NativeFramePipelineProfile, Plugin, PluginBuildError,
};

const CONFIGURED_CODECS_ENV: &str = "VESPER_DECODER_FIXTURE_CODECS";
const DEFAULT_VIDEO_CODEC: &str = "fixture-video";
const PLUGIN_ID: &str = "dev.vesper.decoder-fixture";
const INSTANCE_ID: &str = "dev.vesper.decoder-fixture.native";

#[derive(Debug, Default)]
struct FixtureDecoderFactory;

impl NativeDecoderPluginFactory for FixtureDecoderFactory {
    fn name(&self) -> &str {
        "player-decoder-fixture"
    }

    fn capabilities(&self) -> DecoderCapabilities {
        decoder_capabilities()
    }

    fn native_requirements(&self) -> DecoderNativeRequirements {
        DecoderNativeRequirements {
            required_device_context_kinds: Vec::new(),
            output_handle_kinds: vec![DecoderNativeHandleKind::IoSurface],
            output_pipeline_profiles: vec![NativeFramePipelineProfile::Unknown(
                "io_surface".to_owned(),
            )],
            requires_native_device_context: false,
            accepted_bitstream_formats: vec![DecoderBitstreamFormat::Unknown("fixture".to_owned())],
        }
    }

    fn open_native_session(
        &self,
        config: &DecoderSessionConfig,
    ) -> Result<Box<dyn NativeDecoderSession>, DecoderError> {
        if !self
            .capabilities()
            .supports_codec(&config.codec, config.media_kind)
        {
            return Err(DecoderError::UnsupportedCodec {
                codec: config.codec.clone(),
            });
        }
        Ok(Box::new(FixtureDecoderSession {
            codec: config.codec.clone(),
            media_kind: config.media_kind,
            last_pts_us: None,
            pending_frame: None,
            next_handle: 1,
            outstanding_frames: HashMap::new(),
        }))
    }
}

#[derive(Debug)]
struct FixtureDecoderSession {
    codec: String,
    media_kind: DecoderMediaKind,
    last_pts_us: Option<i64>,
    pending_frame: Option<Vec<u8>>,
    next_handle: usize,
    outstanding_frames: HashMap<usize, Vec<u8>>,
}

impl NativeDecoderSession for FixtureDecoderSession {
    fn session_info(&self) -> DecoderSessionInfo {
        DecoderSessionInfo {
            decoder_name: Some("player-decoder-fixture".to_owned()),
            selected_hardware_backend: Some("fixture-native".to_owned()),
            output_format: Some(match self.media_kind {
                DecoderMediaKind::Audio => DecoderFrameFormat::F32,
                DecoderMediaKind::Video => DecoderFrameFormat::Nv12,
            }),
        }
    }

    fn send_packet(
        &mut self,
        packet: &DecoderPacket,
        data: &[u8],
    ) -> Result<DecoderPacketResult, DecoderError> {
        self.last_pts_us = packet.pts_us;
        self.pending_frame = Some(data.to_vec());
        Ok(DecoderPacketResult { accepted: true })
    }

    fn receive_native_frame(&mut self) -> Result<DecoderReceiveNativeFrameOutput, DecoderError> {
        if self.media_kind != DecoderMediaKind::Video {
            return Err(DecoderError::UnsupportedCapability {
                capability: "video-native-frame-output".to_owned(),
            });
        }
        if self.pending_frame.is_none() {
            return Ok(DecoderReceiveNativeFrameOutput::NeedMoreInput);
        }
        let handle = self.next_handle;
        let next_handle = handle
            .checked_add(1)
            .ok_or_else(|| DecoderError::internal("fixture native-frame handle space exhausted"))?;
        let frame_id = u64::try_from(handle)
            .map_err(|_| DecoderError::internal("fixture native-frame handle is too large"))?;
        let data = self
            .pending_frame
            .take()
            .ok_or(DecoderError::NeedMoreInput)?;
        self.next_handle = next_handle;
        self.outstanding_frames.insert(handle, data);
        Ok(DecoderReceiveNativeFrameOutput::Frame(DecoderNativeFrame {
            metadata: DecoderNativeFrameMetadata {
                media_kind: DecoderMediaKind::Video,
                format: DecoderFrameFormat::Nv12,
                codec: self.codec.clone(),
                pts_us: self.last_pts_us,
                duration_us: Some(33_333),
                width: 2,
                height: 2,
                coded_width: Some(2),
                coded_height: Some(2),
                visible_rect: None,
                handle_kind: DecoderNativeHandleKind::IoSurface,
                pipeline_profile: Some(NativeFramePipelineProfile::Unknown(
                    "io_surface".to_owned(),
                )),
                color_space: None,
                hdr_metadata: None,
                color: None,
                hdr: None,
                sync_info: None,
                transform: None,
                frame_id: Some(frame_id),
                release_tracking: Some(DecoderNativeFrameReleaseTracking {
                    frame_id: Some(frame_id),
                    requires_release: true,
                }),
            },
            handle,
            lease_token: None,
        }))
    }

    fn receive_pcm_frame(&mut self) -> Result<DecoderReceivePcmFrameOutput, DecoderError> {
        if self.media_kind != DecoderMediaKind::Audio {
            return Err(DecoderError::UnsupportedCapability {
                capability: "audio-pcm-output".to_owned(),
            });
        }
        let Some(data) = self.pending_frame.take() else {
            return Ok(DecoderReceivePcmFrameOutput::NeedMoreInput);
        };
        let mut metadata = DecoderPcmFrameMetadata::audio(
            self.codec.clone(),
            DecoderFrameFormat::F32,
            48_000,
            2,
            DecoderPcmSampleLayout::Interleaved,
            1_024,
        );
        metadata.pts_us = self.last_pts_us;
        metadata.duration_us = Some(21_333);
        metadata.channel_layout = Some("stereo".to_owned());
        Ok(DecoderReceivePcmFrameOutput::Frame(DecoderPcmFrame {
            metadata,
            data,
        }))
    }

    fn release_native_frame(&mut self, frame: DecoderNativeFrame) -> Result<(), DecoderError> {
        if frame.metadata.handle_kind != DecoderNativeHandleKind::IoSurface || frame.handle == 0 {
            return Err(DecoderError::abi_violation(
                "fixture native-frame release received an invalid handle",
            ));
        }
        self.outstanding_frames
            .remove(&frame.handle)
            .map(|_| ())
            .ok_or_else(|| DecoderError::abi_violation("fixture native-frame lease is stale"))
    }

    fn flush(&mut self) -> Result<(), DecoderError> {
        self.pending_frame = None;
        self.outstanding_frames.clear();
        Ok(())
    }

    fn close(&mut self) -> Result<(), DecoderError> {
        self.pending_frame = None;
        self.outstanding_frames.clear();
        Ok(())
    }
}

fn decoder_capabilities() -> DecoderCapabilities {
    let mut codecs = configured_video_codecs();
    codecs.push(DecoderCodecCapability {
        codec: "fixture-audio".to_owned(),
        media_kind: DecoderMediaKind::Audio,
        profiles: vec!["fixture".to_owned()],
        output_formats: vec![DecoderFrameFormat::F32],
    });
    DecoderCapabilities {
        codecs,
        supports_hardware_decode: true,
        supports_cpu_video_frames: false,
        supports_audio_frames: true,
        supports_pcm_frames: true,
        supports_gpu_handles: true,
        supports_presentation_release: false,
        supports_flush: true,
        supports_drain: true,
        max_sessions: Some(1),
    }
}

fn configured_video_codecs() -> Vec<DecoderCodecCapability> {
    let configured =
        std::env::var_os(CONFIGURED_CODECS_ENV).map(|value| value.to_string_lossy().into_owned());
    video_codecs_from_configured_list(configured.as_deref())
}

fn video_codecs_from_configured_list(configured: Option<&str>) -> Vec<DecoderCodecCapability> {
    let mut codecs = configured
        .into_iter()
        .flat_map(|value| value.split([',', ';']))
        .map(str::trim)
        .filter(|codec| !codec.is_empty())
        .fold(Vec::<String>::new(), |mut codecs, codec| {
            if !codecs
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(codec))
            {
                codecs.push(codec.to_owned());
            }
            codecs
        });

    if codecs.is_empty() {
        codecs.push(DEFAULT_VIDEO_CODEC.to_owned());
    }

    codecs
        .into_iter()
        .map(|codec| DecoderCodecCapability {
            codec,
            media_kind: DecoderMediaKind::Video,
            profiles: vec!["fixture".to_owned()],
            output_formats: vec![DecoderFrameFormat::Nv12],
        })
        .collect()
}

#[player_plugin::export]
fn decoder_fixture_plugin() -> Result<Plugin, PluginBuildError> {
    Plugin::builder(PLUGIN_ID, "player-decoder-fixture")?
        .with_native_decoder(INSTANCE_ID, FixtureDecoderFactory)?
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_plugin_entry() {
        let entry: extern "C" fn() -> *const player_plugin::__private::VesperPluginRoot =
            vesper_plugin_entry;
        assert!(!entry().is_null());
    }

    #[test]
    fn configured_codec_list_defaults_to_fixture_video() {
        let codecs = video_codecs_from_configured_list(None);
        assert_eq!(codecs.len(), 1);
        assert_eq!(codecs[0].codec, DEFAULT_VIDEO_CODEC);
    }

    #[test]
    fn configured_codec_list_accepts_comma_or_semicolon_separated_video_codecs() {
        let codecs = video_codecs_from_configured_list(Some("H264, HEVC;h264"));
        let names = codecs
            .into_iter()
            .map(|codec| codec.codec)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["H264", "HEVC"]);
    }

    #[test]
    fn capabilities_advertise_native_video_and_audio_pcm() {
        let capabilities = decoder_capabilities();
        assert!(capabilities.supports_hardware_decode);
        assert!(capabilities.supports_gpu_handles);
        assert!(capabilities.supports_audio_frames);
        assert!(capabilities.supports_codec("fixture-audio", DecoderMediaKind::Audio));
        assert!(capabilities.codecs.iter().any(|codec| {
            codec.codec == DEFAULT_VIDEO_CODEC
                && codec.media_kind == DecoderMediaKind::Video
                && codec.output_formats == vec![DecoderFrameFormat::Nv12]
        }));
    }

    #[test]
    fn audio_session_round_trips_pcm_and_need_more_input() {
        let factory = FixtureDecoderFactory;
        let mut session = factory
            .open_native_session(&DecoderSessionConfig {
                codec: "fixture-audio".to_owned(),
                media_kind: DecoderMediaKind::Audio,
                ..DecoderSessionConfig::default()
            })
            .expect("open audio session");
        assert_eq!(
            session.session_info().output_format,
            Some(DecoderFrameFormat::F32)
        );
        session
            .send_packet(
                &DecoderPacket {
                    pts_us: Some(7_000),
                    ..DecoderPacket::default()
                },
                &[1, 2, 3, 4],
            )
            .expect("send audio packet");
        let DecoderReceivePcmFrameOutput::Frame(frame) =
            session.receive_pcm_frame().expect("receive PCM frame")
        else {
            panic!("expected PCM frame");
        };
        assert_eq!(frame.metadata.pts_us, Some(7_000));
        assert_eq!(frame.metadata.channel_layout.as_deref(), Some("stereo"));
        assert_eq!(frame.data, vec![1, 2, 3, 4]);
        assert_eq!(
            session.receive_pcm_frame().expect("need more input"),
            DecoderReceivePcmFrameOutput::NeedMoreInput
        );
    }

    #[test]
    fn video_session_releases_safe_fixture_handle_once() {
        let factory = FixtureDecoderFactory;
        let mut session = factory
            .open_native_session(&DecoderSessionConfig {
                codec: DEFAULT_VIDEO_CODEC.to_owned(),
                media_kind: DecoderMediaKind::Video,
                ..DecoderSessionConfig::default()
            })
            .expect("open video session");
        session
            .send_packet(&DecoderPacket::default(), &[9, 8, 7])
            .expect("send video packet");
        let DecoderReceiveNativeFrameOutput::Frame(frame) = session
            .receive_native_frame()
            .expect("receive native frame")
        else {
            panic!("expected native frame");
        };
        let stale = frame.clone();
        session
            .release_native_frame(frame)
            .expect("release native frame");
        assert!(matches!(
            session.release_native_frame(stale),
            Err(DecoderError::AbiViolation { .. })
        ));
    }
}
