use player_platform_harmony::HarmonySession;
use serde_json::{Value, json};

fn manifest(number: u64, time: u64) -> String {
    let mut sets = String::new();
    for (kind, codec) in [("video", "avc1.42c01e"), ("audio", "mp4a.40.2")] {
        sets.push_str(&format!(r#"<AdaptationSet contentType="{kind}" mimeType="{kind}/mp4"><SegmentTemplate timescale="1000" startNumber="{number}" initialization="{kind}/init.mp4" media="{kind}/$Time$.m4s"><SegmentTimeline><S t="{time}" d="2000" r="5"/></SegmentTimeline></SegmentTemplate><Representation id="{kind}" codecs="{codec}" bandwidth="100000"/></AdaptationSet>"#));
    }
    format!(
        r#"<MPD type="dynamic" minimumUpdatePeriod="PT2S"><Period id="live">{sets}</Period></MPD>"#
    )
}

fn refresh(session: &mut HarmonySession, mpd: &str) -> Value {
    serde_json::from_str(&session.execute_json(&json!({"op":"refresh","mpd":mpd,
        "manifestUrl":"https://media.test/live/a.mpd?secret=redacted","virtualRoot":"https://media.test/.vesper/epoch/"}).to_string(), 0)).unwrap()
}

#[test]
fn refresh_preserves_full_width_ticks_and_increasing_media_sequences() {
    let mut session = HarmonySession::from_config_json(r#"{"kind":"dashLive"}"#)
        .ok()
        .unwrap();
    let number = 9_007_199_254_740_993;
    let time = 9_007_199_254_742_001;
    let first = refresh(&mut session, &manifest(number, time));
    assert_eq!(first["ok"], true, "{first}");
    assert_eq!(first["value"]["mediaSequences"][0], number.to_string());
    assert!(
        first["value"]["resources"].as_array().unwrap().iter().any(
            |entry| entry["upstreamUri"] == format!("https://media.test/live/video/{time}.m4s")
        )
    );
    let playlist = first["value"]["playlists"][1]["text"].as_str().unwrap();
    assert!(playlist.contains(&format!("MEDIA-SEQUENCE:{number}")));
    assert!(!playlist.contains("ENDLIST"));
    let next = refresh(&mut session, &manifest(number + 1, time + 2000));
    assert_eq!(next["ok"], true, "{next}");
    assert_eq!(next["value"]["mediaSequences"][0], (number + 1).to_string());
}

#[test]
fn failed_refresh_cannot_repoint_segments_or_reset_a_window() {
    let mut session = HarmonySession::from_config_json(r#"{"kind":"dashLive"}"#)
        .ok()
        .unwrap();
    assert_eq!(refresh(&mut session, &manifest(41, 80000))["ok"], true);
    for mpd in [
        manifest(40, 78000),
        manifest(42, 81000),
        manifest(42, 82000).replace("video/$Time$", "changed/$Time$"),
    ] {
        let result = refresh(&mut session, &mpd);
        assert_eq!(result["error"]["code"], "resource_changed", "{result}");
        assert!(!result.to_string().contains("secret"));
    }
    assert_eq!(
        refresh(&mut session, &manifest(42, 82000))["ok"],
        true,
        "Failed input must not commit session state"
    );
}

#[test]
fn refresh_keeps_the_target_for_a_playlist_uri_and_does_not_commit_an_oversized_tail() {
    let mut session = HarmonySession::from_config_json(r#"{"kind":"dashLive"}"#)
        .ok()
        .unwrap();
    let first = refresh(&mut session, &manifest(41, 80_000));
    assert_eq!(first["ok"], true, "{first}");

    let tail = |duration_ms| {
        manifest(42, 82_000).replace("r=\"5\"/>", &format!("r=\"4\"/><S d=\"{duration_ms}\"/>"))
    };
    let rejected = refresh(&mut session, &tail(2_500));
    assert_eq!(rejected["error"]["code"], "resource_changed", "{rejected}");
    assert!(
        rejected["error"]["message"]
            .as_str()
            .unwrap()
            .contains("target duration")
    );

    let next = refresh(&mut session, &tail(2_250));
    assert_eq!(next["ok"], true, "{next}");
    assert_eq!(next["value"]["mediaSequences"], json!(["42", "42"]));
    for index in 1..=2 {
        let old = &first["value"]["playlists"][index];
        let new = &next["value"]["playlists"][index];
        assert_eq!(old["uri"], new["uri"]);
        assert!(
            old["text"]
                .as_str()
                .unwrap()
                .contains("#EXT-X-TARGETDURATION:2\n")
        );
        let text = new["text"].as_str().unwrap();
        assert!(text.contains("#EXT-X-TARGETDURATION:2\n"));
        assert!(text.contains("#EXTINF:2.250,"));
    }
}

#[test]
fn profile_rejects_unmodeled_timing_protection_and_unbounded_windows() {
    for mpd in [
        manifest(1, 0).replace("r=\"5\"", "r=\"-1\""),
        manifest(1, 0).replace("<Period id", "<Period start=\"PT1S\" id"),
        manifest(1, 0).replace("<Representation", "<ContentProtection/><Representation"),
        manifest(1, 0).replace("type=\"dynamic\"", "type=\"static\""),
        manifest(1, 0).replace(
            "<SegmentTemplate ",
            "<SegmentTemplate availabilityTimeOffset=\"2\" ",
        ),
        manifest(1, 0).replacen("t=\"0\"", "t=\"1000\"", 1),
    ] {
        let mut session = HarmonySession::from_config_json(r#"{"kind":"dashLive"}"#)
            .ok()
            .unwrap();
        assert_eq!(refresh(&mut session, &mpd)["error"]["code"], "unsupported");
    }
    let mut session = HarmonySession::from_config_json(r#"{"kind":"dashLive"}"#)
        .ok()
        .unwrap();
    assert_eq!(
        refresh(
            &mut session,
            &manifest(1, 0).replace("r=\"5\"", "r=\"2147483647\"")
        )["error"]["code"],
        "capacity"
    );
}
