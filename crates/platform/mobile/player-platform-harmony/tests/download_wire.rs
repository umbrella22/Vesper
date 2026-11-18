use player_platform_harmony::HarmonySession;
use serde_json::{Value, json};
fn call(session: &mut HarmonySession, value: Value) -> Value {
    serde_json::from_str(&session.execute_json(&value.to_string(), 1)).unwrap()
}
fn source() -> Value {
    json!({"uri":"https://example.test/file.mp4?credential=secret","label":"Test","kind":"remote","protocol":"progressive","headers":{"Authorization":"secret"}})
}
fn plan(session: &mut HarmonySession) -> Value {
    let result = call(
        session,
        json!({"op":"plan","source":source(),"inventory":{"lengths":[{"uri":source()["uri"],"length":"4096"}]}}),
    );
    assert_eq!(result["ok"], true, "{result}");
    result["value"]["planId"].clone()
}
#[test]
fn planner_discovery_and_stale_completion_fences_do_not_expose_credentials() {
    let mut session = HarmonySession::from_config_json(r#"{"kind":"downloads","maxTasks":1}"#)
        .ok()
        .unwrap();
    let discovery = call(&mut session, json!({"op":"plan","source":source()}));
    assert_eq!(discovery["value"]["needs"][0]["kind"], "length");
    let plan = plan(&mut session);
    let created = call(
        &mut session,
        json!({"op":"create","planId":plan,"assetId":"asset"}),
    );
    assert!(!created.to_string().contains("secret"));
    let task = created["value"]["task"]["taskId"].clone();
    assert_eq!(
        call(&mut session, json!({"op":"resume","taskId":task}))["error"]["code"],
        "invalid_state"
    );
    let started = call(&mut session, json!({"op":"start","taskId":task}));
    let attempt = started["value"]["attemptId"].clone();
    assert_eq!(started["value"]["task"]["status"], "Preparing");
    assert_eq!(
        call(
            &mut session,
            json!({"op":"prepared","taskId":task,"attemptId":attempt})
        )["value"]["task"]["status"],
        "Downloading"
    );
    call(&mut session, json!({"op":"pause","taskId":task}));
    assert_eq!(
        call(&mut session, json!({"op":"start","taskId":task}))["error"]["code"],
        "invalid_state"
    );
    let late = call(
        &mut session,
        json!({"op":"complete","taskId":task,"attemptId":attempt}),
    );
    assert_eq!(late["error"]["code"], "stale_attempt");
    call(&mut session, json!({"op":"remove","taskId":task}));
    assert_eq!(
        call(
            &mut session,
            json!({"op":"progress","taskId":task,"attemptId":attempt,"receivedBytes":"4096","receivedSegments":0})
        )["error"]["code"],
        "stale_attempt"
    );
    assert_eq!(
        call(
            &mut session,
            json!({"op":"create","planId":plan,"assetId":"replacement"})
        )["error"]["code"],
        "capacity"
    );
}
#[test]
fn ignored_profile_fields_and_unaccounted_large_strings_are_rejected() {
    let mut session = HarmonySession::from_config_json(r#"{"kind":"downloads"}"#)
        .ok()
        .unwrap();
    for profile in [
        json!({"targetOutputFormat":"mp4"}),
        json!({"variantId": "a".repeat(800 * 1024)}),
        json!({"variantId":123}),
    ] {
        let result = call(
            &mut session,
            json!({"op":"plan","source":source(),"profile":profile}),
        );
        assert_eq!(result["ok"], false);
    }
}
#[test]
fn process_restore_requires_access_refresh_and_preserves_full_width_task_ids() {
    let mut session = HarmonySession::from_config_json(r#"{"kind":"downloads"}"#)
        .ok()
        .unwrap();
    let restored = call(
        &mut session,
        json!({"op":"restore","tasks":[{"taskId":"9007199254740993","assetId":"old","totalBytes":"4096","receivedBytes":"1024","verifiedComplete":false}]}),
    );
    assert_eq!(restored["value"]["tasks"][0]["taskId"], "9007199254740993");
    assert_eq!(
        call(
            &mut session,
            json!({"op":"resume","taskId":"9007199254740993"})
        )["error"]["code"],
        "access_context_required"
    );
    let plan = plan(&mut session);
    assert_eq!(
        call(
            &mut session,
            json!({"op":"refresh","taskId":"9007199254740993","planId":plan})
        )["ok"],
        true
    );
    let created = call(
        &mut session,
        json!({"op":"create","assetId":"new","planId":plan}),
    );
    assert_eq!(created["value"]["task"]["taskId"], "9007199254740994");
}

#[test]
fn finite_dash_does_not_silently_rewrite_unsupported_segment_semantics() {
    let mut session = HarmonySession::from_config_json(r#"{"kind":"downloads"}"#)
        .ok()
        .unwrap();
    for media in ["segment-$Bandwidth$.m4s", "segment-$Unknown$.m4s"] {
        let mpd = format!(
            r#"<MPD type="static" mediaPresentationDuration="PT6S"><Period><AdaptationSet mimeType="video/mp4"><Representation id="v" bandwidth="1000"><SegmentTemplate duration="2" initialization="init.mp4" media="{media}"><SegmentTimeline><S d="3" r="1"/></SegmentTimeline></SegmentTemplate></Representation></AdaptationSet></Period></MPD>"#
        );
        let result = call(
            &mut session,
            json!({"op":"plan","source":{"kind":"remote","protocol":"dash","uri":"https://example.test/manifest.mpd","label":"DASH"},"inventory":{"texts":[{"uri":"https://example.test/manifest.mpd","text":mpd}]}}),
        );
        assert_eq!(result["error"]["code"], "unsupported");
        assert!(result["value"].is_null());
    }
}

#[test]
fn removed_restore_cannot_become_a_resumable_task() {
    let mut session = HarmonySession::from_config_json(r#"{"kind":"downloads"}"#)
        .ok()
        .unwrap();
    let result = call(
        &mut session,
        json!({"op":"restore","tasks":[{"taskId":"1","assetId":"removed","receivedBytes":"0","removed":true}]}),
    );
    assert_eq!(result["value"]["tasks"][0]["status"], "Removed");
    let plan = plan(&mut session);
    assert_eq!(
        call(
            &mut session,
            json!({"op":"refresh","taskId":"1","planId":plan})
        )["error"]["code"],
        "invalid_state"
    );
}

#[test]
fn finite_templates_discover_bounded_length_batches_and_reject_quota_before_probes() {
    let mut session = HarmonySession::from_config_json(r#"{"kind":"downloads"}"#).unwrap();
    let uri = "https://example.test/manifest.mpd";
    let template = |seconds| {
        format!(
            r#"<MPD type="static" mediaPresentationDuration="PT{seconds}S"><Period><AdaptationSet mimeType="video/mp4" codecs="avc1.42c01e"><Representation id="v" bandwidth="1000"><SegmentTemplate duration="2" startNumber="7" initialization="init.mp4" media="seg-$Number%05d$.m4s"/></Representation></AdaptationSet></Period></MPD>"#
        )
    };
    let request = |seconds| json!({"op":"plan","source":{"kind":"remote","protocol":"dash","uri":uri,"label":"DASH"},"inventory":{"texts":[{"uri":uri,"text":template(seconds)}]}});
    let result = call(&mut session, request(6));
    let needs = result["value"]["needs"].as_array().unwrap();
    assert_eq!(needs.len(), 4, "{result}");
    assert_eq!(needs[1]["uri"], "https://example.test/seg-00007.m4s");
    let mut complete = request(6);
    complete["inventory"]["lengths"] = Value::Array(
        needs
            .iter()
            .map(|need| json!({"uri":need["uri"],"length":"16"}))
            .collect(),
    );
    let result = call(&mut session, complete);
    assert_eq!(
        result["value"]["index"]["files"].as_array().unwrap().len(),
        5,
        "{result}"
    );
    let batch = call(&mut session, request(7200));
    assert_eq!(batch["value"]["needs"].as_array().unwrap().len(), 128);
    let large = call(&mut session, request(20000));
    assert_eq!(large["error"]["code"], "unsupported", "{large}");
    assert!(large["value"].is_null());
}
