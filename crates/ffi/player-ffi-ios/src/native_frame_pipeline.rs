use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use player_model::MediaSource;
use player_platform_mobile::{
    HDR_PROGRAMMABLE_PROCESSING_NOT_SUPPORTED, MobileNativePluginArtifact,
    MobileSourceNormalizerConfiguration, MobileSourceNormalizerPacketOpen,
    hdr_programmable_processing_not_supported_reason, open_mobile_source_normalizer_packet_session,
};
use player_platform_native_frame::{
    NativeMediaPipeline, NativeMediaVideoPoll, NormalizedVideoPackets,
};
use player_plugin::{
    NativeDecoderPluginFactory, NativeFrameHdrMetadata, SourceNormalizerPacketMediaKind,
    SourceNormalizerPacketTrackInfo,
};
use player_plugin_abi::VESPER_HANDLE_CV_PIXEL_BUFFER;
use player_plugin_loader::native::{NativePlugin, NativeVideoDecoder, NativeVideoFrame};
use player_plugin_loader::{
    DecoderPluginMatchRequest, NativePluginArtifact, PluginDiagnosticRecord, PluginRegistry,
};
use player_runtime::{
    NativeFramePipelineMode, PlayerPlaybackRoute, PlayerPluginDiagnostic,
    PlayerPluginParticipation, SourceNormalizerMode,
};
use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};

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
    pub source_normalizer_plugin_artifacts: Vec<MobileNativePluginArtifact>,
    pub source_normalizer_plugin_library_paths: Vec<PathBuf>,
    pub runtime_profile: Option<String>,
    pub native_frame_pipeline_mode: NativeFramePipelineMode,
    pub decoder_plugin_artifacts: Vec<MobileNativePluginArtifact>,
    pub decoder_plugin_library_paths: Vec<PathBuf>,
    pub frame_processor_plugin_artifacts: Vec<MobileNativePluginArtifact>,
    pub frame_processor_plugin_library_paths: Vec<PathBuf>,
    pub native_plugin_loading_policy: player_runtime::NativePluginLoadingPolicy,
}

/// Maximum number of pending frames the iOS native frame pipeline session tracks
/// before rejecting further stores. The host must release frames through the
/// `player_ffi_ios_native_frame_pipeline_release_frame` FFI entry point to stay
/// within this bound.
const MAX_PENDING_FRAMES: usize = 64;

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
    video: Option<NativeMediaPipeline<NormalizedVideoPackets>>,
    end_of_stream_received: bool,
    exact_seek_target_us: Option<i64>,
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
    lease: Option<NativeVideoFrame>,
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

fn validate_ios_native_plugin_bindings(
    config: &IosNativeFramePipelineOpenConfig,
) -> Result<(), IosNativeFramePipelineOpenError> {
    for (artifacts, paths, capability) in [
        (
            config.source_normalizer_plugin_artifacts.as_slice(),
            config.source_normalizer_plugin_library_paths.as_slice(),
            "iOS SourceNormalizer",
        ),
        (
            config.decoder_plugin_artifacts.as_slice(),
            config.decoder_plugin_library_paths.as_slice(),
            "iOS NativeDecoder",
        ),
    ] {
        if !artifacts.is_empty() && !paths.is_empty() {
            return Err(IosNativeFramePipelineOpenError::new(
                "startupFailure",
                format!(
                    "{capability} configuration cannot mix referenced artifacts with unbound library paths"
                ),
            ));
        }
        if !paths.is_empty() {
            config
                .native_plugin_loading_policy
                .validate_development_raw_paths(capability)
                .map_err(|error| {
                    IosNativeFramePipelineOpenError::new("startupFailure", error.to_string())
                })?;
        }
    }
    Ok(())
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
        if !config.frame_processor_plugin_artifacts.is_empty()
            || !config.frame_processor_plugin_library_paths.is_empty()
        {
            return Err(IosNativeFramePipelineOpenError::new(
                "startupFailure",
                "FrameProcessor has been retired from the iOS native-frame pipeline",
            ));
        }
        validate_ios_native_plugin_bindings(&config)?;
        if config.source_normalizer_plugin_artifacts.is_empty()
            && config.source_normalizer_plugin_library_paths.is_empty()
        {
            return Err(IosNativeFramePipelineOpenError::new(
                "missingSourceNormalizerPacketPlugin",
                "iOS native-frame pipeline requires a SourceNormalizer packet-stream plugin path",
            ));
        }
        if config.decoder_plugin_artifacts.is_empty()
            && config.decoder_plugin_library_paths.is_empty()
        {
            return Err(IosNativeFramePipelineOpenError::new(
                "missingVideoToolboxDecoderPlugin",
                "iOS native-frame pipeline requires a VideoToolbox decoder plugin path",
            ));
        }

        let source = MediaSource::new(config.source_uri.clone());
        let source_normalizer_configuration = MobileSourceNormalizerConfiguration {
            mode: config.source_normalizer_mode,
            plugin_artifacts: config.source_normalizer_plugin_artifacts.clone(),
            plugin_library_paths: config.source_normalizer_plugin_library_paths.clone(),
            native_plugin_loading_policy: config.native_plugin_loading_policy,
            runtime_profile: config.runtime_profile.clone(),
        };
        let packet_open = open_packet_source_normalizer(&source, &source_normalizer_configuration)
            .map_err(|error| {
                IosNativeFramePipelineOpenError::new("missingSourceNormalizerPacketPlugin", error)
            })?;
        let stream_info = packet_open.info;
        let source_normalizer_plugin_name = packet_open.plugin_name;
        let packets = packet_open.session;
        let track = selected_video_track(&stream_info)
            .map_err(|error| IosNativeFramePipelineOpenError::new("unsupportedCodec", error))?;
        if let Some(reason) = hdr_programmable_processing_not_supported_reason(&track) {
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
        let video_output_format = "nv12".to_owned();
        let video_transfer = track
            .color
            .as_ref()
            .and_then(|color| color.transfer.clone());
        let video_bit_depth = track.color.as_ref().and_then(|color| color.bit_depth);
        let hdr_kind = track.hdr.as_ref().map(|hdr| hdr.kind.clone());
        let dolby_vision_mode = track.hdr.as_ref().and_then(dolby_vision_mode);
        if !apple_native_frame_video_codec_supported(&track.codec) {
            return Err(IosNativeFramePipelineOpenError::new(
                "unsupportedCodec",
                format!(
                    "iOS native-frame pipeline supports H264/HEVC packet streams, got {}",
                    track.codec
                ),
            ));
        }

        let video_request = DecoderPluginMatchRequest::video(track.codec.clone());
        let (decoder_factory, decoder_artifact, audio_decoder_plugin_name) =
            if config.decoder_plugin_artifacts.is_empty() {
                let decoder_registry = PluginRegistry::inspect_decoder_support_development(
                    &config.decoder_plugin_library_paths,
                    video_request.clone(),
                );
                let audio_decoder_plugin_name = audio_track.as_ref().and_then(|track| {
                    let audio_registry = PluginRegistry::inspect_decoder_support_development(
                        &config.decoder_plugin_library_paths,
                        DecoderPluginMatchRequest::audio(track.codec.clone()),
                    );
                    audio_pcm_decoder_plugin_name(&audio_registry, &track.codec)
                });
                let decoder_record = decoder_registry
                    .best_native_decoder_for(&video_request)
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
                let decoder_reference = decoder_registry
                    .reference_for_record(decoder_record)
                    .ok_or_else(|| {
                        IosNativeFramePipelineOpenError::new(
                            "missingVideoToolboxDecoderPlugin",
                            "selected decoder has no plugin capability reference",
                        )
                    })?;
                let decoder_factory = decoder_registry
                    .resolve_native_decoder(decoder_reference)
                    .map_err(|error| {
                        IosNativeFramePipelineOpenError::new(
                            "missingVideoToolboxDecoderPlugin",
                            format!("failed to resolve decoder plugin: {error}"),
                        )
                    })?
                    .capability();
                let artifact = decoder_registry
                    .native_artifact(decoder_reference)
                    .map_err(|error| {
                        IosNativeFramePipelineOpenError::new(
                            "missingVideoToolboxDecoderPlugin",
                            error.to_string(),
                        )
                    })?;
                (decoder_factory, artifact, audio_decoder_plugin_name)
            } else {
                let (decoder_factory, artifact) = resolve_selected_ios_decoder(
                    &config.decoder_plugin_artifacts,
                    &video_request,
                    false,
                )?
                .ok_or_else(|| {
                    IosNativeFramePipelineOpenError::new(
                        "missingVideoToolboxDecoderPlugin",
                        format!(
                            "no explicitly selected native-frame decoder supports {} video",
                            track.codec
                        ),
                    )
                })?;
                let audio_decoder_plugin_name = match audio_track.as_ref() {
                    Some(track) => resolve_selected_ios_decoder(
                        &config.decoder_plugin_artifacts,
                        &DecoderPluginMatchRequest::audio(track.codec.clone()),
                        true,
                    )?
                    .map(|(factory, _)| factory.name().to_owned()),
                    None => None,
                };
                (decoder_factory, artifact, audio_decoder_plugin_name)
            };
        let audio_decoder_plugin_ready = audio_decoder_plugin_name.is_some();
        let decoder_plugin_name = decoder_factory.name().to_owned();
        let plugin = decoder_artifact.native_plugin().ok_or_else(|| {
            IosNativeFramePipelineOpenError::new(
                "missingVideoToolboxDecoderPlugin",
                "iOS native-frame playback requires a native VideoToolbox decoder plugin",
            )
        })?;
        let decoder = open_native_video_decoder(&plugin, &track)?;
        let video = NativeMediaPipeline::new(
            NormalizedVideoPackets::new(packets, video_stream_index),
            decoder,
        );

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
            source_normalizer_plugin_name: source_normalizer_plugin_name
                .or_else(|| stream_info.normalizer_name.clone()),
            decoder_plugin_name,
            video: Some(video),
            end_of_stream_received: false,
            exact_seek_target_us: None,
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
            processor_chain: Vec::new(),
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
        if self.end_of_stream_received {
            return Ok(None);
        }
        let pipeline = self
            .video
            .as_mut()
            .ok_or_else(|| "iOS video route is closed".to_owned())?;
        match pipeline.advance().map_err(|error| error.to_string())? {
            NativeMediaVideoPoll::Frame { lease, format } => self.accept_frame(lease, format),
            NativeMediaVideoPoll::Pending => Ok(None),
            NativeMediaVideoPoll::EndOfStream => {
                self.end_of_stream_received = true;
                Ok(None)
            }
        }
    }

    fn accept_frame(
        &mut self,
        lease: NativeVideoFrame,
        format: player_plugin_abi::VesperVideoFormat,
    ) -> Result<Option<IosNativeFramePipelineFrame>, String> {
        let frame = lease.descriptor();
        self.counters.decoded_frames = self.counters.decoded_frames.saturating_add(1);
        if let Some(target) = self.exact_seek_target_us {
            if frame.pts_us < target {
                self.counters.released_frames = self.counters.released_frames.saturating_add(1);
                return Ok(None);
            }
            self.exact_seek_target_us = None;
        }
        if frame.handle_kind != VESPER_HANDLE_CV_PIXEL_BUFFER || frame.plane_count != 1 {
            return Err(format!(
                "iOS presenter requires a CVPixelBuffer, got {}",
                frame.handle_kind
            ));
        }
        Ok(Some(IosNativeFramePipelineFrame {
            handle: frame.handles[0] as usize,
            presentation_time_us: if frame.pts_us == i64::MIN {
                0
            } else {
                frame.pts_us
            },
            duration_us: (frame.duration_us != i64::MIN).then_some(frame.duration_us),
            width: format.width,
            height: format.height,
            frame_id: Some(frame.lease_id),
            lease: Some(lease),
        }))
    }

    pub fn store_frame(&mut self, frame: IosNativeFramePipelineFrame) -> Result<u64, String> {
        if self.pending_frames.len() >= MAX_PENDING_FRAMES {
            Self::complete_frame(&mut self.counters, frame, false);
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

    /// Transfers the plugin lease to the GPU presenter. The pending handle
    /// remains until the host reports the result or invalidates its epoch.
    pub fn take_presentation_lease(&mut self, handle: u64) -> Option<NativeVideoFrame> {
        self.pending_frames.get_mut(&handle)?.lease.take()
    }

    pub fn is_end_of_stream(&self) -> bool {
        self.end_of_stream_received
    }

    pub fn release_pending_frame(&mut self, handle: u64, presented: bool) -> Result<(), String> {
        let frame = self
            .pending_frames
            .remove(&handle)
            .ok_or_else(|| "invalid iOS native-frame pending frame handle".to_owned())?;
        Self::complete_frame(&mut self.counters, frame, presented);
        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), String> {
        self.release_all_pending_frames();
        self.exact_seek_target_us = None;
        let pipeline = self
            .video
            .as_mut()
            .ok_or_else(|| "iOS video route is closed".to_owned())?;
        pipeline.flush().map_err(|error| error.to_string())?;
        self.end_of_stream_received = false;
        Ok(())
    }

    pub fn seek_to(&mut self, position_millis: u64) -> Result<(), String> {
        if !self.seekable {
            return Err("iOS native-frame pipeline source is not seekable".to_owned());
        }
        self.release_all_pending_frames();
        self.exact_seek_target_us = None;
        let pipeline = self
            .video
            .as_mut()
            .ok_or_else(|| "iOS video route is closed".to_owned())?;
        pipeline
            .seek(Duration::from_millis(position_millis))
            .map_err(|error| error.to_string())?;
        self.exact_seek_target_us =
            Some(position_millis.saturating_mul(1_000).min(i64::MAX as u64) as i64);
        self.end_of_stream_received = false;
        self.counters.seek_count = self.counters.seek_count.saturating_add(1);
        Ok(())
    }

    pub fn close(&mut self) {
        self.release_all_pending_frames();
        self.video = None;
    }

    fn release_all_pending_frames(&mut self) {
        for (_, frame) in self.pending_frames.drain() {
            Self::complete_frame(&mut self.counters, frame, false);
        }
    }

    fn complete_frame(
        counters: &mut IosNativeFramePipelineCounters,
        frame: IosNativeFramePipelineFrame,
        presented: bool,
    ) {
        if let Some(lease) = frame.lease {
            lease.complete(presented);
        }
        counters.released_frames = counters.released_frames.saturating_add(1);
        if presented {
            counters.presented_frames = counters.presented_frames.saturating_add(1);
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
                        "audio PCM decoder plugin `{plugin}` is available for {codec}; the current iOS native-frame route keeps swiftNativeAudioBridge active"
                    ),
                    (Some(codec), None) => format!(
                        "no PCM audio decoder plugin is available for {codec}; the current iOS native-frame route uses swiftNativeAudioBridge"
                    ),
                    (None, Some(plugin)) => format!(
                        "audio PCM decoder plugin `{plugin}` is available; the current iOS native-frame route keeps swiftNativeAudioBridge active"
                    ),
                    (None, None) => {
                        "no PCM audio decoder plugin is available; the current iOS native-frame route uses swiftNativeAudioBridge"
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
) -> Result<MobileSourceNormalizerPacketOpen, String> {
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
    open_mobile_source_normalizer_packet_session(source, configuration)
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

fn open_native_video_decoder(
    plugin: &NativePlugin,
    track: &SourceNormalizerPacketTrackInfo,
) -> Result<NativeVideoDecoder, IosNativeFramePipelineOpenError> {
    let unsupported = |message| IosNativeFramePipelineOpenError::new("unsupportedCodec", message);
    let fourcc = match player_plugin::normalize_decoder_codec_identifier(&track.codec).as_str() {
        "h264" | "avc" | "avc1" => *b"avc1",
        "avc3" => *b"avc3",
        "hevc" | "h265" | "hvc1" => *b"hvc1",
        "hev1" => *b"hev1",
        _ => {
            return Err(unsupported(format!(
                "unsupported VideoToolbox codec {}",
                track.codec
            )));
        }
    };
    let params = serde_json::to_vec(&serde_json::json!({ "reorderDepth": track.reorder_depth }))
        .map_err(|error| unsupported(error.to_string()))?;
    plugin
        .open_video_decoder(
            u32::from_be_bytes(fourcc),
            &track.extradata,
            &params,
            Duration::from_millis(300),
        )
        .map_err(|error| unsupported(error.to_string()))
}

fn resolve_selected_ios_decoder(
    artifacts: &[MobileNativePluginArtifact],
    request: &DecoderPluginMatchRequest,
    require_pcm_frames: bool,
) -> Result<
    Option<(Arc<dyn NativeDecoderPluginFactory>, NativePluginArtifact)>,
    IosNativeFramePipelineOpenError,
> {
    for artifact in artifacts {
        let native_artifact = artifact.loader_artifact().map_err(|error| {
            IosNativeFramePipelineOpenError::new(
                "missingVideoToolboxDecoderPlugin",
                format!("decoder reference is invalid: {error}"),
            )
        })?;
        let registry =
            PluginRegistry::load_native_artifacts([native_artifact.clone()]).map_err(|error| {
                IosNativeFramePipelineOpenError::new(
                    "missingVideoToolboxDecoderPlugin",
                    format!(
                        "decoder artifact {} failed to load: {error}",
                        artifact.library_path.display()
                    ),
                )
            })?;
        let resolved = registry
            .resolve_native_decoder(&artifact.reference)
            .map_err(|error| {
                IosNativeFramePipelineOpenError::new(
                    "missingVideoToolboxDecoderPlugin",
                    format!(
                        "decoder reference {} at {} could not be resolved: {error}",
                        artifact.reference.plugin_id(),
                        artifact.library_path.display()
                    ),
                )
            })?;
        let factory = resolved.capability();
        let capabilities = factory.capabilities();
        if !capabilities.supports_codec(&request.codec, request.media_kind) {
            continue;
        }
        if require_pcm_frames && !capabilities.supports_pcm_frames {
            continue;
        }
        let admitted = registry
            .native_artifact(resolved.reference())
            .map_err(|error| {
                IosNativeFramePipelineOpenError::new(
                    "missingVideoToolboxDecoderPlugin",
                    error.to_string(),
                )
            })?;
        return Ok(Some((factory, admitted)));
    }
    Ok(None)
}

fn audio_pcm_decoder_plugin_name(registry: &PluginRegistry, codec: &str) -> Option<String> {
    registry
        .best_pcm_audio_decoder_for(&DecoderPluginMatchRequest::audio(codec))
        .and_then(|record| record.plugin_name.clone())
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
    use std::thread;

    use player_plugin::{
        DecoderBitstreamFormat, DecoderCapabilities, DecoderCodecCapability, DecoderFrameFormat,
        DecoderMediaKind,
    };
    use player_plugin_loader::{
        DecoderPluginCapabilitySummary, PluginCapabilitySummary, PluginDiagnosticStatus,
    };
    use player_runtime::PlayerPluginDiagnosticStatus;

    use super::*;

    #[test]
    #[ignore = "requires an Apple host and explicitly built native plugin ABI FFmpeg and VideoToolbox plugins"]
    fn ios_native_packet_playback_seeks_and_releases_frames() {
        let env = |name| std::env::var(name).unwrap_or_else(|error| panic!("{name}: {error}"));
        let config = IosNativeFramePipelineOpenConfig {
            source_uri: env("VESPER_NATIVE_IOS_MEDIA"),
            source_normalizer_mode: SourceNormalizerMode::PreflightOnly,
            source_normalizer_plugin_artifacts: Vec::new(),
            source_normalizer_plugin_library_paths: vec![PathBuf::from(env(
                "VESPER_NATIVE_IOS_NORMALIZER",
            ))],
            runtime_profile: None,
            native_frame_pipeline_mode: NativeFramePipelineMode::RequireNativeFrame,
            decoder_plugin_artifacts: Vec::new(),
            decoder_plugin_library_paths: vec![PathBuf::from(env("VESPER_NATIVE_IOS_DECODER"))],
            frame_processor_plugin_artifacts: Vec::new(),
            frame_processor_plugin_library_paths: Vec::new(),
            native_plugin_loading_policy:
                player_runtime::NativePluginLoadingPolicy::DevelopmentRawPaths,
        };
        let mut session =
            IosNativeFramePipelineSession::open(config).expect("open packet playback");
        let advance = |session: &mut IosNativeFramePipelineSession| {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(frame) = session.advance().expect("advance packet playback") {
                    break frame;
                }
                assert!(!session.is_end_of_stream() && std::time::Instant::now() < deadline);
                thread::sleep(Duration::from_millis(2));
            }
        };
        let first = advance(&mut session);
        assert!(first.width > 0 && first.height > 0 && first.handle != 0);
        let retired = session.store_frame(first).expect("hold pre-seek lease");
        let lease = session
            .take_presentation_lease(retired)
            .expect("transfer pre-seek lease to presenter");
        assert!(session.take_presentation_lease(retired).is_none());
        session.seek_to(500).expect("seek in the packet stream");
        assert!(session.release_pending_frame(retired, true).is_err());
        assert!(session.take_presentation_lease(retired).is_none());
        let next = advance(&mut session);
        assert!(next.presentation_time_us >= 500_000);
        let handle = session.store_frame(next).expect("hold decoded frame");
        session
            .release_pending_frame(handle, true)
            .expect("release presented frame");
        assert!(session.release_pending_frame(handle, false).is_err());
        assert_eq!(session.counters.presented_frames, 1);
        session.close();
        session.close();
        drop(session);
        assert!(lease.descriptor().release_fn.is_some());
        lease.complete(false);
    }

    #[test]
    fn ios_native_frame_pipeline_validates_loading_policy_and_required_decoders() {
        let config = IosNativeFramePipelineOpenConfig {
            source_uri: "file:///tmp/video.mp4".to_owned(),
            source_normalizer_mode: SourceNormalizerMode::PreflightOnly,
            source_normalizer_plugin_artifacts: Vec::new(),
            source_normalizer_plugin_library_paths: vec![PathBuf::from(
                "/tmp/must-not-open-source-normalizer.dylib",
            )],
            runtime_profile: Some("default".to_owned()),
            native_frame_pipeline_mode: NativeFramePipelineMode::PreferNativeFrame,
            decoder_plugin_artifacts: Vec::new(),
            decoder_plugin_library_paths: vec![PathBuf::from("/tmp/must-not-open-decoder.dylib")],
            frame_processor_plugin_artifacts: Vec::new(),
            frame_processor_plugin_library_paths: Vec::new(),
            native_plugin_loading_policy: player_runtime::NativePluginLoadingPolicy::DenyRawPaths,
        };

        let error = validate_ios_native_plugin_bindings(&config)
            .expect_err("raw paths must be rejected before any plugin opens");

        assert!(
            error
                .message
                .contains("require explicit development loading policy")
        );
        assert!(!error.message.contains("failed to open plugin library"));
        assert!(!error.message.contains("dlopen"));

        let mut retired_config = config.clone();
        retired_config.frame_processor_plugin_library_paths =
            vec![PathBuf::from("/tmp/must-not-open-frame-processor.dylib")];
        let error = IosNativeFramePipelineSession::open(retired_config)
            .err()
            .expect("retired FrameProcessor must be rejected before plugin admission");
        assert_eq!(error.issue_kind, "startupFailure");
        assert!(error.message.contains("FrameProcessor has been retired"));

        let mut development_config = config;
        development_config.native_plugin_loading_policy =
            player_runtime::NativePluginLoadingPolicy::DevelopmentRawPaths;
        development_config.decoder_plugin_library_paths.clear();
        let error = IosNativeFramePipelineSession::open(development_config)
            .err()
            .expect("a decoder is required before opening a source plugin");
        assert!(
            error
                .wire_message()
                .contains("nativeFrameIssueKind=missingVideoToolboxDecoderPlugin")
        );
        assert!(error.message.contains("VideoToolbox decoder plugin path"));
    }

    #[test]
    fn pending_frame_capacity_rejects_and_retires_excess_frames() {
        let mut session = test_session();
        let make_frame = || IosNativeFramePipelineFrame {
            handle: 1,
            presentation_time_us: 0,
            duration_us: None,
            width: 16,
            height: 16,
            frame_id: None,
            lease: None,
        };
        let handles: Vec<_> = (0..MAX_PENDING_FRAMES)
            .map(|_| session.store_frame(make_frame()).expect("store frame"))
            .collect();
        assert_eq!(
            session
                .store_frame(make_frame())
                .expect_err("reject excess frame"),
            "native-frame pending frame limit reached; rejected frame was released"
        );
        assert_eq!(session.counters.released_frames, 1);
        assert!(
            handles
                .iter()
                .all(|handle| session.pending_frame(*handle).is_some())
        );
        session.close();
        assert!(session.pending_frames.is_empty());
        let stale = handles[0];
        assert_eq!(
            session
                .release_pending_frame(stale, true)
                .expect_err("reject stale handle"),
            "invalid iOS native-frame pending frame handle"
        );
        assert_eq!(
            session.counters.released_frames,
            (MAX_PENDING_FRAMES + 1) as u64
        );
        assert_eq!(session.counters.presented_frames, 0);
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
        let session = test_session();

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
    fn open_wire_reports_available_audio_pcm_decoder_without_switching_audio_bridge() {
        let mut session = test_session();
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
        let mut session = test_session();
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

    fn test_session() -> IosNativeFramePipelineSession {
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
            video: None,
            end_of_stream_received: true,
            exact_seek_target_us: None,
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
            plugin_kind: Some(player_plugin_loader::PluginCapabilityKind::Decoder),
            capability_summary: Some(PluginCapabilitySummary::Decoder(
                DecoderPluginCapabilitySummary::from(&capabilities),
            )),
            message: None,
        }
    }
}
