use std::ptr;
use vesper_player_harmony::*;

fn create() -> u64 {
    let text = br#"{"kind":"sources"}"#;
    // SAFETY: The input slice is valid for the duration of the call.
    let reply = unsafe { vesper_harmony_create(text.as_ptr(), text.len()) };
    assert_eq!(reply.status, VesperHarmonyStatus::Ok);
    reply.value
}

fn call(handle: u64, text: &[u8]) -> VesperHarmonyReply {
    // SAFETY: The input slice is valid for the duration of the call.
    unsafe { vesper_harmony_call(handle, text.as_ptr(), text.len(), 1) }
}

#[test]
fn ownership_capacity_and_failure_outputs() {
    assert_eq!(vesper_harmony_abi_version(), 1);
    // SAFETY: Null plus a positive length is rejected before memory access.
    let bad = unsafe { vesper_harmony_create(ptr::null(), 1) };
    assert_eq!(
        (bad.status, bad.reserved, bad.value),
        (VesperHarmonyStatus::InvalidArgument, 0, 0)
    );
    let session = create();
    let mut responses = Vec::new();
    for _ in 0..32 {
        let reply = call(session, br#"{"op":"snapshot"}"#);
        assert_eq!(reply.status, VesperHarmonyStatus::Ok);
        responses.push(reply.value);
    }
    let rejected = call(session, br#"{"op":"register","source":{"uri":"https://example.test/a","kind":"remote","protocol":"progressive"}}"#);
    assert_eq!(
        (rejected.status, rejected.value),
        (VesperHarmonyStatus::Capacity, 0)
    );
    let response = responses.remove(0);
    // SAFETY: A null/zero size probe writes no memory.
    let size = unsafe { vesper_harmony_buffer_read(response, ptr::null_mut(), 0) };
    assert_eq!(size.status, VesperHarmonyStatus::BufferTooSmall);
    let mut short = [0xa5_u8; 1];
    // SAFETY: The output slice is writable for its declared length.
    assert_eq!(
        unsafe { vesper_harmony_buffer_read(response, short.as_mut_ptr(), 1) }.status,
        VesperHarmonyStatus::BufferTooSmall
    );
    assert_eq!(short, [0xa5]);
    let mut bytes = vec![0_u8; size.value as usize];
    // SAFETY: The output slice is writable for its declared length.
    assert_eq!(
        unsafe { vesper_harmony_buffer_read(response, bytes.as_mut_ptr(), bytes.len()) }.status,
        VesperHarmonyStatus::Ok
    );
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(text.contains("\"registrations\":0"));
    vesper_harmony_buffer_release(response);
    vesper_harmony_buffer_release(response);
    let snapshot = call(session, br#"{"op":"snapshot"}"#);
    assert_eq!(snapshot.status, VesperHarmonyStatus::Ok);
    // Responses remain usable after session release.
    vesper_harmony_release(session);
    vesper_harmony_release(session);
    assert_eq!(
        call(session, br#"{}"#).status,
        VesperHarmonyStatus::StaleHandle
    );
    // SAFETY: A null/zero size probe writes no memory.
    assert_eq!(
        unsafe { vesper_harmony_buffer_read(snapshot.value, ptr::null_mut(), 0) }.status,
        VesperHarmonyStatus::BufferTooSmall
    );
    vesper_harmony_buffer_release(snapshot.value);
    for response in responses {
        vesper_harmony_buffer_release(response);
    }
    let handles: Vec<_> = (0..64).map(|_| create()).collect();
    let text = br#"{"kind":"sources"}"#;
    // SAFETY: The input slice is valid for this call.
    assert_eq!(
        unsafe { vesper_harmony_create(text.as_ptr(), text.len()) }.status,
        VesperHarmonyStatus::Capacity
    );
    for handle in handles {
        vesper_harmony_release(handle);
    }
    let replacement = create();
    assert!(replacement > session);
    vesper_harmony_release(replacement);
}
