use super::*;

pub(crate) struct MacosSourceNormalizerRuntimeGuard {
    pub(crate) inner: PlayerRuntime,
    source_normalizer_packet_session:
        Option<Arc<Mutex<Option<Box<dyn SourceNormalizerPacketSession>>>>>,
    pub(crate) source_normalizer_diagnostics: Vec<PlayerPluginDiagnostic>,
}

impl PlayerRuntimeAdapter for MacosSourceNormalizerRuntimeGuard {
    fn source_uri(&self) -> &str {
        self.inner.source_uri()
    }

    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        self.inner.capabilities()
    }

    fn media_info(&self) -> &PlayerMediaInfo {
        self.inner.media_info()
    }

    fn presentation_state(&self) -> PresentationState {
        self.inner.presentation_state()
    }

    fn has_video_surface(&self) -> bool {
        self.inner.has_video_surface()
    }

    fn is_interrupted(&self) -> bool {
        self.inner.is_interrupted()
    }

    fn is_buffering(&self) -> bool {
        self.inner.is_buffering()
    }

    fn playback_rate(&self) -> f32 {
        self.inner.playback_rate()
    }

    fn progress(&self) -> PlaybackProgress {
        self.inner.progress()
    }

    fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
        self.inner
            .drain_events()
            .into_iter()
            .map(|event| match event {
                PlayerRuntimeEvent::Initialized(startup) => PlayerRuntimeEvent::Initialized(
                    append_plugin_diagnostics(startup, &self.source_normalizer_diagnostics),
                ),
                other => other,
            })
            .collect()
    }

    fn dispatch(
        &mut self,
        command: PlayerRuntimeCommand,
    ) -> PlayerResult<PlayerRuntimeCommandResult> {
        self.inner.dispatch(command)
    }

    fn replace_video_surface(
        &mut self,
        video_surface: Option<PlayerVideoSurfaceTarget>,
    ) -> PlayerResult<()> {
        self.inner.replace_video_surface(video_surface)
    }

    fn advance(&mut self) -> PlayerResult<Option<DecodedVideoFrame>> {
        self.inner.advance()
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.inner.next_deadline()
    }
}

impl Drop for MacosSourceNormalizerRuntimeGuard {
    fn drop(&mut self) {
        if let Some(session) = self.source_normalizer_packet_session.take() {
            match session.lock() {
                Ok(mut guard) => {
                    if let Some(mut packet_session) = guard.take()
                        && let Err(error) = packet_session.close()
                    {
                        tracing::warn!(
                            error = %error,
                            "source normalizer packet session close failed while dropping macOS runtime guard"
                        );
                    }
                }
                Err(_) => {
                    tracing::error!(
                        "source normalizer packet session mutex was poisoned while dropping macOS runtime guard"
                    );
                }
            }
        }
    }
}

pub(crate) fn source_normalizer_packet_decoder_unavailable_message(
    normalization: &MacosSourceNormalizationOutcome,
    options: &PlayerRuntimeOptions,
) -> Option<String> {
    let stream_info = normalization.packet_stream_info.as_ref()?;
    let video_stream = match macos_packet_stream_info_from_source_normalizer(stream_info) {
        Ok(video_stream) => video_stream,
        Err(error) => {
            tracing::debug!(
                error = %error,
                "source normalizer stream info could not be converted for decoder availability message"
            );
            return None;
        }
    };
    if options.decoder_plugin_video_mode != PlayerDecoderPluginVideoMode::PreferNativeFrame {
        return Some(format!(
            "source normalizer packet stream for {} video requires native-frame decoder plugin mode",
            video_stream.codec
        ));
    }
    if options.video_surface.is_none() {
        return Some(format!(
            "source normalizer packet stream for {} video requires a macOS video surface",
            video_stream.codec
        ));
    }
    if options.decoder_plugin_library_paths.is_empty() {
        return Some(format!(
            "source normalizer packet stream for {} video requires a decoder plugin path",
            video_stream.codec
        ));
    }
    let request = DecoderPluginMatchRequest::video(video_stream.codec.clone());
    let registry = PluginRegistry::inspect_decoder_support(
        &options.decoder_plugin_library_paths,
        request.clone(),
    );
    Some(format!(
        "source normalizer packet stream for {} video found no matching native-frame decoder plugin: {}",
        video_stream.codec,
        source_normalizer_registry_notes(&registry)
    ))
}

pub(crate) fn prepare_source_normalizer_for_open(
    source: MediaSource,
    options: &PlayerRuntimeOptions,
) -> PlayerResult<MacosSourceNormalizationOutcome> {
    let mut outcome = MacosSourceNormalizationOutcome {
        source: source.clone(),
        packet_session: None,
        packet_stream_info: None,
        diagnostics: Vec::new(),
        selected_profile: None,
        normalized_endpoint: None,
        ready_latency: None,
    };
    if options.source_normalizer_mode == SourceNormalizerMode::Disabled {
        return Ok(outcome);
    }

    if should_bypass_source_normalizer_for_native_adaptive(&source) {
        let protocol = match source.protocol() {
            MediaSourceProtocol::Hls => "HLS",
            MediaSourceProtocol::Dash => "DASH",
            _ => "adaptive",
        };
        outcome
            .diagnostics
            .push(source_normalizer_runtime_diagnostic(
                None,
                format!(
                    "source normalizer packet stream skipped for {protocol} adaptive source; selected {} route",
                    PlayerPlaybackRoute::SystemPlayer.wire_name()
                ),
                PlayerPluginParticipation::Bypassed,
            ));
        return Ok(outcome);
    }

    if options.source_normalizer_plugin_library_paths.is_empty() {
        let message =
            "source normalizer requested but no source normalizer plugin paths are configured"
                .to_owned();
        outcome
            .diagnostics
            .push(source_normalizer_runtime_diagnostic(
                None,
                message.clone(),
                PlayerPluginParticipation::Unknown,
            ));
        return match options.source_normalizer_mode {
            SourceNormalizerMode::RequireNormalized => Err(PlayerError::new(
                PlayerErrorCode::Unsupported,
                format!("{message}; source normalizer mode is RequireNormalized"),
            )),
            SourceNormalizerMode::Disabled
            | SourceNormalizerMode::DiagnosticsOnly
            | SourceNormalizerMode::PreflightOnly
            | SourceNormalizerMode::PreferNormalized => Ok(outcome),
        };
    }

    let registry = PluginRegistry::inspect_source_normalizer_support(
        &options.source_normalizer_plugin_library_paths,
    );
    outcome
        .diagnostics
        .extend(registry.records().iter().map(|record| {
            player_plugin_diagnostic_from_record(
                record,
                source_normalizer_plugin_participation(record),
            )
        }));
    if registry.best_source_normalizer().is_none() {
        let message = format!(
            "source normalizer requested but no supported source normalizer plugin is available: {}",
            source_normalizer_registry_notes(&registry)
        );
        outcome
            .diagnostics
            .push(source_normalizer_runtime_diagnostic(
                None,
                message.clone(),
                PlayerPluginParticipation::Unknown,
            ));
        return match options.source_normalizer_mode {
            SourceNormalizerMode::RequireNormalized => {
                Err(PlayerError::new(PlayerErrorCode::Unsupported, message))
            }
            SourceNormalizerMode::Disabled
            | SourceNormalizerMode::DiagnosticsOnly
            | SourceNormalizerMode::PreflightOnly
            | SourceNormalizerMode::PreferNormalized => Ok(outcome),
        };
    }

    if let Some(packet_record) = registry.best_source_normalizer_packet() {
        match open_source_normalizer_packet_session(&source, options, packet_record) {
            Ok(ready) => {
                outcome.selected_profile = ready.selected_profile.clone();
                outcome.ready_latency = Some(ready.ready_latency);
                outcome.normalized_endpoint =
                    ready.stream_info.session_id.as_ref().map(|session_id| {
                        format!("vesper-source-normalizer-packet://{session_id}")
                    });
                let audio_decoder_registry =
                    source_normalizer_audio_decoder_registry(&ready.stream_info, options);
                let packet_stream_summary =
                    source_normalizer_packet_stream_summary_with_audio_decoder_readiness(
                        &ready.stream_info,
                        audio_decoder_registry.as_ref(),
                    );
                let packet_stream_details =
                    source_normalizer_packet_stream_details_with_audio_decoder_readiness(
                        &ready.stream_info,
                        audio_decoder_registry.as_ref(),
                    );
                outcome.packet_stream_info = Some(ready.stream_info);
                outcome.packet_session = Some(ready.session);
                outcome
                    .diagnostics
                    .push(source_normalizer_runtime_diagnostic_with_details(
                        ready.plugin_name.clone(),
                        format!(
                            "source normalizer selected profile {} via {}; ready in {} ms; output packet_stream; {}; waiting for {} decoder handoff",
                            ready.selected_profile.as_deref().unwrap_or("auto-detected"),
                            ready.plugin_name.as_deref().unwrap_or("unknown-normalizer"),
                            ready.ready_latency.as_millis(),
                            packet_stream_summary,
                            PlayerPlaybackRoute::SdkManagedNativeFrame.wire_name()
                        ),
                        PlayerPluginParticipation::Selected,
                        packet_stream_details,
                    ));
                return Ok(outcome);
            }
            Err(error) => {
                let message =
                    format!("source normalizer packet stream failed before playback: {error}");
                outcome
                    .diagnostics
                    .push(source_normalizer_runtime_diagnostic(
                        packet_record.plugin_name.clone(),
                        message.clone(),
                        PlayerPluginParticipation::Bypassed,
                    ));
                if options.source_normalizer_mode == SourceNormalizerMode::RequireNormalized {
                    return Err(PlayerError::new(PlayerErrorCode::BackendFailure, message));
                }
            }
        }
    } else {
        let message = format!(
            "source normalizer requested but no source normalizer packet route is available: {}",
            source_normalizer_registry_notes(&registry)
        );
        outcome
            .diagnostics
            .push(source_normalizer_runtime_diagnostic(
                None,
                message.clone(),
                PlayerPluginParticipation::Unknown,
            ));
        if options.source_normalizer_mode == SourceNormalizerMode::RequireNormalized {
            return Err(PlayerError::new(PlayerErrorCode::Unsupported, message));
        }
    }

    if options.source_normalizer_mode == SourceNormalizerMode::RequireNormalized {
        let message = "source normalizer mode is RequireNormalized but no normalized packet stream was produced".to_owned();
        outcome
            .diagnostics
            .push(source_normalizer_runtime_diagnostic(
                None,
                message.clone(),
                PlayerPluginParticipation::Unknown,
            ));
        return Err(PlayerError::new(PlayerErrorCode::BackendFailure, message));
    }

    Ok(outcome)
}

pub(crate) fn should_bypass_source_normalizer_for_native_adaptive(source: &MediaSource) -> bool {
    matches!(
        source.protocol(),
        MediaSourceProtocol::Hls | MediaSourceProtocol::Dash
    )
}

pub(crate) struct ReadySourceNormalizerPacketSession {
    pub(crate) session: Arc<Mutex<Option<Box<dyn SourceNormalizerPacketSession>>>>,
    pub(crate) stream_info: player_plugin::SourceNormalizerPacketStreamInfo,
    pub(crate) selected_profile: Option<String>,
    pub(crate) plugin_name: Option<String>,
    pub(crate) ready_latency: Duration,
}

pub(crate) fn open_source_normalizer_packet_session(
    source: &MediaSource,
    _options: &PlayerRuntimeOptions,
    record: &PluginDiagnosticRecord,
) -> Result<ReadySourceNormalizerPacketSession, String> {
    let plugin = LoadedDynamicPlugin::load(&record.path)
        .map_err(|error| format!("failed to load source normalizer plugin: {error}"))?;
    let factory = plugin
        .source_normalizer_packet_plugin_factory()
        .ok_or_else(|| {
            format!(
                "{} is not a packet source normalizer plugin",
                plugin.plugin_name()
            )
        })?;
    let requirements = SourceNormalizerPacketSessionRequirements {
        runtime_profile: String::new(),
        media_kind: Some(SourceNormalizerPacketMediaKind::Video),
        codec: None,
        bitstream_format: None,
        require_seek: false,
        require_flush: true,
        require_lease_cleanup: true,
    };
    let missing_capabilities = requirements.missing_capabilities(&factory.packet_capabilities());
    if !missing_capabilities.is_empty() {
        return Err(format!(
            "source normalizer packet plugin `{}` does not satisfy session requirements: missing {}",
            factory.name(),
            missing_capabilities.join(", ")
        ));
    }
    let config = SourceNormalizerPacketSessionConfig {
        runtime_profile: String::new(),
        input: source.uri().to_owned(),
        headers: Vec::new(),
        startup_timeout_ms: Some(SOURCE_NORMALIZER_STARTUP_TIMEOUT.as_millis() as u64),
        session_timeout_ms: Some(SOURCE_NORMALIZER_SESSION_TIMEOUT.as_millis() as u64),
        preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
    };
    let started = Instant::now();
    let session = factory
        .open_packet_session(&config)
        .map_err(|error| format!("open_packet_session failed: {error}"))?;
    let stream_info = session.stream_info();
    macos_packet_stream_info_from_source_normalizer(&stream_info)
        .map_err(|error| format!("invalid packet stream info: {error}"))?;
    Ok(ReadySourceNormalizerPacketSession {
        selected_profile: stream_info.runtime_profile.clone(),
        plugin_name: stream_info
            .normalizer_name
            .clone()
            .or_else(|| Some(factory.name().to_owned())),
        ready_latency: started.elapsed(),
        stream_info,
        session: Arc::new(Mutex::new(Some(session))),
    })
}

pub(crate) fn macos_packet_stream_info_from_source_normalizer(
    stream_info: &player_plugin::SourceNormalizerPacketStreamInfo,
) -> anyhow::Result<VideoPacketStreamInfo> {
    let track = stream_info
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
        .ok_or_else(|| anyhow::anyhow!("source normalizer packet stream has no video track"))?;
    macos_packet_track_info_from_source_normalizer(track)
}

pub(crate) fn macos_packet_track_info_from_source_normalizer(
    track: &SourceNormalizerPacketTrackInfo,
) -> anyhow::Result<VideoPacketStreamInfo> {
    if track.media_kind != SourceNormalizerPacketMediaKind::Video {
        anyhow::bail!("selected source normalizer packet track is not video");
    }
    Ok(VideoPacketStreamInfo {
        stream_index: usize::try_from(track.stream_index).unwrap_or(usize::MAX),
        codec: track.codec.clone(),
        extradata: track.extradata.clone(),
        width: track.width,
        height: track.height,
        frame_rate: track.frame_rate,
    })
}

pub(crate) fn source_normalizer_runtime_diagnostic(
    plugin_name: Option<String>,
    message: String,
    participation: PlayerPluginParticipation,
) -> PlayerPluginDiagnostic {
    source_normalizer_runtime_diagnostic_with_details(
        plugin_name,
        message,
        participation,
        Vec::new(),
    )
}

pub(crate) fn source_normalizer_runtime_diagnostic_with_details(
    plugin_name: Option<String>,
    message: String,
    participation: PlayerPluginParticipation,
    details: Vec<PlayerPluginDiagnosticDetail>,
) -> PlayerPluginDiagnostic {
    PlayerPluginDiagnostic {
        path: String::new(),
        plugin_name,
        plugin_kind: Some("source_normalizer".to_owned()),
        status: PlayerPluginDiagnosticStatus::Loaded,
        message: Some(message),
        capability: None,
        participation,
        details,
    }
}

pub(crate) fn source_normalizer_packet_stream_summary(
    stream_info: &player_plugin::SourceNormalizerPacketStreamInfo,
) -> String {
    let details = SourceNormalizerPacketStreamDiagnosticDetails::from_stream_info(stream_info);
    format!(
        "tracks video={} audio={}; selectedVideoStreamIndex={}; selectedVideoMediaKind={}; selectedVideoCodec={}; audioStreamIndex={}; audioMediaKind={}; audioTrackCodec={}; seekable={}; durationMs={}; route={}",
        details.video_tracks,
        details.audio_tracks,
        details.selected_video_stream_index,
        details.selected_video_media_kind,
        details.selected_video_codec,
        details.audio_stream_index,
        details.audio_media_kind,
        details.audio_codec,
        details.seekable,
        details.duration_ms,
        details.route
    )
}

#[derive(Debug, Clone)]
pub(crate) struct SourceNormalizerPacketStreamDiagnosticDetails {
    video_tracks: usize,
    audio_tracks: usize,
    selected_video_stream_index: String,
    selected_video_media_kind: &'static str,
    selected_video_codec: String,
    audio_stream_index: String,
    audio_media_kind: &'static str,
    audio_codec: String,
    seekable: bool,
    duration_ms: String,
    route: &'static str,
}

impl SourceNormalizerPacketStreamDiagnosticDetails {
    pub(crate) fn from_stream_info(
        stream_info: &player_plugin::SourceNormalizerPacketStreamInfo,
    ) -> Self {
        let video_tracks = stream_info
            .tracks
            .iter()
            .filter(|track| track.media_kind == SourceNormalizerPacketMediaKind::Video)
            .count();
        let audio_tracks = stream_info
            .tracks
            .iter()
            .filter(|track| track.media_kind == SourceNormalizerPacketMediaKind::Audio)
            .count();
        let selected_video = stream_info
            .selected_track_index
            .and_then(|selected| {
                stream_info
                    .tracks
                    .iter()
                    .find(|track| track.stream_index == selected)
            })
            .filter(|track| track.media_kind == SourceNormalizerPacketMediaKind::Video)
            .or_else(|| {
                stream_info
                    .tracks
                    .iter()
                    .find(|track| track.media_kind == SourceNormalizerPacketMediaKind::Video)
            });
        let audio_track = stream_info
            .tracks
            .iter()
            .find(|track| track.media_kind == SourceNormalizerPacketMediaKind::Audio);
        Self {
            video_tracks,
            audio_tracks,
            selected_video_stream_index: selected_video
                .map(|track| track.stream_index.to_string())
                .unwrap_or_else(|| "none".to_owned()),
            selected_video_media_kind: selected_video
                .map(|track| source_normalizer_packet_media_kind_wire_name(track.media_kind))
                .unwrap_or("none"),
            selected_video_codec: selected_video
                .map(|track| track.codec.clone())
                .unwrap_or_else(|| "unknown".to_owned()),
            audio_stream_index: audio_track
                .map(|track| track.stream_index.to_string())
                .unwrap_or_else(|| "none".to_owned()),
            audio_media_kind: audio_track
                .map(|track| source_normalizer_packet_media_kind_wire_name(track.media_kind))
                .unwrap_or("none"),
            audio_codec: audio_track
                .map(|track| track.codec.clone())
                .unwrap_or_else(|| "none".to_owned()),
            seekable: stream_info.seekable,
            duration_ms: stream_info
                .duration_millis
                .map(|duration| duration.to_string())
                .unwrap_or_else(|| "unknown".to_owned()),
            route: PlayerPlaybackRoute::SdkManagedNativeFrame.wire_name(),
        }
    }

    pub(crate) fn into_plugin_details(self) -> Vec<PlayerPluginDiagnosticDetail> {
        vec![
            source_normalizer_detail("videoTracks", self.video_tracks.to_string()),
            source_normalizer_detail("audioTracks", self.audio_tracks.to_string()),
            source_normalizer_detail("selectedVideoStreamIndex", self.selected_video_stream_index),
            source_normalizer_detail("selectedVideoMediaKind", self.selected_video_media_kind),
            source_normalizer_detail("selectedVideoCodec", self.selected_video_codec),
            source_normalizer_detail("audioStreamIndex", self.audio_stream_index),
            source_normalizer_detail("audioMediaKind", self.audio_media_kind),
            source_normalizer_detail("audioTrackCodec", self.audio_codec),
            source_normalizer_detail("seekable", self.seekable.to_string()),
            source_normalizer_detail("durationMs", self.duration_ms),
            source_normalizer_detail("route", self.route),
        ]
    }
}

pub(crate) fn source_normalizer_packet_stream_details(
    stream_info: &player_plugin::SourceNormalizerPacketStreamInfo,
) -> Vec<PlayerPluginDiagnosticDetail> {
    SourceNormalizerPacketStreamDiagnosticDetails::from_stream_info(stream_info)
        .into_plugin_details()
}

pub(crate) fn source_normalizer_packet_media_kind_wire_name(
    media_kind: SourceNormalizerPacketMediaKind,
) -> &'static str {
    match media_kind {
        SourceNormalizerPacketMediaKind::Video => "video",
        SourceNormalizerPacketMediaKind::Audio => "audio",
        SourceNormalizerPacketMediaKind::Subtitle => "subtitle",
    }
}

pub(crate) fn source_normalizer_packet_stream_summary_with_audio_decoder_readiness(
    stream_info: &player_plugin::SourceNormalizerPacketStreamInfo,
    audio_decoder_registry: Option<&PluginRegistry>,
) -> String {
    format!(
        "{}; {}",
        source_normalizer_packet_stream_summary(stream_info),
        source_normalizer_audio_decoder_readiness_note(stream_info, audio_decoder_registry)
    )
}

pub(crate) fn source_normalizer_packet_stream_details_with_audio_decoder_readiness(
    stream_info: &player_plugin::SourceNormalizerPacketStreamInfo,
    audio_decoder_registry: Option<&PluginRegistry>,
) -> Vec<PlayerPluginDiagnosticDetail> {
    let mut details = source_normalizer_packet_stream_details(stream_info);
    details.extend(source_normalizer_audio_decoder_readiness_details(
        stream_info,
        audio_decoder_registry,
    ));
    details
}

pub(crate) fn source_normalizer_audio_decoder_readiness_note(
    stream_info: &player_plugin::SourceNormalizerPacketStreamInfo,
    audio_decoder_registry: Option<&PluginRegistry>,
) -> String {
    let Some(audio_track) = stream_info
        .tracks
        .iter()
        .find(|track| track.media_kind == SourceNormalizerPacketMediaKind::Audio)
    else {
        return "audioDecoderPlugin=none; audioDecoderPluginReady=false; audioDecoder=none"
            .to_owned();
    };
    let Some(registry) = audio_decoder_registry else {
        return "audioDecoderPlugin=none; audioDecoderPluginReady=false; audioDecoder=none"
            .to_owned();
    };
    let request = DecoderPluginMatchRequest::audio(audio_track.codec.clone());
    if let Some(record) = registry.best_pcm_audio_decoder_for(&request) {
        let plugin = record.plugin_name.as_deref().unwrap_or("unknown-decoder");
        return format!(
            "audioDecoderPlugin={plugin}; audioDecoderPluginReady=true; audioDecoder=none"
        );
    }
    "audioDecoderPlugin=none; audioDecoderPluginReady=false; audioDecoder=none".to_owned()
}

pub(crate) fn source_normalizer_audio_decoder_readiness_details(
    stream_info: &player_plugin::SourceNormalizerPacketStreamInfo,
    audio_decoder_registry: Option<&PluginRegistry>,
) -> Vec<PlayerPluginDiagnosticDetail> {
    let mut details = Vec::new();
    let Some(audio_track) = stream_info
        .tracks
        .iter()
        .find(|track| track.media_kind == SourceNormalizerPacketMediaKind::Audio)
    else {
        details.push(source_normalizer_detail("audioDecoderPlugin", "none"));
        details.push(source_normalizer_detail("audioDecoderPluginReady", "false"));
        details.push(source_normalizer_detail("audioDecoder", "none"));
        return details;
    };
    let plugin_name = audio_decoder_registry.and_then(|registry| {
        registry
            .best_pcm_audio_decoder_for(&DecoderPluginMatchRequest::audio(
                audio_track.codec.clone(),
            ))
            .and_then(|record| record.plugin_name.clone())
    });
    details.push(source_normalizer_detail(
        "audioDecoderPlugin",
        plugin_name.unwrap_or_else(|| "none".to_owned()),
    ));
    details.push(source_normalizer_detail(
        "audioDecoderPluginReady",
        (audio_decoder_registry
            .and_then(|registry| {
                registry.best_pcm_audio_decoder_for(&DecoderPluginMatchRequest::audio(
                    audio_track.codec.clone(),
                ))
            })
            .is_some())
        .to_string(),
    ));
    details.push(source_normalizer_detail("audioDecoder", "none"));
    details
}

pub(crate) fn source_normalizer_detail(
    key: impl Into<String>,
    value: impl Into<String>,
) -> PlayerPluginDiagnosticDetail {
    PlayerPluginDiagnosticDetail {
        key: key.into(),
        value: value.into(),
    }
}

pub(crate) fn source_normalizer_audio_decoder_registry(
    stream_info: &player_plugin::SourceNormalizerPacketStreamInfo,
    options: &PlayerRuntimeOptions,
) -> Option<PluginRegistry> {
    let audio_track = stream_info
        .tracks
        .iter()
        .find(|track| track.media_kind == SourceNormalizerPacketMediaKind::Audio)?;
    if options.decoder_plugin_library_paths.is_empty() {
        return None;
    }
    Some(PluginRegistry::inspect_decoder_support(
        &options.decoder_plugin_library_paths,
        DecoderPluginMatchRequest::audio(audio_track.codec.clone()),
    ))
}

pub(crate) fn source_normalizer_registry_notes(registry: &PluginRegistry) -> String {
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

pub(crate) fn apply_source_normalizer_open_diagnostics(
    mut startup: PlayerRuntimeStartup,
    normalization: &MacosSourceNormalizationOutcome,
) -> PlayerRuntimeStartup {
    for diagnostic in &normalization.diagnostics {
        startup.plugin_diagnostics.push(diagnostic.clone());
    }
    startup
}

pub(crate) fn mark_source_normalizer_packet_stream_participated(
    normalization: &mut MacosSourceNormalizationOutcome,
    decoder_plugin_name: Option<&str>,
) {
    if !normalization.has_packet_stream() {
        return;
    }
    for diagnostic in &mut normalization.diagnostics {
        if diagnostic.plugin_kind.as_deref() != Some("source_normalizer") {
            continue;
        }
        let Some(message) = diagnostic.message.as_mut() else {
            continue;
        };
        if !message.contains("output packet_stream") {
            continue;
        }
        diagnostic.participation = PlayerPluginParticipation::Participated;
        let decoder = decoder_plugin_name.unwrap_or("selected native-frame decoder");
        message.push_str(&format!(
            "; handed to {decoder} and macOS {} presenter",
            PlayerPlaybackRoute::SdkManagedNativeFrame.wire_name()
        ));
    }
}

pub(crate) fn drop_source_normalizer_packet_session(
    normalization: &mut MacosSourceNormalizationOutcome,
) {
    if let Some(packet_session) = normalization.packet_session.take()
        && let Ok(mut guard) = packet_session.lock()
        && let Some(mut session) = guard.take()
    {
        let _ = session.close();
    }
    normalization.packet_stream_info = None;
}

pub(crate) fn attach_source_normalizer_to_runtime(
    bootstrap: PlayerRuntimeBootstrap,
    mut normalization: MacosSourceNormalizationOutcome,
) -> PlayerRuntimeBootstrap {
    if normalization.packet_session.is_some() {
        let packet_session = normalization.packet_session.take();
        let adapter_id = bootstrap.runtime.adapter_id().to_owned();
        let PlayerRuntimeBootstrap {
            runtime,
            initial_frame,
            startup,
        } = bootstrap;
        let source_normalizer_diagnostics = startup
            .plugin_diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.plugin_kind.as_deref() == Some("source_normalizer"))
            .cloned()
            .collect::<Vec<_>>();
        let adapter_id = if adapter_id == MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID {
            MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID
        } else if adapter_id == MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID {
            MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
        } else {
            MACOS_HOST_PLAYER_RUNTIME_ADAPTER_ID
        };
        return PlayerRuntime::from_adapter_bootstrap(
            adapter_id,
            PlayerRuntimeAdapterBootstrap {
                runtime: Box::new(MacosSourceNormalizerRuntimeGuard {
                    inner: runtime,
                    source_normalizer_packet_session: packet_session,
                    source_normalizer_diagnostics,
                }),
                initial_frame,
                startup,
            },
        );
    }
    bootstrap
}
