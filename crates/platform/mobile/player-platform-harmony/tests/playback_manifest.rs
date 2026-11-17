use player_platform_harmony::HarmonySession;
use serde_json::{Value, json};

fn project(protocol: &str, text: &str, routes: Value) -> Value {
    let mut session = HarmonySession::from_config_json(r#"{"kind":"playbackManifest"}"#)
        .ok()
        .unwrap();
    serde_json::from_str(
        &session.execute_json(
            &json!({"protocol": protocol, "manifest": text,
        "manifestUrl": "https://media.test/a/master", "routes": routes})
            .to_string(),
            0,
        ),
    )
    .unwrap()
}

#[test]
fn hls_trick_play_entries_cannot_enter_normal_abr_but_audio_only_remains_playable() {
    let trick_play = "#EXT-X-I-FRAME-STREAM-INF:BANDWIDTH=38728,CODECS=\"avc1.640015\",URI=\"https://trick.test/iframe.m3u8\"\n";
    let regular = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=553010,CODECS=\"avc1.640015,mp4a.40.2\",AUDIO=\"aac\"\nlow.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=3289288,CODECS=\"avc1.640020,mp4a.40.2\",AUDIO=\"aac\"\nhigh.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=64078,CODECS=\"mp4a.40.2\",AUDIO=\"aac\"\naudio.m3u8\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aac\",LANGUAGE=\"en\",URI=\"en.m3u8\"\n";
    let result = project("hls", &format!("{regular}{trick_play}"), json!({}));
    assert_eq!(result["ok"], true, "{result}");
    let output = result["value"]["manifest"].as_str().unwrap();
    assert!(!output.contains("I-FRAME") && !output.contains("trick.test"));
    for expected in ["low.m3u8", "high.m3u8", "audio.m3u8", "en.m3u8"] {
        assert!(output.contains(expected), "{output}");
    }
    assert!(
        !result["value"]["origins"]
            .to_string()
            .contains("trick.test")
    );
    let only_trick_play = project("hls", &format!("#EXTM3U\n{trick_play}"), json!({}));
    assert_eq!(only_trick_play["error"]["code"], "unsupported");
    let audio_only = project(
        "hls",
        "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=64078,CODECS=\"mp4a.40.2\"\naudio.m3u8\n",
        json!({}),
    );
    assert_eq!(audio_only["ok"], true, "{audio_only}");
    assert!(
        audio_only["value"]["manifest"]
            .as_str()
            .unwrap()
            .contains("audio.m3u8")
    );
}

#[test]
fn hls_excludes_ec3_variants_but_keeps_aac_languages_subtitles_and_abr() {
    let text = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=100,CODECS=\"avc1.64001e,mp4a.40.2\",AUDIO=\"aac\",SUBTITLES=\"text\"\nlow.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=300,CODECS=\"avc1.64001e,mp4a.40.2\",AUDIO=\"aac\"\nhigh.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=200,CODECS=\"avc1.64001e,ec-3\",AUDIO=\"ec3\"\nunsupported.m3u8\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aac\",LANGUAGE=\"en\",URI=\"https://audio.test/en.m3u8?x=a,b\"\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aac\",LANGUAGE=\"fr\",URI=\"fr.m3u8\"\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"ec3\",URI=\"bad.m3u8\"\n#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"text\",URI=\"captions.m3u8\"\n";
    let result = project(
        "hls",
        text,
        json!({"https://audio.test": "http://127.0.0.1:1234"}),
    );
    assert_eq!(result["ok"], true, "{result}");
    let output = result["value"]["manifest"].as_str().unwrap();
    assert!(!output.contains("ec3") && !output.contains("unsupported.m3u8"));
    for expected in [
        "low.m3u8",
        "high.m3u8",
        "LANGUAGE=\"fr\"",
        "captions.m3u8",
        "http://127.0.0.1:1234/en.m3u8?x=a,b",
    ] {
        assert!(output.contains(expected), "{output}");
    }
    assert_eq!(
        result["value"]["excludedCodecs"].as_array().unwrap().len(),
        1
    );
}

#[test]
fn dash_filters_webm_and_preserves_text_languages_namespaces_and_template_urls() {
    let text = r#"<MPD xmlns="urn:mpeg:dash:schema:mpd:2011"><Period><AdaptationSet contentType="video"><Representation id="avc" codecs="avc1.4d400c" mimeType="video/mp4"><SegmentTemplate media="https://cdn.test/v/$Number$.m4s?a=1&amp;b=2" initialization="../init.mp4"/></Representation><Representation id="vp9" codecs="vp09.00" mimeType="video/webm"/></AdaptationSet><AdaptationSet contentType="audio" lang="en" mimeType="audio/mp4" codecs="mp4a.40.2"><Representation id="en"/></AdaptationSet><AdaptationSet contentType="audio" lang="de" mimeType="audio/mp4" codecs="mp4a.40.2"><Representation id="de"/></AdaptationSet><AdaptationSet contentType="text" lang="en"><Role schemeIdUri="https://cdn.test/metadata"/><Representation codecs="wvtt" mimeType="application/mp4"><BaseURL>//cdn.test/text.mp4?x=1&amp;y=2</BaseURL></Representation></AdaptationSet></Period></MPD>"#;
    let result = project(
        "dash",
        text,
        json!({"https://cdn.test": "http://127.0.0.1:3210"}),
    );
    assert_eq!(result["ok"], true, "{result}");
    let output = result["value"]["manifest"].as_str().unwrap();
    assert!(!output.contains("vp09") && !output.contains("webm"));
    for expected in [
        "urn:mpeg:dash:schema:mpd:2011",
        "lang=\"en\"",
        "lang=\"de\"",
        "wvtt",
        "http://127.0.0.1:3210/v/$Number$.m4s?a=1&amp;b=2",
        "schemeIdUri=\"https://cdn.test/metadata\"",
        "http://127.0.0.1:3210/text.mp4?x=1&amp;y=2",
    ] {
        assert!(output.contains(expected), "{output}");
    }
}

#[test]
fn protected_empty_codec_profile_and_tls_downgrade_fail_without_echoing_input() {
    for text in [
        "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"secret\"\n",
        "#EXTM3U\n#EXT-X-STREAM-INF:CODECS=\"ec-3\"\nsecret\n",
        "#EXTM3U\nhttp://secret.test/segment\n",
    ] {
        let result = project("hls", text, json!({}));
        assert_eq!(result["ok"], false);
        assert!(!result.to_string().contains("secret"));
    }
    assert_eq!(
        project(
            "dash",
            "<MPD><Period><ContentProtection/></Period></MPD>",
            json!({})
        )["ok"],
        false
    );
    assert_eq!(
        project(
            "hls",
            "#EXTM3U\n",
            json!({"https://media.test":"http://0.0.0.0:1234"})
        )["error"]["code"],
        "invalid_argument"
    );
}

#[test]
fn capability_urls_preserve_parent_root_and_nested_dash_base_resolution() {
    let routes = json!({"https://media.test":"http://127.0.0.1:1234/capability"});
    let hls = project(
        "hls",
        "#EXTM3U\n../../../root.ts?key=a%2Fb\n/root.ts\n",
        routes.clone(),
    );
    assert_eq!(hls["ok"], true, "{hls}");
    assert!(
        hls["value"]["manifest"]
            .as_str()
            .unwrap()
            .contains("http://127.0.0.1:1234/capability/root.ts?key=a%2Fb")
    );
    let dash = project(
        "dash",
        r#"<MPD><BaseURL>/media/</BaseURL><Period><AdaptationSet contentType="video"><BaseURL>v/</BaseURL><Representation codecs="avc1.4d401e" mimeType="video/mp4"><SegmentTemplate media="../chunks/$Number%05d$.m4s?x=a%2Fb" initialization="/init.mp4"/></Representation></AdaptationSet></Period></MPD>"#,
        routes,
    );
    assert_eq!(dash["ok"], true, "{dash}");
    let text = dash["value"]["manifest"].as_str().unwrap();
    assert!(
        text.contains("http://127.0.0.1:1234/capability/media/chunks/$Number%05d$.m4s?x=a%2Fb"),
        "{text}"
    );
    assert!(text.contains("http://127.0.0.1:1234/capability/init.mp4"));
}
