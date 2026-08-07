#![warn(clippy::undocumented_unsafe_blocks)]

use std::any::Any;
use std::ffi::{CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

pub fn catch_ffi_call<S, E>(
    f: impl FnOnce() -> S,
    is_success: impl FnOnce(&S) -> bool,
    on_success: impl FnOnce(),
    on_panic: impl FnOnce(Box<dyn Any + Send>) -> E,
) -> Result<S, E> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(status) => {
            if is_success(&status) {
                on_success();
            }
            Ok(status)
        }
        Err(payload) => Err(on_panic(payload)),
    }
}

pub fn catch_ffi_void(f: impl FnOnce()) {
    let _ = catch_unwind(AssertUnwindSafe(f));
}

pub fn panic_payload_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        return (*message).to_owned();
    }

    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }

    "unknown panic payload".to_owned()
}

pub fn into_c_string_ptr(text: String) -> *mut c_char {
    let sanitized = text.replace('\0', " ");
    CString::new(sanitized).unwrap_or_default().into_raw()
}

pub fn free_c_string(ptr_ref: &mut *mut c_char) {
    if !ptr_ref.is_null() && !(*ptr_ref).is_null() {
        // SAFETY: pointers passed here are produced by `CString::into_raw` in
        // this crate or by a crate using the same allocator contract. The
        // pointer is immediately nulled after ownership is reclaimed.
        unsafe {
            drop(CString::from_raw(*ptr_ref));
        }
    }
    *ptr_ref = ptr::null_mut();
}

/// # Safety
///
/// `out` must be either null or a valid, writable pointer to a `*mut c_char`.
/// This is a low-level FFI helper; callers must uphold the pointer validity
/// contract.
pub unsafe fn clear_c_string_output(out: *mut *mut c_char) -> bool {
    // SAFETY: `as_mut` only creates a temporary reference when `out` is non-null.
    // Callers still own the pointed-to slot; this helper only writes the null
    // sentinel used by FFI output parameters.
    let Some(out) = (unsafe { out.as_mut() }) else {
        return false;
    };
    *out = ptr::null_mut();
    true
}

/// # Safety
///
/// `out` must be either null or a valid, writable pointer to a `*mut c_char`.
/// This is a low-level FFI helper; callers must uphold the pointer validity
/// contract.
pub unsafe fn write_c_string_output(out: *mut *mut c_char, text: String) -> bool {
    // SAFETY: `as_mut` only creates a temporary reference when `out` is non-null.
    // The slot must be writable by the caller's FFI contract; this helper does
    // not assume ownership of any previous value stored there.
    let Some(out) = (unsafe { out.as_mut() }) else {
        return false;
    };
    *out = into_c_string_ptr(text);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn c_string_ptr_replaces_embedded_nul() {
        let mut value = into_c_string_ptr("hello\0world".to_owned());
        // SAFETY: `into_c_string_ptr` returned a live NUL-terminated allocation that remains
        // owned by `value` until it is freed below.
        let text = unsafe { CStr::from_ptr(value) }
            .to_str()
            .expect("string should be utf8");
        assert_eq!(text, "hello world");
        free_c_string(&mut value);
        assert!(value.is_null());
    }

    #[test]
    fn c_string_output_helpers_write_and_clear_slot() {
        let mut value: *mut c_char = ptr::null_mut();

        assert!(
            // SAFETY: `value` is a live writable output slot for the duration of the call.
            unsafe { clear_c_string_output(&mut value) }
        );
        assert!(value.is_null());

        assert!(
            // SAFETY: `value` is a live writable output slot for the duration of the call.
            unsafe { write_c_string_output(&mut value, "hello\0world".to_owned()) }
        );
        // SAFETY: the preceding helper returned a live NUL-terminated allocation in `value`.
        let text = unsafe { CStr::from_ptr(value) }
            .to_str()
            .expect("string should be utf8");
        assert_eq!(text, "hello world");

        free_c_string(&mut value);
        assert!(value.is_null());
    }

    #[test]
    fn c_string_output_helpers_reject_null_slot() {
        assert!(
            // SAFETY: null is explicitly accepted and rejected before dereference by this helper.
            !unsafe { clear_c_string_output(ptr::null_mut()) }
        );
        assert!(
            // SAFETY: null is explicitly accepted and rejected before dereference by this helper.
            !unsafe { write_c_string_output(ptr::null_mut(), "ignored".to_owned()) }
        );
    }

    #[test]
    fn panic_payload_message_reads_common_payloads() {
        assert_eq!(panic_payload_message(&"boom"), "boom");
        assert_eq!(panic_payload_message(&"owned".to_owned()), "owned");
    }

    #[test]
    fn catch_ffi_call_invokes_success_hook_only_for_success_status() {
        let success_count = Arc::new(AtomicUsize::new(0));
        let success_count_for_hook = Arc::clone(&success_count);

        let status = catch_ffi_call(
            || 0_u32,
            |status| *status == 0,
            || {
                success_count_for_hook.fetch_add(1, Ordering::SeqCst);
            },
            |_| "panic".to_owned(),
        )
        .expect("call should not panic");
        assert_eq!(status, 0);
        assert_eq!(success_count.load(Ordering::SeqCst), 1);

        let status = catch_ffi_call(
            || 1_u32,
            |status| *status == 0,
            || {
                success_count.fetch_add(1, Ordering::SeqCst);
            },
            |_| "panic".to_owned(),
        )
        .expect("call should not panic");
        assert_eq!(status, 1);
        assert_eq!(success_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn catch_ffi_call_converts_panic_payload() {
        let error = catch_ffi_call(
            || panic!("boom"),
            |_| true,
            || {},
            |payload| panic_payload_message(payload.as_ref()),
        )
        .expect_err("panic should become an error");

        assert_eq!(error, "boom");
    }

    #[test]
    fn catch_ffi_void_swallows_panic() {
        catch_ffi_void(|| panic!("boom"));
    }
}
