use super::*;
use std::time::{Duration, Instant};

#[test]
fn pending_output_cannot_be_released_to_bypass_admission() {
    let config = br#"{"kind":"sources"}"#;
    // SAFETY: The static byte slice remains valid for the call.
    let session = unsafe { vesper_harmony_create(config.as_ptr(), config.len()) }.value;
    let state = REGISTRY.lock().unwrap_or_else(|e| e.into_inner()).sessions[&session].clone();
    let lock = state.inner.lock().unwrap_or_else(|e| e.into_inner());
    let pending = std::thread::spawn(move || {
        let text = br#"{"op":"snapshot"}"#;
        // SAFETY: The static byte slice remains valid for the call.
        unsafe { vesper_harmony_call(session, text.as_ptr(), text.len(), 1) }
    });
    let started = Instant::now();
    let id = loop {
        let value = REGISTRY
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .outputs
            .iter()
            .find_map(|(id, output)| matches!(output, Output::Pending).then_some(*id));
        if let Some(id) = value {
            break id;
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "call did not reserve its output"
        );
        std::thread::park_timeout(Duration::from_millis(1));
    };
    assert_eq!(
        vesper_harmony_buffer_release(id).status,
        VesperHarmonyStatus::Pending
    );
    assert!(
        REGISTRY
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .outputs
            .contains_key(&id)
    );
    drop(lock);
    let reply = pending.join().unwrap();
    assert_eq!(reply.status, VesperHarmonyStatus::Ok);
    assert_eq!(reply.value, id);
    vesper_harmony_buffer_release(id);
    vesper_harmony_release(session);
}

#[test]
fn panic_does_not_cross_the_abi_result_boundary() {
    let result = guarded(|| panic!("test panic"));
    assert_eq!(
        (result.status, result.value, result.reserved),
        (VesperHarmonyStatus::Panic, 0, 0)
    );
}
