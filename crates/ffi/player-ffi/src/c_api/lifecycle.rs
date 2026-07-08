use super::*;

pub(crate) fn ffi_call(
    out_error: *mut PlayerFfiError,
    f: impl FnOnce() -> PlayerFfiCallStatus,
) -> PlayerFfiCallStatus {
    match player_ffi_common::catch_ffi_call(
        f,
        |status| *status == PlayerFfiCallStatus::Ok,
        || write_success(out_error),
        owned_panic_error,
    ) {
        Ok(status) => status,
        Err(error) => {
            write_error(out_error, error);
            PlayerFfiCallStatus::Error
        }
    }
}

pub(crate) fn ffi_void(f: impl FnOnce()) {
    player_ffi_common::catch_ffi_void(f);
}

pub(crate) fn owned_panic_error(payload: Box<dyn Any + Send>) -> PlayerFfiError {
    let message = player_ffi_common::panic_payload_message(payload.as_ref());
    owned_api_error(
        PlayerFfiErrorCode::BackendFailure,
        &format!("player_ffi caught Rust panic: {message}"),
    )
}

pub(crate) fn write_error(out_error: *mut PlayerFfiError, mut error: PlayerFfiError) {
    if out_error.is_null() {
        free_c_string(&mut error.message);
        return;
    }

    // SAFETY: caller upholds the FFI contract for this pointer operation
    unsafe {
        ptr::write(out_error, error);
    }
}

pub(crate) fn write_success(out_error: *mut PlayerFfiError) {
    if out_error.is_null() {
        return;
    }

    // SAFETY: caller upholds the FFI contract for this pointer operation
    unsafe {
        ptr::write(out_error, PlayerFfiError::default());
    }
}
