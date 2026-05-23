#![warn(clippy::undocumented_unsafe_blocks)]

use std::ffi::{CStr, CString, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use libloading::Library;
use player_plugin::{
    BenchmarkEventBatch, BenchmarkSink, BenchmarkSinkError, BenchmarkSinkReport,
    BenchmarkSinkStatus, CompletedDownloadInfo, DecoderCapabilities, DecoderCodecCapability,
    DecoderError, DecoderMediaKind, DecoderNativeFrame, DecoderNativeRequirements,
    DecoderOperationStatus, DecoderPacket, DecoderPacketResult, DecoderReceiveFrameStatus,
    DecoderReceiveNativeFrameMetadata, DecoderReceiveNativeFrameOutput, DecoderSessionConfig,
    DecoderSessionInfo, FrameProcessorCapabilities, FrameProcessorError,
    FrameProcessorOperationStatus, FrameProcessorOutputFrame, FrameProcessorPluginFactory,
    FrameProcessorReceiveFrameMetadata, FrameProcessorReceiveOutput, FrameProcessorReceiveStatus,
    FrameProcessorSession, FrameProcessorSessionConfig, FrameProcessorSessionInfo,
    FrameProcessorSubmitFrame, FrameProcessorSubmitResult, NativeDecoderPluginFactory,
    NativeDecoderSession, NativeFrame, NativeHandleKind, PipelineEvent, PipelineEventHook,
    PostDownloadProcessor, ProcessorCapabilities, ProcessorError, ProcessorOutput,
    ProcessorProgress, SourceNormalizerError, SourceNormalizerOperationStatus,
    SourceNormalizerPacketCapabilities, SourceNormalizerPacketLease,
    SourceNormalizerPacketPluginFactory, SourceNormalizerPacketSeek, SourceNormalizerPacketSession,
    SourceNormalizerPacketSessionConfig, SourceNormalizerPacketStreamInfo,
    SourceNormalizerReadPacketMetadata, SourceNormalizerReadPacketStatus,
    VESPER_DECODER_PLUGIN_ABI_VERSION_V3, VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_V1,
    VESPER_PLUGIN_ABI_VERSION_V2, VESPER_PLUGIN_ENTRY_SYMBOL,
    VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3, VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2,
    VesperBenchmarkSinkApi, VesperDecoderOpenSessionResult, VesperDecoderPluginApiV2,
    VesperDecoderReceiveNativeFrameResult, VesperFrameProcessorOpenSessionResult,
    VesperFrameProcessorPluginApiV1, VesperFrameProcessorReceiveFrameResult,
    VesperPipelineEventHookApi, VesperPluginBytes, VesperPluginDescriptor, VesperPluginEntryPoint,
    VesperPluginKind, VesperPluginProcessResult, VesperPluginProgressCallbacks,
    VesperPluginResultStatus, VesperPostDownloadProcessorApi,
    VesperSourceNormalizerOpenPacketSessionResult, VesperSourceNormalizerPluginApiV2,
    VesperSourceNormalizerReadPacketResult,
};
use serde::de::DeserializeOwned;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PluginLoadError {
    #[error("failed to open plugin library at {path}: {source}")]
    OpenLibrary {
        path: String,
        #[source]
        source: libloading::Error,
    },
    #[error("failed to resolve plugin entry symbol `{symbol}`: {source}")]
    ResolveEntrySymbol {
        symbol: &'static str,
        #[source]
        source: libloading::Error,
    },
    #[error("plugin descriptor pointer is null")]
    NullDescriptor,
    #[error("plugin ABI version mismatch: expected {expected}, got {actual}")]
    AbiVersionMismatch { expected: u32, actual: u32 },
    #[error("plugin field `{field}` is missing")]
    MissingField { field: &'static str },
    #[error("plugin field `{field}` is not valid UTF-8")]
    InvalidUtf8 { field: &'static str },
    #[error("failed to decode plugin capabilities JSON: {0}")]
    DecodeCapabilities(#[source] serde_json::Error),
    #[error("plugin capabilities payload violates ABI: {0}")]
    CapabilitiesAbiViolation(String),
}

#[derive(Debug, Error)]
enum PluginPayloadError {
    #[error("plugin payload pointer is null while len is {len}")]
    NullPayloadWithLength { len: usize },
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Debug)]
pub struct LoadedDynamicPlugin {
    name: String,
    plugin_kind: VesperPluginKind,
    post_download_processor: Option<Arc<DynamicPostDownloadProcessor>>,
    pipeline_event_hook: Option<Arc<DynamicPipelineEventHook>>,
    benchmark_sink: Option<Arc<DynamicBenchmarkSink>>,
    native_decoder_plugin_factory: Option<Arc<DynamicNativeDecoderPluginFactory>>,
    frame_processor_plugin_factory: Option<Arc<DynamicFrameProcessorPluginFactory>>,
    source_normalizer_packet_plugin_factory:
        Option<Arc<DynamicSourceNormalizerPacketPluginFactory>>,
}

pub struct BenchmarkSinkPluginSession {
    sinks: Vec<Arc<dyn BenchmarkSink>>,
}

impl std::fmt::Debug for BenchmarkSinkPluginSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BenchmarkSinkPluginSession")
            .field("sink_count", &self.sinks.len())
            .finish()
    }
}

impl BenchmarkSinkPluginSession {
    pub fn load_paths(
        paths: impl IntoIterator<Item = impl AsRef<Path>>,
    ) -> Result<Self, PluginLoadError> {
        let mut sinks = Vec::new();
        for path in paths {
            let plugin = LoadedDynamicPlugin::load(path.as_ref())?;
            if let Some(sink) = plugin.benchmark_sink() {
                sinks.push(sink);
            }
        }

        Ok(Self { sinks })
    }

    pub fn is_empty(&self) -> bool {
        self.sinks.is_empty()
    }

    pub fn on_event_batch_json(
        &self,
        batch_json: &str,
    ) -> Result<BenchmarkSinkReport, BenchmarkSinkError> {
        let batch = serde_json::from_str::<BenchmarkEventBatch>(batch_json).map_err(|error| {
            BenchmarkSinkError::PayloadCodec(format!(
                "decode benchmark event batch payload failed: {error}"
            ))
        })?;
        Ok(self.on_event_batch(&batch))
    }

    pub fn on_event_batch_report_json(
        &self,
        batch_json: &str,
    ) -> Result<String, BenchmarkSinkError> {
        serde_json::to_string(&self.on_event_batch_json(batch_json)?).map_err(|error| {
            BenchmarkSinkError::PayloadCodec(format!(
                "encode benchmark sink status failed: {error}"
            ))
        })
    }

    pub fn on_event_batch(&self, batch: &BenchmarkEventBatch) -> BenchmarkSinkReport {
        let mut report = BenchmarkSinkReport::default();
        for sink in &self.sinks {
            match sink.on_event_batch(batch) {
                Ok(status) => {
                    report.accepted_events += status.accepted_events;
                }
                Err(error) => {
                    report.dropped_events += batch.events.len() as u64;
                    report
                        .plugin_errors
                        .push(format!("{}: {error}", sink.name()));
                }
            }
        }
        report
    }

    pub fn flush(&self) -> BenchmarkSinkReport {
        let mut report = BenchmarkSinkReport::default();
        for sink in &self.sinks {
            match sink.flush() {
                Ok(sink_report) => {
                    report.accepted_events += sink_report.accepted_events;
                    report.dropped_events += sink_report.dropped_events;
                    report.plugin_errors.extend(sink_report.plugin_errors);
                }
                Err(error) => {
                    report
                        .plugin_errors
                        .push(format!("{}: {error}", sink.name()));
                }
            }
        }
        report
    }

    pub fn flush_json(&self) -> Result<String, BenchmarkSinkError> {
        serde_json::to_string(&self.flush()).map_err(|error| {
            BenchmarkSinkError::PayloadCodec(format!(
                "encode benchmark sink report failed: {error}"
            ))
        })
    }
}

impl LoadedDynamicPlugin {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, PluginLoadError> {
        let path = path.as_ref();
        let path_string = path.display().to_string();
        // SAFETY: `path` comes from the caller, and the resulting `Library` is
        // stored in `LibraryHolder` so any symbols borrowed from it stay valid.
        let library =
            unsafe { Library::new(path) }.map_err(|source| PluginLoadError::OpenLibrary {
                path: path_string,
                source,
            })?;

        // SAFETY: the symbol name is a static NUL-terminated byte string and
        // the plugin contract requires it to have the `VesperPluginEntryPoint`
        // signature.
        let entry = unsafe { library.get::<VesperPluginEntryPoint>(VESPER_PLUGIN_ENTRY_SYMBOL) }
            .map_err(|source| PluginLoadError::ResolveEntrySymbol {
                symbol: "vesper_plugin_entry",
                source,
            })?;

        // SAFETY: the plugin entry point follows the shared ABI and returns a
        // process-lifetime descriptor pointer when loading succeeds.
        let descriptor_ptr = unsafe { entry() };
        let descriptor =
            // SAFETY: `descriptor_ptr` came from `vesper_plugin_entry`; the ABI
            // guarantees it points to a valid descriptor or null on failure.
            unsafe { descriptor_ptr.as_ref() }.ok_or(PluginLoadError::NullDescriptor)?;
        let library = Arc::new(LibraryHolder { library });
        Self::from_descriptor(Some(library), descriptor)
    }

    pub fn plugin_name(&self) -> &str {
        &self.name
    }

    pub fn plugin_kind(&self) -> VesperPluginKind {
        self.plugin_kind
    }

    pub fn post_download_processor(&self) -> Option<Arc<dyn PostDownloadProcessor>> {
        self.post_download_processor
            .clone()
            .map(|processor| processor as Arc<dyn PostDownloadProcessor>)
    }

    pub fn pipeline_event_hook(&self) -> Option<Arc<dyn PipelineEventHook>> {
        self.pipeline_event_hook
            .clone()
            .map(|hook| hook as Arc<dyn PipelineEventHook>)
    }

    pub fn benchmark_sink(&self) -> Option<Arc<dyn BenchmarkSink>> {
        self.benchmark_sink
            .clone()
            .map(|sink| sink as Arc<dyn BenchmarkSink>)
    }

    pub fn native_decoder_plugin_factory(&self) -> Option<Arc<dyn NativeDecoderPluginFactory>> {
        self.native_decoder_plugin_factory
            .clone()
            .map(|factory| factory as Arc<dyn NativeDecoderPluginFactory>)
    }

    pub fn frame_processor_plugin_factory(&self) -> Option<Arc<dyn FrameProcessorPluginFactory>> {
        self.frame_processor_plugin_factory
            .clone()
            .map(|factory| factory as Arc<dyn FrameProcessorPluginFactory>)
    }

    pub fn source_normalizer_packet_plugin_factory(
        &self,
    ) -> Option<Arc<dyn SourceNormalizerPacketPluginFactory>> {
        self.source_normalizer_packet_plugin_factory
            .clone()
            .map(|factory| factory as Arc<dyn SourceNormalizerPacketPluginFactory>)
    }

    fn from_descriptor(
        library: Option<Arc<LibraryHolder>>,
        descriptor: &VesperPluginDescriptor,
    ) -> Result<Self, PluginLoadError> {
        let expected_abi_version = match descriptor.plugin_kind {
            VesperPluginKind::PostDownloadProcessor => VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
            VesperPluginKind::PipelineEventHook | VesperPluginKind::BenchmarkSink => {
                VESPER_PLUGIN_ABI_VERSION_V2
            }
            VesperPluginKind::Decoder => VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
            VesperPluginKind::FrameProcessor => VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_V1,
            VesperPluginKind::SourceNormalizer => VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2,
        };
        if descriptor.abi_version != expected_abi_version {
            return Err(PluginLoadError::AbiVersionMismatch {
                expected: expected_abi_version,
                actual: descriptor.abi_version,
            });
        }

        let descriptor_name = c_string_field(descriptor.plugin_name, "plugin_name")?;
        match descriptor.plugin_kind {
            VesperPluginKind::PostDownloadProcessor => {
                let api_ptr = descriptor.api.cast::<VesperPostDownloadProcessorApi>();
                let api =
                    // SAFETY: `descriptor.api` must point at the ABI table that
                    // matches `plugin_kind` when the plugin exports a valid
                    // descriptor.
                    unsafe { api_ptr.as_ref() }.ok_or(PluginLoadError::MissingField {
                        field: "post_download_processor_api",
                    })?;
                let processor = DynamicPostDownloadProcessor::new(
                    library,
                    descriptor_name.clone(),
                    CheckedPostDownloadProcessorApi::try_from(*api)?,
                )?;
                Ok(Self {
                    name: descriptor_name,
                    plugin_kind: descriptor.plugin_kind,
                    post_download_processor: Some(Arc::new(processor)),
                    pipeline_event_hook: None,
                    benchmark_sink: None,
                    native_decoder_plugin_factory: None,
                    frame_processor_plugin_factory: None,
                    source_normalizer_packet_plugin_factory: None,
                })
            }
            VesperPluginKind::PipelineEventHook => {
                let api_ptr = descriptor.api.cast::<VesperPipelineEventHookApi>();
                let api =
                    // SAFETY: `descriptor.api` must point at the ABI table that
                    // matches `plugin_kind` when the plugin exports a valid
                    // descriptor.
                    unsafe { api_ptr.as_ref() }.ok_or(PluginLoadError::MissingField {
                        field: "pipeline_event_hook_api",
                    })?;
                let hook = DynamicPipelineEventHook::new(
                    library,
                    descriptor_name.clone(),
                    CheckedPipelineEventHookApi::try_from(*api)?,
                )?;
                Ok(Self {
                    name: descriptor_name,
                    plugin_kind: descriptor.plugin_kind,
                    post_download_processor: None,
                    pipeline_event_hook: Some(Arc::new(hook)),
                    benchmark_sink: None,
                    native_decoder_plugin_factory: None,
                    frame_processor_plugin_factory: None,
                    source_normalizer_packet_plugin_factory: None,
                })
            }
            VesperPluginKind::BenchmarkSink => {
                let api_ptr = descriptor.api.cast::<VesperBenchmarkSinkApi>();
                let api =
                    // SAFETY: `descriptor.api` must point at the ABI table that
                    // matches `plugin_kind` when the plugin exports a valid
                    // descriptor.
                    unsafe { api_ptr.as_ref() }.ok_or(PluginLoadError::MissingField {
                        field: "benchmark_sink_api",
                    })?;
                let sink = DynamicBenchmarkSink::new(
                    library,
                    descriptor_name.clone(),
                    CheckedBenchmarkSinkApi::try_from(*api)?,
                )?;
                Ok(Self {
                    name: descriptor_name,
                    plugin_kind: descriptor.plugin_kind,
                    post_download_processor: None,
                    pipeline_event_hook: None,
                    benchmark_sink: Some(Arc::new(sink)),
                    native_decoder_plugin_factory: None,
                    frame_processor_plugin_factory: None,
                    source_normalizer_packet_plugin_factory: None,
                })
            }
            VesperPluginKind::Decoder => {
                let api_ptr = descriptor.api.cast::<VesperDecoderPluginApiV2>();
                let api =
                    // SAFETY: `descriptor.api` must point at the v2 decoder ABI table
                    // when the plugin exports a valid decoder descriptor.
                    unsafe { api_ptr.as_ref() }.ok_or(PluginLoadError::MissingField {
                        field: "decoder_plugin_api_v2",
                    })?;
                let factory = DynamicNativeDecoderPluginFactory::new(
                    library,
                    descriptor_name.clone(),
                    CheckedNativeDecoderPluginApi::try_from(*api)?,
                )?;
                Ok(Self {
                    name: descriptor_name,
                    plugin_kind: descriptor.plugin_kind,
                    post_download_processor: None,
                    pipeline_event_hook: None,
                    benchmark_sink: None,
                    native_decoder_plugin_factory: Some(Arc::new(factory)),
                    frame_processor_plugin_factory: None,
                    source_normalizer_packet_plugin_factory: None,
                })
            }
            VesperPluginKind::FrameProcessor => {
                let api_ptr = descriptor.api.cast::<VesperFrameProcessorPluginApiV1>();
                let api =
                    // SAFETY: `descriptor.api` must point at the v1 frame processor
                    // ABI table when the plugin exports a valid frame processor descriptor.
                    unsafe { api_ptr.as_ref() }.ok_or(PluginLoadError::MissingField {
                        field: "frame_processor_plugin_api_v1",
                    })?;
                let factory = DynamicFrameProcessorPluginFactory::new(
                    library,
                    descriptor_name.clone(),
                    CheckedFrameProcessorPluginApi::try_from(*api)?,
                )?;
                Ok(Self {
                    name: descriptor_name,
                    plugin_kind: descriptor.plugin_kind,
                    post_download_processor: None,
                    pipeline_event_hook: None,
                    benchmark_sink: None,
                    native_decoder_plugin_factory: None,
                    frame_processor_plugin_factory: Some(Arc::new(factory)),
                    source_normalizer_packet_plugin_factory: None,
                })
            }
            VesperPluginKind::SourceNormalizer => {
                let api_ptr = descriptor.api.cast::<VesperSourceNormalizerPluginApiV2>();
                let api =
                    // SAFETY: `descriptor.api` must point at the v2 source normalizer
                    // ABI table when the plugin exports a valid source normalizer descriptor.
                    unsafe { api_ptr.as_ref() }.ok_or(PluginLoadError::MissingField {
                        field: "source_normalizer_plugin_api_v2",
                    })?;
                let factory = DynamicSourceNormalizerPacketPluginFactory::new(
                    library,
                    descriptor_name.clone(),
                    CheckedSourceNormalizerPacketPluginApi::try_from(*api)?,
                )?;
                Ok(Self {
                    name: descriptor_name,
                    plugin_kind: descriptor.plugin_kind,
                    post_download_processor: None,
                    pipeline_event_hook: None,
                    benchmark_sink: None,
                    native_decoder_plugin_factory: None,
                    frame_processor_plugin_factory: None,
                    source_normalizer_packet_plugin_factory: Some(Arc::new(factory)),
                })
            }
        }
    }
}

/// Codec/media request used when matching decoder plugin capabilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoderPluginMatchRequest {
    pub codec: String,
    pub media_kind: DecoderMediaKind,
}

impl DecoderPluginMatchRequest {
    pub fn video(codec: impl Into<String>) -> Self {
        Self {
            codec: codec.into(),
            media_kind: DecoderMediaKind::Video,
        }
    }
}

/// Structured codec entry reported by one decoder plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoderPluginCodecSummary {
    pub codec: String,
    pub media_kind: DecoderMediaKind,
}

impl From<&DecoderCodecCapability> for DecoderPluginCodecSummary {
    fn from(capability: &DecoderCodecCapability) -> Self {
        Self {
            codec: capability.codec.clone(),
            media_kind: capability.media_kind,
        }
    }
}

/// Compact capability summary for one decoder plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoderPluginCapabilitySummary {
    pub typed_codecs: Vec<DecoderPluginCodecSummary>,
    pub codecs: Vec<String>,
    pub supports_native_frame_output: bool,
    pub native_requirements: Option<DecoderNativeRequirements>,
    pub supports_hardware_decode: bool,
    pub supports_cpu_video_frames: bool,
    pub supports_audio_frames: bool,
    pub supports_gpu_handles: bool,
    pub supports_flush: bool,
    pub supports_drain: bool,
    pub max_sessions: Option<u32>,
}

impl From<&DecoderCapabilities> for DecoderPluginCapabilitySummary {
    fn from(capabilities: &DecoderCapabilities) -> Self {
        Self::from_capabilities(capabilities, false, None)
    }
}

/// Compact capability summary for one frame processor plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameProcessorPluginCapabilitySummary {
    pub accepted_input_handle_kinds: Vec<NativeHandleKind>,
    pub output_handle_kinds: Vec<NativeHandleKind>,
    pub supports_video_frames: bool,
    pub supports_in_place_passthrough: bool,
    pub preserves_dimensions: bool,
    pub may_change_dimensions: bool,
    pub preserves_color_metadata: bool,
    pub preserves_hdr_metadata: bool,
    pub supports_flush: bool,
    pub max_sessions: Option<u32>,
    pub max_in_flight_frames: Option<u32>,
}

impl From<&FrameProcessorCapabilities> for FrameProcessorPluginCapabilitySummary {
    fn from(capabilities: &FrameProcessorCapabilities) -> Self {
        Self {
            accepted_input_handle_kinds: capabilities.accepted_input_handle_kinds.clone(),
            output_handle_kinds: capabilities.output_handle_kinds.clone(),
            supports_video_frames: capabilities.supports_video_frames,
            supports_in_place_passthrough: capabilities.supports_in_place_passthrough,
            preserves_dimensions: capabilities.preserves_dimensions,
            may_change_dimensions: capabilities.may_change_dimensions,
            preserves_color_metadata: capabilities.preserves_color_metadata,
            preserves_hdr_metadata: capabilities.preserves_hdr_metadata,
            supports_flush: capabilities.supports_flush,
            max_sessions: capabilities.max_sessions,
            max_in_flight_frames: capabilities.max_in_flight_frames,
        }
    }
}

/// Compact capability summary for one packet-stream source normalizer plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceNormalizerPacketPluginCapabilitySummary {
    pub supported_runtime_profiles: Vec<String>,
    pub max_level: player_plugin::SourceNormalizerNormalizeLevel,
    pub media_kinds: Vec<player_plugin::SourceNormalizerPacketMediaKind>,
    pub codecs: Vec<String>,
    pub bitstream_formats: Vec<player_plugin::DecoderBitstreamFormat>,
    pub supports_seek: bool,
    pub supports_flush: bool,
    pub required_capabilities: player_plugin::SourceNormalizerRequiredCapabilities,
    pub max_sessions: Option<u32>,
}

impl From<&SourceNormalizerPacketCapabilities> for SourceNormalizerPacketPluginCapabilitySummary {
    fn from(capabilities: &SourceNormalizerPacketCapabilities) -> Self {
        Self {
            supported_runtime_profiles: capabilities.supported_runtime_profiles.clone(),
            max_level: capabilities.max_level,
            media_kinds: capabilities.media_kinds.clone(),
            codecs: capabilities.codecs.clone(),
            bitstream_formats: capabilities.bitstream_formats.clone(),
            supports_seek: capabilities.supports_seek,
            supports_flush: capabilities.supports_flush,
            required_capabilities: capabilities.required_capabilities.clone(),
            max_sessions: capabilities.max_sessions,
        }
    }
}

/// Capability summary for one loaded plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginCapabilitySummary {
    Decoder(DecoderPluginCapabilitySummary),
    FrameProcessor(FrameProcessorPluginCapabilitySummary),
    SourceNormalizerPacket(SourceNormalizerPacketPluginCapabilitySummary),
}

impl DecoderPluginCapabilitySummary {
    fn from_capabilities(
        capabilities: &DecoderCapabilities,
        supports_native_frame_output: bool,
        native_requirements: Option<DecoderNativeRequirements>,
    ) -> Self {
        let typed_codecs = capabilities
            .codecs
            .iter()
            .map(DecoderPluginCodecSummary::from)
            .collect::<Vec<_>>();
        let codecs = capabilities
            .codecs
            .iter()
            .map(|codec| format!("{:?}:{}", codec.media_kind, codec.codec))
            .collect();
        Self {
            typed_codecs,
            codecs,
            supports_native_frame_output,
            native_requirements,
            supports_hardware_decode: capabilities.supports_hardware_decode,
            supports_cpu_video_frames: capabilities.supports_cpu_video_frames,
            supports_audio_frames: capabilities.supports_audio_frames,
            supports_gpu_handles: capabilities.supports_gpu_handles,
            supports_flush: capabilities.supports_flush,
            supports_drain: capabilities.supports_drain,
            max_sessions: capabilities.max_sessions,
        }
    }
}

/// Loader-side diagnostic status for one plugin path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginDiagnosticStatus {
    Loaded,
    LoadFailed,
    UnsupportedKind,
    DecoderSupported,
    DecoderUnsupported,
    FrameProcessorSupported,
    FrameProcessorUnsupported,
    SourceNormalizerSupported,
    SourceNormalizerUnsupported,
}

/// Structured diagnostic record for one dynamic plugin path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDiagnosticRecord {
    pub path: PathBuf,
    pub status: PluginDiagnosticStatus,
    pub plugin_name: Option<String>,
    pub plugin_kind: Option<VesperPluginKind>,
    pub capability_summary: Option<PluginCapabilitySummary>,
    pub message: Option<String>,
}

fn decoder_capability_summary(
    record: &PluginDiagnosticRecord,
) -> Option<&DecoderPluginCapabilitySummary> {
    match record.capability_summary.as_ref() {
        Some(PluginCapabilitySummary::Decoder(summary)) => Some(summary),
        _ => None,
    }
}

fn source_normalizer_packet_capability_summary(
    record: &PluginDiagnosticRecord,
) -> Option<&SourceNormalizerPacketPluginCapabilitySummary> {
    match record.capability_summary.as_ref() {
        Some(PluginCapabilitySummary::SourceNormalizerPacket(summary)) => Some(summary),
        _ => None,
    }
}

impl PluginDiagnosticRecord {
    pub fn from_loaded_plugin(
        path: impl Into<PathBuf>,
        plugin: &LoadedDynamicPlugin,
        decoder_match: Option<&DecoderPluginMatchRequest>,
    ) -> Self {
        let path = path.into();
        match decoder_factory_summary(plugin) {
            Some((name, capabilities, native_frame_output, native_requirements)) => {
                let capability_summary = DecoderPluginCapabilitySummary::from_capabilities(
                    &capabilities,
                    native_frame_output,
                    native_requirements.clone(),
                );
                match decoder_match {
                    Some(request)
                        if capabilities.supports_codec(&request.codec, request.media_kind) =>
                    {
                        Self {
                            path,
                            status: PluginDiagnosticStatus::DecoderSupported,
                            plugin_name: Some(name.clone()),
                            plugin_kind: Some(plugin.plugin_kind()),
                            capability_summary: Some(PluginCapabilitySummary::Decoder(
                                capability_summary,
                            )),
                            message: Some(format!(
                                "{} advertises {:?} {} support{}",
                                name,
                                request.media_kind,
                                request.codec,
                                if native_frame_output {
                                    " with native-frame output"
                                } else {
                                    ""
                                }
                            )),
                        }
                    }
                    Some(request) => Self {
                        path,
                        status: PluginDiagnosticStatus::DecoderUnsupported,
                        plugin_name: Some(name.clone()),
                        plugin_kind: Some(plugin.plugin_kind()),
                        capability_summary: Some(PluginCapabilitySummary::Decoder(
                            capability_summary,
                        )),
                        message: Some(format!(
                            "{} does not advertise {:?} {} support",
                            name, request.media_kind, request.codec
                        )),
                    },
                    None => Self {
                        path,
                        status: PluginDiagnosticStatus::Loaded,
                        plugin_name: Some(name.clone()),
                        plugin_kind: Some(plugin.plugin_kind()),
                        capability_summary: Some(PluginCapabilitySummary::Decoder(
                            capability_summary,
                        )),
                        message: Some(format!(
                            "{} decoder plugin loaded{}",
                            name,
                            if native_frame_output {
                                " with native-frame output"
                            } else {
                                ""
                            }
                        )),
                    },
                }
            }
            None => Self {
                path,
                status: PluginDiagnosticStatus::UnsupportedKind,
                plugin_name: Some(plugin.plugin_name().to_owned()),
                plugin_kind: Some(plugin.plugin_kind()),
                capability_summary: frame_processor_factory_summary(plugin)
                    .map(|capabilities| {
                        PluginCapabilitySummary::FrameProcessor(
                            FrameProcessorPluginCapabilitySummary::from(&capabilities),
                        )
                    })
                    .or_else(|| {
                        source_normalizer_packet_factory_summary(plugin).map(|capabilities| {
                            PluginCapabilitySummary::SourceNormalizerPacket(
                                SourceNormalizerPacketPluginCapabilitySummary::from(&capabilities),
                            )
                        })
                    }),
                message: Some(format!("{} is not a decoder plugin", plugin.plugin_name())),
            },
        }
    }

    pub fn from_loaded_frame_processor_plugin(
        path: impl Into<PathBuf>,
        plugin: &LoadedDynamicPlugin,
    ) -> Self {
        let path = path.into();
        if let Some((name, capabilities)) = frame_processor_summary(plugin) {
            let capability_summary = FrameProcessorPluginCapabilitySummary::from(&capabilities);
            let supported =
                capabilities.supports_video_frames && !capabilities.may_change_dimensions;
            let status = if supported {
                PluginDiagnosticStatus::FrameProcessorSupported
            } else {
                PluginDiagnosticStatus::FrameProcessorUnsupported
            };
            let message = if supported {
                format!("{name} frame processor plugin loaded")
            } else if capabilities.may_change_dimensions {
                format!("{name} frame processor changes frame dimensions, which v1 does not allow")
            } else {
                format!("{name} does not advertise video frame processing support")
            };
            return Self {
                path,
                status,
                plugin_name: Some(name),
                plugin_kind: Some(plugin.plugin_kind()),
                capability_summary: Some(PluginCapabilitySummary::FrameProcessor(
                    capability_summary,
                )),
                message: Some(message),
            };
        }

        let decoder_summary = decoder_factory_summary(plugin).map(
            |(_, capabilities, native_frame_output, native_requirements)| {
                PluginCapabilitySummary::Decoder(DecoderPluginCapabilitySummary::from_capabilities(
                    &capabilities,
                    native_frame_output,
                    native_requirements,
                ))
            },
        );

        Self {
            path,
            status: PluginDiagnosticStatus::UnsupportedKind,
            plugin_name: Some(plugin.plugin_name().to_owned()),
            plugin_kind: Some(plugin.plugin_kind()),
            capability_summary: decoder_summary.or_else(|| {
                source_normalizer_packet_factory_summary(plugin).map(|capabilities| {
                    PluginCapabilitySummary::SourceNormalizerPacket(
                        SourceNormalizerPacketPluginCapabilitySummary::from(&capabilities),
                    )
                })
            }),
            message: Some(format!(
                "{} is not a frame processor plugin",
                plugin.plugin_name()
            )),
        }
    }

    pub fn from_loaded_source_normalizer_plugin(
        path: impl Into<PathBuf>,
        plugin: &LoadedDynamicPlugin,
    ) -> Self {
        let path = path.into();
        if let Some((name, capabilities)) = source_normalizer_packet_summary(plugin) {
            let capability_summary =
                SourceNormalizerPacketPluginCapabilitySummary::from(&capabilities);
            let supported = !capabilities.supported_runtime_profiles.is_empty()
                && !capabilities.media_kinds.is_empty();
            let status = if supported {
                PluginDiagnosticStatus::SourceNormalizerSupported
            } else {
                PluginDiagnosticStatus::SourceNormalizerUnsupported
            };
            let message = if supported {
                format!("{name} source_normalizer_packet_v2 plugin loaded")
            } else if capabilities.supported_runtime_profiles.is_empty() {
                format!("{name} does not advertise packet source normalizer runtime profiles")
            } else {
                format!("{name} does not advertise packet source normalizer media kinds")
            };
            return Self {
                path,
                status,
                plugin_name: Some(name),
                plugin_kind: Some(plugin.plugin_kind()),
                capability_summary: Some(PluginCapabilitySummary::SourceNormalizerPacket(
                    capability_summary,
                )),
                message: Some(message),
            };
        }

        let capability_summary = decoder_factory_summary(plugin)
            .map(
                |(_, capabilities, native_frame_output, native_requirements)| {
                    PluginCapabilitySummary::Decoder(
                        DecoderPluginCapabilitySummary::from_capabilities(
                            &capabilities,
                            native_frame_output,
                            native_requirements,
                        ),
                    )
                },
            )
            .or_else(|| {
                frame_processor_factory_summary(plugin).map(|capabilities| {
                    PluginCapabilitySummary::FrameProcessor(
                        FrameProcessorPluginCapabilitySummary::from(&capabilities),
                    )
                })
            })
            .or_else(|| {
                source_normalizer_packet_factory_summary(plugin).map(|capabilities| {
                    PluginCapabilitySummary::SourceNormalizerPacket(
                        SourceNormalizerPacketPluginCapabilitySummary::from(&capabilities),
                    )
                })
            });

        Self {
            path,
            status: PluginDiagnosticStatus::UnsupportedKind,
            plugin_name: Some(plugin.plugin_name().to_owned()),
            plugin_kind: Some(plugin.plugin_kind()),
            capability_summary,
            message: Some(format!(
                "{} is not a source normalizer plugin",
                plugin.plugin_name()
            )),
        }
    }

    pub fn load_failed(path: impl Into<PathBuf>, error: PluginLoadError) -> Self {
        let path = path.into();
        Self {
            path,
            status: PluginDiagnosticStatus::LoadFailed,
            plugin_name: None,
            plugin_kind: None,
            capability_summary: None,
            message: Some(error.to_string()),
        }
    }

    pub fn summary(&self) -> String {
        self.message
            .clone()
            .or_else(|| self.plugin_name.clone())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}

fn decoder_factory_summary(
    plugin: &LoadedDynamicPlugin,
) -> Option<(
    String,
    DecoderCapabilities,
    bool,
    Option<DecoderNativeRequirements>,
)> {
    plugin.native_decoder_plugin_factory().map(|factory| {
        (
            factory.name().to_owned(),
            factory.capabilities(),
            true,
            Some(factory.native_requirements()),
        )
    })
}

fn frame_processor_summary(
    plugin: &LoadedDynamicPlugin,
) -> Option<(String, FrameProcessorCapabilities)> {
    plugin
        .frame_processor_plugin_factory()
        .map(|factory| (factory.name().to_owned(), factory.capabilities()))
}

fn frame_processor_factory_summary(
    plugin: &LoadedDynamicPlugin,
) -> Option<FrameProcessorCapabilities> {
    plugin
        .frame_processor_plugin_factory()
        .map(|factory| factory.capabilities())
}

fn source_normalizer_packet_summary(
    plugin: &LoadedDynamicPlugin,
) -> Option<(String, SourceNormalizerPacketCapabilities)> {
    plugin
        .source_normalizer_packet_plugin_factory()
        .map(|factory| (factory.name().to_owned(), factory.packet_capabilities()))
}

fn source_normalizer_packet_factory_summary(
    plugin: &LoadedDynamicPlugin,
) -> Option<SourceNormalizerPacketCapabilities> {
    plugin
        .source_normalizer_packet_plugin_factory()
        .map(|factory| factory.packet_capabilities())
}

/// Aggregated loader-side report for inspected dynamic plugin paths.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PluginRegistryReport {
    pub total: usize,
    pub loaded: usize,
    pub failed: usize,
    pub decoder_supported: usize,
    pub decoder_unsupported: usize,
    pub frame_processor_supported: usize,
    pub frame_processor_unsupported: usize,
    pub source_normalizer_supported: usize,
    pub source_normalizer_unsupported: usize,
    pub unsupported_kind: usize,
    pub best_supported_decoder_name: Option<String>,
    pub best_supported_frame_processor_name: Option<String>,
    pub best_supported_source_normalizer_name: Option<String>,
    pub diagnostic_notes: Vec<String>,
}

/// Structured report for dynamic plugins loaded from host-provided paths.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PluginRegistry {
    records: Vec<PluginDiagnosticRecord>,
}

impl PluginRegistry {
    pub fn inspect_decoder_support(
        paths: impl IntoIterator<Item = impl AsRef<Path>>,
        request: DecoderPluginMatchRequest,
    ) -> Self {
        let records = paths
            .into_iter()
            .map(|path| {
                let path = path.as_ref().to_path_buf();
                match LoadedDynamicPlugin::load(&path) {
                    Ok(plugin) => {
                        PluginDiagnosticRecord::from_loaded_plugin(path, &plugin, Some(&request))
                    }
                    Err(error) => PluginDiagnosticRecord::load_failed(path, error),
                }
            })
            .collect();
        Self { records }
    }

    pub fn inspect_frame_processor_support(
        paths: impl IntoIterator<Item = impl AsRef<Path>>,
    ) -> Self {
        let records = paths
            .into_iter()
            .map(|path| {
                let path = path.as_ref().to_path_buf();
                match LoadedDynamicPlugin::load(&path) {
                    Ok(plugin) => {
                        PluginDiagnosticRecord::from_loaded_frame_processor_plugin(path, &plugin)
                    }
                    Err(error) => PluginDiagnosticRecord::load_failed(path, error),
                }
            })
            .collect();
        Self { records }
    }

    pub fn inspect_source_normalizer_support(
        paths: impl IntoIterator<Item = impl AsRef<Path>>,
    ) -> Self {
        let records = paths
            .into_iter()
            .map(|path| {
                let path = path.as_ref().to_path_buf();
                match LoadedDynamicPlugin::load(&path) {
                    Ok(plugin) => {
                        PluginDiagnosticRecord::from_loaded_source_normalizer_plugin(path, &plugin)
                    }
                    Err(error) => PluginDiagnosticRecord::load_failed(path, error),
                }
            })
            .collect();
        Self { records }
    }

    pub fn from_records(records: Vec<PluginDiagnosticRecord>) -> Self {
        Self { records }
    }

    pub fn records(&self) -> &[PluginDiagnosticRecord] {
        &self.records
    }

    pub fn best_decoder_for(
        &self,
        request: &DecoderPluginMatchRequest,
    ) -> Option<&PluginDiagnosticRecord> {
        self.records.iter().find(|record| {
            record.status == PluginDiagnosticStatus::DecoderSupported
                && decoder_capability_summary(record).is_some_and(|capabilities| {
                    capabilities.typed_codecs.iter().any(|codec| {
                        codec.media_kind == request.media_kind
                            && codec.codec.eq_ignore_ascii_case(&request.codec)
                    })
                })
        })
    }

    pub fn best_native_decoder_for(
        &self,
        request: &DecoderPluginMatchRequest,
    ) -> Option<&PluginDiagnosticRecord> {
        self.records.iter().find(|record| {
            record.status == PluginDiagnosticStatus::DecoderSupported
                && decoder_capability_summary(record).is_some_and(|capabilities| {
                    capabilities.supports_native_frame_output
                        && capabilities.typed_codecs.iter().any(|codec| {
                            codec.media_kind == request.media_kind
                                && codec.codec.eq_ignore_ascii_case(&request.codec)
                        })
                })
        })
    }

    pub fn supports_decoder(&self, request: &DecoderPluginMatchRequest) -> bool {
        self.best_decoder_for(request).is_some()
    }

    pub fn supports_native_decoder(&self, request: &DecoderPluginMatchRequest) -> bool {
        self.best_native_decoder_for(request).is_some()
    }

    pub fn frame_processor_supported_plugin_names(&self) -> Vec<&str> {
        self.records
            .iter()
            .filter(|record| record.status == PluginDiagnosticStatus::FrameProcessorSupported)
            .filter_map(|record| record.plugin_name.as_deref())
            .collect()
    }

    pub fn source_normalizer_supported_plugin_names(&self) -> Vec<&str> {
        self.records
            .iter()
            .filter(|record| record.status == PluginDiagnosticStatus::SourceNormalizerSupported)
            .filter_map(|record| record.plugin_name.as_deref())
            .collect()
    }

    pub fn best_source_normalizer(&self) -> Option<&PluginDiagnosticRecord> {
        self.records
            .iter()
            .find(|record| record.status == PluginDiagnosticStatus::SourceNormalizerSupported)
    }

    pub fn best_source_normalizer_packet(&self) -> Option<&PluginDiagnosticRecord> {
        self.records.iter().find(|record| {
            record.status == PluginDiagnosticStatus::SourceNormalizerSupported
                && source_normalizer_packet_capability_summary(record).is_some()
        })
    }

    pub fn best_source_normalizer_for_profile(
        &self,
        runtime_profile: &str,
    ) -> Option<&PluginDiagnosticRecord> {
        self.records.iter().find(|record| {
            record.status == PluginDiagnosticStatus::SourceNormalizerSupported
                && source_normalizer_packet_capability_summary(record).is_some_and(|capabilities| {
                    capabilities
                        .supported_runtime_profiles
                        .iter()
                        .any(|profile| profile.eq_ignore_ascii_case(runtime_profile))
                })
        })
    }

    pub fn decoder_supported_plugin_names(&self) -> Vec<&str> {
        self.records
            .iter()
            .filter(|record| record.status == PluginDiagnosticStatus::DecoderSupported)
            .filter_map(|record| record.plugin_name.as_deref())
            .collect()
    }

    pub fn diagnostic_notes(&self) -> Vec<String> {
        self.records
            .iter()
            .filter(|record| {
                !matches!(
                    record.status,
                    PluginDiagnosticStatus::DecoderSupported
                        | PluginDiagnosticStatus::FrameProcessorSupported
                        | PluginDiagnosticStatus::SourceNormalizerSupported
                )
            })
            .map(PluginDiagnosticRecord::summary)
            .collect()
    }

    pub fn report(&self) -> PluginRegistryReport {
        let mut report = PluginRegistryReport {
            total: self.records.len(),
            ..PluginRegistryReport::default()
        };

        for record in &self.records {
            match record.status {
                PluginDiagnosticStatus::Loaded => {
                    report.loaded += 1;
                    report.diagnostic_notes.push(record.summary());
                }
                PluginDiagnosticStatus::LoadFailed => {
                    report.failed += 1;
                    report.diagnostic_notes.push(record.summary());
                }
                PluginDiagnosticStatus::UnsupportedKind => {
                    report.loaded += 1;
                    report.unsupported_kind += 1;
                    report.diagnostic_notes.push(record.summary());
                }
                PluginDiagnosticStatus::DecoderSupported => {
                    report.loaded += 1;
                    report.decoder_supported += 1;
                    if report.best_supported_decoder_name.is_none() {
                        report.best_supported_decoder_name = record.plugin_name.clone();
                    }
                }
                PluginDiagnosticStatus::DecoderUnsupported => {
                    report.loaded += 1;
                    report.decoder_unsupported += 1;
                    report.diagnostic_notes.push(record.summary());
                }
                PluginDiagnosticStatus::FrameProcessorSupported => {
                    report.loaded += 1;
                    report.frame_processor_supported += 1;
                    if report.best_supported_frame_processor_name.is_none() {
                        report.best_supported_frame_processor_name = record.plugin_name.clone();
                    }
                }
                PluginDiagnosticStatus::FrameProcessorUnsupported => {
                    report.loaded += 1;
                    report.frame_processor_unsupported += 1;
                    report.diagnostic_notes.push(record.summary());
                }
                PluginDiagnosticStatus::SourceNormalizerSupported => {
                    report.loaded += 1;
                    report.source_normalizer_supported += 1;
                    if report.best_supported_source_normalizer_name.is_none() {
                        report.best_supported_source_normalizer_name = record.plugin_name.clone();
                    }
                }
                PluginDiagnosticStatus::SourceNormalizerUnsupported => {
                    report.loaded += 1;
                    report.source_normalizer_unsupported += 1;
                    report.diagnostic_notes.push(record.summary());
                }
            }
        }

        report
    }
}

#[derive(Debug)]
struct LibraryHolder {
    #[allow(dead_code)]
    library: Library,
}

type DestroyFn = unsafe extern "C" fn(context: *mut c_void);
type NameFn = unsafe extern "C" fn(context: *mut c_void) -> *const c_char;
type CapabilitiesJsonFn = unsafe extern "C" fn(context: *mut c_void) -> VesperPluginBytes;
type FreeBytesFn = unsafe extern "C" fn(context: *mut c_void, payload: VesperPluginBytes);
type ProcessJsonFn = unsafe extern "C" fn(
    context: *mut c_void,
    input_json: *const u8,
    input_json_len: usize,
    output_path: *const c_char,
    progress: VesperPluginProgressCallbacks,
) -> VesperPluginProcessResult;
type OnEventJsonFn = unsafe extern "C" fn(
    context: *mut c_void,
    event_json: *const u8,
    event_json_len: usize,
) -> bool;
type OnBenchmarkEventBatchJsonFn = unsafe extern "C" fn(
    context: *mut c_void,
    batch_json: *const u8,
    batch_json_len: usize,
) -> VesperPluginProcessResult;
type BenchmarkFlushJsonFn = unsafe extern "C" fn(context: *mut c_void) -> VesperPluginProcessResult;
type DecoderOpenSessionJsonFn = unsafe extern "C" fn(
    context: *mut c_void,
    config_json: *const u8,
    config_json_len: usize,
) -> VesperDecoderOpenSessionResult;
type DecoderSendPacketFn = unsafe extern "C" fn(
    context: *mut c_void,
    session: *mut c_void,
    packet_json: *const u8,
    packet_json_len: usize,
    packet_data: *const u8,
    packet_data_len: usize,
) -> VesperPluginProcessResult;
type DecoderReceiveNativeFrameFn = unsafe extern "C" fn(
    context: *mut c_void,
    session: *mut c_void,
) -> VesperDecoderReceiveNativeFrameResult;
type DecoderReleaseNativeFrameFn = unsafe extern "C" fn(
    context: *mut c_void,
    session: *mut c_void,
    handle_kind: u32,
    handle: usize,
) -> VesperPluginProcessResult;
type DecoderSessionOperationFn =
    unsafe extern "C" fn(context: *mut c_void, session: *mut c_void) -> VesperPluginProcessResult;
type FrameProcessorOpenSessionJsonFn =
    unsafe extern "C" fn(
        context: *mut c_void,
        config_json: *const u8,
        config_json_len: usize,
    ) -> VesperFrameProcessorOpenSessionResult;
type FrameProcessorSubmitFrameJsonFn = unsafe extern "C" fn(
    context: *mut c_void,
    session: *mut c_void,
    submit_json: *const u8,
    submit_json_len: usize,
    handle: usize,
) -> VesperPluginProcessResult;
type FrameProcessorReceiveFrameFn = unsafe extern "C" fn(
    context: *mut c_void,
    session: *mut c_void,
) -> VesperFrameProcessorReceiveFrameResult;
type FrameProcessorReleaseFrameFn = unsafe extern "C" fn(
    context: *mut c_void,
    session: *mut c_void,
    handle_kind: u32,
    handle: usize,
) -> VesperPluginProcessResult;
type FrameProcessorSessionOperationFn =
    unsafe extern "C" fn(context: *mut c_void, session: *mut c_void) -> VesperPluginProcessResult;
type SourceNormalizerSeekSessionJsonFn = unsafe extern "C" fn(
    context: *mut c_void,
    session: *mut c_void,
    seek_json: *const u8,
    seek_json_len: usize,
) -> VesperPluginProcessResult;
type SourceNormalizerSessionOperationFn =
    unsafe extern "C" fn(context: *mut c_void, session: *mut c_void) -> VesperPluginProcessResult;
type SourceNormalizerOpenPacketSessionJsonFn =
    unsafe extern "C" fn(
        context: *mut c_void,
        config_json: *const u8,
        config_json_len: usize,
    ) -> VesperSourceNormalizerOpenPacketSessionResult;
type SourceNormalizerReadPacketFn = unsafe extern "C" fn(
    context: *mut c_void,
    session: *mut c_void,
) -> VesperSourceNormalizerReadPacketResult;
type SourceNormalizerReleasePacketFn = unsafe extern "C" fn(
    context: *mut c_void,
    session: *mut c_void,
    packet_handle: usize,
) -> VesperPluginProcessResult;

#[derive(Debug, Clone, Copy)]
struct CheckedPostDownloadProcessorApi {
    context: *mut c_void,
    destroy: Option<DestroyFn>,
    name: Option<NameFn>,
    capabilities_json: CapabilitiesJsonFn,
    free_bytes: FreeBytesFn,
    process_json: ProcessJsonFn,
    assemble_json: ProcessJsonFn,
}

// SAFETY: this wrapper only stores function pointers and the opaque plugin
// context from a validated ABI table. The plugin contract requires that these
// values uphold the `Send + Sync` guarantees exposed through
// `PostDownloadProcessor`.
unsafe impl Send for CheckedPostDownloadProcessorApi {}
// SAFETY: same reasoning as above; the validated ABI table is shared behind an
// `Arc` and relies on the plugin to make the context safe for concurrent use.
unsafe impl Sync for CheckedPostDownloadProcessorApi {}

impl TryFrom<VesperPostDownloadProcessorApi> for CheckedPostDownloadProcessorApi {
    type Error = PluginLoadError;

    fn try_from(api: VesperPostDownloadProcessorApi) -> Result<Self, Self::Error> {
        Ok(Self {
            context: api.context,
            destroy: api.destroy,
            name: api.name,
            capabilities_json: api.capabilities_json.ok_or(PluginLoadError::MissingField {
                field: "post_download_processor_api.capabilities_json",
            })?,
            free_bytes: api.free_bytes.ok_or(PluginLoadError::MissingField {
                field: "post_download_processor_api.free_bytes",
            })?,
            process_json: api.process_json.ok_or(PluginLoadError::MissingField {
                field: "post_download_processor_api.process_json",
            })?,
            assemble_json: api.assemble_json.ok_or(PluginLoadError::MissingField {
                field: "post_download_processor_api.assemble_json",
            })?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct CheckedPipelineEventHookApi {
    context: *mut c_void,
    destroy: Option<DestroyFn>,
    name: Option<NameFn>,
    on_event_json: OnEventJsonFn,
}

// SAFETY: this wrapper only stores function pointers and the opaque plugin
// context from a validated ABI table. The plugin contract requires that these
// values uphold the `Send + Sync` guarantees exposed through
// `PipelineEventHook`.
unsafe impl Send for CheckedPipelineEventHookApi {}
// SAFETY: same reasoning as above; the validated ABI table is shared behind an
// `Arc` and relies on the plugin to make the context safe for concurrent use.
unsafe impl Sync for CheckedPipelineEventHookApi {}

impl TryFrom<VesperPipelineEventHookApi> for CheckedPipelineEventHookApi {
    type Error = PluginLoadError;

    fn try_from(api: VesperPipelineEventHookApi) -> Result<Self, Self::Error> {
        Ok(Self {
            context: api.context,
            destroy: api.destroy,
            name: api.name,
            on_event_json: api.on_event_json.ok_or(PluginLoadError::MissingField {
                field: "pipeline_event_hook_api.on_event_json",
            })?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct CheckedBenchmarkSinkApi {
    context: *mut c_void,
    destroy: Option<DestroyFn>,
    name: Option<NameFn>,
    free_bytes: FreeBytesFn,
    on_event_batch_json: OnBenchmarkEventBatchJsonFn,
    flush_json: Option<BenchmarkFlushJsonFn>,
}

// SAFETY: this wrapper only stores function pointers and the opaque plugin
// context from a validated ABI table. The plugin contract requires that these
// values uphold the `Send + Sync` guarantees exposed through `BenchmarkSink`.
unsafe impl Send for CheckedBenchmarkSinkApi {}
// SAFETY: same reasoning as above; the validated ABI table is shared behind an
// `Arc` and relies on the plugin to make the context safe for concurrent use.
unsafe impl Sync for CheckedBenchmarkSinkApi {}

impl TryFrom<VesperBenchmarkSinkApi> for CheckedBenchmarkSinkApi {
    type Error = PluginLoadError;

    fn try_from(api: VesperBenchmarkSinkApi) -> Result<Self, Self::Error> {
        Ok(Self {
            context: api.context,
            destroy: api.destroy,
            name: api.name,
            free_bytes: api.free_bytes.ok_or(PluginLoadError::MissingField {
                field: "benchmark_sink_api.free_bytes",
            })?,
            on_event_batch_json: api
                .on_event_batch_json
                .ok_or(PluginLoadError::MissingField {
                    field: "benchmark_sink_api.on_event_batch_json",
                })?,
            flush_json: api.flush_json,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct CheckedNativeDecoderPluginApi {
    context: *mut c_void,
    destroy: Option<DestroyFn>,
    name: Option<NameFn>,
    capabilities_json: CapabilitiesJsonFn,
    native_requirements_json: CapabilitiesJsonFn,
    free_bytes: FreeBytesFn,
    open_session_json: DecoderOpenSessionJsonFn,
    send_packet: DecoderSendPacketFn,
    receive_native_frame: DecoderReceiveNativeFrameFn,
    release_native_frame: DecoderReleaseNativeFrameFn,
    flush_session: DecoderSessionOperationFn,
    close_session: DecoderSessionOperationFn,
}

// SAFETY: this wrapper only stores function pointers and the opaque plugin
// context from a validated ABI table. The plugin contract requires that these
// values uphold the `Send + Sync` guarantees exposed through
// `NativeDecoderPluginFactory`.
unsafe impl Send for CheckedNativeDecoderPluginApi {}
// SAFETY: same reasoning as above; the validated ABI table is shared behind an
// `Arc` and relies on the plugin to make the context safe for concurrent use.
unsafe impl Sync for CheckedNativeDecoderPluginApi {}

impl TryFrom<VesperDecoderPluginApiV2> for CheckedNativeDecoderPluginApi {
    type Error = PluginLoadError;

    fn try_from(api: VesperDecoderPluginApiV2) -> Result<Self, Self::Error> {
        Ok(Self {
            context: api.context,
            destroy: api.destroy,
            name: api.name,
            capabilities_json: api.capabilities_json.ok_or(PluginLoadError::MissingField {
                field: "decoder_plugin_api_v2.capabilities_json",
            })?,
            native_requirements_json: api.native_requirements_json.ok_or(
                PluginLoadError::MissingField {
                    field: "decoder_plugin_api_v2.native_requirements_json",
                },
            )?,
            free_bytes: api.free_bytes.ok_or(PluginLoadError::MissingField {
                field: "decoder_plugin_api_v2.free_bytes",
            })?,
            open_session_json: api.open_session_json.ok_or(PluginLoadError::MissingField {
                field: "decoder_plugin_api_v2.open_session_json",
            })?,
            send_packet: api.send_packet.ok_or(PluginLoadError::MissingField {
                field: "decoder_plugin_api_v2.send_packet",
            })?,
            receive_native_frame: api.receive_native_frame.ok_or(
                PluginLoadError::MissingField {
                    field: "decoder_plugin_api_v2.receive_native_frame",
                },
            )?,
            release_native_frame: api.release_native_frame.ok_or(
                PluginLoadError::MissingField {
                    field: "decoder_plugin_api_v2.release_native_frame",
                },
            )?,
            flush_session: api.flush_session.ok_or(PluginLoadError::MissingField {
                field: "decoder_plugin_api_v2.flush_session",
            })?,
            close_session: api.close_session.ok_or(PluginLoadError::MissingField {
                field: "decoder_plugin_api_v2.close_session",
            })?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct CheckedFrameProcessorPluginApi {
    context: *mut c_void,
    destroy: Option<DestroyFn>,
    name: Option<NameFn>,
    capabilities_json: CapabilitiesJsonFn,
    free_bytes: FreeBytesFn,
    open_session_json: FrameProcessorOpenSessionJsonFn,
    submit_frame_json: FrameProcessorSubmitFrameJsonFn,
    receive_frame: FrameProcessorReceiveFrameFn,
    release_frame: FrameProcessorReleaseFrameFn,
    flush_session: FrameProcessorSessionOperationFn,
    close_session: FrameProcessorSessionOperationFn,
}

// SAFETY: this wrapper only stores function pointers and the opaque plugin
// context from a validated ABI table. The plugin contract requires that these
// values uphold the `Send + Sync` guarantees exposed through
// `FrameProcessorPluginFactory`.
unsafe impl Send for CheckedFrameProcessorPluginApi {}
// SAFETY: same reasoning as above; the validated ABI table is shared behind an
// `Arc` and relies on the plugin to make the context safe for concurrent use.
unsafe impl Sync for CheckedFrameProcessorPluginApi {}

impl TryFrom<VesperFrameProcessorPluginApiV1> for CheckedFrameProcessorPluginApi {
    type Error = PluginLoadError;

    fn try_from(api: VesperFrameProcessorPluginApiV1) -> Result<Self, Self::Error> {
        Ok(Self {
            context: api.context,
            destroy: api.destroy,
            name: api.name,
            capabilities_json: api.capabilities_json.ok_or(PluginLoadError::MissingField {
                field: "frame_processor_plugin_api_v1.capabilities_json",
            })?,
            free_bytes: api.free_bytes.ok_or(PluginLoadError::MissingField {
                field: "frame_processor_plugin_api_v1.free_bytes",
            })?,
            open_session_json: api.open_session_json.ok_or(PluginLoadError::MissingField {
                field: "frame_processor_plugin_api_v1.open_session_json",
            })?,
            submit_frame_json: api.submit_frame_json.ok_or(PluginLoadError::MissingField {
                field: "frame_processor_plugin_api_v1.submit_frame_json",
            })?,
            receive_frame: api.receive_frame.ok_or(PluginLoadError::MissingField {
                field: "frame_processor_plugin_api_v1.receive_frame",
            })?,
            release_frame: api.release_frame.ok_or(PluginLoadError::MissingField {
                field: "frame_processor_plugin_api_v1.release_frame",
            })?,
            flush_session: api.flush_session.ok_or(PluginLoadError::MissingField {
                field: "frame_processor_plugin_api_v1.flush_session",
            })?,
            close_session: api.close_session.ok_or(PluginLoadError::MissingField {
                field: "frame_processor_plugin_api_v1.close_session",
            })?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct CheckedSourceNormalizerPacketPluginApi {
    context: *mut c_void,
    destroy: Option<DestroyFn>,
    name: Option<NameFn>,
    packet_capabilities_json: CapabilitiesJsonFn,
    free_bytes: FreeBytesFn,
    open_packet_session_json: SourceNormalizerOpenPacketSessionJsonFn,
    read_packet: SourceNormalizerReadPacketFn,
    release_packet: SourceNormalizerReleasePacketFn,
    seek_packet_session_json: Option<SourceNormalizerSeekSessionJsonFn>,
    flush_packet_session: SourceNormalizerSessionOperationFn,
    close_packet_session: SourceNormalizerSessionOperationFn,
}

// SAFETY: this wrapper only stores function pointers and the opaque plugin
// context from a validated ABI table. The plugin contract requires that these
// values uphold the `Send + Sync` guarantees exposed through
// `SourceNormalizerPacketPluginFactory`.
unsafe impl Send for CheckedSourceNormalizerPacketPluginApi {}
// SAFETY: same reasoning as above; the validated ABI table is shared behind an
// `Arc` and relies on the plugin to make the context safe for concurrent use.
unsafe impl Sync for CheckedSourceNormalizerPacketPluginApi {}

impl TryFrom<VesperSourceNormalizerPluginApiV2> for CheckedSourceNormalizerPacketPluginApi {
    type Error = PluginLoadError;

    fn try_from(api: VesperSourceNormalizerPluginApiV2) -> Result<Self, Self::Error> {
        Ok(Self {
            context: api.context,
            destroy: api.destroy,
            name: api.name,
            packet_capabilities_json: api.packet_capabilities_json.ok_or(
                PluginLoadError::MissingField {
                    field: "source_normalizer_plugin_api_v2.packet_capabilities_json",
                },
            )?,
            free_bytes: api.free_bytes.ok_or(PluginLoadError::MissingField {
                field: "source_normalizer_plugin_api_v2.free_bytes",
            })?,
            open_packet_session_json: api.open_packet_session_json.ok_or(
                PluginLoadError::MissingField {
                    field: "source_normalizer_plugin_api_v2.open_packet_session_json",
                },
            )?,
            read_packet: api.read_packet.ok_or(PluginLoadError::MissingField {
                field: "source_normalizer_plugin_api_v2.read_packet",
            })?,
            release_packet: api.release_packet.ok_or(PluginLoadError::MissingField {
                field: "source_normalizer_plugin_api_v2.release_packet",
            })?,
            seek_packet_session_json: api.seek_packet_session_json,
            flush_packet_session: api.flush_packet_session.ok_or(
                PluginLoadError::MissingField {
                    field: "source_normalizer_plugin_api_v2.flush_packet_session",
                },
            )?,
            close_packet_session: api.close_packet_session.ok_or(
                PluginLoadError::MissingField {
                    field: "source_normalizer_plugin_api_v2.close_packet_session",
                },
            )?,
        })
    }
}

#[derive(Debug)]
struct DynamicPostDownloadProcessorInner {
    #[allow(dead_code)]
    library: Option<Arc<LibraryHolder>>,
    name: String,
    api: CheckedPostDownloadProcessorApi,
    capabilities: ProcessorCapabilities,
}

impl Drop for DynamicPostDownloadProcessorInner {
    fn drop(&mut self) {
        if let Some(destroy) = self.api.destroy {
            // SAFETY: `destroy` and `context` come from the validated plugin ABI
            // table and are only invoked once when this wrapper is dropped.
            unsafe { destroy(self.api.context) };
        }
    }
}

#[derive(Debug, Clone)]
struct DynamicPostDownloadProcessor {
    inner: Arc<DynamicPostDownloadProcessorInner>,
}

impl DynamicPostDownloadProcessor {
    fn new(
        library: Option<Arc<LibraryHolder>>,
        fallback_name: String,
        api: CheckedPostDownloadProcessorApi,
    ) -> Result<Self, PluginLoadError> {
        let name = if let Some(name_fn) = api.name {
            // SAFETY: the plugin ABI declares `name_fn` with `api.context`, and
            // the returned pointer is interpreted immediately as an optional
            // NUL-terminated UTF-8 string.
            let name_ptr = unsafe { name_fn(api.context) };
            if name_ptr.is_null() {
                fallback_name
            } else {
                c_string_field(name_ptr, "processor_name")?
            }
        } else {
            fallback_name
        };
        let capabilities = decode_plugin_bytes::<ProcessorCapabilities>(
            // SAFETY: the validated API guarantees `capabilities_json` and
            // `free_bytes` are present and use the shared `VesperPluginBytes`
            // ownership contract documented in `player-plugin`.
            unsafe { (api.capabilities_json)(api.context) },
            api.free_bytes,
            api.context,
        )
        .map_err(map_capabilities_payload_error)?;

        Ok(Self {
            inner: Arc::new(DynamicPostDownloadProcessorInner {
                library,
                name,
                api,
                capabilities,
            }),
        })
    }

    fn call_json_entry(
        &self,
        entry: ProcessJsonFn,
        input: &CompletedDownloadInfo,
        output_path: &Path,
        progress: &dyn ProcessorProgress,
    ) -> Result<ProcessorOutput, ProcessorError> {
        let input_json = serde_json::to_vec(input).map_err(|error| {
            ProcessorError::PayloadCodec(format!(
                "serialize dynamic plugin input for `{}` failed: {error}",
                self.inner.name
            ))
        })?;
        let output_path = CString::new(output_path.to_string_lossy().as_bytes()).map_err(|_| {
            ProcessorError::OutputPath(format!(
                "output path for plugin `{}` contains interior NUL",
                self.inner.name
            ))
        })?;

        let mut adapter = ProgressAdapter { progress };
        let callbacks = VesperPluginProgressCallbacks {
            context: (&mut adapter as *mut ProgressAdapter<'_>).cast::<c_void>(),
            on_progress: Some(progress_on_progress),
            is_cancelled: Some(progress_is_cancelled),
        };

        // SAFETY: the validated plugin API guarantees the JSON entry is present.
        // `input_json` and `output_path` live for the duration of the call, and
        // the ABI contract documents that `callbacks.context` is only valid
        // during this synchronous invocation.
        let result = unsafe {
            entry(
                self.inner.api.context,
                input_json.as_ptr(),
                input_json.len(),
                output_path.as_ptr(),
                callbacks,
            )
        };

        match result.status {
            VesperPluginResultStatus::Success => decode_plugin_bytes::<ProcessorOutput>(
                result.payload,
                self.inner.api.free_bytes,
                self.inner.api.context,
            )
            .map_err(|error| map_plugin_payload_error(&self.inner.name, "success", error)),
            VesperPluginResultStatus::Failure => decode_plugin_bytes::<ProcessorError>(
                result.payload,
                self.inner.api.free_bytes,
                self.inner.api.context,
            )
            .map_err(|error| map_plugin_payload_error(&self.inner.name, "error", error))
            .and_then(Err),
        }
    }
}

impl PostDownloadProcessor for DynamicPostDownloadProcessor {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn supported_input_formats(&self) -> &[player_plugin::ContentFormatKind] {
        &self.inner.capabilities.supported_input_formats
    }

    fn capabilities(&self) -> ProcessorCapabilities {
        self.inner.capabilities.clone()
    }

    fn process(
        &self,
        input: &CompletedDownloadInfo,
        output_path: &Path,
        progress: &dyn ProcessorProgress,
    ) -> Result<ProcessorOutput, ProcessorError> {
        self.call_json_entry(self.inner.api.process_json, input, output_path, progress)
    }

    fn assemble(
        &self,
        input: &CompletedDownloadInfo,
        output_path: &Path,
        progress: &dyn ProcessorProgress,
    ) -> Result<ProcessorOutput, ProcessorError> {
        self.call_json_entry(self.inner.api.assemble_json, input, output_path, progress)
    }
}

#[derive(Debug)]
struct DynamicPipelineEventHookInner {
    #[allow(dead_code)]
    library: Option<Arc<LibraryHolder>>,
    #[allow(dead_code)]
    name: String,
    api: CheckedPipelineEventHookApi,
}

impl Drop for DynamicPipelineEventHookInner {
    fn drop(&mut self) {
        if let Some(destroy) = self.api.destroy {
            // SAFETY: `destroy` and `context` come from the validated plugin ABI
            // table and are only invoked once when this wrapper is dropped.
            unsafe { destroy(self.api.context) };
        }
    }
}

#[derive(Debug, Clone)]
struct DynamicPipelineEventHook {
    inner: Arc<DynamicPipelineEventHookInner>,
}

impl DynamicPipelineEventHook {
    fn new(
        library: Option<Arc<LibraryHolder>>,
        fallback_name: String,
        api: CheckedPipelineEventHookApi,
    ) -> Result<Self, PluginLoadError> {
        let name = if let Some(name_fn) = api.name {
            // SAFETY: the plugin ABI declares `name_fn` with `api.context`, and
            // the returned pointer is interpreted immediately as an optional
            // NUL-terminated UTF-8 string.
            let name_ptr = unsafe { name_fn(api.context) };
            if name_ptr.is_null() {
                fallback_name
            } else {
                c_string_field(name_ptr, "hook_name")?
            }
        } else {
            fallback_name
        };

        Ok(Self {
            inner: Arc::new(DynamicPipelineEventHookInner { library, name, api }),
        })
    }
}

impl PipelineEventHook for DynamicPipelineEventHook {
    fn on_event(&self, event: &PipelineEvent) {
        let Ok(event_json) = serde_json::to_vec(event) else {
            return;
        };

        // SAFETY: the validated hook API guarantees `on_event_json` is present,
        // and `event_json` remains alive for the duration of this synchronous
        // callback.
        let _ = unsafe {
            (self.inner.api.on_event_json)(
                self.inner.api.context,
                event_json.as_ptr(),
                event_json.len(),
            )
        };
    }
}

#[derive(Debug)]
struct DynamicBenchmarkSinkInner {
    #[allow(dead_code)]
    library: Option<Arc<LibraryHolder>>,
    name: String,
    api: CheckedBenchmarkSinkApi,
}

impl Drop for DynamicBenchmarkSinkInner {
    fn drop(&mut self) {
        if let Some(destroy) = self.api.destroy {
            // SAFETY: `destroy` and `context` come from the validated plugin ABI
            // table and are only invoked once when this wrapper is dropped.
            unsafe { destroy(self.api.context) };
        }
    }
}

#[derive(Debug, Clone)]
struct DynamicBenchmarkSink {
    inner: Arc<DynamicBenchmarkSinkInner>,
}

impl DynamicBenchmarkSink {
    fn new(
        library: Option<Arc<LibraryHolder>>,
        fallback_name: String,
        api: CheckedBenchmarkSinkApi,
    ) -> Result<Self, PluginLoadError> {
        let name = if let Some(name_fn) = api.name {
            // SAFETY: the plugin ABI declares `name_fn` with `api.context`, and
            // the returned pointer is interpreted immediately as an optional
            // NUL-terminated UTF-8 string.
            let name_ptr = unsafe { name_fn(api.context) };
            if name_ptr.is_null() {
                fallback_name
            } else {
                c_string_field(name_ptr, "benchmark_sink_name")?
            }
        } else {
            fallback_name
        };

        Ok(Self {
            inner: Arc::new(DynamicBenchmarkSinkInner { library, name, api }),
        })
    }

    fn decode_result<T: DeserializeOwned>(
        &self,
        result: VesperPluginProcessResult,
        operation: &'static str,
    ) -> Result<T, BenchmarkSinkError> {
        match result.status {
            VesperPluginResultStatus::Success => decode_plugin_bytes::<T>(
                result.payload,
                self.inner.api.free_bytes,
                self.inner.api.context,
            )
            .map_err(|error| {
                BenchmarkSinkError::PayloadCodec(format!(
                    "decode benchmark sink `{}` {operation} payload failed: {error}",
                    self.inner.name
                ))
            }),
            VesperPluginResultStatus::Failure => {
                let decoded = decode_plugin_bytes::<BenchmarkSinkError>(
                    result.payload,
                    self.inner.api.free_bytes,
                    self.inner.api.context,
                )
                .unwrap_or_else(|error| {
                    BenchmarkSinkError::PayloadCodec(format!(
                        "decode benchmark sink `{}` {operation} error payload failed: {error}",
                        self.inner.name
                    ))
                });
                Err(decoded)
            }
        }
    }
}

impl BenchmarkSink for DynamicBenchmarkSink {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn on_event_batch(
        &self,
        batch: &BenchmarkEventBatch,
    ) -> Result<BenchmarkSinkStatus, BenchmarkSinkError> {
        let batch_json = serde_json::to_vec(batch).map_err(|error| {
            BenchmarkSinkError::PayloadCodec(format!(
                "serialize benchmark batch for `{}` failed: {error}",
                self.inner.name
            ))
        })?;

        // SAFETY: the validated sink API guarantees `on_event_batch_json` is
        // present, and `batch_json` remains alive for the duration of this
        // synchronous callback.
        let result = unsafe {
            (self.inner.api.on_event_batch_json)(
                self.inner.api.context,
                batch_json.as_ptr(),
                batch_json.len(),
            )
        };
        self.decode_result(result, "batch")
    }

    fn flush(&self) -> Result<BenchmarkSinkReport, BenchmarkSinkError> {
        let Some(flush_json) = self.inner.api.flush_json else {
            return Ok(BenchmarkSinkReport::default());
        };
        // SAFETY: the validated sink API declares `flush_json` with this
        // context. The callback is synchronous and returns plugin-owned bytes.
        let result = unsafe { flush_json(self.inner.api.context) };
        self.decode_result(result, "flush")
    }
}

#[derive(Debug)]
struct DynamicNativeDecoderPluginFactoryInner {
    #[allow(dead_code)]
    library: Option<Arc<LibraryHolder>>,
    name: String,
    api: CheckedNativeDecoderPluginApi,
    capabilities: DecoderCapabilities,
    native_requirements: DecoderNativeRequirements,
}

impl Drop for DynamicNativeDecoderPluginFactoryInner {
    fn drop(&mut self) {
        if let Some(destroy) = self.api.destroy {
            // SAFETY: `destroy` and `context` come from the validated plugin ABI
            // table and are only invoked once when this wrapper is dropped.
            unsafe { destroy(self.api.context) };
        }
    }
}

#[derive(Debug, Clone)]
struct DynamicNativeDecoderPluginFactory {
    inner: Arc<DynamicNativeDecoderPluginFactoryInner>,
}

impl DynamicNativeDecoderPluginFactory {
    fn new(
        library: Option<Arc<LibraryHolder>>,
        fallback_name: String,
        api: CheckedNativeDecoderPluginApi,
    ) -> Result<Self, PluginLoadError> {
        let name = if let Some(name_fn) = api.name {
            // SAFETY: the plugin ABI declares `name_fn` with `api.context`, and
            // the returned pointer is interpreted immediately as an optional
            // NUL-terminated UTF-8 string.
            let name_ptr = unsafe { name_fn(api.context) };
            if name_ptr.is_null() {
                fallback_name
            } else {
                c_string_field(name_ptr, "decoder_name")?
            }
        } else {
            fallback_name
        };
        let capabilities = decode_plugin_bytes::<DecoderCapabilities>(
            // SAFETY: the validated API guarantees `capabilities_json` and
            // `free_bytes` are present and use the shared `VesperPluginBytes`
            // ownership contract documented in `player-plugin`.
            unsafe { (api.capabilities_json)(api.context) },
            api.free_bytes,
            api.context,
        )
        .map_err(map_capabilities_payload_error)?;
        let native_requirements = decode_plugin_bytes::<DecoderNativeRequirements>(
            // SAFETY: the validated API guarantees `native_requirements_json`
            // and `free_bytes` are present and use the shared bytes ownership
            // contract documented in `player-plugin`.
            unsafe { (api.native_requirements_json)(api.context) },
            api.free_bytes,
            api.context,
        )
        .map_err(map_capabilities_payload_error)?;

        Ok(Self {
            inner: Arc::new(DynamicNativeDecoderPluginFactoryInner {
                library,
                name,
                api,
                capabilities,
                native_requirements,
            }),
        })
    }
}

impl NativeDecoderPluginFactory for DynamicNativeDecoderPluginFactory {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn capabilities(&self) -> DecoderCapabilities {
        self.inner.capabilities.clone()
    }

    fn native_requirements(&self) -> DecoderNativeRequirements {
        self.inner.native_requirements.clone()
    }

    fn open_native_session(
        &self,
        config: &DecoderSessionConfig,
    ) -> Result<Box<dyn NativeDecoderSession>, DecoderError> {
        let config_json = serde_json::to_vec(config).map_err(|error| {
            DecoderError::payload_codec(format!(
                "serialize native decoder config for `{}` failed: {error}",
                self.inner.name
            ))
        })?;

        // SAFETY: the validated plugin API guarantees `open_session_json` is
        // present, and `config_json` remains alive for the duration of this
        // synchronous callback.
        let result = unsafe {
            (self.inner.api.open_session_json)(
                self.inner.api.context,
                config_json.as_ptr(),
                config_json.len(),
            )
        };

        match result.status {
            VesperPluginResultStatus::Success => {
                if result.session.is_null() {
                    reclaim_plugin_payload(
                        result.payload,
                        self.inner.api.free_bytes,
                        self.inner.api.context,
                    );
                    return Err(DecoderError::abi_violation(format!(
                        "native decoder plugin `{}` returned a null session pointer",
                        self.inner.name
                    )));
                }
                let session_info = decode_plugin_bytes_or_default::<DecoderSessionInfo>(
                    result.payload,
                    self.inner.api.free_bytes,
                    self.inner.api.context,
                )
                .map_err(|error| {
                    map_decoder_payload_error(&self.inner.name, "open_native", error)
                })?;
                Ok(Box::new(DynamicNativeDecoderSession {
                    factory: self.inner.clone(),
                    session: result.session,
                    session_info,
                    closed: false,
                    outstanding_frames: Vec::new(),
                }))
            }
            VesperPluginResultStatus::Failure => {
                let error = decode_decoder_error_payload(
                    result.payload,
                    self.inner.api.free_bytes,
                    self.inner.api.context,
                    &self.inner.name,
                    "open_native",
                );
                Err(error)
            }
        }
    }
}

#[derive(Debug)]
struct DynamicNativeDecoderSession {
    factory: Arc<DynamicNativeDecoderPluginFactoryInner>,
    session: *mut c_void,
    session_info: DecoderSessionInfo,
    closed: bool,
    outstanding_frames: Vec<DecoderNativeFrame>,
}

// SAFETY: the dynamic native decoder session is only exposed through
// `NativeDecoderSession: Send`; the plugin ABI requires the opaque session
// pointer to be safe to move across threads when exported through this API.
unsafe impl Send for DynamicNativeDecoderSession {}

impl DynamicNativeDecoderSession {
    fn ensure_open(&self) -> Result<(), DecoderError> {
        if self.closed || self.session.is_null() {
            Err(DecoderError::NotConfigured)
        } else {
            Ok(())
        }
    }

    fn decode_operation_result(
        &self,
        result: VesperPluginProcessResult,
        operation: &'static str,
    ) -> Result<(), DecoderError> {
        match result.status {
            VesperPluginResultStatus::Success => {
                let _ = decode_plugin_bytes_or_default::<DecoderOperationStatus>(
                    result.payload,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                )
                .map_err(|error| map_decoder_payload_error(&self.factory.name, operation, error))?;
                Ok(())
            }
            VesperPluginResultStatus::Failure => Err(decode_decoder_error_payload(
                result.payload,
                self.factory.api.free_bytes,
                self.factory.api.context,
                &self.factory.name,
                operation,
            )),
        }
    }

    fn take_outstanding_native_frame(
        &mut self,
        frame: &DecoderNativeFrame,
    ) -> Result<DecoderNativeFrame, DecoderError> {
        let index = self
            .outstanding_frames
            .iter()
            .position(|candidate| candidate.handle == frame.handle)
            .ok_or_else(|| {
                DecoderError::abi_violation(format!(
                    "native decoder plugin `{}` was asked to release an untracked native frame handle",
                    self.factory.name
                ))
            })?;
        Ok(self.outstanding_frames.swap_remove(index))
    }

    fn release_tracked_native_frame(
        &mut self,
        frame: DecoderNativeFrame,
        operation: &'static str,
    ) -> Result<(), DecoderError> {
        let handle_kind =
            native_handle_kind_code(&NativeHandleKind::from(frame.metadata.handle_kind.clone()))
                .map_err(DecoderError::abi_violation)?;
        // SAFETY: the validated plugin API guarantees `release_native_frame` is
        // present. The frame handle was previously returned by this same plugin
        // session and tracked by the loader.
        let result = unsafe {
            (self.factory.api.release_native_frame)(
                self.factory.api.context,
                self.session,
                handle_kind,
                frame.handle,
            )
        };
        self.decode_operation_result(result, operation)
    }

    fn release_outstanding_native_frames(
        &mut self,
        operation: &'static str,
    ) -> Result<(), DecoderError> {
        let mut first_error = None;
        while let Some(frame) = self.outstanding_frames.pop() {
            let release_result = self.release_tracked_native_frame(frame.clone(), operation);
            if release_result.is_err() {
                self.outstanding_frames.push(frame);
            }
            if let Err(error) = release_result
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl NativeDecoderSession for DynamicNativeDecoderSession {
    fn session_info(&self) -> DecoderSessionInfo {
        self.session_info.clone()
    }

    fn send_packet(
        &mut self,
        packet: &DecoderPacket,
        data: &[u8],
    ) -> Result<DecoderPacketResult, DecoderError> {
        self.ensure_open()?;
        let packet_json = serde_json::to_vec(packet).map_err(|error| {
            DecoderError::payload_codec(format!(
                "serialize native decoder packet for `{}` failed: {error}",
                self.factory.name
            ))
        })?;
        let data_ptr = if data.is_empty() {
            std::ptr::null()
        } else {
            data.as_ptr()
        };

        // SAFETY: the validated plugin API guarantees `send_packet` is present.
        // The JSON and packet data buffers remain alive for this synchronous call.
        let result = unsafe {
            (self.factory.api.send_packet)(
                self.factory.api.context,
                self.session,
                packet_json.as_ptr(),
                packet_json.len(),
                data_ptr,
                data.len(),
            )
        };

        match result.status {
            VesperPluginResultStatus::Success => decode_plugin_bytes_or_default::<
                DecoderPacketResult,
            >(
                result.payload,
                self.factory.api.free_bytes,
                self.factory.api.context,
            )
            .map_err(|error| map_decoder_payload_error(&self.factory.name, "send_packet", error)),
            VesperPluginResultStatus::Failure => Err(decode_decoder_error_payload(
                result.payload,
                self.factory.api.free_bytes,
                self.factory.api.context,
                &self.factory.name,
                "send_packet",
            )),
        }
    }

    fn receive_native_frame(&mut self) -> Result<DecoderReceiveNativeFrameOutput, DecoderError> {
        self.ensure_open()?;
        // SAFETY: the validated plugin API guarantees `receive_native_frame` is
        // present and returns plugin-owned byte buffers reclaimed below.
        let result = unsafe {
            (self.factory.api.receive_native_frame)(self.factory.api.context, self.session)
        };

        match result.status {
            VesperPluginResultStatus::Success => {
                let metadata = decode_plugin_bytes::<DecoderReceiveNativeFrameMetadata>(
                    result.metadata,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                )
                .map_err(|error| {
                    map_decoder_payload_error(&self.factory.name, "receive_native_frame", error)
                })?;
                match metadata.status {
                    DecoderReceiveFrameStatus::Frame => {
                        if result.handle == 0 {
                            return Err(DecoderError::abi_violation(format!(
                                "native decoder plugin `{}` returned frame status with a null handle",
                                self.factory.name
                            )));
                        }
                        let frame = metadata.frame.ok_or_else(|| {
                            DecoderError::abi_violation(format!(
                                "native decoder plugin `{}` returned frame status without frame metadata",
                                self.factory.name
                            ))
                        })?;
                        let frame = DecoderNativeFrame {
                            metadata: frame,
                            handle: result.handle,
                        };
                        self.outstanding_frames.push(frame.clone());
                        Ok(DecoderReceiveNativeFrameOutput::Frame(frame))
                    }
                    DecoderReceiveFrameStatus::NeedMoreInput => {
                        Ok(DecoderReceiveNativeFrameOutput::NeedMoreInput)
                    }
                    DecoderReceiveFrameStatus::Eof => Ok(DecoderReceiveNativeFrameOutput::Eof),
                }
            }
            VesperPluginResultStatus::Failure => Err(decode_decoder_error_payload(
                result.metadata,
                self.factory.api.free_bytes,
                self.factory.api.context,
                &self.factory.name,
                "receive_native_frame",
            )),
        }
    }

    fn release_native_frame(&mut self, frame: DecoderNativeFrame) -> Result<(), DecoderError> {
        self.ensure_open()?;
        let frame = self.take_outstanding_native_frame(&frame)?;
        self.release_tracked_native_frame(frame, "release_native_frame")
    }

    fn flush(&mut self) -> Result<(), DecoderError> {
        self.ensure_open()?;
        // SAFETY: the validated plugin API guarantees `flush_session` is present.
        let result =
            unsafe { (self.factory.api.flush_session)(self.factory.api.context, self.session) };
        self.decode_operation_result(result, "flush")
    }

    fn close(&mut self) -> Result<(), DecoderError> {
        if self.closed || self.session.is_null() {
            return Ok(());
        }
        let release_result =
            self.release_outstanding_native_frames("release_native_frame_on_close");
        // SAFETY: the validated plugin API guarantees `close_session` is present
        // and consumes or releases the opaque session pointer exactly once.
        let result =
            unsafe { (self.factory.api.close_session)(self.factory.api.context, self.session) };
        self.closed = true;
        self.session = std::ptr::null_mut();
        let close_result = self.decode_operation_result(result, "close");
        release_result.and(close_result)
    }
}

impl Drop for DynamicNativeDecoderSession {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[derive(Debug)]
struct DynamicFrameProcessorPluginFactoryInner {
    #[allow(dead_code)]
    library: Option<Arc<LibraryHolder>>,
    name: String,
    api: CheckedFrameProcessorPluginApi,
    capabilities: FrameProcessorCapabilities,
}

impl Drop for DynamicFrameProcessorPluginFactoryInner {
    fn drop(&mut self) {
        if let Some(destroy) = self.api.destroy {
            // SAFETY: `destroy` and `context` come from the validated plugin ABI
            // table and are only invoked once when this wrapper is dropped.
            unsafe { destroy(self.api.context) };
        }
    }
}

#[derive(Debug, Clone)]
struct DynamicFrameProcessorPluginFactory {
    inner: Arc<DynamicFrameProcessorPluginFactoryInner>,
}

impl DynamicFrameProcessorPluginFactory {
    fn new(
        library: Option<Arc<LibraryHolder>>,
        fallback_name: String,
        api: CheckedFrameProcessorPluginApi,
    ) -> Result<Self, PluginLoadError> {
        let name = if let Some(name_fn) = api.name {
            // SAFETY: the plugin ABI declares `name_fn` with `api.context`, and
            // the returned pointer is interpreted immediately as an optional
            // NUL-terminated UTF-8 string.
            let name_ptr = unsafe { name_fn(api.context) };
            if name_ptr.is_null() {
                fallback_name
            } else {
                c_string_field(name_ptr, "frame_processor_name")?
            }
        } else {
            fallback_name
        };
        let capabilities = decode_plugin_bytes::<FrameProcessorCapabilities>(
            // SAFETY: the validated API guarantees `capabilities_json` and
            // `free_bytes` are present and use the shared `VesperPluginBytes`
            // ownership contract documented in `player-plugin`.
            unsafe { (api.capabilities_json)(api.context) },
            api.free_bytes,
            api.context,
        )
        .map_err(map_capabilities_payload_error)?;

        Ok(Self {
            inner: Arc::new(DynamicFrameProcessorPluginFactoryInner {
                library,
                name,
                api,
                capabilities,
            }),
        })
    }
}

impl FrameProcessorPluginFactory for DynamicFrameProcessorPluginFactory {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn capabilities(&self) -> FrameProcessorCapabilities {
        self.inner.capabilities.clone()
    }

    fn open_session(
        &self,
        config: &FrameProcessorSessionConfig,
    ) -> Result<Box<dyn FrameProcessorSession>, FrameProcessorError> {
        let config_json = serde_json::to_vec(config).map_err(|error| {
            FrameProcessorError::payload_codec(format!(
                "serialize frame processor config for `{}` failed: {error}",
                self.inner.name
            ))
        })?;

        // SAFETY: the validated plugin API guarantees `open_session_json` is
        // present, and `config_json` remains alive for the duration of this
        // synchronous callback.
        let result = unsafe {
            (self.inner.api.open_session_json)(
                self.inner.api.context,
                config_json.as_ptr(),
                config_json.len(),
            )
        };

        match result.status {
            VesperPluginResultStatus::Success => {
                if result.session.is_null() {
                    reclaim_plugin_payload(
                        result.payload,
                        self.inner.api.free_bytes,
                        self.inner.api.context,
                    );
                    return Err(FrameProcessorError::abi_violation(format!(
                        "frame processor plugin `{}` returned a null session pointer",
                        self.inner.name
                    )));
                }
                let session_info = decode_plugin_bytes_or_default::<FrameProcessorSessionInfo>(
                    result.payload,
                    self.inner.api.free_bytes,
                    self.inner.api.context,
                )
                .map_err(|error| {
                    map_frame_processor_payload_error(&self.inner.name, "open_session", error)
                })?;
                Ok(Box::new(DynamicFrameProcessorSession {
                    factory: self.inner.clone(),
                    session: result.session,
                    session_info,
                    closed: false,
                    outstanding_frames: Vec::new(),
                }))
            }
            VesperPluginResultStatus::Failure => {
                let error = decode_frame_processor_error_payload(
                    result.payload,
                    self.inner.api.free_bytes,
                    self.inner.api.context,
                    &self.inner.name,
                    "open_session",
                );
                Err(error)
            }
        }
    }
}

#[derive(Debug)]
struct DynamicFrameProcessorSession {
    factory: Arc<DynamicFrameProcessorPluginFactoryInner>,
    session: *mut c_void,
    session_info: FrameProcessorSessionInfo,
    closed: bool,
    outstanding_frames: Vec<NativeFrame>,
}

// SAFETY: the dynamic frame processor session is only exposed through
// `FrameProcessorSession: Send`; the plugin ABI requires the opaque session
// pointer to be safe to move across threads when exported through this API.
unsafe impl Send for DynamicFrameProcessorSession {}

impl DynamicFrameProcessorSession {
    fn ensure_open(&self) -> Result<(), FrameProcessorError> {
        if self.closed || self.session.is_null() {
            Err(FrameProcessorError::NotConfigured)
        } else {
            Ok(())
        }
    }

    fn decode_operation_result(
        &self,
        result: VesperPluginProcessResult,
        operation: &'static str,
    ) -> Result<(), FrameProcessorError> {
        match result.status {
            VesperPluginResultStatus::Success => {
                let _ = decode_plugin_bytes_or_default::<FrameProcessorOperationStatus>(
                    result.payload,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                )
                .map_err(|error| {
                    map_frame_processor_payload_error(&self.factory.name, operation, error)
                })?;
                Ok(())
            }
            VesperPluginResultStatus::Failure => Err(decode_frame_processor_error_payload(
                result.payload,
                self.factory.api.free_bytes,
                self.factory.api.context,
                &self.factory.name,
                operation,
            )),
        }
    }

    fn take_outstanding_frame(
        &mut self,
        frame: &NativeFrame,
    ) -> Result<NativeFrame, FrameProcessorError> {
        let index = self
            .outstanding_frames
            .iter()
            .position(|candidate| candidate.handle == frame.handle)
            .ok_or_else(|| {
                FrameProcessorError::abi_violation(format!(
                    "frame processor plugin `{}` was asked to release an untracked output frame handle",
                    self.factory.name
                ))
            })?;
        Ok(self.outstanding_frames.swap_remove(index))
    }

    fn release_tracked_frame(
        &mut self,
        frame: NativeFrame,
        operation: &'static str,
    ) -> Result<(), FrameProcessorError> {
        let handle_kind = native_handle_kind_code(&frame.metadata.handle_kind)
            .map_err(FrameProcessorError::abi_violation)?;
        // SAFETY: the validated plugin API guarantees `release_frame` is
        // present. The frame handle was previously returned by this same plugin
        // session and tracked by the loader.
        let result = unsafe {
            (self.factory.api.release_frame)(
                self.factory.api.context,
                self.session,
                handle_kind,
                frame.handle,
            )
        };
        self.decode_operation_result(result, operation)
    }

    fn release_outstanding_frames(
        &mut self,
        operation: &'static str,
    ) -> Result<(), FrameProcessorError> {
        let mut first_error = None;
        while let Some(frame) = self.outstanding_frames.pop() {
            let release_result = self.release_tracked_frame(frame.clone(), operation);
            if release_result.is_err() {
                self.outstanding_frames.push(frame);
            }
            if let Err(error) = release_result
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl FrameProcessorSession for DynamicFrameProcessorSession {
    fn session_info(&self) -> FrameProcessorSessionInfo {
        self.session_info.clone()
    }

    fn submit_frame(
        &mut self,
        frame: &NativeFrame,
        submit: &FrameProcessorSubmitFrame,
    ) -> Result<FrameProcessorSubmitResult, FrameProcessorError> {
        self.ensure_open()?;
        let submit_json = serde_json::to_vec(submit).map_err(|error| {
            FrameProcessorError::payload_codec(format!(
                "serialize frame processor submit payload for `{}` failed: {error}",
                self.factory.name
            ))
        })?;

        // SAFETY: the validated plugin API guarantees `submit_frame_json` is
        // present. The JSON buffer remains alive for this synchronous call, and
        // the input frame handle is borrowed only for the duration of the call.
        let result = unsafe {
            (self.factory.api.submit_frame_json)(
                self.factory.api.context,
                self.session,
                submit_json.as_ptr(),
                submit_json.len(),
                frame.handle,
            )
        };

        match result.status {
            VesperPluginResultStatus::Success => {
                decode_plugin_bytes_or_default::<FrameProcessorSubmitResult>(
                    result.payload,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                )
                .map_err(|error| {
                    map_frame_processor_payload_error(&self.factory.name, "submit_frame", error)
                })
            }
            VesperPluginResultStatus::Failure => Err(decode_frame_processor_error_payload(
                result.payload,
                self.factory.api.free_bytes,
                self.factory.api.context,
                &self.factory.name,
                "submit_frame",
            )),
        }
    }

    fn receive_frame(&mut self) -> Result<FrameProcessorReceiveOutput, FrameProcessorError> {
        self.ensure_open()?;
        // SAFETY: the validated plugin API guarantees `receive_frame` is
        // present and returns plugin-owned byte buffers reclaimed below.
        let result =
            unsafe { (self.factory.api.receive_frame)(self.factory.api.context, self.session) };

        match result.status {
            VesperPluginResultStatus::Success => {
                let metadata = decode_plugin_bytes::<FrameProcessorReceiveFrameMetadata>(
                    result.metadata,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                )
                .map_err(|error| {
                    map_frame_processor_payload_error(&self.factory.name, "receive_frame", error)
                })?;
                match metadata.status {
                    FrameProcessorReceiveStatus::Frame => {
                        if result.handle == 0 {
                            return Err(FrameProcessorError::abi_violation(format!(
                                "frame processor plugin `{}` returned frame status with a null handle",
                                self.factory.name
                            )));
                        }
                        let frame_metadata = metadata.frame.ok_or_else(|| {
                            FrameProcessorError::abi_violation(format!(
                                "frame processor plugin `{}` returned frame status without frame metadata",
                                self.factory.name
                            ))
                        })?;
                        let frame = NativeFrame {
                            metadata: frame_metadata,
                            handle: result.handle,
                        };
                        if frame_processor_output_requires_release(&frame) {
                            self.outstanding_frames.push(frame.clone());
                        }
                        Ok(FrameProcessorReceiveOutput::Frame(
                            FrameProcessorOutputFrame {
                                frame,
                                timings: metadata.timings,
                                source_frame_id: metadata.source_frame_id,
                            },
                        ))
                    }
                    FrameProcessorReceiveStatus::Pending => {
                        Ok(FrameProcessorReceiveOutput::Pending)
                    }
                    FrameProcessorReceiveStatus::EndOfStream => {
                        Ok(FrameProcessorReceiveOutput::EndOfStream)
                    }
                }
            }
            VesperPluginResultStatus::Failure => Err(decode_frame_processor_error_payload(
                result.metadata,
                self.factory.api.free_bytes,
                self.factory.api.context,
                &self.factory.name,
                "receive_frame",
            )),
        }
    }

    fn release_frame(&mut self, frame: NativeFrame) -> Result<(), FrameProcessorError> {
        self.ensure_open()?;
        let frame = self.take_outstanding_frame(&frame)?;
        self.release_tracked_frame(frame, "release_frame")
    }

    fn flush(&mut self) -> Result<(), FrameProcessorError> {
        self.ensure_open()?;
        let release_result = self.release_outstanding_frames("release_frame_on_flush");
        // SAFETY: the validated plugin API guarantees `flush_session` is present.
        let result =
            unsafe { (self.factory.api.flush_session)(self.factory.api.context, self.session) };
        let flush_result = self.decode_operation_result(result, "flush");
        release_result.and(flush_result)
    }

    fn close(&mut self) -> Result<(), FrameProcessorError> {
        if self.closed || self.session.is_null() {
            return Ok(());
        }
        let release_result = self.release_outstanding_frames("release_frame_on_close");
        // SAFETY: the validated plugin API guarantees `close_session` is present
        // and consumes or releases the opaque session pointer exactly once.
        let result =
            unsafe { (self.factory.api.close_session)(self.factory.api.context, self.session) };
        self.closed = true;
        self.session = std::ptr::null_mut();
        let close_result = self.decode_operation_result(result, "close");
        release_result.and(close_result)
    }
}

impl Drop for DynamicFrameProcessorSession {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[derive(Debug)]
struct DynamicSourceNormalizerPacketPluginFactoryInner {
    #[allow(dead_code)]
    library: Option<Arc<LibraryHolder>>,
    name: String,
    api: CheckedSourceNormalizerPacketPluginApi,
    capabilities: SourceNormalizerPacketCapabilities,
}

impl Drop for DynamicSourceNormalizerPacketPluginFactoryInner {
    fn drop(&mut self) {
        if let Some(destroy) = self.api.destroy {
            // SAFETY: `destroy` and `context` come from the validated plugin ABI
            // table and are only invoked once when this wrapper is dropped.
            unsafe { destroy(self.api.context) };
        }
    }
}

#[derive(Debug, Clone)]
struct DynamicSourceNormalizerPacketPluginFactory {
    inner: Arc<DynamicSourceNormalizerPacketPluginFactoryInner>,
}

impl DynamicSourceNormalizerPacketPluginFactory {
    fn new(
        library: Option<Arc<LibraryHolder>>,
        fallback_name: String,
        api: CheckedSourceNormalizerPacketPluginApi,
    ) -> Result<Self, PluginLoadError> {
        let name = if let Some(name_fn) = api.name {
            // SAFETY: the plugin ABI declares `name_fn` with `api.context`, and
            // the returned pointer is interpreted immediately as an optional
            // NUL-terminated UTF-8 string.
            let name_ptr = unsafe { name_fn(api.context) };
            if name_ptr.is_null() {
                fallback_name
            } else {
                c_string_field(name_ptr, "source_normalizer_packet_name")?
            }
        } else {
            fallback_name
        };
        let capabilities = decode_plugin_bytes::<SourceNormalizerPacketCapabilities>(
            // SAFETY: the validated API guarantees `packet_capabilities_json`
            // and `free_bytes` are present.
            unsafe { (api.packet_capabilities_json)(api.context) },
            api.free_bytes,
            api.context,
        )
        .map_err(map_capabilities_payload_error)?;

        Ok(Self {
            inner: Arc::new(DynamicSourceNormalizerPacketPluginFactoryInner {
                library,
                name,
                api,
                capabilities,
            }),
        })
    }
}

impl SourceNormalizerPacketPluginFactory for DynamicSourceNormalizerPacketPluginFactory {
    fn name(&self) -> &str {
        &self.inner.name
    }

    fn packet_capabilities(&self) -> SourceNormalizerPacketCapabilities {
        self.inner.capabilities.clone()
    }

    fn open_packet_session(
        &self,
        config: &SourceNormalizerPacketSessionConfig,
    ) -> Result<Box<dyn SourceNormalizerPacketSession>, SourceNormalizerError> {
        let config_json = serde_json::to_vec(config).map_err(|error| {
            SourceNormalizerError::payload_codec(format!(
                "serialize source normalizer packet config for `{}` failed: {error}",
                self.inner.name
            ))
        })?;

        // SAFETY: the validated plugin API guarantees
        // `open_packet_session_json` is present, and `config_json` remains
        // alive for the duration of this synchronous callback.
        let result = unsafe {
            (self.inner.api.open_packet_session_json)(
                self.inner.api.context,
                config_json.as_ptr(),
                config_json.len(),
            )
        };

        match result.status {
            VesperPluginResultStatus::Success => {
                if result.session.is_null() {
                    reclaim_plugin_payload(
                        result.payload,
                        self.inner.api.free_bytes,
                        self.inner.api.context,
                    );
                    return Err(SourceNormalizerError::abi_violation(format!(
                        "source normalizer packet plugin `{}` returned a null session pointer",
                        self.inner.name
                    )));
                }
                let stream_info = decode_plugin_bytes::<SourceNormalizerPacketStreamInfo>(
                    result.payload,
                    self.inner.api.free_bytes,
                    self.inner.api.context,
                )
                .map_err(|error| {
                    map_source_normalizer_payload_error(
                        &self.inner.name,
                        "open_packet_session",
                        error,
                    )
                })?;
                Ok(Box::new(DynamicSourceNormalizerPacketSession {
                    factory: self.inner.clone(),
                    session: result.session,
                    stream_info,
                    outstanding_packet: None,
                    closed: false,
                }))
            }
            VesperPluginResultStatus::Failure => Err(decode_source_normalizer_error_payload(
                result.payload,
                self.inner.api.free_bytes,
                self.inner.api.context,
                &self.inner.name,
                "open_packet_session",
            )),
        }
    }
}

#[derive(Debug)]
struct DynamicSourceNormalizerPacketSession {
    factory: Arc<DynamicSourceNormalizerPacketPluginFactoryInner>,
    session: *mut c_void,
    stream_info: SourceNormalizerPacketStreamInfo,
    outstanding_packet: Option<usize>,
    closed: bool,
}

// SAFETY: the dynamic source normalizer packet session is only exposed through
// `SourceNormalizerPacketSession: Send`; the plugin ABI requires the opaque
// session pointer to be safe to move across threads when exported through this
// API.
unsafe impl Send for DynamicSourceNormalizerPacketSession {}

impl DynamicSourceNormalizerPacketSession {
    fn ensure_open(&self) -> Result<(), SourceNormalizerError> {
        if self.closed || self.session.is_null() {
            Err(SourceNormalizerError::NotConfigured)
        } else {
            Ok(())
        }
    }

    fn decode_operation_result(
        &self,
        result: VesperPluginProcessResult,
        operation: &'static str,
    ) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        match result.status {
            VesperPluginResultStatus::Success => {
                decode_plugin_bytes_or_default::<SourceNormalizerOperationStatus>(
                    result.payload,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                )
                .map_err(|error| {
                    map_source_normalizer_payload_error(&self.factory.name, operation, error)
                })
            }
            VesperPluginResultStatus::Failure => Err(decode_source_normalizer_error_payload(
                result.payload,
                self.factory.api.free_bytes,
                self.factory.api.context,
                &self.factory.name,
                operation,
            )),
        }
    }

    fn release_outstanding_packet(
        &mut self,
        operation: &'static str,
    ) -> Result<(), SourceNormalizerError> {
        let Some(packet_handle) = self.outstanding_packet.take() else {
            return Ok(());
        };

        // SAFETY: `release_packet` is present in the validated v2 API and the
        // handle was returned by this same session from a successful read.
        let result = unsafe {
            (self.factory.api.release_packet)(self.factory.api.context, self.session, packet_handle)
        };
        self.decode_operation_result(result, operation).map(|_| ())
    }

    fn release_packet_result(
        &self,
        packet_handle: usize,
    ) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        // SAFETY: `release_packet` is present in the validated v2 API and this
        // method is only called for a handle currently tracked by this session.
        let result = unsafe {
            (self.factory.api.release_packet)(self.factory.api.context, self.session, packet_handle)
        };
        self.decode_operation_result(result, "release_packet")
    }

    fn reclaim_unexpected_packet_handle(&self, packet_handle: usize) {
        if packet_handle == 0 || self.session.is_null() {
            return;
        }
        // SAFETY: this is best-effort cleanup for an ABI-violating result that
        // still returned a plugin-owned handle.
        let result = unsafe {
            (self.factory.api.release_packet)(self.factory.api.context, self.session, packet_handle)
        };
        reclaim_plugin_payload(
            result.payload,
            self.factory.api.free_bytes,
            self.factory.api.context,
        );
    }
}

impl SourceNormalizerPacketSession for DynamicSourceNormalizerPacketSession {
    fn stream_info(&self) -> SourceNormalizerPacketStreamInfo {
        self.stream_info.clone()
    }

    fn read_packet(&mut self) -> Result<SourceNormalizerPacketLease<'_>, SourceNormalizerError> {
        self.ensure_open()?;
        if let Some(packet_handle) = self.outstanding_packet {
            return Err(SourceNormalizerError::abi_violation(format!(
                "source normalizer packet plugin `{}` still has unreleased packet handle {}",
                self.factory.name, packet_handle
            )));
        }

        // SAFETY: the validated plugin API guarantees `read_packet` is present
        // and returns metadata bytes reclaimed below. Packet bytes stay valid
        // until `release_packet` is called for the returned handle.
        let result =
            unsafe { (self.factory.api.read_packet)(self.factory.api.context, self.session) };

        match result.status {
            VesperPluginResultStatus::Success => {
                let metadata = decode_plugin_bytes::<SourceNormalizerReadPacketMetadata>(
                    result.metadata,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                )
                .map_err(|error| {
                    map_source_normalizer_payload_error(&self.factory.name, "read_packet", error)
                })?;

                if metadata.status != SourceNormalizerReadPacketStatus::Packet {
                    if !result.data.is_null() || result.data_len != 0 || result.packet_handle != 0 {
                        self.reclaim_unexpected_packet_handle(result.packet_handle);
                        return Err(SourceNormalizerError::abi_violation(format!(
                            "source normalizer packet plugin `{}` returned packet bytes for {:?}",
                            self.factory.name, metadata.status
                        )));
                    }
                    return Ok(SourceNormalizerPacketLease {
                        metadata,
                        data: &[],
                        handle: 0,
                    });
                }

                if metadata.packet.is_none() {
                    self.reclaim_unexpected_packet_handle(result.packet_handle);
                    return Err(SourceNormalizerError::abi_violation(format!(
                        "source normalizer packet plugin `{}` returned Packet status without packet metadata",
                        self.factory.name
                    )));
                }
                if result.packet_handle == 0 {
                    return Err(SourceNormalizerError::abi_violation(format!(
                        "source normalizer packet plugin `{}` returned Packet status without a packet handle",
                        self.factory.name
                    )));
                }
                if result.data.is_null() && result.data_len > 0 {
                    self.reclaim_unexpected_packet_handle(result.packet_handle);
                    return Err(SourceNormalizerError::abi_violation(format!(
                        "source normalizer packet plugin `{}` returned null packet data with len {}",
                        self.factory.name, result.data_len
                    )));
                }

                self.outstanding_packet = Some(result.packet_handle);
                let data = if result.data_len == 0 {
                    &[]
                } else {
                    // SAFETY: the plugin returned a successful packet lease. The
                    // byte range remains valid until this loader calls
                    // `release_packet` for `result.packet_handle`.
                    unsafe { std::slice::from_raw_parts(result.data, result.data_len) }
                };
                Ok(SourceNormalizerPacketLease {
                    metadata,
                    data,
                    handle: result.packet_handle,
                })
            }
            VesperPluginResultStatus::Failure => {
                if result.packet_handle != 0 {
                    self.reclaim_unexpected_packet_handle(result.packet_handle);
                }
                Err(decode_source_normalizer_error_payload(
                    result.metadata,
                    self.factory.api.free_bytes,
                    self.factory.api.context,
                    &self.factory.name,
                    "read_packet",
                ))
            }
        }
    }

    fn release_packet(&mut self, packet_handle: usize) -> Result<(), SourceNormalizerError> {
        self.ensure_open()?;
        match self.outstanding_packet {
            Some(outstanding) if outstanding == packet_handle => {
                self.release_packet_result(packet_handle)?;
                self.outstanding_packet = None;
                Ok(())
            }
            Some(outstanding) => Err(SourceNormalizerError::abi_violation(format!(
                "source normalizer packet plugin `{}` tried to release packet handle {}, but {} is outstanding",
                self.factory.name, packet_handle, outstanding
            ))),
            None => Err(SourceNormalizerError::abi_violation(format!(
                "source normalizer packet plugin `{}` has no outstanding packet handle to release",
                self.factory.name
            ))),
        }
    }

    fn seek(
        &mut self,
        seek: &SourceNormalizerPacketSeek,
    ) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        self.ensure_open()?;
        self.release_outstanding_packet("release_packet_on_seek")?;
        let Some(seek_packet_session_json) = self.factory.api.seek_packet_session_json else {
            return Err(SourceNormalizerError::unsupported_operation("seek"));
        };
        let seek_json = serde_json::to_vec(seek).map_err(|error| {
            SourceNormalizerError::payload_codec(format!(
                "serialize source normalizer packet seek for `{}` failed: {error}",
                self.factory.name
            ))
        })?;

        // SAFETY: the optional seek callback comes from the validated v2 API and
        // the JSON buffer remains alive for the synchronous call.
        let result = unsafe {
            seek_packet_session_json(
                self.factory.api.context,
                self.session,
                seek_json.as_ptr(),
                seek_json.len(),
            )
        };
        self.decode_operation_result(result, "seek_packet")
    }

    fn flush(&mut self) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
        self.ensure_open()?;
        self.release_outstanding_packet("release_packet_on_flush")?;
        // SAFETY: the validated plugin API guarantees `flush_packet_session` is
        // present for packet v2 sessions.
        let result = unsafe {
            (self.factory.api.flush_packet_session)(self.factory.api.context, self.session)
        };
        self.decode_operation_result(result, "flush_packet")
    }

    fn close(&mut self) -> Result<(), SourceNormalizerError> {
        if self.closed || self.session.is_null() {
            return Ok(());
        }
        let release_result = self.release_outstanding_packet("release_packet_on_close");
        // SAFETY: the validated plugin API guarantees `close_packet_session` is
        // present and consumes or releases the opaque session pointer exactly
        // once.
        let result = unsafe {
            (self.factory.api.close_packet_session)(self.factory.api.context, self.session)
        };
        self.closed = true;
        self.session = std::ptr::null_mut();
        let close_result = self
            .decode_operation_result(result, "close_packet")
            .map(|_| ());
        release_result.and(close_result)
    }
}

impl Drop for DynamicSourceNormalizerPacketSession {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn native_handle_kind_code(handle_kind: &NativeHandleKind) -> Result<u32, String> {
    match handle_kind {
        NativeHandleKind::CvPixelBuffer => Ok(1),
        NativeHandleKind::IoSurface => Ok(2),
        NativeHandleKind::MetalTexture => Ok(3),
        NativeHandleKind::DmaBuf => Ok(4),
        NativeHandleKind::VaapiSurface => Ok(5),
        NativeHandleKind::D3D11Texture2D => Ok(6),
        NativeHandleKind::DxgiSurface => Ok(7),
        NativeHandleKind::VulkanImage => Ok(8),
        NativeHandleKind::Unknown(kind) => Err(format!(
            "native handle kind `{kind}` cannot be released through the dynamic plugin ABI"
        )),
    }
}

fn frame_processor_output_requires_release(frame: &NativeFrame) -> bool {
    frame
        .metadata
        .release_tracking
        .as_ref()
        .is_none_or(|tracking| tracking.requires_release)
}

struct ProgressAdapter<'a> {
    progress: &'a dyn ProcessorProgress,
}

unsafe extern "C" fn progress_on_progress(context: *mut c_void, ratio: f32) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `context` is created from `ProgressAdapter` immediately before the
        // synchronous `process_json` call and remains valid until that call returns.
        let adapter = unsafe { &*(context.cast::<ProgressAdapter<'_>>()) };
        adapter.progress.on_progress(ratio);
    }));
}

unsafe extern "C" fn progress_is_cancelled(context: *mut c_void) -> bool {
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `context` is created from `ProgressAdapter` immediately before the
        // synchronous `process_json` call and remains valid until that call returns.
        let adapter = unsafe { &*(context.cast::<ProgressAdapter<'_>>()) };
        adapter.progress.is_cancelled()
    }))
    .unwrap_or(true)
}

fn c_string_field(pointer: *const c_char, field: &'static str) -> Result<String, PluginLoadError> {
    if pointer.is_null() {
        return Err(PluginLoadError::MissingField { field });
    }

    // SAFETY: `pointer` has been checked for null and the plugin ABI requires
    // all string fields to be valid NUL-terminated strings.
    let value = unsafe { CStr::from_ptr(pointer) };
    value
        .to_str()
        .map(|value| value.to_owned())
        .map_err(|_| PluginLoadError::InvalidUtf8 { field })
}

fn map_plugin_payload_error(
    plugin_name: &str,
    payload_kind: &str,
    error: PluginPayloadError,
) -> ProcessorError {
    match error {
        PluginPayloadError::NullPayloadWithLength { len } => ProcessorError::AbiViolation(format!(
            "plugin `{plugin_name}` returned {payload_kind} payload with null data pointer and len {len}"
        )),
        PluginPayloadError::Json(error) => ProcessorError::PayloadCodec(format!(
            "decode plugin `{plugin_name}` {payload_kind} payload failed: {error}"
        )),
    }
}

fn map_capabilities_payload_error(error: PluginPayloadError) -> PluginLoadError {
    match error {
        PluginPayloadError::NullPayloadWithLength { len } => {
            PluginLoadError::CapabilitiesAbiViolation(format!(
                "plugin returned capabilities payload with null data pointer and len {len}"
            ))
        }
        PluginPayloadError::Json(error) => PluginLoadError::DecodeCapabilities(error),
    }
}

fn map_decoder_payload_error(
    plugin_name: &str,
    payload_kind: &str,
    error: PluginPayloadError,
) -> DecoderError {
    match error {
        PluginPayloadError::NullPayloadWithLength { len } => DecoderError::abi_violation(format!(
            "decoder plugin `{plugin_name}` returned {payload_kind} payload with null data pointer and len {len}"
        )),
        PluginPayloadError::Json(error) => DecoderError::payload_codec(format!(
            "decode decoder plugin `{plugin_name}` {payload_kind} payload failed: {error}"
        )),
    }
}

fn decode_decoder_error_payload(
    payload: VesperPluginBytes,
    free_bytes: FreeBytesFn,
    context: *mut c_void,
    plugin_name: &str,
    payload_kind: &str,
) -> DecoderError {
    decode_plugin_bytes::<DecoderError>(payload, free_bytes, context)
        .unwrap_or_else(|error| map_decoder_payload_error(plugin_name, payload_kind, error))
}

fn map_frame_processor_payload_error(
    plugin_name: &str,
    payload_kind: &str,
    error: PluginPayloadError,
) -> FrameProcessorError {
    match error {
        PluginPayloadError::NullPayloadWithLength { len } => {
            FrameProcessorError::abi_violation(format!(
                "frame processor plugin `{plugin_name}` returned {payload_kind} payload with null data pointer and len {len}"
            ))
        }
        PluginPayloadError::Json(error) => FrameProcessorError::payload_codec(format!(
            "decode frame processor plugin `{plugin_name}` {payload_kind} payload failed: {error}"
        )),
    }
}

fn decode_frame_processor_error_payload(
    payload: VesperPluginBytes,
    free_bytes: FreeBytesFn,
    context: *mut c_void,
    plugin_name: &str,
    payload_kind: &str,
) -> FrameProcessorError {
    decode_plugin_bytes::<FrameProcessorError>(payload, free_bytes, context)
        .unwrap_or_else(|error| map_frame_processor_payload_error(plugin_name, payload_kind, error))
}

fn map_source_normalizer_payload_error(
    plugin_name: &str,
    payload_kind: &str,
    error: PluginPayloadError,
) -> SourceNormalizerError {
    match error {
        PluginPayloadError::NullPayloadWithLength { len } => {
            SourceNormalizerError::abi_violation(format!(
                "source normalizer plugin `{plugin_name}` returned {payload_kind} payload with null data pointer and len {len}"
            ))
        }
        PluginPayloadError::Json(error) => SourceNormalizerError::payload_codec(format!(
            "decode source normalizer plugin `{plugin_name}` {payload_kind} payload failed: {error}"
        )),
    }
}

fn decode_source_normalizer_error_payload(
    payload: VesperPluginBytes,
    free_bytes: FreeBytesFn,
    context: *mut c_void,
    plugin_name: &str,
    payload_kind: &str,
) -> SourceNormalizerError {
    decode_plugin_bytes::<SourceNormalizerError>(payload, free_bytes, context).unwrap_or_else(
        |error| map_source_normalizer_payload_error(plugin_name, payload_kind, error),
    )
}

fn decode_plugin_bytes_or_default<T: Default + DeserializeOwned>(
    payload: VesperPluginBytes,
    free_bytes: FreeBytesFn,
    context: *mut c_void,
) -> Result<T, PluginPayloadError> {
    if payload.data.is_null() && payload.len == 0 {
        // SAFETY: this is a no-op for the null/empty payload and keeps the
        // ownership rule symmetric for all plugin-returned buffers.
        unsafe { free_bytes(context, payload) };
        return Ok(T::default());
    }
    decode_plugin_bytes(payload, free_bytes, context)
}

fn reclaim_plugin_payload(
    payload: VesperPluginBytes,
    free_bytes: FreeBytesFn,
    context: *mut c_void,
) {
    // SAFETY: `free_bytes` is the validated deallocator paired with this
    // payload, and the payload is intentionally discarded.
    unsafe { free_bytes(context, payload) };
}

fn decode_plugin_bytes<T: DeserializeOwned>(
    payload: VesperPluginBytes,
    free_bytes: FreeBytesFn,
    context: *mut c_void,
) -> Result<T, PluginPayloadError> {
    let payload_has_null_data = payload.data.is_null();
    let bytes = if payload_has_null_data || payload.len == 0 {
        Vec::new()
    } else {
        // SAFETY: the plugin ABI requires non-null payloads to point to
        // `payload.len` initialized bytes until `free_bytes` is called.
        let slice = unsafe { std::slice::from_raw_parts(payload.data, payload.len) };
        slice.to_vec()
    };

    // SAFETY: `free_bytes` is the validated deallocator paired with this
    // payload, and the payload is not used again after this call.
    unsafe { free_bytes(context, payload) };

    if payload_has_null_data && payload.len > 0 {
        return Err(PluginPayloadError::NullPayloadWithLength { len: payload.len });
    }

    serde_json::from_slice(&bytes).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::{
        DecoderPluginCodecSummary, DecoderPluginMatchRequest, LoadedDynamicPlugin,
        PluginCapabilitySummary, PluginDiagnosticRecord, PluginDiagnosticStatus, PluginLoadError,
        PluginRegistry,
    };
    use player_plugin::{
        AssemblyMode, BenchmarkEvent, BenchmarkEventBatch, BenchmarkSinkReport,
        BenchmarkSinkStatus, CompletedContentFormat, CompletedDownloadInfo, ContentFormatKind,
        DecoderBitstreamFormat, DecoderCapabilities, DecoderCodecCapability, DecoderError,
        DecoderFrameFormat, DecoderMediaKind, DecoderNativeDeviceContext,
        DecoderNativeDeviceContextKind, DecoderNativeFrameMetadata,
        DecoderNativeFrameReleaseTracking, DecoderNativeHandleKind, DecoderNativeRequirements,
        DecoderOperationStatus, DecoderPacket, DecoderPacketResult,
        DecoderReceiveNativeFrameMetadata, DecoderReceiveNativeFrameOutput, DecoderSessionConfig,
        DecoderSessionInfo, DownloadMetadata, FrameProcessorCapabilities, FrameProcessorError,
        FrameProcessorFrameTimings, FrameProcessorOperationStatus,
        FrameProcessorReceiveFrameMetadata, FrameProcessorReceiveOutput,
        FrameProcessorSessionConfig, FrameProcessorSessionInfo, FrameProcessorSubmitFrame,
        FrameProcessorSubmitResult, FrameProcessorSubmitStatus, NativeFrame, NativeFrameMetadata,
        NativeFrameReleaseTracking, NativeHandleKind, OutputFormat, PipelineEvent,
        ProcessorCapabilities, ProcessorError, ProcessorOutput, ProcessorProgress,
        SourceNormalizerError, SourceNormalizerNormalizeLevel, SourceNormalizerOperationStatus,
        SourceNormalizerPacket, SourceNormalizerPacketCapabilities,
        SourceNormalizerPacketMediaKind, SourceNormalizerPacketPluginFactory,
        SourceNormalizerPacketSeek, SourceNormalizerPacketSession,
        SourceNormalizerPacketSessionConfig, SourceNormalizerPacketStreamInfo,
        SourceNormalizerPacketTrackInfo, SourceNormalizerReadPacketMetadata,
        SourceNormalizerReadPacketStatus, SourceNormalizerRequiredCapabilities,
        VESPER_DECODER_PLUGIN_ABI_VERSION_V3, VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_V1,
        VESPER_PLUGIN_ABI_VERSION_V2, VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
        VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2, VesperBenchmarkSinkApi,
        VesperDecoderOpenSessionResult, VesperDecoderPluginApiV2,
        VesperDecoderReceiveNativeFrameResult, VesperFrameProcessorOpenSessionResult,
        VesperFrameProcessorPluginApiV1, VesperFrameProcessorReceiveFrameResult,
        VesperPipelineEventHookApi, VesperPluginBytes, VesperPluginDescriptor, VesperPluginKind,
        VesperPluginProcessResult, VesperPluginResultStatus, VesperPostDownloadProcessorApi,
        VesperSourceNormalizerOpenPacketSessionResult, VesperSourceNormalizerPluginApiV2,
        VesperSourceNormalizerReadPacketResult,
    };
    use std::collections::BTreeMap;
    use std::env;
    use std::ffi::{c_char, c_void};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, LazyLock, Mutex};

    static PROCESSOR_NAME: &[u8] = b"fixture-processor\0";
    static HOOK_NAME: &[u8] = b"fixture-hook\0";
    static SINK_NAME: &[u8] = b"fixture-benchmark-sink\0";
    static DECODER_NAME: &[u8] = b"fixture-decoder\0";
    static FRAME_PROCESSOR_NAME: &[u8] = b"test-frame-processor\0";
    static SOURCE_NORMALIZER_PACKET_NAME: &[u8] = b"test-source-normalizer-packet\0";
    static EVENTS: LazyLock<Mutex<Vec<PipelineEvent>>> = LazyLock::new(|| Mutex::new(Vec::new()));
    static BENCHMARK_BATCHES: LazyLock<Mutex<Vec<BenchmarkEventBatch>>> =
        LazyLock::new(|| Mutex::new(Vec::new()));
    static NATIVE_FRAME_RELEASES: LazyLock<Mutex<Vec<usize>>> =
        LazyLock::new(|| Mutex::new(Vec::new()));
    static FRAME_PROCESSOR_RELEASES: LazyLock<Mutex<Vec<usize>>> =
        LazyLock::new(|| Mutex::new(Vec::new()));
    static SOURCE_NORMALIZER_PACKET_RELEASES: LazyLock<Mutex<Vec<usize>>> =
        LazyLock::new(|| Mutex::new(Vec::new()));
    static FRAME_PROCESSOR_TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
    static SOURCE_NORMALIZER_PACKET_TEST_LOCK: LazyLock<Mutex<()>> =
        LazyLock::new(|| Mutex::new(()));

    fn frame_processor_test_guard() -> std::sync::MutexGuard<'static, ()> {
        FRAME_PROCESSOR_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn source_normalizer_packet_test_guard() -> std::sync::MutexGuard<'static, ()> {
        SOURCE_NORMALIZER_PACKET_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn reset_source_normalizer_packet_releases() {
        SOURCE_NORMALIZER_PACKET_RELEASES
            .lock()
            .map(|mut releases| releases.clear())
            .unwrap_or_default();
    }

    fn source_normalizer_packet_releases() -> Vec<usize> {
        SOURCE_NORMALIZER_PACKET_RELEASES
            .lock()
            .map(|releases| releases.clone())
            .unwrap_or_default()
    }

    fn fixture_source_normalizer_packet_factory() -> Arc<dyn SourceNormalizerPacketPluginFactory> {
        let api = fixture_source_normalizer_packet_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2,
            plugin_kind: VesperPluginKind::SourceNormalizer,
            plugin_name: SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperSourceNormalizerPluginApiV2).cast(),
        };
        let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
            .expect("load source normalizer packet plugin");
        plugin
            .source_normalizer_packet_plugin_factory()
            .expect("packet factory should be available")
    }

    fn fixture_source_normalizer_packet_session() -> Box<dyn SourceNormalizerPacketSession> {
        fixture_source_normalizer_packet_factory()
            .open_packet_session(&SourceNormalizerPacketSessionConfig {
                runtime_profile: "fixture-packet".to_owned(),
                input: "file:///tmp/input.mp4".to_owned(),
                headers: Vec::new(),
                startup_timeout_ms: None,
                session_timeout_ms: None,
                preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
            })
            .expect("open packet session")
    }

    #[derive(Default)]
    struct RecordingProgress {
        ratios: Mutex<Vec<f32>>,
    }

    impl RecordingProgress {
        fn ratios(&self) -> Vec<f32> {
            self.ratios
                .lock()
                .map(|ratios| ratios.clone())
                .unwrap_or_default()
        }
    }

    impl ProcessorProgress for RecordingProgress {
        fn on_progress(&self, ratio: f32) {
            if let Ok(mut ratios) = self.ratios.lock() {
                ratios.push(ratio);
            }
        }
    }

    #[test]
    fn dynamic_post_download_processor_adapter_round_trips_json() {
        let api = fixture_processor_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
            plugin_kind: VesperPluginKind::PostDownloadProcessor,
            plugin_name: PROCESSOR_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperPostDownloadProcessorApi).cast(),
        };

        let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load plugin");
        let processor = plugin
            .post_download_processor()
            .expect("processor should be available");
        let progress = RecordingProgress::default();
        let output = processor
            .process(
                &CompletedDownloadInfo {
                    asset_id: "asset-a".to_owned(),
                    task_id: Some("1".to_owned()),
                    content_format: CompletedContentFormat::SingleFile {
                        path: PathBuf::from("/tmp/input.mp4"),
                    },
                    metadata: DownloadMetadata::default(),
                    streams: Vec::new(),
                    assembly_mode: AssemblyMode::Single,
                },
                PathBuf::from("/tmp/output.mp4").as_path(),
                &progress,
            )
            .expect("process should succeed");

        assert_eq!(
            processor.capabilities(),
            ProcessorCapabilities {
                supported_input_formats: vec![ContentFormatKind::SingleFile],
                output_formats: vec![OutputFormat::Mp4],
                supports_cancellation: true,
                supports_assembly: false,
                supported_assembly_modes: Vec::new(),
            }
        );
        assert_eq!(
            output,
            ProcessorOutput::MuxedFile {
                path: PathBuf::from("/tmp/output.mp4"),
                format: OutputFormat::Mp4,
            }
        );
        assert_eq!(progress.ratios(), vec![0.5, 1.0]);
    }

    #[test]
    fn dynamic_post_download_processor_assembly_adapter_round_trips_json() {
        let api = fixture_processor_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
            plugin_kind: VesperPluginKind::PostDownloadProcessor,
            plugin_name: PROCESSOR_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperPostDownloadProcessorApi).cast(),
        };

        let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load plugin");
        let processor = plugin
            .post_download_processor()
            .expect("processor should be available");
        let progress = RecordingProgress::default();
        let output = processor
            .assemble(
                &CompletedDownloadInfo {
                    asset_id: "asset-a".to_owned(),
                    task_id: Some("1".to_owned()),
                    content_format: CompletedContentFormat::SingleFile {
                        path: PathBuf::from("/tmp/input.mp4"),
                    },
                    metadata: DownloadMetadata::default(),
                    streams: Vec::new(),
                    assembly_mode: AssemblyMode::Single,
                },
                PathBuf::from("/tmp/assembled.mp4").as_path(),
                &progress,
            )
            .expect("assemble should succeed");

        assert_eq!(
            output,
            ProcessorOutput::MuxedFile {
                path: PathBuf::from("/tmp/assembled.mp4"),
                format: OutputFormat::Mp4,
            }
        );
        assert_eq!(progress.ratios(), vec![0.5, 1.0]);
    }

    #[test]
    fn dynamic_post_download_processor_rejects_v2_descriptor() {
        let api = fixture_processor_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_PLUGIN_ABI_VERSION_V2,
            plugin_kind: VesperPluginKind::PostDownloadProcessor,
            plugin_name: PROCESSOR_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperPostDownloadProcessorApi).cast(),
        };

        let error = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
            .expect_err("post-download processors require ABI v3");

        assert!(matches!(
            error,
            PluginLoadError::AbiVersionMismatch {
                expected: 3,
                actual: 2
            }
        ));
    }

    #[test]
    fn dynamic_post_download_processor_rejects_missing_assembly_entry() {
        let api = VesperPostDownloadProcessorApi {
            assemble_json: None,
            ..fixture_processor_api()
        };
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
            plugin_kind: VesperPluginKind::PostDownloadProcessor,
            plugin_name: PROCESSOR_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperPostDownloadProcessorApi).cast(),
        };

        let error = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
            .expect_err("post-download ABI v3 requires assemble_json");

        assert!(matches!(
            error,
            PluginLoadError::MissingField {
                field: "post_download_processor_api.assemble_json"
            }
        ));
    }

    #[test]
    fn dynamic_pipeline_event_hook_adapter_round_trips_json() {
        if let Ok(mut events) = EVENTS.lock() {
            events.clear();
        }

        let api = fixture_hook_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_PLUGIN_ABI_VERSION_V2,
            plugin_kind: VesperPluginKind::PipelineEventHook,
            plugin_name: HOOK_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperPipelineEventHookApi).cast(),
        };

        let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load hook");
        let hook = plugin
            .pipeline_event_hook()
            .expect("event hook should be available");

        hook.on_event(&PipelineEvent::DownloadTaskCompleted {
            task_id: "42".to_owned(),
        });

        let events = EVENTS
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default();
        assert_eq!(
            events,
            vec![PipelineEvent::DownloadTaskCompleted {
                task_id: "42".to_owned(),
            }]
        );
    }

    #[test]
    fn dynamic_benchmark_sink_adapter_round_trips_json() {
        if let Ok(mut batches) = BENCHMARK_BATCHES.lock() {
            batches.clear();
        }

        let api = fixture_benchmark_sink_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_PLUGIN_ABI_VERSION_V2,
            plugin_kind: VesperPluginKind::BenchmarkSink,
            plugin_name: SINK_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperBenchmarkSinkApi).cast(),
        };

        let plugin =
            LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load benchmark sink");
        assert!(plugin.post_download_processor().is_none());
        assert!(plugin.pipeline_event_hook().is_none());

        let sink = plugin
            .benchmark_sink()
            .expect("benchmark sink should be available");
        let event = BenchmarkEvent {
            run_id: "run-1".to_owned(),
            session_id: "session-1".to_owned(),
            platform: "ios".to_owned(),
            source_protocol: Some("dash".to_owned()),
            event_name: "first_frame_rendered".to_owned(),
            timestamp_ns: 100,
            elapsed_ns: 90,
            thread: Some("main".to_owned()),
            attributes: BTreeMap::from([("width".to_owned(), "1920".to_owned())]),
        };
        let status = sink
            .on_event_batch(&BenchmarkEventBatch {
                events: vec![event.clone()],
            })
            .expect("batch should be accepted");
        let report = sink.flush().expect("flush should succeed");

        assert_eq!(sink.name(), "fixture-benchmark-sink");
        assert_eq!(status.accepted_events, 1);
        assert_eq!(
            BENCHMARK_BATCHES
                .lock()
                .map(|batches| batches.clone())
                .unwrap_or_default(),
            vec![BenchmarkEventBatch {
                events: vec![event],
            }]
        );
        assert_eq!(
            report,
            BenchmarkSinkReport {
                accepted_events: 1,
                dropped_events: 0,
                plugin_errors: Vec::new(),
            }
        );
    }

    #[test]
    fn dynamic_decoder_plugin_rejects_legacy_descriptor_abi() {
        let api = fixture_native_decoder_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: 1,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperDecoderPluginApiV2).cast(),
        };

        let error = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
            .expect_err("legacy ABI descriptors should be rejected");

        assert!(matches!(
            error,
            PluginLoadError::AbiVersionMismatch {
                expected: 3,
                actual: 1
            }
        ));
    }

    #[test]
    fn dynamic_decoder_plugin_surfaces_error_payloads() {
        let api = fixture_native_decoder_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperDecoderPluginApiV2).cast(),
        };
        let plugin =
            LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load decoder plugin");
        let factory = plugin
            .native_decoder_plugin_factory()
            .expect("decoder factory should be available");

        let error = match factory.open_native_session(&DecoderSessionConfig {
            codec: "missing-codec".to_owned(),
            media_kind: DecoderMediaKind::Video,
            ..DecoderSessionConfig::default()
        }) {
            Ok(_) => panic!("unsupported codec should fail"),
            Err(error) => error,
        };

        assert!(matches!(error, DecoderError::UnsupportedCodec { .. }));
    }

    #[test]
    fn dynamic_native_decoder_plugin_adapter_round_trips_native_frame() {
        let api = fixture_native_decoder_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperDecoderPluginApiV2).cast(),
        };

        let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
            .expect("load native decoder plugin");
        let factory = plugin
            .native_decoder_plugin_factory()
            .expect("native decoder factory should be available");
        assert_eq!(factory.name(), "fixture-decoder");
        assert!(factory.capabilities().supports_hardware_decode);
        assert!(factory.capabilities().supports_gpu_handles);

        let mut session = factory
            .open_native_session(&DecoderSessionConfig {
                codec: "fixture-video".to_owned(),
                media_kind: DecoderMediaKind::Video,
                prefer_hardware: true,
                require_cpu_output: false,
                ..DecoderSessionConfig::default()
            })
            .expect("open native decoder session");
        assert_eq!(
            session.session_info().selected_hardware_backend.as_deref(),
            Some("fixture-native")
        );

        let send = session
            .send_packet(
                &DecoderPacket {
                    pts_us: Some(2_000),
                    key_frame: true,
                    ..DecoderPacket::default()
                },
                &[9, 8, 7, 6],
            )
            .expect("send native packet");
        assert!(send.accepted);

        let frame = session
            .receive_native_frame()
            .expect("receive native frame");
        let frame = match frame {
            DecoderReceiveNativeFrameOutput::Frame(frame) => frame,
            other => panic!("expected native frame, got {other:?}"),
        };
        assert_ne!(frame.handle, 0);
        assert_eq!(frame.metadata.pts_us, Some(2_000));
        assert_eq!(
            frame.metadata.handle_kind,
            DecoderNativeHandleKind::IoSurface
        );
        session
            .release_native_frame(frame)
            .expect("release native frame");
        assert_eq!(
            session.receive_native_frame().expect("need more input"),
            DecoderReceiveNativeFrameOutput::NeedMoreInput
        );
        session.close().expect("close native session");
    }

    #[test]
    fn dynamic_native_decoder_plugin_close_releases_unreturned_native_frames() {
        let api = fixture_native_decoder_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperDecoderPluginApiV2).cast(),
        };

        let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
            .expect("load native decoder plugin");
        let factory = plugin
            .native_decoder_plugin_factory()
            .expect("native decoder factory should be available");
        let mut session = factory
            .open_native_session(&DecoderSessionConfig {
                codec: "fixture-video".to_owned(),
                media_kind: DecoderMediaKind::Video,
                prefer_hardware: true,
                require_cpu_output: false,
                ..DecoderSessionConfig::default()
            })
            .expect("open native decoder session");

        session
            .send_packet(
                &DecoderPacket {
                    pts_us: Some(3_000),
                    key_frame: true,
                    ..DecoderPacket::default()
                },
                &[1, 2, 3, 4],
            )
            .expect("send native packet");
        let frame = match session
            .receive_native_frame()
            .expect("receive native frame")
        {
            DecoderReceiveNativeFrameOutput::Frame(frame) => frame,
            other => panic!("expected native frame, got {other:?}"),
        };
        let handle = frame.handle;

        session
            .close()
            .expect("close should release outstanding frame");

        assert!(native_frame_releases().contains(&handle));
    }

    #[test]
    fn dynamic_native_decoder_plugin_rejects_duplicate_native_frame_release() {
        let api = fixture_native_decoder_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperDecoderPluginApiV2).cast(),
        };

        let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
            .expect("load native decoder plugin");
        let factory = plugin
            .native_decoder_plugin_factory()
            .expect("native decoder factory should be available");
        let mut session = factory
            .open_native_session(&DecoderSessionConfig {
                codec: "fixture-video".to_owned(),
                media_kind: DecoderMediaKind::Video,
                prefer_hardware: true,
                require_cpu_output: false,
                ..DecoderSessionConfig::default()
            })
            .expect("open native decoder session");

        session
            .send_packet(
                &DecoderPacket {
                    pts_us: Some(4_000),
                    key_frame: true,
                    ..DecoderPacket::default()
                },
                &[5, 6, 7, 8],
            )
            .expect("send native packet");
        let frame = match session
            .receive_native_frame()
            .expect("receive native frame")
        {
            DecoderReceiveNativeFrameOutput::Frame(frame) => frame,
            other => panic!("expected native frame, got {other:?}"),
        };
        let duplicate = frame.clone();

        session
            .release_native_frame(frame)
            .expect("first release should succeed");
        let error = session
            .release_native_frame(duplicate)
            .expect_err("duplicate release should be rejected before plugin callback");

        assert!(matches!(error, DecoderError::AbiViolation { .. }));
    }

    #[test]
    fn dynamic_native_decoder_plugin_exposes_native_requirements() {
        let api = fixture_native_decoder_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperDecoderPluginApiV2).cast(),
        };

        let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
            .expect("load native decoder plugin");
        let factory = plugin
            .native_decoder_plugin_factory()
            .expect("native decoder factory should be available");
        let requirements = factory.native_requirements();

        assert!(
            requirements
                .output_handle_kinds
                .contains(&DecoderNativeHandleKind::IoSurface)
        );
        assert!(!requirements.requires_native_device_context);
    }

    #[test]
    fn dynamic_native_decoder_plugin_receives_native_device_context() {
        let api = fixture_native_decoder_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperDecoderPluginApiV2).cast(),
        };

        let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
            .expect("load native decoder plugin");
        let factory = plugin
            .native_decoder_plugin_factory()
            .expect("native decoder factory should be available");

        let session = factory
            .open_native_session(&DecoderSessionConfig {
                codec: "fixture-video".to_owned(),
                media_kind: DecoderMediaKind::Video,
                prefer_hardware: true,
                require_cpu_output: false,
                native_device_context: Some(DecoderNativeDeviceContext::D3D11Device {
                    device_ptr: 42,
                }),
                ..DecoderSessionConfig::default()
            })
            .expect("open native decoder session");

        assert_eq!(
            session.session_info().selected_hardware_backend.as_deref(),
            Some("fixture-native-d3d11-device-42")
        );
    }

    #[test]
    fn dynamic_native_decoder_plugin_rejects_null_native_frame_handles() {
        let api = VesperDecoderPluginApiV2 {
            receive_native_frame: Some(fixture_decoder_receive_null_native_frame),
            ..fixture_native_decoder_api()
        };
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperDecoderPluginApiV2).cast(),
        };
        let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
            .expect("load native decoder plugin");
        let factory = plugin
            .native_decoder_plugin_factory()
            .expect("native decoder factory should be available");
        let mut session = factory
            .open_native_session(&DecoderSessionConfig {
                codec: "fixture-video".to_owned(),
                media_kind: DecoderMediaKind::Video,
                ..DecoderSessionConfig::default()
            })
            .expect("open native decoder session");
        session
            .send_packet(&DecoderPacket::default(), &[1])
            .expect("send packet");

        let error = session
            .receive_native_frame()
            .expect_err("null native frame handle should fail");
        assert!(matches!(error, DecoderError::AbiViolation { .. }));
    }

    #[test]
    fn dynamic_native_decoder_plugin_rejects_old_v2_abi_revision() {
        let api = fixture_native_decoder_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_PLUGIN_ABI_VERSION_V2,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperDecoderPluginApiV2).cast(),
        };

        let error = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
            .expect_err("old native-frame v2 ABI revision should be rejected");

        assert!(matches!(
            error,
            PluginLoadError::AbiVersionMismatch { actual: 2, .. }
        ));
    }

    #[test]
    fn plugin_registry_reports_missing_decoder_path() {
        let registry = PluginRegistry::inspect_decoder_support(
            [PathBuf::from("/tmp/missing-vesper-decoder-plugin")],
            DecoderPluginMatchRequest::video("fixture-video"),
        );

        let records = registry.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, PluginDiagnosticStatus::LoadFailed);
        assert!(
            records[0]
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("failed to open plugin library")
        );
    }

    #[test]
    fn plugin_registry_reports_non_decoder_plugin() {
        let api = fixture_processor_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
            plugin_kind: VesperPluginKind::PostDownloadProcessor,
            plugin_name: PROCESSOR_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperPostDownloadProcessorApi).cast(),
        };
        let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load plugin");
        let record = PluginDiagnosticRecord::from_loaded_plugin(
            PathBuf::from("fixture-processor"),
            &plugin,
            Some(&DecoderPluginMatchRequest::video("fixture-video")),
        );

        assert_eq!(record.status, PluginDiagnosticStatus::UnsupportedKind);
        assert_eq!(record.plugin_name.as_deref(), Some("fixture-processor"));
        assert!(
            record
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("not a decoder plugin")
        );
    }

    #[test]
    fn plugin_registry_reports_decoder_codec_match() {
        let api = fixture_native_decoder_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperDecoderPluginApiV2).cast(),
        };
        let plugin =
            LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load decoder plugin");
        let record = PluginDiagnosticRecord::from_loaded_plugin(
            PathBuf::from("fixture-decoder"),
            &plugin,
            Some(&DecoderPluginMatchRequest::video("fixture-video")),
        );

        assert_eq!(record.status, PluginDiagnosticStatus::DecoderSupported);
        assert_eq!(record.plugin_name.as_deref(), Some("fixture-decoder"));
        let Some(PluginCapabilitySummary::Decoder(capabilities)) =
            record.capability_summary.as_ref()
        else {
            panic!("expected decoder capabilities");
        };
        assert!(
            capabilities
                .codecs
                .iter()
                .any(|codec| codec == "Video:fixture-video")
        );
        assert!(
            capabilities
                .typed_codecs
                .contains(&DecoderPluginCodecSummary {
                    codec: "fixture-video".to_owned(),
                    media_kind: DecoderMediaKind::Video,
                })
        );
    }

    #[test]
    fn plugin_registry_reports_decoder_codec_mismatch() {
        let api = fixture_native_decoder_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperDecoderPluginApiV2).cast(),
        };
        let plugin =
            LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load decoder plugin");
        let record = PluginDiagnosticRecord::from_loaded_plugin(
            PathBuf::from("fixture-decoder"),
            &plugin,
            Some(&DecoderPluginMatchRequest::video("unknown-video")),
        );

        assert_eq!(record.status, PluginDiagnosticStatus::DecoderUnsupported);
        assert!(
            record
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("does not advertise")
        );
    }

    #[test]
    fn plugin_registry_report_counts_and_best_decoder_are_stable() {
        let api = fixture_native_decoder_api();
        let decoder_descriptor = VesperPluginDescriptor {
            abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperDecoderPluginApiV2).cast(),
        };
        let decoder =
            LoadedDynamicPlugin::from_descriptor(None, &decoder_descriptor).expect("load decoder");
        let processor_api = fixture_processor_api();
        let processor_descriptor = VesperPluginDescriptor {
            abi_version: VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
            plugin_kind: VesperPluginKind::PostDownloadProcessor,
            plugin_name: PROCESSOR_NAME.as_ptr().cast::<c_char>(),
            api: (&processor_api as *const VesperPostDownloadProcessorApi).cast(),
        };
        let processor = LoadedDynamicPlugin::from_descriptor(None, &processor_descriptor)
            .expect("load processor");

        let request = DecoderPluginMatchRequest::video("fixture-video");
        let registry = PluginRegistry::from_records(vec![
            PluginDiagnosticRecord::from_loaded_plugin(
                PathBuf::from("fixture-decoder-supported"),
                &decoder,
                Some(&request),
            ),
            PluginDiagnosticRecord::from_loaded_plugin(
                PathBuf::from("fixture-decoder-unsupported"),
                &decoder,
                Some(&DecoderPluginMatchRequest::video("missing-video")),
            ),
            PluginDiagnosticRecord::from_loaded_plugin(
                PathBuf::from("fixture-processor"),
                &processor,
                Some(&request),
            ),
            PluginDiagnosticRecord::load_failed(
                PathBuf::from("missing-plugin"),
                PluginLoadError::NullDescriptor,
            ),
        ]);
        let report = registry.report();

        assert!(registry.supports_decoder(&request));
        assert_eq!(
            registry
                .best_decoder_for(&request)
                .and_then(|record| record.plugin_name.as_deref()),
            Some("fixture-decoder")
        );
        assert_eq!(report.total, 4);
        assert_eq!(report.loaded, 3);
        assert_eq!(report.failed, 1);
        assert_eq!(report.decoder_supported, 1);
        assert_eq!(report.decoder_unsupported, 1);
        assert_eq!(report.unsupported_kind, 1);
        assert_eq!(
            report.best_supported_decoder_name.as_deref(),
            Some("fixture-decoder")
        );
        assert_eq!(report.diagnostic_notes.len(), 3);
        assert!(
            report.diagnostic_notes.iter().any(
                |note| note == "fixture-decoder does not advertise Video missing-video support"
            )
        );
    }

    #[test]
    fn plugin_registry_prefers_native_decoder_candidates_when_requested() {
        let native_api = fixture_native_decoder_api();
        let native_descriptor = VesperPluginDescriptor {
            abi_version: VESPER_DECODER_PLUGIN_ABI_VERSION_V3,
            plugin_kind: VesperPluginKind::Decoder,
            plugin_name: DECODER_NAME.as_ptr().cast::<c_char>(),
            api: (&native_api as *const VesperDecoderPluginApiV2).cast(),
        };
        let native_decoder = LoadedDynamicPlugin::from_descriptor(None, &native_descriptor)
            .expect("load native decoder");
        let request = DecoderPluginMatchRequest::video("fixture-video");
        let registry =
            PluginRegistry::from_records(vec![PluginDiagnosticRecord::from_loaded_plugin(
                PathBuf::from("fixture-native-decoder"),
                &native_decoder,
                Some(&request),
            )]);

        assert!(registry.supports_decoder(&request));
        assert!(registry.supports_native_decoder(&request));
        let native_record = registry
            .best_native_decoder_for(&request)
            .expect("native decoder should be selected");
        assert_eq!(native_record.path, PathBuf::from("fixture-native-decoder"));
        assert!(matches!(
            native_record.capability_summary.as_ref(),
            Some(PluginCapabilitySummary::Decoder(capabilities))
                if capabilities.supports_native_frame_output
        ));
    }

    #[test]
    fn dynamic_frame_processor_plugin_adapter_round_trips_native_frame() {
        let _guard = frame_processor_test_guard();
        if let Ok(mut releases) = FRAME_PROCESSOR_RELEASES.lock() {
            releases.clear();
        }
        let api = fixture_frame_processor_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_V1,
            plugin_kind: VesperPluginKind::FrameProcessor,
            plugin_name: FRAME_PROCESSOR_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperFrameProcessorPluginApiV1).cast(),
        };

        let plugin =
            LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load frame processor");
        assert!(plugin.native_decoder_plugin_factory().is_none());
        let factory = plugin
            .frame_processor_plugin_factory()
            .expect("frame processor factory should be available");
        assert_eq!(factory.name(), "test-frame-processor");
        assert!(factory.capabilities().supports_video_frames);

        let input = fixture_native_frame();
        let mut session = factory
            .open_session(&FrameProcessorSessionConfig {
                processor_index: 3,
                input_metadata: input.metadata.clone(),
                max_in_flight_frames: Some(1),
            })
            .expect("open frame processor session");
        assert_eq!(
            session.session_info().processor_name.as_deref(),
            Some("test-frame-processor")
        );

        let submit = session
            .submit_frame(
                &input,
                &FrameProcessorSubmitFrame {
                    metadata: input.metadata.clone(),
                    present_deadline_us: Some(100_000),
                },
            )
            .expect("submit frame");
        assert_eq!(submit.status, FrameProcessorSubmitStatus::Accepted);

        let output = match session.receive_frame().expect("receive output") {
            FrameProcessorReceiveOutput::Frame(output) => output,
            other => panic!("expected processed frame, got {other:?}"),
        };
        assert_ne!(output.frame.handle, 0);
        assert_eq!(output.frame.metadata.pts_us, input.metadata.pts_us);
        assert_eq!(output.source_frame_id, input.metadata.frame_id);
        let output_handle = output.frame.handle;
        session
            .release_frame(output.frame)
            .expect("release processor output");
        assert_eq!(
            session.receive_frame().expect("pending"),
            FrameProcessorReceiveOutput::Pending
        );
        session.close().expect("close frame processor");
        assert!(
            FRAME_PROCESSOR_RELEASES
                .lock()
                .map(|releases| releases.contains(&output_handle))
                .unwrap_or(false)
        );
    }

    #[test]
    fn dynamic_frame_processor_plugin_close_releases_unreturned_outputs() {
        let _guard = frame_processor_test_guard();
        if let Ok(mut releases) = FRAME_PROCESSOR_RELEASES.lock() {
            releases.clear();
        }
        let api = fixture_frame_processor_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_V1,
            plugin_kind: VesperPluginKind::FrameProcessor,
            plugin_name: FRAME_PROCESSOR_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperFrameProcessorPluginApiV1).cast(),
        };
        let plugin =
            LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load frame processor");
        let factory = plugin
            .frame_processor_plugin_factory()
            .expect("frame processor factory should be available");
        let input = fixture_native_frame();
        let mut session = factory
            .open_session(&FrameProcessorSessionConfig {
                processor_index: 0,
                input_metadata: input.metadata.clone(),
                max_in_flight_frames: Some(1),
            })
            .expect("open frame processor session");
        session
            .submit_frame(
                &input,
                &FrameProcessorSubmitFrame::new(input.metadata.clone()),
            )
            .expect("submit frame");
        let output = match session.receive_frame().expect("receive output") {
            FrameProcessorReceiveOutput::Frame(output) => output,
            other => panic!("expected processed frame, got {other:?}"),
        };
        let handle = output.frame.handle;

        session
            .close()
            .expect("close should release outstanding output");

        assert!(
            FRAME_PROCESSOR_RELEASES
                .lock()
                .map(|releases| releases.contains(&handle))
                .unwrap_or(false)
        );
    }

    #[test]
    fn dynamic_frame_processor_plugin_does_not_release_passthrough_outputs() {
        let _guard = frame_processor_test_guard();
        if let Ok(mut releases) = FRAME_PROCESSOR_RELEASES.lock() {
            releases.clear();
        }
        let api = fixture_frame_processor_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_V1,
            plugin_kind: VesperPluginKind::FrameProcessor,
            plugin_name: FRAME_PROCESSOR_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperFrameProcessorPluginApiV1).cast(),
        };
        let plugin =
            LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load frame processor");
        let factory = plugin
            .frame_processor_plugin_factory()
            .expect("frame processor factory should be available");
        let mut input = fixture_native_frame();
        input.metadata.release_tracking = Some(NativeFrameReleaseTracking {
            frame_id: input.metadata.frame_id,
            requires_release: false,
        });
        let mut session = factory
            .open_session(&FrameProcessorSessionConfig {
                processor_index: 0,
                input_metadata: input.metadata.clone(),
                max_in_flight_frames: Some(1),
            })
            .expect("open frame processor session");
        session
            .submit_frame(
                &input,
                &FrameProcessorSubmitFrame::new(input.metadata.clone()),
            )
            .expect("submit frame");
        let output = match session.receive_frame().expect("receive output") {
            FrameProcessorReceiveOutput::Frame(output) => output,
            other => panic!("expected processed frame, got {other:?}"),
        };

        assert_eq!(
            output
                .frame
                .metadata
                .release_tracking
                .as_ref()
                .map(|tracking| tracking.requires_release),
            Some(false)
        );
        assert!(
            session.release_frame(output.frame).is_err(),
            "loader should not track passthrough output for processor release"
        );
        session
            .close()
            .expect("close should not release passthrough output");
        assert!(
            FRAME_PROCESSOR_RELEASES
                .lock()
                .map(|releases| releases.is_empty())
                .unwrap_or(false)
        );
    }

    #[test]
    fn dynamic_frame_processor_plugin_rejects_missing_submit_entry() {
        let api = VesperFrameProcessorPluginApiV1 {
            submit_frame_json: None,
            ..fixture_frame_processor_api()
        };
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_V1,
            plugin_kind: VesperPluginKind::FrameProcessor,
            plugin_name: FRAME_PROCESSOR_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperFrameProcessorPluginApiV1).cast(),
        };

        let error = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
            .expect_err("frame processor ABI requires submit_frame_json");

        assert!(matches!(
            error,
            PluginLoadError::MissingField {
                field: "frame_processor_plugin_api_v1.submit_frame_json"
            }
        ));
    }

    #[test]
    fn dynamic_frame_processor_plugin_rejects_old_abi_revision() {
        let api = fixture_frame_processor_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_PLUGIN_ABI_VERSION_V2,
            plugin_kind: VesperPluginKind::FrameProcessor,
            plugin_name: FRAME_PROCESSOR_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperFrameProcessorPluginApiV1).cast(),
        };

        let error = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
            .expect_err("wrong frame processor ABI revision should be rejected");

        assert!(matches!(
            error,
            PluginLoadError::AbiVersionMismatch {
                expected: 1,
                actual: 2
            }
        ));
    }

    #[test]
    fn plugin_registry_reports_frame_processor_support() {
        let api = fixture_frame_processor_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_V1,
            plugin_kind: VesperPluginKind::FrameProcessor,
            plugin_name: FRAME_PROCESSOR_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperFrameProcessorPluginApiV1).cast(),
        };
        let plugin =
            LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load frame processor");
        let record = PluginDiagnosticRecord::from_loaded_frame_processor_plugin(
            PathBuf::from("test-frame-processor"),
            &plugin,
        );

        assert_eq!(
            record.status,
            PluginDiagnosticStatus::FrameProcessorSupported
        );
        assert_eq!(record.plugin_name.as_deref(), Some("test-frame-processor"));
        assert!(matches!(
            record.capability_summary,
            Some(PluginCapabilitySummary::FrameProcessor(_))
        ));

        let registry = PluginRegistry::from_records(vec![record]);
        let report = registry.report();
        assert_eq!(report.frame_processor_supported, 1);
        assert_eq!(
            report.best_supported_frame_processor_name.as_deref(),
            Some("test-frame-processor")
        );
        assert_eq!(
            registry.frame_processor_supported_plugin_names(),
            vec!["test-frame-processor"]
        );
    }

    #[test]
    fn dynamic_source_normalizer_packet_plugin_round_trips_packet_lifecycle() {
        let _guard = source_normalizer_packet_test_guard();
        reset_source_normalizer_packet_releases();
        let factory = fixture_source_normalizer_packet_factory();
        assert_eq!(factory.name(), "test-source-normalizer-packet");
        assert!(factory.packet_capabilities().supports_codec("h264"));

        let mut session = fixture_source_normalizer_packet_session();
        assert_eq!(
            session.stream_info().normalizer_name.as_deref(),
            Some("test-source-normalizer-packet")
        );

        let packet = session.read_packet().expect("read first packet");
        assert_eq!(
            packet.metadata.status,
            SourceNormalizerReadPacketStatus::Packet
        );
        assert_eq!(packet.data, &[0, 0, 1, 9]);
        let handle = packet.handle;
        drop(packet);

        assert!(
            session.read_packet().is_err(),
            "loader should require release before another read"
        );
        session.release_packet(handle).expect("release packet");
        assert_eq!(source_normalizer_packet_releases(), vec![handle]);
        assert!(
            session.release_packet(handle).is_err(),
            "double release should fail before calling the plugin again"
        );

        let eos = session.read_packet().expect("read eos");
        assert_eq!(
            eos.metadata.status,
            SourceNormalizerReadPacketStatus::EndOfStream
        );
        assert_eq!(eos.handle, 0);
        session.close().expect("close packet session");
        assert!(
            session.read_packet().is_err(),
            "read after close should report not configured"
        );
    }

    #[test]
    fn dynamic_source_normalizer_packet_plugin_seek_releases_outstanding_packet() {
        let _guard = source_normalizer_packet_test_guard();
        reset_source_normalizer_packet_releases();
        let mut session = fixture_source_normalizer_packet_session();

        let packet = session.read_packet().expect("read first packet");
        let handle = packet.handle;
        drop(packet);

        let status = session
            .seek(&SourceNormalizerPacketSeek {
                position_millis: 250,
                exact: false,
            })
            .expect("seek should release outstanding packet");
        assert!(status.completed);
        assert_eq!(source_normalizer_packet_releases(), vec![handle]);

        let packet = session.read_packet().expect("read packet after seek");
        let metadata = packet.metadata.clone();
        let handle_after_seek = packet.handle;
        drop(packet);
        let packet = metadata.packet.expect("packet metadata");
        assert_eq!(packet.pts_us, Some(250_000));
        assert!(packet.discontinuity);

        session
            .release_packet(handle_after_seek)
            .expect("release packet after seek");
    }

    #[test]
    fn dynamic_source_normalizer_packet_plugin_flush_releases_outstanding_packet() {
        let _guard = source_normalizer_packet_test_guard();
        reset_source_normalizer_packet_releases();
        let mut session = fixture_source_normalizer_packet_session();

        let packet = session.read_packet().expect("read first packet");
        let handle = packet.handle;
        drop(packet);

        let status = session
            .flush()
            .expect("flush should release outstanding packet");
        assert!(status.completed);
        assert_eq!(source_normalizer_packet_releases(), vec![handle]);

        let packet = session.read_packet().expect("read packet after flush");
        assert_eq!(
            packet.metadata.status,
            SourceNormalizerReadPacketStatus::Packet
        );
        assert_eq!(
            packet
                .metadata
                .packet
                .as_ref()
                .and_then(|packet| packet.pts_us),
            Some(1_000)
        );
        let handle_after_flush = packet.handle;
        drop(packet);

        session
            .release_packet(handle_after_flush)
            .expect("release packet after flush");
    }

    #[test]
    fn dynamic_source_normalizer_packet_plugin_drop_releases_outstanding_packet() {
        let _guard = source_normalizer_packet_test_guard();
        reset_source_normalizer_packet_releases();
        let mut session = fixture_source_normalizer_packet_session();

        let packet = session.read_packet().expect("read first packet");
        let handle = packet.handle;
        drop(packet);

        drop(session);
        assert_eq!(source_normalizer_packet_releases(), vec![handle]);
    }

    #[test]
    fn dynamic_source_normalizer_packet_plugin_rejects_missing_release_callback() {
        let api = VesperSourceNormalizerPluginApiV2 {
            release_packet: None,
            ..fixture_source_normalizer_packet_api()
        };
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2,
            plugin_kind: VesperPluginKind::SourceNormalizer,
            plugin_name: SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperSourceNormalizerPluginApiV2).cast(),
        };

        let error = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
            .expect_err("packet ABI requires release_packet");

        assert!(matches!(
            error,
            PluginLoadError::MissingField {
                field: "source_normalizer_plugin_api_v2.release_packet"
            }
        ));
    }

    #[test]
    fn plugin_registry_reports_source_normalizer_packet_v2_support() {
        let api = fixture_source_normalizer_packet_api();
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_V2,
            plugin_kind: VesperPluginKind::SourceNormalizer,
            plugin_name: SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperSourceNormalizerPluginApiV2).cast(),
        };
        let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor)
            .expect("load source normalizer packet plugin");
        let record = PluginDiagnosticRecord::from_loaded_source_normalizer_plugin(
            PathBuf::from("test-source-normalizer-packet"),
            &plugin,
        );

        assert_eq!(
            record.status,
            PluginDiagnosticStatus::SourceNormalizerSupported
        );
        assert_eq!(
            record.plugin_name.as_deref(),
            Some("test-source-normalizer-packet")
        );
        assert!(matches!(
            record.capability_summary,
            Some(PluginCapabilitySummary::SourceNormalizerPacket(_))
        ));
        assert!(
            record
                .message
                .as_deref()
                .is_some_and(|message| message.contains("source_normalizer_packet_v2"))
        );

        let registry = PluginRegistry::from_records(vec![record]);
        assert_eq!(
            registry
                .best_source_normalizer_packet()
                .and_then(|record| record.plugin_name.as_deref()),
            Some("test-source-normalizer-packet")
        );
        assert_eq!(
            registry
                .best_source_normalizer_for_profile("fixture-packet")
                .and_then(|record| record.plugin_name.as_deref()),
            Some("test-source-normalizer-packet")
        );
    }

    #[test]
    fn dynamic_post_download_processor_reports_payload_codec_errors() {
        let api = VesperPostDownloadProcessorApi {
            process_json: Some(fixture_payload_codec_process_json),
            ..fixture_processor_api()
        };
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
            plugin_kind: VesperPluginKind::PostDownloadProcessor,
            plugin_name: PROCESSOR_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperPostDownloadProcessorApi).cast(),
        };

        let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load plugin");
        let processor = plugin
            .post_download_processor()
            .expect("processor should be available");
        let error = processor
            .process(
                &CompletedDownloadInfo {
                    asset_id: "asset-a".to_owned(),
                    task_id: Some("1".to_owned()),
                    content_format: CompletedContentFormat::SingleFile {
                        path: PathBuf::from("/tmp/input.mp4"),
                    },
                    metadata: DownloadMetadata::default(),
                    streams: Vec::new(),
                    assembly_mode: AssemblyMode::Single,
                },
                Path::new("/tmp/output.mp4"),
                &RecordingProgress::default(),
            )
            .expect_err("invalid payload should fail");

        assert!(matches!(error, ProcessorError::PayloadCodec(_)));
        assert!(error.to_string().contains("success payload"));
    }

    #[test]
    fn dynamic_post_download_processor_reports_abi_violations() {
        let api = VesperPostDownloadProcessorApi {
            process_json: Some(fixture_null_payload_process_json),
            ..fixture_processor_api()
        };
        let descriptor = VesperPluginDescriptor {
            abi_version: VESPER_POST_DOWNLOAD_PLUGIN_ABI_VERSION_V3,
            plugin_kind: VesperPluginKind::PostDownloadProcessor,
            plugin_name: PROCESSOR_NAME.as_ptr().cast::<c_char>(),
            api: (&api as *const VesperPostDownloadProcessorApi).cast(),
        };

        let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load plugin");
        let processor = plugin
            .post_download_processor()
            .expect("processor should be available");
        let error = processor
            .process(
                &CompletedDownloadInfo {
                    asset_id: "asset-a".to_owned(),
                    task_id: Some("1".to_owned()),
                    content_format: CompletedContentFormat::SingleFile {
                        path: PathBuf::from("/tmp/input.mp4"),
                    },
                    metadata: DownloadMetadata::default(),
                    streams: Vec::new(),
                    assembly_mode: AssemblyMode::Single,
                },
                Path::new("/tmp/output.mp4"),
                &RecordingProgress::default(),
            )
            .expect_err("null payload pointer should fail");

        assert!(matches!(error, ProcessorError::AbiViolation(_)));
        assert!(error.to_string().contains("null data pointer"));
    }

    #[test]
    #[ignore = "requires a built player-remux-ffmpeg shared library artifact"]
    fn dynamic_loader_opens_real_vesper_remux_ffmpeg_shared_library() {
        let plugin_path = resolve_vesper_remux_ffmpeg_plugin_path().unwrap_or_else(|error| {
            panic!("failed to resolve player-remux-ffmpeg plugin path: {error}")
        });

        let plugin = LoadedDynamicPlugin::load(&plugin_path).unwrap_or_else(|error| {
            panic!(
                "failed to load player-remux-ffmpeg shared library `{}`: {error}",
                plugin_path.display()
            )
        });

        assert_eq!(plugin.plugin_name(), "player-remux-ffmpeg");
        assert!(plugin.pipeline_event_hook().is_none());

        let processor = plugin
            .post_download_processor()
            .expect("player-remux-ffmpeg should export a post-download processor");
        assert_eq!(processor.name(), "player-remux-ffmpeg");
        assert_eq!(
            processor.capabilities(),
            ProcessorCapabilities {
                supported_input_formats: vec![
                    ContentFormatKind::HlsSegments,
                    ContentFormatKind::DashSegments,
                    ContentFormatKind::FlvSegments,
                    ContentFormatKind::SingleFile,
                ],
                output_formats: vec![OutputFormat::Mp4, OutputFormat::Mkv],
                supports_cancellation: true,
                supports_assembly: true,
                supported_assembly_modes: vec![
                    AssemblyMode::SeparateAudioVideo,
                    AssemblyMode::MultiAudio,
                    AssemblyMode::WithSubtitles,
                    AssemblyMode::Generic,
                ],
            }
        );

        let progress = RecordingProgress::default();
        let output = processor
            .process(
                &CompletedDownloadInfo {
                    asset_id: "asset-a".to_owned(),
                    task_id: Some("1".to_owned()),
                    content_format: CompletedContentFormat::SingleFile {
                        path: PathBuf::from("/tmp/input.mp4"),
                    },
                    metadata: DownloadMetadata::default(),
                    streams: Vec::new(),
                    assembly_mode: AssemblyMode::Single,
                },
                Path::new("/tmp/output.mp4"),
                &progress,
            )
            .expect("single-file input should be skipped by player-remux-ffmpeg");

        assert_eq!(output, ProcessorOutput::Skipped);
        assert!(progress.ratios().is_empty());
    }

    #[test]
    #[ignore = "requires a built player-decoder-fixture shared library artifact"]
    fn dynamic_loader_opens_real_decoder_fixture_shared_library() {
        let plugin_path = resolve_decoder_fixture_plugin_path()
            .unwrap_or_else(|error| panic!("failed to resolve fixture decoder path: {error}"));

        let plugin = LoadedDynamicPlugin::load(&plugin_path).unwrap_or_else(|error| {
            panic!(
                "failed to load decoder fixture shared library `{}`: {error}",
                plugin_path.display()
            )
        });

        assert_eq!(plugin.plugin_name(), "player-decoder-fixture");
        assert!(plugin.post_download_processor().is_none());
        assert!(plugin.pipeline_event_hook().is_none());
        assert!(plugin.native_decoder_plugin_factory().is_some());
    }

    #[test]
    #[ignore = "requires a built player-decoder-fixture shared library artifact"]
    fn dynamic_loader_opens_real_decoder_fixture_shared_library_as_native_v2() {
        let plugin_path = resolve_decoder_fixture_plugin_path()
            .unwrap_or_else(|error| panic!("failed to resolve fixture decoder path: {error}"));

        let plugin = LoadedDynamicPlugin::load(&plugin_path).unwrap_or_else(|error| {
            panic!(
                "failed to load decoder fixture shared library `{}` as v2: {error}",
                plugin_path.display()
            )
        });

        assert_eq!(plugin.plugin_name(), "player-decoder-fixture");
        assert!(plugin.post_download_processor().is_none());
        assert!(plugin.pipeline_event_hook().is_none());
        let factory = plugin
            .native_decoder_plugin_factory()
            .expect("player-decoder-fixture should export a native decoder factory in v2 mode");
        assert!(factory.capabilities().supports_hardware_decode);
        assert!(factory.capabilities().supports_gpu_handles);
    }

    #[test]
    #[ignore = "requires a built player-decoder-videotoolbox shared library artifact"]
    fn dynamic_loader_opens_real_videotoolbox_decoder_shared_library() {
        let plugin_path = resolve_decoder_videotoolbox_plugin_path().unwrap_or_else(|error| {
            panic!("failed to resolve VideoToolbox decoder plugin path: {error}")
        });

        let plugin = LoadedDynamicPlugin::load(&plugin_path).unwrap_or_else(|error| {
            panic!(
                "failed to load VideoToolbox decoder shared library `{}`: {error}",
                plugin_path.display()
            )
        });

        assert_eq!(plugin.plugin_name(), "player-decoder-videotoolbox");
        let factory = plugin
            .native_decoder_plugin_factory()
            .expect("player-decoder-videotoolbox should export a native decoder factory");
        let capabilities = factory.capabilities();
        assert!(capabilities.supports_codec("H264", DecoderMediaKind::Video));
        assert!(capabilities.supports_codec("HEVC", DecoderMediaKind::Video));
        assert!(capabilities.supports_hardware_decode);
        assert!(capabilities.supports_gpu_handles);

        let session = factory
            .open_native_session(&DecoderSessionConfig {
                codec: "H264".to_owned(),
                media_kind: DecoderMediaKind::Video,
                width: Some(1920),
                height: Some(1080),
                prefer_hardware: true,
                ..DecoderSessionConfig::default()
            })
            .expect("VideoToolbox plugin should open a lazy native session");
        assert_eq!(
            session.session_info().selected_hardware_backend.as_deref(),
            Some("VideoToolbox")
        );
    }

    #[test]
    #[ignore = "requires a built player-decoder-d3d11 shared library artifact"]
    fn dynamic_loader_opens_real_d3d11_decoder_shared_library() {
        let plugin_path = resolve_decoder_d3d11_plugin_path()
            .unwrap_or_else(|error| panic!("failed to resolve D3D11 decoder plugin path: {error}"));

        let plugin = LoadedDynamicPlugin::load(&plugin_path).unwrap_or_else(|error| {
            panic!(
                "failed to load D3D11 decoder shared library `{}`: {error}",
                plugin_path.display()
            )
        });

        assert_eq!(plugin.plugin_name(), "player-decoder-d3d11");
        let factory = plugin
            .native_decoder_plugin_factory()
            .expect("player-decoder-d3d11 should export a native decoder factory");
        let capabilities = factory.capabilities();
        assert!(capabilities.supports_codec("H264", DecoderMediaKind::Video));
        assert!(capabilities.supports_hardware_decode);
        assert!(capabilities.supports_gpu_handles);

        let requirements = factory.native_requirements();
        assert!(requirements.requires_native_device_context);
        assert!(
            requirements
                .required_device_context_kinds
                .contains(&DecoderNativeDeviceContextKind::D3D11Device)
        );
        assert!(
            requirements
                .output_handle_kinds
                .contains(&DecoderNativeHandleKind::D3D11Texture2D)
        );
    }

    #[test]
    #[ignore = "requires a built player-frame-processor-diagnostic shared library artifact"]
    fn dynamic_loader_opens_real_frame_processor_diagnostic_shared_library() {
        let plugin_path =
            resolve_frame_processor_diagnostic_plugin_path().unwrap_or_else(|error| {
                panic!("failed to resolve frame processor diagnostic plugin path: {error}")
            });

        let plugin = LoadedDynamicPlugin::load(&plugin_path).unwrap_or_else(|error| {
            panic!(
                "failed to load frame processor diagnostic shared library `{}`: {error}",
                plugin_path.display()
            )
        });

        assert_eq!(plugin.plugin_name(), "player-frame-processor-diagnostic");
        assert!(plugin.post_download_processor().is_none());
        assert!(plugin.pipeline_event_hook().is_none());
        assert!(plugin.native_decoder_plugin_factory().is_none());
        let factory = plugin
            .frame_processor_plugin_factory()
            .expect("player-frame-processor-diagnostic should export a frame processor factory");
        assert_eq!(factory.name(), "player-frame-processor-diagnostic");
        assert!(factory.capabilities().supports_video_frames);
    }

    fn fixture_processor_api() -> VesperPostDownloadProcessorApi {
        VesperPostDownloadProcessorApi {
            context: std::ptr::null_mut(),
            destroy: None,
            name: Some(fixture_processor_name),
            capabilities_json: Some(fixture_processor_capabilities_json),
            free_bytes: Some(fixture_free_bytes),
            process_json: Some(fixture_processor_process_json),
            assemble_json: Some(fixture_processor_process_json),
        }
    }

    fn fixture_hook_api() -> VesperPipelineEventHookApi {
        VesperPipelineEventHookApi {
            context: std::ptr::null_mut(),
            destroy: None,
            name: Some(fixture_hook_name),
            on_event_json: Some(fixture_hook_on_event_json),
        }
    }

    fn fixture_benchmark_sink_api() -> VesperBenchmarkSinkApi {
        VesperBenchmarkSinkApi {
            context: std::ptr::null_mut(),
            destroy: None,
            name: Some(fixture_benchmark_sink_name),
            free_bytes: Some(fixture_free_bytes),
            on_event_batch_json: Some(fixture_benchmark_on_event_batch_json),
            flush_json: Some(fixture_benchmark_flush_json),
        }
    }

    fn fixture_native_decoder_api() -> VesperDecoderPluginApiV2 {
        VesperDecoderPluginApiV2 {
            context: std::ptr::null_mut(),
            destroy: None,
            name: Some(fixture_decoder_name),
            capabilities_json: Some(fixture_native_decoder_capabilities_json),
            native_requirements_json: Some(fixture_native_decoder_requirements_json),
            free_bytes: Some(fixture_free_bytes),
            open_session_json: Some(fixture_native_decoder_open_session_json),
            send_packet: Some(fixture_decoder_send_packet),
            receive_native_frame: Some(fixture_decoder_receive_native_frame),
            release_native_frame: Some(fixture_decoder_release_native_frame),
            flush_session: Some(fixture_decoder_flush_session),
            close_session: Some(fixture_decoder_close_session),
        }
    }

    fn fixture_frame_processor_api() -> VesperFrameProcessorPluginApiV1 {
        VesperFrameProcessorPluginApiV1 {
            context: std::ptr::null_mut(),
            destroy: None,
            name: Some(fixture_frame_processor_name),
            capabilities_json: Some(fixture_frame_processor_capabilities_json),
            free_bytes: Some(fixture_free_bytes),
            open_session_json: Some(fixture_frame_processor_open_session_json),
            submit_frame_json: Some(fixture_frame_processor_submit_frame_json),
            receive_frame: Some(fixture_frame_processor_receive_frame),
            release_frame: Some(fixture_frame_processor_release_frame),
            flush_session: Some(fixture_frame_processor_flush_session),
            close_session: Some(fixture_frame_processor_close_session),
        }
    }

    fn fixture_source_normalizer_packet_api() -> VesperSourceNormalizerPluginApiV2 {
        VesperSourceNormalizerPluginApiV2 {
            context: std::ptr::null_mut(),
            destroy: None,
            name: Some(fixture_source_normalizer_packet_name),
            packet_capabilities_json: Some(fixture_source_normalizer_packet_capabilities_json),
            open_packet_session_json: Some(fixture_source_normalizer_open_packet_session_json),
            read_packet: Some(fixture_source_normalizer_read_packet),
            release_packet: Some(fixture_source_normalizer_release_packet),
            seek_packet_session_json: Some(fixture_source_normalizer_seek_packet_session_json),
            flush_packet_session: Some(fixture_source_normalizer_flush_packet_session),
            close_packet_session: Some(fixture_source_normalizer_close_packet_session),
            free_bytes: Some(fixture_free_bytes),
        }
    }

    unsafe extern "C" fn fixture_processor_name(_context: *mut c_void) -> *const c_char {
        PROCESSOR_NAME.as_ptr().cast::<c_char>()
    }

    unsafe extern "C" fn fixture_hook_name(_context: *mut c_void) -> *const c_char {
        HOOK_NAME.as_ptr().cast::<c_char>()
    }

    unsafe extern "C" fn fixture_benchmark_sink_name(_context: *mut c_void) -> *const c_char {
        SINK_NAME.as_ptr().cast::<c_char>()
    }

    unsafe extern "C" fn fixture_decoder_name(_context: *mut c_void) -> *const c_char {
        DECODER_NAME.as_ptr().cast::<c_char>()
    }

    unsafe extern "C" fn fixture_frame_processor_name(_context: *mut c_void) -> *const c_char {
        FRAME_PROCESSOR_NAME.as_ptr().cast::<c_char>()
    }

    unsafe extern "C" fn fixture_source_normalizer_packet_name(
        _context: *mut c_void,
    ) -> *const c_char {
        SOURCE_NORMALIZER_PACKET_NAME.as_ptr().cast::<c_char>()
    }

    unsafe extern "C" fn fixture_benchmark_on_event_batch_json(
        _context: *mut c_void,
        batch_json: *const u8,
        batch_json_len: usize,
    ) -> VesperPluginProcessResult {
        let batch = decode_fixture_json::<BenchmarkEventBatch>(batch_json, batch_json_len)
            .expect("decode benchmark batch");
        let accepted_events = batch.events.len() as u64;
        if let Ok(mut batches) = BENCHMARK_BATCHES.lock() {
            batches.push(batch);
        }
        VesperPluginProcessResult {
            status: VesperPluginResultStatus::Success,
            payload: VesperPluginBytes::from_vec(
                serde_json::to_vec(&BenchmarkSinkStatus { accepted_events })
                    .expect("serialize benchmark status"),
            ),
        }
    }

    unsafe extern "C" fn fixture_benchmark_flush_json(
        _context: *mut c_void,
    ) -> VesperPluginProcessResult {
        let accepted_events = BENCHMARK_BATCHES
            .lock()
            .map(|batches| {
                batches
                    .iter()
                    .map(|batch| batch.events.len() as u64)
                    .sum::<u64>()
            })
            .unwrap_or_default();
        VesperPluginProcessResult {
            status: VesperPluginResultStatus::Success,
            payload: VesperPluginBytes::from_vec(
                serde_json::to_vec(&BenchmarkSinkReport {
                    accepted_events,
                    dropped_events: 0,
                    plugin_errors: Vec::new(),
                })
                .expect("serialize benchmark report"),
            ),
        }
    }

    unsafe extern "C" fn fixture_processor_capabilities_json(
        _context: *mut c_void,
    ) -> VesperPluginBytes {
        let capabilities = ProcessorCapabilities {
            supported_input_formats: vec![ContentFormatKind::SingleFile],
            output_formats: vec![OutputFormat::Mp4],
            supports_cancellation: true,
            supports_assembly: false,
            supported_assembly_modes: Vec::new(),
        };
        let payload = serde_json::to_vec(&capabilities).expect("serialize capabilities");
        VesperPluginBytes::from_vec(payload)
    }

    unsafe extern "C" fn fixture_processor_process_json(
        _context: *mut c_void,
        input_json: *const u8,
        input_json_len: usize,
        output_path: *const c_char,
        progress: player_plugin::VesperPluginProgressCallbacks,
    ) -> VesperPluginProcessResult {
        // SAFETY: the fixture passes a valid input buffer for the duration of
        // this synchronous callback.
        let input_json = unsafe { std::slice::from_raw_parts(input_json, input_json_len) };
        let input: CompletedDownloadInfo =
            serde_json::from_slice(input_json).expect("deserialize input");
        assert_eq!(input.asset_id, "asset-a");

        if let Some(on_progress) = progress.on_progress {
            // SAFETY: the host-side fixture keeps `progress.context` alive for
            // the duration of this synchronous call.
            unsafe { on_progress(progress.context, 0.5) };
            // SAFETY: same as above for the second progress update.
            unsafe { on_progress(progress.context, 1.0) };
        }

        // SAFETY: the fixture provides a valid NUL-terminated UTF-8 path.
        let output_path = unsafe { std::ffi::CStr::from_ptr(output_path) }
            .to_str()
            .expect("output path utf8");
        let output = ProcessorOutput::MuxedFile {
            path: PathBuf::from(output_path),
            format: OutputFormat::Mp4,
        };
        let payload = serde_json::to_vec(&output).expect("serialize output");
        VesperPluginProcessResult {
            status: VesperPluginResultStatus::Success,
            payload: VesperPluginBytes::from_vec(payload),
        }
    }

    unsafe extern "C" fn fixture_native_decoder_capabilities_json(
        _context: *mut c_void,
    ) -> VesperPluginBytes {
        let capabilities = DecoderCapabilities {
            codecs: vec![DecoderCodecCapability {
                codec: "fixture-video".to_owned(),
                media_kind: DecoderMediaKind::Video,
                profiles: vec!["baseline".to_owned()],
                output_formats: vec![DecoderFrameFormat::Nv12],
            }],
            supports_hardware_decode: true,
            supports_cpu_video_frames: false,
            supports_audio_frames: false,
            supports_gpu_handles: true,
            supports_flush: true,
            supports_drain: true,
            max_sessions: Some(1),
        };
        VesperPluginBytes::from_vec(serde_json::to_vec(&capabilities).expect("serialize caps"))
    }

    unsafe extern "C" fn fixture_native_decoder_requirements_json(
        _context: *mut c_void,
    ) -> VesperPluginBytes {
        let requirements = DecoderNativeRequirements {
            required_device_context_kinds: Vec::new(),
            output_handle_kinds: vec![DecoderNativeHandleKind::IoSurface],
            requires_native_device_context: false,
            accepted_bitstream_formats: vec![DecoderBitstreamFormat::Unknown("fixture".to_owned())],
        };
        VesperPluginBytes::from_vec(
            serde_json::to_vec(&requirements).expect("serialize native requirements"),
        )
    }

    #[derive(Debug, Default)]
    struct FixtureDecoderSession {
        last_pts_us: Option<i64>,
        pending_frame: Option<Vec<u8>>,
    }

    #[derive(Debug, Default)]
    struct FixtureFrameProcessorSession {
        pending_output: Option<NativeFrame>,
        pending_source_frame_id: Option<u64>,
    }

    struct FixtureSourceNormalizerPacketSession {
        emitted_packet: bool,
        leased_packet: Option<FixtureSourceNormalizerPacketLease>,
        last_seek: Option<u64>,
    }

    struct FixtureSourceNormalizerPacketLease {
        handle: usize,
        data: Vec<u8>,
    }

    unsafe extern "C" fn fixture_native_decoder_open_session_json(
        _context: *mut c_void,
        config_json: *const u8,
        config_json_len: usize,
    ) -> VesperDecoderOpenSessionResult {
        let config = decode_fixture_json::<DecoderSessionConfig>(config_json, config_json_len);
        let config = match config {
            Ok(config) => config,
            Err(error) => return decoder_open_error(error),
        };
        if config.codec != "fixture-video" || config.media_kind != DecoderMediaKind::Video {
            return decoder_open_error(DecoderError::UnsupportedCodec {
                codec: config.codec,
            });
        }

        let session = Box::into_raw(Box::new(FixtureDecoderSession::default()));
        let selected_hardware_backend = match config.native_device_context.as_ref() {
            Some(DecoderNativeDeviceContext::D3D11Device { device_ptr }) => {
                Some(format!("fixture-native-d3d11-device-{device_ptr}"))
            }
            _ => Some("fixture-native".to_owned()),
        };
        let info = DecoderSessionInfo {
            decoder_name: Some("fixture-decoder".to_owned()),
            selected_hardware_backend,
            output_format: Some(DecoderFrameFormat::Nv12),
        };
        VesperDecoderOpenSessionResult {
            status: VesperPluginResultStatus::Success,
            session: session.cast::<c_void>(),
            payload: VesperPluginBytes::from_vec(
                serde_json::to_vec(&info).expect("serialize info"),
            ),
        }
    }

    unsafe extern "C" fn fixture_decoder_send_packet(
        _context: *mut c_void,
        session: *mut c_void,
        packet_json: *const u8,
        packet_json_len: usize,
        packet_data: *const u8,
        packet_data_len: usize,
    ) -> VesperPluginProcessResult {
        // SAFETY: fixture tests pass the session pointer allocated by the
        // matching open-session callback for this ABI table.
        let Some(session) = (unsafe { session.cast::<FixtureDecoderSession>().as_mut() }) else {
            return decoder_process_error(DecoderError::NotConfigured);
        };
        let packet = match decode_fixture_json::<DecoderPacket>(packet_json, packet_json_len) {
            Ok(packet) => packet,
            Err(error) => return decoder_process_error(error),
        };
        let data = if packet_data.is_null() || packet_data_len == 0 {
            Vec::new()
        } else {
            // SAFETY: the host-side fixture passes a valid packet buffer for the
            // duration of this synchronous callback.
            unsafe { std::slice::from_raw_parts(packet_data, packet_data_len) }.to_vec()
        };
        session.last_pts_us = packet.pts_us;
        session.pending_frame = Some(data);
        let result = DecoderPacketResult { accepted: true };
        decoder_process_success(&result)
    }

    unsafe extern "C" fn fixture_decoder_receive_native_frame(
        _context: *mut c_void,
        session: *mut c_void,
    ) -> VesperDecoderReceiveNativeFrameResult {
        // SAFETY: fixture tests pass the session pointer allocated by the
        // matching open-session callback for this ABI table.
        let Some(session) = (unsafe { session.cast::<FixtureDecoderSession>().as_mut() }) else {
            return decoder_native_frame_error(DecoderError::NotConfigured);
        };
        let Some(data) = session.pending_frame.take() else {
            return decoder_native_frame_success(
                &DecoderReceiveNativeFrameMetadata::need_more_input(),
                0,
            );
        };
        let handle = Box::into_raw(Box::new(data)) as usize;
        let metadata = DecoderNativeFrameMetadata {
            media_kind: DecoderMediaKind::Video,
            format: DecoderFrameFormat::Nv12,
            codec: "fixture-video".to_owned(),
            pts_us: session.last_pts_us,
            duration_us: Some(33_333),
            width: 2,
            height: 2,
            coded_width: Some(2),
            coded_height: Some(2),
            visible_rect: None,
            handle_kind: DecoderNativeHandleKind::IoSurface,
            frame_id: Some(handle as u64),
            release_tracking: Some(DecoderNativeFrameReleaseTracking {
                frame_id: Some(handle as u64),
                requires_release: true,
            }),
        };
        decoder_native_frame_success(&DecoderReceiveNativeFrameMetadata::frame(metadata), handle)
    }

    unsafe extern "C" fn fixture_decoder_receive_null_native_frame(
        _context: *mut c_void,
        session: *mut c_void,
    ) -> VesperDecoderReceiveNativeFrameResult {
        // SAFETY: fixture tests pass the session pointer allocated by the
        // matching open-session callback for this ABI table.
        let Some(session) = (unsafe { session.cast::<FixtureDecoderSession>().as_mut() }) else {
            return decoder_native_frame_error(DecoderError::NotConfigured);
        };
        if session.pending_frame.take().is_none() {
            return decoder_native_frame_success(
                &DecoderReceiveNativeFrameMetadata::need_more_input(),
                0,
            );
        };
        let metadata = DecoderNativeFrameMetadata {
            media_kind: DecoderMediaKind::Video,
            format: DecoderFrameFormat::Nv12,
            codec: "fixture-video".to_owned(),
            pts_us: session.last_pts_us,
            duration_us: Some(33_333),
            width: 2,
            height: 2,
            coded_width: Some(2),
            coded_height: Some(2),
            visible_rect: None,
            handle_kind: DecoderNativeHandleKind::IoSurface,
            frame_id: None,
            release_tracking: Some(DecoderNativeFrameReleaseTracking {
                frame_id: None,
                requires_release: true,
            }),
        };
        decoder_native_frame_success(&DecoderReceiveNativeFrameMetadata::frame(metadata), 0)
    }

    unsafe extern "C" fn fixture_decoder_release_native_frame(
        _context: *mut c_void,
        _session: *mut c_void,
        handle_kind: u32,
        handle: usize,
    ) -> VesperPluginProcessResult {
        if handle_kind != 2 || handle == 0 {
            return decoder_process_error(DecoderError::abi_violation(
                "fixture native frame release received an invalid handle",
            ));
        }
        if let Ok(mut releases) = NATIVE_FRAME_RELEASES.lock() {
            releases.push(handle);
        }
        // SAFETY: the handle was allocated with `Box::into_raw` in this test
        // fixture and is released exactly once here.
        let _ = unsafe { Box::from_raw(handle as *mut Vec<u8>) };
        decoder_process_success(&DecoderOperationStatus { completed: true })
    }

    unsafe extern "C" fn fixture_decoder_flush_session(
        _context: *mut c_void,
        session: *mut c_void,
    ) -> VesperPluginProcessResult {
        // SAFETY: fixture tests pass the session pointer allocated by the
        // matching open-session callback for this ABI table.
        let Some(session) = (unsafe { session.cast::<FixtureDecoderSession>().as_mut() }) else {
            return decoder_process_error(DecoderError::NotConfigured);
        };
        session.pending_frame = None;
        decoder_process_success(&DecoderOperationStatus { completed: true })
    }

    unsafe extern "C" fn fixture_decoder_close_session(
        _context: *mut c_void,
        session: *mut c_void,
    ) -> VesperPluginProcessResult {
        if session.is_null() {
            return decoder_process_error(DecoderError::NotConfigured);
        }
        // SAFETY: the session pointer was allocated with `Box::into_raw` by
        // the matching open-session callback and close is called once.
        let _ = unsafe { Box::from_raw(session.cast::<FixtureDecoderSession>()) };
        decoder_process_success(&DecoderOperationStatus { completed: true })
    }

    unsafe extern "C" fn fixture_frame_processor_capabilities_json(
        _context: *mut c_void,
    ) -> VesperPluginBytes {
        let capabilities = FrameProcessorCapabilities {
            accepted_input_handle_kinds: vec![NativeHandleKind::IoSurface],
            output_handle_kinds: vec![NativeHandleKind::IoSurface],
            supports_video_frames: true,
            supports_in_place_passthrough: true,
            preserves_dimensions: true,
            may_change_dimensions: false,
            preserves_color_metadata: true,
            preserves_hdr_metadata: true,
            supports_flush: true,
            max_sessions: Some(1),
            max_in_flight_frames: Some(1),
        };
        VesperPluginBytes::from_vec(
            serde_json::to_vec(&capabilities).expect("serialize frame processor caps"),
        )
    }

    unsafe extern "C" fn fixture_frame_processor_open_session_json(
        _context: *mut c_void,
        config_json: *const u8,
        config_json_len: usize,
    ) -> VesperFrameProcessorOpenSessionResult {
        let config = match decode_frame_processor_fixture_json::<FrameProcessorSessionConfig>(
            config_json,
            config_json_len,
        ) {
            Ok(config) => config,
            Err(error) => return frame_processor_open_error(error),
        };
        if config.input_metadata.handle_kind != NativeHandleKind::IoSurface {
            return frame_processor_open_error(FrameProcessorError::unsupported_handle(format!(
                "{:?}",
                config.input_metadata.handle_kind
            )));
        }

        let session = Box::into_raw(Box::new(FixtureFrameProcessorSession::default()));
        let info = FrameProcessorSessionInfo {
            processor_name: Some("test-frame-processor".to_owned()),
            selected_backend: Some("fixture-native".to_owned()),
            output_handle_kind: Some(NativeHandleKind::IoSurface),
            max_in_flight_frames: Some(1),
        };
        VesperFrameProcessorOpenSessionResult {
            status: VesperPluginResultStatus::Success,
            session: session.cast::<c_void>(),
            payload: VesperPluginBytes::from_vec(
                serde_json::to_vec(&info).expect("serialize frame processor info"),
            ),
        }
    }

    unsafe extern "C" fn fixture_frame_processor_submit_frame_json(
        _context: *mut c_void,
        session: *mut c_void,
        submit_json: *const u8,
        submit_json_len: usize,
        handle: usize,
    ) -> VesperPluginProcessResult {
        let Some(session) = (unsafe { session.cast::<FixtureFrameProcessorSession>().as_mut() })
        else {
            return frame_processor_process_error(FrameProcessorError::NotConfigured);
        };
        let submit = match decode_frame_processor_fixture_json::<FrameProcessorSubmitFrame>(
            submit_json,
            submit_json_len,
        ) {
            Ok(submit) => submit,
            Err(error) => return frame_processor_process_error(error),
        };
        if handle == 0 {
            return frame_processor_process_error(FrameProcessorError::abi_violation(
                "fixture frame processor received a null input handle",
            ));
        }
        if session.pending_output.is_some() {
            return frame_processor_process_success(&FrameProcessorSubmitResult {
                status: FrameProcessorSubmitStatus::Backpressure,
                queue_depth: Some(1),
                in_flight_frames: Some(1),
                message: Some("fixture output is still pending".to_owned()),
            });
        }

        let mut output_metadata = submit.metadata.clone();
        let requires_release = submit
            .metadata
            .release_tracking
            .as_ref()
            .is_none_or(|tracking| tracking.requires_release);
        output_metadata.frame_id = if requires_release {
            Some(handle as u64 + 1)
        } else {
            submit.metadata.frame_id
        };
        output_metadata.release_tracking = Some(NativeFrameReleaseTracking {
            frame_id: output_metadata.frame_id,
            requires_release,
        });
        session.pending_source_frame_id = submit.metadata.frame_id;
        let output_handle = if requires_release {
            Box::into_raw(Box::new(vec![handle as u8])) as usize
        } else {
            handle
        };
        session.pending_output = Some(NativeFrame {
            metadata: output_metadata,
            handle: output_handle,
        });
        frame_processor_process_success(&FrameProcessorSubmitResult {
            status: FrameProcessorSubmitStatus::Accepted,
            queue_depth: Some(1),
            in_flight_frames: Some(1),
            message: None,
        })
    }

    unsafe extern "C" fn fixture_frame_processor_receive_frame(
        _context: *mut c_void,
        session: *mut c_void,
    ) -> VesperFrameProcessorReceiveFrameResult {
        let Some(session) = (unsafe { session.cast::<FixtureFrameProcessorSession>().as_mut() })
        else {
            return frame_processor_receive_error(FrameProcessorError::NotConfigured);
        };
        let Some(output) = session.pending_output.take() else {
            return frame_processor_receive_success(
                &FrameProcessorReceiveFrameMetadata::pending(),
                0,
            );
        };
        let mut metadata = FrameProcessorReceiveFrameMetadata::frame(output.metadata.clone());
        metadata.timings = FrameProcessorFrameTimings {
            queue_wait_us: Some(10),
            process_time_us: Some(20),
            submit_to_ready_us: Some(30),
        };
        metadata.source_frame_id = session.pending_source_frame_id.take();
        frame_processor_receive_success(&metadata, output.handle)
    }

    unsafe extern "C" fn fixture_frame_processor_release_frame(
        _context: *mut c_void,
        _session: *mut c_void,
        handle_kind: u32,
        handle: usize,
    ) -> VesperPluginProcessResult {
        if handle_kind != 2 || handle == 0 {
            return frame_processor_process_error(FrameProcessorError::abi_violation(
                "fixture frame processor release received an invalid handle",
            ));
        }
        if let Ok(mut releases) = FRAME_PROCESSOR_RELEASES.lock() {
            releases.push(handle);
        }
        // SAFETY: the handle was allocated with `Box::into_raw` in this test
        // fixture and is released exactly once here.
        let _ = unsafe { Box::from_raw(handle as *mut Vec<u8>) };
        frame_processor_process_success(&FrameProcessorOperationStatus { completed: true })
    }

    unsafe extern "C" fn fixture_frame_processor_flush_session(
        _context: *mut c_void,
        session: *mut c_void,
    ) -> VesperPluginProcessResult {
        let Some(session) = (unsafe { session.cast::<FixtureFrameProcessorSession>().as_mut() })
        else {
            return frame_processor_process_error(FrameProcessorError::NotConfigured);
        };
        if let Some(output) = session.pending_output.take() {
            // SAFETY: pending fixture outputs are owned by this session and can
            // be reclaimed on flush when the host never received them.
            let _ = unsafe { Box::from_raw(output.handle as *mut Vec<u8>) };
        }
        frame_processor_process_success(&FrameProcessorOperationStatus { completed: true })
    }

    unsafe extern "C" fn fixture_frame_processor_close_session(
        _context: *mut c_void,
        session: *mut c_void,
    ) -> VesperPluginProcessResult {
        if session.is_null() {
            return frame_processor_process_error(FrameProcessorError::NotConfigured);
        }
        // SAFETY: the session pointer was allocated with `Box::into_raw` by
        // the matching open-session callback and close is called once.
        let mut session = unsafe { Box::from_raw(session.cast::<FixtureFrameProcessorSession>()) };
        if let Some(output) = session.pending_output.take() {
            // SAFETY: pending fixture outputs are owned by this session and can
            // be reclaimed on close when the host never received them.
            let _ = unsafe { Box::from_raw(output.handle as *mut Vec<u8>) };
        }
        frame_processor_process_success(&FrameProcessorOperationStatus { completed: true })
    }

    unsafe extern "C" fn fixture_source_normalizer_packet_capabilities_json(
        _context: *mut c_void,
    ) -> VesperPluginBytes {
        let capabilities = SourceNormalizerPacketCapabilities {
            supported_runtime_profiles: vec!["fixture-packet".to_owned()],
            max_level: SourceNormalizerNormalizeLevel::RemuxOnly,
            media_kinds: vec![SourceNormalizerPacketMediaKind::Video],
            codecs: vec!["H264".to_owned()],
            bitstream_formats: vec![DecoderBitstreamFormat::Avcc],
            supports_seek: true,
            supports_flush: true,
            required_capabilities: SourceNormalizerRequiredCapabilities::default(),
            max_sessions: Some(1),
        };
        VesperPluginBytes::from_vec(
            serde_json::to_vec(&capabilities).expect("serialize source normalizer packet caps"),
        )
    }

    unsafe extern "C" fn fixture_source_normalizer_open_packet_session_json(
        _context: *mut c_void,
        config_json: *const u8,
        config_json_len: usize,
    ) -> VesperSourceNormalizerOpenPacketSessionResult {
        let config = match decode_source_normalizer_fixture_json::<
            SourceNormalizerPacketSessionConfig,
        >(config_json, config_json_len)
        {
            Ok(config) => config,
            Err(error) => return source_normalizer_packet_open_error(error),
        };
        if config.input.is_empty() {
            return source_normalizer_packet_open_error(SourceNormalizerError::invalid_input(
                "input must not be empty",
            ));
        }
        if !config
            .runtime_profile
            .eq_ignore_ascii_case("fixture-packet")
        {
            return source_normalizer_packet_open_error(
                SourceNormalizerError::UnsupportedRuntimeProfile {
                    profile: config.runtime_profile,
                },
            );
        }
        let info = SourceNormalizerPacketStreamInfo {
            session_id: Some("fixture-packet-session".to_owned()),
            normalizer_name: Some("test-source-normalizer-packet".to_owned()),
            runtime_profile: Some("fixture-packet".to_owned()),
            selected_backend: Some("fixture".to_owned()),
            tracks: vec![SourceNormalizerPacketTrackInfo {
                stream_index: 0,
                media_kind: SourceNormalizerPacketMediaKind::Video,
                codec: "H264".to_owned(),
                extradata: vec![1, 2, 3],
                bitstream_format: Some(DecoderBitstreamFormat::Avcc),
                width: Some(16),
                height: Some(16),
                coded_width: Some(16),
                coded_height: Some(16),
                sample_rate: None,
                channels: None,
                frame_rate: Some(30.0),
                time_base_num: Some(1),
                time_base_den: Some(90_000),
            }],
            selected_track_index: Some(0),
            duration_millis: Some(1_000),
            seekable: true,
        };
        let session = Box::into_raw(Box::new(FixtureSourceNormalizerPacketSession {
            emitted_packet: false,
            leased_packet: None,
            last_seek: None,
        }));
        VesperSourceNormalizerOpenPacketSessionResult {
            status: VesperPluginResultStatus::Success,
            session: session.cast::<c_void>(),
            payload: VesperPluginBytes::from_vec(
                serde_json::to_vec(&info).expect("serialize source normalizer packet info"),
            ),
        }
    }

    unsafe extern "C" fn fixture_source_normalizer_read_packet(
        _context: *mut c_void,
        session: *mut c_void,
    ) -> VesperSourceNormalizerReadPacketResult {
        let Some(session) = (unsafe {
            session
                .cast::<FixtureSourceNormalizerPacketSession>()
                .as_mut()
        }) else {
            return source_normalizer_read_packet_error(SourceNormalizerError::NotConfigured);
        };
        if session.leased_packet.is_some() {
            return source_normalizer_read_packet_error(SourceNormalizerError::abi_violation(
                "previous packet is still leased",
            ));
        }
        if session.emitted_packet {
            return source_normalizer_read_packet_success(
                &SourceNormalizerReadPacketMetadata::end_of_stream(),
                None,
            );
        }

        session.emitted_packet = true;
        let handle = 0x51;
        session.leased_packet = Some(FixtureSourceNormalizerPacketLease {
            handle,
            data: vec![0, 0, 1, 9],
        });
        let packet = session.leased_packet.as_ref().expect("stored packet");
        source_normalizer_read_packet_success(
            &SourceNormalizerReadPacketMetadata::packet(SourceNormalizerPacket {
                pts_us: session
                    .last_seek
                    .map(|millis| i64::try_from(millis.saturating_mul(1_000)).unwrap_or(i64::MAX))
                    .or(Some(1_000)),
                dts_us: Some(1_000),
                duration_us: Some(33_333),
                stream_index: 0,
                key_frame: true,
                discontinuity: session.last_seek.is_some(),
                end_of_stream: false,
            }),
            Some((packet.data.as_ptr(), packet.data.len(), packet.handle)),
        )
    }

    unsafe extern "C" fn fixture_source_normalizer_release_packet(
        _context: *mut c_void,
        session: *mut c_void,
        packet_handle: usize,
    ) -> VesperPluginProcessResult {
        let Some(session) = (unsafe {
            session
                .cast::<FixtureSourceNormalizerPacketSession>()
                .as_mut()
        }) else {
            return source_normalizer_process_error(SourceNormalizerError::NotConfigured);
        };
        match session.leased_packet.take() {
            Some(packet) if packet.handle == packet_handle => {
                if let Ok(mut releases) = SOURCE_NORMALIZER_PACKET_RELEASES.lock() {
                    releases.push(packet_handle);
                }
                source_normalizer_process_success(&SourceNormalizerOperationStatus {
                    completed: true,
                    message: None,
                })
            }
            Some(packet) => {
                session.leased_packet = Some(packet);
                source_normalizer_process_error(SourceNormalizerError::abi_violation(
                    "unexpected packet handle",
                ))
            }
            None => source_normalizer_process_error(SourceNormalizerError::abi_violation(
                "no packet is leased",
            )),
        }
    }

    unsafe extern "C" fn fixture_source_normalizer_seek_packet_session_json(
        _context: *mut c_void,
        session: *mut c_void,
        seek_json: *const u8,
        seek_json_len: usize,
    ) -> VesperPluginProcessResult {
        let Some(session) = (unsafe {
            session
                .cast::<FixtureSourceNormalizerPacketSession>()
                .as_mut()
        }) else {
            return source_normalizer_process_error(SourceNormalizerError::NotConfigured);
        };
        let seek = match decode_source_normalizer_fixture_json::<SourceNormalizerPacketSeek>(
            seek_json,
            seek_json_len,
        ) {
            Ok(seek) => seek,
            Err(error) => return source_normalizer_process_error(error),
        };
        session.leased_packet = None;
        session.emitted_packet = false;
        session.last_seek = Some(seek.position_millis);
        source_normalizer_process_success(&SourceNormalizerOperationStatus {
            completed: true,
            message: None,
        })
    }

    unsafe extern "C" fn fixture_source_normalizer_flush_packet_session(
        _context: *mut c_void,
        session: *mut c_void,
    ) -> VesperPluginProcessResult {
        let Some(session) = (unsafe {
            session
                .cast::<FixtureSourceNormalizerPacketSession>()
                .as_mut()
        }) else {
            return source_normalizer_process_error(SourceNormalizerError::NotConfigured);
        };
        session.leased_packet = None;
        session.emitted_packet = false;
        source_normalizer_process_success(&SourceNormalizerOperationStatus {
            completed: true,
            message: None,
        })
    }

    unsafe extern "C" fn fixture_source_normalizer_close_packet_session(
        _context: *mut c_void,
        session: *mut c_void,
    ) -> VesperPluginProcessResult {
        if session.is_null() {
            return source_normalizer_process_error(SourceNormalizerError::NotConfigured);
        }
        // SAFETY: the session pointer was allocated with `Box::into_raw` by
        // the matching open-session callback and close is called once.
        let _ = unsafe { Box::from_raw(session.cast::<FixtureSourceNormalizerPacketSession>()) };
        source_normalizer_process_success(&SourceNormalizerOperationStatus {
            completed: true,
            message: None,
        })
    }

    unsafe extern "C" fn fixture_payload_codec_process_json(
        _context: *mut c_void,
        _input_json: *const u8,
        _input_json_len: usize,
        _output_path: *const c_char,
        _progress: player_plugin::VesperPluginProgressCallbacks,
    ) -> VesperPluginProcessResult {
        VesperPluginProcessResult {
            status: VesperPluginResultStatus::Success,
            payload: VesperPluginBytes::from_vec(b"not-json".to_vec()),
        }
    }

    unsafe extern "C" fn fixture_null_payload_process_json(
        _context: *mut c_void,
        _input_json: *const u8,
        _input_json_len: usize,
        _output_path: *const c_char,
        _progress: player_plugin::VesperPluginProgressCallbacks,
    ) -> VesperPluginProcessResult {
        VesperPluginProcessResult {
            status: VesperPluginResultStatus::Success,
            payload: VesperPluginBytes {
                data: std::ptr::null_mut(),
                len: 4,
            },
        }
    }

    unsafe extern "C" fn fixture_hook_on_event_json(
        _context: *mut c_void,
        event_json: *const u8,
        event_json_len: usize,
    ) -> bool {
        // SAFETY: the fixture passes a valid event buffer for the duration of
        // this synchronous callback.
        let event_json = unsafe { std::slice::from_raw_parts(event_json, event_json_len) };
        let event: PipelineEvent = serde_json::from_slice(event_json).expect("deserialize event");
        if let Ok(mut events) = EVENTS.lock() {
            events.push(event);
        }
        true
    }

    fn decode_fixture_json<T: serde::de::DeserializeOwned>(
        data: *const u8,
        len: usize,
    ) -> Result<T, DecoderError> {
        if data.is_null() && len > 0 {
            return Err(DecoderError::abi_violation(
                "fixture JSON pointer was null with non-zero len",
            ));
        }
        let payload = if data.is_null() || len == 0 {
            &[]
        } else {
            // SAFETY: fixture callers pass a valid JSON buffer for the duration
            // of this synchronous callback.
            unsafe { std::slice::from_raw_parts(data, len) }
        };
        serde_json::from_slice(payload)
            .map_err(|error| DecoderError::payload_codec(error.to_string()))
    }

    fn decoder_open_error(error: DecoderError) -> VesperDecoderOpenSessionResult {
        VesperDecoderOpenSessionResult {
            status: VesperPluginResultStatus::Failure,
            session: std::ptr::null_mut(),
            payload: VesperPluginBytes::from_vec(
                serde_json::to_vec(&error).expect("serialize error"),
            ),
        }
    }

    fn decoder_process_success<T: serde::Serialize>(value: &T) -> VesperPluginProcessResult {
        VesperPluginProcessResult {
            status: VesperPluginResultStatus::Success,
            payload: VesperPluginBytes::from_vec(
                serde_json::to_vec(value).expect("serialize value"),
            ),
        }
    }

    fn decoder_process_error(error: DecoderError) -> VesperPluginProcessResult {
        VesperPluginProcessResult {
            status: VesperPluginResultStatus::Failure,
            payload: VesperPluginBytes::from_vec(
                serde_json::to_vec(&error).expect("serialize error"),
            ),
        }
    }

    fn decoder_native_frame_success(
        metadata: &DecoderReceiveNativeFrameMetadata,
        handle: usize,
    ) -> VesperDecoderReceiveNativeFrameResult {
        VesperDecoderReceiveNativeFrameResult {
            status: VesperPluginResultStatus::Success,
            metadata: VesperPluginBytes::from_vec(
                serde_json::to_vec(metadata).expect("serialize native frame metadata"),
            ),
            handle,
        }
    }

    fn decoder_native_frame_error(error: DecoderError) -> VesperDecoderReceiveNativeFrameResult {
        VesperDecoderReceiveNativeFrameResult {
            status: VesperPluginResultStatus::Failure,
            metadata: VesperPluginBytes::from_vec(
                serde_json::to_vec(&error).expect("serialize error"),
            ),
            handle: 0,
        }
    }

    fn decode_frame_processor_fixture_json<T: serde::de::DeserializeOwned>(
        data: *const u8,
        len: usize,
    ) -> Result<T, FrameProcessorError> {
        if data.is_null() && len > 0 {
            return Err(FrameProcessorError::abi_violation(
                "fixture frame processor JSON pointer was null with non-zero len",
            ));
        }
        let payload = if data.is_null() || len == 0 {
            &[]
        } else {
            // SAFETY: fixture callers pass a valid JSON buffer for the duration
            // of this synchronous callback.
            unsafe { std::slice::from_raw_parts(data, len) }
        };
        serde_json::from_slice(payload)
            .map_err(|error| FrameProcessorError::payload_codec(error.to_string()))
    }

    fn frame_processor_open_error(
        error: FrameProcessorError,
    ) -> VesperFrameProcessorOpenSessionResult {
        VesperFrameProcessorOpenSessionResult {
            status: VesperPluginResultStatus::Failure,
            session: std::ptr::null_mut(),
            payload: VesperPluginBytes::from_vec(
                serde_json::to_vec(&error).expect("serialize frame processor error"),
            ),
        }
    }

    fn frame_processor_process_success<T: serde::Serialize>(
        value: &T,
    ) -> VesperPluginProcessResult {
        VesperPluginProcessResult {
            status: VesperPluginResultStatus::Success,
            payload: VesperPluginBytes::from_vec(
                serde_json::to_vec(value).expect("serialize frame processor value"),
            ),
        }
    }

    fn frame_processor_process_error(error: FrameProcessorError) -> VesperPluginProcessResult {
        VesperPluginProcessResult {
            status: VesperPluginResultStatus::Failure,
            payload: VesperPluginBytes::from_vec(
                serde_json::to_vec(&error).expect("serialize frame processor error"),
            ),
        }
    }

    fn frame_processor_receive_success(
        metadata: &FrameProcessorReceiveFrameMetadata,
        handle: usize,
    ) -> VesperFrameProcessorReceiveFrameResult {
        VesperFrameProcessorReceiveFrameResult {
            status: VesperPluginResultStatus::Success,
            metadata: VesperPluginBytes::from_vec(
                serde_json::to_vec(metadata).expect("serialize frame processor metadata"),
            ),
            handle,
        }
    }

    fn frame_processor_receive_error(
        error: FrameProcessorError,
    ) -> VesperFrameProcessorReceiveFrameResult {
        VesperFrameProcessorReceiveFrameResult {
            status: VesperPluginResultStatus::Failure,
            metadata: VesperPluginBytes::from_vec(
                serde_json::to_vec(&error).expect("serialize frame processor error"),
            ),
            handle: 0,
        }
    }

    fn decode_source_normalizer_fixture_json<T: serde::de::DeserializeOwned>(
        data: *const u8,
        len: usize,
    ) -> Result<T, SourceNormalizerError> {
        if data.is_null() && len > 0 {
            return Err(SourceNormalizerError::abi_violation(
                "fixture source normalizer JSON pointer was null with non-zero len",
            ));
        }
        let payload = if data.is_null() || len == 0 {
            &[]
        } else {
            // SAFETY: fixture callers pass a valid JSON buffer for the duration
            // of this synchronous callback.
            unsafe { std::slice::from_raw_parts(data, len) }
        };
        serde_json::from_slice(payload)
            .map_err(|error| SourceNormalizerError::payload_codec(error.to_string()))
    }

    fn source_normalizer_packet_open_error(
        error: SourceNormalizerError,
    ) -> VesperSourceNormalizerOpenPacketSessionResult {
        VesperSourceNormalizerOpenPacketSessionResult {
            status: VesperPluginResultStatus::Failure,
            session: std::ptr::null_mut(),
            payload: VesperPluginBytes::from_vec(
                serde_json::to_vec(&error).expect("serialize source normalizer packet error"),
            ),
        }
    }

    fn source_normalizer_process_success<T: serde::Serialize>(
        value: &T,
    ) -> VesperPluginProcessResult {
        VesperPluginProcessResult {
            status: VesperPluginResultStatus::Success,
            payload: VesperPluginBytes::from_vec(
                serde_json::to_vec(value).expect("serialize source normalizer value"),
            ),
        }
    }

    fn source_normalizer_process_error(error: SourceNormalizerError) -> VesperPluginProcessResult {
        VesperPluginProcessResult {
            status: VesperPluginResultStatus::Failure,
            payload: VesperPluginBytes::from_vec(
                serde_json::to_vec(&error).expect("serialize source normalizer error"),
            ),
        }
    }

    fn source_normalizer_read_packet_success(
        metadata: &SourceNormalizerReadPacketMetadata,
        packet: Option<(*const u8, usize, usize)>,
    ) -> VesperSourceNormalizerReadPacketResult {
        let (data, data_len, packet_handle) = packet.unwrap_or((std::ptr::null(), 0, 0));
        VesperSourceNormalizerReadPacketResult {
            status: VesperPluginResultStatus::Success,
            metadata: VesperPluginBytes::from_vec(
                serde_json::to_vec(metadata).expect("serialize source normalizer packet metadata"),
            ),
            data,
            data_len,
            packet_handle,
        }
    }

    fn source_normalizer_read_packet_error(
        error: SourceNormalizerError,
    ) -> VesperSourceNormalizerReadPacketResult {
        VesperSourceNormalizerReadPacketResult {
            status: VesperPluginResultStatus::Failure,
            metadata: VesperPluginBytes::from_vec(
                serde_json::to_vec(&error).expect("serialize source normalizer packet error"),
            ),
            data: std::ptr::null(),
            data_len: 0,
            packet_handle: 0,
        }
    }

    fn fixture_native_frame() -> NativeFrame {
        NativeFrame {
            metadata: NativeFrameMetadata {
                media_kind: DecoderMediaKind::Video,
                format: DecoderFrameFormat::Nv12,
                codec: "fixture-video".to_owned(),
                pts_us: Some(2_000),
                duration_us: Some(33_333),
                width: 2,
                height: 2,
                coded_width: Some(2),
                coded_height: Some(2),
                visible_rect: None,
                handle_kind: NativeHandleKind::IoSurface,
                frame_id: Some(41),
                release_tracking: Some(NativeFrameReleaseTracking {
                    frame_id: Some(41),
                    requires_release: true,
                }),
            },
            handle: 0xfeed,
        }
    }

    unsafe extern "C" fn fixture_free_bytes(_context: *mut c_void, payload: VesperPluginBytes) {
        // SAFETY: the fixture only reclaims buffers it allocated with
        // `VesperPluginBytes::from_vec`.
        let _ = unsafe { payload.into_vec() };
    }

    fn native_frame_releases() -> Vec<usize> {
        NATIVE_FRAME_RELEASES
            .lock()
            .map(|releases| releases.clone())
            .unwrap_or_default()
    }

    fn resolve_vesper_remux_ffmpeg_plugin_path() -> Result<PathBuf, String> {
        if let Some(path) = env::var_os("VESPER_PLAYER_REMUX_FFMPEG_PLUGIN_PATH") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Ok(path);
            }
            return Err(format!(
                "environment variable VESPER_PLAYER_REMUX_FFMPEG_PLUGIN_PATH points to missing file `{}`",
                path.display()
            ));
        }

        resolve_plugin_path("vesper_remux_ffmpeg")
    }

    fn resolve_decoder_fixture_plugin_path() -> Result<PathBuf, String> {
        if let Some(path) = env::var_os("VESPER_DECODER_FIXTURE_PLUGIN_PATH") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Ok(path);
            }
            return Err(format!(
                "environment variable VESPER_DECODER_FIXTURE_PLUGIN_PATH points to missing file `{}`",
                path.display()
            ));
        }
        if let Some(paths) = env::var_os("VESPER_DECODER_PLUGIN_PATHS")
            && let Some(path) = env::split_paths(&paths).next()
        {
            if path.is_file() {
                return Ok(path);
            }
            return Err(format!(
                "environment variable VESPER_DECODER_PLUGIN_PATHS points to missing file `{}`",
                path.display()
            ));
        }

        resolve_plugin_path("player_decoder_fixture")
    }

    fn resolve_decoder_videotoolbox_plugin_path() -> Result<PathBuf, String> {
        if let Some(path) = env::var_os("VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Ok(path);
            }
            return Err(format!(
                "environment variable VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH points to missing file `{}`",
                path.display()
            ));
        }

        resolve_plugin_path("player_decoder_videotoolbox")
    }

    fn resolve_decoder_d3d11_plugin_path() -> Result<PathBuf, String> {
        if let Some(path) = env::var_os("VESPER_DECODER_D3D11_PLUGIN_PATH") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Ok(path);
            }
            return Err(format!(
                "environment variable VESPER_DECODER_D3D11_PLUGIN_PATH points to missing file `{}`",
                path.display()
            ));
        }

        resolve_plugin_path("player_decoder_d3d11")
    }

    fn resolve_frame_processor_diagnostic_plugin_path() -> Result<PathBuf, String> {
        if let Some(path) = env::var_os("VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Ok(path);
            }
            return Err(format!(
                "environment variable VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH points to missing file `{}`",
                path.display()
            ));
        }
        if let Some(paths) = env::var_os("VESPER_FRAME_PROCESSOR_PLUGIN_PATHS")
            && let Some(path) = env::split_paths(&paths).next()
        {
            if path.is_file() {
                return Ok(path);
            }
            return Err(format!(
                "environment variable VESPER_FRAME_PROCESSOR_PLUGIN_PATHS points to missing file `{}`",
                path.display()
            ));
        }

        resolve_plugin_path("player_frame_processor_diagnostic")
    }

    fn resolve_plugin_path(stem: &str) -> Result<PathBuf, String> {
        let workspace_root = workspace_root()?;
        let target_dir = env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .map(|path| {
                if path.is_absolute() {
                    path
                } else {
                    workspace_root.join(path)
                }
            })
            .unwrap_or_else(|| workspace_root.join("target"));
        let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_owned());
        let library_name = shared_library_name(stem);
        let candidates = [
            target_dir.join(&profile).join(&library_name),
            target_dir.join(&profile).join("deps").join(&library_name),
            target_dir.join("debug").join(&library_name),
            target_dir.join("debug").join("deps").join(&library_name),
            target_dir.join("release").join(&library_name),
            target_dir.join("release").join("deps").join(&library_name),
        ];

        candidates
            .into_iter()
            .find(|path| path.is_file())
            .ok_or_else(|| {
                format!(
                    "could not find `{library_name}` under `{}`; build the plugin crate first or set the matching plugin path environment variable",
                    target_dir.display()
                )
            })
    }

    fn shared_library_name(stem: &str) -> String {
        if cfg!(target_os = "windows") {
            format!("{stem}.dll")
        } else if cfg!(target_os = "macos") {
            format!("lib{stem}.dylib")
        } else {
            format!("lib{stem}.so")
        }
    }

    fn workspace_root() -> Result<PathBuf, String> {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .map(Path::to_path_buf)
            .ok_or_else(|| "failed to derive workspace root from CARGO_MANIFEST_DIR".to_owned())
    }

    #[allow(dead_code)]
    unsafe extern "C" fn fixture_error_process_json(
        _context: *mut c_void,
        _input_json: *const u8,
        _input_json_len: usize,
        _output_path: *const c_char,
        _progress: player_plugin::VesperPluginProgressCallbacks,
    ) -> VesperPluginProcessResult {
        let payload = serde_json::to_vec(&ProcessorError::UnsupportedFormat(
            ContentFormatKind::DashSegments,
        ))
        .expect("serialize error");
        VesperPluginProcessResult {
            status: VesperPluginResultStatus::Failure,
            payload: VesperPluginBytes::from_vec(payload),
        }
    }
}
