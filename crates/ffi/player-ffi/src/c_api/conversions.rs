use super::*;

impl From<FfiPlaybackState> for PlayerFfiPlaybackState {
    fn from(value: FfiPlaybackState) -> Self {
        match value {
            FfiPlaybackState::Ready => Self::Ready,
            FfiPlaybackState::Playing => Self::Playing,
            FfiPlaybackState::Paused => Self::Paused,
            FfiPlaybackState::Finished => Self::Finished,
        }
    }
}

impl From<BridgePixelFormat> for PlayerFfiPixelFormat {
    fn from(value: BridgePixelFormat) -> Self {
        match value {
            BridgePixelFormat::Rgba8888 => Self::Rgba8888,
            BridgePixelFormat::Yuv420p => Self::Yuv420p,
        }
    }
}

impl From<BridgeTimelineKind> for PlayerFfiTimelineKind {
    fn from(value: BridgeTimelineKind) -> Self {
        match value {
            BridgeTimelineKind::Vod => Self::Vod,
            BridgeTimelineKind::Live => Self::Live,
            BridgeTimelineKind::LiveDvr => Self::LiveDvr,
        }
    }
}

impl From<BridgeMediaSourceKind> for PlayerFfiMediaSourceKind {
    fn from(value: BridgeMediaSourceKind) -> Self {
        match value {
            BridgeMediaSourceKind::Local => Self::Local,
            BridgeMediaSourceKind::Remote => Self::Remote,
        }
    }
}

impl From<BridgeMediaSourceProtocol> for PlayerFfiMediaSourceProtocol {
    fn from(value: BridgeMediaSourceProtocol) -> Self {
        match value {
            BridgeMediaSourceProtocol::Unknown => Self::Unknown,
            BridgeMediaSourceProtocol::File => Self::File,
            BridgeMediaSourceProtocol::Content => Self::Content,
            BridgeMediaSourceProtocol::Progressive => Self::Progressive,
            BridgeMediaSourceProtocol::Hls => Self::Hls,
            BridgeMediaSourceProtocol::Dash => Self::Dash,
        }
    }
}

impl From<BridgeRuntimeWarningDomain> for PlayerFfiRuntimeWarningDomain {
    fn from(value: BridgeRuntimeWarningDomain) -> Self {
        match value {
            BridgeRuntimeWarningDomain::FrameProcessor => Self::FrameProcessor,
        }
    }
}

impl From<BridgeFrameProcessorWarningKind> for PlayerFfiFrameProcessorWarningKind {
    fn from(value: BridgeFrameProcessorWarningKind) -> Self {
        match value {
            BridgeFrameProcessorWarningKind::Slow => Self::Slow,
            BridgeFrameProcessorWarningKind::DeadlineMissed => Self::DeadlineMissed,
            BridgeFrameProcessorWarningKind::Backpressure => Self::Backpressure,
            BridgeFrameProcessorWarningKind::BypassActivated => Self::BypassActivated,
            BridgeFrameProcessorWarningKind::LateOutputDropped => Self::LateOutputDropped,
            BridgeFrameProcessorWarningKind::OutputDropped => Self::OutputDropped,
            BridgeFrameProcessorWarningKind::Disabled => Self::Disabled,
            BridgeFrameProcessorWarningKind::Recovered => Self::Recovered,
            BridgeFrameProcessorWarningKind::Unsupported => Self::Unsupported,
        }
    }
}

impl From<BridgeFrameProcessorPolicyAction> for PlayerFfiFrameProcessorPolicyAction {
    fn from(value: BridgeFrameProcessorPolicyAction) -> Self {
        match value {
            BridgeFrameProcessorPolicyAction::Continue => Self::Continue,
            BridgeFrameProcessorPolicyAction::BypassOriginalFrame => Self::BypassOriginalFrame,
            BridgeFrameProcessorPolicyAction::DropOutput => Self::DropOutput,
            BridgeFrameProcessorPolicyAction::DisableProcessor => Self::DisableProcessor,
            BridgeFrameProcessorPolicyAction::FailPlayback => Self::FailPlayback,
            BridgeFrameProcessorPolicyAction::DiagnosticsOnly => Self::DiagnosticsOnly,
        }
    }
}

impl From<PlayerFfiMediaSourceKind> for BridgeMediaSourceKind {
    fn from(value: PlayerFfiMediaSourceKind) -> Self {
        match value {
            PlayerFfiMediaSourceKind::Local => Self::Local,
            PlayerFfiMediaSourceKind::Remote => Self::Remote,
        }
    }
}

impl From<PlayerFfiMediaSourceProtocol> for BridgeMediaSourceProtocol {
    fn from(value: PlayerFfiMediaSourceProtocol) -> Self {
        match value {
            PlayerFfiMediaSourceProtocol::Unknown => Self::Unknown,
            PlayerFfiMediaSourceProtocol::File => Self::File,
            PlayerFfiMediaSourceProtocol::Content => Self::Content,
            PlayerFfiMediaSourceProtocol::Progressive => Self::Progressive,
            PlayerFfiMediaSourceProtocol::Hls => Self::Hls,
            PlayerFfiMediaSourceProtocol::Dash => Self::Dash,
        }
    }
}

impl From<BridgeBufferingPreset> for PlayerFfiBufferingPreset {
    fn from(value: BridgeBufferingPreset) -> Self {
        match value {
            BridgeBufferingPreset::Default => Self::Default,
            BridgeBufferingPreset::Balanced => Self::Balanced,
            BridgeBufferingPreset::Streaming => Self::Streaming,
            BridgeBufferingPreset::Resilient => Self::Resilient,
            BridgeBufferingPreset::LowLatency => Self::LowLatency,
        }
    }
}

impl From<PlayerFfiBufferingPreset> for BridgeBufferingPreset {
    fn from(value: PlayerFfiBufferingPreset) -> Self {
        match value {
            PlayerFfiBufferingPreset::Default => Self::Default,
            PlayerFfiBufferingPreset::Balanced => Self::Balanced,
            PlayerFfiBufferingPreset::Streaming => Self::Streaming,
            PlayerFfiBufferingPreset::Resilient => Self::Resilient,
            PlayerFfiBufferingPreset::LowLatency => Self::LowLatency,
        }
    }
}

impl From<BridgeBufferingPolicy> for PlayerFfiBufferingPolicy {
    fn from(value: BridgeBufferingPolicy) -> Self {
        Self {
            preset: value.preset.into(),
            has_min_buffer_ms: value.min_buffer_ms.is_some(),
            min_buffer_ms: value.min_buffer_ms.unwrap_or_default(),
            has_max_buffer_ms: value.max_buffer_ms.is_some(),
            max_buffer_ms: value.max_buffer_ms.unwrap_or_default(),
            has_buffer_for_playback_ms: value.buffer_for_playback_ms.is_some(),
            buffer_for_playback_ms: value.buffer_for_playback_ms.unwrap_or_default(),
            has_buffer_for_rebuffer_ms: value.buffer_for_rebuffer_ms.is_some(),
            buffer_for_rebuffer_ms: value.buffer_for_rebuffer_ms.unwrap_or_default(),
        }
    }
}

impl From<BridgeRetryBackoff> for PlayerFfiRetryBackoff {
    fn from(value: BridgeRetryBackoff) -> Self {
        match value {
            BridgeRetryBackoff::Fixed => Self::Fixed,
            BridgeRetryBackoff::Linear => Self::Linear,
            BridgeRetryBackoff::Exponential => Self::Exponential,
        }
    }
}

impl From<PlayerFfiRetryBackoff> for BridgeRetryBackoff {
    fn from(value: PlayerFfiRetryBackoff) -> Self {
        match value {
            PlayerFfiRetryBackoff::Fixed => Self::Fixed,
            PlayerFfiRetryBackoff::Linear => Self::Linear,
            PlayerFfiRetryBackoff::Exponential => Self::Exponential,
        }
    }
}

impl From<BridgeRetryPolicy> for PlayerFfiRetryPolicy {
    fn from(value: BridgeRetryPolicy) -> Self {
        Self {
            uses_default_max_attempts: false,
            has_max_attempts: value.max_attempts.is_some(),
            max_attempts: value.max_attempts.unwrap_or_default(),
            has_base_delay_ms: true,
            base_delay_ms: value.base_delay_ms,
            has_max_delay_ms: true,
            max_delay_ms: value.max_delay_ms,
            has_backoff: true,
            backoff: value.backoff.into(),
        }
    }
}

impl From<BridgeCachePreset> for PlayerFfiCachePreset {
    fn from(value: BridgeCachePreset) -> Self {
        match value {
            BridgeCachePreset::Default => Self::Default,
            BridgeCachePreset::Disabled => Self::Disabled,
            BridgeCachePreset::Streaming => Self::Streaming,
            BridgeCachePreset::Resilient => Self::Resilient,
        }
    }
}

impl From<PlayerFfiCachePreset> for BridgeCachePreset {
    fn from(value: PlayerFfiCachePreset) -> Self {
        match value {
            PlayerFfiCachePreset::Default => Self::Default,
            PlayerFfiCachePreset::Disabled => Self::Disabled,
            PlayerFfiCachePreset::Streaming => Self::Streaming,
            PlayerFfiCachePreset::Resilient => Self::Resilient,
        }
    }
}

impl From<BridgeCachePolicy> for PlayerFfiCachePolicy {
    fn from(value: BridgeCachePolicy) -> Self {
        Self {
            preset: value.preset.into(),
            has_max_memory_bytes: value.max_memory_bytes.is_some(),
            max_memory_bytes: value.max_memory_bytes.unwrap_or_default(),
            has_max_disk_bytes: value.max_disk_bytes.is_some(),
            max_disk_bytes: value.max_disk_bytes.unwrap_or_default(),
        }
    }
}

impl From<BridgeResolvedResiliencePolicy> for PlayerFfiResolvedResiliencePolicy {
    fn from(value: BridgeResolvedResiliencePolicy) -> Self {
        Self {
            buffering: value.buffering.into(),
            retry: value.retry.into(),
            cache: value.cache.into(),
        }
    }
}

impl From<BridgePreloadBudgetPolicy> for PlayerFfiPreloadBudgetPolicy {
    fn from(value: BridgePreloadBudgetPolicy) -> Self {
        Self {
            has_max_concurrent_tasks: value.max_concurrent_tasks.is_some(),
            max_concurrent_tasks: value.max_concurrent_tasks.unwrap_or_default(),
            has_max_memory_bytes: value.max_memory_bytes.is_some(),
            max_memory_bytes: value.max_memory_bytes.unwrap_or_default(),
            has_max_disk_bytes: value.max_disk_bytes.is_some(),
            max_disk_bytes: value.max_disk_bytes.unwrap_or_default(),
            has_warmup_window_ms: value.warmup_window_ms.is_some(),
            warmup_window_ms: value.warmup_window_ms.unwrap_or_default(),
        }
    }
}

impl From<BridgeResolvedPreloadBudgetPolicy> for PlayerFfiResolvedPreloadBudgetPolicy {
    fn from(value: BridgeResolvedPreloadBudgetPolicy) -> Self {
        Self {
            max_concurrent_tasks: value.max_concurrent_tasks,
            max_memory_bytes: value.max_memory_bytes,
            max_disk_bytes: value.max_disk_bytes,
            warmup_window_ms: value.warmup_window_ms,
        }
    }
}

impl From<BridgeTrackPreferences> for PlayerFfiTrackPreferences {
    fn from(value: BridgeTrackPreferences) -> Self {
        Self {
            preferred_audio_language: value
                .preferred_audio_language
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            preferred_subtitle_language: value
                .preferred_subtitle_language
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            select_subtitles_by_default: value.select_subtitles_by_default,
            select_undetermined_subtitle_language: value.select_undetermined_subtitle_language,
            audio_selection: value.audio_selection.into(),
            subtitle_selection: value.subtitle_selection.into(),
            abr_policy: value.abr_policy.into(),
        }
    }
}

impl From<BridgeTrackKind> for PlayerFfiTrackKind {
    fn from(value: BridgeTrackKind) -> Self {
        match value {
            BridgeTrackKind::Video => Self::Video,
            BridgeTrackKind::Audio => Self::Audio,
            BridgeTrackKind::Subtitle => Self::Subtitle,
        }
    }
}

impl From<BridgeTrackSelectionMode> for PlayerFfiTrackSelectionMode {
    fn from(value: BridgeTrackSelectionMode) -> Self {
        match value {
            BridgeTrackSelectionMode::Auto => Self::Auto,
            BridgeTrackSelectionMode::Disabled => Self::Disabled,
            BridgeTrackSelectionMode::Track => Self::Track,
        }
    }
}

impl From<BridgeAbrMode> for PlayerFfiAbrMode {
    fn from(value: BridgeAbrMode) -> Self {
        match value {
            BridgeAbrMode::Auto => Self::Auto,
            BridgeAbrMode::Constrained => Self::Constrained,
            BridgeAbrMode::FixedTrack => Self::FixedTrack,
        }
    }
}

impl From<BridgeErrorCode> for PlayerFfiErrorCode {
    fn from(value: BridgeErrorCode) -> Self {
        match value {
            BridgeErrorCode::InvalidArgument => Self::InvalidArgument,
            BridgeErrorCode::InvalidState => Self::InvalidState,
            BridgeErrorCode::InvalidSource => Self::InvalidSource,
            BridgeErrorCode::BackendFailure => Self::BackendFailure,
            BridgeErrorCode::AudioOutputUnavailable => Self::AudioOutputUnavailable,
            BridgeErrorCode::DecodeFailure => Self::DecodeFailure,
            BridgeErrorCode::SeekFailure => Self::SeekFailure,
            BridgeErrorCode::Unsupported => Self::Unsupported,
            BridgeErrorCode::CommandChannelClosed => Self::CommandChannelClosed,
            BridgeErrorCode::EventChannelClosed => Self::EventChannelClosed,
            BridgeErrorCode::Cancelled => Self::Cancelled,
            BridgeErrorCode::Timeout => Self::Timeout,
        }
    }
}

impl From<BridgeErrorCategory> for PlayerFfiErrorCategory {
    fn from(value: BridgeErrorCategory) -> Self {
        match value {
            BridgeErrorCategory::Input => Self::Input,
            BridgeErrorCategory::Source => Self::Source,
            BridgeErrorCategory::Network => Self::Network,
            BridgeErrorCategory::Decode => Self::Decode,
            BridgeErrorCategory::AudioOutput => Self::AudioOutput,
            BridgeErrorCategory::Playback => Self::Playback,
            BridgeErrorCategory::Capability => Self::Capability,
            BridgeErrorCategory::Platform => Self::Platform,
        }
    }
}

impl From<FfiVideoInfo> for PlayerFfiVideoInfo {
    fn from(value: FfiVideoInfo) -> Self {
        Self {
            codec: into_c_string_ptr(value.codec),
            width: value.width,
            height: value.height,
            has_frame_rate: value.frame_rate.is_some(),
            frame_rate: value.frame_rate.unwrap_or_default(),
        }
    }
}

impl From<FfiAudioInfo> for PlayerFfiAudioInfo {
    fn from(value: FfiAudioInfo) -> Self {
        Self {
            codec: into_c_string_ptr(value.codec),
            sample_rate: value.sample_rate,
            channels: value.channels,
        }
    }
}

impl From<BridgeTrack> for PlayerFfiTrack {
    fn from(value: BridgeTrack) -> Self {
        Self {
            id: into_c_string_ptr(value.id),
            kind: value.kind.into(),
            label: value
                .label
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            language: value
                .language
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            codec: value
                .codec
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            has_bit_rate: value.bit_rate.is_some(),
            bit_rate: value.bit_rate.unwrap_or_default(),
            has_width: value.width.is_some(),
            width: value.width.unwrap_or_default(),
            has_height: value.height.is_some(),
            height: value.height.unwrap_or_default(),
            has_frame_rate: value.frame_rate.is_some(),
            frame_rate: value.frame_rate.unwrap_or_default(),
            has_channels: value.channels.is_some(),
            channels: value.channels.unwrap_or_default(),
            has_sample_rate: value.sample_rate.is_some(),
            sample_rate: value.sample_rate.unwrap_or_default(),
            is_default: value.is_default,
            is_forced: value.is_forced,
        }
    }
}

impl From<BridgeTrackCatalog> for PlayerFfiTrackCatalog {
    fn from(value: BridgeTrackCatalog) -> Self {
        let tracks = value
            .tracks
            .into_iter()
            .map(PlayerFfiTrack::from)
            .collect::<Vec<_>>();
        let (tracks, len) = into_owned_struct_array(tracks);

        Self {
            tracks,
            len,
            adaptive_video: value.adaptive_video,
            adaptive_audio: value.adaptive_audio,
        }
    }
}

impl From<BridgeTrackSelection> for PlayerFfiTrackSelection {
    fn from(value: BridgeTrackSelection) -> Self {
        Self {
            mode: value.mode.into(),
            track_id: value
                .track_id
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
        }
    }
}

impl From<BridgeAbrPolicy> for PlayerFfiAbrPolicy {
    fn from(value: BridgeAbrPolicy) -> Self {
        Self {
            mode: value.mode.into(),
            track_id: value
                .track_id
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            has_max_bit_rate: value.max_bit_rate.is_some(),
            max_bit_rate: value.max_bit_rate.unwrap_or_default(),
            has_max_width: value.max_width.is_some(),
            max_width: value.max_width.unwrap_or_default(),
            has_max_height: value.max_height.is_some(),
            max_height: value.max_height.unwrap_or_default(),
        }
    }
}

impl From<BridgeTrackSelectionSnapshot> for PlayerFfiTrackSelectionSnapshot {
    fn from(value: BridgeTrackSelectionSnapshot) -> Self {
        Self {
            video: value.video.into(),
            audio: value.audio.into(),
            subtitle: value.subtitle.into(),
            abr_policy: value.abr_policy.into(),
        }
    }
}

impl From<BridgeMediaInfo> for PlayerFfiMediaInfo {
    fn from(value: BridgeMediaInfo) -> Self {
        Self {
            source_uri: into_c_string_ptr(value.source_uri),
            source_kind: value.source_kind.into(),
            source_protocol: value.source_protocol.into(),
            has_duration: value.duration_ms.is_some(),
            duration_ms: value.duration_ms.unwrap_or_default(),
            has_bit_rate: value.bit_rate.is_some(),
            bit_rate: value.bit_rate.unwrap_or_default(),
            audio_streams: value.audio_streams,
            video_streams: value.video_streams,
            has_best_video: value.best_video.is_some(),
            best_video: value
                .best_video
                .map(PlayerFfiVideoInfo::from)
                .unwrap_or_default(),
            has_best_audio: value.best_audio.is_some(),
            best_audio: value
                .best_audio
                .map(PlayerFfiAudioInfo::from)
                .unwrap_or_default(),
            track_catalog: value.track_catalog.into(),
            track_selection: value.track_selection.into(),
        }
    }
}

impl From<FfiAudioOutputInfo> for PlayerFfiAudioOutputInfo {
    fn from(value: FfiAudioOutputInfo) -> Self {
        Self {
            device_name: value
                .device_name
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            has_channels: value.channels.is_some(),
            channels: value.channels.unwrap_or_default(),
            has_sample_rate: value.sample_rate.is_some(),
            sample_rate: value.sample_rate.unwrap_or_default(),
            sample_format: value
                .sample_format
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
        }
    }
}

impl From<FfiDecodedAudioSummary> for PlayerFfiDecodedAudioSummary {
    fn from(value: FfiDecodedAudioSummary) -> Self {
        Self {
            channels: value.channels,
            sample_rate: value.sample_rate,
            duration_ms: value.duration_ms,
        }
    }
}

impl From<BridgeVideoDecodeMode> for PlayerFfiVideoDecodeMode {
    fn from(value: BridgeVideoDecodeMode) -> Self {
        match value {
            BridgeVideoDecodeMode::Software => Self::Software,
            BridgeVideoDecodeMode::Hardware => Self::Hardware,
        }
    }
}

impl From<BridgeVideoDecodeInfo> for PlayerFfiVideoDecodeInfo {
    fn from(value: BridgeVideoDecodeInfo) -> Self {
        Self {
            selected_mode: value.selected_mode.into(),
            hardware_available: value.hardware_available,
            hardware_backend: value
                .hardware_backend
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            fallback_reason: value
                .fallback_reason
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
        }
    }
}

impl From<BridgePluginDiagnosticStatus> for PlayerFfiPluginDiagnosticStatus {
    fn from(value: BridgePluginDiagnosticStatus) -> Self {
        match value {
            BridgePluginDiagnosticStatus::Loaded => Self::Loaded,
            BridgePluginDiagnosticStatus::LoadFailed => Self::LoadFailed,
            BridgePluginDiagnosticStatus::UnsupportedKind => Self::UnsupportedKind,
            BridgePluginDiagnosticStatus::DecoderSupported => Self::DecoderSupported,
            BridgePluginDiagnosticStatus::DecoderUnsupported => Self::DecoderUnsupported,
            BridgePluginDiagnosticStatus::FrameProcessorSupported => Self::FrameProcessorSupported,
            BridgePluginDiagnosticStatus::FrameProcessorUnsupported => {
                Self::FrameProcessorUnsupported
            }
            BridgePluginDiagnosticStatus::SourceNormalizerSupported => {
                Self::SourceNormalizerSupported
            }
            BridgePluginDiagnosticStatus::SourceNormalizerUnsupported => {
                Self::SourceNormalizerUnsupported
            }
        }
    }
}

impl From<crate::FfiPluginCodecCapability> for PlayerFfiPluginCodecCapability {
    fn from(value: crate::FfiPluginCodecCapability) -> Self {
        Self {
            media_kind: into_c_string_ptr(value.media_kind),
            codec: into_c_string_ptr(value.codec),
        }
    }
}

impl From<BridgePluginDecoderCapabilitySummary> for PlayerFfiPluginDecoderCapabilitySummary {
    fn from(value: BridgePluginDecoderCapabilitySummary) -> Self {
        let codecs = value
            .codecs
            .into_iter()
            .map(PlayerFfiPluginCodecCapability::from)
            .collect::<Vec<_>>();
        let (codecs, codecs_len) = into_owned_struct_array(codecs);
        let (legacy_codecs, legacy_codecs_len) = into_owned_c_string_array(value.legacy_codecs);
        Self {
            codecs,
            codecs_len,
            legacy_codecs,
            legacy_codecs_len,
            supports_native_frame_output: value.supports_native_frame_output,
            supports_hardware_decode: value.supports_hardware_decode,
            supports_cpu_video_frames: value.supports_cpu_video_frames,
            supports_audio_frames: value.supports_audio_frames,
            supports_gpu_handles: value.supports_gpu_handles,
            supports_flush: value.supports_flush,
            supports_drain: value.supports_drain,
            has_max_sessions: value.max_sessions.is_some(),
            max_sessions: value.max_sessions.unwrap_or_default(),
        }
    }
}

impl From<BridgePluginFrameProcessorCapabilitySummary>
    for PlayerFfiPluginFrameProcessorCapabilitySummary
{
    fn from(value: BridgePluginFrameProcessorCapabilitySummary) -> Self {
        let (accepted_input_handle_kinds, accepted_input_handle_kinds_len) =
            into_owned_c_string_array(value.accepted_input_handle_kinds);
        let (output_handle_kinds, output_handle_kinds_len) =
            into_owned_c_string_array(value.output_handle_kinds);
        Self {
            accepted_input_handle_kinds,
            accepted_input_handle_kinds_len,
            output_handle_kinds,
            output_handle_kinds_len,
            supports_video_frames: value.supports_video_frames,
            supports_in_place_passthrough: value.supports_in_place_passthrough,
            preserves_dimensions: value.preserves_dimensions,
            may_change_dimensions: value.may_change_dimensions,
            preserves_color_metadata: value.preserves_color_metadata,
            preserves_hdr_metadata: value.preserves_hdr_metadata,
            supports_flush: value.supports_flush,
            has_max_sessions: value.max_sessions.is_some(),
            max_sessions: value.max_sessions.unwrap_or_default(),
            has_max_in_flight_frames: value.max_in_flight_frames.is_some(),
            max_in_flight_frames: value.max_in_flight_frames.unwrap_or_default(),
        }
    }
}

impl From<BridgePluginSourceNormalizerCapabilitySummary>
    for PlayerFfiPluginSourceNormalizerCapabilitySummary
{
    fn from(value: BridgePluginSourceNormalizerCapabilitySummary) -> Self {
        let (supported_runtime_profiles, supported_runtime_profiles_len) =
            into_owned_c_string_array(value.supported_runtime_profiles);
        let (supported_output_routes, supported_output_routes_len) =
            into_owned_c_string_array(value.supported_output_routes);
        let (media_kinds, media_kinds_len) = into_owned_c_string_array(value.media_kinds);
        let (codecs, codecs_len) = into_owned_c_string_array(value.codecs);
        let (bitstream_formats, bitstream_formats_len) =
            into_owned_c_string_array(value.bitstream_formats);
        let (content_types, content_types_len) = into_owned_c_string_array(value.content_types);
        let (required_libraries, required_libraries_len) =
            into_owned_c_string_array(value.required_libraries);
        let (required_demuxers, required_demuxers_len) =
            into_owned_c_string_array(value.required_demuxers);
        let (required_muxers, required_muxers_len) =
            into_owned_c_string_array(value.required_muxers);
        let (required_protocols, required_protocols_len) =
            into_owned_c_string_array(value.required_protocols);
        let (required_parsers, required_parsers_len) =
            into_owned_c_string_array(value.required_parsers);
        let (required_bitstream_filters, required_bitstream_filters_len) =
            into_owned_c_string_array(value.required_bitstream_filters);
        Self {
            supported_runtime_profiles,
            supported_runtime_profiles_len,
            supported_output_routes,
            supported_output_routes_len,
            max_level: into_c_string_ptr(value.max_level),
            media_kinds,
            media_kinds_len,
            codecs,
            codecs_len,
            bitstream_formats,
            bitstream_formats_len,
            supports_seek: value.supports_seek,
            supports_flush: value.supports_flush,
            supports_growing_resources: value.supports_growing_resources,
            supports_range_reads: value.supports_range_reads,
            supports_cancel: value.supports_cancel,
            content_types,
            content_types_len,
            required_libraries,
            required_libraries_len,
            required_demuxers,
            required_demuxers_len,
            required_muxers,
            required_muxers_len,
            required_protocols,
            required_protocols_len,
            required_parsers,
            required_parsers_len,
            required_bitstream_filters,
            required_bitstream_filters_len,
            required_tls: value
                .required_tls
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            requires_network: value.requires_network,
            has_session_read_buffer_bytes: value.session_read_buffer_bytes.is_some(),
            session_read_buffer_bytes: value.session_read_buffer_bytes.unwrap_or_default(),
            has_manifest_snapshot_bytes: value.manifest_snapshot_bytes.is_some(),
            manifest_snapshot_bytes: value.manifest_snapshot_bytes.unwrap_or_default(),
            has_session_disk_soft_cap_bytes: value.session_disk_soft_cap_bytes.is_some(),
            session_disk_soft_cap_bytes: value.session_disk_soft_cap_bytes.unwrap_or_default(),
            has_global_disk_soft_cap_bytes: value.global_disk_soft_cap_bytes.is_some(),
            global_disk_soft_cap_bytes: value.global_disk_soft_cap_bytes.unwrap_or_default(),
            has_max_sessions: value.max_sessions.is_some(),
            max_sessions: value.max_sessions.unwrap_or_default(),
        }
    }
}

impl From<BridgePluginCapabilitySummary> for PlayerFfiPluginCapabilitySummary {
    fn from(value: BridgePluginCapabilitySummary) -> Self {
        match value {
            BridgePluginCapabilitySummary::Decoder(summary) => Self {
                kind: PlayerFfiPluginCapabilityKind::Decoder,
                decoder: summary.into(),
                frame_processor: PlayerFfiPluginFrameProcessorCapabilitySummary::default(),
                source_normalizer: PlayerFfiPluginSourceNormalizerCapabilitySummary::default(),
            },
            BridgePluginCapabilitySummary::FrameProcessor(summary) => Self {
                kind: PlayerFfiPluginCapabilityKind::FrameProcessor,
                decoder: PlayerFfiPluginDecoderCapabilitySummary::default(),
                frame_processor: summary.into(),
                source_normalizer: PlayerFfiPluginSourceNormalizerCapabilitySummary::default(),
            },
            BridgePluginCapabilitySummary::SourceNormalizer(summary) => Self {
                kind: PlayerFfiPluginCapabilityKind::SourceNormalizer,
                decoder: PlayerFfiPluginDecoderCapabilitySummary::default(),
                frame_processor: PlayerFfiPluginFrameProcessorCapabilitySummary::default(),
                source_normalizer: summary.into(),
            },
        }
    }
}

impl From<BridgePluginParticipation> for PlayerFfiPluginParticipation {
    fn from(value: BridgePluginParticipation) -> Self {
        match value {
            BridgePluginParticipation::Unknown => Self::Unknown,
            BridgePluginParticipation::Available => Self::Available,
            BridgePluginParticipation::Selected => Self::Selected,
            BridgePluginParticipation::Participated => Self::Participated,
            BridgePluginParticipation::Bypassed => Self::Bypassed,
        }
    }
}

impl From<BridgePluginDiagnostic> for PlayerFfiPluginDiagnostic {
    fn from(value: BridgePluginDiagnostic) -> Self {
        Self {
            path: into_c_string_ptr(value.path),
            plugin_name: value
                .plugin_name
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            plugin_kind: value
                .plugin_kind
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            status: value.status.into(),
            message: value
                .message
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            capability: value
                .capability
                .map(PlayerFfiPluginCapabilitySummary::from)
                .unwrap_or_default(),
            participation: value.participation.into(),
        }
    }
}

impl From<BridgeStartup> for PlayerFfiStartup {
    fn from(value: BridgeStartup) -> Self {
        let plugin_diagnostics = value
            .plugin_diagnostics
            .into_iter()
            .map(PlayerFfiPluginDiagnostic::from)
            .collect::<Vec<_>>();
        let (plugin_diagnostics, plugin_diagnostics_len) =
            into_owned_struct_array(plugin_diagnostics);
        Self {
            ffmpeg_initialized: value.ffmpeg_initialized,
            has_audio_output: value.audio_output.is_some(),
            audio_output: value
                .audio_output
                .map(PlayerFfiAudioOutputInfo::from)
                .unwrap_or_default(),
            has_decoded_audio: value.decoded_audio.is_some(),
            decoded_audio: value
                .decoded_audio
                .map(PlayerFfiDecodedAudioSummary::from)
                .unwrap_or_default(),
            has_video_decode: value.video_decode.is_some(),
            video_decode: value
                .video_decode
                .map(PlayerFfiVideoDecodeInfo::from)
                .unwrap_or_default(),
            plugin_diagnostics,
            plugin_diagnostics_len,
        }
    }
}

impl From<BridgeProgress> for PlayerFfiProgress {
    fn from(value: BridgeProgress) -> Self {
        Self {
            position_ms: value.position_ms,
            has_duration: value.duration_ms.is_some(),
            duration_ms: value.duration_ms.unwrap_or_default(),
            has_ratio: value.ratio.is_some(),
            ratio: value.ratio.unwrap_or_default(),
        }
    }
}

impl From<BridgeSeekableRange> for PlayerFfiSeekableRange {
    fn from(value: BridgeSeekableRange) -> Self {
        Self {
            start_ms: value.start_ms,
            end_ms: value.end_ms,
        }
    }
}

impl From<BridgeTimelineSnapshot> for PlayerFfiTimelineSnapshot {
    fn from(value: BridgeTimelineSnapshot) -> Self {
        Self {
            kind: value.kind.into(),
            is_seekable: value.is_seekable,
            has_seekable_range: value.seekable_range.is_some(),
            seekable_range: value
                .seekable_range
                .map(PlayerFfiSeekableRange::from)
                .unwrap_or_default(),
            has_live_edge: value.live_edge_ms.is_some(),
            live_edge_ms: value.live_edge_ms.unwrap_or_default(),
            position_ms: value.position_ms,
            has_duration: value.duration_ms.is_some(),
            duration_ms: value.duration_ms.unwrap_or_default(),
            has_ratio: value.ratio.is_some(),
            ratio: value.ratio.unwrap_or_default(),
        }
    }
}

impl From<BridgeSnapshot> for PlayerFfiSnapshot {
    fn from(value: BridgeSnapshot) -> Self {
        Self {
            source_uri: into_c_string_ptr(value.source_uri),
            state: value.state.into(),
            has_video_surface: value.has_video_surface,
            is_interrupted: value.is_interrupted,
            is_buffering: value.is_buffering,
            playback_rate: value.playback_rate,
            progress: value.progress.into(),
            timeline: value.timeline.into(),
            media_info: value.media_info.into(),
        }
    }
}

impl From<BridgeVideoFrame> for PlayerFfiVideoFrame {
    fn from(value: BridgeVideoFrame) -> Self {
        let (bytes, len) = into_owned_bytes(value.bytes);

        Self {
            presentation_time_ms: value.presentation_time_ms,
            width: value.width,
            height: value.height,
            bytes_per_row: value.bytes_per_row,
            pixel_format: value.pixel_format.into(),
            bytes,
            len,
        }
    }
}

impl From<FfiFirstFrameReady> for PlayerFfiFirstFrameReady {
    fn from(value: FfiFirstFrameReady) -> Self {
        Self {
            presentation_time_ms: value.presentation_time_ms,
            width: value.width,
            height: value.height,
        }
    }
}

impl From<BridgeFrameProcessorWarning> for PlayerFfiFrameProcessorWarning {
    fn from(value: BridgeFrameProcessorWarning) -> Self {
        Self {
            kind: value.kind.into(),
            plugin_name: into_c_string_ptr(value.plugin_name),
            processor_index: value.processor_index,
            has_frame_id: value.frame_id.is_some(),
            frame_id: value.frame_id.unwrap_or_default(),
            has_frame_pts_us: value.frame_pts_us.is_some(),
            frame_pts_us: value.frame_pts_us.unwrap_or_default(),
            has_frame_duration_us: value.frame_duration_us.is_some(),
            frame_duration_us: value.frame_duration_us.unwrap_or_default(),
            input_handle_kind: value
                .input_handle_kind
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            output_handle_kind: value
                .output_handle_kind
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            has_queue_depth: value.queue_depth.is_some(),
            queue_depth: value.queue_depth.unwrap_or_default(),
            has_in_flight_frames: value.in_flight_frames.is_some(),
            in_flight_frames: value.in_flight_frames.unwrap_or_default(),
            has_queue_wait_us: value.queue_wait_us.is_some(),
            queue_wait_us: value.queue_wait_us.unwrap_or_default(),
            has_process_time_us: value.process_time_us.is_some(),
            process_time_us: value.process_time_us.unwrap_or_default(),
            has_submit_to_ready_us: value.submit_to_ready_us.is_some(),
            submit_to_ready_us: value.submit_to_ready_us.unwrap_or_default(),
            has_present_deadline_us: value.present_deadline_us.is_some(),
            present_deadline_us: value.present_deadline_us.unwrap_or_default(),
            has_deadline_overrun_us: value.deadline_overrun_us.is_some(),
            deadline_overrun_us: value.deadline_overrun_us.unwrap_or_default(),
            has_consecutive_miss_count: value.consecutive_miss_count.is_some(),
            consecutive_miss_count: value.consecutive_miss_count.unwrap_or_default(),
            policy_action: value.policy_action.into(),
            message: value
                .message
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
        }
    }
}

impl From<BridgeRuntimeWarning> for PlayerFfiRuntimeWarning {
    fn from(value: BridgeRuntimeWarning) -> Self {
        let domain = value.domain().into();
        match value {
            BridgeRuntimeWarning::FrameProcessor(warning) => Self {
                domain,
                frame_processor: warning.into(),
            },
        }
    }
}

impl From<BridgeEvent> for PlayerFfiEvent {
    fn from(value: BridgeEvent) -> Self {
        match value {
            BridgeEvent::Initialized(startup) => Self {
                kind: PlayerFfiEventKind::Initialized,
                initialized: startup.into(),
                ..Self::default()
            },
            BridgeEvent::MetadataReady(media_info) => Self {
                kind: PlayerFfiEventKind::MetadataReady,
                metadata_ready: media_info.into(),
                ..Self::default()
            },
            BridgeEvent::FirstFrameReady(frame) => Self {
                kind: PlayerFfiEventKind::FirstFrameReady,
                first_frame_ready: frame.into(),
                ..Self::default()
            },
            BridgeEvent::PlaybackStateChanged(state) => Self {
                kind: PlayerFfiEventKind::PlaybackStateChanged,
                playback_state: state.into(),
                ..Self::default()
            },
            BridgeEvent::InterruptionChanged { interrupted } => Self {
                kind: PlayerFfiEventKind::InterruptionChanged,
                interrupted,
                ..Self::default()
            },
            BridgeEvent::BufferingChanged { buffering } => Self {
                kind: PlayerFfiEventKind::BufferingChanged,
                buffering,
                ..Self::default()
            },
            BridgeEvent::VideoSurfaceChanged { attached } => Self {
                kind: PlayerFfiEventKind::VideoSurfaceChanged,
                surface_attached: attached,
                ..Self::default()
            },
            BridgeEvent::AudioOutputChanged(audio_output) => Self {
                kind: PlayerFfiEventKind::AudioOutputChanged,
                has_audio_output: audio_output.is_some(),
                audio_output: audio_output
                    .map(PlayerFfiAudioOutputInfo::from)
                    .unwrap_or_default(),
                ..Self::default()
            },
            BridgeEvent::PlaybackRateChanged { rate } => Self {
                kind: PlayerFfiEventKind::PlaybackRateChanged,
                playback_rate: rate,
                ..Self::default()
            },
            BridgeEvent::SeekCompleted { position_ms } => Self {
                kind: PlayerFfiEventKind::SeekCompleted,
                seek_position_ms: position_ms,
                ..Self::default()
            },
            BridgeEvent::RetryScheduled { attempt, delay_ms } => Self {
                kind: PlayerFfiEventKind::RetryScheduled,
                retry_attempt: attempt,
                retry_delay_ms: delay_ms,
                ..Self::default()
            },
            BridgeEvent::Warning(warning) => Self {
                kind: PlayerFfiEventKind::Warning,
                warning: warning.into(),
                ..Self::default()
            },
            BridgeEvent::Error(error) => Self {
                kind: PlayerFfiEventKind::Error,
                error: owned_bridge_error(error),
                ..Self::default()
            },
            BridgeEvent::Ended => Self {
                kind: PlayerFfiEventKind::Ended,
                ..Self::default()
            },
        }
    }
}
