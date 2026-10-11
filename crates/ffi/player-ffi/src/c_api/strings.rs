use std::ptr::NonNull;

use super::*;

pub(crate) fn owned_bridge_error(error: BridgeError) -> PlayerFfiError {
    let details_json = error
        .details_json()
        .map(|details| into_c_string_ptr(details.to_owned()))
        .unwrap_or(std::ptr::null_mut());
    PlayerFfiError {
        code: error.code().into(),
        category: error.category().into(),
        retriable: error.is_retriable(),
        message: into_c_string_ptr(error.message().to_owned()),
        details_json,
    }
}

pub(crate) fn owned_api_error(code: PlayerFfiErrorCode, message: &str) -> PlayerFfiError {
    PlayerFfiError {
        code,
        category: api_error_category(code),
        retriable: false,
        message: into_c_string_ptr(message.to_owned()),
        details_json: std::ptr::null_mut(),
    }
}

pub(crate) fn api_error_category(code: PlayerFfiErrorCode) -> PlayerFfiErrorCategory {
    match code {
        PlayerFfiErrorCode::NullPointer
        | PlayerFfiErrorCode::InvalidUtf8
        | PlayerFfiErrorCode::InvalidArgument => PlayerFfiErrorCategory::Input,
        PlayerFfiErrorCode::InvalidState
        | PlayerFfiErrorCode::SeekFailure
        | PlayerFfiErrorCode::CommandChannelClosed
        | PlayerFfiErrorCode::EventChannelClosed
        | PlayerFfiErrorCode::Cancelled
        | PlayerFfiErrorCode::Timeout => PlayerFfiErrorCategory::Playback,
        PlayerFfiErrorCode::InvalidSource => PlayerFfiErrorCategory::Source,
        PlayerFfiErrorCode::AudioOutputUnavailable => PlayerFfiErrorCategory::AudioOutput,
        PlayerFfiErrorCode::DecodeFailure => PlayerFfiErrorCategory::Decode,
        PlayerFfiErrorCode::Unsupported => PlayerFfiErrorCategory::Capability,
        PlayerFfiErrorCode::BackendFailure | PlayerFfiErrorCode::None => {
            PlayerFfiErrorCategory::Platform
        }
    }
}

pub(crate) fn into_c_string_ptr(text: String) -> *mut c_char {
    player_ffi_common::into_c_string_ptr(text)
}

pub(crate) fn into_owned_bytes(bytes: Vec<u8>) -> (*mut u8, usize) {
    if bytes.is_empty() {
        return (ptr::null_mut(), 0);
    }

    let slice = Box::into_non_null(bytes.into_boxed_slice());
    let len = slice.len();
    (slice.as_ptr().cast(), len)
}

pub(crate) fn into_owned_struct_array<T>(values: Vec<T>) -> (*mut T, usize) {
    if values.is_empty() {
        return (ptr::null_mut(), 0);
    }

    let slice = Box::into_non_null(values.into_boxed_slice());
    let len = slice.len();
    (slice.as_ptr().cast(), len)
}

pub(crate) fn into_owned_c_string_array(values: Vec<String>) -> (*mut *mut c_char, usize) {
    let values = values
        .into_iter()
        .map(into_c_string_ptr)
        .collect::<Vec<_>>();
    into_owned_struct_array(values)
}

pub(crate) fn free_c_string(ptr_ref: &mut *mut c_char) {
    player_ffi_common::free_c_string(ptr_ref);
}

pub(crate) fn free_c_string_array(ptr_ref: &mut *mut *mut c_char, len: usize) {
    if let Some(strings) = NonNull::new(*ptr_ref) {
        let slice = NonNull::slice_from_raw_parts(strings, len);
        // SAFETY: the caller supplies the boxed slice from `into_owned_struct_array`
        // with its original length and transfers sole responsibility for release.
        // Nested C strings are freed before the containing slice is dropped.
        let mut strings = unsafe { Box::from_non_null(slice) };
        for value in strings.iter_mut() {
            free_c_string(value);
        }
    }
    *ptr_ref = ptr::null_mut();
}
