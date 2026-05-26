use super::*;

pub(crate) fn strict_frame_processor_selection(
    selection: &MacosNativeFrameDecoderSelection,
) -> bool {
    selection.frame_processor_mode == FrameProcessorMode::RequireProcessed
        && !selection.frame_processor_paths.is_empty()
}

#[derive(Debug, Clone)]
pub(crate) struct MacosNativeFrameDecoderSelection {
    pub(crate) plugin_path: PathBuf,
    pub(crate) plugin_name: Option<String>,
    pub(crate) video_surface: PlayerVideoSurfaceTarget,
    pub(crate) frame_processor_paths: Vec<PathBuf>,
    pub(crate) frame_processor_mode: FrameProcessorMode,
    pub(crate) frame_processor_policy: FrameProcessorPolicy,
}

pub(crate) fn select_macos_native_frame_decoder(
    source: &MediaSource,
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
    interrupt_flag: Option<Arc<AtomicBool>>,
) -> Option<MacosNativeFrameDecoderSelection> {
    if options.decoder_plugin_video_mode != PlayerDecoderPluginVideoMode::PreferNativeFrame {
        return None;
    }
    let video_surface = options.video_surface?;
    if options.decoder_plugin_library_paths.is_empty() {
        return None;
    }
    let codec =
        native_frame_decoder_codec(source, media_info, interrupt_flag).unwrap_or_else(|| {
            media_info
                .best_video
                .as_ref()
                .map(|video| video.codec.clone())
                .unwrap_or_default()
        });
    if codec.is_empty() {
        return None;
    }
    let request = DecoderPluginMatchRequest::video(codec);
    let registry = PluginRegistry::inspect_decoder_support(
        &options.decoder_plugin_library_paths,
        request.clone(),
    );
    let record = registry.best_native_decoder_for(&request)?;
    let requirements = match record.capability_summary.as_ref() {
        Some(PluginCapabilitySummary::Decoder(capabilities)) => {
            capabilities.native_requirements.as_ref()
        }
        _ => None,
    };
    if requirements.is_some_and(|requirements| {
        requirements.requires_native_device_context
            || (!requirements.output_handle_kinds.is_empty()
                && !requirements
                    .output_handle_kinds
                    .contains(&DecoderNativeHandleKind::CvPixelBuffer))
    }) {
        return None;
    }
    Some(MacosNativeFrameDecoderSelection {
        plugin_path: record.path.clone(),
        plugin_name: record.plugin_name.clone(),
        video_surface,
        frame_processor_paths: if options.frame_processor_mode == FrameProcessorMode::Disabled {
            Vec::new()
        } else {
            options.frame_processor_library_paths.clone()
        },
        frame_processor_mode: options.frame_processor_mode,
        frame_processor_policy: options.frame_processor_policy.clone(),
    })
}

pub(crate) fn select_macos_source_normalizer_packet_decoder(
    stream_info: Option<&player_plugin::SourceNormalizerPacketStreamInfo>,
    options: &PlayerRuntimeOptions,
) -> Option<MacosNativeFrameDecoderSelection> {
    if options.decoder_plugin_video_mode != PlayerDecoderPluginVideoMode::PreferNativeFrame {
        return None;
    }
    let video_surface = options.video_surface?;
    if options.decoder_plugin_library_paths.is_empty() {
        return None;
    }
    let stream_info = stream_info?;
    let video_stream = match macos_packet_stream_info_from_source_normalizer(stream_info) {
        Ok(video_stream) => video_stream,
        Err(error) => {
            tracing::debug!(
                error = %error,
                "source normalizer stream info could not be converted for decoder selection"
            );
            return None;
        }
    };
    if video_stream.codec.is_empty() {
        return None;
    }
    let request = DecoderPluginMatchRequest::video(video_stream.codec);
    let registry = PluginRegistry::inspect_decoder_support(
        &options.decoder_plugin_library_paths,
        request.clone(),
    );
    let record = registry.best_native_decoder_for(&request)?;
    let requirements = match record.capability_summary.as_ref() {
        Some(PluginCapabilitySummary::Decoder(capabilities)) => {
            capabilities.native_requirements.as_ref()
        }
        _ => None,
    };
    if requirements.is_some_and(|requirements| {
        requirements.requires_native_device_context
            || (!requirements.output_handle_kinds.is_empty()
                && !requirements
                    .output_handle_kinds
                    .contains(&DecoderNativeHandleKind::CvPixelBuffer))
    }) {
        return None;
    }
    Some(MacosNativeFrameDecoderSelection {
        plugin_path: record.path.clone(),
        plugin_name: record.plugin_name.clone(),
        video_surface,
        frame_processor_paths: if options.frame_processor_mode == FrameProcessorMode::Disabled {
            Vec::new()
        } else {
            options.frame_processor_library_paths.clone()
        },
        frame_processor_mode: options.frame_processor_mode,
        frame_processor_policy: options.frame_processor_policy.clone(),
    })
}

pub(crate) fn native_frame_decoder_codec(
    source: &MediaSource,
    media_info: &PlayerMediaInfo,
    interrupt_flag: Option<Arc<AtomicBool>>,
) -> Option<String> {
    if let Some(best_video) = media_info.best_video.as_ref() {
        return Some(best_video.codec.clone());
    }
    if source.protocol() != MediaSourceProtocol::Hls {
        return None;
    }

    let backend = match FfmpegBackend::new() {
        Ok(backend) => backend,
        Err(error) => {
            tracing::debug!(error = %error, "failed to initialize FFmpeg backend for codec probing");
            return None;
        }
    };
    match backend.open_video_packet_source_with_interrupt(source.clone(), interrupt_flag) {
        Ok(packet_source) => Some(packet_source.stream_info().codec.clone()),
        Err(error) => {
            tracing::debug!(error = %error, "failed to open FFmpeg packet source for codec probing");
            None
        }
    }
}

pub(crate) fn macos_decoder_bitstream_format(codec: &str) -> DecoderBitstreamFormat {
    match codec.to_ascii_uppercase().as_str() {
        "HEVC" | "H265" | "HVC1" | "HEV1" => DecoderBitstreamFormat::Hvcc,
        _ => DecoderBitstreamFormat::Avcc,
    }
}

pub(crate) fn macos_native_frame_decoder_video_decode_info(
    plugin_name: Option<&str>,
) -> PlayerVideoDecodeInfo {
    PlayerVideoDecodeInfo {
        selected_mode: PlayerVideoDecodeMode::Hardware,
        hardware_available: true,
        hardware_backend: Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned()),
        fallback_reason: plugin_name.map(|name| {
            format!("decoder plugin `{name}` selected for native-frame VideoToolbox playback")
        }),
    }
}

pub(crate) fn macos_native_frame_decoder_capabilities() -> PlayerRuntimeAdapterCapabilities {
    PlayerRuntimeAdapterCapabilities {
        adapter_id: MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
        backend_family: PlayerRuntimeAdapterBackendFamily::SoftwareDesktop,
        supports_audio_output: true,
        supports_frame_output: false,
        supports_external_video_surface: true,
        supports_seek: true,
        supports_stop: true,
        supports_playback_rate: true,
        playback_rate_min: Some(player_runtime::MIN_PLAYBACK_RATE),
        playback_rate_max: Some(player_runtime::MAX_PLAYBACK_RATE),
        natural_playback_rate_max: Some(player_runtime::NATURAL_PLAYBACK_RATE_MAX),
        supports_hardware_decode: true,
        supports_streaming: true,
        supports_hdr: true,
    }
}

pub(crate) fn duration_from_micros(value: i64) -> Option<Duration> {
    if value < 0 {
        return None;
    }
    Some(Duration::from_micros(value as u64))
}
