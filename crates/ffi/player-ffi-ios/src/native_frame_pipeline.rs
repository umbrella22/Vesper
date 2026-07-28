use std::collections::HashMap;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use player_model::MediaSource;
use player_platform_mobile::{
    HDR_PROGRAMMABLE_PROCESSING_NOT_SUPPORTED, MobileSourceNormalizerConfiguration,
    hdr_programmable_processing_not_supported_reason,
};
use player_plugin::{
    DecoderBitstreamFormat, DecoderFrameFormat, DecoderMediaKind, DecoderNativeFrame,
    DecoderNativeHandleKind, DecoderPacket, DecoderReceiveNativeFrameOutput, DecoderSessionConfig,
    FrameProcessorError, FrameProcessorFrameTimings, FrameProcessorReceiveOutput,
    FrameProcessorSession, FrameProcessorSessionConfig, FrameProcessorSessionRequirements,
    FrameProcessorSubmitFrame, FrameProcessorSubmitStatus, NativeDecoderSession, NativeFrame,
    NativeFrameHdrMetadata, NativeFrameMetadata, NativeFramePipelineProfile, NativeHandleKind,
    SourceNormalizerPacketMediaKind, SourceNormalizerPacketSeek, SourceNormalizerPacketSession,
    SourceNormalizerPacketSessionConfig, SourceNormalizerPacketSessionRequirements,
    SourceNormalizerPacketTrackInfo, SourceNormalizerReadPacketStatus,
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

/// Maximum number of pending frames the iOS native frame pipeline session tracks
/// before rejecting further stores. The host must release frames through the
/// `player_ffi_ios_native_frame_pipeline_release_frame` FFI entry point to stay
/// within this bound.
const MAX_PENDING_FRAMES: usize = 64;
const MAX_REJECTED_FRAMES_PENDING_CLEANUP: usize = 8;
const MAX_PROCESSOR_OUTPUTS_PENDING_CLEANUP: usize = 16;

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
    video_output_format: String,
    video_transfer: Option<String>,
    video_bit_depth: Option<u8>,
    hdr_kind: Option<String>,
    dolby_vision_mode: Option<String>,
    source_normalizer_plugin_name: Option<String>,
    decoder_plugin_name: String,
    processor_plugin_names: Vec<String>,
    packet_session: Box<dyn SourceNormalizerPacketSession>,
    decoder_session: Box<dyn NativeDecoderSession>,
    frame_processor_chain: Option<IosFrameProcessorChain>,
    end_of_input_sent: bool,
    end_of_stream_received: bool,
    exact_seek_target_us: Option<i64>,
    next_frame_handle: u64,
    pending_frames: HashMap<u64, IosNativeFramePipelineFrame>,
    rejected_frames_pending_cleanup: Vec<IosNativeFramePipelineFrame>,
    counters: IosNativeFramePipelineCounters,
}

pub struct IosNativeFramePipelineFrame {
    pub handle: usize,
    pub presentation_time_us: i64,
    pub duration_us: Option<i64>,
    pub width: u32,
    pub height: u32,
    pub frame_id: Option<u64>,
    decoder_frame_released: bool,
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
    pub video_output_format: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_transfer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_bit_depth: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hdr_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dolby_vision_mode: Option<String>,
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
    pub video_output_format: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_transfer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_bit_depth: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hdr_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dolby_vision_mode: Option<String>,
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
    processor_outputs_pending_cleanup: Vec<ProcessorOwnedNativeFrame>,
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
        if let Some(reason) = hdr_programmable_processing_not_supported_reason(&track) {
            let _ = packet_session.close();
            return Err(IosNativeFramePipelineOpenError::new(
                HDR_PROGRAMMABLE_PROCESSING_NOT_SUPPORTED,
                reason,
            ));
        }
        let video_stream_index = track.stream_index;
        let audio_track = selected_audio_track(&stream_info);
        let has_audio_track = audio_track.is_some();
        let audio_track_codec = audio_track.as_ref().map(|track| track.codec.clone());
        let audio_stream_index = audio_track.as_ref().map(|track| track.stream_index);
        let video_output_format = decoder_frame_format_label(&DecoderFrameFormat::Nv12);
        let video_transfer = track
            .color
            .as_ref()
            .and_then(|color| color.transfer.clone());
        let video_bit_depth = track.color.as_ref().and_then(|color| color.bit_depth);
        let hdr_kind = track.hdr.as_ref().map(|hdr| hdr.kind.clone());
        let dolby_vision_mode = track.hdr.as_ref().and_then(dolby_vision_mode);
        if !apple_native_frame_video_codec_supported(&track.codec) {
            let _ = packet_session.close();
            return Err(IosNativeFramePipelineOpenError::new(
                "unsupportedCodec",
                format!(
                    "iOS native-frame pipeline supports H264/HEVC packet streams, got {}",
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
        let decoder_session = decoder_factory
            .open_native_session(&video_decoder_session_config(&track))
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
            video_output_format,
            video_transfer,
            video_bit_depth,
            hdr_kind,
            dolby_vision_mode,
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
            exact_seek_target_us: None,
            next_frame_handle: 1,
            pending_frames: HashMap::new(),
            rejected_frames_pending_cleanup: Vec::new(),
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
            video_output_format: self.video_output_format.clone(),
            video_transfer: self.video_transfer.clone(),
            video_bit_depth: self.video_bit_depth,
            hdr_kind: self.hdr_kind.clone(),
            dolby_vision_mode: self.dolby_vision_mode.clone(),
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
            video_output_format: self.video_output_format.clone(),
            video_transfer: self.video_transfer.clone(),
            video_bit_depth: self.video_bit_depth,
            hdr_kind: self.hdr_kind.clone(),
            dolby_vision_mode: self.dolby_vision_mode.clone(),
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
        self.release_all_rejected_frames_pending_cleanup()?;
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

    pub fn store_frame(&mut self, frame: IosNativeFramePipelineFrame) -> Result<u64, String> {
        if self.pending_frames.len() >= MAX_PENDING_FRAMES {
            let cleanup_error = self.release_all_rejected_frames_pending_cleanup().err();
            if let Err(error) = self.release_rejected_frame(frame) {
                return Err(error);
            }
            if let Some(error) = cleanup_error {
                return Err(format!(
                    "native-frame pending frame limit reached; previous rejected-frame cleanup is still pending, and the current rejected frame was released: {error}",
                ));
            }
            return Err(
                "native-frame pending frame limit reached; rejected frame was released".to_owned(),
            );
        }
        let handle = self.allocate_frame_handle();
        self.pending_frames.insert(handle, frame);
        Ok(handle)
    }

    fn allocate_frame_handle(&mut self) -> u64 {
        let handle = self.next_frame_handle.max(1);
        self.next_frame_handle = self.next_frame_handle.wrapping_add(1).max(1);
        handle
    }

    pub fn pending_frame(&self, handle: u64) -> Option<&IosNativeFramePipelineFrame> {
        self.pending_frames.get(&handle)
    }

    pub fn is_end_of_stream(&self) -> bool {
        self.end_of_stream_received
    }

    pub fn release_pending_frame(&mut self, handle: u64, presented: bool) -> Result<(), String> {
        let mut frame = self
            .pending_frames
            .remove(&handle)
            .ok_or_else(|| "invalid iOS native-frame pending frame handle".to_owned())?;

        if let Err(error) = self.release_stored_frame(&mut frame, presented) {
            self.pending_frames.insert(handle, frame);
            return Err(error);
        }

        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), String> {
        self.release_all_pending_frames()?;
        self.exact_seek_target_us = None;
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
        self.release_all_pending_frames()?;
        self.exact_seek_target_us = None;
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
        self.exact_seek_target_us =
            Some(position_millis.saturating_mul(1_000).min(i64::MAX as u64) as i64);
        self.end_of_input_sent = false;
        self.end_of_stream_received = false;
        self.counters.seek_count = self.counters.seek_count.saturating_add(1);
        Ok(())
    }

    pub fn close(&mut self) {
        let _ = self.release_all_pending_frames();
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
        if let Some(target_us) = self.exact_seek_target_us {
            if frame
                .metadata
                .pts_us
                .is_none_or(|pts_us| pts_us < target_us)
            {
                self.decoder_session
                    .release_native_frame(frame)
                    .map_err(|error| {
                        format!("VideoToolbox exact-seek preroll frame release failed: {error}")
                    })?;
                self.counters.released_frames = self.counters.released_frames.saturating_add(1);
                return Ok(None);
            }
            self.exact_seek_target_us = None;
        }
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
            decoder_frame_released: false,
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

    fn release_all_pending_frames(&mut self) -> Result<(), String> {
        self.release_all_rejected_frames_pending_cleanup()?;
        let handles = self.pending_frames.keys().copied().collect::<Vec<_>>();
        for handle in handles {
            self.release_pending_frame(handle, false)?;
        }
        Ok(())
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

    fn release_rejected_frame(&mut self, frame: IosNativeFramePipelineFrame) -> Result<(), String> {
        let mut frame = frame;
        match self.release_stored_frame(&mut frame, false) {
            Ok(()) => Ok(()),
            Err(error) => {
                if self.rejected_frames_pending_cleanup.len() < MAX_REJECTED_FRAMES_PENDING_CLEANUP
                {
                    self.rejected_frames_pending_cleanup.push(frame);
                    return Err(format!(
                        "native-frame pending frame limit reached; rejected frame was retained for cleanup after release failed: {error}",
                    ));
                }
                self.close();
                Err(format!(
                    "native-frame pending frame limit reached; rejected-frame cleanup capacity ({MAX_REJECTED_FRAMES_PENDING_CLEANUP}) is saturated; session was closed after release failed: {error}",
                ))
            }
        }
    }

    fn release_all_rejected_frames_pending_cleanup(&mut self) -> Result<(), String> {
        while let Some(mut frame) = self.rejected_frames_pending_cleanup.pop() {
            if let Err(error) = self.release_stored_frame(&mut frame, false) {
                self.rejected_frames_pending_cleanup.push(frame);
                return Err(error);
            }
        }
        Ok(())
    }

    fn release_stored_frame(
        &mut self,
        frame: &mut IosNativeFramePipelineFrame,
        presented: bool,
    ) -> Result<(), String> {
        if !frame.decoder_frame_released {
            self.decoder_session
                .release_native_frame(frame.frame.decoder_frame.clone())
                .map_err(|error| format!("VideoToolbox frame release failed: {error}"))?;
            frame.decoder_frame_released = true;
        }
        if let Some(chain) = self.frame_processor_chain.as_mut() {
            chain.release_processor_outputs(&mut frame.frame.processor_outputs)?;
        }
        self.counters.released_frames = self.counters.released_frames.saturating_add(1);
        if presented {
            self.counters.presented_frames = self.counters.presented_frames.saturating_add(1);
        }
        Ok(())
    }

    fn release_pipeline_frame(&mut self, frame: IosPipelineFrame) -> Result<(), String> {
        let mut frame = IosNativeFramePipelineFrame {
            handle: frame.presentation_frame.handle,
            presentation_time_us: frame.presentation_frame.metadata.pts_us.unwrap_or(0),
            duration_us: frame.presentation_frame.metadata.duration_us,
            width: frame.presentation_frame.metadata.width,
            height: frame.presentation_frame.metadata.height,
            frame_id: frame.presentation_frame.metadata.frame_id,
            decoder_frame_released: false,
            frame,
        };
        self.release_stored_frame(&mut frame, false)
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
                    PlayerPlaybackRoute::SdkManagedNativeFrame
                        .wire_name()
                        .to_owned(),
                ),
                ("presenter", "MetalLayer".to_owned()),
                ("videoOutputFormat", self.video_output_format.clone()),
                (
                    "videoTransfer",
                    self.video_transfer
                        .clone()
                        .unwrap_or_else(|| "unknown".to_owned()),
                ),
                (
                    "videoBitDepth",
                    self.video_bit_depth
                        .map(|bit_depth| bit_depth.to_string())
                        .unwrap_or_else(|| "unknown".to_owned()),
                ),
                (
                    "hdrKind",
                    self.hdr_kind
                        .clone()
                        .unwrap_or_else(|| "sdr".to_owned()),
                ),
                (
                    "dolbyVisionMode",
                    self.dolby_vision_mode
                        .clone()
                        .unwrap_or_else(|| "none".to_owned()),
                ),
                ("clockSource", self.clock_source().to_owned()),
                ("audioDecoder", self.audio_decoder_kind().to_owned()),
                ("audioOutput", self.audio_output_kind().to_owned()),
                ("audioPipeline", self.audio_pipeline_kind().to_owned()),
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
            status: if end_of_stream {
                "endOfStream"
            } else {
                "pending"
            },
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
            .best_source_normalizer_packet_for_profile(profile)
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
    let runtime_profile = canonical_runtime_profile(configuration).unwrap_or_default();
    let requirements = SourceNormalizerPacketSessionRequirements {
        runtime_profile: runtime_profile.clone(),
        media_kind: Some(SourceNormalizerPacketMediaKind::Video),
        codec: None,
        bitstream_format: None,
        require_seek: false,
        require_flush: true,
        require_lease_cleanup: true,
    };
    let missing_capabilities = requirements.missing_capabilities(&factory.packet_capabilities());
    if !missing_capabilities.is_empty() {
        return Err(format!(
            "SourceNormalizer packet plugin `{}` does not satisfy session requirements: missing {}",
            factory.name(),
            missing_capabilities.join(", ")
        ));
    }
    let session = factory
        .open_packet_session(&SourceNormalizerPacketSessionConfig {
            runtime_profile,
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
        color_space: track
            .color
            .as_ref()
            .and_then(|color| color.primaries.clone()),
        hdr_metadata: track.hdr.as_ref().map(|hdr| hdr.kind.clone()),
        color: track.color.clone(),
        hdr: track.hdr.clone(),
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
        let requirements = FrameProcessorSessionRequirements::native_video(input_metadata.clone());
        let missing_capabilities = requirements.missing_capabilities(&capabilities);
        if !missing_capabilities.is_empty() {
            return Err(format!(
                "frame processor `{}` does not satisfy session requirements for CVPixelBuffer VideoToolbox input: missing {}",
                factory.name(),
                missing_capabilities.join(", ")
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
            processor_outputs_pending_cleanup: Vec::new(),
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
        self.release_processor_outputs_pending_cleanup()
            .map_err(|error| (error, decoder_frame.clone()))?;
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
                    .map_err(|error| frame_processor_error(self.mode, node, error))
            };
            let submit_result = match submit_result {
                Ok(submit_result) => submit_result,
                Err(error) => {
                    self.release_outputs_before_error(processor_outputs, &decoder_frame)?;
                    return Err((error, decoder_frame));
                }
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
                        self.release_outputs_before_error(processor_outputs, &decoder_frame)?;
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
                        self.release_outputs_before_error(processor_outputs, &decoder_frame)?;
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
                node.session
                    .receive_frame()
                    .map_err(|error| frame_processor_error(self.mode, node, error))
            };
            let output = match output {
                Ok(output) => output,
                Err(error) => {
                    self.release_outputs_before_error(processor_outputs, &decoder_frame)?;
                    return Err((error, decoder_frame));
                }
            };
            let FrameProcessorReceiveOutput::Frame(output) = output else {
                counters.bypassed_frames = counters.bypassed_frames.saturating_add(1);
                if self.mode == FrameProcessorMode::RequireProcessed {
                    let plugin_name = self.processors[node_index].plugin_name.clone();
                    self.release_outputs_before_error(processor_outputs, &decoder_frame)?;
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

    fn release_outputs_before_error(
        &mut self,
        mut outputs: Vec<ProcessorOwnedNativeFrame>,
        decoder_frame: &DecoderNativeFrame,
    ) -> Result<(), (String, DecoderNativeFrame)> {
        match self.release_processor_outputs(&mut outputs) {
            Ok(()) => Ok(()),
            Err(error) => {
                if let Err(retain_error) = self.retain_processor_outputs_for_cleanup(&mut outputs) {
                    return Err((
                        format!(
                            "{error}; additionally failed to retain processor outputs for cleanup: {retain_error}"
                        ),
                        decoder_frame.clone(),
                    ));
                }
                Err((error, decoder_frame.clone()))
            }
        }
    }

    fn release_processor_outputs(
        &mut self,
        outputs: &mut Vec<ProcessorOwnedNativeFrame>,
    ) -> Result<(), String> {
        while let Some(output) = outputs.pop() {
            let processor_index = output.processor_index;
            let Some(node) = self
                .processors
                .iter_mut()
                .find(|node| node.processor_index == processor_index)
            else {
                outputs.push(output);
                return Err(format!(
                    "frame processor output release failed: processor index {} is no longer available",
                    processor_index
                ));
            };
            if let Err(error) = node.session.release_frame(output.frame.clone()) {
                let plugin_name = node.plugin_name.clone();
                outputs.push(output);
                return Err(format!(
                    "frame processor `{plugin_name}` output release failed: {error}",
                ));
            }
        }
        Ok(())
    }

    fn retain_processor_outputs_for_cleanup(
        &mut self,
        outputs: &mut Vec<ProcessorOwnedNativeFrame>,
    ) -> Result<(), String> {
        if outputs.is_empty() {
            return Ok(());
        }
        let retained = self.processor_outputs_pending_cleanup.len();
        let incoming = outputs.len();
        if retained.saturating_add(incoming) > MAX_PROCESSOR_OUTPUTS_PENDING_CLEANUP {
            return Err(format!(
                "frame processor output cleanup capacity ({MAX_PROCESSOR_OUTPUTS_PENDING_CLEANUP}) would be exceeded; retained={retained} incoming={incoming}",
            ));
        }
        self.processor_outputs_pending_cleanup.append(outputs);
        Ok(())
    }

    fn release_processor_outputs_pending_cleanup(&mut self) -> Result<(), String> {
        if self.processor_outputs_pending_cleanup.is_empty() {
            return Ok(());
        }
        let mut outputs = std::mem::take(&mut self.processor_outputs_pending_cleanup);
        match self.release_processor_outputs(&mut outputs) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.processor_outputs_pending_cleanup = outputs;
                Err(error)
            }
        }
    }

    fn flush(&mut self) {
        let _ = self.release_processor_outputs_pending_cleanup();
        for node in &mut self.processors {
            let _ = node.session.flush();
        }
    }

    fn close(&mut self) {
        let _ = self.release_processor_outputs_pending_cleanup();
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

fn decoder_frame_format_label(format: &DecoderFrameFormat) -> String {
    match format {
        DecoderFrameFormat::Rgba8888 => "rgba8888".to_owned(),
        DecoderFrameFormat::Bgra8888 => "bgra8888".to_owned(),
        DecoderFrameFormat::Yuv420p => "yuv420p".to_owned(),
        DecoderFrameFormat::Nv12 => "nv12".to_owned(),
        DecoderFrameFormat::P010 => "p010".to_owned(),
        DecoderFrameFormat::F32 => "f32".to_owned(),
        DecoderFrameFormat::S16 => "s16".to_owned(),
        DecoderFrameFormat::Unknown(label) => label.clone(),
    }
}

fn dolby_vision_mode(hdr: &NativeFrameHdrMetadata) -> Option<String> {
    if !hdr.is_dolby_vision() {
        return None;
    }
    let Some(dolby_vision) = hdr.dolby_vision.as_ref() else {
        return Some("unsupported".to_owned());
    };
    if dolby_vision.has_bl && dolby_vision.compatibility_id.is_some_and(|id| id > 0) {
        Some("compatibleBaseLayer".to_owned())
    } else {
        Some("unsupported".to_owned())
    }
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
    let codec = codec.trim().to_ascii_lowercase();
    let codec = codec.strip_prefix("video/").unwrap_or(&codec);
    if codec == "hevc"
        || codec == "h265"
        || codec.starts_with("hvc1")
        || codec.starts_with("hev1")
        || codec.starts_with("dvh1")
        || codec.starts_with("dvhe")
    {
        DecoderBitstreamFormat::Hvcc
    } else {
        DecoderBitstreamFormat::Avcc
    }
}

fn video_decoder_session_config(track: &SourceNormalizerPacketTrackInfo) -> DecoderSessionConfig {
    DecoderSessionConfig {
        codec: track.codec.clone(),
        media_kind: DecoderMediaKind::Video,
        extradata: track.extradata.clone(),
        bitstream_format: Some(
            track
                .bitstream_format
                .clone()
                .unwrap_or_else(|| decoder_bitstream_format(&track.codec)),
        ),
        width: track.width,
        height: track.height,
        coded_width: track.coded_width.or(track.width),
        coded_height: track.coded_height.or(track.height),
        reorder_depth: track.reorder_depth,
        prefer_hardware: true,
        require_cpu_output: false,
        color: track.color.clone(),
        hdr: track.hdr.clone(),
        ..DecoderSessionConfig::default()
    }
}

fn apple_native_frame_video_codec_supported(codec: &str) -> bool {
    let codec = codec.trim().to_ascii_lowercase();
    let codec = codec.strip_prefix("video/").unwrap_or(&codec);
    codec == "h264"
        || codec == "avc"
        || codec.starts_with("avc1")
        || codec == "hevc"
        || codec == "h265"
        || codec.starts_with("hvc1")
        || codec.starts_with("hev1")
        || codec.starts_with("dvh1")
        || codec.starts_with("dvhe")
}

fn configured_runtime_profile(configuration: &MobileSourceNormalizerConfiguration) -> Option<&str> {
    configuration
        .runtime_profile
        .as_deref()
        .map(str::trim)
        .filter(|profile| !profile.is_empty())
}

fn canonical_runtime_profile(
    configuration: &MobileSourceNormalizerConfiguration,
) -> Option<String> {
    configured_runtime_profile(configuration).map(str::to_owned)
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
    use std::collections::VecDeque;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use player_plugin::{
        DecoderCapabilities, DecoderCodecCapability, DecoderError, DecoderPacketResult,
        FrameProcessorError, FrameProcessorOutputFrame, FrameProcessorSubmitResult,
        SourceNormalizerOperationStatus, SourceNormalizerPacket, SourceNormalizerPacketLease,
        SourceNormalizerPacketStreamInfo, SourceNormalizerPacketTrackInfo,
        SourceNormalizerReadPacketMetadata, VesperPluginKind,
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
        SubmitProcessor(usize),
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
        let handle = session
            .store_frame(test_pipeline_frame(91, Some(42_000)))
            .expect("frame should be stored");

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
                FlushEvent::ReleaseDecoder(91),
                FlushEvent::ReleaseProcessor(1_091),
                FlushEvent::Processor,
                FlushEvent::Decoder,
                FlushEvent::Packet,
                FlushEvent::Seek(42_000)
            ]
        );
    }

    #[test]
    fn exact_seek_releases_preroll_before_frame_processor() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut session = test_session(events.clone());
        session.seek_to(1_000).expect("seek should succeed");
        assert_eq!(session.exact_seek_target_us, Some(1_000_000));
        session.decoder_session = Box::new(TestDecoderSession {
            events: events.clone(),
            release_failures_remaining: Arc::new(AtomicUsize::new(0)),
            frames: VecDeque::from([
                test_decoder_frame(501, None),
                test_decoder_frame(502, Some(0)),
                test_decoder_frame(503, Some(1_000_000)),
            ]),
        });

        assert!(
            session
                .receive_frame()
                .expect("missing-PTS preroll should be handled")
                .is_none()
        );
        assert!(
            session
                .receive_frame()
                .expect("pre-target preroll should be handled")
                .is_none()
        );
        let visible = session
            .receive_frame()
            .expect("target frame should be handled")
            .expect("target frame should be visible");

        assert_eq!(visible.presentation_time_us, 1_000_000);
        assert_eq!(session.exact_seek_target_us, None);
        assert!(session.pending_frames.is_empty());
        assert_eq!(session.counters.decoded_frames, 3);
        assert_eq!(session.counters.released_frames, 2);
        assert_eq!(
            events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_slice(),
            &[
                FlushEvent::Processor,
                FlushEvent::Decoder,
                FlushEvent::Packet,
                FlushEvent::Seek(1_000),
                FlushEvent::ReleaseDecoder(501),
                FlushEvent::ReleaseDecoder(502),
                FlushEvent::SubmitProcessor(503),
            ]
        );
        session
            .release_pipeline_frame(visible.frame)
            .expect("visible frame should be released");
    }

    #[test]
    fn flush_clears_exact_seek_target() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut session = test_session(events.clone());
        session.seek_to(1_000).expect("seek should succeed");
        events
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();

        session.flush().expect("flush should succeed");

        assert_eq!(session.exact_seek_target_us, None);
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
    fn exact_seek_target_saturates_to_decoder_timestamp_range() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut session = test_session(events);

        session.seek_to(u64::MAX).expect("seek should succeed");

        assert_eq!(session.exact_seek_target_us, Some(i64::MAX));
    }

    #[test]
    fn pending_frame_release_failure_keeps_handle_for_retry() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let release_failures = Arc::new(AtomicUsize::new(1));
        let mut session =
            test_session_with_decoder_release_failures(events.clone(), release_failures);
        let handle = session
            .store_frame(test_pipeline_frame(92, Some(43_000)))
            .expect("frame should be stored");

        let error = session
            .release_pending_frame(handle, true)
            .expect_err("first release should fail");

        assert!(error.contains("VideoToolbox frame release failed"));
        assert!(
            session.pending_frame(handle).is_some(),
            "failed release must keep the frame available for retry"
        );
        assert_eq!(session.counters.released_frames, 0);
        assert_eq!(session.counters.presented_frames, 0);
        assert!(
            events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_empty(),
            "processor output must not be released before decoder release succeeds"
        );

        session
            .release_pending_frame(handle, true)
            .expect("second release should succeed");

        assert!(session.pending_frame(handle).is_none());
        assert_eq!(session.counters.released_frames, 1);
        assert_eq!(session.counters.presented_frames, 1);
        assert_eq!(
            events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_slice(),
            &[
                FlushEvent::ReleaseDecoder(92),
                FlushEvent::ReleaseProcessor(1_092)
            ]
        );
    }

    #[test]
    fn flush_release_failure_keeps_pending_frame_for_retry_and_stops_flush() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let release_failures = Arc::new(AtomicUsize::new(1));
        let mut session =
            test_session_with_decoder_release_failures(events.clone(), release_failures);
        let handle = session
            .store_frame(test_pipeline_frame(93, Some(44_000)))
            .expect("frame should be stored");

        let error = session.flush().expect_err("first flush should fail");

        assert!(error.contains("VideoToolbox frame release failed"));
        assert!(
            session.pending_frame(handle).is_some(),
            "failed bulk release must keep the frame available for retry"
        );
        assert_eq!(session.counters.released_frames, 0);
        assert!(
            events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_empty(),
            "flush must stop before processor, decoder, or packet flush after release failure"
        );

        session.flush().expect("second flush should succeed");

        assert!(session.pending_frame(handle).is_none());
        assert_eq!(session.counters.released_frames, 1);
        assert_eq!(
            events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_slice(),
            &[
                FlushEvent::ReleaseDecoder(93),
                FlushEvent::ReleaseProcessor(1_093),
                FlushEvent::Processor,
                FlushEvent::Decoder,
                FlushEvent::Packet
            ]
        );
    }

    #[test]
    fn seek_release_failure_keeps_pending_frame_for_retry_and_stops_seek() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let release_failures = Arc::new(AtomicUsize::new(1));
        let mut session =
            test_session_with_decoder_release_failures(events.clone(), release_failures);
        let handle = session
            .store_frame(test_pipeline_frame(94, Some(45_000)))
            .expect("frame should be stored");

        let error = session.seek_to(45_000).expect_err("first seek should fail");

        assert!(error.contains("VideoToolbox frame release failed"));
        assert!(
            session.pending_frame(handle).is_some(),
            "failed bulk release must keep the frame available for retry"
        );
        assert_eq!(session.counters.released_frames, 0);
        assert_eq!(session.counters.seek_count, 0);
        assert!(
            events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_empty(),
            "seek must stop before processor, decoder, packet flush, or packet seek after release failure"
        );

        session.seek_to(45_000).expect("second seek should succeed");

        assert!(session.pending_frame(handle).is_none());
        assert_eq!(session.counters.released_frames, 1);
        assert_eq!(session.counters.seek_count, 1);
        assert_eq!(
            events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_slice(),
            &[
                FlushEvent::ReleaseDecoder(94),
                FlushEvent::ReleaseProcessor(1_094),
                FlushEvent::Processor,
                FlushEvent::Decoder,
                FlushEvent::Packet,
                FlushEvent::Seek(45_000)
            ]
        );
    }

    #[test]
    fn frame_processor_chain_releases_prior_outputs_when_later_submit_fails() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut chain = test_two_node_processor_chain(
            Box::new(TestReadyProcessorSession {
                events: events.clone(),
                output_handle: 2_001,
            }),
            Box::new(TestFailingSubmitProcessorSession),
        );
        let mut counters = IosNativeFramePipelineCounters::default();
        let decoder_frame = test_decoder_frame(101, Some(101_000));

        let error = chain
            .process(&mut counters, decoder_frame)
            .expect_err("later submit failure should fail the chain")
            .0;

        assert!(error.contains("submit failed"));
        assert_eq!(
            events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_slice(),
            &[FlushEvent::ReleaseProcessor(2_001)]
        );
    }

    #[test]
    fn frame_processor_chain_retains_prior_outputs_when_cleanup_release_fails() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let release_failures = Arc::new(AtomicUsize::new(1));
        let mut chain = test_two_node_processor_chain(
            Box::new(TestReadyFailingReleaseProcessorSession {
                events: events.clone(),
                output_handle: 2_003,
                release_failures_remaining: release_failures,
            }),
            Box::new(TestFailingSubmitProcessorSession),
        );
        let mut counters = IosNativeFramePipelineCounters::default();
        let decoder_frame = test_decoder_frame(103, Some(103_000));

        let error = chain
            .process(&mut counters, decoder_frame)
            .expect_err("cleanup release failure should fail the chain")
            .0;

        assert!(error.contains("output release failed"));
        assert_eq!(chain.processor_outputs_pending_cleanup.len(), 1);
        chain
            .release_processor_outputs_pending_cleanup()
            .expect("retained processor output should be retryable");
        assert_eq!(chain.processor_outputs_pending_cleanup.len(), 0);
        let events = events.lock().unwrap_or_else(|error| error.into_inner());
        assert_eq!(
            events
                .iter()
                .filter(|event| **event == FlushEvent::ReleaseProcessor(2_003))
                .count(),
            2
        );
    }

    #[test]
    fn frame_processor_chain_releases_prior_outputs_when_later_receive_fails() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut chain = test_two_node_processor_chain(
            Box::new(TestReadyProcessorSession {
                events: events.clone(),
                output_handle: 2_002,
            }),
            Box::new(TestFailingReceiveProcessorSession),
        );
        let mut counters = IosNativeFramePipelineCounters::default();
        let decoder_frame = test_decoder_frame(102, Some(102_000));

        let error = chain
            .process(&mut counters, decoder_frame)
            .expect_err("later receive failure should fail the chain")
            .0;

        assert!(error.contains("receive failed"));
        assert_eq!(
            events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_slice(),
            &[FlushEvent::ReleaseProcessor(2_002)]
        );
    }

    #[test]
    fn store_frame_releases_rejected_frame_when_pending_limit_is_full() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut session = test_session(events.clone());
        for index in 0..MAX_PENDING_FRAMES {
            let handle = session
                .store_frame(test_pipeline_frame(index, Some(index as i64 * 1_000)))
                .expect("frame should be stored while under the cap");
            assert!(session.pending_frame(handle).is_some());
        }

        let result = session.store_frame(test_pipeline_frame(900, Some(900_000)));

        assert!(result.is_err());
        assert_eq!(session.pending_frames.len(), MAX_PENDING_FRAMES);
        assert_eq!(session.counters.released_frames, 1);
        assert_eq!(
            events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_slice(),
            &[
                FlushEvent::ReleaseDecoder(900),
                FlushEvent::ReleaseProcessor(1_900)
            ]
        );
    }

    #[test]
    fn store_frame_retains_rejected_frame_for_cleanup_when_release_fails() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let release_failures = Arc::new(AtomicUsize::new(1));
        let mut session =
            test_session_with_decoder_release_failures(events.clone(), release_failures);
        for index in 0..MAX_PENDING_FRAMES {
            let handle = session
                .store_frame(test_pipeline_frame(index, Some(index as i64 * 1_000)))
                .expect("frame should be stored while under the cap");
            assert!(session.pending_frame(handle).is_some());
        }

        let result = session.store_frame(test_pipeline_frame(901, Some(901_000)));

        let error = result.expect_err("over-cap rejected frame release should fail");
        assert!(error.contains("retained for cleanup"));
        assert_eq!(session.pending_frames.len(), MAX_PENDING_FRAMES);
        assert_eq!(session.rejected_frames_pending_cleanup.len(), 1);
        assert_eq!(session.counters.released_frames, 0);
        assert!(
            events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_empty(),
            "processor output must not be released before decoder release succeeds"
        );

        session
            .release_all_pending_frames()
            .expect("retained frames should be released on retry");

        assert_eq!(session.pending_frames.len(), 0);
        assert_eq!(session.rejected_frames_pending_cleanup.len(), 0);
        assert_eq!(
            session.counters.released_frames,
            (MAX_PENDING_FRAMES + 1) as u64
        );
        assert!(
            events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .contains(&FlushEvent::ReleaseDecoder(901))
        );
        assert!(
            events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .contains(&FlushEvent::ReleaseProcessor(1_901))
        );
    }

    #[test]
    fn store_frame_does_not_grow_pending_frames_when_prior_cleanup_is_pending() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let release_failures = Arc::new(AtomicUsize::new(2));
        let mut session =
            test_session_with_decoder_release_failures(events.clone(), release_failures);
        for index in 0..MAX_PENDING_FRAMES {
            let handle = session
                .store_frame(test_pipeline_frame(index, Some(index as i64 * 1_000)))
                .expect("frame should be stored while under the cap");
            assert!(session.pending_frame(handle).is_some());
        }

        let first_error = session
            .store_frame(test_pipeline_frame(902, Some(902_000)))
            .expect_err("first over-cap rejected frame release should fail");
        let second_error = session
            .store_frame(test_pipeline_frame(903, Some(903_000)))
            .expect_err("second over-cap rejected frame release should fail");

        assert!(first_error.contains("retained for cleanup"));
        assert!(second_error.contains("rejected frame was released"));
        assert_eq!(session.pending_frames.len(), MAX_PENDING_FRAMES);
        assert_eq!(session.rejected_frames_pending_cleanup.len(), 1);
        assert_eq!(session.counters.released_frames, 1);
        assert!(
            events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .contains(&FlushEvent::ReleaseProcessor(1_903)),
            "second rejected frame should still be released instead of growing pending storage"
        );

        session
            .release_all_pending_frames()
            .expect("retained frames should be released on retry");

        assert_eq!(session.pending_frames.len(), 0);
        assert_eq!(session.rejected_frames_pending_cleanup.len(), 0);
        assert_eq!(
            session.counters.released_frames,
            (MAX_PENDING_FRAMES + 2) as u64
        );
        let events = events.lock().unwrap_or_else(|error| error.into_inner());
        assert!(events.contains(&FlushEvent::ReleaseDecoder(902)));
        assert!(events.contains(&FlushEvent::ReleaseProcessor(1_902)));
        assert!(events.contains(&FlushEvent::ReleaseDecoder(903)));
        assert!(events.contains(&FlushEvent::ReleaseProcessor(1_903)));
    }

    #[test]
    fn pending_frame_processor_release_failure_remains_retryable_without_duplicate_decoder_release()
    {
        let events = Arc::new(Mutex::new(Vec::new()));
        let processor_release_failures = Arc::new(AtomicUsize::new(1));
        let mut session = test_session_with_processor_release_failures(
            events.clone(),
            processor_release_failures,
        );
        let handle = session
            .store_frame(test_pipeline_frame(904, Some(904_000)))
            .expect("frame should be stored");

        let error = session
            .release_pending_frame(handle, false)
            .expect_err("processor release failure should keep pending frame retryable");

        assert!(error.contains("processor"));
        assert!(session.pending_frame(handle).is_some());
        assert_eq!(session.counters.released_frames, 0);

        session
            .release_pending_frame(handle, false)
            .expect("retry should release retained processor output");

        assert!(session.pending_frame(handle).is_none());
        assert_eq!(session.counters.released_frames, 1);
        let events = events.lock().unwrap_or_else(|error| error.into_inner());
        assert_eq!(
            events
                .iter()
                .filter(|event| **event == FlushEvent::ReleaseDecoder(904))
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| **event == FlushEvent::ReleaseProcessor(1_904))
                .count(),
            2
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
    fn open_and_status_wire_report_sdr_video_summary() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut session = test_session(events);
        session.video_output_format = "nv12".to_owned();
        session.video_transfer = Some("bt709".to_owned());
        session.video_bit_depth = Some(8);

        let open = session.open_wire(7);
        let status = session.status_wire(7, None);

        assert_eq!(open.video_output_format, "nv12");
        assert_eq!(open.video_transfer.as_deref(), Some("bt709"));
        assert_eq!(open.video_bit_depth, Some(8));
        assert_eq!(open.hdr_kind.as_deref(), None);
        assert_eq!(open.dolby_vision_mode.as_deref(), None);
        assert_eq!(status.video_output_format, "nv12");
        assert_eq!(status.video_transfer.as_deref(), Some("bt709"));
        assert_eq!(status.video_bit_depth, Some(8));
        assert_eq!(status.hdr_kind.as_deref(), None);
        assert_eq!(status.dolby_vision_mode.as_deref(), None);
        assert!(open.diagnostics.iter().any(|diagnostic| {
            diagnostic.plugin_kind.as_deref() == Some("native_frame_pipeline")
                && diagnostic
                    .details
                    .details
                    .iter()
                    .any(|(key, value)| key == "videoOutputFormat" && value == "nv12")
                && diagnostic
                    .details
                    .details
                    .iter()
                    .any(|(key, value)| key == "hdrKind" && value == "sdr")
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
                    supports_pcm_frames: false,
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
                    supports_pcm_frames: true,
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

    #[test]
    fn apple_native_frame_video_codec_gate_accepts_hevc_aliases() {
        for codec in [
            "h264", "avc1", "HEVC", "h265", "hvc1", "hev1", "dvh1", "dvhe",
        ] {
            assert!(
                apple_native_frame_video_codec_supported(codec),
                "{codec} should be accepted by the Apple native-frame codec gate"
            );
        }
        assert!(!apple_native_frame_video_codec_supported("vp9"));
        assert_eq!(
            decoder_bitstream_format("hvc1"),
            DecoderBitstreamFormat::Hvcc
        );
        assert_eq!(
            decoder_bitstream_format("HEVC"),
            DecoderBitstreamFormat::Hvcc
        );
        assert_eq!(
            decoder_bitstream_format("dvh1"),
            DecoderBitstreamFormat::Hvcc
        );
        assert_eq!(
            decoder_bitstream_format("avc1"),
            DecoderBitstreamFormat::Avcc
        );
    }

    #[test]
    fn video_decoder_session_config_preserves_reorder_depth() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut track = TestPacketSession { events }
            .stream_info()
            .tracks
            .into_iter()
            .next()
            .expect("test packet session should expose a video track");
        track.reorder_depth = Some(4);

        let config = video_decoder_session_config(&track);

        assert_eq!(config.reorder_depth, Some(4));
    }

    #[test]
    fn hdr_track_reports_programmable_processing_rejection() {
        let mut track = SourceNormalizerPacketTrackInfo {
            stream_index: 0,
            media_kind: SourceNormalizerPacketMediaKind::Video,
            codec: "hvc1".to_owned(),
            extradata: Vec::new(),
            bitstream_format: Some(DecoderBitstreamFormat::Hvcc),
            width: Some(1_920),
            height: Some(1_080),
            coded_width: Some(1_920),
            coded_height: Some(1_080),
            reorder_depth: None,
            sample_rate: None,
            channels: None,
            channel_layout: None,
            codec_delay_samples: None,
            priming_samples: None,
            trailing_padding_samples: None,
            seek_preroll_samples: None,
            color: None,
            hdr: None,
            frame_rate: None,
            time_base_num: None,
            time_base_den: None,
        };
        track.hdr = Some(NativeFrameHdrMetadata {
            kind: "hdr10".to_owned(),
            mastering_display: None,
            content_light: None,
            dolby_vision: None,
        });

        let reason = hdr_programmable_processing_not_supported_reason(&track)
            .expect("HDR track should be rejected for native-frame processing");

        assert!(reason.contains(HDR_PROGRAMMABLE_PROCESSING_NOT_SUPPORTED));
    }

    fn test_session(events: Arc<Mutex<Vec<FlushEvent>>>) -> IosNativeFramePipelineSession {
        test_session_with_decoder_release_failures(events, Arc::new(AtomicUsize::new(0)))
    }

    fn test_session_with_decoder_release_failures(
        events: Arc<Mutex<Vec<FlushEvent>>>,
        release_failures_remaining: Arc<AtomicUsize>,
    ) -> IosNativeFramePipelineSession {
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
            video_output_format: "nv12".to_owned(),
            video_transfer: None,
            video_bit_depth: Some(8),
            hdr_kind: None,
            dolby_vision_mode: None,
            source_normalizer_plugin_name: Some("test-source-normalizer".to_owned()),
            decoder_plugin_name: "test-decoder".to_owned(),
            processor_plugin_names: vec!["test-processor".to_owned()],
            packet_session: Box::new(TestPacketSession {
                events: events.clone(),
            }),
            decoder_session: Box::new(TestDecoderSession {
                events: events.clone(),
                release_failures_remaining,
                frames: VecDeque::new(),
            }),
            frame_processor_chain: Some(IosFrameProcessorChain {
                processors: vec![IosFrameProcessorNode {
                    plugin_name: "test-processor".to_owned(),
                    processor_index: 0,
                    session: Box::new(TestProcessorSession { events }),
                }],
                mode: FrameProcessorMode::PreferProcessed,
                policy: FrameProcessorPolicy::default(),
                processor_outputs_pending_cleanup: Vec::new(),
            }),
            end_of_input_sent: true,
            end_of_stream_received: true,
            exact_seek_target_us: None,
            next_frame_handle: 1,
            pending_frames: HashMap::new(),
            rejected_frames_pending_cleanup: Vec::new(),
            counters: IosNativeFramePipelineCounters::default(),
        }
    }

    fn test_session_with_processor_release_failures(
        events: Arc<Mutex<Vec<FlushEvent>>>,
        processor_release_failures_remaining: Arc<AtomicUsize>,
    ) -> IosNativeFramePipelineSession {
        let mut session = test_session(events.clone());
        session.frame_processor_chain = Some(IosFrameProcessorChain {
            processors: vec![IosFrameProcessorNode {
                plugin_name: "test-processor".to_owned(),
                processor_index: 0,
                session: Box::new(TestFailingReleaseProcessorSession {
                    events,
                    release_failures_remaining: processor_release_failures_remaining,
                }),
            }],
            mode: FrameProcessorMode::PreferProcessed,
            policy: FrameProcessorPolicy::default(),
            processor_outputs_pending_cleanup: Vec::new(),
        });
        session
    }

    fn test_two_node_processor_chain(
        first: Box<dyn FrameProcessorSession>,
        second: Box<dyn FrameProcessorSession>,
    ) -> IosFrameProcessorChain {
        IosFrameProcessorChain {
            processors: vec![
                IosFrameProcessorNode {
                    plugin_name: "test-processor-0".to_owned(),
                    processor_index: 0,
                    session: first,
                },
                IosFrameProcessorNode {
                    plugin_name: "test-processor-1".to_owned(),
                    processor_index: 1,
                    session: second,
                },
            ],
            mode: FrameProcessorMode::PreferProcessed,
            policy: FrameProcessorPolicy::default(),
            processor_outputs_pending_cleanup: Vec::new(),
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
            decoder_frame_released: false,
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
                color: None,
                hdr: None,
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
                color: None,
                hdr: None,
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
                        reorder_depth: None,
                        sample_rate: None,
                        channels: None,
                        channel_layout: None,
                        codec_delay_samples: None,
                        priming_samples: None,
                        trailing_padding_samples: None,
                        seek_preroll_samples: None,
                        color: None,
                        hdr: None,
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
                        reorder_depth: None,
                        sample_rate: Some(48_000),
                        channels: Some(2),
                        channel_layout: Some("stereo".to_owned()),
                        codec_delay_samples: None,
                        priming_samples: None,
                        trailing_padding_samples: None,
                        seek_preroll_samples: Some(1_024),
                        color: None,
                        hdr: None,
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
        release_failures_remaining: Arc<AtomicUsize>,
        frames: VecDeque<DecoderNativeFrame>,
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
            Ok(self
                .frames
                .pop_front()
                .map_or(DecoderReceiveNativeFrameOutput::NeedMoreInput, |frame| {
                    DecoderReceiveNativeFrameOutput::Frame(frame)
                }))
        }

        fn release_native_frame(&mut self, frame: DecoderNativeFrame) -> Result<(), DecoderError> {
            if self
                .release_failures_remaining
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    if remaining > 0 {
                        Some(remaining - 1)
                    } else {
                        None
                    }
                })
                .is_ok()
            {
                return Err(DecoderError::internal("release failed"));
            }
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

    struct TestFailingReleaseProcessorSession {
        events: Arc<Mutex<Vec<FlushEvent>>>,
        release_failures_remaining: Arc<AtomicUsize>,
    }

    struct TestReadyProcessorSession {
        events: Arc<Mutex<Vec<FlushEvent>>>,
        output_handle: usize,
    }

    struct TestReadyFailingReleaseProcessorSession {
        events: Arc<Mutex<Vec<FlushEvent>>>,
        output_handle: usize,
        release_failures_remaining: Arc<AtomicUsize>,
    }

    struct TestFailingSubmitProcessorSession;

    struct TestFailingReceiveProcessorSession;

    impl FrameProcessorSession for TestProcessorSession {
        fn session_info(&self) -> player_plugin::FrameProcessorSessionInfo {
            player_plugin::FrameProcessorSessionInfo::default()
        }

        fn submit_frame(
            &mut self,
            frame: &NativeFrame,
            _submit: &FrameProcessorSubmitFrame,
        ) -> Result<FrameProcessorSubmitResult, FrameProcessorError> {
            self.events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(FlushEvent::SubmitProcessor(frame.handle));
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

    impl FrameProcessorSession for TestFailingReleaseProcessorSession {
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
            let remaining = self.release_failures_remaining.load(Ordering::SeqCst);
            if remaining > 0 {
                self.release_failures_remaining
                    .fetch_sub(1, Ordering::SeqCst);
                return Err(FrameProcessorError::internal(
                    "processor release failed in test",
                ));
            }
            Ok(())
        }

        fn flush(&mut self) -> Result<(), FrameProcessorError> {
            Ok(())
        }

        fn close(&mut self) -> Result<(), FrameProcessorError> {
            Ok(())
        }
    }

    impl FrameProcessorSession for TestReadyProcessorSession {
        fn session_info(&self) -> player_plugin::FrameProcessorSessionInfo {
            player_plugin::FrameProcessorSessionInfo::default()
        }

        fn submit_frame(
            &mut self,
            _frame: &NativeFrame,
            _submit: &FrameProcessorSubmitFrame,
        ) -> Result<FrameProcessorSubmitResult, FrameProcessorError> {
            Ok(FrameProcessorSubmitResult {
                status: FrameProcessorSubmitStatus::Accepted,
                ..FrameProcessorSubmitResult::default()
            })
        }

        fn receive_frame(&mut self) -> Result<FrameProcessorReceiveOutput, FrameProcessorError> {
            Ok(FrameProcessorReceiveOutput::Frame(
                FrameProcessorOutputFrame {
                    frame: test_processor_frame(
                        self.output_handle,
                        Some(self.output_handle as i64),
                    ),
                    timings: FrameProcessorFrameTimings::default(),
                    source_frame_id: Some(self.output_handle as u64),
                },
            ))
        }

        fn release_frame(&mut self, frame: NativeFrame) -> Result<(), FrameProcessorError> {
            self.events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(FlushEvent::ReleaseProcessor(frame.handle));
            Ok(())
        }

        fn flush(&mut self) -> Result<(), FrameProcessorError> {
            Ok(())
        }

        fn close(&mut self) -> Result<(), FrameProcessorError> {
            Ok(())
        }
    }

    impl FrameProcessorSession for TestReadyFailingReleaseProcessorSession {
        fn session_info(&self) -> player_plugin::FrameProcessorSessionInfo {
            player_plugin::FrameProcessorSessionInfo::default()
        }

        fn submit_frame(
            &mut self,
            _frame: &NativeFrame,
            _submit: &FrameProcessorSubmitFrame,
        ) -> Result<FrameProcessorSubmitResult, FrameProcessorError> {
            Ok(FrameProcessorSubmitResult {
                status: FrameProcessorSubmitStatus::Accepted,
                ..FrameProcessorSubmitResult::default()
            })
        }

        fn receive_frame(&mut self) -> Result<FrameProcessorReceiveOutput, FrameProcessorError> {
            Ok(FrameProcessorReceiveOutput::Frame(
                FrameProcessorOutputFrame {
                    frame: test_processor_frame(
                        self.output_handle,
                        Some(self.output_handle as i64),
                    ),
                    timings: FrameProcessorFrameTimings::default(),
                    source_frame_id: Some(self.output_handle as u64),
                },
            ))
        }

        fn release_frame(&mut self, frame: NativeFrame) -> Result<(), FrameProcessorError> {
            self.events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(FlushEvent::ReleaseProcessor(frame.handle));
            let remaining = self.release_failures_remaining.load(Ordering::SeqCst);
            if remaining > 0 {
                self.release_failures_remaining
                    .fetch_sub(1, Ordering::SeqCst);
                return Err(FrameProcessorError::internal(
                    "processor release failed in test",
                ));
            }
            Ok(())
        }

        fn flush(&mut self) -> Result<(), FrameProcessorError> {
            Ok(())
        }

        fn close(&mut self) -> Result<(), FrameProcessorError> {
            Ok(())
        }
    }

    impl FrameProcessorSession for TestFailingSubmitProcessorSession {
        fn session_info(&self) -> player_plugin::FrameProcessorSessionInfo {
            player_plugin::FrameProcessorSessionInfo::default()
        }

        fn submit_frame(
            &mut self,
            _frame: &NativeFrame,
            _submit: &FrameProcessorSubmitFrame,
        ) -> Result<FrameProcessorSubmitResult, FrameProcessorError> {
            Err(FrameProcessorError::internal("submit failed"))
        }

        fn receive_frame(&mut self) -> Result<FrameProcessorReceiveOutput, FrameProcessorError> {
            Ok(FrameProcessorReceiveOutput::Pending)
        }

        fn release_frame(&mut self, _frame: NativeFrame) -> Result<(), FrameProcessorError> {
            Ok(())
        }

        fn flush(&mut self) -> Result<(), FrameProcessorError> {
            Ok(())
        }

        fn close(&mut self) -> Result<(), FrameProcessorError> {
            Ok(())
        }
    }

    impl FrameProcessorSession for TestFailingReceiveProcessorSession {
        fn session_info(&self) -> player_plugin::FrameProcessorSessionInfo {
            player_plugin::FrameProcessorSessionInfo::default()
        }

        fn submit_frame(
            &mut self,
            _frame: &NativeFrame,
            _submit: &FrameProcessorSubmitFrame,
        ) -> Result<FrameProcessorSubmitResult, FrameProcessorError> {
            Ok(FrameProcessorSubmitResult {
                status: FrameProcessorSubmitStatus::Accepted,
                ..FrameProcessorSubmitResult::default()
            })
        }

        fn receive_frame(&mut self) -> Result<FrameProcessorReceiveOutput, FrameProcessorError> {
            Err(FrameProcessorError::internal("receive failed"))
        }

        fn release_frame(&mut self, _frame: NativeFrame) -> Result<(), FrameProcessorError> {
            Ok(())
        }

        fn flush(&mut self) -> Result<(), FrameProcessorError> {
            Ok(())
        }

        fn close(&mut self) -> Result<(), FrameProcessorError> {
            Ok(())
        }
    }
}
