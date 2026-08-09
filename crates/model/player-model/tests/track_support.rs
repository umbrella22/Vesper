use player_model::{
    MediaTrackCatalog, MediaTrackSupport, MediaTrackSupportReason, MediaTrackSupportSource,
    MediaTrackSupportStatus,
};

#[test]
fn missing_track_support_defaults_to_unknown_without_disabling_selection() {
    let support = MediaTrackSupport::default();

    assert_eq!(support.status, MediaTrackSupportStatus::Unknown);
    assert_eq!(support.reason, MediaTrackSupportReason::PlatformUnknown);
    assert_eq!(support.source, MediaTrackSupportSource::Unavailable);
    assert!(support.can_attempt_explicit_selection());
}

#[test]
fn explicitly_unsupported_tracks_cannot_be_selected() {
    for status in [
        MediaTrackSupportStatus::ExceedsCapabilities,
        MediaTrackSupportStatus::Unsupported,
    ] {
        let support = MediaTrackSupport {
            status,
            ..MediaTrackSupport::default()
        };

        assert!(!support.can_attempt_explicit_selection());
    }
}

#[test]
fn empty_catalog_uses_wire_compatibility_sentinels() {
    let catalog = MediaTrackCatalog::default();

    assert_eq!(catalog.catalog_revision, 0);
    assert_eq!(catalog.playback_path, None);
}
