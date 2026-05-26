use super::{
    FfiPlayerInitializer, PlayerFfiAbrMode, PlayerFfiCallStatus, PlayerFfiCommandKind,
    PlayerFfiError, PlayerFfiErrorCode, PlayerFfiEventKind, PlayerFfiFrameProcessorPolicyAction,
    PlayerFfiFrameProcessorWarningKind, PlayerFfiHandle, PlayerFfiInitializerHandle,
    PlayerFfiMediaInfo, PlayerFfiPlaybackState, PlayerFfiPluginCapabilityKind,
    PlayerFfiPluginDiagnosticStatus, PlayerFfiPluginParticipation, PlayerFfiRuntimeWarningDomain,
    PlayerFfiSnapshot, PlayerFfiStartup, PlayerFfiTrackKind, PlayerFfiVideoFrame,
    into_initializer_handle, player_ffi_event_list_free, player_ffi_initializer_destroy,
    player_ffi_initializer_initialize, player_ffi_initializer_media_info,
    player_ffi_initializer_probe_uri, player_ffi_initializer_startup, player_ffi_media_info_free,
    player_ffi_player_destroy, player_ffi_player_dispatch, player_ffi_player_drain_events,
    player_ffi_player_set_playback_rate, player_ffi_snapshot_free, player_ffi_startup_free,
    player_ffi_video_frame_free,
};
use crate::FfiErrorCode;
use player_runtime::{
    DecodedVideoFrame, FrameProcessorPolicyAction, FrameProcessorWarning,
    FrameProcessorWarningKind, MediaAbrMode, MediaAbrPolicy, MediaSourceKind, MediaSourceProtocol,
    MediaTrack, MediaTrackCatalog, MediaTrackKind, MediaTrackSelection,
    MediaTrackSelectionSnapshot, PlaybackProgress, PlayerAudioInfo, PlayerMediaInfo,
    PlayerPluginCapabilitySummary, PlayerPluginCodecCapability,
    PlayerPluginDecoderCapabilitySummary, PlayerPluginDiagnostic, PlayerPluginDiagnosticStatus,
    PlayerPluginFrameProcessorCapabilitySummary, PlayerPluginParticipation, PlayerResult,
    PlayerRuntimeAdapter, PlayerRuntimeAdapterBackendFamily, PlayerRuntimeAdapterBootstrap,
    PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory, PlayerRuntimeAdapterInitializer,
    PlayerRuntimeCommand, PlayerRuntimeCommandResult, PlayerRuntimeEvent, PlayerRuntimeInitializer,
    PlayerRuntimeOptions, PlayerRuntimeStartup, PlayerRuntimeWarning, PlayerVideoInfo,
    PresentationState, VideoPixelFormat,
};
use std::ffi::{CStr, CString};
use std::ptr;
use std::time::Duration;

#[test]
fn initializer_probe_uri_rejects_null_output_pointer() {
    unsafe {
        let uri = CString::new("https://example.com/master.m3u8").expect("valid uri");
        let mut error = PlayerFfiError::default();

        let status = player_ffi_initializer_probe_uri(uri.as_ptr(), ptr::null_mut(), &mut error);

        assert_eq!(status, PlayerFfiCallStatus::Error);
        assert_eq!(error.code, PlayerFfiErrorCode::NullPointer);
        assert_eq!(copy_c_string(error.message), "out_initializer was null");
        super::player_ffi_error_free(&mut error);
    }
}

#[test]
fn initializer_probe_uri_rejects_null_output_without_error_pointer() {
    unsafe {
        let uri = CString::new("https://example.com/master.m3u8").expect("valid uri");

        let status =
            player_ffi_initializer_probe_uri(uri.as_ptr(), ptr::null_mut(), ptr::null_mut());

        assert_eq!(status, PlayerFfiCallStatus::Error);
    }
}

#[test]
fn initializer_initialize_and_dispatch_accept_optional_frame_output() {
    unsafe {
        let initializer = fake_initializer("https://example.com/master.m3u8");
        let handle = into_initializer_handle(initializer).expect("initializer handle should fit");
        let mut player_handle = PlayerFfiHandle::default();
        let mut has_initial_frame = false;
        let mut initial_frame = PlayerFfiVideoFrame::default();
        let mut startup = PlayerFfiStartup::default();
        let mut error = PlayerFfiError::default();

        let status = player_ffi_initializer_initialize(
            handle,
            &mut player_handle,
            &mut has_initial_frame,
            &mut initial_frame,
            &mut startup,
            &mut error,
        );

        assert_eq!(status, PlayerFfiCallStatus::Ok);
        assert_ne!(player_handle.raw, 0);
        assert!(has_initial_frame);
        assert_eq!(initial_frame.width, 2);
        assert!(startup.ffmpeg_initialized);
        assert_eq!(
            copy_c_string(startup.video_decode.hardware_backend),
            "stub-hw"
        );
        player_ffi_video_frame_free(&mut initial_frame);
        player_ffi_startup_free(&mut startup);

        let mut applied = false;
        let mut snapshot = PlayerFfiSnapshot::default();
        let dispatch_status = player_ffi_player_dispatch(
            player_handle,
            PlayerFfiCommandKind::Play,
            0,
            &mut applied,
            ptr::null_mut(),
            &mut snapshot,
            &mut error,
        );

        assert_eq!(dispatch_status, PlayerFfiCallStatus::Ok);
        assert!(applied);
        assert_eq!(snapshot.state, PlayerFfiPlaybackState::Playing);
        assert_eq!(
            copy_c_string(snapshot.source_uri),
            "https://example.com/master.m3u8"
        );
        assert_eq!(snapshot.media_info.track_catalog.len, 1);
        assert_eq!(
            (*snapshot.media_info.track_catalog.tracks).kind,
            PlayerFfiTrackKind::Video
        );
        assert_eq!(
            snapshot.media_info.track_selection.abr_policy.mode,
            PlayerFfiAbrMode::FixedTrack
        );
        player_ffi_snapshot_free(&mut snapshot);

        let mut events = super::PlayerFfiEventList::default();
        let drain_status = player_ffi_player_drain_events(player_handle, &mut events, &mut error);
        assert_eq!(drain_status, PlayerFfiCallStatus::Ok);
        assert_eq!(events.len, 1);
        assert_eq!((*events.ptr).kind, PlayerFfiEventKind::PlaybackStateChanged);
        assert_eq!(
            (*events.ptr).playback_state,
            PlayerFfiPlaybackState::Playing
        );
        player_ffi_event_list_free(&mut events);

        let destroy_status = player_ffi_player_destroy(player_handle, &mut error);
        assert_eq!(destroy_status, PlayerFfiCallStatus::Ok);
    }
}

#[test]
fn ffi_call_converts_panics_into_backend_failure() {
    let mut error = super::owned_api_error(PlayerFfiErrorCode::InvalidState, "stale error");

    let status = super::ffi_call(&mut error, || -> PlayerFfiCallStatus {
        panic!("ffi panic smoke");
    });

    assert_eq!(status, PlayerFfiCallStatus::Error);
    assert_eq!(error.code, PlayerFfiErrorCode::BackendFailure);
    assert_eq!(error.category, super::PlayerFfiErrorCategory::Platform);
    assert!(copy_c_string(error.message).contains("ffi panic smoke"));
    unsafe { super::player_ffi_error_free(&mut error) };
}

#[test]
fn ffi_error_code_ordinals_append_new_player_error_codes() {
    assert_eq!(PlayerFfiErrorCode::None as i32, 0);
    assert_eq!(PlayerFfiErrorCode::NullPointer as i32, 1);
    assert_eq!(PlayerFfiErrorCode::InvalidUtf8 as i32, 2);
    assert_eq!(PlayerFfiErrorCode::InvalidArgument as i32, 3);
    assert_eq!(PlayerFfiErrorCode::InvalidState as i32, 4);
    assert_eq!(PlayerFfiErrorCode::InvalidSource as i32, 5);
    assert_eq!(PlayerFfiErrorCode::BackendFailure as i32, 6);
    assert_eq!(PlayerFfiErrorCode::AudioOutputUnavailable as i32, 7);
    assert_eq!(PlayerFfiErrorCode::DecodeFailure as i32, 8);
    assert_eq!(PlayerFfiErrorCode::SeekFailure as i32, 9);
    assert_eq!(PlayerFfiErrorCode::Unsupported as i32, 10);
    assert_eq!(PlayerFfiErrorCode::CommandChannelClosed as i32, 11);
    assert_eq!(PlayerFfiErrorCode::EventChannelClosed as i32, 12);
    assert_eq!(PlayerFfiErrorCode::Cancelled as i32, 13);
    assert_eq!(PlayerFfiErrorCode::Timeout as i32, 14);
}

#[test]
fn bridge_error_code_mapping_preserves_legacy_and_appended_values() {
    let cases = [
        (
            FfiErrorCode::InvalidArgument,
            PlayerFfiErrorCode::InvalidArgument,
        ),
        (FfiErrorCode::InvalidState, PlayerFfiErrorCode::InvalidState),
        (
            FfiErrorCode::InvalidSource,
            PlayerFfiErrorCode::InvalidSource,
        ),
        (
            FfiErrorCode::BackendFailure,
            PlayerFfiErrorCode::BackendFailure,
        ),
        (
            FfiErrorCode::AudioOutputUnavailable,
            PlayerFfiErrorCode::AudioOutputUnavailable,
        ),
        (
            FfiErrorCode::DecodeFailure,
            PlayerFfiErrorCode::DecodeFailure,
        ),
        (FfiErrorCode::SeekFailure, PlayerFfiErrorCode::SeekFailure),
        (FfiErrorCode::Unsupported, PlayerFfiErrorCode::Unsupported),
        (
            FfiErrorCode::CommandChannelClosed,
            PlayerFfiErrorCode::CommandChannelClosed,
        ),
        (
            FfiErrorCode::EventChannelClosed,
            PlayerFfiErrorCode::EventChannelClosed,
        ),
        (FfiErrorCode::Cancelled, PlayerFfiErrorCode::Cancelled),
        (FfiErrorCode::Timeout, PlayerFfiErrorCode::Timeout),
    ];

    for (bridge_code, ffi_code) in cases {
        assert_eq!(PlayerFfiErrorCode::from(bridge_code), ffi_code);
    }
}

#[test]
fn player_drain_events_preserves_order_and_is_one_shot() {
    unsafe {
        let initializer = fake_initializer("https://example.com/master.m3u8");
        let handle = into_initializer_handle(initializer).expect("initializer handle should fit");
        let mut player_handle = PlayerFfiHandle::default();
        let mut has_initial_frame = false;
        let mut initial_frame = PlayerFfiVideoFrame::default();
        let mut startup = PlayerFfiStartup::default();
        let mut error = PlayerFfiError::default();

        let status = player_ffi_initializer_initialize(
            handle,
            &mut player_handle,
            &mut has_initial_frame,
            &mut initial_frame,
            &mut startup,
            &mut error,
        );
        assert_eq!(status, PlayerFfiCallStatus::Ok);
        assert_ne!(player_handle.raw, 0);
        player_ffi_video_frame_free(&mut initial_frame);
        player_ffi_startup_free(&mut startup);

        let mut applied = false;
        let mut snapshot = PlayerFfiSnapshot::default();
        let play_status = player_ffi_player_dispatch(
            player_handle,
            PlayerFfiCommandKind::Play,
            0,
            &mut applied,
            ptr::null_mut(),
            &mut snapshot,
            &mut error,
        );
        assert_eq!(play_status, PlayerFfiCallStatus::Ok);
        assert!(applied);
        player_ffi_snapshot_free(&mut snapshot);

        let rate_status = player_ffi_player_set_playback_rate(
            player_handle,
            1.25,
            &mut applied,
            &mut snapshot,
            &mut error,
        );
        assert_eq!(rate_status, PlayerFfiCallStatus::Ok);
        assert!(applied);
        player_ffi_snapshot_free(&mut snapshot);

        let mut events = super::PlayerFfiEventList::default();
        let drain_status = player_ffi_player_drain_events(player_handle, &mut events, &mut error);
        assert_eq!(drain_status, PlayerFfiCallStatus::Ok);
        assert_eq!(events.len, 2);
        assert_eq!((*events.ptr).kind, PlayerFfiEventKind::PlaybackStateChanged);
        assert_eq!(
            (*events.ptr.add(1)).kind,
            PlayerFfiEventKind::PlaybackRateChanged
        );
        assert_eq!((*events.ptr.add(1)).playback_rate, 1.25);
        player_ffi_event_list_free(&mut events);

        let second_drain_status =
            player_ffi_player_drain_events(player_handle, &mut events, &mut error);
        assert_eq!(second_drain_status, PlayerFfiCallStatus::Ok);
        assert_eq!(events.len, 0);
        assert!(events.ptr.is_null());
        player_ffi_event_list_free(&mut events);

        let destroy_status = player_ffi_player_destroy(player_handle, &mut error);
        assert_eq!(destroy_status, PlayerFfiCallStatus::Ok);
    }
}

#[test]
fn player_drain_events_preserves_runtime_warning_payload() {
    unsafe {
        let initializer = fake_initializer("https://example.com/warning.m3u8");
        let handle = into_initializer_handle(initializer).expect("initializer handle should fit");
        let mut player_handle = PlayerFfiHandle::default();
        let mut has_initial_frame = false;
        let mut initial_frame = PlayerFfiVideoFrame::default();
        let mut startup = PlayerFfiStartup::default();
        let mut error = PlayerFfiError::default();

        let status = player_ffi_initializer_initialize(
            handle,
            &mut player_handle,
            &mut has_initial_frame,
            &mut initial_frame,
            &mut startup,
            &mut error,
        );
        assert_eq!(status, PlayerFfiCallStatus::Ok);
        player_ffi_video_frame_free(&mut initial_frame);
        player_ffi_startup_free(&mut startup);

        let mut applied = false;
        let mut snapshot = PlayerFfiSnapshot::default();
        let dispatch_status = player_ffi_player_dispatch(
            player_handle,
            PlayerFfiCommandKind::SeekTo,
            42,
            &mut applied,
            ptr::null_mut(),
            &mut snapshot,
            &mut error,
        );
        assert_eq!(dispatch_status, PlayerFfiCallStatus::Ok);
        player_ffi_snapshot_free(&mut snapshot);

        let mut events = super::PlayerFfiEventList::default();
        let drain_status = player_ffi_player_drain_events(player_handle, &mut events, &mut error);
        assert_eq!(drain_status, PlayerFfiCallStatus::Ok);
        assert_eq!(events.len, 1);

        let event = &*events.ptr;
        assert_eq!(event.kind, PlayerFfiEventKind::Warning);
        assert_eq!(
            event.warning.domain,
            PlayerFfiRuntimeWarningDomain::FrameProcessor
        );
        let warning = &event.warning.frame_processor;
        assert_eq!(
            warning.kind,
            PlayerFfiFrameProcessorWarningKind::DeadlineMissed
        );
        assert_eq!(copy_c_string(warning.plugin_name), "fixture-processor");
        assert_eq!(warning.processor_index, 2);
        assert!(warning.has_frame_id);
        assert_eq!(warning.frame_id, 7);
        assert!(warning.has_frame_pts_us);
        assert_eq!(warning.frame_pts_us, 33_000);
        assert_eq!(copy_c_string(warning.input_handle_kind), "CvPixelBuffer");
        assert_eq!(copy_c_string(warning.output_handle_kind), "CvPixelBuffer");
        assert!(warning.has_process_time_us);
        assert_eq!(warning.process_time_us, 50_000);
        assert_eq!(
            warning.policy_action,
            PlayerFfiFrameProcessorPolicyAction::BypassOriginalFrame
        );
        assert_eq!(
            copy_c_string(warning.message),
            "processor output missed frame deadline"
        );
        player_ffi_event_list_free(&mut events);

        let destroy_status = player_ffi_player_destroy(player_handle, &mut error);
        assert_eq!(destroy_status, PlayerFfiCallStatus::Ok);
    }
}

#[test]
fn initializer_media_info_and_startup_round_trip_fake_runtime_payload() {
    unsafe {
        let handle = into_initializer_handle(fake_initializer("https://example.com/video.mp4"))
            .expect("initializer handle should fit");
        let mut media_info = PlayerFfiMediaInfo::default();
        let mut startup = PlayerFfiStartup::default();
        let mut error = PlayerFfiError::default();

        let media_status = player_ffi_initializer_media_info(handle, &mut media_info, &mut error);
        assert_eq!(media_status, PlayerFfiCallStatus::Ok);
        assert_eq!(
            copy_c_string(media_info.source_uri),
            "https://example.com/video.mp4"
        );
        assert_eq!(
            media_info.source_kind,
            super::PlayerFfiMediaSourceKind::Remote
        );
        assert_eq!(
            media_info.source_protocol,
            super::PlayerFfiMediaSourceProtocol::Progressive
        );
        assert!(media_info.has_duration);
        assert_eq!(media_info.duration_ms, 60_000);
        assert!(media_info.has_best_video);
        assert_eq!(copy_c_string(media_info.best_video.codec), "h264");
        assert_eq!(media_info.track_catalog.len, 1);
        assert_eq!(
            (*media_info.track_catalog.tracks).kind,
            PlayerFfiTrackKind::Video
        );
        player_ffi_media_info_free(&mut media_info);

        let startup_status = player_ffi_initializer_startup(handle, &mut startup, &mut error);
        assert_eq!(startup_status, PlayerFfiCallStatus::Ok);
        assert!(startup.ffmpeg_initialized);
        assert!(startup.has_audio_output);
        assert_eq!(
            copy_c_string(startup.audio_output.device_name),
            "Stub Speaker"
        );
        assert!(startup.has_video_decode);
        assert_eq!(
            copy_c_string(startup.video_decode.hardware_backend),
            "stub-hw"
        );
        assert_eq!(startup.plugin_diagnostics_len, 2);
        let diagnostics =
            std::slice::from_raw_parts(startup.plugin_diagnostics, startup.plugin_diagnostics_len);
        assert_eq!(
            copy_c_string(diagnostics[0].path),
            "/tmp/player-decoder-fixture.dylib"
        );
        assert_eq!(copy_c_string(diagnostics[0].plugin_name), "fixture-decoder");
        assert_eq!(
            diagnostics[0].status,
            PlayerFfiPluginDiagnosticStatus::DecoderSupported
        );
        assert_eq!(
            diagnostics[0].capability.kind,
            PlayerFfiPluginCapabilityKind::Decoder
        );
        assert_eq!(diagnostics[0].capability.decoder.codecs_len, 1);
        assert_eq!(
            copy_c_string((*diagnostics[0].capability.decoder.codecs).media_kind),
            "Video"
        );
        assert_eq!(
            copy_c_string((*diagnostics[0].capability.decoder.codecs).codec),
            "h264"
        );
        assert_eq!(diagnostics[0].capability.decoder.legacy_codecs_len, 1);
        assert_eq!(
            copy_c_string(*diagnostics[0].capability.decoder.legacy_codecs),
            "Video:h264"
        );
        assert!(
            diagnostics[0]
                .capability
                .decoder
                .supports_native_frame_output
        );
        assert_eq!(
            diagnostics[1].status,
            PlayerFfiPluginDiagnosticStatus::FrameProcessorSupported
        );
        assert_eq!(
            diagnostics[1].capability.kind,
            PlayerFfiPluginCapabilityKind::FrameProcessor
        );
        assert_eq!(
            diagnostics[1]
                .capability
                .frame_processor
                .accepted_input_handle_kinds_len,
            1
        );
        assert_eq!(
            copy_c_string(
                *diagnostics[1]
                    .capability
                    .frame_processor
                    .accepted_input_handle_kinds
            ),
            "CvPixelBuffer"
        );
        assert_eq!(
            diagnostics[1]
                .capability
                .frame_processor
                .max_in_flight_frames,
            4
        );
        assert_eq!(
            diagnostics[0].participation,
            PlayerFfiPluginParticipation::Participated
        );
        assert_eq!(
            diagnostics[1].participation,
            PlayerFfiPluginParticipation::Available
        );
        let fixture = include_str!("../../../../../fixtures/contracts/plugin_diagnostics.json");
        assert!(fixture.contains("\"status\": \"decoderSupported\""));
        assert!(fixture.contains("\"status\": \"frameProcessorSupported\""));
        assert!(fixture.contains("\"participation\": \"participated\""));
        assert!(fixture.contains("\"participation\": \"available\""));
        assert!(fixture.contains("\"codec\": \"h264\""));
        assert!(fixture.contains("\"maxInFlightFrames\": 4"));
        player_ffi_startup_free(&mut startup);

        let destroy_status = player_ffi_initializer_destroy(handle, &mut error);
        assert_eq!(destroy_status, PlayerFfiCallStatus::Ok);
    }
}

#[test]
fn initializer_initialize_rejects_invalid_handle() {
    unsafe {
        let mut player_handle = PlayerFfiHandle::default();
        let mut has_initial_frame = false;
        let mut initial_frame = PlayerFfiVideoFrame::default();
        let mut startup = PlayerFfiStartup::default();
        let mut error = PlayerFfiError::default();

        let status = player_ffi_initializer_initialize(
            PlayerFfiInitializerHandle::default(),
            &mut player_handle,
            &mut has_initial_frame,
            &mut initial_frame,
            &mut startup,
            &mut error,
        );

        assert_eq!(status, PlayerFfiCallStatus::Error);
        assert_eq!(error.code, PlayerFfiErrorCode::InvalidState);
        assert_eq!(
            copy_c_string(error.message),
            "initializer handle was invalid"
        );
        super::player_ffi_error_free(&mut error);
    }
}

#[test]
fn initializer_handle_becomes_invalid_after_initialize_consumes_it() {
    unsafe {
        let handle = into_initializer_handle(fake_initializer("https://example.com/consumed.m3u8"))
            .expect("initializer handle should fit");
        let mut player_handle = PlayerFfiHandle::default();
        let mut has_initial_frame = false;
        let mut initial_frame = PlayerFfiVideoFrame::default();
        let mut startup = PlayerFfiStartup::default();
        let mut error = PlayerFfiError::default();

        let status = player_ffi_initializer_initialize(
            handle,
            &mut player_handle,
            &mut has_initial_frame,
            &mut initial_frame,
            &mut startup,
            &mut error,
        );
        assert_eq!(status, PlayerFfiCallStatus::Ok);
        player_ffi_video_frame_free(&mut initial_frame);
        player_ffi_startup_free(&mut startup);

        let mut consumed_startup = PlayerFfiStartup::default();
        let startup_status =
            player_ffi_initializer_startup(handle, &mut consumed_startup, &mut error);
        assert_eq!(startup_status, PlayerFfiCallStatus::Error);
        assert_eq!(error.code, PlayerFfiErrorCode::InvalidState);
        assert_eq!(
            copy_c_string(error.message),
            "initializer handle was invalid"
        );
        super::player_ffi_error_free(&mut error);

        let destroy_status = player_ffi_initializer_destroy(handle, &mut error);
        assert_eq!(destroy_status, PlayerFfiCallStatus::Error);
        assert_eq!(error.code, PlayerFfiErrorCode::InvalidState);
        assert_eq!(
            copy_c_string(error.message),
            "initializer handle was invalid"
        );
        super::player_ffi_error_free(&mut error);

        let player_destroy_status = player_ffi_player_destroy(player_handle, &mut error);
        assert_eq!(player_destroy_status, PlayerFfiCallStatus::Ok);
    }
}

#[test]
fn player_destroy_rejects_double_destroy_with_invalid_state() {
    unsafe {
        let handle =
            into_initializer_handle(fake_initializer("https://example.com/double-destroy.m3u8"))
                .expect("initializer handle should fit");
        let mut player_handle = PlayerFfiHandle::default();
        let mut has_initial_frame = false;
        let mut initial_frame = PlayerFfiVideoFrame::default();
        let mut startup = PlayerFfiStartup::default();
        let mut error = PlayerFfiError::default();

        let status = player_ffi_initializer_initialize(
            handle,
            &mut player_handle,
            &mut has_initial_frame,
            &mut initial_frame,
            &mut startup,
            &mut error,
        );
        assert_eq!(status, PlayerFfiCallStatus::Ok);
        player_ffi_video_frame_free(&mut initial_frame);
        player_ffi_startup_free(&mut startup);

        let first_destroy_status = player_ffi_player_destroy(player_handle, &mut error);
        assert_eq!(first_destroy_status, PlayerFfiCallStatus::Ok);

        let second_destroy_status = player_ffi_player_destroy(player_handle, &mut error);
        assert_eq!(second_destroy_status, PlayerFfiCallStatus::Error);
        assert_eq!(error.code, PlayerFfiErrorCode::InvalidState);
        assert_eq!(copy_c_string(error.message), "player handle was invalid");
        super::player_ffi_error_free(&mut error);
    }
}

fn copy_c_string(value: *mut std::ffi::c_char) -> String {
    if value.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(value).to_string_lossy().into_owned() }
}

fn fake_initializer(uri: &str) -> FfiPlayerInitializer {
    let factory = FakeRuntimeAdapterFactory;
    let inner = PlayerRuntimeInitializer::probe_uri_with_options_and_factory(
        uri,
        PlayerRuntimeOptions::default(),
        &factory,
    )
    .expect("fake initializer should probe");
    FfiPlayerInitializer { inner }
}

struct FakeRuntimeAdapterFactory;

impl PlayerRuntimeAdapterFactory for FakeRuntimeAdapterFactory {
    fn adapter_id(&self) -> &'static str {
        "ffi-test-adapter"
    }

    fn probe_source_with_options(
        &self,
        source: player_model::MediaSource,
        _options: PlayerRuntimeOptions,
    ) -> PlayerResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
        Ok(Box::new(FakeRuntimeAdapterInitializer::new(
            source.uri().to_owned(),
        )))
    }
}

struct FakeRuntimeAdapterInitializer {
    source_uri: String,
    media_info: PlayerMediaInfo,
    startup: PlayerRuntimeStartup,
    initial_frame: DecodedVideoFrame,
    dispatch_frame: DecodedVideoFrame,
}

impl FakeRuntimeAdapterInitializer {
    fn new(source_uri: String) -> Self {
        let track_id = "video:1080p".to_owned();
        let media_info = PlayerMediaInfo {
            source_uri: source_uri.clone(),
            source_kind: MediaSourceKind::Remote,
            source_protocol: if source_uri.ends_with(".m3u8") {
                MediaSourceProtocol::Hls
            } else {
                MediaSourceProtocol::Progressive
            },
            duration: Some(Duration::from_secs(60)),
            bit_rate: Some(2_400_000),
            audio_streams: 1,
            video_streams: 1,
            best_video: Some(PlayerVideoInfo {
                codec: "h264".to_owned(),
                width: 1920,
                height: 1080,
                frame_rate: Some(30.0),
            }),
            best_audio: Some(PlayerAudioInfo {
                codec: "aac".to_owned(),
                sample_rate: 48_000,
                channels: 2,
            }),
            track_catalog: MediaTrackCatalog {
                tracks: vec![MediaTrack {
                    id: track_id.clone(),
                    kind: MediaTrackKind::Video,
                    label: Some("1080p".to_owned()),
                    language: None,
                    codec: Some("avc1".to_owned()),
                    bit_rate: Some(2_400_000),
                    width: Some(1920),
                    height: Some(1080),
                    frame_rate: Some(30.0),
                    channels: None,
                    sample_rate: None,
                    is_default: true,
                    is_forced: false,
                }],
                adaptive_video: true,
                adaptive_audio: false,
            },
            track_selection: MediaTrackSelectionSnapshot {
                video: MediaTrackSelection::auto(),
                audio: MediaTrackSelection::auto(),
                subtitle: MediaTrackSelection::disabled(),
                abr_policy: MediaAbrPolicy {
                    mode: MediaAbrMode::FixedTrack,
                    track_id: Some(track_id),
                    max_bit_rate: Some(2_400_000),
                    max_width: Some(1920),
                    max_height: Some(1080),
                },
            },
        };
        let startup = PlayerRuntimeStartup {
            ffmpeg_initialized: true,
            audio_output: Some(player_runtime::PlayerAudioOutputInfo {
                device_name: Some("Stub Speaker".to_owned()),
                channels: Some(2),
                sample_rate: Some(48_000),
                sample_format: Some("f32".to_owned()),
            }),
            decoded_audio: None,
            video_decode: Some(player_runtime::PlayerVideoDecodeInfo {
                selected_mode: player_runtime::PlayerVideoDecodeMode::Hardware,
                hardware_available: true,
                hardware_backend: Some("stub-hw".to_owned()),
                fallback_reason: None,
            }),
            plugin_diagnostics: vec![
                PlayerPluginDiagnostic {
                    path: "/tmp/player-decoder-fixture.dylib".to_owned(),
                    plugin_name: Some("fixture-decoder".to_owned()),
                    plugin_kind: Some("decoder".to_owned()),
                    status: PlayerPluginDiagnosticStatus::DecoderSupported,
                    message: Some("fixture decoder loaded".to_owned()),
                    participation: PlayerPluginParticipation::Participated,
                    capability: Some(PlayerPluginCapabilitySummary::Decoder(
                        PlayerPluginDecoderCapabilitySummary {
                            codecs: vec![PlayerPluginCodecCapability {
                                media_kind: "Video".to_owned(),
                                codec: "h264".to_owned(),
                            }],
                            legacy_codecs: vec!["Video:h264".to_owned()],
                            supports_native_frame_output: true,
                            supports_hardware_decode: true,
                            supports_cpu_video_frames: false,
                            supports_audio_frames: false,
                            supports_gpu_handles: true,
                            supports_flush: true,
                            supports_drain: true,
                            max_sessions: Some(1),
                        },
                    )),
                },
                PlayerPluginDiagnostic {
                    path: "/tmp/player-frame-processor-fixture.dylib".to_owned(),
                    plugin_name: Some("fixture-processor".to_owned()),
                    plugin_kind: Some("frame_processor".to_owned()),
                    status: PlayerPluginDiagnosticStatus::FrameProcessorSupported,
                    message: Some("fixture processor loaded".to_owned()),
                    participation: PlayerPluginParticipation::Available,
                    capability: Some(PlayerPluginCapabilitySummary::FrameProcessor(
                        PlayerPluginFrameProcessorCapabilitySummary {
                            accepted_input_handle_kinds: vec!["CvPixelBuffer".to_owned()],
                            output_handle_kinds: vec!["CvPixelBuffer".to_owned()],
                            supports_video_frames: true,
                            supports_in_place_passthrough: true,
                            preserves_dimensions: true,
                            may_change_dimensions: false,
                            preserves_color_metadata: true,
                            preserves_hdr_metadata: true,
                            supports_flush: true,
                            max_sessions: Some(2),
                            max_in_flight_frames: Some(4),
                        },
                    )),
                },
            ],
        };

        Self {
            source_uri,
            media_info,
            startup,
            initial_frame: fake_frame(10),
            dispatch_frame: fake_frame(20),
        }
    }
}

impl PlayerRuntimeAdapterInitializer for FakeRuntimeAdapterInitializer {
    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        fake_capabilities()
    }

    fn media_info(&self) -> PlayerMediaInfo {
        self.media_info.clone()
    }

    fn startup(&self) -> PlayerRuntimeStartup {
        self.startup.clone()
    }

    fn initialize(self: Box<Self>) -> PlayerResult<PlayerRuntimeAdapterBootstrap> {
        Ok(PlayerRuntimeAdapterBootstrap {
            runtime: Box::new(FakeRuntimeAdapter {
                source_uri: self.source_uri,
                media_info: self.media_info,
                state: PresentationState::Ready,
                playback_rate: 1.0,
                progress: PlaybackProgress::new(
                    Duration::from_secs(12),
                    Some(Duration::from_secs(60)),
                ),
                pending_events: Vec::new(),
                dispatch_frame: self.dispatch_frame,
            }),
            initial_frame: Some(self.initial_frame),
            startup: self.startup,
        })
    }
}

struct FakeRuntimeAdapter {
    source_uri: String,
    media_info: PlayerMediaInfo,
    state: PresentationState,
    playback_rate: f32,
    progress: PlaybackProgress,
    pending_events: Vec<PlayerRuntimeEvent>,
    dispatch_frame: DecodedVideoFrame,
}

impl PlayerRuntimeAdapter for FakeRuntimeAdapter {
    fn source_uri(&self) -> &str {
        &self.source_uri
    }

    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        fake_capabilities()
    }

    fn media_info(&self) -> &PlayerMediaInfo {
        &self.media_info
    }

    fn presentation_state(&self) -> PresentationState {
        self.state
    }

    fn playback_rate(&self) -> f32 {
        self.playback_rate
    }

    fn progress(&self) -> PlaybackProgress {
        self.progress
    }

    fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
        self.pending_events.drain(..).collect()
    }

    fn dispatch(
        &mut self,
        command: PlayerRuntimeCommand,
    ) -> PlayerResult<PlayerRuntimeCommandResult> {
        match command {
            PlayerRuntimeCommand::Play => {
                self.state = PresentationState::Playing;
                self.pending_events
                    .push(PlayerRuntimeEvent::PlaybackStateChanged(
                        PresentationState::Playing,
                    ));
                Ok(PlayerRuntimeCommandResult {
                    applied: true,
                    frame: Some(self.dispatch_frame.clone()),
                    snapshot: PlayerRuntimeAdapter::snapshot(self),
                })
            }
            PlayerRuntimeCommand::SetPlaybackRate { rate } => {
                self.playback_rate = rate;
                self.pending_events
                    .push(PlayerRuntimeEvent::PlaybackRateChanged { rate });
                Ok(PlayerRuntimeCommandResult {
                    applied: true,
                    frame: None,
                    snapshot: PlayerRuntimeAdapter::snapshot(self),
                })
            }
            PlayerRuntimeCommand::SeekTo { position } => {
                self.progress = PlaybackProgress::new(position, Some(Duration::from_secs(60)));
                self.pending_events.push(PlayerRuntimeEvent::Warning(
                    PlayerRuntimeWarning::FrameProcessor(FrameProcessorWarning {
                        kind: FrameProcessorWarningKind::DeadlineMissed,
                        plugin_name: "fixture-processor".to_owned(),
                        processor_index: 2,
                        frame_id: Some(7),
                        frame_pts_us: Some(33_000),
                        frame_duration_us: Some(33_000),
                        input_handle_kind: Some("CvPixelBuffer".to_owned()),
                        output_handle_kind: Some("CvPixelBuffer".to_owned()),
                        queue_depth: Some(1),
                        in_flight_frames: Some(1),
                        queue_wait_us: Some(100),
                        process_time_us: Some(50_000),
                        submit_to_ready_us: Some(50_000),
                        present_deadline_us: Some(49_000),
                        deadline_overrun_us: Some(34_000),
                        consecutive_miss_count: Some(3),
                        policy_action: FrameProcessorPolicyAction::BypassOriginalFrame,
                        message: Some("processor output missed frame deadline".to_owned()),
                    }),
                ));
                Ok(PlayerRuntimeCommandResult {
                    applied: true,
                    frame: None,
                    snapshot: PlayerRuntimeAdapter::snapshot(self),
                })
            }
            _ => Ok(PlayerRuntimeCommandResult {
                applied: false,
                frame: None,
                snapshot: PlayerRuntimeAdapter::snapshot(self),
            }),
        }
    }

    fn advance(&mut self) -> PlayerResult<Option<DecodedVideoFrame>> {
        Ok(None)
    }

    fn next_deadline(&self) -> Option<std::time::Instant> {
        None
    }
}

fn fake_capabilities() -> PlayerRuntimeAdapterCapabilities {
    PlayerRuntimeAdapterCapabilities {
        adapter_id: "ffi-test-adapter",
        backend_family: PlayerRuntimeAdapterBackendFamily::Unknown,
        supports_audio_output: true,
        supports_frame_output: true,
        supports_external_video_surface: false,
        supports_seek: true,
        supports_stop: true,
        supports_playback_rate: true,
        playback_rate_min: Some(0.5),
        playback_rate_max: Some(3.0),
        natural_playback_rate_max: Some(2.0),
        supports_hardware_decode: true,
        supports_streaming: true,
        supports_hdr: false,
    }
}

fn fake_frame(presentation_time_ms: u64) -> DecodedVideoFrame {
    DecodedVideoFrame {
        presentation_time: Duration::from_millis(presentation_time_ms),
        width: 2,
        height: 2,
        bytes_per_row: 8,
        pixel_format: VideoPixelFormat::Rgba8888,
        bytes: vec![255; 16],
    }
}
