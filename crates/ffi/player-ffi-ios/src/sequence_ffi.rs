use std::ffi::c_char;
use std::ptr;
use std::sync::{Arc, Mutex, MutexGuard};

use player_platform_ios::IosSequenceBridgeSession;

use crate::conversions::{
    ffi_call, ffi_void, free_c_string, into_c_string_ptr, owned_api_error, read_required_c_string,
    write_error,
};
use crate::handles::{IosSequenceBridgeSessionHandle, lock_registry, sequence_sessions};
use crate::{PlayerFfiCallStatus, PlayerFfiError, PlayerFfiErrorCode};

fn lock_session(
    session: &IosSequenceBridgeSessionHandle,
) -> MutexGuard<'_, IosSequenceBridgeSession> {
    match session.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn clone_sequence_session(handle: u64) -> Result<IosSequenceBridgeSessionHandle, PlayerFfiError> {
    let sessions = lock_registry(sequence_sessions()).map_err(|_| {
        owned_api_error(
            PlayerFfiErrorCode::InvalidArgument,
            "sequence session registry lock failed",
        )
    })?;
    sessions.get(handle).cloned().ok_or_else(|| {
        owned_api_error(
            PlayerFfiErrorCode::InvalidArgument,
            "invalid sequence session handle",
        )
    })
}

unsafe fn write_json_result(
    out_json: *mut *mut c_char,
    value: String,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    if out_json.is_null() {
        write_error(
            out_error,
            owned_api_error(PlayerFfiErrorCode::NullPointer, "out_json was null"),
        );
        return PlayerFfiCallStatus::Error;
    }
    // SAFETY: the caller guarantees that `out_json` points to writable storage.
    unsafe { ptr::write(out_json, into_c_string_ptr(value)) };
    PlayerFfiCallStatus::Ok
}

/// Creates an iOS playback-sequence session.
///
/// # Safety
///
/// `config_json` must be a valid null-terminated UTF-8 string. Output pointers
/// must be writable when non-null.
pub(crate) unsafe fn player_ffi_sequence_session_create_json_impl(
    config_json: *const c_char,
    out_handle: *mut u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_handle.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_handle was null"),
            );
            return PlayerFfiCallStatus::Error;
        }
        let config_json = match read_required_c_string(config_json, "config_json") {
            Ok(value) => value,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let session = match IosSequenceBridgeSession::from_config_json(&config_json) {
            Ok(session) => session,
            Err(error) => {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidArgument,
                        &format!("{}: {}", error.code, error.message),
                    ),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        let Ok(mut sessions) = lock_registry(sequence_sessions()) else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "sequence session registry lock failed",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };
        let handle = sessions.insert(Arc::new(Mutex::new(session)));
        // SAFETY: the caller guarantees that `out_handle` points to writable storage.
        unsafe { ptr::write(out_handle, handle) };
        PlayerFfiCallStatus::Ok
    })
}

/// Disposes an iOS playback-sequence session. Repeated disposal is a no-op.
///
/// # Safety
///
/// The caller must not concurrently use the same handle during disposal.
pub(crate) unsafe fn player_ffi_sequence_session_dispose_impl(handle: u64) {
    ffi_void(|| {
        if let Ok(mut sessions) = lock_registry(sequence_sessions()) {
            let _ = sessions.remove(handle);
        }
    });
}

/// Executes one bounded sequence command and returns a Rust-owned JSON string.
///
/// # Safety
///
/// `command_json` must be a valid null-terminated UTF-8 string. Output pointers
/// must be writable when non-null.
pub(crate) unsafe fn player_ffi_sequence_session_execute_json_impl(
    handle: u64,
    command_json: *const c_char,
    wall_epoch_ms: u64,
    out_json: *mut *mut c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        let command_json = match read_required_c_string(command_json, "command_json") {
            Ok(value) => value,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let session = match clone_sequence_session(handle) {
            Ok(session) => session,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let response = lock_session(&session).execute_json(&command_json, wall_epoch_ms);
        // SAFETY: this function forwards the caller's documented output-pointer contract.
        unsafe { write_json_result(out_json, response, out_error) }
    })
}

/// Returns the authoritative sequence snapshot as a Rust-owned JSON string.
///
/// # Safety
///
/// Output pointers must be writable when non-null.
pub(crate) unsafe fn player_ffi_sequence_session_snapshot_json_impl(
    handle: u64,
    out_json: *mut *mut c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        let session = match clone_sequence_session(handle) {
            Ok(session) => session,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let response = lock_session(&session).snapshot_json();
        // SAFETY: this function forwards the caller's documented output-pointer contract.
        unsafe { write_json_result(out_json, response, out_error) }
    })
}

/// Drains at most `max_count` sequence events into a Rust-owned JSON string.
///
/// # Safety
///
/// Output pointers must be writable when non-null.
pub(crate) unsafe fn player_ffi_sequence_session_drain_events_json_impl(
    handle: u64,
    max_count: usize,
    out_json: *mut *mut c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        let session = match clone_sequence_session(handle) {
            Ok(session) => session,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let response = lock_session(&session).drain_events_json(max_count);
        // SAFETY: this function forwards the caller's documented output-pointer contract.
        unsafe { write_json_result(out_json, response, out_error) }
    })
}

/// Returns safe sequence preload intents as a Rust-owned JSON string.
///
/// # Safety
///
/// Output pointers must be writable when non-null.
pub(crate) unsafe fn player_ffi_sequence_session_preload_intents_json_impl(
    handle: u64,
    wall_epoch_ms: u64,
    out_json: *mut *mut c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        let session = match clone_sequence_session(handle) {
            Ok(session) => session,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let response = lock_session(&session).preload_intents_json(wall_epoch_ms);
        // SAFETY: this function forwards the caller's documented output-pointer contract.
        unsafe { write_json_result(out_json, response, out_error) }
    })
}

/// Releases a string returned by a sequence JSON API.
///
/// # Safety
///
/// `value` must be null or originate from a sequence JSON API in this library.
pub(crate) unsafe fn player_ffi_sequence_string_free_impl(value: *mut c_char) {
    ffi_void(|| {
        let mut value = value;
        free_c_string(&mut value);
    });
}

#[cfg(test)]
mod tests {
    use std::ffi::{CStr, CString};
    use std::ptr;

    use super::*;
    use crate::{
        player_ffi_error_free, player_ffi_sequence_session_create_json,
        player_ffi_sequence_session_dispose, player_ffi_sequence_session_execute_json,
        player_ffi_sequence_session_snapshot_json, player_ffi_sequence_string_free,
    };

    fn take_json(value: *mut c_char) -> String {
        assert!(!value.is_null());
        // SAFETY: the pointer was returned by a successful sequence JSON API
        // and remains owned by Rust until freed below.
        let json = unsafe { CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: allocator pairing is guaranteed by the sequence FFI API.
        unsafe { player_ffi_sequence_string_free(value) };
        json
    }

    #[test]
    fn sequence_ffi_executes_two_item_progressive_slice_and_fences_handles() {
        let config = CString::new(r#"{"sequenceId":"ffi-sequence"}"#).expect("config");
        let mut handle = 0_u64;
        let mut error = PlayerFfiError::default();
        // SAFETY: all pointers reference initialized caller-owned storage for this call.
        let created = unsafe {
            player_ffi_sequence_session_create_json(config.as_ptr(), &mut handle, &mut error)
        };
        assert_eq!(created, PlayerFfiCallStatus::Ok);
        assert_ne!(handle, 0);

        let command = CString::new(
            r#"{"type":"replace","items":[{"itemId":"a","providerNamespace":"example.provider","contentIdentity":"a","mediaKind":"vod","resolvedSource":{"sourceReference":"source-a-1","cacheIdentity":{"providerNamespace":"example.provider","contentIdentity":"a","renditionIdentity":"1080p","resourceIdentity":"media","accessPartition":"public","sourceRevision":1}}},{"itemId":"b","providerNamespace":"example.provider","contentIdentity":"b","mediaKind":"vod","resolvedSource":{"sourceReference":"source-b-1","cacheIdentity":{"providerNamespace":"example.provider","contentIdentity":"b","renditionIdentity":"1080p","resourceIdentity":"media","accessPartition":"public","sourceRevision":1}}}],"activeItemId":"a"}"#,
        )
        .expect("command");
        let mut output = ptr::null_mut();
        // SAFETY: all pointers reference initialized caller-owned storage for this call.
        let replaced = unsafe {
            player_ffi_sequence_session_execute_json(
                handle,
                command.as_ptr(),
                1_000,
                &mut output,
                &mut error,
            )
        };
        assert_eq!(replaced, PlayerFfiCallStatus::Ok);
        assert!(take_json(output).contains(r#""ok":true"#));

        let next = CString::new(r#"{"type":"next"}"#).expect("next");
        output = ptr::null_mut();
        // SAFETY: all pointers reference initialized caller-owned storage for this call.
        let advanced = unsafe {
            player_ffi_sequence_session_execute_json(
                handle,
                next.as_ptr(),
                1_000,
                &mut output,
                &mut error,
            )
        };
        assert_eq!(advanced, PlayerFfiCallStatus::Ok);
        assert!(take_json(output).contains(r#""itemId":"b""#));

        output = ptr::null_mut();
        // SAFETY: output pointers reference initialized caller-owned storage.
        let snapshot =
            unsafe { player_ffi_sequence_session_snapshot_json(handle, &mut output, &mut error) };
        assert_eq!(snapshot, PlayerFfiCallStatus::Ok);
        let snapshot = take_json(output);
        assert!(!snapshot.contains("https://"));
        assert!(!snapshot.contains("headers"));

        // SAFETY: handle disposal is idempotent by contract.
        unsafe {
            player_ffi_sequence_session_dispose(handle);
            player_ffi_sequence_session_dispose(handle);
        }
        output = ptr::null_mut();
        // SAFETY: output pointers reference initialized caller-owned storage.
        let stale =
            unsafe { player_ffi_sequence_session_snapshot_json(handle, &mut output, &mut error) };
        assert_eq!(stale, PlayerFfiCallStatus::Error);
        assert!(output.is_null());
        // SAFETY: the error owns strings allocated by the matching FFI library.
        unsafe { player_ffi_error_free(&mut error) };
    }
}
