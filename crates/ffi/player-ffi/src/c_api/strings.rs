use super::*;

pub(crate) fn owned_bridge_error(error: BridgeError) -> PlayerFfiError {
    PlayerFfiError {
        code: error.code().into(),
        category: error.category().into(),
        retriable: error.is_retriable(),
        message: into_c_string_ptr(error.message().to_owned()),
    }
}

pub(crate) fn owned_api_error(code: PlayerFfiErrorCode, message: &str) -> PlayerFfiError {
    PlayerFfiError {
        code,
        category: api_error_category(code),
        retriable: false,
        message: into_c_string_ptr(message.to_owned()),
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
    let sanitized = text.replace('\0', " ");
    CString::new(sanitized).unwrap_or_default().into_raw()
}

pub(crate) fn into_owned_bytes(bytes: Vec<u8>) -> (*mut u8, usize) {
    if bytes.is_empty() {
        return (ptr::null_mut(), 0);
    }

    let mut boxed = bytes.into_boxed_slice();
    let len = boxed.len();
    let ptr = boxed.as_mut_ptr();
    mem::forget(boxed);
    (ptr, len)
}

pub(crate) fn into_owned_struct_array<T>(values: Vec<T>) -> (*mut T, usize) {
    if values.is_empty() {
        return (ptr::null_mut(), 0);
    }

    let mut boxed = values.into_boxed_slice();
    let len = boxed.len();
    let ptr = boxed.as_mut_ptr();
    mem::forget(boxed);
    (ptr, len)
}

pub(crate) fn into_owned_c_string_array(values: Vec<String>) -> (*mut *mut c_char, usize) {
    let values = values
        .into_iter()
        .map(into_c_string_ptr)
        .collect::<Vec<_>>();
    into_owned_struct_array(values)
}

pub(crate) fn free_c_string(ptr_ref: &mut *mut c_char) {
    if !ptr_ref.is_null() && !(*ptr_ref).is_null() {
        unsafe {
            drop(CString::from_raw(*ptr_ref));
        }
    }
    *ptr_ref = ptr::null_mut();
}

pub(crate) fn free_c_string_array(ptr_ref: &mut *mut *mut c_char, len: usize) {
    if !ptr_ref.is_null() && !(*ptr_ref).is_null() {
        unsafe {
            let mut boxed = Box::from_raw(ptr::slice_from_raw_parts_mut(*ptr_ref, len));
            for value in boxed.iter_mut() {
                free_c_string(value);
            }
        }
    }
    *ptr_ref = ptr::null_mut();
}
