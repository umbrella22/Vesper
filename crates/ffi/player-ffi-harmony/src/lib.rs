//! Internal Harmony C ABI. Handles and response buffers have separate ownership.
//!
//! Callers provide valid readable/writable memory for pointer/length pairs. Null
//! is accepted only for a zero-length slice. No pointer is retained after a call.
//! Input is UTF-8 JSON, limited to 1 MiB. Outputs are immutable, limited to 1 MiB
//! each and 32 outstanding responses. Release every response with buffer_release.
//! Sessions have 64 process slots. Identities are monotonically allocated and
//! never reused. Session release and buffer release are independently idempotent.
//! Calls perform finite CPU work; media, network and file work belongs to the host.
#![warn(clippy::undocumented_unsafe_blocks)]

use std::collections::BTreeMap;
use std::panic::{UnwindSafe, catch_unwind};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use player_platform_harmony::{HarmonySession, MAX_JSON_BYTES, failure};

#[cfg(test)]
mod tests;

/// Stable raw status. Hosts must preserve unrecognized numeric values.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VesperHarmonyStatus {
    Ok = 0,
    InvalidArgument = 1,
    StaleHandle = 2,
    Capacity = 3,
    Panic = 4,
    BufferTooSmall = 5,
    Pending = 6,
}

/// By-value result. Failure has zero value, except BufferTooSmall reports size.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VesperHarmonyReply {
    pub status: VesperHarmonyStatus,
    pub reserved: u32,
    pub value: u64,
}

impl VesperHarmonyReply {
    fn ok(value: u64) -> Self {
        Self {
            status: VesperHarmonyStatus::Ok,
            reserved: 0,
            value,
        }
    }
    fn error(status: VesperHarmonyStatus) -> Self {
        Self {
            status,
            reserved: 0,
            value: 0,
        }
    }
}

struct Session {
    live: AtomicBool,
    inner: Mutex<HarmonySession>,
}

enum Output {
    Pending,
    Ready(Arc<[u8]>),
}

struct Registry {
    next: u64,
    sessions: BTreeMap<u64, Arc<Session>>,
    outputs: BTreeMap<u64, Output>,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    next: 1,
    sessions: BTreeMap::new(),
    outputs: BTreeMap::new(),
});

impl Registry {
    fn identity(&mut self) -> Option<u64> {
        let id = self.next;
        self.next = self.next.checked_add(1)?;
        Some(id)
    }
}

fn guarded(operation: impl FnOnce() -> VesperHarmonyReply + UnwindSafe) -> VesperHarmonyReply {
    catch_unwind(operation)
        .unwrap_or_else(|_| VesperHarmonyReply::error(VesperHarmonyStatus::Panic))
}

/// Owns an output reservation until a completed immutable response replaces it.
struct Reservation {
    id: u64,
    committed: bool,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if !self.committed {
            REGISTRY
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .outputs
                .remove(&self.id);
        }
    }
}

/// Reads a caller-owned UTF-8 slice only for the duration of this ABI call.
///
/// # Safety
/// `data` must point to `length` readable bytes when length is positive.
unsafe fn input<'a>(data: *const u8, length: usize) -> Result<&'a str, VesperHarmonyReply> {
    if length > MAX_JSON_BYTES || (length > 0 && data.is_null()) {
        return Err(VesperHarmonyReply::error(
            VesperHarmonyStatus::InvalidArgument,
        ));
    }
    if length == 0 {
        return Ok("");
    }
    // SAFETY: Non-null and bounded here; the caller guarantees readable memory.
    let bytes = unsafe { std::slice::from_raw_parts(data, length) };
    std::str::from_utf8(bytes)
        .map_err(|_| VesperHarmonyReply::error(VesperHarmonyStatus::InvalidArgument))
}

/// Returns the internal bridge ABI version.
// SAFETY: This unique exported C symbol has the signature in the generated header.
#[unsafe(no_mangle)]
pub extern "C" fn vesper_harmony_abi_version() -> u32 {
    1
}

/// Constructs a source or sequence session. No native player is allocated.
///
/// # Safety
/// `data` must identify `length` readable bytes; no input memory is retained.
// SAFETY: The symbol is uniquely prefixed and uses C-compatible types.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vesper_harmony_create(
    data: *const u8,
    length: usize,
) -> VesperHarmonyReply {
    guarded(|| {
        // SAFETY: The caller's slice obligation is inherited unchanged.
        let text = match unsafe { input(data, length) } {
            Ok(text) => text,
            Err(error) => return error,
        };
        let session = match HarmonySession::from_config_json(text) {
            Ok(inner) => Arc::new(Session {
                live: AtomicBool::new(true),
                inner: Mutex::new(inner),
            }),
            Err(_) => return VesperHarmonyReply::error(VesperHarmonyStatus::InvalidArgument),
        };
        let mut registry = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        if registry.sessions.len() >= 64 {
            return VesperHarmonyReply::error(VesperHarmonyStatus::Capacity);
        }
        let Some(id) = registry.identity() else {
            return VesperHarmonyReply::error(VesperHarmonyStatus::Capacity);
        };
        registry.sessions.insert(id, session);
        VesperHarmonyReply::ok(id)
    })
}

/// Executes a command and returns an owned response identity, not a pointer.
/// Output capacity is reserved before any command mutation. A response survives
/// session release and must be released separately by the caller.
///
/// # Safety
/// `data` must identify `length` readable bytes; no input memory is retained.
// SAFETY: The symbol is uniquely prefixed and uses C-compatible types.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vesper_harmony_call(
    handle: u64,
    data: *const u8,
    length: usize,
    now_epoch_ms: u64,
) -> VesperHarmonyReply {
    guarded(|| {
        // SAFETY: The caller's slice obligation is inherited unchanged.
        let text = match unsafe { input(data, length) } {
            Ok(text) => text,
            Err(error) => return error,
        };
        let (session, mut reservation) = {
            let mut registry = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
            let Some(session) = registry.sessions.get(&handle).cloned() else {
                return VesperHarmonyReply::error(VesperHarmonyStatus::StaleHandle);
            };
            if registry.outputs.len() >= 32 {
                return VesperHarmonyReply::error(VesperHarmonyStatus::Capacity);
            }
            let Some(id) = registry.identity() else {
                return VesperHarmonyReply::error(VesperHarmonyStatus::Capacity);
            };
            registry.outputs.insert(id, Output::Pending);
            (
                session,
                Reservation {
                    id,
                    committed: false,
                },
            )
        };
        // The process registry lock is released before the per-session operation.
        let mut inner = session.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !session.live.load(Ordering::Acquire) {
            return VesperHarmonyReply::error(VesperHarmonyStatus::StaleHandle);
        }
        let mut output = inner.execute_json(text, now_epoch_ms);
        drop(inner);
        if output.len() > MAX_JSON_BYTES {
            output = failure("capacity", "response exceeds the bridge byte limit").to_string();
        }
        let mut registry = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        let Some(slot) = registry.outputs.get_mut(&reservation.id) else {
            return VesperHarmonyReply::error(VesperHarmonyStatus::StaleHandle);
        };
        *slot = Output::Ready(Arc::from(output.into_bytes()));
        reservation.committed = true;
        VesperHarmonyReply::ok(reservation.id)
    })
}

/// Copies an immutable response. A size probe uses null/zero and returns
/// BufferTooSmall with the exact byte count. Short buffers are not modified.
///
/// # Safety
/// `destination` must identify `capacity` writable bytes when capacity is positive.
/// The caller must not alias the destination with memory concurrently accessed.
// SAFETY: The symbol is uniquely prefixed and uses C-compatible types.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vesper_harmony_buffer_read(
    handle: u64,
    destination: *mut u8,
    capacity: usize,
) -> VesperHarmonyReply {
    guarded(|| {
        if capacity > MAX_JSON_BYTES || (capacity > 0 && destination.is_null()) {
            return VesperHarmonyReply::error(VesperHarmonyStatus::InvalidArgument);
        }
        let bytes = {
            let registry = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
            match registry.outputs.get(&handle) {
                Some(Output::Ready(bytes)) => bytes.clone(),
                Some(Output::Pending) => {
                    return VesperHarmonyReply::error(VesperHarmonyStatus::Pending);
                }
                None => return VesperHarmonyReply::error(VesperHarmonyStatus::StaleHandle),
            }
        };
        if capacity < bytes.len() {
            return VesperHarmonyReply {
                status: VesperHarmonyStatus::BufferTooSmall,
                reserved: 0,
                value: bytes.len() as u64,
            };
        }
        if !bytes.is_empty() {
            // SAFETY: Capacity was checked; caller guarantees writable, unaliased
            // destination. Source is a private immutable Rust allocation.
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), destination, bytes.len());
            }
        }
        VesperHarmonyReply::ok(bytes.len() as u64)
    })
}

/// Releases a ready response identity; unknown and released identities succeed.
/// A pending reservation belongs to its executing call and returns Pending.
// SAFETY: The symbol is uniquely prefixed and accepts a by-value integer only.
#[unsafe(no_mangle)]
pub extern "C" fn vesper_harmony_buffer_release(handle: u64) -> VesperHarmonyReply {
    guarded(|| {
        let mut registry = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(registry.outputs.get(&handle), Some(Output::Pending)) {
            return VesperHarmonyReply::error(VesperHarmonyStatus::Pending);
        }
        registry.outputs.remove(&handle);
        VesperHarmonyReply::ok(0)
    })
}

/// Closes a session. Subsequent calls reject the stale handle. No blocking host
/// cleanup or platform callbacks run under the registry lock.
// SAFETY: The symbol is uniquely prefixed and accepts a by-value integer only.
#[unsafe(no_mangle)]
pub extern "C" fn vesper_harmony_release(handle: u64) -> VesperHarmonyReply {
    guarded(|| {
        let removed = REGISTRY
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sessions
            .remove(&handle);
        if let Some(session) = removed {
            session.live.store(false, Ordering::Release);
        }
        VesperHarmonyReply::ok(0)
    })
}
