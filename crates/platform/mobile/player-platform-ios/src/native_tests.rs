use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{
    IOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID, IosAvPlayerBridge, IosAvPlayerBridgeBindings,
    IosAvPlayerBridgeContext, IosAvPlayerSnapshot, IosAvPlayerStateTracker, IosHostBridgeSession,
    IosHostCommand, IosHostEvent, IosHostSnapshot, IosHostTimelineKind, IosManagedNativeSession,
    IosNativeCommandSink, IosNativePlayerBridge, IosNativePlayerCommand, IosNativePlayerProbe,
    IosNativePlayerRuntimeAdapterFactory, IosNativePlayerSession, IosNativePlayerSessionBootstrap,
    IosOpaqueHandle, IosPlayerItemStatus, IosTimeControlStatus, IosVideoSurfaceKind,
    host_video_surface_target, resolve_bridge_context,
};
use player_model::MediaSource;
use player_runtime::{
    DecodedVideoFrame, FrameProcessorMode, MediaAbrMode, MediaAbrPolicy, MediaTrack,
    MediaTrackCatalog, MediaTrackKind, MediaTrackSelection, MediaTrackSelectionSnapshot,
    PlaybackProgress, PlayerErrorCode, PlayerMediaInfo, PlayerPluginParticipation,
    PlayerResilienceMetrics, PlayerResult, PlayerRuntimeAdapterBackendFamily,
    PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory, PlayerRuntimeCommand,
    PlayerRuntimeCommandResult, PlayerRuntimeEvent, PlayerRuntimeOptions, PlayerRuntimeStartup,
    PlayerSnapshot, PlayerTimelineSnapshot, PlayerVideoSurfaceKind, PlayerVideoSurfaceTarget,
    PresentationState,
};

#[test]
fn ios_factory_exposes_native_capabilities() {
    let factory = IosNativePlayerRuntimeAdapterFactory::default();
    let initializer = factory
        .probe_source_with_options(
            MediaSource::new("placeholder.mp4"),
            PlayerRuntimeOptions::default(),
        )
        .expect("ios skeleton probe should succeed");

    let capabilities = initializer.capabilities();
    assert_eq!(
        capabilities.adapter_id,
        IOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID
    );
    assert!(capabilities.supports_external_video_surface);
    assert!(capabilities.supports_hardware_decode);
}

#[test]
fn ios_frame_processor_config_reports_missing_plugin_diagnostic() {
    let factory = IosNativePlayerRuntimeAdapterFactory::default();
    let initializer = factory
        .probe_source_with_options(
            MediaSource::new("placeholder.mp4"),
            PlayerRuntimeOptions::default()
                .with_frame_processor_mode(FrameProcessorMode::DiagnosticsOnly),
        )
        .expect("ios skeleton probe should succeed");

    let startup = initializer.startup();
    let diagnostic = startup
        .plugin_diagnostics
        .iter()
        .find(|diagnostic| diagnostic.plugin_kind.as_deref() == Some("frame_processor"))
        .expect("frame processor configuration should report a diagnostic");
    assert_eq!(diagnostic.participation, PlayerPluginParticipation::Unknown);
    assert!(
        diagnostic
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("no plugin paths")
    );
}

#[test]
fn ios_factory_is_initialize_unsupported_without_bridge() {
    let factory = IosNativePlayerRuntimeAdapterFactory::default();
    let initializer = factory
        .probe_source_with_options(
            MediaSource::new("placeholder.mp4"),
            PlayerRuntimeOptions::default(),
        )
        .expect("ios skeleton probe should succeed");

    let error = match initializer.initialize() {
        Ok(_) => panic!("ios skeleton initialize should be unsupported"),
        Err(error) => error,
    };
    assert_eq!(error.code(), PlayerErrorCode::Unsupported);
}

#[test]
fn ios_factory_can_initialize_with_bridge() {
    let factory = IosNativePlayerRuntimeAdapterFactory::with_bridge(Arc::new(FakeIosBridge));
    let initializer = factory
        .probe_source_with_options(
            MediaSource::new("placeholder.mp4"),
            PlayerRuntimeOptions::default(),
        )
        .expect("ios bridge probe should succeed");

    let bootstrap = initializer
        .initialize()
        .expect("ios bridge initialize should succeed");
    assert!(bootstrap.initial_frame.is_none());
    assert_eq!(
        bootstrap.runtime.capabilities().backend_family,
        PlayerRuntimeAdapterBackendFamily::NativeIos
    );
}

#[test]
fn ios_state_tracker_maps_ready_play_pause_and_end() {
    let mut tracker = IosAvPlayerStateTracker::default();

    let ready = tracker.observe(&IosAvPlayerSnapshot {
        item_status: IosPlayerItemStatus::ReadyToPlay,
        time_control_status: IosTimeControlStatus::Paused,
        playback_rate: 1.0,
        position: Duration::ZERO,
        duration: Some(Duration::from_secs(8)),
        reached_end: false,
        error_message: None,
    });
    assert_eq!(ready.presentation_state, PresentationState::Ready);

    let playing = tracker.observe(&IosAvPlayerSnapshot {
        item_status: IosPlayerItemStatus::ReadyToPlay,
        time_control_status: IosTimeControlStatus::Playing,
        playback_rate: 1.0,
        position: Duration::from_secs(1),
        duration: Some(Duration::from_secs(8)),
        reached_end: false,
        error_message: None,
    });
    assert_eq!(playing.presentation_state, PresentationState::Playing);

    let paused = tracker.observe(&IosAvPlayerSnapshot {
        item_status: IosPlayerItemStatus::ReadyToPlay,
        time_control_status: IosTimeControlStatus::Paused,
        playback_rate: 1.0,
        position: Duration::from_secs(2),
        duration: Some(Duration::from_secs(8)),
        reached_end: false,
        error_message: None,
    });
    assert_eq!(paused.presentation_state, PresentationState::Paused);

    let finished = tracker.observe(&IosAvPlayerSnapshot {
        item_status: IosPlayerItemStatus::ReadyToPlay,
        time_control_status: IosTimeControlStatus::Paused,
        playback_rate: 1.0,
        position: Duration::from_secs(8),
        duration: Some(Duration::from_secs(8)),
        reached_end: true,
        error_message: None,
    });
    assert_eq!(finished.presentation_state, PresentationState::Finished);
    assert!(
        finished
            .emitted_events
            .iter()
            .any(|event| matches!(event, player_runtime::PlayerRuntimeEvent::Ended))
    );
}

#[test]
fn ios_state_tracker_keeps_playing_while_native_player_is_waiting() {
    let mut tracker = IosAvPlayerStateTracker::default();
    tracker.seed(PresentationState::Playing, 1.0);

    let observation = tracker.observe(&IosAvPlayerSnapshot {
        item_status: IosPlayerItemStatus::ReadyToPlay,
        time_control_status: IosTimeControlStatus::WaitingToPlay,
        playback_rate: 1.0,
        position: Duration::from_millis(250),
        duration: Some(Duration::from_secs(10)),
        reached_end: false,
        error_message: None,
    });

    assert_eq!(observation.presentation_state, PresentationState::Playing);
    assert!(observation.is_buffering);
    assert!(
        !observation
            .emitted_events
            .iter()
            .any(|event| matches!(event, PlayerRuntimeEvent::PlaybackStateChanged(_)))
    );
    assert!(observation.emitted_events.iter().any(|event| matches!(
        event,
        PlayerRuntimeEvent::BufferingChanged { buffering: true }
    )));
}

#[test]
fn ios_state_tracker_emits_backend_error_event() {
    let mut tracker = IosAvPlayerStateTracker::default();

    let observation = tracker.observe(&IosAvPlayerSnapshot {
        item_status: IosPlayerItemStatus::Failed,
        time_control_status: IosTimeControlStatus::Paused,
        playback_rate: 1.0,
        position: Duration::ZERO,
        duration: None,
        reached_end: false,
        error_message: Some("avplayer item failed".to_owned()),
    });

    assert!(observation.emitted_events.iter().any(|event| matches!(
        event,
        player_runtime::PlayerRuntimeEvent::Error(error)
        if error.code() == PlayerErrorCode::BackendFailure
    )));
}

#[test]
fn ios_managed_session_replays_from_start_when_finished() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingIosCommandSink::new(commands.clone());
    let mut session = IosManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);

    session.apply_snapshot(&IosAvPlayerSnapshot {
        item_status: IosPlayerItemStatus::ReadyToPlay,
        time_control_status: IosTimeControlStatus::Paused,
        playback_rate: 1.0,
        position: Duration::from_secs(6),
        duration: Some(Duration::from_secs(6)),
        reached_end: true,
        error_message: None,
    });

    let result = session
        .dispatch(PlayerRuntimeCommand::Play)
        .expect("play from finished should be bridged");

    assert!(result.applied);
    assert_eq!(result.snapshot.state, PresentationState::Playing);
    assert_eq!(
        *commands.lock().expect("commands lock"),
        vec![
            IosNativePlayerCommand::SeekTo {
                position: Duration::ZERO,
            },
            IosNativePlayerCommand::Play,
        ]
    );
}

#[test]
fn ios_managed_session_validates_pause_and_playback_rate() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingIosCommandSink::new(commands.clone());
    let mut session = IosManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);

    let pause_error = session
        .dispatch(PlayerRuntimeCommand::Pause)
        .expect_err("pause before play should be invalid");
    assert_eq!(pause_error.code(), PlayerErrorCode::InvalidState);

    let rate_error = session
        .dispatch(PlayerRuntimeCommand::SetPlaybackRate { rate: 4.0 })
        .expect_err("out-of-range playback rate should fail");
    assert_eq!(rate_error.code(), PlayerErrorCode::InvalidArgument);
    assert!(commands.lock().expect("commands lock").is_empty());
}

#[test]
fn ios_managed_session_dispatches_audio_track_selection() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingIosCommandSink::new(commands.clone());
    let mut session = IosManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);

    let result = session
        .dispatch(PlayerRuntimeCommand::SetAudioTrackSelection {
            selection: MediaTrackSelection::track("audio-en"),
        })
        .expect("audio track selection should dispatch");

    assert!(result.applied);
    assert_eq!(
        *commands.lock().expect("commands lock"),
        vec![IosNativePlayerCommand::SetAudioTrackSelection {
            selection: MediaTrackSelection::track("audio-en"),
        }]
    );
    assert_eq!(
        result.snapshot.media_info.track_selection.audio,
        MediaTrackSelection::track("audio-en")
    );
    let events = session.drain_events();
    assert!(events.iter().any(|event| matches!(
        event,
        PlayerRuntimeEvent::MetadataReady(media_info)
            if media_info.track_selection.audio == MediaTrackSelection::track("audio-en")
    )));
}

#[test]
fn ios_managed_session_dispatches_constrained_abr_by_bitrate() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingIosCommandSink::new(commands.clone());
    let mut session = IosManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);
    let policy = MediaAbrPolicy {
        mode: MediaAbrMode::Constrained,
        track_id: None,
        max_bit_rate: Some(1_500_000),
        max_width: None,
        max_height: None,
    };

    let result = session
        .dispatch(PlayerRuntimeCommand::SetAbrPolicy {
            policy: policy.clone(),
        })
        .expect("constrained abr should dispatch");

    assert!(result.applied);
    assert_eq!(
        *commands.lock().expect("commands lock"),
        vec![IosNativePlayerCommand::SetAbrPolicy {
            policy: policy.clone(),
        }]
    );
    assert_eq!(
        result.snapshot.media_info.track_selection.abr_policy,
        policy
    );
}

#[test]
fn ios_managed_session_dispatches_constrained_abr_by_resolution() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingIosCommandSink::new(commands.clone());
    let mut session = IosManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);
    let policy = MediaAbrPolicy {
        mode: MediaAbrMode::Constrained,
        track_id: None,
        max_bit_rate: None,
        max_width: Some(1280),
        max_height: Some(720),
    };

    let result = session
        .dispatch(PlayerRuntimeCommand::SetAbrPolicy {
            policy: policy.clone(),
        })
        .expect("resolution-constrained abr should dispatch");

    assert!(result.applied);
    assert_eq!(
        *commands.lock().expect("commands lock"),
        vec![IosNativePlayerCommand::SetAbrPolicy {
            policy: policy.clone(),
        }]
    );
    assert_eq!(
        result.snapshot.media_info.track_selection.abr_policy,
        policy
    );
}

#[test]
fn ios_managed_session_rejects_partial_resolution_abr_limit() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingIosCommandSink::new(commands.clone());
    let mut session = IosManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);

    let error = session
        .dispatch(PlayerRuntimeCommand::SetAbrPolicy {
            policy: MediaAbrPolicy {
                mode: MediaAbrMode::Constrained,
                track_id: None,
                max_bit_rate: None,
                max_width: Some(1280),
                max_height: None,
            },
        })
        .expect_err("partial resolution abr limit should be rejected");

    assert_eq!(error.code(), PlayerErrorCode::InvalidArgument);
    assert!(commands.lock().expect("commands lock").is_empty());
}

#[test]
fn ios_managed_session_rejects_video_track_selection() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingIosCommandSink::new(commands.clone());
    let mut session = IosManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);

    let error = session
        .dispatch(PlayerRuntimeCommand::SetVideoTrackSelection {
            selection: MediaTrackSelection::track("video-main"),
        })
        .expect_err("video track selection should be unsupported");

    assert_eq!(error.code(), PlayerErrorCode::Unsupported);
    assert!(commands.lock().expect("commands lock").is_empty());
}

#[test]
fn ios_managed_session_updates_from_native_snapshot() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingIosCommandSink::new(commands);
    let mut session = IosManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);

    session.apply_snapshot(&IosAvPlayerSnapshot {
        item_status: IosPlayerItemStatus::ReadyToPlay,
        time_control_status: IosTimeControlStatus::Playing,
        playback_rate: 1.25,
        position: Duration::from_millis(900),
        duration: Some(Duration::from_secs(5)),
        reached_end: false,
        error_message: None,
    });

    assert_eq!(session.presentation_state(), PresentationState::Playing);
    assert!((session.playback_rate() - 1.25).abs() < f32::EPSILON);
    assert_eq!(session.progress().position(), Duration::from_millis(900));
    let events = session.drain_events();
    assert!(events.iter().any(|event| matches!(
        event,
        player_runtime::PlayerRuntimeEvent::PlaybackRateChanged { rate }
        if (*rate - 1.25).abs() < f32::EPSILON
    )));
}

#[test]
fn ios_managed_session_controller_delivers_async_updates() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingIosCommandSink::new(commands);
    let (mut session, controller) =
        IosManagedNativeSession::with_controller("placeholder.mp4", test_media_info(), sink);

    controller.apply_snapshot(IosAvPlayerSnapshot {
        item_status: IosPlayerItemStatus::ReadyToPlay,
        time_control_status: IosTimeControlStatus::Playing,
        playback_rate: 1.5,
        position: Duration::from_secs(2),
        duration: Some(Duration::from_secs(12)),
        reached_end: false,
        error_message: None,
    });
    controller.report_seek_completed(Duration::from_secs(3));
    controller.report_retry_scheduled(2, Duration::from_millis(1_500));
    controller.report_error(PlayerErrorCode::BackendFailure, "avplayer callback failed");

    let events = session.drain_events();
    assert_eq!(session.presentation_state(), PresentationState::Playing);
    assert!((session.playback_rate() - 1.5).abs() < f32::EPSILON);
    assert_eq!(session.progress().position(), Duration::from_secs(3));
    assert!(events.iter().any(|event| matches!(
        event,
        PlayerRuntimeEvent::SeekCompleted { position } if *position == Duration::from_secs(3)
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        PlayerRuntimeEvent::RetryScheduled { attempt: 2, delay }
        if *delay == Duration::from_millis(1_500)
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        PlayerRuntimeEvent::Error(error)
        if error.code() == PlayerErrorCode::BackendFailure
    )));
    assert_eq!(session.snapshot().resilience_metrics.retry_count, 2);
}

#[test]
fn ios_managed_session_snapshot_pumps_pending_media_info_updates() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingIosCommandSink::new(commands);
    let (mut session, controller) =
        IosManagedNativeSession::with_controller("placeholder.mp4", test_media_info(), sink);
    let track_selection = MediaTrackSelectionSnapshot {
        video: MediaTrackSelection::auto(),
        audio: MediaTrackSelection::track("audio-en"),
        subtitle: MediaTrackSelection::track("subtitle-en"),
        abr_policy: MediaAbrPolicy {
            mode: MediaAbrMode::Constrained,
            track_id: None,
            max_bit_rate: Some(900_000),
            max_width: None,
            max_height: None,
        },
    };

    controller.report_media_info(test_track_catalog(), track_selection.clone());

    let snapshot = session.snapshot();
    assert_eq!(snapshot.media_info.track_catalog, test_track_catalog());
    assert_eq!(snapshot.media_info.track_selection, track_selection);
}

#[test]
fn ios_managed_session_emits_initial_and_interruption_events() {
    let mut session = IosManagedNativeSession::new(
        "placeholder.mp4",
        test_media_info(),
        RecordingIosCommandSink::new(Arc::new(Mutex::new(Vec::new()))),
    );

    session.emit_initial_runtime_events(PlayerRuntimeStartup {
        ffmpeg_initialized: false,
        audio_output: None,
        decoded_audio: None,
        video_decode: None,
        plugin_diagnostics: Vec::new(),
    });

    let initial_events = session.drain_events();
    assert!(matches!(
        initial_events.first(),
        Some(PlayerRuntimeEvent::Initialized(_))
    ));
    assert!(
        initial_events
            .iter()
            .any(|event| matches!(event, PlayerRuntimeEvent::MetadataReady(_)))
    );
    assert!(initial_events.iter().any(|event| matches!(
        event,
        PlayerRuntimeEvent::PlaybackStateChanged(PresentationState::Ready)
    )));

    session
        .dispatch(PlayerRuntimeCommand::Play)
        .expect("play should seed playback intent");
    let _ = session.drain_events();

    session.apply_snapshot(&IosAvPlayerSnapshot {
        item_status: IosPlayerItemStatus::ReadyToPlay,
        time_control_status: IosTimeControlStatus::WaitingToPlay,
        playback_rate: 1.0,
        position: Duration::from_millis(250),
        duration: Some(Duration::from_secs(12)),
        reached_end: false,
        error_message: None,
    });
    let buffering_events = session.drain_events();
    assert!(buffering_events.iter().any(|event| matches!(
        event,
        PlayerRuntimeEvent::BufferingChanged { buffering: true }
    )));

    session.controller().report_interruption_changed(true);
    let interrupted_events = session.drain_events();
    assert!(interrupted_events.iter().any(|event| matches!(
        event,
        PlayerRuntimeEvent::InterruptionChanged { interrupted: true }
    )));
    assert!(interrupted_events.iter().any(|event| matches!(
        event,
        PlayerRuntimeEvent::BufferingChanged { buffering: false }
    )));
    assert!(session.snapshot().is_interrupted);
    assert!(!session.snapshot().is_buffering);
}

#[test]
fn ios_managed_session_replace_video_surface_emits_event() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let surfaces = Arc::new(Mutex::new(Vec::new()));
    let mut session = IosManagedNativeSession::new(
        "placeholder.mp4",
        test_media_info(),
        SurfaceRecordingIosCommandSink::new(commands, surfaces.clone()),
    );

    session
        .replace_video_surface(Some(host_video_surface_target()))
        .expect("surface attachment should succeed");
    assert!(session.snapshot().has_video_surface);
    assert_eq!(surfaces.lock().expect("surface lock").len(), 1);

    let attach_events = session.drain_events();
    assert!(attach_events.iter().any(|event| matches!(
        event,
        PlayerRuntimeEvent::VideoSurfaceChanged { attached: true }
    )));

    session
        .replace_video_surface(None)
        .expect("surface detach should succeed");
    assert!(!session.snapshot().has_video_surface);
    let detach_events = session.drain_events();
    assert!(detach_events.iter().any(|event| matches!(
        event,
        PlayerRuntimeEvent::VideoSurfaceChanged { attached: false }
    )));
}

#[test]
fn ios_avplayer_bridge_bindings_can_initialize_managed_session() {
    let bridge = IosAvPlayerBridge::new(
        IosAvPlayerBridgeContext {
            av_player: IosOpaqueHandle(2),
            video_surface: None,
        },
        Arc::new(FakeIosAvBindings::default()),
    );
    let factory = IosNativePlayerRuntimeAdapterFactory::with_bridge(Arc::new(bridge));
    let initializer = factory
        .probe_source_with_options(
            MediaSource::new("placeholder.mp4"),
            PlayerRuntimeOptions::default(),
        )
        .expect("ios av bridge probe should succeed");

    let bootstrap = initializer
        .initialize()
        .expect("ios av bridge initialize should succeed");
    assert!(bootstrap.initial_frame.is_none());
    assert_eq!(
        bootstrap.runtime.capabilities().backend_family,
        PlayerRuntimeAdapterBackendFamily::NativeIos
    );
}

#[test]
fn ios_resolve_bridge_context_prefers_runtime_surface_option() {
    let context = resolve_bridge_context(
        &IosAvPlayerBridgeContext {
            av_player: IosOpaqueHandle(7),
            video_surface: None,
        },
        &PlayerRuntimeOptions::default().with_video_surface(PlayerVideoSurfaceTarget {
            kind: PlayerVideoSurfaceKind::PlayerLayer,
            handle: 42,
        }),
    )
    .expect("runtime surface should resolve");

    let resolved_surface = context.video_surface.expect("resolved surface");
    assert_eq!(resolved_surface.kind, IosVideoSurfaceKind::PlayerLayer);
    assert_eq!(resolved_surface.handle, IosOpaqueHandle(42));
}

#[test]
fn ios_host_snapshot_conversion_preserves_timeline_shape() {
    let snapshot = PlayerSnapshot {
        source_uri: "placeholder.mp4".to_owned(),
        state: PresentationState::Playing,
        has_video_surface: true,
        is_interrupted: true,
        is_buffering: true,
        playback_rate: 1.5,
        progress: PlaybackProgress::new(Duration::from_secs(5), Some(Duration::from_secs(20))),
        timeline: PlayerTimelineSnapshot::vod(
            PlaybackProgress::new(Duration::from_secs(5), Some(Duration::from_secs(20))),
            true,
        ),
        media_info: test_media_info(),
        resilience_metrics: PlayerResilienceMetrics::default(),
    };

    let host = IosHostSnapshot::from_player_snapshot(&snapshot);
    assert_eq!(host.playback_state, PresentationState::Playing);
    assert!(host.has_video_surface);
    assert!(host.is_interrupted);
    assert!(host.is_buffering);
    assert_eq!(host.position_ms, 5_000);
    assert_eq!(host.duration_ms, Some(20_000));
    assert_eq!(host.seekable_range.expect("seekable range").end_ms, 20_000);
}

#[test]
fn ios_host_snapshot_conversion_uses_effective_live_edge_for_live_dvr() {
    let snapshot = PlayerSnapshot {
        source_uri: "https://example.com/live.m3u8".to_owned(),
        state: PresentationState::Playing,
        has_video_surface: true,
        is_interrupted: false,
        is_buffering: false,
        playback_rate: 1.0,
        progress: PlaybackProgress::new(Duration::from_secs(84), None),
        timeline: PlayerTimelineSnapshot::live_dvr(
            PlaybackProgress::new(Duration::from_secs(84), None),
            player_runtime::PlayerSeekableRange {
                start: Duration::ZERO,
                end: Duration::from_secs(120),
            },
            None,
        ),
        media_info: test_media_info(),
        resilience_metrics: PlayerResilienceMetrics::default(),
    };

    let host = IosHostSnapshot::from_player_snapshot(&snapshot);
    assert_eq!(host.timeline_kind, IosHostTimelineKind::LiveDvr);
    assert_eq!(host.live_edge_ms, Some(120_000));
    assert_eq!(host.position_ms, 84_000);
}

#[test]
fn ios_host_event_conversion_maps_runtime_events() {
    let rate =
        IosHostEvent::from_runtime_event(&PlayerRuntimeEvent::PlaybackRateChanged { rate: 1.25 });
    assert!(matches!(
        rate,
        Some(IosHostEvent::PlaybackRateChanged { rate })
        if (rate - 1.25).abs() < f32::EPSILON
    ));

    let interruption = IosHostEvent::from_runtime_event(&PlayerRuntimeEvent::InterruptionChanged {
        interrupted: true,
    });
    assert!(matches!(
        interruption,
        Some(IosHostEvent::InterruptionChanged { interrupted: true })
    ));

    let retry = IosHostEvent::from_runtime_event(&PlayerRuntimeEvent::RetryScheduled {
        attempt: 3,
        delay: Duration::from_secs(2),
    });
    assert!(matches!(
        retry,
        Some(IosHostEvent::RetryScheduled {
            attempt: 3,
            delay_ms: 2_000,
        })
    ));
}

#[test]
fn ios_host_bridge_session_drains_native_commands() {
    let mut session = IosHostBridgeSession::new("placeholder.mp4");
    session
        .dispatch_command(PlayerRuntimeCommand::Play)
        .expect("play should dispatch");
    session
        .dispatch_command(PlayerRuntimeCommand::SetPlaybackRate { rate: 1.5 })
        .expect("rate should dispatch");

    let commands = session.drain_native_commands();
    assert_eq!(
        commands,
        vec![
            IosHostCommand::Play,
            IosHostCommand::SetPlaybackRate { rate: 1.5 },
        ]
    );
}

#[test]
fn ios_host_bridge_session_reports_surface_and_interruption_events() {
    let mut session = IosHostBridgeSession::new("placeholder.mp4");
    session.set_surface_attached(true);
    session.report_interruption_changed(true);
    session.report_seek_completed(Duration::from_millis(900));

    let events = session.drain_events();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, IosHostEvent::VideoSurfaceChanged { attached: true }))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        IosHostEvent::InterruptionChanged { interrupted: true }
    )));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, IosHostEvent::SeekCompleted { position_ms: 900 }))
    );
    assert!(session.snapshot().has_video_surface);
}

#[test]
fn ios_host_bridge_session_uses_media_info_duration_for_hls_vod_snapshot() {
    let mut session = IosHostBridgeSession::new("https://example.com/master.m3u8");
    session.session.media_info.duration = Some(Duration::from_secs(24));

    let snapshot = session.snapshot();
    assert_eq!(snapshot.timeline_kind, IosHostTimelineKind::Vod);
    assert!(snapshot.is_seekable);
    assert_eq!(snapshot.duration_ms, Some(24_000));
    assert_eq!(
        snapshot.seekable_range.expect("seekable range").end_ms,
        24_000
    );
}

#[test]
fn ios_host_bridge_session_promotes_unknown_hls_duration_to_live_snapshot() {
    let mut session = IosHostBridgeSession::new("https://example.com/master.m3u8");

    let snapshot = session.snapshot();
    assert_eq!(snapshot.timeline_kind, IosHostTimelineKind::Live);
    assert!(!snapshot.is_seekable);
    assert!(snapshot.seekable_range.is_none());
    assert_eq!(snapshot.duration_ms, None);
    assert_eq!(snapshot.live_edge_ms, None);
}

struct FakeIosBridge;

#[derive(Default)]
struct FakeIosAvBindings {
    commands: Arc<Mutex<Vec<IosNativePlayerCommand>>>,
}

struct RecordingIosCommandSink {
    commands: Arc<Mutex<Vec<IosNativePlayerCommand>>>,
}

struct SurfaceRecordingIosCommandSink {
    commands: Arc<Mutex<Vec<IosNativePlayerCommand>>>,
    surfaces: Arc<Mutex<Vec<Option<PlayerVideoSurfaceTarget>>>>,
}

impl RecordingIosCommandSink {
    fn new(commands: Arc<Mutex<Vec<IosNativePlayerCommand>>>) -> Self {
        Self { commands }
    }
}

impl SurfaceRecordingIosCommandSink {
    fn new(
        commands: Arc<Mutex<Vec<IosNativePlayerCommand>>>,
        surfaces: Arc<Mutex<Vec<Option<PlayerVideoSurfaceTarget>>>>,
    ) -> Self {
        Self { commands, surfaces }
    }
}

impl IosNativeCommandSink for RecordingIosCommandSink {
    fn submit_command(&mut self, command: IosNativePlayerCommand) -> PlayerResult<()> {
        self.commands.lock().expect("commands lock").push(command);
        Ok(())
    }
}

impl IosNativeCommandSink for SurfaceRecordingIosCommandSink {
    fn submit_command(&mut self, command: IosNativePlayerCommand) -> PlayerResult<()> {
        self.commands.lock().expect("commands lock").push(command);
        Ok(())
    }

    fn attach_video_surface(
        &mut self,
        video_surface: PlayerVideoSurfaceTarget,
    ) -> PlayerResult<()> {
        self.surfaces
            .lock()
            .expect("surface lock")
            .push(Some(video_surface));
        Ok(())
    }

    fn detach_video_surface(&mut self) -> PlayerResult<()> {
        self.surfaces.lock().expect("surface lock").push(None);
        Ok(())
    }
}

impl IosAvPlayerBridgeBindings for FakeIosAvBindings {
    fn probe_source(
        &self,
        _context: &IosAvPlayerBridgeContext,
        source: &MediaSource,
        _options: &PlayerRuntimeOptions,
    ) -> PlayerResult<IosNativePlayerProbe> {
        Ok(IosNativePlayerProbe {
            media_info: PlayerMediaInfo {
                source_uri: source.uri().to_owned(),
                source_kind: source.kind(),
                source_protocol: source.protocol(),
                duration: Some(Duration::from_secs(1)),
                bit_rate: None,
                audio_streams: 1,
                video_streams: 1,
                best_video: None,
                best_audio: None,
                track_catalog: test_track_catalog(),
                track_selection: test_track_selection(),
            },
            startup: PlayerRuntimeStartup {
                ffmpeg_initialized: false,
                audio_output: None,
                decoded_audio: None,
                video_decode: None,
                plugin_diagnostics: Vec::new(),
            },
        })
    }

    fn create_command_sink(
        &self,
        _context: IosAvPlayerBridgeContext,
        _source: &MediaSource,
        _options: &PlayerRuntimeOptions,
        _media_info: &PlayerMediaInfo,
        _startup: &PlayerRuntimeStartup,
        controller: super::IosManagedNativeSessionController,
    ) -> PlayerResult<Box<dyn IosNativeCommandSink>> {
        controller.apply_snapshot(IosAvPlayerSnapshot {
            item_status: IosPlayerItemStatus::ReadyToPlay,
            time_control_status: IosTimeControlStatus::Paused,
            playback_rate: 1.0,
            position: Duration::ZERO,
            duration: Some(Duration::from_secs(1)),
            reached_end: false,
            error_message: None,
        });
        Ok(Box::new(RecordingIosCommandSink::new(
            self.commands.clone(),
        )))
    }
}

fn test_media_info() -> PlayerMediaInfo {
    PlayerMediaInfo {
        source_uri: "placeholder.mp4".to_owned(),
        source_kind: player_runtime::MediaSourceKind::Local,
        source_protocol: player_runtime::MediaSourceProtocol::File,
        duration: Some(Duration::from_secs(12)),
        bit_rate: None,
        audio_streams: 1,
        video_streams: 1,
        best_video: None,
        best_audio: None,
        track_catalog: test_track_catalog(),
        track_selection: test_track_selection(),
    }
}

impl IosNativePlayerBridge for FakeIosBridge {
    fn probe_source(
        &self,
        source: &MediaSource,
        _options: &PlayerRuntimeOptions,
    ) -> PlayerResult<IosNativePlayerProbe> {
        Ok(IosNativePlayerProbe {
            media_info: PlayerMediaInfo {
                source_uri: source.uri().to_owned(),
                source_kind: source.kind(),
                source_protocol: source.protocol(),
                duration: Some(Duration::from_secs(1)),
                bit_rate: None,
                audio_streams: 1,
                video_streams: 1,
                best_video: None,
                best_audio: None,
                track_catalog: test_track_catalog(),
                track_selection: test_track_selection(),
            },
            startup: PlayerRuntimeStartup {
                ffmpeg_initialized: false,
                audio_output: None,
                decoded_audio: None,
                video_decode: None,
                plugin_diagnostics: Vec::new(),
            },
        })
    }

    fn initialize_session(
        &self,
        source: MediaSource,
        _options: PlayerRuntimeOptions,
        media_info: &PlayerMediaInfo,
        _startup: &PlayerRuntimeStartup,
    ) -> PlayerResult<IosNativePlayerSessionBootstrap> {
        Ok(IosNativePlayerSessionBootstrap {
            runtime: Box::new(FakeIosSession {
                source_uri: source.uri().to_owned(),
                media_info: media_info.clone(),
            }),
            initial_frame: None,
        })
    }
}

fn test_track_catalog() -> MediaTrackCatalog {
    MediaTrackCatalog {
        tracks: vec![
            MediaTrack {
                id: "video-main".to_owned(),
                kind: MediaTrackKind::Video,
                label: Some("Main Video".to_owned()),
                language: None,
                codec: Some("h264".to_owned()),
                bit_rate: Some(2_000_000),
                width: Some(1280),
                height: Some(720),
                frame_rate: Some(30.0),
                channels: None,
                sample_rate: None,
                is_default: true,
                is_forced: false,
            },
            MediaTrack {
                id: "audio-en".to_owned(),
                kind: MediaTrackKind::Audio,
                label: Some("English".to_owned()),
                language: Some("en".to_owned()),
                codec: Some("aac".to_owned()),
                bit_rate: Some(128_000),
                width: None,
                height: None,
                frame_rate: None,
                channels: Some(2),
                sample_rate: Some(48_000),
                is_default: true,
                is_forced: false,
            },
            MediaTrack {
                id: "subtitle-en".to_owned(),
                kind: MediaTrackKind::Subtitle,
                label: Some("English CC".to_owned()),
                language: Some("en".to_owned()),
                codec: None,
                bit_rate: None,
                width: None,
                height: None,
                frame_rate: None,
                channels: None,
                sample_rate: None,
                is_default: false,
                is_forced: false,
            },
        ],
        adaptive_video: true,
        adaptive_audio: true,
    }
}

fn test_track_selection() -> MediaTrackSelectionSnapshot {
    MediaTrackSelectionSnapshot {
        video: MediaTrackSelection::auto(),
        audio: MediaTrackSelection::auto(),
        subtitle: MediaTrackSelection::disabled(),
        abr_policy: MediaAbrPolicy::default(),
    }
}

struct FakeIosSession {
    source_uri: String,
    media_info: PlayerMediaInfo,
}

impl IosNativePlayerSession for FakeIosSession {
    fn source_uri(&self) -> &str {
        &self.source_uri
    }

    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        super::ios_native_capabilities()
    }

    fn media_info(&self) -> &PlayerMediaInfo {
        &self.media_info
    }

    fn presentation_state(&self) -> PresentationState {
        PresentationState::Ready
    }

    fn playback_rate(&self) -> f32 {
        1.0
    }

    fn progress(&self) -> PlaybackProgress {
        PlaybackProgress::new(Duration::ZERO, self.media_info.duration)
    }

    fn drain_events(&mut self) -> Vec<player_runtime::PlayerRuntimeEvent> {
        Vec::new()
    }

    fn dispatch(
        &mut self,
        _command: PlayerRuntimeCommand,
    ) -> PlayerResult<PlayerRuntimeCommandResult> {
        Err(player_runtime::PlayerError::new(
            PlayerErrorCode::Unsupported,
            "fake ios session does not implement commands",
        ))
    }

    fn advance(&mut self) -> PlayerResult<Option<DecodedVideoFrame>> {
        Ok(None)
    }

    fn next_deadline(&self) -> Option<Instant> {
        None
    }
}
