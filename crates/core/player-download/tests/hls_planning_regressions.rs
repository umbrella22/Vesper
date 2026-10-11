use player_download::{
    DownloadAssetIndex, DownloadContentFormat, DownloadPlanner, DownloadPlanningClient,
    DownloadProfile, DownloadSource, PlayerError, PlayerErrorCategory, PlayerErrorCode,
};
use player_model::MediaSource;
use std::collections::HashMap;

struct Client(HashMap<String, String>);
impl DownloadPlanningClient for Client {
    fn fetch_text(&self, uri: &str) -> Result<String, PlayerError> {
        self.0.get(uri).cloned().ok_or_else(|| {
            PlayerError::with_category(
                PlayerErrorCode::InvalidSource,
                PlayerErrorCategory::Source,
                format!("unexpected manifest {uri}"),
            )
        })
    }
    fn content_length(&self, _uri: &str) -> Result<Option<u64>, PlayerError> {
        Ok(Some(16))
    }
}
fn plan(
    root: &str,
    fixtures: &[(&str, &str)],
    profile: &DownloadProfile,
) -> Result<DownloadAssetIndex, Box<PlayerError>> {
    DownloadPlanner::new(Client(
        fixtures
            .iter()
            .map(|(u, t)| (u.to_string(), t.to_string()))
            .collect(),
    ))
    .plan(
        &DownloadSource::new(MediaSource::new(root), DownloadContentFormat::HlsSegments),
        profile,
    )
    .map_err(Box::new)
}
const MEDIA: &str = "#EXTM3U\n#EXT-X-ENDLIST\n#EXTINF:4,\nseg.ts\n";

#[test]
fn hls_rewrite_retains_map_switch_and_discontinuity_at_segment_boundary() {
    let index = plan("https://cdn.test/index.m3u8", &[("https://cdn.test/index.m3u8",
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-MAP:URI=\"init-a.mp4\"\n#EXTINF:4,\na.m4s\n#EXT-X-DISCONTINUITY\n#EXT-X-MAP:URI=\"init-b.mp4\"\n#EXTINF:4,\nb.m4s\n#EXT-X-ENDLIST\n")], &DownloadProfile::default()).unwrap();
    let text = index.resources[0].generated_text.as_ref().unwrap();
    let a = text
        .find("#EXT-X-MAP:URI=\"segments/media-init-0.mp4\"")
        .unwrap();
    let first = text.find("segments/media-00000.m4s").unwrap();
    let discontinuity = text.find("#EXT-X-DISCONTINUITY\n").unwrap();
    let b = text
        .find("#EXT-X-MAP:URI=\"segments/media-init-1.mp4\"")
        .unwrap();
    let second = text.find("segments/media-00001.m4s").unwrap();
    assert!(a < first && first < discontinuity && discontinuity < b && b < second);
}

#[test]
fn hls_encrypted_media_and_session_keys_are_explicitly_unsupported() {
    for text in [format!("#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n{MEDIA}"),
        "#EXTM3U\n#EXT-X-SESSION-KEY:METHOD=SAMPLE-AES,URI=\"key.bin\"\n#EXT-X-STREAM-INF:BANDWIDTH=1\nvideo.m3u8\n".to_string()] {
        let error = plan("https://cdn.test/index.m3u8", &[("https://cdn.test/index.m3u8", &text), ("https://cdn.test/video.m3u8", MEDIA)], &DownloadProfile::default()).unwrap_err();
        assert_eq!(error.code(), PlayerErrorCode::Unsupported);
    }
    let plain = format!("#EXT-X-KEY:METHOD=NONE\n{MEDIA}");
    assert!(
        plan(
            "https://cdn.test/index.m3u8",
            &[("https://cdn.test/index.m3u8", &plain)],
            &DownloadProfile::default()
        )
        .is_ok()
    );
}

#[test]
fn hls_audio_selection_stays_in_the_selected_variant_group() {
    let master = "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",LANGUAGE=\"en\",DEFAULT=YES,URI=\"wrong.m3u8\"\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"b\",LANGUAGE=\"ja\",URI=\"right.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=1,AUDIO=\"b\"\nvideo.m3u8\n";
    let profile = DownloadProfile {
        preferred_audio_language: Some("en".to_owned()),
        ..Default::default()
    };
    let index = plan(
        "https://cdn.test/master.m3u8",
        &[
            ("https://cdn.test/master.m3u8", master),
            ("https://cdn.test/video.m3u8", MEDIA),
            ("https://cdn.test/right.m3u8", MEDIA),
        ],
        &profile,
    )
    .unwrap();
    assert!(
        index
            .segments
            .iter()
            .any(|s| s.uri == "https://cdn.test/seg.ts")
    );
    assert_eq!(index.streams.len(), 2);
}

#[test]
fn hls_inband_or_unreferenced_audio_does_not_fetch_an_unrelated_playlist() {
    for audio_ref in ["", ",AUDIO=\"inband\""] {
        let master = format!(
            "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"external\",DEFAULT=YES,URI=\"wrong.m3u8\"\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"inband\",DEFAULT=YES\n#EXT-X-STREAM-INF:BANDWIDTH=1{audio_ref}\nvideo.m3u8\n"
        );
        let index = plan(
            "https://cdn.test/master.m3u8",
            &[
                ("https://cdn.test/master.m3u8", &master),
                ("https://cdn.test/video.m3u8", MEDIA),
            ],
            &DownloadProfile::default(),
        )
        .unwrap();
        assert_eq!(index.streams.len(), 1);
    }
}

#[test]
fn hls_master_with_one_child_keeps_every_referenced_playlist() {
    let index = plan(
        "https://cdn.test/master.m3u8",
        &[
            (
                "https://cdn.test/master.m3u8",
                "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nvideo.m3u8\n",
            ),
            ("https://cdn.test/video.m3u8", MEDIA),
        ],
        &DownloadProfile::default(),
    )
    .unwrap();
    assert!(
        index.resources[0]
            .generated_text
            .as_ref()
            .unwrap()
            .contains("video.m3u8\n")
    );
    assert!(index.resources.iter().any(|r| {
        r.relative_path
            .as_deref()
            .is_some_and(|p| p == "video.m3u8")
    }));
}
