use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{
    ANDROID_NATIVE_PLAYER_RUNTIME_ADAPTER_ID, AndroidExoPlaybackSnapshot, AndroidExoPlaybackState,
    AndroidExoPlayerBridge, AndroidExoPlayerBridgeBindings, AndroidExoPlayerBridgeContext,
    AndroidExoSeekableRange, AndroidExoStateTracker, AndroidHostBridgeSession, AndroidHostCommand,
    AndroidHostEvent, AndroidHostSnapshot, AndroidHostTimelineKind, AndroidManagedNativeSession,
    AndroidNativeCommandSink, AndroidNativePlayerBridge, AndroidNativePlayerCommand,
    AndroidNativePlayerProbe, AndroidNativePlayerRuntimeAdapterFactory, AndroidNativePlayerSession,
    AndroidNativePlayerSessionBootstrap, AndroidOpaqueHandle,
};
use player_model::MediaSource;
use player_runtime::{
    DecodedVideoFrame, FrameProcessorMode, MediaAbrMode, MediaAbrPolicy, MediaTrack,
    MediaTrackCatalog, MediaTrackKind, MediaTrackSelection, MediaTrackSelectionSnapshot,
    PlaybackProgress, PlayerErrorCode, PlayerMediaInfo, PlayerPluginParticipation,
    PlayerResilienceMetrics, PlayerResult, PlayerRuntimeAdapterBackendFamily,
    PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory, PlayerRuntimeCommand,
    PlayerRuntimeCommandResult, PlayerRuntimeEvent, PlayerRuntimeOptions, PlayerRuntimeStartup,
    PlayerSnapshot, PlayerTimelineSnapshot, PresentationState,
};
#[test]
fn android_factory_exposes_native_capabilities() {
    let factory = AndroidNativePlayerRuntimeAdapterFactory::default();
    let initializer = factory
        .probe_source_with_options(
            MediaSource::new("placeholder.mp4"),
            PlayerRuntimeOptions::default(),
        )
        .expect("android skeleton probe should succeed");

    let capabilities = initializer.capabilities();
    assert_eq!(
        capabilities.adapter_id,
        ANDROID_NATIVE_PLAYER_RUNTIME_ADAPTER_ID
    );
    assert!(capabilities.supports_external_video_surface);
    assert!(capabilities.supports_hardware_decode);
}

#[test]
fn android_frame_processor_config_reports_missing_plugin_diagnostic() {
    let factory = AndroidNativePlayerRuntimeAdapterFactory::default();
    let initializer = factory
        .probe_source_with_options(
            MediaSource::new("placeholder.mp4"),
            PlayerRuntimeOptions::default()
                .with_frame_processor_mode(FrameProcessorMode::DiagnosticsOnly),
        )
        .expect("android skeleton probe should succeed");

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
fn android_factory_is_initialize_unsupported_without_bridge() {
    let factory = AndroidNativePlayerRuntimeAdapterFactory::default();
    let initializer = factory
        .probe_source_with_options(
            MediaSource::new("placeholder.mp4"),
            PlayerRuntimeOptions::default(),
        )
        .expect("android skeleton probe should succeed");

    let error = match initializer.initialize() {
        Ok(_) => panic!("android skeleton initialize should be unsupported"),
        Err(error) => error,
    };
    assert_eq!(error.code(), PlayerErrorCode::Unsupported);
}

#[test]
fn android_factory_can_initialize_with_bridge() {
    let factory =
        AndroidNativePlayerRuntimeAdapterFactory::with_bridge(Arc::new(FakeAndroidBridge));
    let initializer = factory
        .probe_source_with_options(
            MediaSource::new("placeholder.mp4"),
            PlayerRuntimeOptions::default(),
        )
        .expect("android bridge probe should succeed");

    let bootstrap = initializer
        .initialize()
        .expect("android bridge initialize should succeed");
    assert!(bootstrap.initial_frame.is_none());
    assert_eq!(
        bootstrap.runtime.capabilities().backend_family,
        PlayerRuntimeAdapterBackendFamily::NativeAndroid
    );
}

#[test]
fn android_state_tracker_maps_ready_pause_and_end() {
    let mut tracker = AndroidExoStateTracker::default();

    let ready = tracker.observe(&AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ready,
        play_when_ready: false,
        playback_rate: 1.0,
        position: Duration::ZERO,
        duration: Some(Duration::from_secs(12)),
        is_live: false,
        is_seekable: true,
        seekable_range: Some(AndroidExoSeekableRange {
            start: Duration::ZERO,
            end: Duration::from_secs(12),
        }),
        live_edge: None,
    });
    assert_eq!(ready.presentation_state, PresentationState::Ready);
    assert_eq!(ready.emitted_events.len(), 1);

    let playing = tracker.observe(&AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ready,
        play_when_ready: true,
        playback_rate: 1.0,
        position: Duration::from_secs(1),
        duration: Some(Duration::from_secs(12)),
        is_live: false,
        is_seekable: true,
        seekable_range: Some(AndroidExoSeekableRange {
            start: Duration::ZERO,
            end: Duration::from_secs(12),
        }),
        live_edge: None,
    });
    assert_eq!(playing.presentation_state, PresentationState::Playing);

    let paused = tracker.observe(&AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ready,
        play_when_ready: false,
        playback_rate: 1.0,
        position: Duration::from_secs(3),
        duration: Some(Duration::from_secs(12)),
        is_live: false,
        is_seekable: true,
        seekable_range: Some(AndroidExoSeekableRange {
            start: Duration::ZERO,
            end: Duration::from_secs(12),
        }),
        live_edge: None,
    });
    assert_eq!(paused.presentation_state, PresentationState::Paused);

    let finished = tracker.observe(&AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ended,
        play_when_ready: false,
        playback_rate: 1.0,
        position: Duration::from_secs(12),
        duration: Some(Duration::from_secs(12)),
        is_live: false,
        is_seekable: true,
        seekable_range: Some(AndroidExoSeekableRange {
            start: Duration::ZERO,
            end: Duration::from_secs(12),
        }),
        live_edge: None,
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
fn android_state_tracker_reports_playback_rate_changes() {
    let mut tracker = AndroidExoStateTracker::default();

    let first = tracker.observe(&AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ready,
        play_when_ready: false,
        playback_rate: 1.0,
        position: Duration::ZERO,
        duration: None,
        is_live: false,
        is_seekable: false,
        seekable_range: None,
        live_edge: None,
    });
    assert!(first.emitted_events.iter().all(|event| !matches!(
        event,
        player_runtime::PlayerRuntimeEvent::PlaybackRateChanged { .. }
    )));

    let second = tracker.observe(&AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ready,
        play_when_ready: true,
        playback_rate: 1.5,
        position: Duration::from_millis(500),
        duration: None,
        is_live: false,
        is_seekable: false,
        seekable_range: None,
        live_edge: None,
    });
    assert_eq!(second.playback_rate, 1.5);
    assert!(second.emitted_events.iter().any(|event| matches!(
        event,
        player_runtime::PlayerRuntimeEvent::PlaybackRateChanged { rate }
        if (*rate - 1.5).abs() < f32::EPSILON
    )));
}

#[test]
fn android_managed_session_replays_from_start_when_finished() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingAndroidCommandSink::new(commands.clone());
    let mut session = AndroidManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);

    session.apply_snapshot(&AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ended,
        play_when_ready: false,
        playback_rate: 1.0,
        position: Duration::from_secs(9),
        duration: Some(Duration::from_secs(9)),
        is_live: false,
        is_seekable: true,
        seekable_range: Some(AndroidExoSeekableRange {
            start: Duration::ZERO,
            end: Duration::from_secs(9),
        }),
        live_edge: None,
    });

    let result = session
        .dispatch(PlayerRuntimeCommand::Play)
        .expect("play from finished should be bridged");

    assert!(result.applied);
    assert_eq!(result.snapshot.state, PresentationState::Playing);
    assert_eq!(
        *commands.lock().expect("commands lock"),
        vec![
            AndroidNativePlayerCommand::SeekTo {
                position: Duration::ZERO,
            },
            AndroidNativePlayerCommand::Play,
        ]
    );
}

#[test]
fn android_managed_session_validates_pause_and_playback_rate() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingAndroidCommandSink::new(commands.clone());
    let mut session = AndroidManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);

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
fn android_managed_session_updates_from_native_snapshot() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingAndroidCommandSink::new(commands);
    let mut session = AndroidManagedNativeSession::new("placeholder.mp4", test_media_info(), sink);

    session.apply_snapshot(&AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ready,
        play_when_ready: true,
        playback_rate: 1.25,
        position: Duration::from_millis(750),
        duration: Some(Duration::from_secs(5)),
        is_live: false,
        is_seekable: true,
        seekable_range: Some(AndroidExoSeekableRange {
            start: Duration::ZERO,
            end: Duration::from_secs(5),
        }),
        live_edge: None,
    });

    assert_eq!(session.presentation_state(), PresentationState::Playing);
    assert!((session.playback_rate() - 1.25).abs() < f32::EPSILON);
    assert_eq!(session.progress().position(), Duration::from_millis(750));
    let events = session.drain_events();
    assert!(events.iter().any(|event| matches!(
        event,
        player_runtime::PlayerRuntimeEvent::PlaybackRateChanged { rate }
        if (*rate - 1.25).abs() < f32::EPSILON
    )));
}

#[test]
fn android_managed_session_controller_delivers_async_updates() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingAndroidCommandSink::new(commands);
    let (mut session, controller) =
        AndroidManagedNativeSession::with_controller("placeholder.mp4", test_media_info(), sink);

    controller.apply_snapshot(AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ready,
        play_when_ready: true,
        playback_rate: 1.5,
        position: Duration::from_secs(2),
        duration: Some(Duration::from_secs(12)),
        is_live: false,
        is_seekable: true,
        seekable_range: Some(AndroidExoSeekableRange {
            start: Duration::ZERO,
            end: Duration::from_secs(12),
        }),
        live_edge: None,
    });
    controller.report_seek_completed(Duration::from_secs(3));
    controller.report_retry_scheduled(2, Duration::from_millis(1_500));
    controller.report_error(PlayerErrorCode::BackendFailure, "bridge callback failed");

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
fn android_managed_session_controller_delivers_media_info_updates() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingAndroidCommandSink::new(commands);
    let (mut session, controller) = AndroidManagedNativeSession::with_controller(
        "https://example.com/master.m3u8",
        test_media_info(),
        sink,
    );

    let track_catalog = MediaTrackCatalog {
        tracks: vec![
            MediaTrack {
                id: "video-720p".to_owned(),
                kind: MediaTrackKind::Video,
                label: Some("720p".to_owned()),
                language: None,
                codec: Some("avc1.64001f".to_owned()),
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
                codec: Some("mp4a.40.2".to_owned()),
                bit_rate: Some(128_000),
                width: None,
                height: None,
                frame_rate: None,
                channels: Some(2),
                sample_rate: Some(48_000),
                is_default: true,
                is_forced: false,
            },
        ],
        adaptive_video: true,
        adaptive_audio: false,
    };
    let track_selection = MediaTrackSelectionSnapshot {
        video: MediaTrackSelection::track("video-720p"),
        audio: MediaTrackSelection::track("audio-en"),
        subtitle: MediaTrackSelection::disabled(),
        abr_policy: MediaAbrPolicy {
            mode: MediaAbrMode::FixedTrack,
            track_id: Some("video-720p".to_owned()),
            max_bit_rate: None,
            max_width: None,
            max_height: None,
        },
    };

    controller.report_media_info(track_catalog.clone(), track_selection.clone());

    let events = session.drain_events();
    assert_eq!(session.media_info().track_catalog, track_catalog);
    assert_eq!(session.media_info().track_selection, track_selection);
    assert!(events.iter().any(|event| matches!(
        event,
        PlayerRuntimeEvent::MetadataReady(media_info)
        if media_info.track_catalog == track_catalog
            && media_info.track_selection == track_selection
    )));
}

#[test]
fn android_managed_session_dispatches_video_track_selection() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingAndroidCommandSink::new(commands.clone());
    let mut session = AndroidManagedNativeSession::new(
        "https://example.com/master.m3u8",
        test_media_info_with_tracks(),
        sink,
    );

    let result = session
        .dispatch(PlayerRuntimeCommand::SetVideoTrackSelection {
            selection: MediaTrackSelection::track("video-720p"),
        })
        .expect("video track selection should dispatch");

    assert!(result.applied);
    assert_eq!(
        session.media_info().track_selection.video,
        MediaTrackSelection::track("video-720p"),
    );
    assert_eq!(
        session.media_info().track_selection.abr_policy,
        MediaAbrPolicy {
            mode: MediaAbrMode::FixedTrack,
            track_id: Some("video-720p".to_owned()),
            max_bit_rate: None,
            max_width: None,
            max_height: None,
        },
    );
    assert_eq!(
        *commands.lock().expect("commands lock"),
        vec![AndroidNativePlayerCommand::SetVideoTrackSelection {
            selection: MediaTrackSelection::track("video-720p"),
        }],
    );
    let events = session.drain_events();
    assert!(events.iter().any(|event| matches!(
        event,
        PlayerRuntimeEvent::MetadataReady(media_info)
        if media_info.track_selection.video == MediaTrackSelection::track("video-720p")
            && media_info.track_selection.abr_policy.mode == MediaAbrMode::FixedTrack
    )));
}

#[test]
fn android_managed_session_dispatches_constrained_abr_policy() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingAndroidCommandSink::new(commands.clone());
    let mut session = AndroidManagedNativeSession::new(
        "https://example.com/master.m3u8",
        test_media_info_with_tracks(),
        sink,
    );

    let policy = MediaAbrPolicy {
        mode: MediaAbrMode::Constrained,
        track_id: None,
        max_bit_rate: Some(1_000_000),
        max_width: Some(960),
        max_height: Some(540),
    };
    let result = session
        .dispatch(PlayerRuntimeCommand::SetAbrPolicy {
            policy: policy.clone(),
        })
        .expect("constrained ABR should dispatch");

    assert!(result.applied);
    assert_eq!(session.media_info().track_selection.abr_policy, policy);
    assert_eq!(
        *commands.lock().expect("commands lock"),
        vec![AndroidNativePlayerCommand::SetAbrPolicy {
            policy: policy.clone(),
        }],
    );
    let events = session.drain_events();
    assert!(events.iter().any(|event| matches!(
        event,
        PlayerRuntimeEvent::MetadataReady(media_info)
        if media_info.track_selection.abr_policy == policy
    )));
}

#[test]
fn android_managed_session_rejects_unknown_video_track_selection() {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingAndroidCommandSink::new(commands);
    let mut session = AndroidManagedNativeSession::new(
        "https://example.com/master.m3u8",
        test_media_info_with_tracks(),
        sink,
    );

    let error = session
        .dispatch(PlayerRuntimeCommand::SetVideoTrackSelection {
            selection: MediaTrackSelection::track("missing-video"),
        })
        .expect_err("missing video track should fail");

    assert_eq!(error.code(), PlayerErrorCode::InvalidArgument);
}

#[test]
fn android_exoplayer_bridge_bindings_can_initialize_managed_session() {
    let bridge = AndroidExoPlayerBridge::new(
        AndroidExoPlayerBridgeContext {
            java_vm: AndroidOpaqueHandle(1),
            exo_player: AndroidOpaqueHandle(2),
            video_surface: None,
        },
        Arc::new(FakeAndroidExoBindings::default()),
    );
    let factory = AndroidNativePlayerRuntimeAdapterFactory::with_bridge(Arc::new(bridge));
    let initializer = factory
        .probe_source_with_options(
            MediaSource::new("placeholder.mp4"),
            PlayerRuntimeOptions::default(),
        )
        .expect("android exo bridge probe should succeed");

    let bootstrap = initializer
        .initialize()
        .expect("android exo bridge initialize should succeed");
    assert!(bootstrap.initial_frame.is_none());
    assert_eq!(
        bootstrap.runtime.capabilities().backend_family,
        PlayerRuntimeAdapterBackendFamily::NativeAndroid
    );
}

#[test]
fn android_host_snapshot_conversion_preserves_timeline_shape() {
    let snapshot = PlayerSnapshot {
        source_uri: "placeholder.mp4".to_owned(),
        state: PresentationState::Playing,
        has_video_surface: true,
        is_interrupted: false,
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

    let host = AndroidHostSnapshot::from_player_snapshot(&snapshot);
    assert_eq!(host.playback_state, PresentationState::Playing);
    assert!(host.is_buffering);
    assert_eq!(host.position_ms, 5_000);
    assert_eq!(host.duration_ms, Some(20_000));
    assert_eq!(host.seekable_range.expect("seekable range").end_ms, 20_000);
}

#[test]
fn android_host_snapshot_conversion_uses_effective_live_edge_for_live_dvr() {
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

    let host = AndroidHostSnapshot::from_player_snapshot(&snapshot);
    assert_eq!(host.timeline_kind, AndroidHostTimelineKind::LiveDvr);
    assert_eq!(host.live_edge_ms, Some(120_000));
    assert_eq!(host.position_ms, 84_000);
}

#[test]
fn android_host_event_conversion_maps_runtime_events() {
    let rate = AndroidHostEvent::from_runtime_event(&PlayerRuntimeEvent::PlaybackRateChanged {
        rate: 1.25,
    });
    assert!(matches!(
        rate,
        Some(AndroidHostEvent::PlaybackRateChanged { rate })
        if (rate - 1.25).abs() < f32::EPSILON
    ));

    let seek = AndroidHostEvent::from_runtime_event(&PlayerRuntimeEvent::SeekCompleted {
        position: Duration::from_millis(1250),
    });
    assert!(matches!(
        seek,
        Some(AndroidHostEvent::SeekCompleted { position_ms: 1250 })
    ));

    let retry = AndroidHostEvent::from_runtime_event(&PlayerRuntimeEvent::RetryScheduled {
        attempt: 3,
        delay: Duration::from_secs(2),
    });
    assert!(matches!(
        retry,
        Some(AndroidHostEvent::RetryScheduled {
            attempt: 3,
            delay_ms: 2_000,
        })
    ));

    let initialized = AndroidHostEvent::from_runtime_event(&PlayerRuntimeEvent::Initialized(
        PlayerRuntimeStartup {
            ffmpeg_initialized: false,
            audio_output: None,
            decoded_audio: None,
            video_decode: None,
            plugin_diagnostics: Vec::new(),
        },
    ));
    assert!(initialized.is_none());
}

#[test]
fn android_host_bridge_session_drains_native_commands() {
    let mut session = AndroidHostBridgeSession::new("placeholder.mp4");
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
            AndroidHostCommand::Play,
            AndroidHostCommand::SetPlaybackRate { rate: 1.5 },
        ]
    );
}

#[test]
fn android_host_bridge_session_reports_surface_and_seek_events() {
    let mut session = AndroidHostBridgeSession::new("placeholder.mp4");
    session.set_surface_attached(true);
    session.report_seek_completed(Duration::from_millis(900));

    let events = session.drain_events();
    assert!(events.iter().any(|event| matches!(
        event,
        AndroidHostEvent::VideoSurfaceChanged { attached: true }
    )));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AndroidHostEvent::SeekCompleted { position_ms: 900 }))
    );
}

#[test]
fn android_host_bridge_session_uses_media_info_duration_for_hls_vod_snapshot() {
    let mut session = AndroidHostBridgeSession::new("https://example.com/master.m3u8");
    session.session.media_info.duration = Some(Duration::from_secs(24));

    let snapshot = session.snapshot();
    assert_eq!(snapshot.timeline_kind, AndroidHostTimelineKind::Vod);
    assert!(snapshot.is_seekable);
    assert_eq!(snapshot.duration_ms, Some(24_000));
    assert_eq!(
        snapshot.seekable_range.expect("seekable range").end_ms,
        24_000
    );
}

#[test]
fn android_host_bridge_session_promotes_unknown_hls_duration_to_live_snapshot() {
    let mut session = AndroidHostBridgeSession::new("https://example.com/master.m3u8");

    let snapshot = session.snapshot();
    assert_eq!(snapshot.timeline_kind, AndroidHostTimelineKind::Live);
    assert!(!snapshot.is_seekable);
    assert!(snapshot.seekable_range.is_none());
    assert_eq!(snapshot.duration_ms, None);
    assert_eq!(snapshot.live_edge_ms, None);
}

#[test]
fn android_host_bridge_session_promotes_live_seekable_window_to_live_dvr_snapshot() {
    let mut session = AndroidHostBridgeSession::new("https://example.com/live.m3u8");
    session.apply_exo_snapshot(AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ready,
        play_when_ready: true,
        playback_rate: 1.0,
        position: Duration::from_secs(84),
        duration: None,
        is_live: true,
        is_seekable: true,
        seekable_range: Some(AndroidExoSeekableRange {
            start: Duration::ZERO,
            end: Duration::from_secs(120),
        }),
        live_edge: Some(Duration::from_secs(120)),
    });

    let snapshot = session.snapshot();
    assert_eq!(snapshot.timeline_kind, AndroidHostTimelineKind::LiveDvr);
    assert!(snapshot.is_seekable);
    assert_eq!(
        snapshot.seekable_range.expect("seekable range").end_ms,
        120_000
    );
    assert_eq!(snapshot.live_edge_ms, Some(120_000));
    assert_eq!(snapshot.position_ms, 84_000);
    assert_eq!(snapshot.duration_ms, Some(120_000));
}

struct FakeAndroidBridge;

#[derive(Default)]
struct FakeAndroidExoBindings {
    commands: Arc<Mutex<Vec<AndroidNativePlayerCommand>>>,
}

struct RecordingAndroidCommandSink {
    commands: Arc<Mutex<Vec<AndroidNativePlayerCommand>>>,
}

impl RecordingAndroidCommandSink {
    fn new(commands: Arc<Mutex<Vec<AndroidNativePlayerCommand>>>) -> Self {
        Self { commands }
    }
}

impl AndroidNativeCommandSink for RecordingAndroidCommandSink {
    fn submit_command(&mut self, command: AndroidNativePlayerCommand) -> PlayerResult<()> {
        self.commands.lock().expect("commands lock").push(command);
        Ok(())
    }
}

impl AndroidExoPlayerBridgeBindings for FakeAndroidExoBindings {
    fn probe_source(
        &self,
        _context: &AndroidExoPlayerBridgeContext,
        source: &MediaSource,
        _options: &PlayerRuntimeOptions,
    ) -> PlayerResult<AndroidNativePlayerProbe> {
        Ok(AndroidNativePlayerProbe {
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
                track_catalog: Default::default(),
                track_selection: Default::default(),
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
        _context: AndroidExoPlayerBridgeContext,
        _source: &MediaSource,
        _options: &PlayerRuntimeOptions,
        _media_info: &PlayerMediaInfo,
        _startup: &PlayerRuntimeStartup,
        controller: super::AndroidManagedNativeSessionController,
    ) -> PlayerResult<Box<dyn AndroidNativeCommandSink>> {
        controller.apply_snapshot(AndroidExoPlaybackSnapshot {
            playback_state: AndroidExoPlaybackState::Ready,
            play_when_ready: false,
            playback_rate: 1.0,
            position: Duration::ZERO,
            duration: Some(Duration::from_secs(1)),
            is_live: false,
            is_seekable: true,
            seekable_range: Some(AndroidExoSeekableRange {
                start: Duration::ZERO,
                end: Duration::from_secs(1),
            }),
            live_edge: None,
        });
        Ok(Box::new(RecordingAndroidCommandSink::new(
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
        track_catalog: Default::default(),
        track_selection: Default::default(),
    }
}

fn test_media_info_with_tracks() -> PlayerMediaInfo {
    PlayerMediaInfo {
        source_uri: "https://example.com/master.m3u8".to_owned(),
        source_kind: player_runtime::MediaSourceKind::Remote,
        source_protocol: player_runtime::MediaSourceProtocol::Hls,
        duration: Some(Duration::from_secs(120)),
        bit_rate: None,
        audio_streams: 1,
        video_streams: 2,
        best_video: None,
        best_audio: None,
        track_catalog: MediaTrackCatalog {
            tracks: vec![
                MediaTrack {
                    id: "video-720p".to_owned(),
                    kind: MediaTrackKind::Video,
                    label: Some("720p".to_owned()),
                    language: None,
                    codec: Some("avc1.64001f".to_owned()),
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
                    codec: Some("mp4a.40.2".to_owned()),
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
                    id: "text-en".to_owned(),
                    kind: MediaTrackKind::Subtitle,
                    label: Some("English CC".to_owned()),
                    language: Some("en".to_owned()),
                    codec: Some("wvtt".to_owned()),
                    bit_rate: None,
                    width: None,
                    height: None,
                    frame_rate: None,
                    channels: None,
                    sample_rate: None,
                    is_default: true,
                    is_forced: false,
                },
            ],
            adaptive_video: true,
            adaptive_audio: false,
        },
        track_selection: Default::default(),
    }
}

impl AndroidNativePlayerBridge for FakeAndroidBridge {
    fn probe_source(
        &self,
        source: &MediaSource,
        _options: &PlayerRuntimeOptions,
    ) -> PlayerResult<AndroidNativePlayerProbe> {
        Ok(AndroidNativePlayerProbe {
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
                track_catalog: Default::default(),
                track_selection: Default::default(),
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
    ) -> PlayerResult<AndroidNativePlayerSessionBootstrap> {
        Ok(AndroidNativePlayerSessionBootstrap {
            runtime: Box::new(FakeAndroidSession {
                source_uri: source.uri().to_owned(),
                media_info: media_info.clone(),
            }),
            initial_frame: None,
        })
    }
}

struct FakeAndroidSession {
    source_uri: String,
    media_info: PlayerMediaInfo,
}

impl AndroidNativePlayerSession for FakeAndroidSession {
    fn source_uri(&self) -> &str {
        &self.source_uri
    }

    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        super::android_native_capabilities()
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
            "fake android session does not implement commands",
        ))
    }

    fn advance(&mut self) -> PlayerResult<Option<DecodedVideoFrame>> {
        Ok(None)
    }

    fn next_deadline(&self) -> Option<Instant> {
        None
    }
}
