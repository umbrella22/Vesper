use super::*;

pub(crate) use player_platform_desktop::diagnostics::{
    append_plugin_diagnostics, mark_plugin_diagnostics_fallback,
    player_plugin_diagnostic_from_record, plugin_diagnostic_label,
};

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
        video_decode = apply_decoder_plugin_registry_to_video_decode(
            video_decode,
            media_info,
            &registry,
            selected_decoder.as_deref(),
        );
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
        Some(format!(
            "system VideoToolbox hardware decode support detected; Apple platforms should prefer the {} route, while the {} route remains available as fallback",
            PlayerPlaybackRoute::SystemPlayer.wire_name(),
            PlayerPlaybackRoute::SoftwareDecoder.wire_name()
        ))
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
                selected_decoder.as_deref(),
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
    let selected_decoder = selected_decoder_plugin_name(media_info, options, &registry);
    apply_decoder_plugin_registry_to_video_decode(
        video_decode,
        media_info,
        &registry,
        selected_decoder.as_deref(),
    )
}

pub(crate) fn apply_decoder_plugin_registry_to_video_decode(
    mut video_decode: PlayerVideoDecodeInfo,
    media_info: &PlayerMediaInfo,
    registry: &PluginRegistry,
    selected_decoder: Option<&str>,
) -> PlayerVideoDecodeInfo {
    if video_decode
        .fallback_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("decoder plugin"))
    {
        return video_decode;
    }

    if let Some(diagnostic) = decoder_plugin_diagnostic(media_info, registry, selected_decoder) {
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
            "native-frame decoder plugin playback requested for {} video but no decoder plugin paths are configured; selected {} route",
            best_video.codec,
            PlayerPlaybackRoute::SoftwareDecoder.wire_name()
        ))
    } else if options.video_surface.is_none() {
        Some(format!(
            "native-frame decoder plugin playback requested for {} video but no macOS video surface is available; selected {} route",
            best_video.codec,
            PlayerPlaybackRoute::SoftwareDecoder.wire_name()
        ))
    } else if let Err(error) =
        options.validate_native_plugin_loading_policy("macOS native-frame decoder")
    {
        Some(format!(
            "{error}; selected {} route",
            PlayerPlaybackRoute::SoftwareDecoder.wire_name()
        ))
    } else {
        let request = DecoderPluginMatchRequest::video(best_video.codec.clone());
        let registry = PluginRegistry::inspect_decoder_support_development(
            &options.decoder_plugin_library_paths,
            request.clone(),
        );
        (!registry.supports_native_decoder(&request)).then(|| {
            format!(
                "native-frame decoder plugin playback requested for {} video but no matching native-frame decoder is available; selected {} route",
                best_video.codec,
                PlayerPlaybackRoute::SoftwareDecoder.wire_name()
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
    if let Err(error) = options.validate_native_plugin_loading_policy("macOS decoder diagnostics") {
        tracing::warn!(error = %error);
        return None;
    }
    Some(PluginRegistry::inspect_decoder_support_development(
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
    if let Err(error) =
        options.validate_native_plugin_loading_policy("macOS frame processor diagnostics")
    {
        tracing::warn!(error = %error);
        return None;
    }
    Some(PluginRegistry::inspect_frame_processor_support_development(
        &options.frame_processor_library_paths,
    ))
}

pub(crate) fn decoder_plugin_diagnostic(
    media_info: &PlayerMediaInfo,
    registry: &PluginRegistry,
    selected_decoder: Option<&str>,
) -> Option<String> {
    let best_video = media_info.best_video.as_ref()?;
    let request = DecoderPluginMatchRequest::video(best_video.codec.clone());
    let report = registry.report();
    let supported_plugins = decoder_plugin_supported_labels(registry);

    if registry.supports_decoder(&request) {
        let route_note = selected_decoder
            .map(|decoder| {
                let audio_note = macos_audio_decoder_readiness_note(media_info, registry);
                format!(
                    "selected {} route via {decoder}; {}; audioOutput=none; clockSource=video; presenter=macOS MetalLayer",
                    PlayerPlaybackRoute::SdkManagedNativeFrame.wire_name(),
                    audio_note
                )
            })
            .unwrap_or_else(|| {
                format!(
                    "available for {} route; diagnostic-only until native-frame mode and surface select it",
                    PlayerPlaybackRoute::SdkManagedNativeFrame.wire_name()
                )
            });
        return Some(format!(
            "decoder plugin found {}/{} candidate(s) for {} video: {}; {}",
            report.decoder_supported,
            report.total,
            best_video.codec,
            supported_plugins.join(", "),
            route_note
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

pub(crate) fn macos_audio_decoder_readiness_note(
    media_info: &PlayerMediaInfo,
    registry: &PluginRegistry,
) -> String {
    let Some(audio) = media_info.best_audio.as_ref() else {
        return "audioTrack=none; audioDecoderPlugin=none; audioDecoderPluginReady=false; audioDecoder=none".to_owned();
    };
    let request = DecoderPluginMatchRequest::audio(audio.codec.clone());
    if let Some(record) = registry.best_pcm_audio_decoder_for(&request) {
        let plugin = record.plugin_name.as_deref().unwrap_or("unknown-decoder");
        return format!(
            "audioTrackCodec={}; audioDecoderPlugin={plugin}; audioDecoderPluginReady=true; audioDecoder=none",
            audio.codec
        );
    }
    format!(
        "audioTrackCodec={}; audioDecoderPlugin=none; audioDecoderPluginReady=false; audioDecoder=none",
        audio.codec
    )
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
