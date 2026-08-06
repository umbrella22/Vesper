#![deny(unsafe_code)]

//! Diagnostic packet-stream SourceNormalizer plugin.

use std::sync::atomic::{AtomicU64, Ordering};

use player_plugin::{
    DecoderBitstreamFormat, Plugin, PluginBuildError, SourceNormalizerError,
    SourceNormalizerNormalizeLevel, SourceNormalizerOperationStatus, SourceNormalizerPacket,
    SourceNormalizerPacketCapabilities, SourceNormalizerPacketLease,
    SourceNormalizerPacketMediaKind, SourceNormalizerPacketPluginFactory,
    SourceNormalizerPacketSeek, SourceNormalizerPacketSession, SourceNormalizerPacketSessionConfig,
    SourceNormalizerPacketStreamInfo, SourceNormalizerPacketTrackInfo,
    SourceNormalizerReadPacketMetadata, SourceNormalizerRequiredCapabilities,
};

const PLUGIN_ID: &str = "io.github.ikaros.vesper.source-normalizer-diagnostic";
const INSTANCE_ID: &str = "io.github.ikaros.vesper.source-normalizer-diagnostic.packet";
const PLUGIN_NAME: &str = "player-source-normalizer-diagnostic";
const DIAGNOSTIC_PACKET_BYTES: &[u8] = b"vesper-diagnostic-source-normalizer-packet";
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Default)]
struct DiagnosticPacketFactory;

impl SourceNormalizerPacketPluginFactory for DiagnosticPacketFactory {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn packet_capabilities(&self) -> SourceNormalizerPacketCapabilities {
        diagnostic_packet_capabilities()
    }

    fn open_packet_session(
        &self,
        config: &SourceNormalizerPacketSessionConfig,
    ) -> Result<Box<dyn SourceNormalizerPacketSession>, SourceNormalizerError> {
        if config.input.is_empty() {
            return Err(SourceNormalizerError::invalid_input(
                "input must not be empty",
            ));
        }
        let capabilities = diagnostic_packet_capabilities();
        if !capabilities.supports_runtime_profile(&config.runtime_profile) {
            return Err(SourceNormalizerError::UnsupportedRuntimeProfile {
                profile: config.runtime_profile.clone(),
            });
        }
        if config.preferred_media_kind != SourceNormalizerPacketMediaKind::Video {
            return Err(SourceNormalizerError::unsupported_operation(
                "non-video packet streams",
            ));
        }

        let session_number = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(DiagnosticPacketSession {
            stream_info: SourceNormalizerPacketStreamInfo {
                session_id: Some(format!("diagnostic-packet-{session_number}")),
                normalizer_name: Some(PLUGIN_NAME.to_owned()),
                runtime_profile: Some(config.runtime_profile.clone()),
                selected_backend: Some("diagnostic-packet".to_owned()),
                tracks: vec![diagnostic_video_track()],
                selected_track_index: Some(0),
                duration_millis: Some(1_000),
                seekable: true,
            },
            emitted_packet: false,
            leased_packet: None,
            last_seek_millis: None,
            closed: false,
        }))
    }
}

#[derive(Debug)]
struct DiagnosticPacketSession {
    stream_info: SourceNormalizerPacketStreamInfo,
    emitted_packet: bool,
    leased_packet: Option<DiagnosticPacketLease>,
    last_seek_millis: Option<u64>,
    closed: bool,
}

#[derive(Debug)]
struct DiagnosticPacketLease {
    handle: usize,
    data: Vec<u8>,
}

impl SourceNormalizerPacketSession for DiagnosticPacketSession {
    fn stream_info(&self) -> SourceNormalizerPacketStreamInfo {
        self.stream_info.clone()
    }

    fn read_packet(&mut self) -> Result<SourceNormalizerPacketLease<'_>, SourceNormalizerError> {
        if self.closed {
            return Err(SourceNormalizerError::NotConfigured);
        }
        if self.leased_packet.is_some() {
            return Err(SourceNormalizerError::abi_violation(
                "previous packet lease has not been released",
            ));
        }
        if self.emitted_packet {
            return Ok(SourceNormalizerPacketLease {
                metadata: SourceNormalizerReadPacketMetadata::end_of_stream(),
                data: &[],
                handle: 0,
            });
        }

        self.emitted_packet = true;
        let packet = self.leased_packet.insert(DiagnosticPacketLease {
            handle: 1,
            data: DIAGNOSTIC_PACKET_BYTES.to_vec(),
        });
        Ok(SourceNormalizerPacketLease {
            metadata: SourceNormalizerReadPacketMetadata::packet(SourceNormalizerPacket {
                pts_us: self
                    .last_seek_millis
                    .map(|millis| i64::try_from(millis.saturating_mul(1_000)).unwrap_or(i64::MAX))
                    .or(Some(0)),
                dts_us: Some(0),
                duration_us: Some(33_333),
                stream_index: 0,
                media_kind: SourceNormalizerPacketMediaKind::Video,
                key_frame: true,
                discontinuity: self.last_seek_millis.is_some(),
                sample_rate: None,
                channels: None,
                channel_layout: None,
                sample_format: None,
                frame_count: None,
                end_of_stream: false,
            }),
            data: &packet.data,
            handle: packet.handle,
        })
    }

    fn release_packet(&mut self, packet_handle: usize) -> Result<(), SourceNormalizerError> {
        if self.closed {
            return Err(SourceNormalizerError::NotConfigured);
        }
        match self.leased_packet.take() {
            Some(packet) if packet.handle == packet_handle => Ok(()),
            Some(packet) => {
                self.leased_packet = Some(packet);
                Err(SourceNormalizerError::abi_violation(format!(
                    "unknown packet handle {packet_handle}"
                )))
            }
            None => Err(SourceNormalizerError::abi_violation(
                "no packet lease is outstanding",
            )),
        }
    }

    fn seek(
        &mut self,
        seek: &SourceNormalizerPacketSeek,
    ) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        self.ensure_open()?;
        self.leased_packet = None;
        self.emitted_packet = false;
        self.last_seek_millis = Some(seek.position_millis);
        Ok(completed_operation())
    }

    fn flush(&mut self) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        self.ensure_open()?;
        self.leased_packet = None;
        self.emitted_packet = false;
        Ok(completed_operation())
    }

    fn close(&mut self) -> Result<(), SourceNormalizerError> {
        if self.closed {
            return Ok(());
        }
        self.leased_packet = None;
        self.closed = true;
        Ok(())
    }
}

impl DiagnosticPacketSession {
    fn ensure_open(&self) -> Result<(), SourceNormalizerError> {
        if self.closed {
            Err(SourceNormalizerError::NotConfigured)
        } else {
            Ok(())
        }
    }
}

impl Drop for DiagnosticPacketSession {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[player_plugin::export]
fn diagnostic_source_normalizer_plugin() -> Result<Plugin, PluginBuildError> {
    Plugin::builder(PLUGIN_ID, PLUGIN_NAME)?
        .with_source_normalizer_packet(INSTANCE_ID, DiagnosticPacketFactory)?
        .build()
}

fn diagnostic_packet_capabilities() -> SourceNormalizerPacketCapabilities {
    SourceNormalizerPacketCapabilities {
        supported_runtime_profiles: vec![
            "diagnostic-packet".to_owned(),
            "diagnostic-fmp4".to_owned(),
            "diagnostic-hls".to_owned(),
        ],
        max_level: SourceNormalizerNormalizeLevel::RemuxOnly,
        media_kinds: vec![SourceNormalizerPacketMediaKind::Video],
        codecs: vec!["H264".to_owned()],
        bitstream_formats: vec![DecoderBitstreamFormat::Avcc],
        supports_seek: true,
        supports_flush: true,
        required_capabilities: SourceNormalizerRequiredCapabilities::default(),
        max_sessions: None,
    }
}

fn diagnostic_video_track() -> SourceNormalizerPacketTrackInfo {
    SourceNormalizerPacketTrackInfo {
        stream_index: 0,
        media_kind: SourceNormalizerPacketMediaKind::Video,
        codec: "H264".to_owned(),
        extradata: vec![1, 66, 0, 30],
        bitstream_format: Some(DecoderBitstreamFormat::Avcc),
        width: Some(16),
        height: Some(16),
        coded_width: Some(16),
        coded_height: Some(16),
        sample_rate: None,
        channels: None,
        channel_layout: None,
        codec_delay_samples: None,
        priming_samples: None,
        trailing_padding_samples: None,
        seek_preroll_samples: None,
        color: None,
        hdr: None,
        frame_rate: Some(30.0),
        reorder_depth: None,
        time_base_num: Some(1),
        time_base_den: Some(90_000),
    }
}

fn completed_operation() -> SourceNormalizerOperationStatus {
    SourceNormalizerOperationStatus {
        completed: true,
        message: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use player_plugin::SourceNormalizerReadPacketStatus;

    fn packet_config(input: &str) -> SourceNormalizerPacketSessionConfig {
        SourceNormalizerPacketSessionConfig {
            runtime_profile: "diagnostic-packet".to_owned(),
            input: input.to_owned(),
            headers: Vec::new(),
            startup_timeout_ms: None,
            session_timeout_ms: None,
            preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
        }
    }

    #[test]
    fn exports_plugin_entry() {
        let entry: extern "C" fn() -> *const player_plugin::__private::VesperPluginRoot =
            vesper_plugin_entry;
        assert!(!entry().is_null());
    }

    #[test]
    fn packet_capabilities_match_diagnostic_contract() {
        let capabilities = DiagnosticPacketFactory.packet_capabilities();

        assert!(capabilities.supports_runtime_profile("diagnostic-packet"));
        assert!(capabilities.supports_codec("h264"));
        assert_eq!(capabilities.max_sessions, None);
    }

    #[test]
    fn safe_packet_lifecycle_returns_synthetic_packet_then_eof() {
        let mut session = DiagnosticPacketFactory
            .open_packet_session(&packet_config("file:///tmp/input.mp4"))
            .expect("open diagnostic packet session");
        assert_eq!(
            session.stream_info().normalizer_name.as_deref(),
            Some(PLUGIN_NAME)
        );

        let handle = {
            let packet = session.read_packet().expect("read packet");
            assert_eq!(
                packet.metadata.status,
                SourceNormalizerReadPacketStatus::Packet
            );
            assert!(!packet.data.is_empty());
            packet.handle
        };
        session.release_packet(handle).expect("release packet");

        let eof = session.read_packet().expect("read eof");
        assert_eq!(
            eof.metadata.status,
            SourceNormalizerReadPacketStatus::EndOfStream
        );
        assert!(eof.data.is_empty());
        assert_eq!(eof.handle, 0);
        session.close().expect("close session");
        session.close().expect("close remains idempotent");
    }

    #[test]
    fn read_requires_release_before_next_packet() {
        let mut session = DiagnosticPacketFactory
            .open_packet_session(&packet_config("file:///tmp/input.mp4"))
            .expect("open diagnostic packet session");

        let _packet = session.read_packet().expect("read first packet");
        let error = session
            .read_packet()
            .expect_err("second read requires releasing the first packet");

        assert!(matches!(error, SourceNormalizerError::AbiViolation { .. }));
    }

    #[test]
    fn seek_releases_author_lease_and_resets_packet_timestamp() {
        let mut session = DiagnosticPacketFactory
            .open_packet_session(&packet_config("file:///tmp/input.mp4"))
            .expect("open diagnostic packet session");
        let _packet = session.read_packet().expect("read first packet");

        session
            .seek(&SourceNormalizerPacketSeek {
                position_millis: 123,
                exact: false,
            })
            .expect("seek packet session");

        let packet = session.read_packet().expect("read packet after seek");
        assert_eq!(
            packet
                .metadata
                .packet
                .as_ref()
                .and_then(|packet| packet.pts_us),
            Some(123_000)
        );
    }

    #[test]
    fn open_rejects_empty_input() {
        let error = match DiagnosticPacketFactory.open_packet_session(&packet_config("")) {
            Ok(_) => panic!("empty input must be rejected"),
            Err(error) => error,
        };

        assert!(matches!(error, SourceNormalizerError::InvalidInput { .. }));
    }
}
