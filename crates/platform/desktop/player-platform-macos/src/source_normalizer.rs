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
                    "source normalizer packet stream skipped for {protocol} adaptive source; selected native adaptive playback path"
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
                outcome.packet_stream_info = Some(ready.stream_info);
                outcome.packet_session = Some(ready.session);
                outcome
                    .diagnostics
                    .push(source_normalizer_runtime_diagnostic(
                        ready.plugin_name.clone(),
                        format!(
                            "source normalizer selected profile {} via {}; ready in {} ms; output packet_stream",
                            ready.selected_profile.as_deref().unwrap_or("auto-detected"),
                            ready.plugin_name.as_deref().unwrap_or("unknown-normalizer"),
                            ready.ready_latency.as_millis()
                        ),
                        PlayerPluginParticipation::Participated,
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
            "source normalizer requested but no source_normalizer_packet_v2 plugin is available: {}",
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
    PlayerPluginDiagnostic {
        path: String::new(),
        plugin_name,
        plugin_kind: Some("source_normalizer".to_owned()),
        status: PlayerPluginDiagnosticStatus::Loaded,
        message: Some(message),
        capability: None,
        participation,
    }
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
