use super::*;

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
                    .or_else(|| source_normalizer_capability_summary(plugin)),
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
            capability_summary: decoder_summary
                .or_else(|| source_normalizer_capability_summary(plugin)),
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
        if let Some((name, capabilities)) = source_normalizer_resource_summary(plugin) {
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
                format!("{name} source_normalizer_resource_v3 plugin loaded")
            } else if capabilities.supported_runtime_profiles.is_empty() {
                format!("{name} does not advertise resource source normalizer runtime profiles")
            } else {
                format!("{name} does not advertise resource source normalizer output routes")
            };
            return Self {
                path,
                status,
                plugin_name: Some(name),
                plugin_kind: Some(plugin.plugin_kind()),
                capability_summary: Some(PluginCapabilitySummary::SourceNormalizerResource(
                    capability_summary,
                )),
                message: Some(message),
            };
        }

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
            .or_else(|| source_normalizer_capability_summary(plugin));

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

fn source_normalizer_resource_summary(
    plugin: &LoadedDynamicPlugin,
) -> Option<(String, SourceNormalizerResourceCapabilities)> {
    plugin
        .source_normalizer_resource_plugin_factory()
        .map(|factory| (factory.name().to_owned(), factory.resource_capabilities()))
}

fn source_normalizer_packet_factory_summary(
    plugin: &LoadedDynamicPlugin,
) -> Option<SourceNormalizerPacketCapabilities> {
    plugin
        .source_normalizer_packet_plugin_factory()
        .map(|factory| factory.packet_capabilities())
}

fn source_normalizer_resource_factory_summary(
    plugin: &LoadedDynamicPlugin,
) -> Option<SourceNormalizerResourceCapabilities> {
    plugin
        .source_normalizer_resource_plugin_factory()
        .map(|factory| factory.resource_capabilities())
}

fn source_normalizer_capability_summary(
    plugin: &LoadedDynamicPlugin,
) -> Option<PluginCapabilitySummary> {
    source_normalizer_resource_factory_summary(plugin)
        .map(|capabilities| {
            PluginCapabilitySummary::SourceNormalizerResource(
                SourceNormalizerResourcePluginCapabilitySummary::from(&capabilities),
            )
        })
        .or_else(|| {
            source_normalizer_packet_factory_summary(plugin).map(|capabilities| {
                PluginCapabilitySummary::SourceNormalizerPacket(
                    SourceNormalizerPacketPluginCapabilitySummary::from(&capabilities),
                )
            })
        })
}
