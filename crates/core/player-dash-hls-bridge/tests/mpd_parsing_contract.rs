use player_dash_hls_bridge::{
    dash::{DashAdaptationKind, parse_mpd, parse_mpd_with_base_uri},
    ops::{execute_json, template_segments},
};
use serde_json::{Value, json};

#[test]
fn representation_mime_types_reach_playable_selection() {
    let manifest = parse_mpd(
        r#"<MPD mediaPresentationDuration="PT6S"><Period>
          <AdaptationSet lang="und">
            <Representation id="v" mimeType="video/mp4" codecs="avc1.64001f">
              <SegmentTemplate duration="2" initialization="init.mp4" media="$Number$.m4s"/>
            </Representation>
          </AdaptationSet>
          <AdaptationSet lang="en">
            <Representation id="a" mimeType="audio/mp4" codecs="mp4a.40.2">
              <SegmentTemplate duration="2" initialization="init.mp4" media="$Number$.m4s"/>
            </Representation>
          </AdaptationSet>
        </Period></MPD>"#,
    )
    .expect("representation MIME types are valid DASH metadata");
    assert_eq!(
        manifest.periods[0].adaptation_sets[0].kind,
        DashAdaptationKind::Video
    );
    assert_eq!(
        manifest.periods[0].adaptation_sets[1].kind,
        DashAdaptationKind::Audio
    );
    let selected: Value = serde_json::from_str(
        &execute_json(
            &json!({
                "operation": "selected_playable_representations",
                "manifest": manifest,
                "variantPolicy": "all",
            })
            .to_string(),
        )
        .expect("audio and video remain playable"),
    )
    .expect("selected representation response");
    assert_eq!(selected["video"].as_array().unwrap().len(), 1);
    assert_eq!(selected["audio"].as_array().unwrap().len(), 1);
}

#[test]
fn kind_inference_uses_effective_codecs_and_later_representations() {
    for (codecs, expected) in [
        ("avc1.64001f", DashAdaptationKind::Video),
        ("hvc1.1.6.L93", DashAdaptationKind::Video),
        ("av01.0.05M.08", DashAdaptationKind::Video),
        ("mp4a.40.2", DashAdaptationKind::Audio),
        ("ec-3", DashAdaptationKind::Audio),
        ("opus", DashAdaptationKind::Audio),
        ("wvtt", DashAdaptationKind::Subtitle),
        ("stpp.ttml.im1t", DashAdaptationKind::Subtitle),
    ] {
        let mpd = format!(
            r#"<MPD><Period>
          <AdaptationSet mimeType="application/mp4" codecs="{codecs}">
            <Representation id="track"/>
          </AdaptationSet>
        </Period></MPD>"#
        );
        assert_eq!(
            parse_mpd(&mpd).unwrap().periods[0].adaptation_sets[0].kind,
            expected,
            "{codecs}"
        );
    }
    let manifest = parse_mpd(
        r#"<MPD><Period><AdaptationSet>
      <Representation id="unknown" mimeType="application/octet-stream"/>
      <Representation id="audio" mimeType="audio/mp4"/>
    </AdaptationSet></Period></MPD>"#,
    )
    .unwrap();
    assert_eq!(
        manifest.periods[0].adaptation_sets[0].kind,
        DashAdaptationKind::Audio
    );
}

#[test]
fn inferred_subtitles_do_not_require_an_initialization_template() {
    let manifest = parse_mpd(
        r#"<MPD><Period>
      <AdaptationSet mimeType="application/mp4" codecs="wvtt">
        <SegmentTemplate duration="2" media="sub-$Number$.vtt"/>
        <Representation id="sub-en"/>
      </AdaptationSet>
    </Period></MPD>"#,
    )
    .expect("infer subtitle kind before parsing its template");
    assert_eq!(
        manifest.periods[0].adaptation_sets[0].kind,
        DashAdaptationKind::Subtitle
    );
}

#[test]
fn kind_inference_preserves_explicit_types_and_representation_overrides() {
    let manifest = parse_mpd(
        r#"<MPD><Period>
      <AdaptationSet contentType="video"><Representation id="v" codecs="future"/></AdaptationSet>
      <AdaptationSet mimeType="application/mp4" codecs="stpp">
        <Representation id="a" codecs="mp4a.40.2"/>
      </AdaptationSet>
      <AdaptationSet><Representation id="unknown" codecs="avc1future"/></AdaptationSet>
    </Period></MPD>"#,
    )
    .unwrap();
    let kinds: Vec<_> = manifest.periods[0]
        .adaptation_sets
        .iter()
        .map(|set| set.kind)
        .collect();
    assert_eq!(
        kinds,
        [
            DashAdaptationKind::Video,
            DashAdaptationKind::Audio,
            DashAdaptationKind::Unknown
        ]
    );
}

fn mpd_with_base_url(base_url: &str) -> String {
    format!(
        r#"<MPD><Period><AdaptationSet mimeType="video/mp4">
      <Representation id="v"><BaseURL>{base_url}</BaseURL>
        <SegmentBase indexRange="100-199"><Initialization range="0-99"/></SegmentBase>
      </Representation>
    </AdaptationSet></Period></MPD>"#
    )
}

#[test]
fn base_urls_preserve_uri_components() {
    for (base, reference, expected) in [
        ("https://h/p/m.mpd?acl=/*", "video/", "https://h/p/video/"),
        (
            "https://h/p/m.mpd#section/path",
            "video/",
            "https://h/p/video/",
        ),
        (
            "https://h/p/m.mpd",
            "//cdn2.example.com/v/",
            "https://cdn2.example.com/v/",
        ),
        (
            "https://h/p/m.mpd",
            "video.mp4?src=https://origin/x",
            "https://h/p/video.mp4?src=https://origin/x",
        ),
        (
            "https://h/p/m.mpd",
            "video.mp4?sig=a//b/../c",
            "https://h/p/video.mp4?sig=a//b/../c",
        ),
        (
            "https://h/p/m.mpd",
            "?token=new",
            "https://h/p/m.mpd?token=new",
        ),
        (
            "https://h/p/m.mpd?token=old",
            "#part",
            "https://h/p/m.mpd?token=old#part",
        ),
        (
            "https://h/p/m.mpd",
            "v//clip.mp4",
            "https://h/p/v//clip.mp4",
        ),
        ("https://h/p/m.mpd", "../clip.mp4", "https://h/clip.mp4"),
        ("https://h", "clip.mp4", "https://h/clip.mp4"),
        (
            "file:///tmp/media/m.mpd",
            "../clip.mp4",
            "file:///tmp/clip.mp4",
        ),
    ] {
        let mpd = mpd_with_base_url(&reference.replace('&', "&amp;"));
        let manifest = parse_mpd_with_base_uri(&mpd, Some(base)).unwrap();
        assert_eq!(
            manifest.periods[0].adaptation_sets[0].representations[0].base_url, expected,
            "base={base}, reference={reference}"
        );
    }
}

#[test]
fn base_urls_remain_relative_without_a_manifest_uri() {
    for (base, reference, expected) in [
        ("media/", "..//clip.mp4", ".//clip.mp4"),
        ("media/", "../a:b", "./a:b"),
        ("media//", ".", "media//"),
        ("media///", "..", "media//"),
        ("media//dir/", "..", "media//"),
        ("/media/", "..//clip.mp4", "/.//clip.mp4"),
        ("media/", "clip.mp4", "media/clip.mp4"),
        ("media/", "../clip.mp4?sig=a//b", "clip.mp4?sig=a//b"),
        ("media//", "clip.mp4", "media//clip.mp4"),
        ("../media/", "clip.mp4", "../media/clip.mp4"),
        ("media/m.mpd?old=1", "?new=2", "media/m.mpd?new=2"),
        ("media/m.mpd?old=1", "#part", "media/m.mpd?old=1#part"),
        ("media/", "/clip.mp4", "/clip.mp4"),
        (
            "//cdn.example.com/media/",
            "../clip.mp4?sig=a//b",
            "//cdn.example.com/clip.mp4?sig=a//b",
        ),
        (
            "//cdn.example.com",
            "clip.mp4",
            "//cdn.example.com/clip.mp4",
        ),
    ] {
        let mpd = mpd_with_base_url(reference).replacen(
            "<MPD>",
            &format!("<MPD><BaseURL>{base}</BaseURL>"),
            1,
        );
        let manifest = parse_mpd(&mpd).unwrap();
        assert_eq!(
            manifest.periods[0].adaptation_sets[0].representations[0].base_url,
            expected
        );
    }
}

#[test]
fn nested_relative_base_urls_match_resolution_with_a_manifest_uri() {
    let manifest_uri = "https://h/root/manifest.mpd";
    for (base, period_base) in [
        ("media//", "."),
        ("media/", "../a:b/"),
        ("media/", "..///cdn/"),
        ("/media/", "..//"),
    ] {
        let mpd = mpd_with_base_url("clip.mp4")
            .replacen("<MPD>", &format!("<MPD><BaseURL>{base}</BaseURL>"), 1)
            .replacen(
                "<Period>",
                &format!("<Period><BaseURL>{period_base}</BaseURL>"),
                1,
            );
        let relative = parse_mpd(&mpd).unwrap();
        let absolute = parse_mpd_with_base_uri(&mpd, Some(manifest_uri)).unwrap();
        let relative_url = &relative.periods[0].adaptation_sets[0].representations[0].base_url;
        let resolved = url::Url::parse(manifest_uri)
            .unwrap()
            .join(relative_url)
            .unwrap();
        assert_eq!(
            resolved.as_str(),
            absolute.periods[0].adaptation_sets[0].representations[0].base_url,
            "base={base}, period={period_base}"
        );
    }
}

#[test]
fn date_durations_expand_fixed_segment_templates() {
    for (duration, milliseconds) in [
        ("P1DT2H", 93_600_000),
        ("P0Y0M0DT0H3M30.000S", 210_000),
        ("P1D", 86_400_000),
        ("PT3M30S", 210_000),
    ] {
        let manifest = parse_mpd(&format!(
            r#"<MPD mediaPresentationDuration="{duration}"><Period>
          <AdaptationSet mimeType="video/mp4"><Representation id="v">
            <SegmentTemplate duration="3600" initialization="init.mp4" media="$Number$.m4s"/>
          </Representation></AdaptationSet>
        </Period></MPD>"#
        ))
        .unwrap();
        assert_eq!(manifest.duration_ms, Some(milliseconds));
        let template = manifest.periods[0].adaptation_sets[0].representations[0]
            .segment_template
            .as_ref()
            .unwrap();
        let segments =
            template_segments(Some(manifest.manifest_type), manifest.duration_ms, template)
                .expect("duration reaches segment expansion");
        assert_eq!(
            segments.iter().map(|segment| segment.duration).sum::<f64>(),
            milliseconds as f64 / 1000.0
        );
    }
}

#[test]
fn xml_entities_decode_once_in_resource_urls_and_attributes() {
    let manifest = parse_mpd(&mpd_with_base_url(
        "https://h/v.mp4?a=1&#38;b=2&#x26;c=&amp;lt;",
    ))
    .unwrap();
    assert_eq!(
        manifest.periods[0].adaptation_sets[0].representations[0].base_url,
        "https://h/v.mp4?a=1&b=2&c=&lt;"
    );

    let manifest = parse_mpd(
        r#"<MPD><Period><AdaptationSet mimeType="video/mp4">
      <Label>&#x1F600; &amp;lt; &amp;#38;</Label>
      <Representation id="v"><SegmentTemplate duration="2" initialization="init.mp4"
        media="seg-$Number$.m4s?a=1&#38;b=2"/></Representation>
    </AdaptationSet></Period></MPD>"#,
    )
    .unwrap();
    let adaptation = &manifest.periods[0].adaptation_sets[0];
    assert_eq!(adaptation.label.as_deref(), Some("😀 &lt; &#38;"));
    assert_eq!(
        adaptation.representations[0]
            .segment_template
            .as_ref()
            .unwrap()
            .media,
        "seg-$Number$.m4s?a=1&b=2"
    );
}

#[test]
fn malformed_xml_entities_report_invalid_mpd() {
    for entity in [
        "&#0;",
        "&#xD800;",
        "&#x110000;",
        "&#xnope;",
        "&unknown;",
        "&amp",
    ] {
        let error = parse_mpd(&mpd_with_base_url(&format!(
            "https://h/v.mp4?token={entity}"
        )))
        .expect_err("invalid entities must not become resource URLs");
        assert!(
            matches!(error, player_dash_hls_bridge::DashHlsError::InvalidMpd(_)),
            "{entity}"
        );
    }
}
