use std::any::Any;
use std::ffi::{CStr, CString, c_char};
use std::mem;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::{Mutex, OnceLock};

use crate::{
    FfiAbrMode as BridgeAbrMode, FfiAbrPolicy as BridgeAbrPolicy, FfiAudioInfo, FfiAudioOutputInfo,
    FfiBufferingPolicy as BridgeBufferingPolicy, FfiBufferingPreset as BridgeBufferingPreset,
    FfiCachePolicy as BridgeCachePolicy, FfiCachePreset as BridgeCachePreset, FfiCommand,
    FfiDecodedAudioSummary, FfiError as BridgeError, FfiErrorCategory as BridgeErrorCategory,
    FfiErrorCode as BridgeErrorCode, FfiEvent as BridgeEvent, FfiFirstFrameReady,
    FfiFrameProcessorPolicyAction as BridgeFrameProcessorPolicyAction,
    FfiFrameProcessorWarning as BridgeFrameProcessorWarning,
    FfiFrameProcessorWarningKind as BridgeFrameProcessorWarningKind,
    FfiMediaInfo as BridgeMediaInfo, FfiMediaSourceKind as BridgeMediaSourceKind,
    FfiMediaSourceProtocol as BridgeMediaSourceProtocol, FfiPixelFormat as BridgePixelFormat,
    FfiPlaybackState, FfiPlayer, FfiPlayerInitializer,
    FfiPluginCapabilitySummary as BridgePluginCapabilitySummary,
    FfiPluginDecoderCapabilitySummary as BridgePluginDecoderCapabilitySummary,
    FfiPluginDiagnostic as BridgePluginDiagnostic,
    FfiPluginDiagnosticStatus as BridgePluginDiagnosticStatus,
    FfiPluginFrameProcessorCapabilitySummary as BridgePluginFrameProcessorCapabilitySummary,
    FfiPluginParticipation as BridgePluginParticipation,
    FfiPluginSourceNormalizerCapabilitySummary as BridgePluginSourceNormalizerCapabilitySummary,
    FfiPreloadBudgetPolicy as BridgePreloadBudgetPolicy, FfiProgress as BridgeProgress,
    FfiResolvedPreloadBudgetPolicy as BridgeResolvedPreloadBudgetPolicy,
    FfiResolvedResiliencePolicy as BridgeResolvedResiliencePolicy,
    FfiRetryBackoff as BridgeRetryBackoff, FfiRetryPolicy as BridgeRetryPolicy,
    FfiRuntimeWarning as BridgeRuntimeWarning,
    FfiRuntimeWarningDomain as BridgeRuntimeWarningDomain, FfiSeekableRange as BridgeSeekableRange,
    FfiSnapshot as BridgeSnapshot, FfiStartup as BridgeStartup,
    FfiTimelineKind as BridgeTimelineKind, FfiTimelineSnapshot as BridgeTimelineSnapshot,
    FfiTrack as BridgeTrack, FfiTrackCatalog as BridgeTrackCatalog,
    FfiTrackKind as BridgeTrackKind, FfiTrackPreferences as BridgeTrackPreferences,
    FfiTrackSelection as BridgeTrackSelection, FfiTrackSelectionMode as BridgeTrackSelectionMode,
    FfiTrackSelectionSnapshot as BridgeTrackSelectionSnapshot,
    FfiVideoDecodeInfo as BridgeVideoDecodeInfo, FfiVideoDecodeMode as BridgeVideoDecodeMode,
    FfiVideoFrame as BridgeVideoFrame, FfiVideoInfo, resolve_preload_budget,
    resolve_resilience_policy, resolve_track_preferences,
};

mod types;
pub use types::*;

mod strings;
pub(crate) use strings::*;
mod commands;
pub(crate) use commands::*;
mod conversions;
mod free;
pub(crate) use free::*;
mod handles;
pub(crate) use handles::*;
mod lifecycle;
pub(crate) use lifecycle::*;
#[cfg(test)]
mod tests;

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_initializer_probe_uri(
    uri: *const c_char,
    out_initializer: *mut PlayerFfiInitializerHandle,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_initializer.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_initializer was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        write_default_if_non_null(out_initializer);
        let uri = match read_uri(uri) {
            Ok(uri) => uri,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        match FfiPlayerInitializer::probe_uri(uri) {
            Ok(initializer) => {
                let Some(handle) = into_initializer_handle(initializer) else {
                    write_error(
                        out_error,
                        owned_api_error(
                            PlayerFfiErrorCode::BackendFailure,
                            "initializer handle registry overflow",
                        ),
                    );
                    return PlayerFfiCallStatus::Error;
                };
                write_handle(out_initializer, handle);
                PlayerFfiCallStatus::Ok
            }
            Err(error) => {
                write_error(out_error, owned_bridge_error(error));
                PlayerFfiCallStatus::Error
            }
        }
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_resolve_resilience_policy(
    source_kind: PlayerFfiMediaSourceKind,
    source_protocol: PlayerFfiMediaSourceProtocol,
    buffering_policy: *const PlayerFfiBufferingPolicy,
    retry_policy: *const PlayerFfiRetryPolicy,
    cache_policy: *const PlayerFfiCachePolicy,
    out_policy: *mut PlayerFfiResolvedResiliencePolicy,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_policy.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_policy was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let buffering_policy = match read_buffering_policy(buffering_policy) {
            Ok(policy) => policy,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let retry_policy = match read_retry_policy(retry_policy) {
            Ok(policy) => policy,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let cache_policy = match read_cache_policy(cache_policy) {
            Ok(policy) => policy,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let resolved = resolve_resilience_policy(
            source_kind.into(),
            source_protocol.into(),
            buffering_policy,
            retry_policy,
            cache_policy,
        );

        unsafe {
            ptr::write(out_policy, resolved.into());
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_resolve_preload_budget(
    preload_budget: *const PlayerFfiPreloadBudgetPolicy,
    out_budget: *mut PlayerFfiResolvedPreloadBudgetPolicy,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_budget.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_budget was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let preload_budget = match read_preload_budget(preload_budget) {
            Ok(preload_budget) => preload_budget,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let resolved = resolve_preload_budget(preload_budget);
        unsafe {
            ptr::write(out_budget, resolved.into());
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_resolve_track_preferences(
    track_preferences: *const PlayerFfiTrackPreferences,
    out_preferences: *mut PlayerFfiTrackPreferences,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_preferences.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_preferences was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let track_preferences = match read_track_preferences(track_preferences) {
            Ok(track_preferences) => track_preferences,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let resolved = resolve_track_preferences(track_preferences);
        unsafe {
            ptr::write(out_preferences, resolved.into());
        }
        PlayerFfiCallStatus::Ok
    })
}

#[unsafe(no_mangle)]
/// Destroys an initializer handle.
///
/// Passing a zero-initialized handle is a no-op. Passing a stale or already
/// consumed handle returns `PlayerFfiErrorCode::InvalidState`.
/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
pub unsafe extern "C" fn player_ffi_initializer_destroy(
    handle: PlayerFfiInitializerHandle,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if handle.is_invalid() {
            return PlayerFfiCallStatus::Ok;
        }

        if destroy_initializer_handle(handle) {
            PlayerFfiCallStatus::Ok
        } else {
            write_error(out_error, invalid_initializer_handle_error());
            PlayerFfiCallStatus::Error
        }
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_initializer_media_info(
    handle: PlayerFfiInitializerHandle,
    out_media_info: *mut PlayerFfiMediaInfo,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_media_info.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_media_info was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let Some(initializer) =
            with_initializer_ref(handle, |initializer| initializer.media_info())
        else {
            write_error(out_error, invalid_initializer_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        unsafe {
            ptr::write(out_media_info, initializer.into());
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_initializer_startup(
    handle: PlayerFfiInitializerHandle,
    out_startup: *mut PlayerFfiStartup,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_startup.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_startup was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let Some(startup) = with_initializer_ref(handle, |initializer| initializer.startup())
        else {
            write_error(out_error, invalid_initializer_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        unsafe {
            ptr::write(out_startup, startup.into());
        }
        PlayerFfiCallStatus::Ok
    })
}

#[unsafe(no_mangle)]
/// Consumes `handle` and initializes a player instance.
///
/// On both success and error, `handle` is consumed and must not be passed to
/// `player_ffi_initializer_destroy` or any other `player_ffi_initializer_*`
/// function afterwards. Reusing the consumed handle returns
/// `PlayerFfiErrorCode::InvalidState`.
/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
pub unsafe extern "C" fn player_ffi_initializer_initialize(
    handle: PlayerFfiInitializerHandle,
    out_player: *mut PlayerFfiHandle,
    out_has_initial_frame: *mut bool,
    out_initial_frame: *mut PlayerFfiVideoFrame,
    out_startup: *mut PlayerFfiStartup,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_player.is_null()
            || out_has_initial_frame.is_null()
            || out_initial_frame.is_null()
            || out_startup.is_null()
        {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::NullPointer,
                    "one or more initialize output pointers were null",
                ),
            );
            return PlayerFfiCallStatus::Error;
        }

        write_default_if_non_null(out_player);
        let Some(initializer) = take_initializer(handle) else {
            write_error(out_error, invalid_initializer_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        match initializer.initialize() {
            Ok(bootstrap) => {
                let has_initial_frame = bootstrap.initial_frame.is_some();
                let initial_frame = bootstrap
                    .initial_frame
                    .map(PlayerFfiVideoFrame::from)
                    .unwrap_or_default();
                let Some(player_handle) = into_player_handle(bootstrap.player) else {
                    write_error(
                        out_error,
                        owned_api_error(
                            PlayerFfiErrorCode::BackendFailure,
                            "player handle registry overflow",
                        ),
                    );
                    return PlayerFfiCallStatus::Error;
                };
                unsafe {
                    ptr::write(out_player, player_handle);
                    ptr::write(out_has_initial_frame, has_initial_frame);
                    ptr::write(out_initial_frame, initial_frame);
                    ptr::write(out_startup, bootstrap.startup.into());
                }
                PlayerFfiCallStatus::Ok
            }
            Err(error) => {
                write_error(out_error, owned_bridge_error(error));
                PlayerFfiCallStatus::Error
            }
        }
    })
}

#[unsafe(no_mangle)]
/// Destroys a player handle.
///
/// Passing a zero-initialized handle is a no-op. Passing a stale or already
/// destroyed handle returns `PlayerFfiErrorCode::InvalidState`.
/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
pub unsafe extern "C" fn player_ffi_player_destroy(
    handle: PlayerFfiHandle,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if handle.is_invalid() {
            return PlayerFfiCallStatus::Ok;
        }

        if destroy_player_handle(handle) {
            PlayerFfiCallStatus::Ok
        } else {
            write_error(out_error, invalid_player_handle_error());
            PlayerFfiCallStatus::Error
        }
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_player_snapshot(
    handle: PlayerFfiHandle,
    out_snapshot: *mut PlayerFfiSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_snapshot.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_snapshot was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let Some(snapshot) = with_player_ref(handle, |player| player.snapshot()) else {
            write_error(out_error, invalid_player_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        unsafe {
            ptr::write(out_snapshot, snapshot.into());
        }
        PlayerFfiCallStatus::Ok
    })
}

#[unsafe(no_mangle)]
/// Dispatches a player command and writes the resulting snapshot.
///
/// `out_frame` is optional. Pass `NULL` when the caller does not need an
/// immediate frame payload for this dispatch.
/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
pub unsafe extern "C" fn player_ffi_player_dispatch(
    handle: PlayerFfiHandle,
    command: PlayerFfiCommandKind,
    position_ms: u64,
    out_applied: *mut bool,
    out_frame: *mut PlayerFfiVideoFrame,
    out_snapshot: *mut PlayerFfiSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_applied.is_null() || out_snapshot.is_null() {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::NullPointer,
                    "out_applied or out_snapshot was null",
                ),
            );
            return PlayerFfiCallStatus::Error;
        }

        let Some(result) = with_player_mut(handle, |player| {
            player.dispatch(to_bridge_command(command, position_ms))
        }) else {
            write_error(out_error, invalid_player_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        match result {
            Ok(result) => {
                unsafe {
                    ptr::write(out_applied, result.applied);
                    ptr::write(out_snapshot, result.snapshot.into());
                    if !out_frame.is_null() {
                        let frame = result
                            .frame
                            .map(PlayerFfiVideoFrame::from)
                            .unwrap_or_default();
                        ptr::write(out_frame, frame);
                    }
                }
                PlayerFfiCallStatus::Ok
            }
            Err(error) => {
                write_error(out_error, owned_bridge_error(error));
                PlayerFfiCallStatus::Error
            }
        }
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_player_set_playback_rate(
    handle: PlayerFfiHandle,
    playback_rate: f32,
    out_applied: *mut bool,
    out_snapshot: *mut PlayerFfiSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_applied.is_null() || out_snapshot.is_null() {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::NullPointer,
                    "out_applied or out_snapshot was null",
                ),
            );
            return PlayerFfiCallStatus::Error;
        }

        let Some(result) =
            with_player_mut(handle, |player| player.set_playback_rate(playback_rate))
        else {
            write_error(out_error, invalid_player_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        match result {
            Ok(result) => {
                unsafe {
                    ptr::write(out_applied, result.applied);
                    ptr::write(out_snapshot, result.snapshot.into());
                }
                PlayerFfiCallStatus::Ok
            }
            Err(error) => {
                write_error(out_error, owned_bridge_error(error));
                PlayerFfiCallStatus::Error
            }
        }
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_player_set_video_track_selection(
    handle: PlayerFfiHandle,
    selection: *const PlayerFfiTrackSelection,
    out_applied: *mut bool,
    out_snapshot: *mut PlayerFfiSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_applied.is_null() || out_snapshot.is_null() {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::NullPointer,
                    "out_applied or out_snapshot was null",
                ),
            );
            return PlayerFfiCallStatus::Error;
        }

        let selection = match read_track_selection(selection) {
            Ok(selection) => selection,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let Some(result) =
            with_player_mut(handle, |player| player.set_video_track_selection(selection))
        else {
            write_error(out_error, invalid_player_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        match result {
            Ok(result) => {
                unsafe {
                    ptr::write(out_applied, result.applied);
                    ptr::write(out_snapshot, result.snapshot.into());
                }
                PlayerFfiCallStatus::Ok
            }
            Err(error) => {
                write_error(out_error, owned_bridge_error(error));
                PlayerFfiCallStatus::Error
            }
        }
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_player_set_audio_track_selection(
    handle: PlayerFfiHandle,
    selection: *const PlayerFfiTrackSelection,
    out_applied: *mut bool,
    out_snapshot: *mut PlayerFfiSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_applied.is_null() || out_snapshot.is_null() {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::NullPointer,
                    "out_applied or out_snapshot was null",
                ),
            );
            return PlayerFfiCallStatus::Error;
        }

        let selection = match read_track_selection(selection) {
            Ok(selection) => selection,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let Some(result) =
            with_player_mut(handle, |player| player.set_audio_track_selection(selection))
        else {
            write_error(out_error, invalid_player_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        match result {
            Ok(result) => {
                unsafe {
                    ptr::write(out_applied, result.applied);
                    ptr::write(out_snapshot, result.snapshot.into());
                }
                PlayerFfiCallStatus::Ok
            }
            Err(error) => {
                write_error(out_error, owned_bridge_error(error));
                PlayerFfiCallStatus::Error
            }
        }
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_player_set_subtitle_track_selection(
    handle: PlayerFfiHandle,
    selection: *const PlayerFfiTrackSelection,
    out_applied: *mut bool,
    out_snapshot: *mut PlayerFfiSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_applied.is_null() || out_snapshot.is_null() {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::NullPointer,
                    "out_applied or out_snapshot was null",
                ),
            );
            return PlayerFfiCallStatus::Error;
        }

        let selection = match read_track_selection(selection) {
            Ok(selection) => selection,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let Some(result) = with_player_mut(handle, |player| {
            player.set_subtitle_track_selection(selection)
        }) else {
            write_error(out_error, invalid_player_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        match result {
            Ok(result) => {
                unsafe {
                    ptr::write(out_applied, result.applied);
                    ptr::write(out_snapshot, result.snapshot.into());
                }
                PlayerFfiCallStatus::Ok
            }
            Err(error) => {
                write_error(out_error, owned_bridge_error(error));
                PlayerFfiCallStatus::Error
            }
        }
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_player_set_abr_policy(
    handle: PlayerFfiHandle,
    policy: *const PlayerFfiAbrPolicy,
    out_applied: *mut bool,
    out_snapshot: *mut PlayerFfiSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_applied.is_null() || out_snapshot.is_null() {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::NullPointer,
                    "out_applied or out_snapshot was null",
                ),
            );
            return PlayerFfiCallStatus::Error;
        }

        let policy = match read_abr_policy(policy) {
            Ok(policy) => policy,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let Some(result) = with_player_mut(handle, |player| player.set_abr_policy(policy)) else {
            write_error(out_error, invalid_player_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        match result {
            Ok(result) => {
                unsafe {
                    ptr::write(out_applied, result.applied);
                    ptr::write(out_snapshot, result.snapshot.into());
                }
                PlayerFfiCallStatus::Ok
            }
            Err(error) => {
                write_error(out_error, owned_bridge_error(error));
                PlayerFfiCallStatus::Error
            }
        }
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_player_drain_events(
    handle: PlayerFfiHandle,
    out_events: *mut PlayerFfiEventList,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_events.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_events was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let Some(events) = with_player_mut(handle, |player| {
            player
                .drain_events()
                .into_iter()
                .map(PlayerFfiEvent::from)
                .collect::<Vec<_>>()
        }) else {
            write_error(out_error, invalid_player_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        let (ptr, len) = into_owned_struct_array(events);

        unsafe {
            ptr::write(out_events, PlayerFfiEventList { ptr, len });
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_player_advance(
    handle: PlayerFfiHandle,
    out_frame: *mut PlayerFfiVideoFrame,
    out_has_frame: *mut bool,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_frame.is_null() || out_has_frame.is_null() {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::NullPointer,
                    "out_frame or out_has_frame was null",
                ),
            );
            return PlayerFfiCallStatus::Error;
        }

        let Some(result) = with_player_mut(handle, |player| player.advance()) else {
            write_error(out_error, invalid_player_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        match result {
            Ok(frame) => {
                unsafe {
                    ptr::write(out_has_frame, frame.is_some());
                    ptr::write(
                        out_frame,
                        frame.map(PlayerFfiVideoFrame::from).unwrap_or_default(),
                    );
                }
                PlayerFfiCallStatus::Ok
            }
            Err(error) => {
                write_error(out_error, owned_bridge_error(error));
                PlayerFfiCallStatus::Error
            }
        }
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_player_next_deadline_delay_ms(
    handle: PlayerFfiHandle,
    out_has_deadline: *mut bool,
    out_delay_ms: *mut u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_has_deadline.is_null() || out_delay_ms.is_null() {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::NullPointer,
                    "out_has_deadline or out_delay_ms was null",
                ),
            );
            return PlayerFfiCallStatus::Error;
        }

        let Some(deadline) = with_player_ref(handle, |player| player.next_deadline_delay_ms())
        else {
            write_error(out_error, invalid_player_handle_error());
            return PlayerFfiCallStatus::Error;
        };

        unsafe {
            ptr::write(out_has_deadline, deadline.is_some());
            ptr::write(out_delay_ms, deadline.unwrap_or_default());
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_error_free(error: *mut PlayerFfiError) {
    ffi_void(|| {
        let Some(error) = error_mut(error) else {
            return;
        };

        free_c_string(&mut error.message);
        *error = PlayerFfiError::default();
    });
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_media_info_free(media_info: *mut PlayerFfiMediaInfo) {
    ffi_void(|| {
        let Some(media_info) = media_info_mut(media_info) else {
            return;
        };

        free_media_info(media_info);
    });
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_track_preferences_free(
    track_preferences: *mut PlayerFfiTrackPreferences,
) {
    ffi_void(|| {
        let Some(track_preferences) = track_preferences_mut(track_preferences) else {
            return;
        };

        free_track_preferences(track_preferences);
    });
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_startup_free(startup: *mut PlayerFfiStartup) {
    ffi_void(|| {
        let Some(startup) = startup_mut(startup) else {
            return;
        };

        free_startup(startup);
    });
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_snapshot_free(snapshot: *mut PlayerFfiSnapshot) {
    ffi_void(|| {
        let Some(snapshot) = snapshot_mut(snapshot) else {
            return;
        };

        free_snapshot(snapshot);
    });
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_video_frame_free(frame: *mut PlayerFfiVideoFrame) {
    ffi_void(|| {
        let Some(frame) = video_frame_mut(frame) else {
            return;
        };

        free_video_frame(frame);
    });
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_event_list_free(events: *mut PlayerFfiEventList) {
    ffi_void(|| {
        let Some(events) = event_list_mut(events) else {
            return;
        };

        if !events.ptr.is_null() {
            unsafe {
                let mut boxed =
                    Box::from_raw(ptr::slice_from_raw_parts_mut(events.ptr, events.len));
                for event in boxed.iter_mut() {
                    free_event(event);
                }
            }
        }
        *events = PlayerFfiEventList::default();
    });
}
