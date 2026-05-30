use std::collections::HashMap;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use player_model::MediaSource;
use player_platform_mobile::MobileSourceNormalizerConfiguration;
use player_plugin::{
    DecoderBitstreamFormat, DecoderFrameFormat, DecoderMediaKind, DecoderNativeFrame,
    DecoderNativeHandleKind, DecoderPacket, DecoderReceiveNativeFrameOutput, DecoderSessionConfig,
    FrameProcessorError, FrameProcessorFrameTimings, FrameProcessorReceiveOutput,
    FrameProcessorSession, FrameProcessorSessionConfig, FrameProcessorSubmitFrame,
    FrameProcessorSubmitStatus, NativeDecoderSession, NativeFrame, NativeFrameMetadata,
    NativeFramePipelineProfile, NativeHandleKind, SourceNormalizerPacketMediaKind,
    SourceNormalizerPacketSeek, SourceNormalizerPacketSession, SourceNormalizerPacketSessionConfig,
    SourceNormalizerReadPacketStatus,
};
use player_plugin_loader::{
    DecoderPluginMatchRequest, LoadedDynamicPlugin, PluginDiagnosticRecord, PluginRegistry,
};
use player_runtime::{
    FrameProcessorMode, FrameProcessorPolicy, NativeFramePipelineMode, PlayerPlaybackRoute,
    PlayerPluginDiagnostic, PlayerPluginParticipation, SourceNormalizerMode,
};
use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};

const SOURCE_NORMALIZER_STARTUP_TIMEOUT_MS: u64 = 10_000;
const SOURCE_NORMALIZER_PACKET_SESSION_TIMEOUT_MS: u64 = 30_000;
const DECODER_DRAIN_RETRY_INTERVAL: Duration = Duration::from_millis(2);
const MAX_PACKET_READ_ATTEMPTS_PER_ADVANCE: usize = 8;
const MAX_DECODE_RECEIVE_ATTEMPTS_PER_ADVANCE: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IosNativeFramePipelineOpenError {
    pub issue_kind: &'static str,
    pub message: String,
}

impl IosNativeFramePipelineOpenError {
    fn new(issue_kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            issue_kind,
            message: message.into(),
        }
    }

    pub fn wire_message(&self) -> String {
        format!("nativeFrameIssueKind={}; {}", self.issue_kind, self.message)
    }
}

#[derive(Debug, Clone)]
pub struct IosNativeFramePipelineOpenConfig {
    pub source_uri: String,
    pub source_normalizer_mode: SourceNormalizerMode,
    pub source_normalizer_plugin_library_paths: Vec<PathBuf>,
    pub runtime_profile: Option<String>,
    pub native_frame_pipeline_mode: NativeFramePipelineMode,
    pub decoder_plugin_library_paths: Vec<PathBuf>,
    pub frame_processor_plugin_library_paths: Vec<PathBuf>,
    pub max_in_flight_frames: Option<u32>,
}

pub struct IosNativeFramePipelineSession {
    source_uri: String,
    duration_millis: Option<u64>,
    seekable: bool,
    has_audio_track: bool,
    audio_track_codec: Option<String>,
    audio_stream_index: Option<u32>,
    audio_decoder_plugin_name: Option<String>,
    audio_decoder_plugin_ready: bool,
    video_stream_index: u32,
    source_normalizer_plugin_name: Option<String>,
    decoder_plugin_name: String,
    processor_plugin_names: Vec<String>,
    packet_session: Box<dyn SourceNormalizerPacketSession>,
    decoder_session: Box<dyn NativeDecoderSession>,
    frame_processor_chain: Option<IosFrameProcessorChain>,
    end_of_input_sent: bool,
    end_of_stream_received: bool,
    next_frame_handle: u64,
    pending_frames: HashMap<u64, IosNativeFramePipelineFrame>,
    counters: IosNativeFramePipelineCounters,
}

pub struct IosNativeFramePipelineFrame {
    pub handle: usize,
    pub presentation_time_us: i64,
    pub duration_us: Option<i64>,
    pub width: u32,
    pub height: u32,
    pub frame_id: Option<u64>,
    frame: IosPipelineFrame,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IosNativeFramePipelineCounters {
    pub decoded_frames: u64,
    pub processed_frames: u64,
    pub bypassed_frames: u64,
    pub presented_frames: u64,
    pub skipped_audio_packets: u64,
    pub skipped_video_packets: u64,
    pub skipped_other_packets: u64,
    pub seek_count: u64,
    pub deadline_misses: u64,
    pub backpressure_count: u64,
    pub late_dropped: u64,
    pub released_frames: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IosNativeFramePipelineWire {
    pub handle: u64,
    pub route: &'static str,
    pub source_input: &'static str,
    pub decoder_adapter: &'static str,
    pub decoder_plugin: String,
    pub audio_decoder: &'static str,
    pub audio_output: &'static str,
    pub audio_pipeline: &'static str,
    pub audio_rate_control: &'static str,
    pub selected_profile: &'static str,
    pub presenter_profile: &'static str,
    pub participation: &'static str,
    pub source_uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_millis: Option<u64>,
    pub seekable: bool,
    pub has_audio_track: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_track_codec: Option<String>,
    pub selected_video_stream_index: u32,
    pub selected_video_media_kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_stream_index: Option<u32>,
    pub audio_media_kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_decoder_plugin: Option<String>,
    pub audio_decoder_plugin_ready: bool,
    pub clock_source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_normalizer_plugin: Option<String>,
    pub processor_chain: Vec<String>,
    pub counters: IosNativeFramePipelineCounters,
    pub diagnostics: Vec<IosNativeFramePipelineDiagnosticWire>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IosNativeFramePipelineStatusWire {
    pub handle: u64,
    pub route: &'static str,
    pub participation: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_millis: Option<u64>,
    pub seekable: bool,
    pub has_audio_track: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_track_codec: Option<String>,
    pub selected_video_stream_index: u32,
    pub selected_video_media_kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_stream_index: Option<u32>,
    pub audio_media_kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_decoder_plugin: Option<String>,
    pub audio_decoder_plugin_ready: bool,
    pub clock_source: &'static str,
    pub audio_decoder: &'static str,
    pub audio_output: &'static str,
    pub audio_pipeline: &'static str,
    pub audio_rate_control: &'static str,
    pub counters: IosNativeFramePipelineCounters,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IosNativeFramePipelineFrameWire {
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pixel_buffer: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation_time_us: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_us: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<u64>,
    pub counters: IosNativeFramePipelineCounters,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IosNativeFramePipelineDiagnosticWire {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_kind: Option<String>,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub participation: &'static str,
    #[serde(skip_serializing_if = "IosNativeFramePipelineDiagnosticDetailsWire::is_empty")]
    pub details: IosNativeFramePipelineDiagnosticDetailsWire,
}

#[derive(Debug, Clone, Default)]
pub struct IosNativeFramePipelineDiagnosticDetailsWire {
    details: Vec<(String, String)>,
}

impl IosNativeFramePipelineDiagnosticDetailsWire {
    fn is_empty(&self) -> bool {
        self.details.is_empty()
    }

    fn from_pairs(pairs: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>) -> Self {
        Self {
            details: pairs
                .into_iter()
                .map(|(key, value)| (key.into(), value.into()))
                .collect(),
        }
    }
}

impl Serialize for IosNativeFramePipelineDiagnosticDetailsWire {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.details.len()))?;
        for (key, value) in &self.details {
            map.serialize_entry(key.as_str(), value.as_str())?;
        }
        map.end()
    }
}

struct IosFrameProcessorChain {
    processors: Vec<IosFrameProcessorNode>,
    mode: FrameProcessorMode,
    policy: FrameProcessorPolicy,
}

struct IosFrameProcessorNode {
    plugin_name: String,
    processor_index: usize,
    session: Box<dyn FrameProcessorSession>,
}

#[derive(Debug)]
struct IosPipelineFrame {
    decoder_frame: DecoderNativeFrame,
    presentation_frame: DecoderNativeFrame,
    processor_outputs: Vec<ProcessorOwnedNativeFrame>,
}

#[derive(Debug)]
struct ProcessorOwnedNativeFrame {
    processor_index: usize,
    frame: NativeFrame,
}

impl IosNativeFramePipelineSession {
    pub fn open(
        config: IosNativeFramePipelineOpenConfig,
    ) -> Result<Self, IosNativeFramePipelineOpenError> {
        if !matches!(
            config.native_frame_pipeline_mode,
            NativeFramePipelineMode::PreferNativeFrame
                | NativeFramePipelineMode::RequireNativeFrame
        ) {
            return Err(IosNativeFramePipelineOpenError::new(
                "startupFailure",
                "iOS native-frame pipeline must be explicitly preferred or required",
            ));
        }
        if config.source_normalizer_plugin_library_paths.is_empty() {
            return Err(IosNativeFramePipelineOpenError::new(
                "missingSourceNormalizerPacketPlugin",
                "iOS native-frame pipeline requires a SourceNormalizer packet-stream plugin path",
            ));
        }
        if config.decoder_plugin_library_paths.is_empty() {
            return Err(IosNativeFramePipelineOpenError::new(
                "missingVideoToolboxDecoderPlugin",
                "iOS native-frame pipeline requires a VideoToolbox decoder plugin path",
            ));
        }

        let source = MediaSource::new(config.source_uri.clone());
        let source_normalizer_configuration = MobileSourceNormalizerConfiguration {
            mode: config.source_normalizer_mode,
            plugin_library_paths: config.source_normalizer_plugin_library_paths.clone(),
            runtime_profile: config.runtime_profile.clone(),
        };
        let (source_record, mut packet_session) = open_packet_source_normalizer(
            &source,
            &source_normalizer_configuration,
        )
        .map_err(|error| {
            IosNativeFramePipelineOpenError::new("missingSourceNormalizerPacketPlugin", error)
        })?;
        let stream_info = packet_session.stream_info();
        let track = selected_video_track(&stream_info)
            .map_err(|error| IosNativeFramePipelineOpenError::new("unsupportedCodec", error))?;
        let video_stream_index = track.stream_index;
        let audio_track = selected_audio_track(&stream_info);
        let has_audio_track = audio_track.is_some();
        let audio_track_codec = audio_track.as_ref().map(|track| track.codec.clone());
        let audio_stream_index = audio_track.as_ref().map(|track| track.stream_index);
        if !track.codec.eq_ignore_ascii_case("h264") && !track.codec.eq_ignore_ascii_case("avc1") {
            let _ = packet_session.close();
            return Err(IosNativeFramePipelineOpenError::new(
                "unsupportedCodec",
                format!(
                    "iOS native-frame pipeline first pass only supports H264 packet streams, got {}",
                    track.codec
                ),
            ));
        }

        let decoder_registry = PluginRegistry::inspect_decoder_support(
            &config.decoder_plugin_library_paths,
            DecoderPluginMatchRequest::video(track.codec.clone()),
        );
        let audio_decoder_plugin_name = audio_track.as_ref().and_then(|track| {
            let audio_registry = PluginRegistry::inspect_decoder_support(
                &config.decoder_plugin_library_paths,
                DecoderPluginMatchRequest::audio(track.codec.clone()),
            );
            audio_pcm_decoder_plugin_name(&audio_registry, &track.codec)
        });
        let audio_decoder_plugin_ready = audio_decoder_plugin_name.is_some();
        let decoder_record = decoder_registry
            .best_native_decoder_for(&DecoderPluginMatchRequest::video(track.codec.clone()))
            .ok_or_else(|| {
                IosNativeFramePipelineOpenError::new(
                    "missingVideoToolboxDecoderPlugin",
                    format!(
                        "no native-frame decoder plugin supports {} video: {}",
                        track.codec,
                        registry_notes(&decoder_registry)
                    ),
                )
            })?;
        let decoder_plugin = LoadedDynamicPlugin::load(&decoder_record.path).map_err(|error| {
            IosNativeFramePipelineOpenError::new(
                "missingVideoToolboxDecoderPlugin",
                format!("failed to load decoder plugin: {error}"),
            )
        })?;
        let decoder_factory = decoder_plugin
            .native_decoder_plugin_factory()
            .ok_or_else(|| {
                IosNativeFramePipelineOpenError::new(
                    "missingVideoToolboxDecoderPlugin",
                    format!(
                        "{} is not a native-frame decoder plugin",
                        decoder_plugin.plugin_name()
                    ),
                )
            })?;
        let decoder_plugin_name = decoder_factory.name().to_owned();
        let decoder_bitstream_format = track
            .bitstream_format
            .clone()
            .unwrap_or_else(|| decoder_bitstream_format(&track.codec));
        let decoder_session = decoder_factory
            .open_native_session(&DecoderSessionConfig {
                codec: track.codec.clone(),
                media_kind: DecoderMediaKind::Video,
                extradata: track.extradata.clone(),
                bitstream_format: Some(decoder_bitstream_format),
                width: track.width,
                height: track.height,
                coded_width: track.coded_width.or(track.width),
                coded_height: track.coded_height.or(track.height),
                prefer_hardware: true,
                require_cpu_output: false,
                ..DecoderSessionConfig::default()
            })
            .map_err(|error| {
                IosNativeFramePipelineOpenError::new(
                    "unsupportedCodec",
                    format!("failed to open VideoToolbox decoder session: {error}"),
                )
            })?;

        let frame_processor_mode = if config.frame_processor_plugin_library_paths.is_empty() {
            FrameProcessorMode::Disabled
        } else {
            FrameProcessorMode::PreferProcessed
        };
        let frame_processor_chain = open_frame_processor_chain(
            &track,
            &config.frame_processor_plugin_library_paths,
            frame_processor_mode,
            FrameProcessorPolicy {
                max_in_flight_frames_per_processor: config.max_in_flight_frames.unwrap_or(1).max(1),
                ..FrameProcessorPolicy::default()
            },
        )
        .map_err(|error| IosNativeFramePipelineOpenError::new("startupFailure", error))?;
        let processor_plugin_names = frame_processor_chain
            .as_ref()
            .map(|chain| {
                chain
                    .processors
                    .iter()
                    .map(|node| node.plugin_name.clone())
                    .collect()
            })
            .unwrap_or_default();

        Ok(Self {
            source_uri: config.source_uri,
            duration_millis: stream_info.duration_millis,
            seekable: stream_info.seekable,
            has_audio_track,
            audio_track_codec,
            audio_stream_index,
            audio_decoder_plugin_name,
            audio_decoder_plugin_ready,
            video_stream_index,
            source_normalizer_plugin_name: source_record
                .plugin_name
                .clone()
                .or_else(|| stream_info.normalizer_name.clone()),
            decoder_plugin_name,
            processor_plugin_names,
            packet_session,
            decoder_session,
            frame_processor_chain,
            end_of_input_sent: false,
            end_of_stream_received: false,
            next_frame_handle: 1,
            pending_frames: HashMap::new(),
            counters: IosNativeFramePipelineCounters::default(),
        })
    }

    pub fn open_wire(&self, handle: u64) -> IosNativeFramePipelineWire {
        IosNativeFramePipelineWire {
            handle,
            route: PlayerPlaybackRoute::SdkManagedNativeFrame.wire_name(),
            source_input: "sourceNormalizerPacket",
            decoder_adapter: "VideoToolbox",
            decoder_plugin: self.decoder_plugin_name.clone(),
            audio_decoder: self.audio_decoder_kind(),
            audio_output: self.audio_output_kind(),
            audio_pipeline: self.audio_pipeline_kind(),
            audio_rate_control: self.audio_rate_control_kind(),
            selected_profile: "VideoToolboxCvPixelBuffer",
            presenter_profile: "MetalLayer",
            participation: "selected",
            source_uri: self.source_uri.clone(),
            duration_millis: self.duration_millis,
            seekable: self.seekable,
            has_audio_track: self.has_audio_track,
            audio_track_codec: self.audio_track_codec.clone(),
            selected_video_stream_index: self.video_stream_index,
            selected_video_media_kind: "video",
            audio_stream_index: self.audio_stream_index,
            audio_media_kind: self.audio_media_kind(),
            audio_decoder_plugin: self.audio_decoder_plugin_name.clone(),
            audio_decoder_plugin_ready: self.audio_decoder_plugin_ready,
            clock_source: self.clock_source(),
            source_normalizer_plugin: self.source_normalizer_plugin_name.clone(),
            processor_chain: self.processor_plugin_names.clone(),
            counters: self.counters.clone(),
            diagnostics: self.diagnostics(PlayerPluginParticipation::Selected),
        }
    }

    pub fn status_wire(
        &self,
        handle: u64,
        message: Option<String>,
    ) -> IosNativeFramePipelineStatusWire {
        IosNativeFramePipelineStatusWire {
            handle,
            route: PlayerPlaybackRoute::SdkManagedNativeFrame.wire_name(),
            participation: if self.counters.presented_frames > 0 {
                "participated"
            } else {
                "selected"
            },
            duration_millis: self.duration_millis,
            seekable: self.seekable,
            has_audio_track: self.has_audio_track,
            audio_track_codec: self.audio_track_codec.clone(),
            selected_video_stream_index: self.video_stream_index,
            selected_video_media_kind: "video",
            audio_stream_index: self.audio_stream_index,
            audio_media_kind: self.audio_media_kind(),
            audio_decoder_plugin: self.audio_decoder_plugin_name.clone(),
            audio_decoder_plugin_ready: self.audio_decoder_plugin_ready,
            clock_source: self.clock_source(),
            audio_decoder: self.audio_decoder_kind(),
            audio_output: self.audio_output_kind(),
            audio_pipeline: self.audio_pipeline_kind(),
            audio_rate_control: self.audio_rate_control_kind(),
            counters: self.counters.clone(),
            message,
        }
    }

    pub fn advance(&mut self) -> Result<Option<IosNativeFramePipelineFrame>, String> {
        if self.end_of_stream_received {
            return Ok(None);
        }
        for _ in 0..MAX_DECODE_RECEIVE_ATTEMPTS_PER_ADVANCE {
            match self.receive_frame()? {
                Some(frame) => return Ok(Some(frame)),
                None if self.end_of_stream_received => return Ok(None),
                None => {}
            }
            if !self.end_of_input_sent {
                self.send_packet_or_eos()?;
            } else {
                thread::sleep(DECODER_DRAIN_RETRY_INTERVAL);
            }
        }
        Ok(None)
    }

    pub fn release_presented_frame(
        &mut self,
        frame: IosNativeFramePipelineFrame,
    ) -> Result<(), String> {
        self.counters.presented_frames = self.counters.presented_frames.saturating_add(1);
        self.release_pipeline_frame(frame.frame)
    }

    pub fn release_dropped_frame(
        &mut self,
        frame: IosNativeFramePipelineFrame,
    ) -> Result<(), String> {
        self.release_pipeline_frame(frame.frame)
    }

    pub fn store_frame(&mut self, frame: IosNativeFramePipelineFrame) -> u64 {
        let handle = self.next_frame_handle.max(1);
        self.next_frame_handle = self.next_frame_handle.wrapping_add(1).max(1);
        self.pending_frames.insert(handle, frame);
        handle
    }

    pub fn pending_frame(&self, handle: u64) -> Option<&IosNativeFramePipelineFrame> {
        self.pending_frames.get(&handle)
    }

    pub fn is_end_of_stream(&self) -> bool {
        self.end_of_stream_received
    }

    pub fn release_pending_frame(&mut self, handle: u64, presented: bool) -> Result<(), String> {
        let frame = self
            .pending_frames
            .remove(&handle)
            .ok_or_else(|| "invalid iOS native-frame pending frame handle".to_owned())?;
        if presented {
            self.release_presented_frame(frame)
        } else {
            self.release_dropped_frame(frame)
        }
    }

    pub fn flush(&mut self) -> Result<(), String> {
        self.release_all_pending_frames();
        if let Some(chain) = self.frame_processor_chain.as_mut() {
            chain.flush();
        }
        self.decoder_session
            .flush()
            .map_err(|error| format!("decoder flush failed: {error}"))?;
        self.packet_session
            .flush()
            .map_err(|error| format!("source normalizer packet flush failed: {error}"))?;
        self.end_of_input_sent = false;
        self.end_of_stream_received = false;
        Ok(())
    }

    pub fn seek_to(&mut self, position_millis: u64) -> Result<(), String> {
        if !self.seekable {
            return Err("iOS native-frame pipeline source is not seekable".to_owned());
        }
        self.release_all_pending_frames();
        if let Some(chain) = self.frame_processor_chain.as_mut() {
            chain.flush();
        }
        self.decoder_session
            .flush()
            .map_err(|error| format!("decoder flush before seek failed: {error}"))?;
        self.packet_session.flush().map_err(|error| {
            format!("source normalizer packet flush before seek failed: {error}")
        })?;
        self.packet_session
            .seek(&SourceNormalizerPacketSeek {
                position_millis,
                exact: true,
            })
            .map_err(|error| format!("source normalizer packet seek failed: {error}"))?;
        self.end_of_input_sent = false;
        self.end_of_stream_received = false;
        self.counters.seek_count = self.counters.seek_count.saturating_add(1);
        Ok(())
    }

    pub fn close(&mut self) {
        self.release_all_pending_frames();
        if let Some(chain) = self.frame_processor_chain.as_mut() {
            chain.close();
        }
        let _ = self.decoder_session.close();
        let _ = self.packet_session.close();
    }

    fn receive_frame(&mut self) -> Result<Option<IosNativeFramePipelineFrame>, String> {
        let output = self
            .decoder_session
            .receive_native_frame()
            .map_err(|error| format!("VideoToolbox receive frame failed: {error}"))?;
        let DecoderReceiveNativeFrameOutput::Frame(frame) = output else {
            if matches!(output, DecoderReceiveNativeFrameOutput::Eof) {
                self.end_of_stream_received = true;
            }
            return Ok(None);
        };
        self.counters.decoded_frames = self.counters.decoded_frames.saturating_add(1);
        let pipeline_frame = match process_frame(
            self.frame_processor_chain.as_mut(),
            &mut self.counters,
            frame,
        ) {
            Ok(frame) => frame,
            Err((error, frame_for_release)) => {
                let _ = self.decoder_session.release_native_frame(frame_for_release);
                return Err(error);
            }
        };
        if pipeline_frame.presentation_frame.metadata.handle_kind
            != DecoderNativeHandleKind::CvPixelBuffer
        {
            let handle_kind = format!(
                "{:?}",
                pipeline_frame.presentation_frame.metadata.handle_kind
            );
            let _ = self.release_pipeline_frame(pipeline_frame);
            return Err(format!(
                "iOS native-frame presenter only accepts CVPixelBuffer handles, got {handle_kind}"
            ));
        }
        let presentation = &pipeline_frame.presentation_frame;
        Ok(Some(IosNativeFramePipelineFrame {
            handle: presentation.handle,
            presentation_time_us: presentation.metadata.pts_us.unwrap_or(0),
            duration_us: presentation.metadata.duration_us,
            width: presentation.metadata.width,
            height: presentation.metadata.height,
            frame_id: presentation.metadata.frame_id,
            frame: pipeline_frame,
        }))
    }

    fn send_packet_or_eos(&mut self) -> Result<(), String> {
        for _ in 0..MAX_PACKET_READ_ATTEMPTS_PER_ADVANCE {
            let lease = self
                .packet_session
                .read_packet()
                .map_err(|error| format!("source normalizer packet read failed: {error}"))?;
            match lease.metadata.status {
                SourceNormalizerReadPacketStatus::Packet => {
                    let data = lease.data.to_vec();
                    let packet_handle = lease.handle;
                    let packet = lease.metadata.packet.clone();
                    self.packet_session
                        .release_packet(packet_handle)
                        .map_err(|error| {
                            format!("source normalizer packet release failed: {error}")
                        })?;
                    let Some(packet) = packet else {
                        return Err("source normalizer packet metadata was missing".to_owned());
                    };
                    if packet.media_kind != SourceNormalizerPacketMediaKind::Video {
                        match packet.media_kind {
                            SourceNormalizerPacketMediaKind::Audio => {
                                self.counters.skipped_audio_packets =
                                    self.counters.skipped_audio_packets.saturating_add(1);
                            }
                            SourceNormalizerPacketMediaKind::Video => {}
                            SourceNormalizerPacketMediaKind::Subtitle => {
                                self.counters.skipped_other_packets =
                                    self.counters.skipped_other_packets.saturating_add(1);
                            }
                        }
                        continue;
                    }
                    if packet.stream_index != self.video_stream_index {
                        self.counters.skipped_video_packets =
                            self.counters.skipped_video_packets.saturating_add(1);
                        continue;
                    }
                    let decoder_packet = decoder_packet_from_source_normalizer_packet(packet)?;
                    self.decoder_session
                        .send_packet(&decoder_packet, &data)
                        .map_err(|error| format!("VideoToolbox send packet failed: {error}"))?;
                    return Ok(());
                }
                SourceNormalizerReadPacketStatus::NeedMoreData => {
                    thread::sleep(DECODER_DRAIN_RETRY_INTERVAL);
                }
                SourceNormalizerReadPacketStatus::EndOfStream => {
                    self.decoder_session
                        .send_packet(
                            &DecoderPacket {
                                end_of_stream: true,
                                ..DecoderPacket::default()
                            },
                            &[],
                        )
                        .map_err(|error| format!("VideoToolbox send EOS failed: {error}"))?;
                    self.end_of_input_sent = true;
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    fn release_all_pending_frames(&mut self) {
        let pending_frames = std::mem::take(&mut self.pending_frames);
        for (_, frame) in pending_frames {
            let _ = self.release_dropped_frame(frame);
        }
    }

    fn clock_source(&self) -> &'static str {
        if self.has_audio_track {
            "swiftNativeAudioBridge"
        } else {
            "video"
        }
    }

    fn audio_decoder_kind(&self) -> &'static str {
        if self.has_audio_track {
            "swiftNativeAudioBridge"
        } else {
            "none"
        }
    }

    fn audio_output_kind(&self) -> &'static str {
        if self.has_audio_track {
            "swiftNativeAudioBridge"
        } else {
            "none"
        }
    }

    fn audio_pipeline_kind(&self) -> &'static str {
        if self.has_audio_track {
            "swiftNativeAudioBridgeV1"
        } else {
            "none"
        }
    }

    fn audio_rate_control_kind(&self) -> &'static str {
        if self.has_audio_track {
            "swiftNativeAudioBridgeTimePitch"
        } else {
            "none"
        }
    }

    fn audio_media_kind(&self) -> &'static str {
        if self.has_audio_track {
            "audio"
        } else {
            "none"
        }
    }

    fn release_pipeline_frame(&mut self, frame: IosPipelineFrame) -> Result<(), String> {
        if let Some(chain) = self.frame_processor_chain.as_mut() {
            chain.release_processor_outputs(frame.processor_outputs);
        }
        self.decoder_session
            .release_native_frame(frame.decoder_frame)
            .map_err(|error| format!("VideoToolbox frame release failed: {error}"))?;
        self.counters.released_frames = self.counters.released_frames.saturating_add(1);
        Ok(())
    }

    fn diagnostics(
        &self,
        source_participation: PlayerPluginParticipation,
    ) -> Vec<IosNativeFramePipelineDiagnosticWire> {
        let mut diagnostics = Vec::new();
        diagnostics.push(IosNativeFramePipelineDiagnosticWire {
            path: String::new(),
            plugin_name: self.source_normalizer_plugin_name.clone(),
            plugin_kind: Some("source_normalizer".to_owned()),
            status: "sourceNormalizerSupported",
            message: Some(self.source_normalizer_packet_stream_message()),
            participation: participation_wire_name(source_participation),
            details: self.source_normalizer_packet_stream_details(),
        });
        diagnostics.push(IosNativeFramePipelineDiagnosticWire {
            path: String::new(),
            plugin_name: Some(self.decoder_plugin_name.clone()),
            plugin_kind: Some("decoder".to_owned()),
            status: "decoderSupported",
            message: Some("VideoToolbox native-frame decoder selected".to_owned()),
            participation: participation_wire_name(source_participation),
            details: IosNativeFramePipelineDiagnosticDetailsWire::from_pairs([
                (
                    "route",
                    PlayerPlaybackRoute::SdkManagedNativeFrame.wire_name(),
                ),
                ("decoderAdapter", "VideoToolbox"),
            ]),
        });
        if self.has_audio_track {
            diagnostics.push(IosNativeFramePipelineDiagnosticWire {
                path: String::new(),
                plugin_name: self.audio_decoder_plugin_name.clone(),
                plugin_kind: Some("decoder".to_owned()),
                status: if self.audio_decoder_plugin_ready {
                    "decoderSupported"
                } else {
                    "decoderUnsupported"
                },
                message: Some(match (
                    self.audio_track_codec.as_deref(),
                    self.audio_decoder_plugin_name.as_deref(),
                ) {
                    (Some(codec), Some(plugin)) => format!(
                        "audio PCM decoder plugin `{plugin}` is available for {codec}; iOS v1 keeps swiftNativeAudioBridge active"
                    ),
                    (Some(codec), None) => format!(
                        "no PCM audio decoder plugin is available for {codec}; iOS v1 uses swiftNativeAudioBridge"
                    ),
                    (None, Some(plugin)) => format!(
                        "audio PCM decoder plugin `{plugin}` is available; iOS v1 keeps swiftNativeAudioBridge active"
                    ),
                    (None, None) => {
                        "no PCM audio decoder plugin is available; iOS v1 uses swiftNativeAudioBridge"
                            .to_owned()
                    }
                }),
                participation: if self.audio_decoder_plugin_ready {
                    participation_wire_name(PlayerPluginParticipation::Available)
                } else {
                    participation_wire_name(PlayerPluginParticipation::Bypassed)
                },
                details: IosNativeFramePipelineDiagnosticDetailsWire::from_pairs([
                    (
                        "audioTrackCodec",
                        self.audio_track_codec.as_deref().unwrap_or("none"),
                    ),
                    (
                        "audioDecoderPlugin",
                        self.audio_decoder_plugin_name.as_deref().unwrap_or("none"),
                    ),
                    (
                        "audioDecoderPluginReady",
                        if self.audio_decoder_plugin_ready {
                            "true"
                        } else {
                            "false"
                        },
                    ),
                    ("audioDecoder", self.audio_decoder_kind()),
                    ("audioOutput", self.audio_output_kind()),
                    ("audioPipeline", self.audio_pipeline_kind()),
                ]),
            });
        }
        diagnostics.push(IosNativeFramePipelineDiagnosticWire {
            path: String::new(),
            plugin_name: Some("vesper-ios-native-frame-pipeline".to_owned()),
            plugin_kind: Some("native_frame_pipeline".to_owned()),
            status: "loaded",
            message: Some(format!(
                "iOS native-frame pipeline selected MetalLayer presentation; audioDecoder={}; audioOutput={}; audioPipeline={}; clockSource={}",
                self.audio_decoder_kind(),
                self.audio_output_kind(),
                self.audio_pipeline_kind(),
                self.clock_source()
            )),
            participation: participation_wire_name(source_participation),
            details: IosNativeFramePipelineDiagnosticDetailsWire::from_pairs([
                (
                    "route",
                    PlayerPlaybackRoute::SdkManagedNativeFrame.wire_name(),
                ),
                ("presenter", "MetalLayer"),
                ("clockSource", self.clock_source()),
                ("audioDecoder", self.audio_decoder_kind()),
                ("audioOutput", self.audio_output_kind()),
                ("audioPipeline", self.audio_pipeline_kind()),
            ]),
        });
        diagnostics
    }

    fn source_normalizer_packet_stream_message(&self) -> String {
        let audio_stream_index = self
            .audio_stream_index
            .map(|index| index.to_string())
            .unwrap_or_else(|| "none".to_owned());
        let audio_media_kind = self.audio_media_kind();
        let audio_track_codec = self.audio_track_codec.as_deref().unwrap_or("none");
        let duration_ms = self
            .duration_millis
            .map(|duration| duration.to_string())
            .unwrap_or_else(|| "unknown".to_owned());
        format!(
            "SourceNormalizer packet stream selected for iOS native-frame pipeline; selectedVideoStreamIndex={}; selectedVideoMediaKind=video; audioStreamIndex={audio_stream_index}; audioMediaKind={audio_media_kind}; audioTrackCodec={audio_track_codec}; seekable={}; durationMs={duration_ms}; route={}",
            self.video_stream_index,
            self.seekable,
            PlayerPlaybackRoute::SdkManagedNativeFrame.wire_name()
        )
    }

    fn source_normalizer_packet_stream_details(
        &self,
    ) -> IosNativeFramePipelineDiagnosticDetailsWire {
        IosNativeFramePipelineDiagnosticDetailsWire::from_pairs([
            (
                "selectedVideoStreamIndex",
                self.video_stream_index.to_string(),
            ),
            ("selectedVideoMediaKind", "video".to_owned()),
            (
                "audioStreamIndex",
                self.audio_stream_index
                    .map(|index| index.to_string())
                    .unwrap_or_else(|| "none".to_owned()),
            ),
            ("audioMediaKind", self.audio_media_kind().to_owned()),
            (
                "audioTrackCodec",
                self.audio_track_codec
                    .clone()
                    .unwrap_or_else(|| "none".to_owned()),
            ),
            ("seekable", self.seekable.to_string()),
            (
                "durationMs",
                self.duration_millis
                    .map(|duration| duration.to_string())
                    .unwrap_or_else(|| "unknown".to_owned()),
            ),
            (
                "route",
                PlayerPlaybackRoute::SdkManagedNativeFrame
                    .wire_name()
                    .to_owned(),
            ),
        ])
    }
}

impl Drop for IosNativeFramePipelineSession {
    fn drop(&mut self) {
        self.close();
    }
}

pub fn native_frame_pipeline_open_json(
    handle: u64,
    session: &IosNativeFramePipelineSession,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&session.open_wire(handle))
}

pub fn native_frame_pipeline_status_json(
    handle: u64,
    session: &IosNativeFramePipelineSession,
    message: Option<String>,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&session.status_wire(handle, message))
}

pub fn native_frame_pipeline_frame_json(
    frame_handle: Option<u64>,
    frame: Option<&IosNativeFramePipelineFrame>,
    counters: IosNativeFramePipelineCounters,
    end_of_stream: bool,
    message: Option<String>,
) -> Result<String, serde_json::Error> {
    let wire = match frame {
        Some(frame) => IosNativeFramePipelineFrameWire {
            status: "frame",
            message,
            handle: frame_handle,
            pixel_buffer: Some(frame.handle),
            presentation_time_us: Some(frame.presentation_time_us),
            duration_us: frame.duration_us,
            width: Some(frame.width),
            height: Some(frame.height),
            frame_id: frame.frame_id,
            counters,
        },
        None => IosNativeFramePipelineFrameWire {
            status: if end_of_stream { "endOfStream" } else { "pending" },
            message,
            handle: None,
            pixel_buffer: None,
            presentation_time_us: None,
            duration_us: None,
            width: None,
            height: None,
            frame_id: None,
            counters,
        },
    };
    serde_json::to_string(&wire)
}

fn open_packet_source_normalizer(
    source: &MediaSource,
    configuration: &MobileSourceNormalizerConfiguration,
) -> Result<
    (
        PluginDiagnosticRecord,
        Box<dyn SourceNormalizerPacketSession>,
    ),
    String,
> {
    if !matches!(
        configuration.mode,
        SourceNormalizerMode::PreflightOnly
            | SourceNormalizerMode::PreferNormalized
            | SourceNormalizerMode::RequireNormalized
    ) {
        return Err(
            "iOS native-frame pipeline requires SourceNormalizer preflight or normalized mode"
                .to_owned(),
        );
    }
    let registry =
        PluginRegistry::inspect_source_normalizer_support(&configuration.plugin_library_paths);
    let record = match configured_runtime_profile(configuration) {
        Some(profile) => registry
            .best_source_normalizer_for_profile(profile)
            .filter(|record| record.capability_summary.is_some())
            .ok_or_else(|| {
                format!(
                    "no SourceNormalizer packet plugin supports runtime profile '{profile}': {}",
                    registry_notes(&registry)
                )
            })?,
        None => registry.best_source_normalizer_packet().ok_or_else(|| {
            format!(
                "no SourceNormalizer packet plugin is available: {}",
                registry_notes(&registry)
            )
        })?,
    };
    let plugin = LoadedDynamicPlugin::load(&record.path)
        .map_err(|error| format!("failed to load SourceNormalizer plugin: {error}"))?;
    let factory = plugin
        .source_normalizer_packet_plugin_factory()
        .ok_or_else(|| {
            format!(
                "{} is not a packet-stream SourceNormalizer plugin",
                plugin.plugin_name()
            )
        })?;
    let session = factory
        .open_packet_session(&SourceNormalizerPacketSessionConfig {
            runtime_profile: configured_runtime_profile(configuration)
                .unwrap_or_default()
                .to_owned(),
            input: source.uri().to_owned(),
            headers: Vec::new(),
            startup_timeout_ms: Some(SOURCE_NORMALIZER_STARTUP_TIMEOUT_MS),
            session_timeout_ms: Some(SOURCE_NORMALIZER_PACKET_SESSION_TIMEOUT_MS),
            preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
        })
        .map_err(|error| format!("open SourceNormalizer packet session failed: {error}"))?;
    Ok((record.clone(), session))
}

fn open_frame_processor_chain(
    track: &player_plugin::SourceNormalizerPacketTrackInfo,
    paths: &[PathBuf],
    mode: FrameProcessorMode,
    policy: FrameProcessorPolicy,
) -> Result<Option<IosFrameProcessorChain>, String> {
    if mode == FrameProcessorMode::Disabled || paths.is_empty() {
        return Ok(None);
    }
    let input_metadata = NativeFrameMetadata {
        media_kind: DecoderMediaKind::Video,
        format: DecoderFrameFormat::Nv12,
        codec: track.codec.clone(),
        pts_us: None,
        duration_us: None,
        width: track.width.unwrap_or(0),
        height: track.height.unwrap_or(0),
        coded_width: track.coded_width.or(track.width),
        coded_height: track.coded_height.or(track.height),
        visible_rect: None,
        handle_kind: NativeHandleKind::CvPixelBuffer,
        pipeline_profile: Some(NativeFramePipelineProfile::VideoToolboxCvPixelBuffer),
        color_space: None,
        hdr_metadata: None,
        sync_info: None,
        transform: None,
        frame_id: None,
        release_tracking: None,
    };
    let mut processors = Vec::new();
    for (processor_index, path) in paths.iter().enumerate().take(policy.max_chain_depth) {
        let plugin = LoadedDynamicPlugin::load(path).map_err(|error| {
            format!(
                "failed to load frame processor plugin {}: {error}",
                path.display()
            )
        })?;
        let factory = plugin.frame_processor_plugin_factory().ok_or_else(|| {
            format!(
                "plugin `{}` does not export a frame processor API",
                plugin.plugin_name()
            )
        })?;
        let capabilities = factory.capabilities();
        if !capabilities.supports_video_frames {
            return Err(format!(
                "frame processor `{}` does not support video frames",
                factory.name()
            ));
        }
        if capabilities.may_change_dimensions {
            return Err(format!(
                "frame processor `{}` changes frame dimensions, which mobile native-frame v1 does not allow",
                factory.name()
            ));
        }
        if !capabilities.supports_input_metadata(&input_metadata) {
            return Err(format!(
                "frame processor `{}` does not accept CVPixelBuffer VideoToolbox input",
                factory.name()
            ));
        }
        let session = factory
            .open_session(&FrameProcessorSessionConfig {
                processor_index,
                input_metadata: input_metadata.clone(),
                max_in_flight_frames: Some(policy.max_in_flight_frames_per_processor),
            })
            .map_err(|error| {
                format!(
                    "failed to open frame processor `{}`: {error}",
                    factory.name()
                )
            })?;
        processors.push(IosFrameProcessorNode {
            plugin_name: factory.name().to_owned(),
            processor_index,
            session,
        });
    }
    if processors.is_empty() {
        Ok(None)
    } else {
        Ok(Some(IosFrameProcessorChain {
            processors,
            mode,
            policy,
        }))
    }
}

fn process_frame(
    chain: Option<&mut IosFrameProcessorChain>,
    counters: &mut IosNativeFramePipelineCounters,
    decoder_frame: DecoderNativeFrame,
) -> Result<IosPipelineFrame, (String, DecoderNativeFrame)> {
    let Some(chain) = chain else {
        return Ok(IosPipelineFrame {
            decoder_frame: decoder_frame.clone(),
            presentation_frame: decoder_frame,
            processor_outputs: Vec::new(),
        });
    };
    chain.process(counters, decoder_frame)
}

impl IosFrameProcessorChain {
    fn process(
        &mut self,
        counters: &mut IosNativeFramePipelineCounters,
        decoder_frame: DecoderNativeFrame,
    ) -> Result<IosPipelineFrame, (String, DecoderNativeFrame)> {
        let mut current_frame = NativeFrame::from(decoder_frame.clone());
        let mut processor_outputs = Vec::new();
        let mut using_processor_output = false;
        for node_index in 0..self.processors.len() {
            let submit = FrameProcessorSubmitFrame {
                metadata: current_frame.metadata.clone(),
                present_deadline_us: current_frame
                    .metadata
                    .pts_us
                    .map(|pts| pts.saturating_add(self.policy.frame_deadline.as_micros() as i64)),
            };
            let submit_result = {
                let node = &mut self.processors[node_index];
                node.session
                    .submit_frame(&current_frame, &submit)
                    .map_err(|error| {
                        (
                            frame_processor_error(self.mode, node, error),
                            decoder_frame.clone(),
                        )
                    })?
            };
            match submit_result.status {
                FrameProcessorSubmitStatus::Accepted => {}
                FrameProcessorSubmitStatus::Bypassed | FrameProcessorSubmitStatus::Backpressure => {
                    counters.bypassed_frames = counters.bypassed_frames.saturating_add(1);
                    if submit_result.status == FrameProcessorSubmitStatus::Backpressure {
                        counters.backpressure_count = counters.backpressure_count.saturating_add(1);
                    }
                    if self.mode == FrameProcessorMode::RequireProcessed {
                        let plugin_name = self.processors[node_index].plugin_name.clone();
                        self.release_processor_outputs(processor_outputs);
                        return Err((
                            format!("frame processor `{plugin_name}` bypassed in strict mode"),
                            decoder_frame,
                        ));
                    }
                    current_frame = NativeFrame::from(decoder_frame.clone());
                    using_processor_output = false;
                    continue;
                }
                FrameProcessorSubmitStatus::Rejected => {
                    counters.bypassed_frames = counters.bypassed_frames.saturating_add(1);
                    if self.mode == FrameProcessorMode::RequireProcessed {
                        let plugin_name = self.processors[node_index].plugin_name.clone();
                        self.release_processor_outputs(processor_outputs);
                        return Err((
                            format!(
                                "frame processor `{plugin_name}` rejected a frame in strict mode"
                            ),
                            decoder_frame,
                        ));
                    }
                    current_frame = NativeFrame::from(decoder_frame.clone());
                    using_processor_output = false;
                    continue;
                }
            }

            let output = {
                let node = &mut self.processors[node_index];
                node.session.receive_frame().map_err(|error| {
                    (
                        frame_processor_error(self.mode, node, error),
                        decoder_frame.clone(),
                    )
                })?
            };
            let FrameProcessorReceiveOutput::Frame(output) = output else {
                counters.bypassed_frames = counters.bypassed_frames.saturating_add(1);
                if self.mode == FrameProcessorMode::RequireProcessed {
                    let plugin_name = self.processors[node_index].plugin_name.clone();
                    self.release_processor_outputs(processor_outputs);
                    return Err((
                        format!(
                            "frame processor `{plugin_name}` did not return a ready frame in strict mode"
                        ),
                        decoder_frame,
                    ));
                }
                current_frame = NativeFrame::from(decoder_frame.clone());
                using_processor_output = false;
                continue;
            };

            if is_late_output(&output.timings, &self.policy) {
                counters.deadline_misses = counters.deadline_misses.saturating_add(1);
            }
            if should_drop_output(&output.timings, &self.policy) {
                counters.late_dropped = counters.late_dropped.saturating_add(1);
                release_output_if_needed(&mut self.processors[node_index], output.frame);
                current_frame = NativeFrame::from(decoder_frame.clone());
                using_processor_output = false;
                continue;
            }
            counters.processed_frames = counters.processed_frames.saturating_add(1);
            if output_frame_requires_processor_release(&output.frame) {
                let processor_index = self.processors[node_index].processor_index;
                processor_outputs.push(ProcessorOwnedNativeFrame {
                    processor_index,
                    frame: output.frame.clone(),
                });
            }
            current_frame = output.frame;
            using_processor_output = self.mode != FrameProcessorMode::DiagnosticsOnly;
            if !using_processor_output {
                current_frame = NativeFrame::from(decoder_frame.clone());
            }
        }

        let presentation_frame = if using_processor_output
            && matches!(
                self.mode,
                FrameProcessorMode::PreferProcessed | FrameProcessorMode::RequireProcessed
            ) {
            DecoderNativeFrame::from(current_frame)
        } else {
            decoder_frame.clone()
        };
        Ok(IosPipelineFrame {
            decoder_frame,
            presentation_frame,
            processor_outputs,
        })
    }

    fn release_processor_outputs(&mut self, mut outputs: Vec<ProcessorOwnedNativeFrame>) {
        while let Some(output) = outputs.pop() {
            if let Some(node) = self
                .processors
                .iter_mut()
                .find(|node| node.processor_index == output.processor_index)
            {
                let _ = node.session.release_frame(output.frame);
            }
        }
    }

    fn flush(&mut self) {
        for node in &mut self.processors {
            let _ = node.session.flush();
        }
    }

    fn close(&mut self) {
        for node in &mut self.processors {
            let _ = node.session.close();
        }
    }
}

fn release_output_if_needed(node: &mut IosFrameProcessorNode, frame: NativeFrame) {
    if output_frame_requires_processor_release(&frame) {
        let _ = node.session.release_frame(frame);
    }
}

fn selected_video_track(
    stream_info: &player_plugin::SourceNormalizerPacketStreamInfo,
) -> Result<player_plugin::SourceNormalizerPacketTrackInfo, String> {
    stream_info
        .selected_track_index
        .and_then(|selected| {
            stream_info
                .tracks
                .iter()
                .find(|track| track.stream_index == selected)
        })
        .or_else(|| {
            stream_info
                .tracks
                .iter()
                .find(|track| track.media_kind == SourceNormalizerPacketMediaKind::Video)
        })
        .cloned()
        .ok_or_else(|| "SourceNormalizer packet stream has no video track".to_owned())
}

fn selected_audio_track(
    stream_info: &player_plugin::SourceNormalizerPacketStreamInfo,
) -> Option<player_plugin::SourceNormalizerPacketTrackInfo> {
    stream_info
        .tracks
        .iter()
        .find(|track| track.media_kind == SourceNormalizerPacketMediaKind::Audio)
        .cloned()
}

fn audio_pcm_decoder_plugin_name(registry: &PluginRegistry, codec: &str) -> Option<String> {
    registry
        .best_pcm_audio_decoder_for(&DecoderPluginMatchRequest::audio(codec))
        .and_then(|record| record.plugin_name.clone())
}

fn decoder_packet_from_source_normalizer_packet(
    packet: player_plugin::SourceNormalizerPacket,
) -> Result<DecoderPacket, String> {
    DecoderPacket::try_from(packet).map_err(|error| error.to_string())
}

fn decoder_bitstream_format(codec: &str) -> DecoderBitstreamFormat {
    match codec.to_ascii_uppercase().as_str() {
        "HEVC" | "H265" | "HVC1" | "HEV1" => DecoderBitstreamFormat::Hvcc,
        _ => DecoderBitstreamFormat::Avcc,
    }
}

fn configured_runtime_profile(configuration: &MobileSourceNormalizerConfiguration) -> Option<&str> {
    configuration
        .runtime_profile
        .as_deref()
        .map(str::trim)
        .filter(|profile| !profile.is_empty())
}

fn registry_notes(registry: &PluginRegistry) -> String {
    let notes = registry
        .records()
        .iter()
        .map(PluginDiagnosticRecord::summary)
        .collect::<Vec<_>>();
    if notes.is_empty() {
        "no plugin paths were inspected".to_owned()
    } else {
        notes.join("; ")
    }
}

fn frame_processor_error(
    mode: FrameProcessorMode,
    node: &IosFrameProcessorNode,
    error: FrameProcessorError,
) -> String {
    if mode == FrameProcessorMode::RequireProcessed {
        format!(
            "frame processor `{}` at index {} failed in strict mode: {error}",
            node.plugin_name, node.processor_index
        )
    } else {
        format!(
            "frame processor `{}` at index {} failed: {error}",
            node.plugin_name, node.processor_index
        )
    }
}

fn output_frame_requires_processor_release(frame: &NativeFrame) -> bool {
    frame
        .metadata
        .release_tracking
        .as_ref()
        .is_none_or(|tracking| tracking.requires_release)
}

fn is_late_output(timings: &FrameProcessorFrameTimings, policy: &FrameProcessorPolicy) -> bool {
    timings
        .submit_to_ready_us
        .is_some_and(|elapsed| elapsed > policy.frame_deadline.as_micros() as u64)
}

fn should_drop_output(timings: &FrameProcessorFrameTimings, policy: &FrameProcessorPolicy) -> bool {
    timings.submit_to_ready_us.is_some_and(|elapsed| {
        elapsed > (policy.frame_deadline + policy.late_output_tolerance).as_micros() as u64
    })
}

fn participation_wire_name(participation: PlayerPluginParticipation) -> &'static str {
    participation.wire_name()
}

#[allow(dead_code)]
fn diagnostic_from_runtime(value: &PlayerPluginDiagnostic) -> IosNativeFramePipelineDiagnosticWire {
    IosNativeFramePipelineDiagnosticWire {
        path: value.path.clone(),
        plugin_name: value.plugin_name.clone(),
        plugin_kind: value.plugin_kind.clone(),
        status: value.status.wire_name(),
        message: value.message.clone(),
        participation: participation_wire_name(value.participation),
        details: IosNativeFramePipelineDiagnosticDetailsWire::from_pairs(
            value
                .details
                .iter()
                .map(|detail| (detail.key.clone(), detail.value.clone())),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use player_plugin::{
        DecoderCapabilities, DecoderCodecCapability, DecoderError, DecoderPacketResult,
        FrameProcessorSubmitResult, SourceNormalizerOperationStatus, SourceNormalizerPacket,
        SourceNormalizerPacketLease, SourceNormalizerPacketStreamInfo,
        SourceNormalizerPacketTrackInfo, SourceNormalizerReadPacketMetadata, VesperPluginKind,
    };
    use player_plugin_loader::{
        DecoderPluginCapabilitySummary, PluginCapabilitySummary, PluginDiagnosticStatus,
    };
    use player_runtime::PlayerPluginDiagnosticStatus;

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum FlushEvent {
        ReleaseDecoder(usize),
        ReleaseProcessor(usize),
        Processor,
        Decoder,
        Packet,
        Seek(u64),
    }

    #[test]
    fn seek_flushes_processor_before_decoder_and_packet_session() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut session = test_session(events.clone());

        session.seek_to(42_000).expect("seek should succeed");

        assert_eq!(
            events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_slice(),
            &[
                FlushEvent::Processor,
                FlushEvent::Decoder,
                FlushEvent::Packet,
                FlushEvent::Seek(42_000)
            ]
        );
        assert_eq!(session.counters.seek_count, 1);
    }

    #[test]
    fn seek_releases_pending_frames_before_flush_and_packet_seek() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut session = test_session(events.clone());
        let handle = session.store_frame(test_pipeline_frame(91, Some(42_000)));

        assert!(session.pending_frame(handle).is_some());
        session.seek_to(42_000).expect("seek should succeed");

        assert!(session.pending_frame(handle).is_none());
        assert_eq!(session.counters.released_frames, 1);
        assert_eq!(
            events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_slice(),
            &[
                FlushEvent::ReleaseProcessor(1_091),
                FlushEvent::ReleaseDecoder(91),
                FlushEvent::Processor,
                FlushEvent::Decoder,
                FlushEvent::Packet,
                FlushEvent::Seek(42_000)
            ]
        );
    }

    #[test]
    fn flush_uses_shared_processor_decoder_packet_order() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut session = test_session(events.clone());

        session.flush().expect("flush should succeed");

        assert_eq!(
            events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_slice(),
            &[
                FlushEvent::Processor,
                FlushEvent::Decoder,
                FlushEvent::Packet
            ]
        );
    }

    #[test]
    fn diagnostic_from_runtime_uses_shared_wire_names() {
        let diagnostic = diagnostic_from_runtime(&PlayerPluginDiagnostic {
            path: "/tmp/plugin.dylib".to_owned(),
            plugin_name: Some("test-plugin".to_owned()),
            plugin_kind: Some("decoder".to_owned()),
            status: PlayerPluginDiagnosticStatus::DecoderSupported,
            message: Some("selected test decoder".to_owned()),
            capability: None,
            participation: PlayerPluginParticipation::Participated,
            details: vec![player_runtime::PlayerPluginDiagnosticDetail {
                key: "route".to_owned(),
                value: "sdkManagedNativeFrame".to_owned(),
            }],
        });

        assert_eq!(diagnostic.status, "decoderSupported");
        assert_eq!(diagnostic.participation, "participated");
        assert!(
            diagnostic
                .details
                .details
                .iter()
                .any(|(key, value)| { key == "route" && value == "sdkManagedNativeFrame" })
        );
    }

    #[test]
    fn open_and_status_wire_use_swift_native_audio_bridge_clock() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let session = test_session(events);

        let open = session.open_wire(7);
        let status = session.status_wire(7, None);

        assert_eq!(
            open.route,
            PlayerPlaybackRoute::SdkManagedNativeFrame.wire_name()
        );
        assert_eq!(open.clock_source, "swiftNativeAudioBridge");
        assert_eq!(open.audio_decoder, "swiftNativeAudioBridge");
        assert_eq!(open.audio_output, "swiftNativeAudioBridge");
        assert_eq!(open.audio_pipeline, "swiftNativeAudioBridgeV1");
        assert_eq!(open.audio_rate_control, "swiftNativeAudioBridgeTimePitch");
        assert_eq!(open.audio_track_codec.as_deref(), Some("aac"));
        assert_eq!(open.selected_video_stream_index, 0);
        assert_eq!(open.selected_video_media_kind, "video");
        assert_eq!(open.audio_stream_index, Some(1));
        assert_eq!(open.audio_media_kind, "audio");
        assert_eq!(open.audio_decoder_plugin.as_deref(), None);
        assert!(!open.audio_decoder_plugin_ready);
        assert_eq!(status.clock_source, "swiftNativeAudioBridge");
        assert_eq!(status.audio_decoder, "swiftNativeAudioBridge");
        assert_eq!(status.audio_output, "swiftNativeAudioBridge");
        assert_eq!(status.audio_pipeline, "swiftNativeAudioBridgeV1");
        assert_eq!(status.audio_rate_control, "swiftNativeAudioBridgeTimePitch");
        assert_eq!(status.audio_track_codec.as_deref(), Some("aac"));
        assert_eq!(status.selected_video_stream_index, 0);
        assert_eq!(status.selected_video_media_kind, "video");
        assert_eq!(status.audio_stream_index, Some(1));
        assert_eq!(status.audio_media_kind, "audio");
        assert_eq!(status.audio_decoder_plugin.as_deref(), None);
        assert!(!status.audio_decoder_plugin_ready);
        assert!(open.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .as_deref()
                .is_some_and(|message| message.contains("audioDecoder=swiftNativeAudioBridge"))
        }));
        assert!(open.diagnostics.iter().any(|diagnostic| {
            diagnostic.plugin_kind.as_deref() == Some("source_normalizer")
                && diagnostic.message.as_deref().is_some_and(|message| {
                    message.contains("selectedVideoStreamIndex=0")
                        && message.contains("selectedVideoMediaKind=video")
                        && message.contains("audioStreamIndex=1")
                        && message.contains("audioMediaKind=audio")
                        && message.contains("route=sdkManagedNativeFrame")
                })
        }));
        assert!(open.diagnostics.iter().any(|diagnostic| {
            diagnostic.status == "decoderUnsupported"
                && diagnostic.participation == "bypassed"
                && diagnostic
                    .message
                    .as_deref()
                    .is_some_and(|message| message.contains("no PCM audio decoder plugin"))
        }));
    }

    #[test]
    fn open_wire_reports_available_audio_pcm_decoder_without_switching_v1_audio_bridge() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut session = test_session(events);
        session.audio_decoder_plugin_name = Some("test-audio-decoder".to_owned());
        session.audio_decoder_plugin_ready = true;

        let open = session.open_wire(7);
        let status = session.status_wire(7, None);

        assert_eq!(open.clock_source, "swiftNativeAudioBridge");
        assert_eq!(open.audio_decoder, "swiftNativeAudioBridge");
        assert_eq!(
            open.audio_decoder_plugin.as_deref(),
            Some("test-audio-decoder")
        );
        assert!(open.audio_decoder_plugin_ready);
        assert_eq!(status.clock_source, "swiftNativeAudioBridge");
        assert_eq!(
            status.audio_decoder_plugin.as_deref(),
            Some("test-audio-decoder")
        );
        assert!(status.audio_decoder_plugin_ready);
        assert!(open.diagnostics.iter().any(|diagnostic| {
            diagnostic.plugin_name.as_deref() == Some("test-audio-decoder")
                && diagnostic.status == "decoderSupported"
                && diagnostic.participation == "available"
                && diagnostic.message.as_deref().is_some_and(|message| {
                    message.contains("audio PCM decoder plugin")
                        && message.contains("swiftNativeAudioBridge active")
                })
        }));
    }

    #[test]
    fn audio_pcm_decoder_selection_ignores_packet_only_audio_decoders() {
        let registry = PluginRegistry::from_records(vec![
            decoder_record(
                "packet-only-audio-decoder",
                DecoderCapabilities {
                    codecs: vec![DecoderCodecCapability {
                        codec: "aac".to_owned(),
                        media_kind: DecoderMediaKind::Audio,
                        profiles: Vec::new(),
                        output_formats: Vec::new(),
                    }],
                    supports_audio_frames: false,
                    ..DecoderCapabilities::default()
                },
            ),
            decoder_record(
                "pcm-audio-decoder",
                DecoderCapabilities {
                    codecs: vec![DecoderCodecCapability {
                        codec: "AAC".to_owned(),
                        media_kind: DecoderMediaKind::Audio,
                        profiles: Vec::new(),
                        output_formats: vec![DecoderFrameFormat::F32],
                    }],
                    supports_audio_frames: true,
                    ..DecoderCapabilities::default()
                },
            ),
        ]);

        assert_eq!(
            audio_pcm_decoder_plugin_name(&registry, "aac").as_deref(),
            Some("pcm-audio-decoder")
        );
        assert_eq!(audio_pcm_decoder_plugin_name(&registry, "mp3"), None);
    }

    fn test_session(events: Arc<Mutex<Vec<FlushEvent>>>) -> IosNativeFramePipelineSession {
        IosNativeFramePipelineSession {
            source_uri: "file:///tmp/video.mp4".to_owned(),
            duration_millis: Some(60_000),
            seekable: true,
            has_audio_track: true,
            audio_track_codec: Some("aac".to_owned()),
            audio_stream_index: Some(1),
            audio_decoder_plugin_name: None,
            audio_decoder_plugin_ready: false,
            video_stream_index: 0,
            source_normalizer_plugin_name: Some("test-source-normalizer".to_owned()),
            decoder_plugin_name: "test-decoder".to_owned(),
            processor_plugin_names: vec!["test-processor".to_owned()],
            packet_session: Box::new(TestPacketSession {
                events: events.clone(),
            }),
            decoder_session: Box::new(TestDecoderSession {
                events: events.clone(),
            }),
            frame_processor_chain: Some(IosFrameProcessorChain {
                processors: vec![IosFrameProcessorNode {
                    plugin_name: "test-processor".to_owned(),
                    processor_index: 0,
                    session: Box::new(TestProcessorSession { events }),
                }],
                mode: FrameProcessorMode::PreferProcessed,
                policy: FrameProcessorPolicy::default(),
            }),
            end_of_input_sent: true,
            end_of_stream_received: true,
            next_frame_handle: 1,
            pending_frames: HashMap::new(),
            counters: IosNativeFramePipelineCounters::default(),
        }
    }

    fn decoder_record(
        plugin_name: &'static str,
        capabilities: DecoderCapabilities,
    ) -> PluginDiagnosticRecord {
        PluginDiagnosticRecord {
            path: PathBuf::from(plugin_name),
            status: PluginDiagnosticStatus::DecoderSupported,
            plugin_name: Some(plugin_name.to_owned()),
            plugin_kind: Some(VesperPluginKind::Decoder),
            capability_summary: Some(PluginCapabilitySummary::Decoder(
                DecoderPluginCapabilitySummary::from(&capabilities),
            )),
            message: None,
        }
    }

    fn test_pipeline_frame(handle: usize, pts_us: Option<i64>) -> IosNativeFramePipelineFrame {
        let decoder_frame = test_decoder_frame(handle, pts_us);
        let processor_frame = test_processor_frame(handle + 1_000, pts_us);
        IosNativeFramePipelineFrame {
            handle: decoder_frame.handle,
            presentation_time_us: decoder_frame.metadata.pts_us.unwrap_or(0),
            duration_us: decoder_frame.metadata.duration_us,
            width: decoder_frame.metadata.width,
            height: decoder_frame.metadata.height,
            frame_id: decoder_frame.metadata.frame_id,
            frame: IosPipelineFrame {
                decoder_frame: decoder_frame.clone(),
                presentation_frame: decoder_frame,
                processor_outputs: vec![ProcessorOwnedNativeFrame {
                    processor_index: 0,
                    frame: processor_frame,
                }],
            },
        }
    }

    fn test_decoder_frame(handle: usize, pts_us: Option<i64>) -> DecoderNativeFrame {
        DecoderNativeFrame {
            metadata: player_plugin::DecoderNativeFrameMetadata {
                media_kind: DecoderMediaKind::Video,
                format: DecoderFrameFormat::Nv12,
                codec: "h264".to_owned(),
                pts_us,
                duration_us: Some(41_667),
                width: 2,
                height: 2,
                coded_width: Some(2),
                coded_height: Some(2),
                visible_rect: None,
                handle_kind: DecoderNativeHandleKind::CvPixelBuffer,
                pipeline_profile: Some(NativeFramePipelineProfile::VideoToolboxCvPixelBuffer),
                color_space: Some("bt709".to_owned()),
                hdr_metadata: None,
                sync_info: None,
                transform: None,
                frame_id: Some(handle as u64),
                release_tracking: Some(player_plugin::DecoderNativeFrameReleaseTracking {
                    frame_id: Some(handle as u64),
                    requires_release: true,
                }),
            },
            handle,
        }
    }

    fn test_processor_frame(handle: usize, pts_us: Option<i64>) -> NativeFrame {
        NativeFrame {
            metadata: NativeFrameMetadata {
                media_kind: DecoderMediaKind::Video,
                format: DecoderFrameFormat::Nv12,
                codec: "h264".to_owned(),
                pts_us,
                duration_us: Some(41_667),
                width: 2,
                height: 2,
                coded_width: Some(2),
                coded_height: Some(2),
                visible_rect: None,
                handle_kind: NativeHandleKind::CvPixelBuffer,
                pipeline_profile: Some(NativeFramePipelineProfile::VideoToolboxCvPixelBuffer),
                color_space: Some("bt709".to_owned()),
                hdr_metadata: None,
                sync_info: None,
                transform: None,
                frame_id: Some(handle as u64),
                release_tracking: Some(player_plugin::NativeFrameReleaseTracking {
                    frame_id: Some(handle as u64),
                    requires_release: true,
                }),
            },
            handle,
        }
    }

    struct TestPacketSession {
        events: Arc<Mutex<Vec<FlushEvent>>>,
    }

    impl SourceNormalizerPacketSession for TestPacketSession {
        fn stream_info(&self) -> SourceNormalizerPacketStreamInfo {
            SourceNormalizerPacketStreamInfo {
                tracks: vec![
                    SourceNormalizerPacketTrackInfo {
                        stream_index: 0,
                        media_kind: SourceNormalizerPacketMediaKind::Video,
                        codec: "h264".to_owned(),
                        extradata: Vec::new(),
                        bitstream_format: Some(DecoderBitstreamFormat::Avcc),
                        width: Some(2),
                        height: Some(2),
                        coded_width: Some(2),
                        coded_height: Some(2),
                        sample_rate: None,
                        channels: None,
                        channel_layout: None,
                        codec_delay_samples: None,
                        priming_samples: None,
                        trailing_padding_samples: None,
                        seek_preroll_samples: None,
                        frame_rate: None,
                        time_base_num: None,
                        time_base_den: None,
                    },
                    SourceNormalizerPacketTrackInfo {
                        stream_index: 1,
                        media_kind: SourceNormalizerPacketMediaKind::Audio,
                        codec: "aac".to_owned(),
                        extradata: Vec::new(),
                        bitstream_format: None,
                        width: None,
                        height: None,
                        coded_width: None,
                        coded_height: None,
                        sample_rate: Some(48_000),
                        channels: Some(2),
                        channel_layout: Some("stereo".to_owned()),
                        codec_delay_samples: None,
                        priming_samples: None,
                        trailing_padding_samples: None,
                        seek_preroll_samples: Some(1_024),
                        frame_rate: None,
                        time_base_num: Some(1),
                        time_base_den: Some(48_000),
                    },
                ],
                selected_track_index: Some(0),
                duration_millis: Some(60_000),
                seekable: true,
                ..SourceNormalizerPacketStreamInfo::default()
            }
        }

        fn read_packet(
            &mut self,
        ) -> Result<SourceNormalizerPacketLease<'_>, player_plugin::SourceNormalizerError> {
            static PACKET_DATA: &[u8] = &[1, 2, 3];
            Ok(SourceNormalizerPacketLease {
                metadata: SourceNormalizerReadPacketMetadata::packet(SourceNormalizerPacket {
                    stream_index: 0,
                    media_kind: SourceNormalizerPacketMediaKind::Video,
                    channel_layout: None,
                    ..SourceNormalizerPacket::default()
                }),
                data: PACKET_DATA,
                handle: 1,
            })
        }

        fn release_packet(
            &mut self,
            _packet_handle: usize,
        ) -> Result<(), player_plugin::SourceNormalizerError> {
            Ok(())
        }

        fn seek(
            &mut self,
            seek: &SourceNormalizerPacketSeek,
        ) -> Result<SourceNormalizerOperationStatus, player_plugin::SourceNormalizerError> {
            self.events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(FlushEvent::Seek(seek.position_millis));
            Ok(SourceNormalizerOperationStatus {
                completed: true,
                message: None,
            })
        }

        fn flush(
            &mut self,
        ) -> Result<SourceNormalizerOperationStatus, player_plugin::SourceNormalizerError> {
            self.events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(FlushEvent::Packet);
            Ok(SourceNormalizerOperationStatus {
                completed: true,
                message: None,
            })
        }

        fn close(&mut self) -> Result<(), player_plugin::SourceNormalizerError> {
            Ok(())
        }
    }

    struct TestDecoderSession {
        events: Arc<Mutex<Vec<FlushEvent>>>,
    }

    impl NativeDecoderSession for TestDecoderSession {
        fn session_info(&self) -> player_plugin::DecoderSessionInfo {
            player_plugin::DecoderSessionInfo::default()
        }

        fn send_packet(
            &mut self,
            _packet: &DecoderPacket,
            _data: &[u8],
        ) -> Result<DecoderPacketResult, DecoderError> {
            Ok(DecoderPacketResult::default())
        }

        fn receive_native_frame(
            &mut self,
        ) -> Result<DecoderReceiveNativeFrameOutput, DecoderError> {
            Ok(DecoderReceiveNativeFrameOutput::NeedMoreInput)
        }

        fn release_native_frame(&mut self, frame: DecoderNativeFrame) -> Result<(), DecoderError> {
            self.events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(FlushEvent::ReleaseDecoder(frame.handle));
            Ok(())
        }

        fn flush(&mut self) -> Result<(), DecoderError> {
            self.events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(FlushEvent::Decoder);
            Ok(())
        }

        fn close(&mut self) -> Result<(), DecoderError> {
            Ok(())
        }
    }

    struct TestProcessorSession {
        events: Arc<Mutex<Vec<FlushEvent>>>,
    }

    impl FrameProcessorSession for TestProcessorSession {
        fn session_info(&self) -> player_plugin::FrameProcessorSessionInfo {
            player_plugin::FrameProcessorSessionInfo::default()
        }

        fn submit_frame(
            &mut self,
            _frame: &NativeFrame,
            _submit: &FrameProcessorSubmitFrame,
        ) -> Result<FrameProcessorSubmitResult, FrameProcessorError> {
            Ok(FrameProcessorSubmitResult::default())
        }

        fn receive_frame(&mut self) -> Result<FrameProcessorReceiveOutput, FrameProcessorError> {
            Ok(FrameProcessorReceiveOutput::Pending)
        }

        fn release_frame(&mut self, frame: NativeFrame) -> Result<(), FrameProcessorError> {
            self.events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(FlushEvent::ReleaseProcessor(frame.handle));
            Ok(())
        }

        fn flush(&mut self) -> Result<(), FrameProcessorError> {
            self.events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(FlushEvent::Processor);
            Ok(())
        }

        fn close(&mut self) -> Result<(), FrameProcessorError> {
            Ok(())
        }
    }
}
