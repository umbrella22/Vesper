use super::*;

pub(crate) fn apply_video_decode_diagnostics(
    mut startup: PlayerRuntimeStartup,
    video_decode: &PlayerVideoDecodeInfo,
) -> PlayerRuntimeStartup {
    match startup.video_decode.as_mut() {
        Some(current) => {
            if !current.hardware_available {
                current.hardware_available = video_decode.hardware_available;
            }
            if current.hardware_backend.is_none() {
                current.hardware_backend = video_decode.hardware_backend.clone();
            }
            if current.fallback_reason.is_none() {
                current.fallback_reason = video_decode.fallback_reason.clone();
            }
        }
        None => {
            startup.video_decode = Some(video_decode.clone());
        }
    }
    startup
}

pub(crate) fn macos_runtime_diagnostics(
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
) -> MacosRuntimeDiagnostics {
    let mut video_decode = macos_video_decode_info(media_info);
    let mut plugin_diagnostics = Vec::new();

    if let Some(registry) = decoder_plugin_registry(media_info, options) {
        let selected_decoder = selected_decoder_plugin_name(media_info, options, &registry);
        video_decode =
            apply_decoder_plugin_registry_to_video_decode(video_decode, media_info, &registry);
        plugin_diagnostics.extend(registry.records().iter().map(|record| {
            player_plugin_diagnostic_from_record(
                record,
                decoder_plugin_participation(record, selected_decoder.as_deref(), options),
            )
        }));
    }
    if let Some(registry) = frame_processor_plugin_registry(options) {
        plugin_diagnostics.extend(registry.records().iter().map(|record| {
            player_plugin_diagnostic_from_record(
                record,
                frame_processor_plugin_participation(record),
            )
        }));
    }

    video_decode =
        apply_native_frame_plugin_preference_to_video_decode(video_decode, media_info, options);

    MacosRuntimeDiagnostics {
        video_decode,
        plugin_diagnostics,
        has_video_surface: false,
    }
}

pub(crate) fn apply_macos_runtime_diagnostics(
    startup: PlayerRuntimeStartup,
    diagnostics: &MacosRuntimeDiagnostics,
) -> PlayerRuntimeStartup {
    let startup = apply_video_decode_diagnostics(startup, &diagnostics.video_decode);
    append_plugin_diagnostics(startup, &diagnostics.plugin_diagnostics)
}

pub(crate) fn append_plugin_diagnostics(
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

pub(crate) fn same_plugin_diagnostic(
    left: &PlayerPluginDiagnostic,
    right: &PlayerPluginDiagnostic,
) -> bool {
    left.path == right.path
        && left.plugin_name == right.plugin_name
        && left.plugin_kind == right.plugin_kind
        && left.status == right.status
        && left.message == right.message
}

pub(crate) fn macos_video_decode_info(media_info: &PlayerMediaInfo) -> PlayerVideoDecodeInfo {
    let Some(best_video) = media_info.best_video.as_ref() else {
        return PlayerVideoDecodeInfo {
            selected_mode: PlayerVideoDecodeMode::Software,
            hardware_available: false,
            hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
            fallback_reason: Some("source does not expose a decodable video stream".to_owned()),
        };
    };

    let support = probe_videotoolbox_hardware_decode(&best_video.codec);
    let fallback_reason = if support.hardware_available {
        Some(
            "system VideoToolbox hardware decode support detected; Apple platforms should prefer the native backend, while the software desktop path remains available as fallback"
                .to_owned(),
        )
    } else {
        support.fallback_reason.clone()
    };

    PlayerVideoDecodeInfo {
        selected_mode: PlayerVideoDecodeMode::Software,
        hardware_available: support.hardware_available,
        hardware_backend: support.hardware_backend,
        fallback_reason,
    }
}

pub(crate) fn apply_decoder_plugin_diagnostics(
    mut startup: PlayerRuntimeStartup,
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
) -> PlayerRuntimeStartup {
    if let Some(registry) = decoder_plugin_registry(media_info, options) {
        let selected_decoder = selected_decoder_plugin_name(media_info, options, &registry);
        startup
            .plugin_diagnostics
            .extend(registry.records().iter().map(|record| {
                player_plugin_diagnostic_from_record(
                    record,
                    decoder_plugin_participation(record, selected_decoder.as_deref(), options),
                )
            }));
        if let Some(video_decode) = startup.video_decode.take() {
            startup.video_decode = Some(apply_decoder_plugin_registry_to_video_decode(
                video_decode,
                media_info,
                &registry,
            ));
        }
    }
    apply_frame_processor_plugin_diagnostics(startup, options)
}

pub(crate) fn apply_frame_processor_plugin_diagnostics(
    mut startup: PlayerRuntimeStartup,
    options: &PlayerRuntimeOptions,
) -> PlayerRuntimeStartup {
    let Some(registry) = frame_processor_plugin_registry(options) else {
        return startup;
    };
    startup
        .plugin_diagnostics
        .extend(registry.records().iter().map(|record| {
            player_plugin_diagnostic_from_record(
                record,
                frame_processor_plugin_participation(record),
            )
        }));
    startup
}

#[cfg(test)]
pub(crate) fn apply_decoder_plugin_diagnostics_to_video_decode(
    video_decode: PlayerVideoDecodeInfo,
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
) -> PlayerVideoDecodeInfo {
    let Some(registry) = decoder_plugin_registry(media_info, options) else {
        return video_decode;
    };
    apply_decoder_plugin_registry_to_video_decode(video_decode, media_info, &registry)
}

pub(crate) fn apply_decoder_plugin_registry_to_video_decode(
    mut video_decode: PlayerVideoDecodeInfo,
    media_info: &PlayerMediaInfo,
    registry: &PluginRegistry,
) -> PlayerVideoDecodeInfo {
    if video_decode
        .fallback_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("decoder plugin"))
    {
        return video_decode;
    }

    if let Some(diagnostic) = decoder_plugin_diagnostic(media_info, registry) {
        video_decode.fallback_reason = Some(match video_decode.fallback_reason.take() {
            Some(existing) if !existing.is_empty() => format!("{existing}; {diagnostic}"),
            _ => diagnostic,
        });
    }

    video_decode
}

pub(crate) fn apply_native_frame_plugin_preference_to_video_decode(
    mut video_decode: PlayerVideoDecodeInfo,
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
) -> PlayerVideoDecodeInfo {
    if options.decoder_plugin_video_mode != PlayerDecoderPluginVideoMode::PreferNativeFrame
        || video_decode.selected_mode == PlayerVideoDecodeMode::Hardware
    {
        return video_decode;
    }

    let Some(best_video) = media_info.best_video.as_ref() else {
        return video_decode;
    };

    let reason = if options.decoder_plugin_library_paths.is_empty() {
        Some(format!(
            "native-frame decoder plugin playback requested for {} video but no decoder plugin paths are configured; selected FFmpeg software path",
            best_video.codec
        ))
    } else if options.video_surface.is_none() {
        Some(format!(
            "native-frame decoder plugin playback requested for {} video but no macOS video surface is available; selected FFmpeg software path",
            best_video.codec
        ))
    } else {
        let request = DecoderPluginMatchRequest::video(best_video.codec.clone());
        let registry = PluginRegistry::inspect_decoder_support(
            &options.decoder_plugin_library_paths,
            request.clone(),
        );
        (!registry.supports_native_decoder(&request)).then(|| {
            format!(
                "native-frame decoder plugin playback requested for {} video but no matching native-frame decoder is available; selected FFmpeg software path",
                best_video.codec
            )
        })
    };

    if let Some(reason) = reason {
        video_decode.fallback_reason = Some(match video_decode.fallback_reason.take() {
            Some(existing) if !existing.is_empty() => format!("{existing}; {reason}"),
            _ => reason,
        });
    }

    video_decode
}

pub(crate) fn decoder_plugin_registry(
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
) -> Option<PluginRegistry> {
    let best_video = media_info.best_video.as_ref()?;
    if options.decoder_plugin_library_paths.is_empty() {
        return None;
    }
    Some(PluginRegistry::inspect_decoder_support(
        &options.decoder_plugin_library_paths,
        DecoderPluginMatchRequest::video(best_video.codec.clone()),
    ))
}

pub(crate) fn frame_processor_plugin_registry(
    options: &PlayerRuntimeOptions,
) -> Option<PluginRegistry> {
    if options.frame_processor_mode == FrameProcessorMode::Disabled
        || options.frame_processor_library_paths.is_empty()
    {
        return None;
    }
    Some(PluginRegistry::inspect_frame_processor_support(
        &options.frame_processor_library_paths,
    ))
}

pub(crate) fn decoder_plugin_diagnostic(
    media_info: &PlayerMediaInfo,
    registry: &PluginRegistry,
) -> Option<String> {
    let best_video = media_info.best_video.as_ref()?;
    let request = DecoderPluginMatchRequest::video(best_video.codec.clone());
    let report = registry.report();
    let supported_plugins = decoder_plugin_supported_labels(registry);

    if registry.supports_decoder(&request) {
        return Some(format!(
            "decoder plugin found {}/{} candidate(s) for {} video: {}; diagnostic-only, playback still uses native-first/FFmpeg fallback",
            report.decoder_supported,
            report.total,
            best_video.codec,
            supported_plugins.join(", ")
        ));
    }

    let compact_notes = decoder_plugin_compact_notes(registry);
    Some(format!(
        "decoder plugin paths configured for {} video: {}/{} supported, {} unsupported codec, {} load failed, {} non-decoder{}",
        best_video.codec,
        report.decoder_supported,
        report.total,
        report.decoder_unsupported,
        report.failed,
        report.unsupported_kind,
        if compact_notes.is_empty() {
            String::new()
        } else {
            format!(" ({})", compact_notes.join("; "))
        }
    ))
}

pub(crate) fn decoder_plugin_supported_labels(registry: &PluginRegistry) -> Vec<String> {
    registry
        .records()
        .iter()
        .filter(|record| record.status == PluginDiagnosticStatus::DecoderSupported)
        .map(|record| {
            let name = record.plugin_name.as_deref().unwrap_or("unknown-decoder");
            if matches!(
                record.capability_summary.as_ref(),
                Some(PluginCapabilitySummary::Decoder(capabilities))
                    if capabilities.supports_native_frame_output
            ) {
                format!("{name} native-frame")
            } else {
                name.to_owned()
            }
        })
        .collect()
}

pub(crate) fn decoder_plugin_compact_notes(registry: &PluginRegistry) -> Vec<String> {
    let mut notes = Vec::new();
    let failed_paths = registry
        .records()
        .iter()
        .filter(|record| record.status == PluginDiagnosticStatus::LoadFailed)
        .map(|record| record.path.display().to_string())
        .collect::<Vec<_>>();
    if !failed_paths.is_empty() {
        notes.push(format!("load failed: {}", failed_paths.join(", ")));
    }

    let unsupported_codecs = registry
        .records()
        .iter()
        .filter(|record| record.status == PluginDiagnosticStatus::DecoderUnsupported)
        .map(plugin_diagnostic_label)
        .collect::<Vec<_>>();
    if !unsupported_codecs.is_empty() {
        notes.push(format!(
            "unsupported codec: {}",
            unsupported_codecs.join(", ")
        ));
    }

    let non_decoders = registry
        .records()
        .iter()
        .filter(|record| record.status == PluginDiagnosticStatus::UnsupportedKind)
        .map(plugin_diagnostic_label)
        .collect::<Vec<_>>();
    if !non_decoders.is_empty() {
        notes.push(format!("non-decoder: {}", non_decoders.join(", ")));
    }

    notes
}

pub(crate) fn plugin_diagnostic_label(record: &PluginDiagnosticRecord) -> String {
    record
        .plugin_name
        .clone()
        .unwrap_or_else(|| record.path.display().to_string())
}

pub(crate) fn player_plugin_diagnostic_from_record(
    record: &PluginDiagnosticRecord,
    participation: PlayerPluginParticipation,
) -> PlayerPluginDiagnostic {
    PlayerPluginDiagnostic {
        path: record.path.display().to_string(),
        plugin_name: record.plugin_name.clone(),
        plugin_kind: record.plugin_kind.map(plugin_kind_label).map(str::to_owned),
        status: match record.status {
            PluginDiagnosticStatus::Loaded => PlayerPluginDiagnosticStatus::Loaded,
            PluginDiagnosticStatus::LoadFailed => PlayerPluginDiagnosticStatus::LoadFailed,
            PluginDiagnosticStatus::UnsupportedKind => {
                PlayerPluginDiagnosticStatus::UnsupportedKind
            }
            PluginDiagnosticStatus::DecoderSupported => {
                PlayerPluginDiagnosticStatus::DecoderSupported
            }
            PluginDiagnosticStatus::DecoderUnsupported => {
                PlayerPluginDiagnosticStatus::DecoderUnsupported
            }
            PluginDiagnosticStatus::FrameProcessorSupported => {
                PlayerPluginDiagnosticStatus::FrameProcessorSupported
            }
            PluginDiagnosticStatus::FrameProcessorUnsupported => {
                PlayerPluginDiagnosticStatus::FrameProcessorUnsupported
            }
            PluginDiagnosticStatus::SourceNormalizerSupported => {
                PlayerPluginDiagnosticStatus::SourceNormalizerSupported
            }
            PluginDiagnosticStatus::SourceNormalizerUnsupported => {
                PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported
            }
        },
        message: record.message.clone(),
        capability: record
            .capability_summary
            .as_ref()
            .and_then(player_plugin_capability_summary_from_loader),
        participation,
    }
}

pub(crate) fn selected_decoder_plugin_name(
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
    registry: &PluginRegistry,
) -> Option<String> {
    if options.decoder_plugin_video_mode != PlayerDecoderPluginVideoMode::PreferNativeFrame
        || options.video_surface.is_none()
    {
        return None;
    }
    let best_video = media_info.best_video.as_ref()?;
    registry
        .best_native_decoder_for(&DecoderPluginMatchRequest::video(best_video.codec.clone()))
        .and_then(|record| record.plugin_name.clone())
}

pub(crate) fn decoder_plugin_participation(
    record: &PluginDiagnosticRecord,
    selected_decoder: Option<&str>,
    options: &PlayerRuntimeOptions,
) -> PlayerPluginParticipation {
    if record.status != PluginDiagnosticStatus::DecoderSupported {
        return PlayerPluginParticipation::Unknown;
    }
    if selected_decoder.is_some_and(|selected| record.plugin_name.as_deref() == Some(selected)) {
        return PlayerPluginParticipation::Participated;
    }
    if options.decoder_plugin_video_mode == PlayerDecoderPluginVideoMode::PreferNativeFrame {
        PlayerPluginParticipation::Bypassed
    } else {
        PlayerPluginParticipation::Available
    }
}

pub(crate) fn frame_processor_plugin_participation(
    record: &PluginDiagnosticRecord,
) -> PlayerPluginParticipation {
    if record.status == PluginDiagnosticStatus::FrameProcessorSupported {
        PlayerPluginParticipation::Available
    } else {
        PlayerPluginParticipation::Unknown
    }
}

pub(crate) fn source_normalizer_plugin_participation(
    record: &PluginDiagnosticRecord,
) -> PlayerPluginParticipation {
    if record.status == PluginDiagnosticStatus::SourceNormalizerSupported {
        PlayerPluginParticipation::Available
    } else {
        PlayerPluginParticipation::Unknown
    }
}

pub(crate) fn player_plugin_capability_summary_from_loader(
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

pub(crate) fn player_source_normalizer_capability_summary_from_loader(
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

pub(crate) fn player_source_normalizer_resource_capability_summary_from_loader(
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

pub(crate) fn player_decoder_capability_summary_from_loader(
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
        supports_audio_frames: summary.supports_audio_frames,
        supports_gpu_handles: summary.supports_gpu_handles,
        supports_flush: summary.supports_flush,
        supports_drain: summary.supports_drain,
        max_sessions: summary.max_sessions,
    }
}

pub(crate) fn player_decoder_codec_summary_from_loader(
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

pub(crate) fn player_frame_processor_capability_summary_from_loader(
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

pub(crate) fn native_handle_kind_label(handle_kind: &NativeHandleKind) -> String {
    match handle_kind {
        NativeHandleKind::CvPixelBuffer => "cv_pixel_buffer".to_owned(),
        NativeHandleKind::IoSurface => "io_surface".to_owned(),
        NativeHandleKind::MetalTexture => "metal_texture".to_owned(),
        NativeHandleKind::DmaBuf => "dma_buf".to_owned(),
        NativeHandleKind::VaapiSurface => "vaapi_surface".to_owned(),
        NativeHandleKind::D3D11Texture2D => "d3d11_texture_2d".to_owned(),
        NativeHandleKind::DxgiSurface => "dxgi_surface".to_owned(),
        NativeHandleKind::VulkanImage => "vulkan_image".to_owned(),
        NativeHandleKind::Unknown(name) => name.clone(),
    }
}

pub(crate) fn plugin_kind_label(kind: VesperPluginKind) -> &'static str {
    match kind {
        VesperPluginKind::PostDownloadProcessor => "post_download_processor",
        VesperPluginKind::PipelineEventHook => "pipeline_event_hook",
        VesperPluginKind::Decoder => "decoder",
        VesperPluginKind::BenchmarkSink => "benchmark_sink",
        VesperPluginKind::FrameProcessor => "frame_processor",
        VesperPluginKind::SourceNormalizer => "source_normalizer",
    }
}
