use player_model::MediaSource;
use player_platform_desktop::download::{DesktopDownloadController, prepare_download_task};
use player_runtime::{DownloadSegmentRecord, PlayerErrorCode};
use std::path::PathBuf;

#[test]
fn prepared_task_rejects_out_of_root_members_before_registering_the_task() {
    let source = MediaSource::new(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
    let mut controller = DesktopDownloadController::new();
    let prepared = prepare_download_task("path-validation", &source, "fixture").unwrap();
    let mut resource = prepared.clone();
    resource.asset_index.resources[0].relative_path = Some(PathBuf::from(".."));
    let mut segment = prepared.clone();
    segment.asset_index.segments.push(DownloadSegmentRecord {
        segment_id: "test".into(),
        uri: source.uri().into(),
        relative_path: Some(std::env::temp_dir()),
        sequence: None,
        byte_range: None,
        size_bytes: None,
        checksum: None,
    });
    let mut derived = prepared;
    derived.asset_index.resources[0].relative_path = None;
    derived.asset_index.resources[0].uri = "https://media.invalid/../clip.mp4".into();
    for task in [resource, segment, derived] {
        let error = controller
            .create_prepared_task("path-validation".into(), "fixture".into(), task)
            .unwrap_err();
        assert_eq!(error.code(), PlayerErrorCode::InvalidSource);
        assert!(error.message().contains("relative file"));
        assert!(controller.tasks().is_empty());
        assert_eq!(controller.label_for_asset("path-validation"), None);
    }
}
