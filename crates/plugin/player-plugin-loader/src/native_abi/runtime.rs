use std::collections::HashSet;
use std::ffi::c_void;
use std::mem::size_of;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use player_plugin_abi::{
    VESPER_MAX_OWNED_BYTES, VESPER_MAX_SESSIONS_PER_INTERFACE, VesperByteSlice, VesperJsonOut,
    VesperOpenSessionOut, VesperOwnedBytes, VesperStatus, status,
};
use serde::de::DeserializeOwned;
use thiserror::Error;

use super::PluginOwner;

static NEXT_INTERFACE_TOKEN: AtomicU64 = AtomicU64::new(1);
pub(super) const OPEN_FAILURE_CLOSE_ATTEMPTS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ActiveSessionError {
    Exhausted,
    Duplicate { session_id: u64 },
}

#[derive(Debug, Default)]
struct ActiveSessionState {
    active: HashSet<u64>,
    opening: usize,
}

#[derive(Debug, Default)]
pub(super) struct ActiveSessionRegistry {
    state: Mutex<ActiveSessionState>,
}

impl ActiveSessionRegistry {
    pub(super) fn reserve_open(&self) -> Result<ActiveSessionReservation<'_>, ActiveSessionError> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.active.len().saturating_add(state.opening) >= VESPER_MAX_SESSIONS_PER_INTERFACE {
            return Err(ActiveSessionError::Exhausted);
        }
        state.opening += 1;
        Ok(ActiveSessionReservation {
            registry: self,
            pending: true,
        })
    }

    pub(super) fn remove(&self, session_id: u64) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .active
            .remove(&session_id);
    }
}

pub(super) struct ActiveSessionReservation<'a> {
    registry: &'a ActiveSessionRegistry,
    pending: bool,
}

impl ActiveSessionReservation<'_> {
    pub(super) fn register(mut self, session_id: u64) -> Result<(), ActiveSessionError> {
        let mut state = self
            .registry
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.opening = state.opening.saturating_sub(1);
        self.pending = false;
        if !state.active.insert(session_id) {
            return Err(ActiveSessionError::Duplicate { session_id });
        }
        Ok(())
    }
}

impl Drop for ActiveSessionReservation<'_> {
    fn drop(&mut self) {
        if !self.pending {
            return;
        }
        let mut state = self
            .registry
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.opening = state.opening.saturating_sub(1);
    }
}

#[derive(Debug, Error)]
pub(super) enum NativeAbiBoundaryError {
    #[error("plugin interface `{interface}` is poisoned before `{operation}`")]
    Poisoned {
        interface: String,
        operation: &'static str,
    },
    #[error("plugin interface `{interface}` panicked while calling `{operation}`")]
    CallbackPanic {
        interface: String,
        operation: &'static str,
    },
    #[error("plugin interface `{interface}` violated the ABI during `{operation}`: {detail}")]
    AbiViolation {
        interface: String,
        operation: &'static str,
        detail: String,
    },
    #[error("plugin interface `{interface}` returned status {status} from `{operation}`: {detail}")]
    ReportedFailure {
        interface: String,
        operation: &'static str,
        status: VesperStatus,
        detail: String,
    },
}

#[derive(Debug)]
pub(super) enum JsonCallResult {
    Success(Vec<u8>),
    Failure {
        status: VesperStatus,
        payload: Vec<u8>,
    },
}

#[derive(Debug)]
pub(super) enum OpenCallResult {
    Success {
        session_id: u64,
        payload: Vec<u8>,
    },
    Failure {
        status: VesperStatus,
        payload: Vec<u8>,
    },
}

#[derive(Debug)]
pub(super) struct InterfaceRuntime {
    owner: Arc<PluginOwner>,
    context: NonNull<c_void>,
    interface: String,
    interface_token: u64,
    poisoned: AtomicBool,
}

// SAFETY: native ABI interface contexts belong to the shared root owner and the ABI
// requires interface factories to support concurrent shared calls. Session
// mutability is represented by separate session wrappers, not this context.
unsafe impl Send for InterfaceRuntime {}
// SAFETY: the same native ABI contract permits shared interface calls, while poison is
// synchronized through an atomic and owner destruction is Arc-controlled.
unsafe impl Sync for InterfaceRuntime {}

impl InterfaceRuntime {
    pub(super) fn new(
        owner: Arc<PluginOwner>,
        context: *mut c_void,
        plugin_id: &str,
        instance_id: &str,
    ) -> Result<Self, NativeAbiBoundaryError> {
        let interface = format!("{plugin_id}:{instance_id}");
        let context =
            NonNull::new(context).ok_or_else(|| NativeAbiBoundaryError::AbiViolation {
                interface: interface.clone(),
                operation: "construct_wrapper",
                detail: "validated interface context became null".to_owned(),
            })?;
        Ok(Self {
            owner,
            context,
            interface,
            interface_token: next_interface_token(),
            poisoned: AtomicBool::new(false),
        })
    }

    pub(super) fn context(&self) -> *mut c_void {
        self.context.as_ptr()
    }

    pub(super) fn interface_token(&self) -> u64 {
        self.interface_token
    }

    pub(super) fn ensure_healthy(
        &self,
        operation: &'static str,
    ) -> Result<(), NativeAbiBoundaryError> {
        if self.poisoned.load(Ordering::Acquire) {
            Err(NativeAbiBoundaryError::Poisoned {
                interface: self.interface.clone(),
                operation,
            })
        } else {
            Ok(())
        }
    }

    pub(super) fn contract_violation(
        &self,
        operation: &'static str,
        detail: impl Into<String>,
    ) -> NativeAbiBoundaryError {
        self.poisoned.store(true, Ordering::Release);
        NativeAbiBoundaryError::AbiViolation {
            interface: self.interface.clone(),
            operation,
            detail: detail.into(),
        }
    }

    pub(super) fn reported_failure(
        &self,
        operation: &'static str,
        status: VesperStatus,
        detail: impl Into<String>,
    ) -> NativeAbiBoundaryError {
        NativeAbiBoundaryError::ReportedFailure {
            interface: self.interface.clone(),
            operation,
            status,
            detail: detail.into(),
        }
    }

    pub(super) fn decode_json<T>(
        &self,
        operation: &'static str,
        payload: &[u8],
    ) -> Result<T, NativeAbiBoundaryError>
    where
        T: DeserializeOwned,
    {
        serde_json::from_slice(payload).map_err(|error| {
            self.contract_violation(operation, format!("returned malformed JSON: {error}"))
        })
    }

    pub(super) fn invoke_json(
        &self,
        operation: &'static str,
        allowed_failure_statuses: &[VesperStatus],
        invoke: impl FnOnce(*mut VesperJsonOut) -> VesperStatus,
    ) -> Result<JsonCallResult, NativeAbiBoundaryError> {
        self.invoke_json_inner(operation, allowed_failure_statuses, false, invoke)
    }

    pub(super) fn invoke_cleanup_json(
        &self,
        operation: &'static str,
        allowed_failure_statuses: &[VesperStatus],
        invoke: impl FnOnce(*mut VesperJsonOut) -> VesperStatus,
    ) -> Result<JsonCallResult, NativeAbiBoundaryError> {
        self.invoke_json_inner(operation, allowed_failure_statuses, true, invoke)
    }

    pub(super) fn invoke_open(
        &self,
        operation: &'static str,
        allowed_failure_statuses: &[VesperStatus],
        invoke: impl FnOnce(*mut VesperOpenSessionOut) -> VesperStatus,
        cleanup: impl FnOnce(u64),
    ) -> Result<OpenCallResult, NativeAbiBoundaryError> {
        let mut out = VesperOpenSessionOut::default();
        let call = self.invoke_callback(operation, false, || invoke(&mut out));
        let payload = self.capture_owned_bytes(operation, out.payload);
        let output_check = self.validate_out_prefix(
            operation,
            out.struct_size,
            out.reserved,
            size_of::<VesperOpenSessionOut>() as u32,
        );
        let result = (|| {
            let payload = payload?;
            output_check?;
            let raw_status = call?;
            match self.classify_json_status(
                operation,
                raw_status,
                allowed_failure_statuses,
                payload,
            )? {
                JsonCallResult::Success(payload) => {
                    if out.session_id == 0 {
                        return Err(self.contract_violation(
                            operation,
                            "successful open returned the zero session sentinel",
                        ));
                    }
                    Ok(OpenCallResult::Success {
                        session_id: out.session_id,
                        payload,
                    })
                }
                JsonCallResult::Failure { status, payload } => {
                    if out.session_id != 0 {
                        return Err(self.contract_violation(
                            operation,
                            format!(
                                "failed open returned non-zero session id {}",
                                out.session_id
                            ),
                        ));
                    }
                    Ok(OpenCallResult::Failure { status, payload })
                }
            }
        })();
        if result.is_err() && out.session_id != 0 {
            cleanup(out.session_id);
        }
        result
    }

    fn invoke_json_inner(
        &self,
        operation: &'static str,
        allowed_failure_statuses: &[VesperStatus],
        cleanup: bool,
        invoke: impl FnOnce(*mut VesperJsonOut) -> VesperStatus,
    ) -> Result<JsonCallResult, NativeAbiBoundaryError> {
        let mut out = VesperJsonOut::default();
        let call = self.invoke_callback(operation, cleanup, || invoke(&mut out));
        let payload = self.capture_owned_bytes(operation, out.payload)?;
        self.validate_out_prefix(
            operation,
            out.struct_size,
            out.reserved,
            size_of::<VesperJsonOut>() as u32,
        )?;
        let raw_status = call?;
        self.classify_json_status(operation, raw_status, allowed_failure_statuses, payload)
    }

    pub(super) fn invoke_callback<T>(
        &self,
        operation: &'static str,
        cleanup: bool,
        invoke: impl FnOnce() -> T,
    ) -> Result<T, NativeAbiBoundaryError> {
        if !cleanup {
            self.ensure_healthy(operation)?;
        }
        catch_unwind(AssertUnwindSafe(invoke)).map_err(|_| {
            self.poisoned.store(true, Ordering::Release);
            NativeAbiBoundaryError::CallbackPanic {
                interface: self.interface.clone(),
                operation,
            }
        })
    }

    pub(super) fn capture_owned_bytes(
        &self,
        operation: &'static str,
        bytes: VesperOwnedBytes,
    ) -> Result<Vec<u8>, NativeAbiBoundaryError> {
        OwnedPluginBytes::new(bytes, self.owner.clone())
            .copy_bytes()
            .map_err(|detail| {
                self.contract_violation(operation, format!("invalid owned bytes: {detail}"))
            })
    }

    pub(super) fn validate_out_prefix(
        &self,
        operation: &'static str,
        struct_size: u32,
        reserved: u32,
        required_size: u32,
    ) -> Result<(), NativeAbiBoundaryError> {
        if struct_size < required_size {
            return Err(self.contract_violation(
                operation,
                format!(
                    "output struct is truncated: required {required_size} bytes, got {struct_size}"
                ),
            ));
        }
        if reserved != 0 {
            return Err(self.contract_violation(
                operation,
                format!("output reserved field is {reserved}, expected 0"),
            ));
        }
        Ok(())
    }

    pub(super) fn classify_json_status(
        &self,
        operation: &'static str,
        raw_status: VesperStatus,
        allowed_failure_statuses: &[VesperStatus],
        payload: Vec<u8>,
    ) -> Result<JsonCallResult, NativeAbiBoundaryError> {
        if raw_status == status::OK {
            return Ok(JsonCallResult::Success(payload));
        }
        if allowed_failure_statuses.contains(&raw_status) {
            return Ok(JsonCallResult::Failure {
                status: raw_status,
                payload,
            });
        }
        Err(self.contract_violation(
            operation,
            format!("returned unexpected status {raw_status}"),
        ))
    }
}

fn next_interface_token() -> u64 {
    loop {
        let token = NEXT_INTERFACE_TOKEN.fetch_add(1, Ordering::Relaxed);
        if token != 0 {
            return token;
        }
    }
}

pub(super) fn borrowed_bytes(bytes: &[u8]) -> VesperByteSlice {
    if bytes.is_empty() {
        VesperByteSlice::empty()
    } else {
        VesperByteSlice {
            data: bytes.as_ptr(),
            len: bytes.len() as u64,
        }
    }
}

struct OwnedPluginBytes {
    bytes: VesperOwnedBytes,
    owner: Arc<PluginOwner>,
}

impl OwnedPluginBytes {
    fn new(bytes: VesperOwnedBytes, owner: Arc<PluginOwner>) -> Self {
        Self { bytes, owner }
    }

    fn copy_bytes(&self) -> Result<Vec<u8>, String> {
        if self.bytes.len > VESPER_MAX_OWNED_BYTES {
            return Err(format!(
                "length {} exceeds the {VESPER_MAX_OWNED_BYTES}-byte protocol limit",
                self.bytes.len
            ));
        }
        if self.bytes.len == 0 {
            return if self.bytes.data.is_null() {
                Ok(Vec::new())
            } else {
                Err("non-null pointer paired with zero length".to_owned())
            };
        }
        if self.bytes.data.is_null() {
            return Err(format!(
                "null pointer paired with length {}",
                self.bytes.len
            ));
        }
        let len = usize::try_from(self.bytes.len)
            .map_err(|_| format!("length {} does not fit usize", self.bytes.len))?;
        if len > isize::MAX as usize {
            return Err(format!("length {} exceeds isize::MAX", self.bytes.len));
        }
        // SAFETY: the native ABI promises that a non-null plugin-owned output
        // points to `len` readable initialized bytes until `free_bytes`. The
        // shared protocol cap and representability checks run before this read.
        let bytes = unsafe { std::slice::from_raw_parts(self.bytes.data.cast_const(), len) };
        Ok(bytes.to_vec())
    }
}

impl Drop for OwnedPluginBytes {
    fn drop(&mut self) {
        self.owner.free_bytes(self.bytes);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct FixtureOwner {
        frees: AtomicUsize,
        destroys: AtomicUsize,
    }

    unsafe extern "C" fn free_bytes(owner: *mut c_void, bytes: VesperOwnedBytes) {
        // SAFETY: each test passes a live `FixtureOwner` as the owner pointer.
        let owner = unsafe { &*owner.cast::<FixtureOwner>() };
        owner.frees.fetch_add(1, Ordering::SeqCst);
        // SAFETY: test callbacks create every non-empty payload with
        // `VesperOwnedBytes::from_vec` in this binary and release it once here.
        let _ = unsafe { bytes.into_vec() };
    }

    unsafe extern "C" fn destroy_owner(owner: *mut c_void) {
        // SAFETY: each test keeps the fixture alive until the runtime drops.
        let owner = unsafe { &*owner.cast::<FixtureOwner>() };
        owner.destroys.fetch_add(1, Ordering::SeqCst);
    }

    fn fixture_runtime(owner: &mut FixtureOwner) -> InterfaceRuntime {
        let owner_ptr = std::ptr::from_mut(owner).cast::<c_void>();
        InterfaceRuntime::new(
            Arc::new(PluginOwner {
                // SAFETY: `owner_ptr` was created from a live mutable reference.
                owner: unsafe { NonNull::new_unchecked(owner_ptr) },
                free_bytes,
                destroy_owner,
                library: None,
            }),
            owner_ptr,
            "dev.vesper.fixture",
            "dev.vesper.fixture.hook",
        )
        .expect("fixture runtime")
    }

    #[test]
    fn unexpected_status_reclaims_bytes_and_poison_blocks_the_next_call() {
        let mut owner = FixtureOwner {
            frees: AtomicUsize::new(0),
            destroys: AtomicUsize::new(0),
        };
        let runtime = fixture_runtime(&mut owner);
        let calls = AtomicUsize::new(0);
        let first = runtime.invoke_json("on_event_json", &[status::FAILURE], |out| {
            calls.fetch_add(1, Ordering::SeqCst);
            // SAFETY: `out` is the live host-owned output for this call.
            unsafe { (*out).payload = VesperOwnedBytes::from_vec(b"{}".to_vec()) };
            status::PANIC
        });
        assert!(matches!(
            first,
            Err(NativeAbiBoundaryError::AbiViolation { .. })
        ));
        assert_eq!(owner.frees.load(Ordering::SeqCst), 1);

        let second = runtime.invoke_json("on_event_json", &[status::FAILURE], |_out| {
            calls.fetch_add(1, Ordering::SeqCst);
            status::OK
        });
        assert!(matches!(
            second,
            Err(NativeAbiBoundaryError::Poisoned { .. })
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        drop(runtime);
        assert_eq!(owner.destroys.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn truncated_output_reclaims_payload_before_poisoning() {
        let mut owner = FixtureOwner {
            frees: AtomicUsize::new(0),
            destroys: AtomicUsize::new(0),
        };
        let runtime = fixture_runtime(&mut owner);
        let result = runtime.invoke_json("flush_json", &[status::FAILURE], |out| {
            // SAFETY: `out` is the live host-owned output for this call.
            unsafe {
                (*out).payload = VesperOwnedBytes::from_vec(b"{}".to_vec());
                (*out).struct_size = (size_of::<VesperJsonOut>() - 1) as u32;
            }
            status::OK
        });
        assert!(matches!(
            result,
            Err(NativeAbiBoundaryError::AbiViolation { .. })
        ));
        assert_eq!(owner.frees.load(Ordering::SeqCst), 1);
        drop(runtime);
        assert_eq!(owner.destroys.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn callback_panic_reclaims_payload_and_poison_blocks_reentry() {
        let mut owner = FixtureOwner {
            frees: AtomicUsize::new(0),
            destroys: AtomicUsize::new(0),
        };
        let runtime = fixture_runtime(&mut owner);
        let result = runtime.invoke_json("flush_json", &[status::FAILURE], |out| {
            // SAFETY: `out` is the live host-owned output for this call.
            unsafe { (*out).payload = VesperOwnedBytes::from_vec(b"{}".to_vec()) };
            panic!("fixture panic")
        });
        assert!(matches!(
            result,
            Err(NativeAbiBoundaryError::CallbackPanic { .. })
        ));
        assert_eq!(owner.frees.load(Ordering::SeqCst), 1);
        assert!(matches!(
            runtime.ensure_healthy("flush_json"),
            Err(NativeAbiBoundaryError::Poisoned { .. })
        ));
        drop(runtime);
        assert_eq!(owner.destroys.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn active_session_registry_rejects_duplicates_and_enforces_capacity() {
        let registry = ActiveSessionRegistry::default();
        for session_id in 1..=VESPER_MAX_SESSIONS_PER_INTERFACE as u64 {
            registry
                .reserve_open()
                .expect("session capacity")
                .register(session_id)
                .expect("unique session id");
        }
        assert!(matches!(
            registry.reserve_open(),
            Err(ActiveSessionError::Exhausted)
        ));

        registry.remove(1);
        registry
            .reserve_open()
            .expect("released capacity")
            .register(VESPER_MAX_SESSIONS_PER_INTERFACE as u64 + 1)
            .expect("replacement session");
        registry.remove(2);
        assert!(matches!(
            registry
                .reserve_open()
                .expect("duplicate reservation")
                .register(3),
            Err(ActiveSessionError::Duplicate { session_id: 3 })
        ));
    }
}
