use std::sync::{Arc, Mutex};
use std::time::Duration;

use player_model::MediaSource;
use player_platform_android::{
    AndroidExoPlaybackSnapshot, AndroidExoPlaybackState, AndroidManagedNativeSession,
    AndroidNativeCommandSink, AndroidNativePlayerCommand, AndroidNativePlayerSession,
};
use player_runtime::{
    MAX_PENDING_RUNTIME_EVENTS, MediaAbrMode, MediaAbrPolicy, MediaTrack, MediaTrackCatalog,
    MediaTrackKind, MediaTrackSelection, MediaTrackSelectionSnapshot, PlayerMediaInfo,
    PlayerResult, PlayerRuntimeCommand, PlayerRuntimeEvent, PresentationState,
};

#[derive(Clone, Default)]
struct RecordingCommandSink(Arc<Mutex<Vec<AndroidNativePlayerCommand>>>);

impl RecordingCommandSink {
    fn drain_commands(&self) -> Vec<AndroidNativePlayerCommand> {
        std::mem::take(&mut *self.0.lock().expect("command sink lock"))
    }
}

impl AndroidNativeCommandSink for RecordingCommandSink {
    fn submit_command(&mut self, command: AndroidNativePlayerCommand) -> PlayerResult<()> {
        self.0.lock().expect("command sink lock").push(command);
        Ok(())
    }
}

fn media_info() -> PlayerMediaInfo {
    let source = MediaSource::new("https://example.test/a.mp4");
    let track = |id: &str, kind, codec: &str| MediaTrack {
        id: id.to_owned(),
        kind,
        label: None,
        language: None,
        codec: Some(codec.to_owned()),
        bit_rate: None,
        width: None,
        height: None,
        frame_rate: None,
        channels: None,
        sample_rate: None,
        is_default: true,
        is_forced: false,
        support: Default::default(),
    };
    PlayerMediaInfo {
        source_uri: source.uri().to_owned(),
        source_kind: source.kind(),
        source_protocol: source.protocol(),
        duration: Some(Duration::from_secs(60)),
        bit_rate: None,
        audio_streams: 1,
        video_streams: 1,
        best_video: None,
        best_audio: None,
        track_catalog: MediaTrackCatalog {
            tracks: vec![
                track("video-a", MediaTrackKind::Video, "avc1.64001f"),
                track("audio-a", MediaTrackKind::Audio, "mp4a.40.2"),
                track("subtitle-a", MediaTrackKind::Subtitle, "wvtt"),
            ],
            catalog_revision: 7,
            ..Default::default()
        },
        track_selection: MediaTrackSelectionSnapshot {
            video: MediaTrackSelection::auto(),
            audio: MediaTrackSelection::track("audio-a"),
            subtitle: MediaTrackSelection::track("subtitle-a"),
            abr_policy: MediaAbrPolicy::default(),
        },
    }
}

#[test]
fn abr_command_events_stay_bounded_without_draining_with_native_feedback() {
    let initial_media_info = media_info();
    let commands = RecordingCommandSink::default();
    let (mut session, controller) = AndroidManagedNativeSession::with_controller(
        initial_media_info.source_uri.clone(),
        initial_media_info.clone(),
        commands.clone(),
    );
    let healthy_snapshot = AndroidExoPlaybackSnapshot {
        playback_state: AndroidExoPlaybackState::Ready,
        play_when_ready: true,
        playback_rate: 1.0,
        position: Duration::from_secs(1),
        duration: initial_media_info.duration,
        is_live: false,
        is_seekable: true,
        seekable_range: None,
        live_edge: None,
    };
    session.apply_snapshot(&healthy_snapshot);
    session.drain_events();
    assert_eq!(session.take_dropped_event_count(), 0);

    // Model the retained A session during Android's same-main-turn source-switch window.
    // Consume execution commands and mandatory native feedback, but not runtime events.
    let submitted = MAX_PENDING_RUNTIME_EVENTS + 1;
    let mut accepted = 0;
    let mut native_commands_drained = 0;
    let constrained = MediaAbrPolicy {
        mode: MediaAbrMode::Constrained,
        max_bit_rate: Some(123_456),
        ..MediaAbrPolicy::default()
    };
    for index in 0..submitted {
        let policy = if index % 2 == 0 {
            constrained.clone()
        } else {
            MediaAbrPolicy::default()
        };
        let result = session
            .dispatch(PlayerRuntimeCommand::SetAbrPolicy {
                policy: policy.clone(),
                expected_catalog_revision: None,
            })
            .expect("valid ABR policy should dispatch");
        assert!(result.applied);
        accepted += 1;
        let native_commands = commands.drain_commands();
        assert_eq!(
            native_commands,
            vec![AndroidNativePlayerCommand::SetAbrPolicy {
                policy: policy.clone(),
                expected_catalog_revision: None,
            }]
        );
        native_commands_drained += native_commands.len();
        assert_eq!(result.snapshot.state, PresentationState::Playing);
        assert_eq!(result.snapshot.source_uri, initial_media_info.source_uri);
        assert_eq!(
            result.snapshot.media_info.track_catalog,
            initial_media_info.track_catalog
        );
        assert_eq!(
            result.snapshot.media_info.track_selection.abr_policy,
            policy
        );

        // Mirror dispatchRustCommand's unchanged healthy snapshot and matching track-state
        // feedback. Pump the public controller updates without draining the raw event queue.
        controller.apply_snapshot(healthy_snapshot.clone());
        controller.report_media_info(
            result.snapshot.media_info.track_catalog,
            result.snapshot.media_info.track_selection,
        );
        assert!(session.advance().expect("feedback should apply").is_none());
        assert_eq!(session.pending_update_count(), 0);
        assert_eq!(
            session.media_info().track_selection.video,
            initial_media_info.track_selection.video
        );
        assert_eq!(
            session.media_info().track_selection.audio,
            initial_media_info.track_selection.audio
        );
        assert_eq!(
            session.media_info().track_selection.subtitle,
            initial_media_info.track_selection.subtitle
        );
    }

    let dropped = session.take_dropped_event_count();
    let events = session.drain_events();
    let Some(PlayerRuntimeEvent::MetadataReady(latest)) = events.last() else {
        panic!("latest raw event should contain the command-applied metadata");
    };
    eprintln!(
        "{}",
        serde_json::json!({
            "cap": MAX_PENDING_RUNTIME_EVENTS,
            "submitted": submitted,
            "accepted": accepted,
            "native_commands_drained": native_commands_drained,
            "retained_events": events.len(),
            "dropped_events": dropped,
            "within_cap": events.len() <= MAX_PENDING_RUNTIME_EVENTS,
            "latest_event_max_bit_rate": latest.track_selection.abr_policy.max_bit_rate,
            "current_max_bit_rate": session.media_info().track_selection.abr_policy.max_bit_rate,
            "audio_id": latest.track_selection.audio.track_id,
            "subtitle_id": latest.track_selection.subtitle.track_id,
            "catalog_revision": latest.track_catalog.catalog_revision,
        })
    );
    assert_eq!(accepted, submitted);
    assert_eq!(native_commands_drained, submitted);
    assert_eq!(
        events.len(),
        MAX_PENDING_RUNTIME_EVENTS,
        "raw runtime events must stay bounded"
    );
    assert_eq!(dropped, 1, "the evicted event must be counted exactly once");
    assert_eq!(latest.track_selection.abr_policy, constrained);
    assert_eq!(session.media_info().track_selection.abr_policy, constrained);
    for event in &events {
        let PlayerRuntimeEvent::MetadataReady(info) = event else {
            panic!("unchanged native feedback must not add unrelated events");
        };
        assert_eq!(info.source_uri, initial_media_info.source_uri);
        assert_eq!(info.track_catalog, initial_media_info.track_catalog);
        assert_eq!(
            info.track_selection.video,
            initial_media_info.track_selection.video
        );
        assert_eq!(
            info.track_selection.audio,
            initial_media_info.track_selection.audio
        );
        assert_eq!(
            info.track_selection.subtitle,
            initial_media_info.track_selection.subtitle
        );
    }
    assert!(session.drain_events().is_empty());
    assert_eq!(
        session.take_dropped_event_count(),
        0,
        "drop accounting must reset after taking it"
    );
}
