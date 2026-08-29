use super::*;
use player_plugin::PluginReference;

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

    pub fn audio(codec: impl Into<String>) -> Self {
        Self {
            codec: codec.into(),
            media_kind: DecoderMediaKind::Audio,
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
    pub supports_audio_packets: bool,
    pub supports_audio_frames: bool,
    pub supports_pcm_frames: bool,
    pub supports_gpu_handles: bool,
    pub supports_presentation_release: bool,
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
    pub accepted_input_pipeline_profiles: Vec<NativeFramePipelineProfile>,
    pub output_pipeline_profiles: Vec<NativeFramePipelineProfile>,
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
            accepted_input_pipeline_profiles: capabilities.accepted_input_pipeline_profiles.clone(),
            output_pipeline_profiles: capabilities.output_pipeline_profiles.clone(),
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

/// Compact capability summary for one resource-output source normalizer plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceNormalizerResourcePluginCapabilitySummary {
    pub supported_runtime_profiles: Vec<String>,
    pub supported_output_routes: Vec<String>,
    pub max_level: player_plugin::SourceNormalizerNormalizeLevel,
    pub content_types: Vec<String>,
    pub supports_growing_resources: bool,
    pub supports_range_reads: bool,
    pub supports_cancel: bool,
    pub required_capabilities: player_plugin::SourceNormalizerRequiredCapabilities,
    pub cache_policy: player_plugin::SourceNormalizerResourceCachePolicy,
    pub max_sessions: Option<u32>,
}

impl From<&SourceNormalizerResourceCapabilities>
    for SourceNormalizerResourcePluginCapabilitySummary
{
    fn from(capabilities: &SourceNormalizerResourceCapabilities) -> Self {
        Self {
            supported_runtime_profiles: capabilities.supported_runtime_profiles.clone(),
            supported_output_routes: capabilities
                .supported_output_routes
                .iter()
                .map(|route| route.wire_name().to_owned())
                .collect(),
            max_level: capabilities.max_level,
            content_types: capabilities.content_types.clone(),
            supports_growing_resources: capabilities.supports_growing_resources,
            supports_range_reads: capabilities.supports_range_reads,
            supports_cancel: capabilities.supports_cancel,
            required_capabilities: capabilities.required_capabilities.clone(),
            cache_policy: capabilities.cache_policy.clone(),
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
    SourceNormalizerResource(SourceNormalizerResourcePluginCapabilitySummary),
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
            supports_audio_packets: capabilities
                .codecs
                .iter()
                .any(|codec| codec.media_kind == DecoderMediaKind::Audio),
            supports_audio_frames: capabilities.supports_audio_frames,
            supports_pcm_frames: capabilities.supports_pcm_frames,
            supports_gpu_handles: capabilities.supports_gpu_handles,
            supports_presentation_release: capabilities.supports_presentation_release,
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

/// Capability availability derived from loader inspection only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginCapabilityAvailability {
    /// The record does not describe a probed capability.
    Unknown,
    /// The probed capability is available for later selection.
    Available,
    /// Loading or capability inspection rejected the plugin.
    Unavailable,
}

impl PluginCapabilityAvailability {
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Available => "available",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Diagnostic capability family independent from the native ABI layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginCapabilityKind {
    PostDownloadProcessor,
    PipelineEventHook,
    Decoder,
    BenchmarkSink,
    FrameProcessor,
    SourceNormalizer,
}

impl PluginCapabilityKind {
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::PostDownloadProcessor => "post_download_processor",
            Self::PipelineEventHook => "pipeline_event_hook",
            Self::Decoder => "decoder",
            Self::BenchmarkSink => "benchmark_sink",
            Self::FrameProcessor => "frame_processor",
            Self::SourceNormalizer => "source_normalizer",
        }
    }
}

impl PluginDiagnosticStatus {
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Loaded => "loaded",
            Self::LoadFailed => "loadFailed",
            Self::UnsupportedKind => "unsupportedKind",
            Self::DecoderSupported => "decoderSupported",
            Self::DecoderUnsupported => "decoderUnsupported",
            Self::FrameProcessorSupported => "frameProcessorSupported",
            Self::FrameProcessorUnsupported => "frameProcessorUnsupported",
            Self::SourceNormalizerSupported => "sourceNormalizerSupported",
            Self::SourceNormalizerUnsupported => "sourceNormalizerUnsupported",
        }
    }

    /// Returns availability established by this loader diagnostic.
    ///
    /// Availability never implies route selection or runtime participation.
    pub const fn capability_availability(self) -> PluginCapabilityAvailability {
        match self {
            Self::DecoderSupported
            | Self::FrameProcessorSupported
            | Self::SourceNormalizerSupported => PluginCapabilityAvailability::Available,
            Self::Loaded => PluginCapabilityAvailability::Unknown,
            Self::LoadFailed
            | Self::UnsupportedKind
            | Self::DecoderUnsupported
            | Self::FrameProcessorUnsupported
            | Self::SourceNormalizerUnsupported => PluginCapabilityAvailability::Unavailable,
        }
    }
}

/// Structured diagnostic record for one dynamic plugin path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDiagnosticRecord {
    pub path: PathBuf,
    pub status: PluginDiagnosticStatus,
    pub plugin_name: Option<String>,
    pub plugin_kind: Option<PluginCapabilityKind>,
    pub capability_summary: Option<PluginCapabilitySummary>,
    pub message: Option<String>,
}

pub(crate) fn decoder_capability_summary(
    record: &PluginDiagnosticRecord,
) -> Option<&DecoderPluginCapabilitySummary> {
    match record.capability_summary.as_ref() {
        Some(PluginCapabilitySummary::Decoder(summary)) => Some(summary),
        _ => None,
    }
}

pub(crate) fn source_normalizer_packet_capability_summary(
    record: &PluginDiagnosticRecord,
) -> Option<&SourceNormalizerPacketPluginCapabilitySummary> {
    match record.capability_summary.as_ref() {
        Some(PluginCapabilitySummary::SourceNormalizerPacket(summary)) => Some(summary),
        _ => None,
    }
}

pub(crate) fn source_normalizer_resource_capability_summary(
    record: &PluginDiagnosticRecord,
) -> Option<&SourceNormalizerResourcePluginCapabilitySummary> {
    match record.capability_summary.as_ref() {
        Some(PluginCapabilitySummary::SourceNormalizerResource(summary)) => Some(summary),
        _ => None,
    }
}

impl PluginDiagnosticRecord {
    /// Returns the capability availability established during inspection.
    pub const fn capability_availability(&self) -> PluginCapabilityAvailability {
        self.status.capability_availability()
    }

    pub(crate) fn from_native_decoder_interface(
        path: impl Into<PathBuf>,
        plugin: &LoadedNativePlugin,
        reference: &PluginReference,
        decoder_match: &DecoderPluginMatchRequest,
    ) -> Self {
        let path = path.into();
        match plugin.resolve_native_decoder(reference) {
            Ok(factory) => {
                let capabilities = factory.capabilities();
                let native_requirements = factory.native_requirements();
                let capability_summary = DecoderPluginCapabilitySummary::from_capabilities(
                    &capabilities,
                    true,
                    Some(native_requirements),
                );
                let supported =
                    capabilities.supports_codec(&decoder_match.codec, decoder_match.media_kind);
                let status = if supported {
                    PluginDiagnosticStatus::DecoderSupported
                } else {
                    PluginDiagnosticStatus::DecoderUnsupported
                };
                let message = if supported {
                    format!(
                        "{} instance `{}` advertises {:?} {} support with native-frame output",
                        factory.name(),
                        reference.capability_instance_id().unwrap_or("unknown"),
                        decoder_match.media_kind,
                        decoder_match.codec
                    )
                } else {
                    format!(
                        "{} instance `{}` does not advertise {:?} {} support",
                        factory.name(),
                        reference.capability_instance_id().unwrap_or("unknown"),
                        decoder_match.media_kind,
                        decoder_match.codec
                    )
                };
                Self {
                    path,
                    status,
                    plugin_name: Some(factory.name().to_owned()),
                    plugin_kind: Some(PluginCapabilityKind::Decoder),
                    capability_summary: Some(PluginCapabilitySummary::Decoder(capability_summary)),
                    message: Some(message),
                }
            }
            Err(error) => Self {
                path,
                status: PluginDiagnosticStatus::DecoderUnsupported,
                plugin_name: Some(plugin.plugin_name().to_owned()),
                plugin_kind: Some(PluginCapabilityKind::Decoder),
                capability_summary: None,
                message: Some(format!(
                    "instance `{}` is unavailable: {error}",
                    reference.capability_instance_id().unwrap_or("unknown")
                )),
            },
        }
    }

    pub(crate) fn from_native_frame_processor_interface(
        path: impl Into<PathBuf>,
        plugin: &LoadedNativePlugin,
        reference: &PluginReference,
    ) -> Self {
        let path = path.into();
        match plugin.resolve_frame_processor(reference) {
            Ok(factory) => {
                let capabilities = factory.capabilities();
                let capability_summary = FrameProcessorPluginCapabilitySummary::from(&capabilities);
                let supported =
                    capabilities.supports_video_frames && !capabilities.may_change_dimensions;
                let status = if supported {
                    PluginDiagnosticStatus::FrameProcessorSupported
                } else {
                    PluginDiagnosticStatus::FrameProcessorUnsupported
                };
                let message = if supported {
                    format!(
                        "{} frame processor instance `{}` loaded",
                        factory.name(),
                        reference.capability_instance_id().unwrap_or("unknown")
                    )
                } else if capabilities.may_change_dimensions {
                    format!(
                        "{} frame processor instance `{}` changes frame dimensions, which the current interface does not allow",
                        factory.name(),
                        reference.capability_instance_id().unwrap_or("unknown")
                    )
                } else {
                    format!(
                        "{} frame processor instance `{}` does not advertise video frame processing support",
                        factory.name(),
                        reference.capability_instance_id().unwrap_or("unknown")
                    )
                };
                Self {
                    path,
                    status,
                    plugin_name: Some(factory.name().to_owned()),
                    plugin_kind: Some(PluginCapabilityKind::FrameProcessor),
                    capability_summary: Some(PluginCapabilitySummary::FrameProcessor(
                        capability_summary,
                    )),
                    message: Some(message),
                }
            }
            Err(error) => Self {
                path,
                status: PluginDiagnosticStatus::FrameProcessorUnsupported,
                plugin_name: Some(plugin.plugin_name().to_owned()),
                plugin_kind: Some(PluginCapabilityKind::FrameProcessor),
                capability_summary: None,
                message: Some(format!(
                    "instance `{}` is unavailable: {error}",
                    reference.capability_instance_id().unwrap_or("unknown")
                )),
            },
        }
    }

    pub(crate) fn from_native_source_packet_interface(
        path: impl Into<PathBuf>,
        plugin: &LoadedNativePlugin,
        reference: &PluginReference,
    ) -> Self {
        let path = path.into();
        match plugin.resolve_source_packet(reference) {
            Ok(factory) => {
                let capabilities = factory.packet_capabilities();
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
                    format!(
                        "{} source normalizer packet instance `{}` loaded",
                        factory.name(),
                        reference.capability_instance_id().unwrap_or("unknown")
                    )
                } else if capabilities.supported_runtime_profiles.is_empty() {
                    format!(
                        "{} packet instance `{}` does not advertise source normalizer runtime profiles",
                        factory.name(),
                        reference.capability_instance_id().unwrap_or("unknown")
                    )
                } else {
                    format!(
                        "{} packet instance `{}` does not advertise source normalizer media kinds",
                        factory.name(),
                        reference.capability_instance_id().unwrap_or("unknown")
                    )
                };
                Self {
                    path,
                    status,
                    plugin_name: Some(factory.name().to_owned()),
                    plugin_kind: Some(PluginCapabilityKind::SourceNormalizer),
                    capability_summary: Some(PluginCapabilitySummary::SourceNormalizerPacket(
                        capability_summary,
                    )),
                    message: Some(message),
                }
            }
            Err(error) => Self {
                path,
                status: PluginDiagnosticStatus::SourceNormalizerUnsupported,
                plugin_name: Some(plugin.plugin_name().to_owned()),
                plugin_kind: Some(PluginCapabilityKind::SourceNormalizer),
                capability_summary: None,
                message: Some(format!(
                    "instance `{}` is unavailable: {error}",
                    reference.capability_instance_id().unwrap_or("unknown")
                )),
            },
        }
    }

    pub(crate) fn from_native_source_resource_interface(
        path: impl Into<PathBuf>,
        plugin: &LoadedNativePlugin,
        reference: &PluginReference,
    ) -> Self {
        let path = path.into();
        match plugin.resolve_source_resource(reference) {
            Ok(factory) => {
                let capabilities = factory.resource_capabilities();
                let capability_summary =
                    SourceNormalizerResourcePluginCapabilitySummary::from(&capabilities);
                let supported = !capabilities.supported_runtime_profiles.is_empty()
                    && !capabilities.supported_output_routes.is_empty();
                let status = if supported {
                    PluginDiagnosticStatus::SourceNormalizerSupported
                } else {
                    PluginDiagnosticStatus::SourceNormalizerUnsupported
                };
                let message = if supported {
                    format!(
                        "{} source normalizer resource instance `{}` loaded",
                        factory.name(),
                        reference.capability_instance_id().unwrap_or("unknown")
                    )
                } else if capabilities.supported_runtime_profiles.is_empty() {
                    format!(
                        "{} resource instance `{}` does not advertise source normalizer runtime profiles",
                        factory.name(),
                        reference.capability_instance_id().unwrap_or("unknown")
                    )
                } else {
                    format!(
                        "{} resource instance `{}` does not advertise source normalizer output routes",
                        factory.name(),
                        reference.capability_instance_id().unwrap_or("unknown")
                    )
                };
                Self {
                    path,
                    status,
                    plugin_name: Some(factory.name().to_owned()),
                    plugin_kind: Some(PluginCapabilityKind::SourceNormalizer),
                    capability_summary: Some(PluginCapabilitySummary::SourceNormalizerResource(
                        capability_summary,
                    )),
                    message: Some(message),
                }
            }
            Err(error) => Self {
                path,
                status: PluginDiagnosticStatus::SourceNormalizerUnsupported,
                plugin_name: Some(plugin.plugin_name().to_owned()),
                plugin_kind: Some(PluginCapabilityKind::SourceNormalizer),
                capability_summary: None,
                message: Some(format!(
                    "instance `{}` is unavailable: {error}",
                    reference.capability_instance_id().unwrap_or("unknown")
                )),
            },
        }
    }

    pub(crate) fn unsupported_native_interface(
        path: impl Into<PathBuf>,
        plugin: &LoadedNativePlugin,
        interface: &'static str,
    ) -> Self {
        Self {
            path: path.into(),
            status: PluginDiagnosticStatus::UnsupportedKind,
            plugin_name: Some(plugin.plugin_name().to_owned()),
            plugin_kind: None,
            capability_summary: None,
            message: Some(format!(
                "{} does not expose interface {interface}",
                plugin.plugin_name()
            )),
        }
    }

    pub fn load_failed(path: impl Into<PathBuf>, error: PluginLoadError) -> Self {
        Self::load_failed_message(path, error.to_string())
    }

    pub(crate) fn load_failed_message(
        path: impl Into<PathBuf>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            status: PluginDiagnosticStatus::LoadFailed,
            plugin_name: None,
            plugin_kind: None,
            capability_summary: None,
            message: Some(message.into()),
        }
    }

    pub fn summary(&self) -> String {
        self.message
            .clone()
            .or_else(|| self.plugin_name.clone())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}
