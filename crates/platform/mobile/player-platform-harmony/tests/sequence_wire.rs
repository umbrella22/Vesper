use player_platform_harmony::HarmonySession;
use serde_json::{Value, json};

fn call(session: &mut HarmonySession, command: Value) -> Value {
    serde_json::from_str(&session.execute_json(&command.to_string(), 1)).unwrap()
}

#[test]
fn sequence_envelope_and_full_width_warmup_identity_round_trip() {
    let mut session = HarmonySession::from_config_json(
        r#"{"kind":"sequence","configuration":{"sequenceId":"feed"}}"#,
    )
    .ok()
    .unwrap();
    let result = call(&mut session, json!({"op":"snapshot"}));
    assert_eq!(result["ok"], true);
    assert!(result["value"]["sessionGeneration"].is_string());
    let replaced = call(
        &mut session,
        json!({"op":"command", "command":{"type":"replace", "activeItemId":"a", "items":[{
            "itemId":"a", "providerNamespace":"test", "contentIdentity":"a", "mediaKind":"vod",
            "resolvedSource":{"sourceReference":"a", "cacheIdentity":{"providerNamespace":"test","contentIdentity":"a",
                "renditionIdentity":"one","resourceIdentity":"media","accessPartition":"account","sourceRevision":"1"},
                "warmupGoal":"progressiveRange"},
            "preloadProfile":{"expectedMemoryBytes":"0","expectedDiskBytes":"1024","ttlMs":30000,"warmupWindowMs":5000}
        }]}}),
    );
    assert_eq!(replaced["ok"], true, "{replaced}");
    let intents = call(&mut session, json!({"op":"preloadIntents"}));
    let intent = &intents["value"]["intents"][0];
    let task_id = intent["warmupTaskId"].as_str().unwrap();
    assert!(task_id.parse::<u64>().unwrap() > 0);
    let report = call(
        &mut session,
        json!({"op":"command","command":{"type":"reportWarmup",
        "sessionGeneration":intent["sessionGeneration"], "taskId":task_id,"itemId":"a","sourceRevision":"1",
        "warmupGoal":"progressiveRange","status":"completed","expectedBytes":"1024","actualBytes":"1024",
        "cacheHit":false,"cacheEntries":1,"cacheBytes":"1024","evictedEntries":"0"}}),
    );
    assert_eq!(report["ok"], true, "{report}");
    let snapshot = call(&mut session, json!({"op":"snapshot"}));
    assert_eq!(snapshot["value"]["warmupTasks"][0]["taskId"], task_id);
    let invalid = call(
        &mut session,
        json!({"op":"command", "command":{"type":"markSourceExpired","itemId":"a","sourceRevision":1}}),
    );
    assert_eq!(invalid["error"]["code"], "invalid_argument");
}

#[test]
fn dash_errors_do_not_echo_manifest_credentials() {
    let mut session = HarmonySession::from_config_json(r#"{"kind":"dash"}"#)
        .ok()
        .unwrap();
    let result = call(
        &mut session,
        json!({"operation":"parse_manifest","manifestUrl":"https://secret/token","mpd":"broken"}),
    );
    assert_eq!(result["ok"], false);
    assert!(!result.to_string().contains("secret"));
    let protected = call(
        &mut session,
        json!({"operation":"parse_manifest","manifestUrl":"https://example.test/a.mpd",
        "mpd":"<MPD xmlns:x='urn:test'><Period><x:ContentProtection schemeIdUri='secret'/></Period></MPD>"}),
    );
    assert_eq!(protected["error"]["code"], "unsupported");
    assert!(
        protected["error"]["message"]
            .as_str()
            .unwrap()
            .contains("protected")
    );
}

#[test]
fn dash_timing_offsets_are_rejected_before_lossy_manifest_mapping() {
    let mut session = HarmonySession::from_config_json(r#"{"kind":"dash"}"#)
        .ok()
        .unwrap();
    for mpd in [
        r#"<MPD type="static"><Period start="PT1S"/></MPD>"#,
        r#"<MPD type="static"><Period><AdaptationSet><SegmentBase presentationTimeOffset="1000" timescale="1000"/></AdaptationSet></Period></MPD>"#,
        r#"<MPD type="static"><Period><AdaptationSet><SegmentTemplate presentationTimeOffset="0.0"/></AdaptationSet></Period></MPD>"#,
        r#"<MPD type="static"><Period><AdaptationSet><SegmentTemplate presentationTimeOffset="-1"/></AdaptationSet></Period></MPD>"#,
    ] {
        let result = call(
            &mut session,
            json!({"operation":"parse_manifest","manifestUrl":"https://example.test/a.mpd","mpd":mpd}),
        );
        assert_eq!(result["error"]["code"], "unsupported");
        assert!(
            result["error"]["message"]
                .as_str()
                .unwrap()
                .contains("presentation offsets")
        );
    }
}

#[test]
fn zero_dash_presentation_offsets_preserve_the_unshifted_playback_timeline() {
    let mut session = HarmonySession::from_config_json(r#"{"kind":"dash"}"#)
        .ok()
        .unwrap();
    for offset in ["0", "000", "+0", "&#48;"] {
        let mpd = format!(
            r#"<MPD type="static" mediaPresentationDuration="PT4S"><Period><AdaptationSet mimeType="video/mp4" contentType="video"><SegmentTemplate timescale="90000" duration="180000" initialization="$RepresentationID$-Header.m4s" media="$RepresentationID$-$Number$.m4s" presentationTimeOffset="{offset}"/><Representation id="video" bandwidth="200000" codecs="avc1.4D401E" width="320" height="180"/></AdaptationSet></Period></MPD>"#
        );
        let result = call(
            &mut session,
            json!({"operation":"parse_manifest","manifestUrl":"https://example.test/a.mpd","mpd":mpd}),
        );
        assert_eq!(result["ok"], true, "offset {offset}: {result}");
        assert_eq!(
            result["value"]["periods"][0]["adaptationSets"][0]["representations"][0]["id"],
            "video"
        );
    }
}

#[test]
fn template_timeline_offsets_are_preserved_in_the_shared_model() {
    let mut session = HarmonySession::from_config_json(r#"{"kind":"dash"}"#)
        .ok()
        .unwrap();
    let mpd = r#"<MPD type="static" mediaPresentationDuration="PT4S"><Period><AdaptationSet mimeType="video/mp4" contentType="video"><SegmentTemplate timescale="1000" presentationTimeOffset="9000" startNumber="41" initialization="init.mp4" media="$Time$.m4s"><SegmentTimeline><S t="9000" d="2000" r="1"/></SegmentTimeline></SegmentTemplate><Representation id="video" bandwidth="200000" codecs="avc1.4D401E" width="320" height="180"/></AdaptationSet></Period></MPD>"#;
    let result = call(
        &mut session,
        json!({"operation":"parse_manifest","manifestUrl":"https://example.test/a.mpd","mpd":mpd}),
    );
    assert_eq!(result["ok"], true, "{result}");
    let template = &result["value"]["periods"][0]["adaptationSets"][0]["representations"][0]["segmentTemplate"];
    assert_eq!(template["presentationTimeOffset"], 9000);
    assert_eq!(template["startNumber"], 41);
    assert_eq!(template["timeline"][0]["startTime"], 9000);
}
