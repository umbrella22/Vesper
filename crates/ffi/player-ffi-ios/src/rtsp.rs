use serde_json::{Value, json};

fn failure(code: &str, message: &str) -> Value {
    json!({"phase": "failed", "code": code, "message": message})
}

pub fn execute(request: &str) -> Value {
    let Ok(value) = serde_json::from_str::<Value>(request) else {
        return failure(
            "rtsp_invalid_command",
            "The RTSP bridge command is invalid.",
        );
    };
    match value.get("op").and_then(Value::as_str) {
        Some("open") => {
            #[cfg(feature = "rtsp-bridge")]
            {
                let Some(uri) = value.get("uri").and_then(Value::as_str) else {
                    return failure("rtsp_invalid_url", "Expected an RTSP URL.");
                };
                bundled::open(uri)
            }
            #[cfg(not(feature = "rtsp-bridge"))]
            failure(
                "rtsp_bridge_not_bundled",
                "RTSP playback requires an iOS build with the optional RTSP bridge.",
            )
        }
        Some("status" | "close" | "retirementStatus") => {
            #[cfg(feature = "rtsp-bridge")]
            if let Some(handle) = value.get("handle").and_then(Value::as_u64) {
                return match value["op"].as_str() {
                    Some("close") => {
                        bundled::close(handle);
                        json!({"phase": "closed"})
                    }
                    Some("retirementStatus") => bundled::retirement_status(handle),
                    _ => bundled::status(handle),
                };
            }
            failure("rtsp_bridge_closed", "The RTSP bridge session is closed.")
        }
        _ => failure(
            "rtsp_invalid_command",
            "The RTSP bridge command is invalid.",
        ),
    }
}

#[cfg(feature = "rtsp-bridge")]
mod bundled {
    use super::*;
    use player_rtsp_hls_bridge::SessionRetirement;
    use player_rtsp_hls_bridge::host::Session;
    use std::collections::HashMap;
    use std::sync::{
        LazyLock, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    };

    static NEXT: AtomicU64 = AtomicU64::new(1);
    static SESSIONS: LazyLock<Mutex<Registry>> = LazyLock::new(Mutex::default);

    #[derive(Default)]
    struct Registry {
        active: HashMap<u64, Session>,
        retiring: HashMap<u64, SessionRetirement>,
    }

    fn sessions() -> MutexGuard<'static, Registry> {
        SESSIONS.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn open(uri: &str) -> Value {
        let session = match Session::start(uri) {
            Ok(session) => session,
            Err(error) => return failure(error.code(), error.message()),
        };
        // Core reservation bounds the number of simultaneous pending/active opens.
        // Terminal records also count until the host disposes its handle.
        let mut registry = sessions();
        if registry.active.len() >= 2 {
            return failure(
                "rtsp_bridge_busy",
                "The maximum number of RTSP bridge sessions is active.",
            );
        }
        let handle = NEXT.fetch_add(1, Ordering::Relaxed);
        registry.active.insert(handle, session);
        json!({"phase": "connecting", "handle": handle})
    }

    pub fn close(handle: u64) {
        let session = {
            let mut registry = sessions();
            let session = registry.active.remove(&handle);
            // The unfinished entries are bounded by the reservations held by workers.
            registry
                .retiring
                .retain(|_, retirement| !retirement.is_retired());
            if let Some(retirement) = session.as_ref().and_then(Session::retirement)
                && !retirement.is_retired()
            {
                registry.retiring.insert(handle, retirement);
            }
            session
        };
        drop(session);
    }

    pub fn retirement_status(handle: u64) -> Value {
        let mut registry = sessions();
        registry
            .retiring
            .retain(|_, retirement| !retirement.is_retired());
        let phase = if registry.retiring.contains_key(&handle) {
            "closing"
        } else if registry.active.contains_key(&handle) {
            "active"
        } else {
            "closed"
        };
        json!({"phase": phase})
    }

    pub fn status(handle: u64) -> Value {
        sessions().active.get_mut(&handle).map_or_else(
            || failure("rtsp_bridge_closed", "The RTSP bridge session is closed."),
            Session::status,
        )
    }
}
