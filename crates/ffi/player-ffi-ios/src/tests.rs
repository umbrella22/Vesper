use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{
    PlayerFfiAbrPolicy, PlayerFfiBufferingPolicy, PlayerFfiCachePolicy, PlayerFfiCallStatus,
    PlayerFfiDownloadAssetIndex, PlayerFfiDownloadConfig, PlayerFfiDownloadContentFormat,
    PlayerFfiDownloadExportCallbacks, PlayerFfiDownloadSource, PlayerFfiDownloadTask,
    PlayerFfiDownloadTaskStatus, PlayerFfiError, PlayerFfiErrorCategory, PlayerFfiErrorCode,
    PlayerFfiPlaylistConfig, PlayerFfiResolvedResiliencePolicy, PlayerFfiRetryPolicy,
    PlayerFfiTrackSelection, dash_bridge_error_to_ffi, map_player_error, player_error_to_ffi,
    player_ffi_dash_bridge_parse_sidx, player_ffi_download_session_create,
    player_ffi_download_session_dispose, player_ffi_download_session_export_task,
    player_ffi_ios_native_frame_pipeline_advance, player_ffi_ios_native_frame_pipeline_close,
    player_ffi_ios_native_frame_pipeline_open, player_ffi_ios_native_frame_pipeline_release_frame,
    player_ffi_ios_native_frame_pipeline_seek, player_ffi_ios_playback_event_hook_session_close,
    player_ffi_ios_playback_event_hook_session_drain_json,
    player_ffi_ios_playback_event_hook_session_flush,
    player_ffi_ios_playback_event_hook_session_submit_json, player_ffi_ios_plugin_abi_summary_json,
    player_ffi_preload_session_fail, player_ffi_resolve_resilience_policy,
    player_ffi_source_normalizer_resource_open,
};
use crate::handles::HandleRegistry;
use player_plugin::PluginTransport;
use player_plugin_abi::{
    VESPER_INTERFACE_MAJOR, VESPER_INTERFACE_MINOR, VESPER_PLUGIN_ABI_MAJOR,
    VESPER_PLUGIN_ABI_MINOR,
};
use player_plugin_loader::PluginRegistry;
use player_runtime::{PlayerError, PlayerErrorCategory, PlayerErrorCode, SubtitleErrorDetails};

#[test]
fn inbound_policy_ordinals_reject_unknown_values() {
    let cases = [
        (
            "buffering preset",
            invalid_buffering_policy as fn() -> PlayerFfiError,
        ),
        ("retry backoff", invalid_retry_policy),
        ("cache preset", invalid_cache_policy),
    ];

    for (label, run) in cases {
        let mut error = run();
        assert_eq!(error.code, PlayerFfiErrorCode::InvalidArgument, "{label}");
        assert!(ffi_error_message(&error).contains("99"), "{label}");
        unsafe { super::player_ffi_error_free(&mut error) };
    }
}

#[test]
fn inbound_track_and_playlist_ordinals_reject_unknown_values() {
    let selection = PlayerFfiTrackSelection {
        mode: 7,
        ..PlayerFfiTrackSelection::default()
    };
    let mut selection_error = super::conversions::read_track_selection(&selection)
        .expect_err("unknown selection mode should be rejected");
    assert_eq!(selection_error.code, PlayerFfiErrorCode::InvalidArgument);
    assert!(ffi_error_message(&selection_error).contains("selection.mode"));
    unsafe { super::player_ffi_error_free(&mut selection_error) };

    let abr = PlayerFfiAbrPolicy {
        mode: 7,
        ..PlayerFfiAbrPolicy::default()
    };
    let mut abr_error =
        super::conversions::read_abr_policy(&abr).expect_err("unknown ABR mode should be rejected");
    assert_eq!(abr_error.code, PlayerFfiErrorCode::InvalidArgument);
    assert!(ffi_error_message(&abr_error).contains("policy.mode"));
    unsafe { super::player_ffi_error_free(&mut abr_error) };

    let config = PlayerFfiPlaylistConfig {
        repeat_mode: 7,
        failure_strategy: 1,
        ..PlayerFfiPlaylistConfig::default()
    };
    let mut playlist_error = super::conversions::read_playlist_config(&config)
        .expect_err("unknown playlist repeat mode should be rejected");
    assert_eq!(playlist_error.code, PlayerFfiErrorCode::InvalidArgument);
    assert!(ffi_error_message(&playlist_error).contains("config.repeat_mode"));
    unsafe { super::player_ffi_error_free(&mut playlist_error) };
}

#[test]
fn download_config_decodes_capability_specific_plugin_references() {
    let post_download = CString::new(
        r#"[{"pluginId":"io.github.ikaros.vesper.remux-ffmpeg","capabilityInstanceId":"io.github.ikaros.vesper.remux-ffmpeg.default","transport":"native"}]"#,
    )
    .expect("post-download references CString");
    let event_hooks =
        CString::new(r#"[{"pluginId":"dev.vesper.event-hook","transport":"native"}]"#)
            .expect("event-hook references CString");
    let config = PlayerFfiDownloadConfig {
        post_download_plugin_references_json: post_download.as_ptr(),
        event_hook_plugin_references_json: event_hooks.as_ptr(),
        ..PlayerFfiDownloadConfig::default()
    };

    let resolved = super::conversions::read_download_config(&config)
        .expect("download plugin references should decode");

    assert_eq!(resolved.post_download_plugin_references.len(), 1);
    assert_eq!(
        resolved.post_download_plugin_references[0].plugin_id(),
        "io.github.ikaros.vesper.remux-ffmpeg"
    );
    assert_eq!(
        resolved.post_download_plugin_references[0].capability_instance_id(),
        Some("io.github.ikaros.vesper.remux-ffmpeg.default")
    );
    assert_eq!(
        resolved.post_download_plugin_references[0].transport(),
        PluginTransport::Native
    );
    assert_eq!(resolved.event_hook_plugin_references.len(), 1);
    assert_eq!(
        resolved.event_hook_plugin_references[0].plugin_id(),
        "dev.vesper.event-hook"
    );
}

#[test]
fn download_config_rejects_missing_or_invalid_plugin_reference_json() {
    let empty_references = CString::new("[]").expect("empty references CString");
    let missing = PlayerFfiDownloadConfig {
        event_hook_plugin_references_json: empty_references.as_ptr(),
        ..PlayerFfiDownloadConfig::default()
    };
    let mut missing_error = super::conversions::read_download_config(&missing)
        .expect_err("missing post-download reference JSON should fail");
    assert_eq!(missing_error.code, PlayerFfiErrorCode::NullPointer);
    assert!(ffi_error_message(&missing_error).contains("post_download_plugin_references_json"));
    unsafe { super::player_ffi_error_free(&mut missing_error) };

    let invalid_references =
        CString::new(r#"[{"pluginId":"not-reverse-dns","transport":"native"}]"#)
            .expect("invalid references CString");
    let invalid = PlayerFfiDownloadConfig {
        post_download_plugin_references_json: invalid_references.as_ptr(),
        event_hook_plugin_references_json: empty_references.as_ptr(),
        ..PlayerFfiDownloadConfig::default()
    };
    let mut invalid_error = super::conversions::read_download_config(&invalid)
        .expect_err("invalid plugin identity should fail");
    assert_eq!(invalid_error.code, PlayerFfiErrorCode::InvalidArgument);
    assert!(ffi_error_message(&invalid_error).contains("reverse-DNS"));
    unsafe { super::player_ffi_error_free(&mut invalid_error) };
}

#[test]
fn download_session_create_preserves_plugin_selection_error_and_zeroes_handle() {
    let post_download = CString::new(
        r#"[{"pluginId":"dev.vesper.missing-remux","capabilityInstanceId":"dev.vesper.missing-remux.post-download","transport":"native"}]"#,
    )
    .expect("post-download references CString");
    let empty_references = CString::new("[]").expect("empty references CString");
    let plugin_registry_handle =
        super::plugin_registry::register_test_plugin_registry(PluginRegistry::default())
            .expect("empty plugin registry should register");
    let config = PlayerFfiDownloadConfig {
        plugin_registry_handle,
        post_download_plugin_references_json: post_download.as_ptr(),
        event_hook_plugin_references_json: empty_references.as_ptr(),
        ..PlayerFfiDownloadConfig::default()
    };
    let mut out_handle = u64::MAX;
    let mut error = PlayerFfiError::default();

    let status =
        unsafe { player_ffi_download_session_create(&config, &mut out_handle, &mut error) };
    unsafe { super::player_ffi_ios_plugin_registry_dispose(plugin_registry_handle) };

    assert_eq!(status, PlayerFfiCallStatus::Error);
    assert_eq!(out_handle, 0);
    assert_eq!(error.code, PlayerFfiErrorCode::InvalidArgument);
    assert_eq!(error.category, PlayerFfiErrorCategory::Input);
    let message = ffi_error_message(&error);
    assert!(
        message.contains("failed to resolve ios post-download plugin"),
        "{message}"
    );
    assert!(
        message.contains("plugin `dev.vesper.missing-remux` is not loaded for transport Native"),
        "{message}"
    );
    unsafe { super::player_ffi_error_free(&mut error) };
}

#[test]
fn benchmark_session_create_zeroes_handle_before_input_validation() {
    let mut out_handle = u64::MAX;
    let mut error = PlayerFfiError::default();

    let status = unsafe {
        super::player_ffi_benchmark_session_create_with_references_json(
            0,
            ptr::null(),
            &mut out_handle,
            &mut error,
        )
    };

    assert_eq!(status, PlayerFfiCallStatus::Error);
    assert_eq!(out_handle, 0);
    assert_eq!(error.code, PlayerFfiErrorCode::InvalidArgument);
    assert!(ffi_error_message(&error).contains("references_json was null"));
    unsafe { super::player_ffi_error_free(&mut error) };
}

#[test]
fn restored_download_task_rejects_unknown_status_and_error_ordinals() {
    let asset_id = test_c_string("asset-1");
    let source_uri = test_c_string("https://example.test/video.m3u8");
    let make_task = || PlayerFfiDownloadTask {
        asset_id: asset_id.as_ptr() as *mut c_char,
        source: PlayerFfiDownloadSource {
            source_uri: source_uri.as_ptr() as *mut c_char,
            content_format: PlayerFfiDownloadContentFormat::HlsSegments as u32,
            ..PlayerFfiDownloadSource::default()
        },
        asset_index: PlayerFfiDownloadAssetIndex {
            content_format: PlayerFfiDownloadContentFormat::HlsSegments as u32,
            ..PlayerFfiDownloadAssetIndex::default()
        },
        ..PlayerFfiDownloadTask::default()
    };

    let mut invalid_status = make_task();
    invalid_status.status = 99;
    let mut status_error =
        super::conversions::read_download_task(&invalid_status, std::time::Instant::now())
            .expect_err("unknown restored task status should be rejected");
    assert_eq!(status_error.code, PlayerFfiErrorCode::InvalidArgument);
    assert!(ffi_error_message(&status_error).contains("task.status"));
    unsafe { super::player_ffi_error_free(&mut status_error) };

    let mut invalid_error = make_task();
    invalid_error.status = PlayerFfiDownloadTaskStatus::Failed as u32;
    invalid_error.has_error = true;
    invalid_error.error_code = 99;
    invalid_error.error_category = PlayerFfiErrorCategory::Network as u32;
    let mut code_error =
        super::conversions::read_download_task(&invalid_error, std::time::Instant::now())
            .expect_err("unknown restored task error code should be rejected");
    assert_eq!(code_error.code, PlayerFfiErrorCode::InvalidArgument);
    assert!(ffi_error_message(&code_error).contains("error code"));
    unsafe { super::player_ffi_error_free(&mut code_error) };
}

fn invalid_buffering_policy() -> PlayerFfiError {
    let policy = PlayerFfiBufferingPolicy {
        preset: 99,
        ..PlayerFfiBufferingPolicy::default()
    };
    super::conversions::read_buffering_policy(&policy)
        .expect_err("unknown buffering preset should be rejected")
}

fn invalid_retry_policy() -> PlayerFfiError {
    let policy = PlayerFfiRetryPolicy {
        has_backoff: true,
        backoff: 99,
        ..PlayerFfiRetryPolicy::default()
    };
    super::conversions::read_retry_policy(&policy)
        .expect_err("unknown retry backoff should be rejected")
}

fn invalid_cache_policy() -> PlayerFfiError {
    let policy = PlayerFfiCachePolicy {
        preset: 99,
        ..PlayerFfiCachePolicy::default()
    };
    super::conversions::read_cache_policy(&policy)
        .expect_err("unknown cache preset should be rejected")
}

#[test]
fn ffi_handle_registry_reuses_slot_with_new_generation_and_rejects_stale_handle() {
    let mut registry = HandleRegistry::default();
    let first = registry.insert(7_u32);

    assert_eq!(registry.get(first), Some(&7));
    assert_eq!(registry.remove(first), Some(7));

    let second = registry.insert(9_u32);
    assert_ne!(first, second);
    assert!(registry.get(first).is_none());
    assert_eq!(registry.get(second), Some(&9));
}

#[test]
fn registry_lock_recovers_after_poisoning() {
    let registry = std::sync::Mutex::new(Vec::<u32>::new());
    let _ = std::panic::catch_unwind(|| {
        let mut values = registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        values.push(1);
        panic!("poison registry for regression coverage");
    });

    let mut values =
        crate::handles::lock_registry(&registry).unwrap_or_else(|poisoned| poisoned.into_inner());
    values.push(2);
    assert_eq!(&*values, &[1, 2]);
}

struct DownloadExportLockProbe {
    handle: u64,
    session_lock_was_available: AtomicBool,
}

unsafe extern "C" fn probe_download_export_session_lock(context: *mut c_void, _ratio: f32) {
    // SAFETY: the test keeps the probe alive for the full synchronous export call.
    let probe = unsafe { &*(context.cast::<DownloadExportLockProbe>()) };
    let session = {
        let sessions = crate::handles::lock_registry(crate::handles::download_sessions())
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        sessions.get(probe.handle).cloned()
    };
    let lock_was_available = session
        .as_ref()
        .is_some_and(|session| session.try_lock().is_ok());
    probe
        .session_lock_was_available
        .fetch_or(lock_was_available, Ordering::SeqCst);
}

#[test]
fn download_export_runs_host_progress_callback_without_holding_session_lock() {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    use player_model::MediaSource;
    use player_platform_ios::IosDownloadBridgeSession;
    use player_runtime::{
        DownloadAssetIndex, DownloadContentFormat, DownloadProfile, DownloadSource,
    };

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let temp_dir = std::env::temp_dir().join(format!("vesper-ios-export-lock-{unique}"));
    std::fs::create_dir_all(&temp_dir).expect("create export test directory");
    let source_path = temp_dir.join("input.mp4");
    let output_path = temp_dir.join("output.mp4");
    std::fs::write(&source_path, b"vesper export lock regression").expect("write export source");

    let mut session = IosDownloadBridgeSession::new(false);
    let task_id = session
        .create_task(
            "asset-export-lock",
            DownloadSource::new(
                MediaSource::new(format!("file://{}", source_path.display())),
                DownloadContentFormat::SingleFile,
            ),
            DownloadProfile::default(),
            DownloadAssetIndex::default(),
            Instant::now(),
        )
        .expect("create download task");
    session
        .complete_task(task_id, Some(source_path), Instant::now())
        .expect("complete download task");

    let handle = {
        let mut sessions = crate::handles::lock_registry(crate::handles::download_sessions())
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        sessions.insert(Arc::new(Mutex::new(session)))
    };
    let probe = DownloadExportLockProbe {
        handle,
        session_lock_was_available: AtomicBool::new(false),
    };
    let output_path = CString::new(PathBuf::from(&output_path).to_string_lossy().as_bytes())
        .expect("output path CString");
    let callbacks = PlayerFfiDownloadExportCallbacks {
        context: (&probe as *const DownloadExportLockProbe).cast_mut().cast(),
        on_progress: Some(probe_download_export_session_lock),
        is_cancelled: None,
    };
    let mut error = PlayerFfiError::default();

    let status = unsafe {
        player_ffi_download_session_export_task(
            handle,
            task_id.get(),
            output_path.as_ptr(),
            callbacks,
            &mut error,
        )
    };

    unsafe { player_ffi_download_session_dispose(handle) };
    if status != PlayerFfiCallStatus::Ok {
        unsafe { super::player_ffi_error_free(&mut error) };
    }
    std::fs::remove_dir_all(temp_dir).expect("remove export test directory");

    assert_eq!(status, PlayerFfiCallStatus::Ok);
    assert!(
        probe.session_lock_was_available.load(Ordering::SeqCst),
        "the export callback must be able to re-enter the same download session"
    );
}

#[test]
fn task_carrying_download_commands_repeat_the_nested_task_id_at_the_ffi_boundary() {
    use std::time::Instant;

    use player_model::MediaSource;
    use player_platform_ios::{IosDownloadBridgeSession, IosDownloadCommand};
    use player_runtime::{
        DownloadAssetIndex, DownloadContentFormat, DownloadProfile, DownloadSource,
    };

    let mut session = IosDownloadBridgeSession::new(false);
    let task_id = session
        .create_task(
            "asset-command-id",
            DownloadSource::new(
                MediaSource::new("https://example.com/video.mp4"),
                DownloadContentFormat::SingleFile,
            ),
            DownloadProfile::default(),
            DownloadAssetIndex::default(),
            Instant::now(),
        )
        .expect("create download task");
    let task = session.task(task_id).expect("download task snapshot");

    for command in [
        IosDownloadCommand::Prepare { task: task.clone() },
        IosDownloadCommand::Start { task: task.clone() },
        IosDownloadCommand::Resume { task: task.clone() },
        IosDownloadCommand::Remove { task },
    ] {
        let ffi = super::PlayerFfiDownloadCommand::from(command);
        assert_eq!(ffi.task_id, task_id.get());
        assert_eq!(ffi.task.task_id, task_id.get());
        let mut ffi = ffi;
        super::conversions::download_command_free(&mut ffi);
    }
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
fn resolve_resilience_policy_rejects_invalid_raw_source_kind() {
    let buffering = PlayerFfiBufferingPolicy::default();
    let retry = PlayerFfiRetryPolicy::default();
    let cache = PlayerFfiCachePolicy::default();
    let mut policy = PlayerFfiResolvedResiliencePolicy::default();
    let mut error = PlayerFfiError::default();

    let status = unsafe {
        player_ffi_resolve_resilience_policy(
            99,
            0,
            &buffering,
            &retry,
            &cache,
            &mut policy,
            &mut error,
        )
    };

    assert_eq!(status, PlayerFfiCallStatus::Error);
    assert_eq!(error.code, PlayerFfiErrorCode::InvalidArgument);
    assert!(ffi_error_message(&error).contains("source_kind"));
    unsafe { super::player_ffi_error_free(&mut error) };
}

#[test]
fn preload_fail_rejects_invalid_raw_error_code_before_handle_lookup() {
    let message = CString::new("boom").expect("message");
    let mut error = PlayerFfiError::default();

    let status = unsafe {
        player_ffi_preload_session_fail(
            0xDEAD_BEEF,
            1,
            999,
            PlayerFfiErrorCategory::Playback as u32,
            true,
            message.as_ptr(),
            &mut error,
        )
    };

    assert_eq!(status, PlayerFfiCallStatus::Error);
    assert_eq!(error.code, PlayerFfiErrorCode::InvalidArgument);
    assert!(ffi_error_message(&error).contains("error code"));
    unsafe { super::player_ffi_error_free(&mut error) };
}

#[test]
fn output_c_strings_replace_embedded_nul_with_space() {
    let mut value = super::conversions::into_c_string_ptr("hello\0world".to_owned());

    let text = unsafe { CStr::from_ptr(value) }
        .to_str()
        .expect("sanitized output should be UTF-8");
    assert_eq!(text, "hello world");

    super::conversions::free_c_string(&mut value);
    assert!(value.is_null());
}

#[test]
fn read_string_list_rejects_null_elements() {
    let item = CString::new("one").expect("test string");
    let mut values = [item.as_ptr() as *mut c_char, ptr::null_mut()];

    let mut error =
        super::conversions::read_string_list(values.as_mut_ptr(), values.len(), "items")
            .expect_err("null list elements should be rejected");

    assert_eq!(error.code, PlayerFfiErrorCode::InvalidArgument);
    assert!(ffi_error_message(&error).contains("items[1]"));
    unsafe { super::player_ffi_error_free(&mut error) };
}

#[test]
fn player_error_mapping_preserves_legacy_and_appended_values() {
    let cases = [
        (
            PlayerErrorCode::InvalidArgument,
            PlayerErrorCategory::Input,
            PlayerFfiErrorCode::InvalidArgument,
            PlayerFfiErrorCategory::Input,
        ),
        (
            PlayerErrorCode::InvalidState,
            PlayerErrorCategory::Playback,
            PlayerFfiErrorCode::InvalidState,
            PlayerFfiErrorCategory::Playback,
        ),
        (
            PlayerErrorCode::InvalidSource,
            PlayerErrorCategory::Source,
            PlayerFfiErrorCode::InvalidSource,
            PlayerFfiErrorCategory::Source,
        ),
        (
            PlayerErrorCode::BackendFailure,
            PlayerErrorCategory::Platform,
            PlayerFfiErrorCode::BackendFailure,
            PlayerFfiErrorCategory::Platform,
        ),
        (
            PlayerErrorCode::AudioOutputUnavailable,
            PlayerErrorCategory::AudioOutput,
            PlayerFfiErrorCode::AudioOutputUnavailable,
            PlayerFfiErrorCategory::AudioOutput,
        ),
        (
            PlayerErrorCode::DecodeFailure,
            PlayerErrorCategory::Decode,
            PlayerFfiErrorCode::DecodeFailure,
            PlayerFfiErrorCategory::Decode,
        ),
        (
            PlayerErrorCode::SeekFailure,
            PlayerErrorCategory::Playback,
            PlayerFfiErrorCode::SeekFailure,
            PlayerFfiErrorCategory::Playback,
        ),
        (
            PlayerErrorCode::Unsupported,
            PlayerErrorCategory::Capability,
            PlayerFfiErrorCode::Unsupported,
            PlayerFfiErrorCategory::Capability,
        ),
        (
            PlayerErrorCode::CommandChannelClosed,
            PlayerErrorCategory::Playback,
            PlayerFfiErrorCode::CommandChannelClosed,
            PlayerFfiErrorCategory::Playback,
        ),
        (
            PlayerErrorCode::EventChannelClosed,
            PlayerErrorCategory::Playback,
            PlayerFfiErrorCode::EventChannelClosed,
            PlayerFfiErrorCategory::Playback,
        ),
        (
            PlayerErrorCode::Cancelled,
            PlayerErrorCategory::Playback,
            PlayerFfiErrorCode::Cancelled,
            PlayerFfiErrorCategory::Playback,
        ),
        (
            PlayerErrorCode::Timeout,
            PlayerErrorCategory::Playback,
            PlayerFfiErrorCode::Timeout,
            PlayerFfiErrorCategory::Playback,
        ),
    ];

    for (player_code, player_category, ffi_code, ffi_category) in cases {
        let player_error = PlayerError::with_category(player_code, player_category, "error");
        assert_eq!(map_player_error(&player_error), (ffi_code, ffi_category));

        let mut ffi_error = player_error_to_ffi(player_error);
        assert_eq!(ffi_error.code, ffi_code);
        assert_eq!(ffi_error.category, ffi_category);
        unsafe { super::player_ffi_error_free(&mut ffi_error) };
    }
}

#[test]
fn ffi_error_code_direct_mapping_preserves_legacy_and_appended_values() {
    let cases = [
        (
            PlayerFfiErrorCode::InvalidArgument,
            PlayerErrorCode::InvalidArgument,
        ),
        (
            PlayerFfiErrorCode::InvalidState,
            PlayerErrorCode::InvalidState,
        ),
        (
            PlayerFfiErrorCode::InvalidSource,
            PlayerErrorCode::InvalidSource,
        ),
        (
            PlayerFfiErrorCode::BackendFailure,
            PlayerErrorCode::BackendFailure,
        ),
        (
            PlayerFfiErrorCode::AudioOutputUnavailable,
            PlayerErrorCode::AudioOutputUnavailable,
        ),
        (
            PlayerFfiErrorCode::DecodeFailure,
            PlayerErrorCode::DecodeFailure,
        ),
        (
            PlayerFfiErrorCode::SeekFailure,
            PlayerErrorCode::SeekFailure,
        ),
        (
            PlayerFfiErrorCode::Unsupported,
            PlayerErrorCode::Unsupported,
        ),
        (
            PlayerFfiErrorCode::CommandChannelClosed,
            PlayerErrorCode::CommandChannelClosed,
        ),
        (
            PlayerFfiErrorCode::EventChannelClosed,
            PlayerErrorCode::EventChannelClosed,
        ),
        (PlayerFfiErrorCode::Cancelled, PlayerErrorCode::Cancelled),
        (PlayerFfiErrorCode::Timeout, PlayerErrorCode::Timeout),
    ];

    for (ffi_code, code) in cases {
        assert_eq!(PlayerErrorCode::from(ffi_code), code);
    }
}

#[test]
fn player_error_to_ffi_preserves_structured_subtitle_details() {
    let error = PlayerError::new(PlayerErrorCode::Timeout, "outer timeout").with_subtitle_details(
        SubtitleErrorDetails::new(
            "future_subtitle_code",
            "future_phase",
            Some("opaque-track".to_owned()),
            true,
            "typed timeout",
        )
        .with_transaction(Some(42), Some(9)),
    );
    let mut ffi_error = player_error_to_ffi(error);
    assert!(!ffi_error.details_json.is_null());
    // SAFETY: player_error_to_ffi returns a valid owned NUL-terminated string.
    let json = unsafe { CStr::from_ptr(ffi_error.details_json) }
        .to_string_lossy()
        .into_owned();
    let details: SubtitleErrorDetails = serde_json::from_str(&json).expect("subtitle details");
    let payload: serde_json::Value = serde_json::from_str(&json).expect("details envelope");
    assert_eq!(payload["domain"], "subtitle");
    assert_eq!(details.code, "future_subtitle_code");
    assert_eq!(details.phase, "future_phase");
    assert_eq!(details.command_id, Some(42));
    assert_eq!(details.source_epoch, Some(9));
    unsafe { super::player_ffi_error_free(&mut ffi_error) };
}

#[test]
fn dash_bridge_error_to_ffi_emits_structured_non_subtitle_envelope() {
    let error =
        player_dash_hls_bridge::DashHlsError::UnsupportedMpd("multi-period manifest".to_owned());
    let mut ffi_error = dash_bridge_error_to_ffi(&error);
    assert!(!ffi_error.details_json.is_null());
    // SAFETY: dash_bridge_error_to_ffi returns a valid owned C string.
    let json = unsafe { CStr::from_ptr(ffi_error.details_json) }
        .to_string_lossy()
        .into_owned();
    let payload: serde_json::Value = serde_json::from_str(&json).expect("DASH details");
    assert_eq!(payload["domain"], "dash");
    assert_eq!(payload["code"], "dash_manifest_unsupported");
    unsafe { super::player_ffi_error_free(&mut ffi_error) };
}

#[test]
fn dash_bridge_parse_sidx_ffi_preserves_structured_error_details() {
    let truncated_sidx = [0, 0, 0, 16, b's', b'i', b'd', b'x'];
    let mut response_json = ptr::null_mut();
    let mut ffi_error = PlayerFfiError::default();

    let status = unsafe {
        player_ffi_dash_bridge_parse_sidx(
            truncated_sidx.as_ptr(),
            truncated_sidx.len(),
            &mut response_json,
            &mut ffi_error,
        )
    };

    assert_eq!(status, PlayerFfiCallStatus::Error);
    assert!(response_json.is_null());
    assert!(!ffi_error.details_json.is_null());
    // SAFETY: the FFI entry point returned an owned NUL-terminated details string.
    let details = unsafe { CStr::from_ptr(ffi_error.details_json) }
        .to_string_lossy()
        .into_owned();
    let payload: serde_json::Value = serde_json::from_str(&details).expect("DASH details");
    assert_eq!(payload["domain"], "dash");
    assert_eq!(payload["code"], "dash_mp4_invalid");
    unsafe { super::player_ffi_error_free(&mut ffi_error) };
}

#[test]
fn plugin_abi_summary_reports_root_and_typed_interface_versions() {
    let mut out_json: *mut c_char = ptr::null_mut();
    let mut error = PlayerFfiError::default();

    let status = unsafe { player_ffi_ios_plugin_abi_summary_json(&mut out_json, &mut error) };

    assert_eq!(status, PlayerFfiCallStatus::Ok);
    assert!(error.message.is_null());
    assert!(!out_json.is_null());
    let json = unsafe {
        std::ffi::CStr::from_ptr(out_json)
            .to_string_lossy()
            .into_owned()
    };
    unsafe { super::player_ffi_mobile_plugin_diagnostics_string_free(out_json) };
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse ABI summary JSON");
    assert_eq!(
        value["rootAbi"]["major"].as_u64(),
        Some(VESPER_PLUGIN_ABI_MAJOR as u64)
    );
    assert_eq!(
        value["rootAbi"]["minor"].as_u64(),
        Some(VESPER_PLUGIN_ABI_MINOR as u64)
    );
    for interface in [
        "postDownloadProcessor",
        "pipelineEventHook",
        "benchmarkSink",
        "nativeDecoder",
        "frameProcessor",
        "sourceNormalizerPacket",
        "sourceNormalizerResource",
    ] {
        assert_eq!(
            value["typedInterfaces"][interface]["major"].as_u64(),
            Some(VESPER_INTERFACE_MAJOR as u64)
        );
        assert_eq!(
            value["typedInterfaces"][interface]["minor"].as_u64(),
            Some(VESPER_INTERFACE_MINOR as u64)
        );
    }
    assert!(value.get("decoderAbiVersion").is_none());
    assert!(value.get("frameProcessorAbiVersion").is_none());
    assert!(value.get("sourceNormalizerAbiVersion").is_none());
    assert_eq!(
        value["abiSemantics"].as_str(),
        Some("stable-root-typed-interfaces")
    );
    assert_eq!(
        value["capabilityMatching"].as_str(),
        Some("explicit-plugin-reference")
    );
}

#[test]
fn source_normalizer_resource_open_can_return_bypass_diagnostics_without_handle() {
    let source = test_c_string("file:///tmp/video.flv");
    let artifacts = test_c_string("[]");
    let output_root = test_c_string("/tmp/vesper-source-normalizer");
    let mut out_handle = 99_u64;
    let mut out_json: *mut c_char = ptr::null_mut();
    let mut error = PlayerFfiError::default();

    let status = unsafe {
        player_ffi_source_normalizer_resource_open(
            source.as_ptr(),
            3,
            artifacts.as_ptr(),
            ptr::null(),
            output_root.as_ptr(),
            false,
            &mut out_handle,
            &mut out_json,
            &mut error,
        )
    };

    assert_eq!(status, PlayerFfiCallStatus::Ok);
    assert_eq!(out_handle, 0);
    assert!(error.message.is_null());
    assert!(
        !out_json.is_null(),
        "preferNormalized bypass diagnostics should be returned even without a resource handle"
    );
    let json = unsafe {
        std::ffi::CStr::from_ptr(out_json)
            .to_string_lossy()
            .into_owned()
    };
    unsafe { super::player_ffi_mobile_plugin_diagnostics_string_free(out_json) };
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse diagnostics JSON");
    let diagnostics = value.as_array().expect("diagnostics should be an array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["status"].as_str() == Some("sourceNormalizerUnsupported")
            && diagnostic["participation"].as_str() == Some("bypassed")
            && diagnostic["message"]
                .as_str()
                .is_some_and(|message| message.contains("no plugin paths"))
    }));
}

#[test]
fn ios_native_frame_pipeline_open_requires_source_normalizer_packet_plugin_reference() {
    let source = test_c_string("file:///tmp/video.mp4");
    let source_artifacts = test_c_string("[]");
    let decoder_artifacts = test_c_string(
        r#"[{"reference":{"pluginId":"io.github.ikaros.vesper.decoder-videotoolbox","capabilityInstanceId":"io.github.ikaros.vesper.decoder-videotoolbox.video","transport":"native"},"libraryPath":"/tmp/libdecoder.dylib"}]"#,
    );
    let frame_artifacts = test_c_string("[]");
    let mut out_handle = 99_u64;
    let mut out_json: *mut c_char = ptr::null_mut();
    let mut error = PlayerFfiError::default();

    let status = unsafe {
        player_ffi_ios_native_frame_pipeline_open(
            source.as_ptr(),
            2,
            source_artifacts.as_ptr(),
            ptr::null(),
            3,
            decoder_artifacts.as_ptr(),
            frame_artifacts.as_ptr(),
            0,
            &mut out_handle,
            &mut out_json,
            &mut error,
        )
    };

    assert_eq!(status, PlayerFfiCallStatus::Error);
    assert_eq!(out_handle, 0);
    assert!(out_json.is_null());
    assert_eq!(error.code, PlayerFfiErrorCode::BackendFailure);
    let message = ffi_error_message(&error);
    assert!(message.contains("nativeFrameIssueKind=missingSourceNormalizerPacketPlugin"));
    assert!(message.contains("SourceNormalizer packet-stream plugin path"));
    unsafe { super::player_ffi_error_free(&mut error) };
}

#[test]
fn ios_native_frame_pipeline_open_requires_videotoolbox_decoder_plugin_reference() {
    let source = test_c_string("file:///tmp/video.mp4");
    let source_artifacts = test_c_string(
        r#"[{"reference":{"pluginId":"io.github.ikaros.vesper.source-normalizer-ffmpeg","capabilityInstanceId":"io.github.ikaros.vesper.source-normalizer-ffmpeg.packet","transport":"native"},"libraryPath":"/tmp/libsource_normalizer.dylib"}]"#,
    );
    let decoder_artifacts = test_c_string("[]");
    let frame_artifacts = test_c_string("[]");
    let mut out_handle = 99_u64;
    let mut out_json: *mut c_char = ptr::null_mut();
    let mut error = PlayerFfiError::default();

    let status = unsafe {
        player_ffi_ios_native_frame_pipeline_open(
            source.as_ptr(),
            2,
            source_artifacts.as_ptr(),
            ptr::null(),
            3,
            decoder_artifacts.as_ptr(),
            frame_artifacts.as_ptr(),
            0,
            &mut out_handle,
            &mut out_json,
            &mut error,
        )
    };

    assert_eq!(status, PlayerFfiCallStatus::Error);
    assert_eq!(out_handle, 0);
    assert!(out_json.is_null());
    assert_eq!(error.code, PlayerFfiErrorCode::BackendFailure);
    let message = ffi_error_message(&error);
    assert!(message.contains("nativeFrameIssueKind=missingVideoToolboxDecoderPlugin"));
    assert!(message.contains("VideoToolbox decoder plugin path"));
    unsafe { super::player_ffi_error_free(&mut error) };
}

#[test]
fn ios_native_frame_pipeline_invalid_handles_fail_and_close_is_idempotent() {
    let mut out_json: *mut c_char = ptr::null_mut();
    let mut error = PlayerFfiError::default();

    let advance_status = unsafe {
        player_ffi_ios_native_frame_pipeline_advance(0xDEAD_BEEF, &mut out_json, &mut error)
    };
    assert_eq!(advance_status, PlayerFfiCallStatus::Error);
    assert_eq!(error.code, PlayerFfiErrorCode::InvalidArgument);
    assert!(ffi_error_message(&error).contains("invalid native-frame pipeline handle"));
    unsafe { super::player_ffi_error_free(&mut error) };

    let release_status = unsafe {
        player_ffi_ios_native_frame_pipeline_release_frame(
            0xDEAD_BEEF,
            1,
            false,
            &mut out_json,
            &mut error,
        )
    };
    assert_eq!(release_status, PlayerFfiCallStatus::Error);
    assert_eq!(error.code, PlayerFfiErrorCode::InvalidArgument);
    assert!(ffi_error_message(&error).contains("invalid native-frame pipeline handle"));
    unsafe { super::player_ffi_error_free(&mut error) };

    let seek_status = unsafe {
        player_ffi_ios_native_frame_pipeline_seek(0xDEAD_BEEF, 1_000, &mut out_json, &mut error)
    };
    assert_eq!(seek_status, PlayerFfiCallStatus::Error);
    assert_eq!(error.code, PlayerFfiErrorCode::InvalidArgument);
    assert!(ffi_error_message(&error).contains("invalid native-frame pipeline handle"));
    unsafe { super::player_ffi_error_free(&mut error) };

    unsafe {
        player_ffi_ios_native_frame_pipeline_close(0xDEAD_BEEF);
        player_ffi_ios_native_frame_pipeline_close(0xDEAD_BEEF);
    }
}

#[test]
fn ios_native_frame_pipeline_seek_requires_json_output_pointer() {
    let mut error = PlayerFfiError::default();

    let status = unsafe {
        player_ffi_ios_native_frame_pipeline_seek(0xDEAD_BEEF, 1_000, ptr::null_mut(), &mut error)
    };

    assert_eq!(status, PlayerFfiCallStatus::Error);
    assert_eq!(error.code, PlayerFfiErrorCode::NullPointer);
    assert!(ffi_error_message(&error).contains("out_json was null"));
    unsafe { super::player_ffi_error_free(&mut error) };
}

#[test]
fn ios_playback_event_hook_invalid_handles_are_rejected() {
    let mut error = PlayerFfiError::default();
    let event = test_c_string(
        r#"{"runId":"run-1","sessionId":"session-1","platform":"ios","protocol":null,"eventName":"play","timestampNs":1,"thread":"main","resourceIdentity":"playback-session:1","attributes":{},"diagnostic":null}"#,
    );
    let status = unsafe {
        player_ffi_ios_playback_event_hook_session_submit_json(
            0xDEAD_BEEF,
            event.as_ptr(),
            &mut error,
        )
    };
    assert_eq!(status, PlayerFfiCallStatus::Error);
    assert_eq!(error.code, PlayerFfiErrorCode::InvalidArgument);
    assert!(ffi_error_message(&error).contains("invalid playback event-hook session handle"));
    unsafe { super::player_ffi_error_free(&mut error) };

    let flush_status =
        unsafe { player_ffi_ios_playback_event_hook_session_flush(0xDEAD_BEEF, 100, &mut error) };
    assert_eq!(flush_status, PlayerFfiCallStatus::Error);
    assert_eq!(error.code, PlayerFfiErrorCode::InvalidArgument);
    unsafe { super::player_ffi_error_free(&mut error) };

    let mut report_json = ptr::null_mut();
    let drain_status = unsafe {
        player_ffi_ios_playback_event_hook_session_drain_json(
            0xDEAD_BEEF,
            &mut report_json,
            &mut error,
        )
    };
    assert_eq!(drain_status, PlayerFfiCallStatus::Error);
    assert_eq!(error.code, PlayerFfiErrorCode::InvalidArgument);
    assert!(report_json.is_null());
    unsafe { super::player_ffi_error_free(&mut error) };

    unsafe {
        player_ffi_ios_playback_event_hook_session_close(0xDEAD_BEEF, &mut error);
    }
    assert_eq!(error.code, PlayerFfiErrorCode::InvalidArgument);
    unsafe { super::player_ffi_error_free(&mut error) };
}

#[test]
fn ios_playback_event_hook_empty_session_has_idempotent_lifecycle() {
    let references = test_c_string("[]");
    let plugin_registry_handle =
        super::plugin_registry::register_test_plugin_registry(PluginRegistry::default())
            .expect("empty plugin registry should register");
    let mut handle = 0;
    let mut error = PlayerFfiError::default();
    let created = unsafe {
        super::player_ffi_ios_playback_event_hook_session_create(
            plugin_registry_handle,
            references.as_ptr(),
            &mut handle,
            &mut error,
        )
    };
    unsafe { super::player_ffi_ios_plugin_registry_dispose(plugin_registry_handle) };
    assert_eq!(created, PlayerFfiCallStatus::Ok);
    assert_ne!(handle, 0);

    let event = test_c_string(
        r#"{"runId":"run-1","sessionId":"session-1","platform":"ios","protocol":null,"eventName":"play","timestampNs":1,"thread":"main","resourceIdentity":"playback-session:1","attributes":{},"diagnostic":null}"#,
    );
    let submitted = unsafe {
        player_ffi_ios_playback_event_hook_session_submit_json(handle, event.as_ptr(), &mut error)
    };
    assert_eq!(submitted, PlayerFfiCallStatus::Ok);
    assert_eq!(
        unsafe { player_ffi_ios_playback_event_hook_session_flush(handle, 100, &mut error) },
        PlayerFfiCallStatus::Ok
    );

    let mut report_json = ptr::null_mut();
    assert_eq!(
        unsafe {
            player_ffi_ios_playback_event_hook_session_drain_json(
                handle,
                &mut report_json,
                &mut error,
            )
        },
        PlayerFfiCallStatus::Ok
    );
    assert!(!report_json.is_null());
    let report_text = unsafe { CStr::from_ptr(report_json) }
        .to_string_lossy()
        .into_owned();
    assert!(report_text.contains("\"reports\":[]"));
    unsafe { super::player_ffi_ios_playback_event_hook_report_string_free(report_json) };

    assert_eq!(
        unsafe { player_ffi_ios_playback_event_hook_session_close(handle, &mut error) },
        PlayerFfiCallStatus::Ok
    );
    assert_eq!(
        unsafe { player_ffi_ios_playback_event_hook_session_close(handle, &mut error) },
        PlayerFfiCallStatus::Ok
    );
    unsafe { super::player_ffi_ios_playback_event_hook_session_dispose(handle) };
}

fn ffi_error_message(error: &PlayerFfiError) -> String {
    if error.message.is_null() {
        return String::new();
    }
    unsafe {
        std::ffi::CStr::from_ptr(error.message)
            .to_string_lossy()
            .into_owned()
    }
}

fn test_c_string(value: &str) -> CString {
    match CString::new(value) {
        Ok(value) => value,
        Err(error) => panic!("test string contained an unexpected NUL byte: {error}"),
    }
}
