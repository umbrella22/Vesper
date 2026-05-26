use super::*;

pub(crate) fn should_trigger_runtime_fallback_for_advance(error: &PlayerError) -> bool {
    if error.code() != PlayerErrorCode::BackendFailure {
        return false;
    }
    let message = error.message().to_ascii_lowercase();
    message.contains("failed to present decoded video frame")
        || message.contains("failed to present seeked video frame")
        || message.contains("present")
        || message.contains("native-frame decoder")
        || message.contains("videotoolbox")
}

pub(crate) fn should_trigger_runtime_fallback_for_command(
    command: &PlayerRuntimeCommand,
    error: &PlayerError,
) -> bool {
    if error.code() != PlayerErrorCode::BackendFailure {
        return false;
    }
    let message = error.message().to_ascii_lowercase();
    match command {
        PlayerRuntimeCommand::SeekTo { .. } => {
            message.contains("seek") || message.contains("present")
        }
        PlayerRuntimeCommand::Play => message.contains("play") || message.contains("present"),
        PlayerRuntimeCommand::SetPlaybackRate { .. } => {
            message.contains("rate") || message.contains("present")
        }
        _ => false,
    }
}

pub(crate) fn should_prefer_native_host_runtime(
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
) -> bool {
    if should_route_macos_host_to_decoder_plugin_path(media_info, options) {
        return false;
    }
    options.video_surface.is_some() || media_info.best_video.is_none()
}

pub(crate) fn should_route_macos_host_to_decoder_plugin_path(
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
) -> bool {
    media_info.best_video.is_some()
        && options.decoder_plugin_video_mode == PlayerDecoderPluginVideoMode::PreferNativeFrame
}

pub(crate) fn macos_host_software_path_reason(
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
) -> Option<String> {
    let best_video = media_info.best_video.as_ref()?;
    if should_route_macos_host_to_decoder_plugin_path(media_info, options) {
        return Some(format!(
            "native-frame decoder plugin playback requested for {} video; selected desktop decoder plugin path",
            best_video.codec
        ));
    }
    Some(format!(
        "macos native host runtime requires an external video surface for {} playback; selected software desktop path",
        best_video.codec
    ))
}

pub(crate) fn probe_macos_host_runtime_initializer_with_factories(
    source: MediaSource,
    options: PlayerRuntimeOptions,
    native_factory: &dyn PlayerRuntimeAdapterFactory,
    software_fallback_factory: Arc<dyn MacosHostFallbackFactory>,
) -> PlayerResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
    match native_factory.probe_source_with_options(source.clone(), options.clone()) {
        Ok(initializer) => {
            let capabilities = initializer.capabilities();
            let media_info = initializer.media_info();
            let startup =
                apply_decoder_plugin_diagnostics(initializer.startup(), &media_info, &options);

            if should_prefer_native_host_runtime(&media_info, &options) {
                Ok(Box::new(MacosHostRuntimeAdapterInitializer {
                    selection: MacosHostRuntimeSelection::NativePreferred {
                        initializer,
                        source,
                        options,
                        software_fallback_factory,
                    },
                    capabilities,
                    media_info,
                    startup,
                }))
            } else {
                let fallback_reason = macos_host_software_path_reason(&media_info, &options);
                probe_software_fallback_initializer(
                    source,
                    options,
                    software_fallback_factory.as_ref(),
                    fallback_reason,
                )
            }
        }
        Err(native_error) => probe_software_fallback_initializer(
            source,
            options,
            software_fallback_factory.as_ref(),
            Some(format!(
                "macos native host runtime probe failed; selected software desktop path: {}",
                native_error.message()
            )),
        ),
    }
}

pub(crate) fn probe_software_fallback_initializer(
    source: MediaSource,
    options: PlayerRuntimeOptions,
    software_factory: &dyn MacosHostFallbackFactory,
    fallback_reason: Option<String>,
) -> PlayerResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
    let initializer = software_factory.probe_source_with_options(source, options.clone())?;
    let capabilities = initializer.capabilities();
    let media_info = initializer.media_info();
    let mut startup = initializer.startup();
    apply_video_decode_fallback_reason(&mut startup, fallback_reason);
    startup = apply_decoder_plugin_diagnostics(startup, &media_info, &options);

    Ok(Box::new(MacosHostRuntimeAdapterInitializer {
        selection: MacosHostRuntimeSelection::SoftwarePreferred { initializer },
        capabilities,
        media_info,
        startup,
    }))
}

pub(crate) fn apply_video_decode_fallback_reason(
    startup: &mut PlayerRuntimeStartup,
    fallback_reason: Option<String>,
) {
    if let (Some(video_decode), Some(fallback_reason)) =
        (startup.video_decode.as_mut(), fallback_reason)
    {
        video_decode.fallback_reason = Some(match video_decode.fallback_reason.take() {
            Some(existing) if !existing.is_empty() => format!("{fallback_reason}; {existing}"),
            _ => fallback_reason,
        });
    }
}

pub(crate) fn open_software_fallback_runtime(
    source: MediaSource,
    options: PlayerRuntimeOptions,
    fallback_reason: Option<String>,
    normalization: MacosSourceNormalizationOutcome,
) -> PlayerResult<PlayerRuntimeBootstrap> {
    let forward_strict_frame_processor_error = strict_frame_processor_fallback_enabled(&options);
    let open_options = without_source_normalizer_options(options);
    match PlayerRuntime::open_source_with_factory(
        source,
        open_options,
        macos_runtime_adapter_factory(),
    ) {
        Ok(mut bootstrap) => {
            if let Some(fallback_reason) = fallback_reason
                && let Some(video_decode) = bootstrap.startup.video_decode.as_mut()
            {
                video_decode.fallback_reason = Some(match video_decode.fallback_reason.take() {
                    Some(existing) if !existing.is_empty() => {
                        format!("{fallback_reason}; {existing}")
                    }
                    _ => fallback_reason,
                });
            }
            bootstrap.startup =
                apply_source_normalizer_open_diagnostics(bootstrap.startup, &normalization);
            Ok(attach_source_normalizer_to_runtime(
                bootstrap,
                normalization,
            ))
        }
        Err(software_error) => match fallback_reason {
            Some(fallback_reason) => {
                if should_forward_strict_frame_processor_fallback_error(
                    forward_strict_frame_processor_error,
                    &software_error,
                ) {
                    return Err(software_error);
                }
                Err(PlayerError::new(
                    PlayerErrorCode::BackendFailure,
                    format!(
                        "macos native host playback failed and software fallback also failed: native={}, software={}",
                        fallback_reason,
                        software_error.message()
                    ),
                ))
            }
            None => Err(software_error),
        },
    }
}

pub(crate) fn open_software_fallback_runtime_with_interrupt(
    source: MediaSource,
    options: PlayerRuntimeOptions,
    interrupt_flag: Arc<AtomicBool>,
    fallback_reason: Option<String>,
    normalization: MacosSourceNormalizationOutcome,
) -> PlayerResult<PlayerRuntimeBootstrap> {
    let forward_strict_frame_processor_error = strict_frame_processor_fallback_enabled(&options);
    let open_options = without_source_normalizer_options(options);
    match open_macos_software_runtime_source_with_options_and_interrupt(
        source,
        open_options,
        interrupt_flag,
    ) {
        Ok(mut bootstrap) => {
            apply_video_decode_fallback_reason(&mut bootstrap.startup, fallback_reason);
            bootstrap.startup =
                apply_source_normalizer_open_diagnostics(bootstrap.startup, &normalization);
            Ok(attach_source_normalizer_to_runtime(
                bootstrap,
                normalization,
            ))
        }
        Err(software_error) => match fallback_reason {
            Some(fallback_reason) => {
                if should_forward_strict_frame_processor_fallback_error(
                    forward_strict_frame_processor_error,
                    &software_error,
                ) {
                    return Err(software_error);
                }
                Err(PlayerError::new(
                    PlayerErrorCode::BackendFailure,
                    format!(
                        "macos native host playback failed and software fallback also failed: native={}, software={}",
                        fallback_reason,
                        software_error.message()
                    ),
                ))
            }
            None => Err(software_error),
        },
    }
}

pub(crate) fn open_software_fallback_adapter_with_factory(
    source: MediaSource,
    options: PlayerRuntimeOptions,
    software_factory: &dyn MacosHostFallbackFactory,
    fallback_reason: Option<String>,
) -> PlayerResult<PlayerRuntimeAdapterBootstrap> {
    let forward_strict_frame_processor_error = strict_frame_processor_fallback_enabled(&options);
    let initializer = software_factory.probe_source_with_options(source, options)?;
    let mut startup = initializer.startup();
    apply_video_decode_fallback_reason(&mut startup, fallback_reason);
    let mut bootstrap = match initializer.initialize() {
        Ok(bootstrap) => bootstrap,
        Err(software_error)
            if should_forward_strict_frame_processor_fallback_error(
                forward_strict_frame_processor_error,
                &software_error,
            ) =>
        {
            return Err(software_error);
        }
        Err(software_error) => return Err(software_error),
    };
    bootstrap.startup = startup;
    Ok(bootstrap)
}

pub(crate) fn strict_frame_processor_fallback_enabled(options: &PlayerRuntimeOptions) -> bool {
    options.frame_processor_mode == FrameProcessorMode::RequireProcessed
        && !options.frame_processor_library_paths.is_empty()
}

pub(crate) fn without_source_normalizer_options(
    mut options: PlayerRuntimeOptions,
) -> PlayerRuntimeOptions {
    options.source_normalizer_mode = SourceNormalizerMode::Disabled;
    options.source_normalizer_plugin_library_paths.clear();
    options
}

pub(crate) fn should_forward_strict_frame_processor_fallback_error(
    strict_frame_processor_fallback: bool,
    error: &PlayerError,
) -> bool {
    strict_frame_processor_fallback
        && error.code() == PlayerErrorCode::BackendFailure
        && error
            .message()
            .contains("frame processor initialization failed in strict mode")
}
