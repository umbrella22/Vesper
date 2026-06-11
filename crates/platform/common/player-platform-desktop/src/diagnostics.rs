use player_plugin::{DecoderMediaKind, NativeHandleKind, VesperPluginKind};
use player_plugin_loader::{
    DecoderPluginCapabilitySummary, DecoderPluginCodecSummary,
    FrameProcessorPluginCapabilitySummary, PluginCapabilitySummary, PluginDiagnosticRecord,
    PluginDiagnosticStatus, SourceNormalizerPacketPluginCapabilitySummary,
    SourceNormalizerResourcePluginCapabilitySummary,
};
use player_runtime::{
    PlayerPluginCapabilitySummary, PlayerPluginCodecCapability,
    PlayerPluginDecoderCapabilitySummary, PlayerPluginDiagnostic, PlayerPluginDiagnosticDetail,
    PlayerPluginDiagnosticStatus, PlayerPluginFrameProcessorCapabilitySummary,
    PlayerPluginParticipation, PlayerRuntimeStartup,
};

pub fn append_plugin_diagnostics(
    mut startup: PlayerRuntimeStartup,
    diagnostics: &[PlayerPluginDiagnostic],
) -> PlayerRuntimeStartup {
    for diagnostic in diagnostics {
        if startup
            .plugin_diagnostics
            .iter()
            .any(|existing| same_plugin_diagnostic(existing, diagnostic))
        {
            continue;
        }
        startup.plugin_diagnostics.push(diagnostic.clone());
    }
    startup
}

pub fn mark_plugin_diagnostics_fallback(
    diagnostics: &mut [PlayerPluginDiagnostic],
    fallback_reason: &str,
) {
    for diagnostic in diagnostics {
        if matches!(
            diagnostic.participation,
            PlayerPluginParticipation::Selected | PlayerPluginParticipation::Participated
        ) {
            diagnostic.participation = PlayerPluginParticipation::Fallback;
            diagnostic.message = Some(match diagnostic.message.take() {
                Some(existing) if !existing.is_empty() => {
                    format!("{existing}; fallbackReason={fallback_reason}")
                }
                _ => format!("fallbackReason={fallback_reason}"),
            });
            if !diagnostic
                .details
                .iter()
                .any(|detail| detail.key == "fallbackReason")
            {
                diagnostic.details.push(PlayerPluginDiagnosticDetail {
                    key: "fallbackReason".to_owned(),
                    value: fallback_reason.to_owned(),
                });
            }
        }
    }
}

pub fn plugin_diagnostic_detail(
    key: impl Into<String>,
    value: impl Into<String>,
) -> PlayerPluginDiagnosticDetail {
    PlayerPluginDiagnosticDetail {
        key: key.into(),
        value: value.into(),
    }
}

pub fn unsupported_plugin_diagnostic(
    plugin_kind: &str,
    status: PlayerPluginDiagnosticStatus,
    message: impl Into<String>,
    detail_key: &str,
) -> PlayerPluginDiagnostic {
    PlayerPluginDiagnostic {
        path: String::new(),
        plugin_name: None,
        plugin_kind: Some(plugin_kind.to_owned()),
        status,
        message: Some(message.into()),
        capability: None,
        participation: PlayerPluginParticipation::Unknown,
        details: vec![plugin_diagnostic_detail(detail_key, "unsupported")],
    }
}

pub fn same_plugin_diagnostic(
    left: &PlayerPluginDiagnostic,
    right: &PlayerPluginDiagnostic,
) -> bool {
    left.path == right.path
        && left.plugin_name == right.plugin_name
        && left.plugin_kind == right.plugin_kind
        && left.status == right.status
        && left.message == right.message
        && left.details == right.details
}

pub fn plugin_diagnostic_label(record: &PluginDiagnosticRecord) -> String {
    record
        .plugin_name
        .clone()
        .unwrap_or_else(|| record.path.display().to_string())
}

pub fn player_plugin_diagnostic_from_record(
    record: &PluginDiagnosticRecord,
    participation: PlayerPluginParticipation,
) -> PlayerPluginDiagnostic {
    PlayerPluginDiagnostic {
        path: record.path.display().to_string(),
        plugin_name: record.plugin_name.clone(),
        plugin_kind: record.plugin_kind.map(plugin_kind_label).map(str::to_owned),
        status: runtime_status_from_loader(record.status),
        message: record.message.clone(),
        capability: record
            .capability_summary
            .as_ref()
            .and_then(player_plugin_capability_summary_from_loader),
        participation,
        details: Vec::new(),
    }
}

pub fn available_plugin_diagnostic_from_record(
    record: &PluginDiagnosticRecord,
) -> PlayerPluginDiagnostic {
    let participation = if matches!(
        record.status,
        PluginDiagnosticStatus::DecoderSupported
            | PluginDiagnosticStatus::FrameProcessorSupported
            | PluginDiagnosticStatus::SourceNormalizerSupported
    ) {
        PlayerPluginParticipation::Available
    } else {
        PlayerPluginParticipation::Unknown
    };
    player_plugin_diagnostic_from_record(record, participation)
}

pub fn runtime_status_from_loader(status: PluginDiagnosticStatus) -> PlayerPluginDiagnosticStatus {
    PlayerPluginDiagnosticStatus::from_wire_name(status.wire_name())
        .unwrap_or(PlayerPluginDiagnosticStatus::UnsupportedKind)
}

pub fn player_plugin_capability_summary_from_loader(
    summary: &PluginCapabilitySummary,
) -> Option<PlayerPluginCapabilitySummary> {
    match summary {
        PluginCapabilitySummary::Decoder(summary) => Some(PlayerPluginCapabilitySummary::Decoder(
            player_decoder_capability_summary_from_loader(summary),
        )),
        PluginCapabilitySummary::FrameProcessor(summary) => {
            Some(PlayerPluginCapabilitySummary::FrameProcessor(
                player_frame_processor_capability_summary_from_loader(summary),
            ))
        }
        PluginCapabilitySummary::SourceNormalizerPacket(summary) => {
            Some(PlayerPluginCapabilitySummary::SourceNormalizer(
                player_source_normalizer_capability_summary_from_loader(summary),
            ))
        }
        PluginCapabilitySummary::SourceNormalizerResource(summary) => {
            Some(PlayerPluginCapabilitySummary::SourceNormalizer(
                player_source_normalizer_resource_capability_summary_from_loader(summary),
            ))
        }
    }
}

pub fn player_source_normalizer_capability_summary_from_loader(
    summary: &SourceNormalizerPacketPluginCapabilitySummary,
) -> player_runtime::PlayerPluginSourceNormalizerCapabilitySummary {
    player_runtime::PlayerPluginSourceNormalizerCapabilitySummary {
        supported_runtime_profiles: summary.supported_runtime_profiles.clone(),
        supported_output_routes: vec!["packetStream".to_owned()],
        max_level: format!("{:?}", summary.max_level),
        media_kinds: summary
            .media_kinds
            .iter()
            .map(|kind| format!("{kind:?}"))
            .collect(),
        codecs: summary.codecs.clone(),
        bitstream_formats: summary
            .bitstream_formats
            .iter()
            .map(|format| format!("{format:?}"))
            .collect(),
        supports_seek: summary.supports_seek,
        supports_flush: summary.supports_flush,
        supports_growing_resources: false,
        supports_range_reads: false,
        supports_cancel: false,
        content_types: Vec::new(),
        required_libraries: summary.required_capabilities.libraries.clone(),
        required_demuxers: summary.required_capabilities.demuxers.clone(),
        required_muxers: summary.required_capabilities.muxers.clone(),
        required_protocols: summary.required_capabilities.protocols.clone(),
        required_parsers: summary.required_capabilities.parsers.clone(),
        required_bitstream_filters: summary.required_capabilities.bitstream_filters.clone(),
        required_tls: summary.required_capabilities.tls.clone(),
        requires_network: summary.required_capabilities.network,
        session_read_buffer_bytes: None,
        manifest_snapshot_bytes: None,
        session_disk_soft_cap_bytes: None,
        global_disk_soft_cap_bytes: None,
        max_sessions: summary.max_sessions,
    }
}

pub fn player_source_normalizer_resource_capability_summary_from_loader(
    summary: &SourceNormalizerResourcePluginCapabilitySummary,
) -> player_runtime::PlayerPluginSourceNormalizerCapabilitySummary {
    player_runtime::PlayerPluginSourceNormalizerCapabilitySummary {
        supported_runtime_profiles: summary.supported_runtime_profiles.clone(),
        supported_output_routes: summary.supported_output_routes.clone(),
        max_level: format!("{:?}", summary.max_level),
        media_kinds: Vec::new(),
        codecs: Vec::new(),
        bitstream_formats: Vec::new(),
        supports_seek: false,
        supports_flush: false,
        supports_growing_resources: summary.supports_growing_resources,
        supports_range_reads: summary.supports_range_reads,
        supports_cancel: summary.supports_cancel,
        content_types: summary.content_types.clone(),
        required_libraries: summary.required_capabilities.libraries.clone(),
        required_demuxers: summary.required_capabilities.demuxers.clone(),
        required_muxers: summary.required_capabilities.muxers.clone(),
        required_protocols: summary.required_capabilities.protocols.clone(),
        required_parsers: summary.required_capabilities.parsers.clone(),
        required_bitstream_filters: summary.required_capabilities.bitstream_filters.clone(),
        required_tls: summary.required_capabilities.tls.clone(),
        requires_network: summary.required_capabilities.network,
        session_read_buffer_bytes: Some(summary.cache_policy.session_read_buffer_bytes),
        manifest_snapshot_bytes: Some(summary.cache_policy.manifest_snapshot_bytes),
        session_disk_soft_cap_bytes: Some(summary.cache_policy.session_disk_soft_cap_bytes),
        global_disk_soft_cap_bytes: Some(summary.cache_policy.global_disk_soft_cap_bytes),
        max_sessions: summary.max_sessions,
    }
}

pub fn player_decoder_capability_summary_from_loader(
    summary: &DecoderPluginCapabilitySummary,
) -> PlayerPluginDecoderCapabilitySummary {
    PlayerPluginDecoderCapabilitySummary {
        codecs: summary
            .typed_codecs
            .iter()
            .map(player_decoder_codec_summary_from_loader)
            .collect(),
        legacy_codecs: summary.codecs.clone(),
        supports_native_frame_output: summary.supports_native_frame_output,
        supports_hardware_decode: summary.supports_hardware_decode,
        supports_cpu_video_frames: summary.supports_cpu_video_frames,
        supports_audio_packets: summary.supports_audio_packets,
        supports_audio_frames: summary.supports_audio_frames,
        supports_pcm_frames: summary.supports_pcm_frames,
        supports_gpu_handles: summary.supports_gpu_handles,
        supports_flush: summary.supports_flush,
        supports_drain: summary.supports_drain,
        max_sessions: summary.max_sessions,
    }
}

pub fn player_decoder_codec_summary_from_loader(
    summary: &DecoderPluginCodecSummary,
) -> PlayerPluginCodecCapability {
    PlayerPluginCodecCapability {
        media_kind: match summary.media_kind {
            DecoderMediaKind::Video => "video",
            DecoderMediaKind::Audio => "audio",
        }
        .to_owned(),
        codec: summary.codec.clone(),
    }
}

pub fn player_frame_processor_capability_summary_from_loader(
    summary: &FrameProcessorPluginCapabilitySummary,
) -> PlayerPluginFrameProcessorCapabilitySummary {
    PlayerPluginFrameProcessorCapabilitySummary {
        accepted_input_handle_kinds: summary
            .accepted_input_handle_kinds
            .iter()
            .map(native_handle_kind_label)
            .collect(),
        output_handle_kinds: summary
            .output_handle_kinds
            .iter()
            .map(native_handle_kind_label)
            .collect(),
        accepted_input_pipeline_profiles: summary
            .accepted_input_pipeline_profiles
            .iter()
            .map(|profile| profile.label())
            .collect(),
        output_pipeline_profiles: summary
            .output_pipeline_profiles
            .iter()
            .map(|profile| profile.label())
            .collect(),
        supports_video_frames: summary.supports_video_frames,
        supports_in_place_passthrough: summary.supports_in_place_passthrough,
        preserves_dimensions: summary.preserves_dimensions,
        may_change_dimensions: summary.may_change_dimensions,
        preserves_color_metadata: summary.preserves_color_metadata,
        preserves_hdr_metadata: summary.preserves_hdr_metadata,
        supports_flush: summary.supports_flush,
        max_sessions: summary.max_sessions,
        max_in_flight_frames: summary.max_in_flight_frames,
    }
}

pub fn native_handle_kind_label(handle_kind: &NativeHandleKind) -> String {
    match handle_kind {
        NativeHandleKind::CvPixelBuffer => "cv_pixel_buffer".to_owned(),
        NativeHandleKind::IoSurface => "io_surface".to_owned(),
        NativeHandleKind::MetalTexture => "metal_texture".to_owned(),
        NativeHandleKind::DmaBuf => "dma_buf".to_owned(),
        NativeHandleKind::VaapiSurface => "vaapi_surface".to_owned(),
        NativeHandleKind::D3D11Texture2D => "d3d11_texture_2d".to_owned(),
        NativeHandleKind::DxgiSurface => "dxgi_surface".to_owned(),
        NativeHandleKind::VulkanImage => "vulkan_image".to_owned(),
        NativeHandleKind::MediaCodecHardwareBuffer => "media_codec_hardware_buffer".to_owned(),
        NativeHandleKind::MediaCodecSurfaceTexture => "media_codec_surface_texture".to_owned(),
        NativeHandleKind::Unknown(name) => name.clone(),
    }
}

pub fn plugin_kind_label(kind: VesperPluginKind) -> &'static str {
    match kind {
        VesperPluginKind::PostDownloadProcessor => "post_download_processor",
        VesperPluginKind::PipelineEventHook => "pipeline_event_hook",
        VesperPluginKind::Decoder => "decoder",
        VesperPluginKind::BenchmarkSink => "benchmark_sink",
        VesperPluginKind::FrameProcessor => "frame_processor",
        VesperPluginKind::SourceNormalizer => "source_normalizer",
    }
}
