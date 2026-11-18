use player_download::{
    DownloadContentFormat, DownloadPlanner, DownloadPlanningClient, DownloadProfile,
    DownloadSource, PlayerError,
};
use player_model::MediaSource;
use std::cell::RefCell;

struct Client {
    text: String,
    probes: RefCell<Vec<String>>,
}
impl DownloadPlanningClient for Client {
    fn fetch_text(&self, _: &str) -> Result<String, PlayerError> {
        Ok(self.text.clone())
    }
    fn content_length(&self, uri: &str) -> Result<Option<u64>, PlayerError> {
        self.probes.borrow_mut().push(uri.to_owned());
        Ok(Some(16))
    }
}
fn planner(media: &str, id: &str) -> DownloadPlanner<Client> {
    DownloadPlanner::new(Client {
        text: format!(
            r#"<MPD type="static" mediaPresentationDuration="PT6S"><Period><AdaptationSet mimeType="video/mp4"><BaseURL>media/</BaseURL><Representation id="{id}" bandwidth="1000"><SegmentTemplate duration="2" startNumber="7" initialization="init-$RepresentationID$.mp4" media="{media}"/></Representation></AdaptationSet></Period></MPD>"#
        ),
        probes: RefCell::new(Vec::new()),
    })
}
fn source() -> DownloadSource {
    DownloadSource::new(
        MediaSource::new("https://cdn.test/asset/manifest.mpd"),
        DownloadContentFormat::DashSegments,
    )
}

#[test]
fn finite_number_templates_preserve_start_padding_base_and_literal_representation_id() {
    let p = planner("$RepresentationID$-$Number%05d$.m4s", "v$Number$");
    let index = p.plan(&source(), &DownloadProfile::default()).unwrap();
    assert_eq!(index.segments.len(), 3);
    assert_eq!(
        index.segments[0].uri,
        "https://cdn.test/asset/media/v$Number$-00007.m4s"
    );
    assert_eq!(index.segments[2].sequence, Some(9));
    let mpd = index
        .resources
        .iter()
        .find_map(|x| x.generated_text.as_deref())
        .unwrap();
    assert!(mpd.contains("startNumber=\"7\""));
    assert!(mpd.contains("seg-$Number%05d$.m4s"));
    assert!(!mpd.contains("https://"));
}

#[test]
fn unsupported_and_malformed_tokens_fail_before_resource_probes() {
    for media in [
        "seg-$Time$.m4s",
        "seg-$Bandwidth$.m4s",
        "seg-$$.m4s",
        "seg-$Unknown$.m4s",
        "seg-$Number.m4s",
        "seg-$Number%05x$.m4s",
        "seg-$Number%0999999999d$.m4s",
        "seg-$Number%00d$.m4s",
    ] {
        let p = planner(media, "v");
        assert!(
            p.plan(&source(), &DownloadProfile::default()).is_err(),
            "{media}"
        );
        assert!(p.client().probes.borrow().is_empty(), "{media}");
    }
}

#[test]
fn output_file_quota_is_checked_before_probes() {
    let p = planner("seg-$Number$.m4s", "v").with_maximum_files(4);
    let error = p.plan(&source(), &DownloadProfile::default()).unwrap_err();
    assert!(error.to_string().contains("capacity"));
    assert!(p.client().probes.borrow().is_empty());
    assert!(
        planner("seg-$Number$.m4s", "v")
            .with_maximum_files(5)
            .plan(&source(), &DownloadProfile::default())
            .is_ok()
    );
}

#[test]
fn finite_timeline_preserves_ticks_offset_repeats_and_local_numbering() {
    let p = DownloadPlanner::new(Client {
        text: r#"<MPD type="static" mediaPresentationDuration="PT6S"><Period><AdaptationSet mimeType="video/mp4"><SegmentTemplate timescale="1000" presentationTimeOffset="9000" startNumber="41" initialization="init-$RepresentationID$.mp4" media="$RepresentationID$-$Time%06d$.m4s"><SegmentTimeline><S t="9000" d="2000" r="-1"/></SegmentTimeline></SegmentTemplate><Representation id="v" bandwidth="1000"/></AdaptationSet></Period></MPD>"#.to_owned(), probes: RefCell::new(Vec::new()) });
    let index = p.plan(&source(), &DownloadProfile::default()).unwrap();
    assert_eq!(
        index
            .segments
            .iter()
            .map(|segment| segment.uri.as_str())
            .collect::<Vec<_>>(),
        vec![
            "https://cdn.test/asset/v-009000.m4s",
            "https://cdn.test/asset/v-011000.m4s",
            "https://cdn.test/asset/v-013000.m4s"
        ]
    );
    assert_eq!(index.segments[2].sequence, Some(43));
    let mpd = index.resources[0].generated_text.as_ref().unwrap();
    assert!(mpd.contains("presentationTimeOffset=\"9000\""));
    assert!(mpd.contains("startNumber=\"41\""));
    assert!(mpd.contains("<S t=\"9000\" d=\"2000\" r=\"-1\"/>"));
    assert!(mpd.contains("seg-$Number%05d$.m4s"));
    assert!(!mpd.contains("$Time"));
}

#[test]
fn unbounded_and_invalid_timeline_fails_before_probes() {
    for timeline in [
        "<S d=\"2\" r=\"-1\"/>",
        "<S d=\"0\"/>",
        "<S d=\"2\" r=\"-2\"/>",
    ] {
        let p = DownloadPlanner::new(Client {
            text: format!(
                r#"<MPD type="static"><Period><AdaptationSet mimeType="video/mp4"><Representation id="v"><SegmentTemplate media="$Time$.m4s"><SegmentTimeline>{timeline}</SegmentTimeline></SegmentTemplate></Representation></AdaptationSet></Period></MPD>"#
            ),
            probes: RefCell::new(Vec::new()),
        });
        assert!(p.plan(&source(), &DownloadProfile::default()).is_err());
        assert!(p.client().probes.borrow().is_empty());
    }
}

#[test]
fn empty_or_duplicate_timelines_never_fall_back_to_fixed_duration() {
    for timeline in [
        "<SegmentTimeline></SegmentTimeline>",
        "<SegmentTimeline/>",
        "<SegmentTimeline><S d=\"2\"/></SegmentTimeline><SegmentTimeline><S d=\"2\"/></SegmentTimeline>",
    ] {
        let p = DownloadPlanner::new(Client {
            text: format!(
                r#"<MPD type="static" mediaPresentationDuration="PT6S"><Period><AdaptationSet mimeType="video/mp4"><Representation id="v"><SegmentTemplate duration="2" media="$Number$.m4s">{timeline}</SegmentTemplate></Representation></AdaptationSet></Period></MPD>"#
            ),
            probes: RefCell::new(Vec::new()),
        });
        assert!(
            p.plan(&source(), &DownloadProfile::default()).is_err(),
            "{timeline}"
        );
        assert!(p.client().probes.borrow().is_empty());
    }
}
