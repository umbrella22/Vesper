use super::*;

pub(crate) fn ffi_call(
    out_error: *mut PlayerFfiError,
    f: impl FnOnce() -> PlayerFfiCallStatus,
) -> PlayerFfiCallStatus {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(status) => {
            if status == PlayerFfiCallStatus::Ok {
                write_success(out_error);
            }
            status
        }
        Err(payload) => {
            write_error(out_error, owned_panic_error(payload));
            PlayerFfiCallStatus::Error
        }
    }
}

pub(crate) fn ffi_void(f: impl FnOnce()) {
    let _ = catch_unwind(AssertUnwindSafe(f));
}

pub(crate) fn owned_panic_error(payload: Box<dyn Any + Send>) -> PlayerFfiError {
    let message = panic_payload_message(payload.as_ref());
    owned_api_error(
        PlayerFfiErrorCode::BackendFailure,
        &format!("player_ffi caught Rust panic: {message}"),
    )
}

pub(crate) fn panic_payload_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        return (*message).to_owned();
    }

    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }

    "unknown panic payload".to_owned()
}

pub(crate) fn write_error(out_error: *mut PlayerFfiError, mut error: PlayerFfiError) {
    if out_error.is_null() {
        free_c_string(&mut error.message);
        return;
    }

    unsafe {
        ptr::write(out_error, error);
    }
}

pub(crate) fn write_success(out_error: *mut PlayerFfiError) {
    if out_error.is_null() {
        return;
    }

    unsafe {
        ptr::write(out_error, PlayerFfiError::default());
    }
}
