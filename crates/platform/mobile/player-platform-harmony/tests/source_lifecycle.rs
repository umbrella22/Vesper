use player_platform_harmony::HarmonySession;
use serde_json::{Value, json};

fn command(session: &mut HarmonySession, value: Value, now: u64) -> Value {
    serde_json::from_str(&session.execute_json(&value.to_string(), now)).unwrap()
}

fn source() -> Value {
    json!({"uri":"https://example.test/video.mp4", "label":"Test", "kind":"remote", "protocol":"progressive", "headers":{"Authorization":"secret"}})
}

#[test]
fn external_subtitle_identity_and_flags_survive_source_registration() {
    let mut session = HarmonySession::from_config_json(r#"{"kind":"sources"}"#)
        .ok()
        .unwrap();
    let subtitle = json!({"id":"日本語/強制#1", "uri":"https://example.test/captions.vtt", "mimeType":"text/vtt", "language":"jpn", "label":"Japanese captions", "isDefault":true, "isForced":false});
    let mut value = source();
    value["externalSubtitles"] = json!([subtitle.clone()]);
    let registration = command(&mut session, json!({"op":"register","source":value}), 1);
    assert_eq!(registration["ok"], true, "{registration}");
    let acquisition = command(
        &mut session,
        json!({"op":"acquire","sourceId":registration["value"]["sourceId"]}),
        2,
    );
    assert_eq!(
        acquisition["value"]["descriptor"]["externalSubtitles"][0],
        subtitle
    );
}

#[test]
fn external_subtitles_reject_missing_duplicate_or_invalid_identity_fields() {
    let valid =
        json!({"id":"caption", "uri":"https://example.test/captions.vtt", "mimeType":"text/vtt"});
    let mut missing = valid.clone();
    missing.as_object_mut().unwrap().remove("id");
    let mut flag = valid.clone();
    flag["isForced"] = json!(1);
    let mut long = valid.clone();
    long["id"] = json!("a".repeat(257));
    let mut empty = valid.clone();
    empty["id"] = json!("");
    let mut control = valid.clone();
    control["id"] = json!("caption\nidentity");
    for items in [
        json!([missing]),
        json!([valid.clone(), valid]),
        json!([flag]),
        json!([long]),
        json!([empty]),
        json!([control]),
    ] {
        let mut session = HarmonySession::from_config_json(r#"{"kind":"sources"}"#)
            .ok()
            .unwrap();
        let mut value = source();
        value["externalSubtitles"] = items;
        let result = command(&mut session, json!({"op":"register","source":value}), 1);
        assert_eq!(result["error"]["code"], "invalid_argument", "{result}");
    }
}

#[test]
fn ordinary_close_keeps_a_lease_but_revocation_and_expiry_reject_activation() {
    let mut session = HarmonySession::from_config_json(r#"{"kind":"sources"}"#)
        .ok()
        .unwrap();
    let registration = command(
        &mut session,
        json!({"op":"register","source":source(),"expiresAtEpochMs":100}),
        1,
    );
    let id = registration["value"]["sourceId"].clone();
    let acquisition = command(&mut session, json!({"op":"acquire","sourceId":id}), 2);
    assert_eq!(acquisition["value"]["expiresAtEpochMs"], 100);
    let lease = acquisition["value"]["leaseId"].clone();
    command(&mut session, json!({"op":"closeSource","sourceId":id}), 3);
    assert_eq!(
        command(&mut session, json!({"op":"acquire","sourceId":id}), 4)["error"]["code"],
        "closed"
    );
    assert_eq!(
        command(
            &mut session,
            json!({"op":"validateLease","leaseId":lease}),
            99
        )["ok"],
        true
    );
    assert_eq!(
        command(
            &mut session,
            json!({"op":"validateLease","leaseId":lease}),
            100
        )["error"]["code"],
        "expired"
    );
    assert_eq!(
        command(
            &mut session,
            json!({"op":"validateLease","leaseId":lease}),
            99
        )["error"]["code"],
        "expired"
    );
    command(&mut session, json!({"op":"invalidate"}), 5);
    assert_eq!(
        command(
            &mut session,
            json!({"op":"validateLease","leaseId":lease}),
            6
        )["error"]["code"],
        "invalidated"
    );
    command(
        &mut session,
        json!({"op":"releaseLease","leaseId":lease}),
        7,
    );
    command(
        &mut session,
        json!({"op":"releaseLease","leaseId":lease}),
        7,
    );
    assert_eq!(
        command(&mut session, json!({"op":"snapshot"}), 8)["value"]["registrations"],
        0
    );
    assert_eq!(
        command(
            &mut session,
            json!({"op":"validateLease","leaseId":lease}),
            8
        )["error"]["code"],
        "stale_lease"
    );
}

#[test]
fn capacity_includes_closed_but_retained_sources_and_identities_are_not_reused() {
    let mut session = HarmonySession::from_config_json(r#"{"kind":"sources","maxSources":1}"#)
        .ok()
        .unwrap();
    let first =
        command(&mut session, json!({"op":"register","source":source()}), 1)["value"]["sourceId"]
            .clone();
    let lease =
        command(&mut session, json!({"op":"acquire","sourceId":first}), 1)["value"]["leaseId"]
            .clone();
    command(
        &mut session,
        json!({"op":"closeSource","sourceId":first}),
        1,
    );
    assert_eq!(
        command(
            &mut session,
            json!({"op":"validateLease","leaseId":lease}),
            1
        )["ok"],
        true
    );
    assert_eq!(
        command(&mut session, json!({"op":"register","source":source()}), 1)["error"]["code"],
        "capacity"
    );
    command(
        &mut session,
        json!({"op":"releaseLease","leaseId":lease}),
        1,
    );
    let next =
        command(&mut session, json!({"op":"register","source":source()}), 1)["value"]["sourceId"]
            .clone();
    assert_ne!(first, next);
    assert_eq!(
        command(&mut session, json!({"op":"snapshot"}), 1)["value"]["leases"],
        0
    );
}

#[test]
fn monotonic_expiry_cannot_be_extended_by_an_unobserved_clock_rollback() {
    let mut session = HarmonySession::from_config_json(r#"{"kind":"sources"}"#)
        .ok()
        .unwrap();
    let id = command(
        &mut session,
        json!({"op":"register","source":source(),"expiresAtEpochMs":1020}),
        1000,
    )["value"]["sourceId"]
        .clone();
    let lease =
        command(&mut session, json!({"op":"acquire","sourceId":id}), 1000)["value"]["leaseId"]
            .clone();
    std::thread::sleep(std::time::Duration::from_millis(30));
    assert_eq!(
        command(
            &mut session,
            json!({"op":"validateLease","leaseId":lease}),
            999
        )["error"]["code"],
        "expired"
    );
}

#[test]
fn errors_do_not_echo_source_credentials_or_accept_unknown_protocols() {
    let mut session = HarmonySession::from_config_json(r#"{"kind":"sources"}"#)
        .ok()
        .unwrap();
    let mut protected = source();
    protected["drmConfiguration"] = json!({"licenseUri":"https://secret.test/token"});
    let result = command(&mut session, json!({"op":"register","source":protected}), 1);
    assert_eq!(result["error"]["code"], "unsupported");
    assert!(!result.to_string().contains("secret"));
    let mut unknown = source();
    unknown["protocol"] = json!("future_protocol");
    assert_eq!(
        command(&mut session, json!({"op":"register","source":unknown}), 1)["error"]["code"],
        "unsupported"
    );
    assert_eq!(
        command(&mut session, json!({"op":"snapshot"}), 1)["value"]["registrations"],
        0
    );
}
