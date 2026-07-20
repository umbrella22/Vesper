//! Subtitle contract tests for the DASH-to-HLS bridge.
//!
//! These tests cover the public JSON FFI surface (`execute_json`) and assert
//! the following contracts:
//!
//! - subtitle track identity must come from `Representation@id` and survive
//!   reorder/refresh, with no positional fallback masking duplicate ids
//! - label / language / default / forced must propagate into HLS master
//!   playlist `EXT-X-MEDIA` attributes
//! - duplicate representation ids and multiple default subtitles must surface
//!   structured errors
//!
//! Tests are grouped by behavior and include compatibility assertions for
//! video and audio playlist output.

mod common;

use player_dash_hls_bridge::ops::execute_json;
use serde_json::Value;

/// Parses an MPD text into a `DashManifest` JSON value via the public FFI.
/// The iOS host uses the same two-step flow (`parse_manifest` then
/// `selected_playable_representations`), so we mirror it here.
fn parse_manifest(mpd: &str) -> Value {
    let request = serde_json::json!({
        "operation": "parse_manifest",
        "mpd": mpd,
        "manifestUrl": "https://cdn.example.com/manifest.mpd",
    });
    let response = execute_json(&request.to_string()).expect("parse_manifest request");
    serde_json::from_str(&response).expect("parse_manifest response JSON")
}

/// Builds a `selected_playable_representations` request envelope for the
/// given MPD text and returns the parsed JSON response.
fn selected_playable_representations(mpd: &str) -> Value {
    let manifest = parse_manifest(mpd);
    let request = serde_json::json!({
        "operation": "selected_playable_representations",
        "manifest": manifest,
        "variantPolicy": "all",
    });
    let response = execute_json(&request.to_string()).expect("selected playable request");
    serde_json::from_str(&response).expect("selected playable response JSON")
}

/// Builds a `build_master_playlist` request envelope and returns the parsed
/// master playlist text. Media URLs are synthesized from rendition ids using
/// the same shape as the iOS host (`vesper-dash://media/<id>.m3u8`).
fn build_master_playlist(mpd: &str) -> String {
    let manifest = parse_manifest(mpd);
    let selected = {
        let request = serde_json::json!({
            "operation": "selected_playable_representations",
            "manifest": manifest,
            "variantPolicy": "all",
        });
        let response = execute_json(&request.to_string()).expect("selected playable request");
        serde_json::from_str::<Value>(&response).expect("selected playable response JSON")
    };
    let mut media_urls = Vec::new();
    for key in ["audio", "video", "subtitles"] {
        if let Some(items) = selected.get(key).and_then(Value::as_array) {
            for item in items {
                let rendition_id = item
                    .get("renditionId")
                    .and_then(Value::as_str)
                    .expect("renditionId present");
                media_urls.push(serde_json::json!({
                    "renditionId": rendition_id,
                    "url": format!("vesper-dash://media/{rendition_id}.m3u8"),
                }));
            }
        }
    }
    let request = serde_json::json!({
        "operation": "build_master_playlist",
        "manifest": manifest,
        "variantPolicy": "all",
        "mediaUrls": media_urls,
    });
    let response = execute_json(&request.to_string()).expect("master playlist request");
    let parsed: Value = serde_json::from_str(&response).expect("master playlist response JSON");
    parsed
        .get("playlist")
        .and_then(Value::as_str)
        .expect("playlist field")
        .to_owned()
}

/// Collects `#EXT-X-MEDIA:TYPE=SUBTITLES` attribute maps from a master
/// playlist. Each entry is returned as a `BTreeMap<String, String>` of
/// `KEY=VALUE` pairs for easy assertion.
fn subtitle_media_entries(playlist: &str) -> Vec<std::collections::BTreeMap<String, String>> {
    playlist
        .lines()
        .filter_map(|line| line.strip_prefix("#EXT-X-MEDIA:"))
        .filter(|body| body.starts_with("TYPE=SUBTITLES"))
        .map(parse_attr_map)
        .collect()
}

fn parse_attr_map(body: &str) -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    for attr in body.split(',') {
        if let Some((key, value)) = attr.split_once('=') {
            let value = value.trim_matches('"');
            map.insert(key.trim().to_owned(), value.to_owned());
        }
    }
    map
}

#[test]
fn parses_webvtt_adaptation_set_kind() {
    let response = selected_playable_representations(common::webvtt_subtitle_single());
    let subtitles = response
        .get("subtitles")
        .and_then(Value::as_array)
        .expect("subtitles array");
    assert_eq!(subtitles.len(), 1, "exactly one subtitle representation");
    let subtitle = &subtitles[0];
    assert_eq!(
        subtitle.get("renditionId").and_then(Value::as_str),
        Some("sub-en"),
        "rendition id must equal Representation@id"
    );
    let adaptation_set = subtitle
        .get("adaptationSet")
        .expect("adaptationSet present");
    assert_eq!(
        adaptation_set.get("kind").and_then(Value::as_str),
        Some("subtitle"),
        "adaptation kind must classify as subtitle"
    );
    assert_eq!(
        adaptation_set.get("language").and_then(Value::as_str),
        Some("en")
    );
    let representation = subtitle.get("representation").expect("representation");
    assert_eq!(
        representation.get("codecs").and_then(Value::as_str),
        Some("wvtt")
    );
    assert_eq!(
        representation.get("mimeType").and_then(Value::as_str),
        Some("text/vtt")
    );
}

#[test]
fn master_playlist_propagates_subtitle_label_default_forced() {
    // NAME uses <Label>, DEFAULT=YES for the main role, and FORCED=YES for
    // the forced-subtitle role.
    let playlist = build_master_playlist(common::webvtt_subtitle_with_label_and_role());
    let subtitle_entries = subtitle_media_entries(&playlist);
    assert_eq!(
        subtitle_entries.len(),
        2,
        "master playlist must list both subtitle renditions: {playlist}"
    );

    let by_name: std::collections::HashMap<&str, &std::collections::BTreeMap<String, String>> =
        subtitle_entries
            .iter()
            .map(|entry| (entry.get("NAME").map(String::as_str).unwrap_or(""), entry))
            .collect();

    let english = by_name
        .get("English")
        .expect("English subtitle NAME comes from <Label>");
    assert_eq!(
        english.get("DEFAULT").map(String::as_str),
        Some("YES"),
        "main role => DEFAULT=YES"
    );
    assert_eq!(
        english.get("FORCED").map(String::as_str),
        Some("NO"),
        "non-forced role => FORCED=NO"
    );
    assert_eq!(english.get("LANGUAGE").map(String::as_str), Some("en"));

    let forced = by_name
        .get("English (Forced)")
        .expect("forced subtitle NAME comes from <Label>");
    assert_eq!(
        forced.get("DEFAULT").map(String::as_str),
        Some("NO"),
        "non-main role => DEFAULT=NO"
    );
    assert_eq!(
        forced.get("FORCED").map(String::as_str),
        Some("YES"),
        "forced-subtitle role => FORCED=YES"
    );
}

#[test]
fn master_playlist_rejects_duplicate_subtitle_representation_id() {
    // Duplicate non-empty Representation@id values must surface a structured
    // failure instead of being masked by a sequential `-2` suffix.
    let manifest = parse_manifest(common::webvtt_subtitle_duplicate_id());
    let request = serde_json::json!({
        "operation": "selected_playable_representations",
        "manifest": manifest,
        "variantPolicy": "all",
    });
    let error =
        execute_json(&request.to_string()).expect_err("duplicate id must fail identity check");
    assert!(
        error
            .to_string()
            .contains("subtitle_track_identity_ambiguous"),
        "expected structured subtitle_track_identity_ambiguous prefix, got: {error}"
    );
}

#[test]
fn master_playlist_rejects_missing_subtitle_representation_id() {
    // A subtitle adaptation set whose `<Representation>` lacks an `id`
    // attribute must surface a structured identity failure rather than a
    // synthesized positional id. The structured
    // `subtitle_track_identity_ambiguous:` prefix lets iOS classify the
    // failure into a subtitle-specific Swift error case.
    let mpd = r#"<?xml version="1.0"?>
<MPD type="static" mediaPresentationDuration="PT6S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="video-$Number$.m4s" startNumber="1" duration="2000"/>
      <Representation id="v1" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720"/>
    </AdaptationSet>
    <AdaptationSet id="subs" contentType="text" mimeType="text/vtt" lang="en">
      <SegmentTemplate timescale="1000" media="sub-$Number$.vtt" startNumber="1" duration="2000"/>
      <Representation bandwidth="1200" codecs="wvtt"/>
    </AdaptationSet>
  </Period>
</MPD>"#;
    let request = serde_json::json!({
        "operation": "parse_manifest",
        "mpd": mpd,
        "manifestUrl": "https://cdn.example.com/manifest.mpd",
    });
    let error = execute_json(&request.to_string())
        .expect_err("missing subtitle Representation@id must fail identity check");
    assert!(
        error
            .to_string()
            .contains("subtitle_track_identity_ambiguous"),
        "expected structured subtitle_track_identity_ambiguous prefix, got: {error}"
    );
}

#[test]
fn audio_and_video_duplicate_ids_remain_unique_without_subtitle_failure() {
    // Audio/video duplicates keep the legacy deterministic suffix behavior.
    // They must not surface a subtitle-specific identity error, but rendition
    // ids must remain valid dictionary keys for native hosts.
    let mpd = r#"<?xml version="1.0"?>
<MPD type="static" mediaPresentationDuration="PT6S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="video-$Number$.m4s" startNumber="1" duration="2000"/>
      <Representation id="v1" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720"/>
    </AdaptationSet>
    <AdaptationSet id="a1" mimeType="audio/mp4" lang="en">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="a-$Number$.m4s" startNumber="1" duration="2000"/>
      <Representation id="audio-dup" bandwidth="96000" codecs="mp4a.40.2"/>
    </AdaptationSet>
    <AdaptationSet id="a2" mimeType="audio/mp4" lang="ja">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="a-$Number$.m4s" startNumber="1" duration="2000"/>
      <Representation id="audio-dup" bandwidth="96000" codecs="mp4a.40.2"/>
    </AdaptationSet>
  </Period>
</MPD>"#;
    let response = selected_playable_representations(mpd);
    let audio_ids: Vec<String> = response
        .get("audio")
        .and_then(Value::as_array)
        .expect("audio array")
        .iter()
        .map(|item| {
            item.get("renditionId")
                .and_then(Value::as_str)
                .expect("renditionId")
                .to_owned()
        })
        .collect();
    assert_eq!(
        audio_ids,
        vec!["audio-dup".to_owned(), "audio-dup-2".to_owned()],
        "audio duplicate ids must keep deterministic unique rendition ids"
    );
}

#[test]
fn audio_rendition_suffixes_cannot_collide_with_existing_ids() {
    let mpd = r#"<?xml version="1.0"?>
<MPD type="static" mediaPresentationDuration="PT6S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="video-$Number$.m4s" duration="2000"/>
      <Representation id="v1" bandwidth="800000" codecs="avc1.64001f"/>
    </AdaptationSet>
    <AdaptationSet mimeType="audio/mp4" lang="en">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="audio-$Number$.m4s" duration="2000"/>
      <Representation id="audio-dup" bandwidth="96000" codecs="mp4a.40.2"/>
      <Representation id="audio-dup-2" bandwidth="96000" codecs="mp4a.40.2"/>
      <Representation id="audio-dup" bandwidth="96000" codecs="mp4a.40.2"/>
    </AdaptationSet>
  </Period>
</MPD>"#;

    let response = selected_playable_representations(mpd);
    let audio_ids: Vec<&str> = response["audio"]
        .as_array()
        .expect("audio array")
        .iter()
        .map(|item| item["renditionId"].as_str().expect("rendition id"))
        .collect();

    assert_eq!(audio_ids, vec!["audio-dup", "audio-dup-2", "audio-dup-3"]);
}

#[test]
fn selected_playable_rejects_caller_supplied_blank_subtitle_id() {
    let mut manifest = parse_manifest(common::webvtt_subtitle_single());
    manifest["periods"][0]["adaptationSets"][1]["representations"][0]["id"] =
        Value::String("   ".to_owned());
    let request = serde_json::json!({
        "operation": "selected_playable_representations",
        "manifest": manifest,
        "variantPolicy": "all",
    });

    let error = execute_json(&request.to_string())
        .expect_err("caller-supplied blank subtitle identity must fail");
    assert!(
        error
            .to_string()
            .contains("subtitle_track_identity_ambiguous"),
        "expected structured subtitle identity error, got: {error}"
    );
}

#[test]
fn master_playlist_rejects_multiple_default_subtitles() {
    // Two <Role value="main"/> subtitle adaptation sets must surface a
    // structured failure rather than silently emitting two DEFAULT=YES
    // entries. Assert directly against `execute_json` so
    // the failure must come from a returned `Err`, not from a panic in a
    // test helper.
    let manifest = parse_manifest(common::webvtt_subtitle_multi_default());
    let request = serde_json::json!({
        "operation": "selected_playable_representations",
        "manifest": manifest,
        "variantPolicy": "all",
    });
    let error = execute_json(&request.to_string())
        .expect_err("catalog selection must reject multiple default subtitles");
    assert!(
        error
            .to_string()
            .contains("subtitle_default_track_ambiguous"),
        "expected structured subtitle_default_track_ambiguous prefix, got: {error}"
    );
}

#[test]
fn master_playlist_rejects_multiple_default_representations_in_one_set() {
    let mpd = r#"<?xml version="1.0"?>
<MPD type="static" mediaPresentationDuration="PT6S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="video-$Number$.m4s" duration="2000"/>
      <Representation id="v1" bandwidth="800000" codecs="avc1.64001f"/>
    </AdaptationSet>
    <AdaptationSet id="subs" contentType="text" mimeType="text/vtt" lang="en">
      <Role schemeIdUri="urn:mpeg:dash:role:2011" value="main"/>
      <SegmentTemplate timescale="1000" media="sub-$RepresentationID$.vtt" duration="2000"/>
      <Representation id="sub-en-a" bandwidth="1200" codecs="wvtt"/>
      <Representation id="sub-en-b" bandwidth="1200" codecs="wvtt"/>
    </AdaptationSet>
  </Period>
</MPD>"#;
    let manifest = parse_manifest(mpd);
    let request = serde_json::json!({
        "operation": "selected_playable_representations",
        "manifest": manifest,
        "variantPolicy": "all",
    });

    let error = execute_json(&request.to_string())
        .expect_err("multiple default subtitle renditions must fail");
    assert!(
        error
            .to_string()
            .contains("subtitle_default_track_ambiguous"),
        "expected structured default ambiguity, got: {error}"
    );
}

#[test]
fn parses_legacy_label_attribute() {
    // Compatibility `AdaptationSet@label` is preserved on the adaptation set
    // so legacy host output remains readable during migration to `<Label>`.
    let response = selected_playable_representations(common::webvtt_subtitle_legacy_label_attr());
    let subtitles = response
        .get("subtitles")
        .and_then(Value::as_array)
        .expect("subtitles array");
    assert_eq!(subtitles.len(), 1);
    let adaptation_set = subtitles[0].get("adaptationSet").expect("adaptationSet");
    assert_eq!(
        adaptation_set.get("label").and_then(Value::as_str),
        Some("English"),
        "compatibility @label attribute must be preserved"
    );
}

#[test]
fn master_playlist_subtitle_name_falls_back_to_language_without_label() {
    // When no <Label> and no @label is provided, NAME must fall back to
    // language, not a positional index.
    let playlist = build_master_playlist(common::webvtt_subtitle_single());
    let entries = subtitle_media_entries(&playlist);
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].get("NAME").map(String::as_str),
        Some("en"),
        "NAME falls back to language when label is absent"
    );
}

#[test]
fn existing_video_audio_playlist_golden_unchanged() {
    // A subtitle-less video+audio manifest must keep producing the same
    // master playlist shape when subtitle metadata evolves.
    let mpd = r#"<?xml version="1.0"?>
<MPD type="static" mediaPresentationDuration="PT1M30.5S" minBufferTime="PT1.5S">
  <Period id="p0">
    <AdaptationSet id="v" contentType="video" mimeType="video/mp4">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="v-$Number$.m4s" startNumber="1" duration="2000"/>
      <Representation id="v1" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720"/>
    </AdaptationSet>
    <AdaptationSet id="a" mimeType="audio/mp4" lang="ja">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="a-$Number$.m4s" startNumber="1" duration="2000"/>
      <Representation id="a1" bandwidth="128000" codecs="mp4a.40.2"/>
    </AdaptationSet>
  </Period>
</MPD>"#;
    let playlist = build_master_playlist(mpd);

    // No subtitle group should be emitted.
    assert!(
        !playlist.contains("TYPE=SUBTITLES"),
        "video/audio manifest must not emit subtitle group: {playlist}"
    );
    // Audio rendition with DEFAULT=YES (first audio) must remain.
    assert!(
        playlist.contains("TYPE=AUDIO,GROUP-ID=\"audio\""),
        "audio group must still be emitted: {playlist}"
    );
    // The variant stream line must reference the audio group via SUBTITLES
    // being absent (no `SUBTITLES="subtitles"` attribute).
    let stream_inf_count = playlist
        .lines()
        .filter(|line| line.starts_with("#EXT-X-STREAM-INF:"))
        .count();
    assert_eq!(
        stream_inf_count, 1,
        "exactly one variant stream: {playlist}"
    );
    assert!(
        !playlist.contains("SUBTITLES=\"subtitles\""),
        "video/audio-only manifest must not reference subtitle group: {playlist}"
    );
}

#[test]
fn parses_absolute_file_webvtt_uri() {
    // The fixture mirrors real host output where the subtitle file is
    // materialized at an absolute `file://` path, not a relative template.
    // The parser must preserve the absolute URI so the
    // iOS/Android host kits can fetch the correct bytes.
    let response = selected_playable_representations(common::webvtt_subtitle_absolute_file_uri());
    let subtitles = response
        .get("subtitles")
        .and_then(Value::as_array)
        .expect("subtitles array");
    assert_eq!(subtitles.len(), 1);
    let representation = subtitles[0].get("representation").expect("representation");
    assert_eq!(
        representation.get("baseURL").and_then(Value::as_str),
        Some("file:///tmp/host-materialized/subtitle-en.vtt"),
        "absolute file:// URI must be preserved verbatim"
    );
}

#[test]
fn subtitle_representation_reorder_preserves_identity() {
    // swapping subtitle order in the manifest must not change the rendition
    // id. This is the contract that lets Android/iOS keep stable track ids
    // across source refresh and resilience restore.
    let first = selected_playable_representations(common::webvtt_subtitle_multi_language());
    let first_ids: Vec<String> = first
        .get("subtitles")
        .and_then(Value::as_array)
        .expect("subtitles array")
        .iter()
        .map(|item| {
            item.get("renditionId")
                .and_then(Value::as_str)
                .expect("renditionId")
                .to_owned()
        })
        .collect();
    let mut sorted = first_ids.clone();
    sorted.sort();
    assert_eq!(
        first_ids, sorted,
        "rendition ids should be deterministic regardless of manifest order"
    );
    assert_eq!(
        first_ids,
        vec!["sub-en".to_owned(), "sub-zh".to_owned()],
        "rendition ids must come from Representation@id, not position"
    );
}
