use player_dash_hls_bridge::dash::{DashManifestType, DashSegmentTemplate};
use player_dash_hls_bridge::ops::template_segments;

fn template(duration: u64, timescale: u64) -> DashSegmentTemplate {
    DashSegmentTemplate {
        timescale,
        duration: Some(duration),
        start_number: 1,
        presentation_time_offset: 0,
        initialization: None,
        media: "chunk-$Number$.m4s".to_owned(),
        timeline: Vec::new(),
    }
}

#[test]
fn fixed_template_preserves_near_integer_tick_identity() {
    let segments = template_segments(
        Some(DashManifestType::Static),
        Some(6_003),
        &template(2_001, 1_000),
    )
    .unwrap();
    assert_eq!(segments.len(), 3);
    assert_eq!(
        segments.iter().map(|s| s.number).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(
        segments
            .iter()
            .map(|s| s.presentation_time)
            .collect::<Vec<_>>(),
        vec![0, 2_001, 4_002]
    );
    assert!(segments.iter().all(|s| (s.duration - 2.001).abs() < 1e-12));
}

#[test]
fn fixed_template_clips_only_the_final_segment() {
    let segments = template_segments(
        Some(DashManifestType::Static),
        Some(6_004),
        &template(2_001, 1_000),
    )
    .unwrap();
    assert_eq!(segments.len(), 4);
    assert!((segments[3].duration - 0.001).abs() < 1e-12);
    assert_eq!(segments[3].presentation_time, 6_003);
}

#[test]
fn fixed_template_rejects_zero_and_checks_number_overflow() {
    assert!(template_segments(Some(DashManifestType::Static), Some(1), &template(0, 1)).is_err());
    let mut value = template(1, 1_000);
    value.start_number = u64::MAX;
    assert!(template_segments(Some(DashManifestType::Static), Some(2), &value).is_err());
}
