use player_download::{
    DownloadContentFormat, DownloadPlanner, DownloadPlanningClient, DownloadProfile,
    DownloadSource, PlayerResult,
};
use player_model::MediaSource;
use std::cell::RefCell;
use std::path::PathBuf;

struct RecordingClient {
    manifest_uri: String,
    manifest: String,
    requested_uris: RefCell<Vec<String>>,
}

impl DownloadPlanningClient for RecordingClient {
    fn fetch_text(&self, uri: &str) -> PlayerResult<String> {
        assert_eq!(uri, self.manifest_uri, "only the supplied VOD is fetched");
        Ok(self.manifest.clone())
    }

    fn content_length(&self, uri: &str) -> PlayerResult<Option<u64>> {
        self.requested_uris.borrow_mut().push(uri.to_owned());
        Ok(Some(16))
    }
}

fn assert_segment_resolution(manifest_uri: &str, reference: &str, expected_uri: &str) {
    let planner = DownloadPlanner::new(RecordingClient {
        manifest_uri: manifest_uri.to_owned(),
        manifest: format!(
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\n{reference}\n#EXT-X-ENDLIST\n"
        ),
        requested_uris: RefCell::new(Vec::new()),
    });
    let source = DownloadSource::new(
        MediaSource::new(manifest_uri),
        DownloadContentFormat::HlsSegments,
    );
    let index = planner
        .plan(&source, &DownloadProfile::default())
        .expect("one finite VOD segment must be plannable");

    assert_eq!(index.segments.len(), 1);
    assert_eq!(index.total_size_bytes, Some(16));
    let requested_uris = planner.client().requested_uris.borrow();
    assert_eq!(
        requested_uris.as_slice(),
        [expected_uri.to_owned()],
        "Content-Length must target the resolved URI; returned segment URI: {:?}",
        index.segments[0].uri
    );
    assert_eq!(
        index.segments[0].uri, expected_uri,
        "the index must retain the same resolved URI that was probed"
    );
}

#[test]
fn hls_https_network_path_changes_authority() {
    assert_segment_resolution(
        "https://origin.test/video/main.m3u8",
        "//cdn.test/seg.ts",
        "https://cdn.test/seg.ts",
    );
}

#[test]
fn hls_https_network_path_preserves_authority_port_and_query() {
    assert_segment_resolution(
        "https://origin.test:8443/video/main.m3u8?manifest=one",
        "//cdn.test:9443/segments/seg.ts?token=https",
        "https://cdn.test:9443/segments/seg.ts?token=https",
    );
}

#[test]
fn hls_http_network_path_preserves_scheme_authority_port_and_query() {
    assert_segment_resolution(
        "http://origin.test:8080/video/main.m3u8?manifest=one",
        "//cdn.test:9080/segments/seg.ts?token=http",
        "http://cdn.test:9080/segments/seg.ts?token=http",
    );
}

#[test]
fn hls_existing_reference_forms_are_preserved() {
    let manifest_uri = "https://origin.test:8443/video/main.m3u8?manifest=one";
    for (reference, expected_uri) in [
        (
            "seg.ts?token=relative",
            "https://origin.test:8443/video/seg.ts?token=relative",
        ),
        (
            "https://cdn.test:9443/seg.ts?token=full",
            "https://cdn.test:9443/seg.ts?token=full",
        ),
        (
            "http://cdn.test:9080/seg.ts?token=full",
            "http://cdn.test:9080/seg.ts?token=full",
        ),
        (
            "/segments/seg.ts?token=root",
            "https://origin.test:8443/segments/seg.ts?token=root",
        ),
    ] {
        assert_segment_resolution(manifest_uri, reference, expected_uri);
    }

    let local_manifest = PathBuf::from("fixtures").join("video").join("main.m3u8");
    let local_segment = PathBuf::from("fixtures").join("video").join("seg.ts");
    assert_segment_resolution(
        local_manifest.to_string_lossy().as_ref(),
        "seg.ts",
        local_segment.to_string_lossy().as_ref(),
    );
}

#[test]
fn hls_https_network_path_with_full_url_query_inherits_manifest_scheme() {
    assert_segment_resolution(
        "https://origin.test/video/main.m3u8",
        "//cdn.test:9443/seg.ts?next=https://auth.test/a",
        "https://cdn.test:9443/seg.ts?next=https://auth.test/a",
    );
}

#[test]
fn hls_http_network_path_with_full_url_query_inherits_manifest_scheme() {
    assert_segment_resolution(
        "http://origin.test:8080/video/main.m3u8",
        "//cdn.test:9080/seg.ts?next=https://auth.test/a",
        "http://cdn.test:9080/seg.ts?next=https://auth.test/a",
    );
}

#[test]
fn hls_local_network_path_reference_behavior_is_preserved() {
    let local_manifest = PathBuf::from("fixtures").join("video").join("main.m3u8");
    let reference = "//cdn.test/seg.ts";
    let local_segment = PathBuf::from(reference);
    assert_segment_resolution(
        local_manifest.to_string_lossy().as_ref(),
        reference,
        local_segment.to_string_lossy().as_ref(),
    );

    let query_reference = "//cdn.test/seg.ts?next=https://auth.test/a";
    assert_segment_resolution(
        local_manifest.to_string_lossy().as_ref(),
        query_reference,
        query_reference,
    );
}

#[test]
fn hls_https_relative_reference_with_url_query_keeps_manifest_directory() {
    assert_segment_resolution(
        "https://origin.test:8443/video/main.m3u8?manifest=one",
        "seg.ts?next=https://auth.test/a",
        "https://origin.test:8443/video/seg.ts?next=https://auth.test/a",
    );
}

#[test]
fn hls_http_relative_reference_with_url_query_keeps_manifest_directory() {
    assert_segment_resolution(
        "http://origin.test:8080/video/main.m3u8?manifest=one",
        "seg.ts?next=http://auth.test/a",
        "http://origin.test:8080/video/seg.ts?next=http://auth.test/a",
    );
}

#[test]
fn hls_root_relative_reference_with_url_query_keeps_manifest_authority() {
    for (manifest_uri, expected_uri) in [
        (
            "https://origin.test:8443/video/main.m3u8",
            "https://origin.test:8443/segments/seg.ts?next=https://auth.test/a",
        ),
        (
            "http://origin.test:8080/video/main.m3u8",
            "http://origin.test:8080/segments/seg.ts?next=https://auth.test/a",
        ),
    ] {
        assert_segment_resolution(
            manifest_uri,
            "/segments/seg.ts?next=https://auth.test/a",
            expected_uri,
        );
    }
}

#[test]
fn hls_leading_absolute_scheme_data_and_local_path_forms_are_preserved() {
    for reference in [
        "https://cdn.test:9443/seg.ts?next=http://auth.test/a",
        "http://cdn.test:9080/seg.ts?next=https://auth.test/a",
        "data:application/octet-stream,segment",
    ] {
        assert_segment_resolution("https://origin.test/video/main.m3u8", reference, reference);
    }

    let local_manifest = PathBuf::from("fixtures").join("video").join("main.m3u8");
    let local_segment = PathBuf::from("fixtures").join("video").join("seg.ts");
    assert_segment_resolution(
        local_manifest.to_string_lossy().as_ref(),
        "seg.ts",
        local_segment.to_string_lossy().as_ref(),
    );
    assert_segment_resolution(
        local_manifest.to_string_lossy().as_ref(),
        "/segments/seg.ts",
        "/segments/seg.ts",
    );
    let data_reference = "data:application/octet-stream,segment";
    assert_segment_resolution(
        local_manifest.to_string_lossy().as_ref(),
        data_reference,
        data_reference,
    );
}
