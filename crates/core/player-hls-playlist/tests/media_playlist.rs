use player_hls_playlist::{
    ByteRange, DurationPrecision, Initialization, MediaPlaylistOptions, MediaSegment,
    PlaylistError, PlaylistKind, write_media_playlist,
};

#[test]
fn rejects_blank_or_tag_like_segment_lines_and_preserves_valid_uris() {
    let options = MediaPlaylistOptions {
        kind: PlaylistKind::Live,
        target_duration: 2,
        media_sequence: 0,
        discontinuity_sequence: None,
        independent_segments: false,
        duration_precision: DurationPrecision::Milliseconds,
    };
    let render = |uri: &str| {
        let segment = MediaSegment {
            duration_seconds: 1.0,
            uri: uri.into(),
            byte_range: None,
            discontinuity: false,
            initialization: None,
        };
        write_media_playlist(options, [segment].into_iter())
    };

    for invalid in [
        "",
        " \t",
        "\u{2003}",
        "#comment",
        " \t#EXT-X-ENDLIST",
        "seg\nnext",
    ] {
        assert_eq!(
            render(invalid),
            Err(PlaylistError::InvalidUri("media segment")),
            "URI {invalid:?} would not be read as a single URI line"
        );
    }
    let uri = "segments/%23a%20b.m4s?part=2&name=a%2Fb#fragment";
    assert_eq!(render(uri).expect("playlist").lines().last(), Some(uri));
}

#[test]
fn target_duration_checks_the_serialized_extinf() {
    let options = MediaPlaylistOptions {
        kind: PlaylistKind::Live,
        target_duration: 2,
        media_sequence: 41,
        discontinuity_sequence: None,
        independent_segments: false,
        duration_precision: DurationPrecision::Milliseconds,
    };
    let segment = |duration_seconds| MediaSegment {
        duration_seconds,
        uri: "segment.m4s".into(),
        byte_range: None,
        discontinuity: false,
        initialization: None,
    };
    let accepted = write_media_playlist(options, [segment(2.25)].into_iter()).expect("fits target");
    assert!(accepted.contains("#EXT-X-TARGETDURATION:2\n"));
    assert!(accepted.contains("#EXTINF:2.250,\n"));
    assert!(!accepted.contains("#EXT-X-INDEPENDENT-SEGMENTS"));
    assert!(!accepted.contains("#EXT-X-ENDLIST"));
    assert_eq!(
        write_media_playlist(options, [segment(2.4996)].into_iter()),
        Err(PlaylistError::TargetDurationExceeded { target_duration: 2 })
    );
    let precise = MediaPlaylistOptions {
        duration_precision: DurationPrecision::Microseconds,
        ..options
    };
    assert!(
        write_media_playlist(precise, [segment(2.4996)].into_iter())
            .expect("more precise EXTINF still fits")
            .contains("#EXTINF:2.499600,\n")
    );
}

#[test]
fn duration_must_remain_positive_at_the_selected_precision() {
    assert_eq!(
        DurationPrecision::Milliseconds.validate(0.0004, 2),
        Err(PlaylistError::InvalidSegmentDuration)
    );
    assert_eq!(
        DurationPrecision::Microseconds.validate(0.0000004, 2),
        Err(PlaylistError::InvalidSegmentDuration)
    );
    assert!(DurationPrecision::Microseconds.validate(0.0004, 2).is_ok());
}

#[test]
fn writes_a_new_map_after_a_discontinuity_and_preserves_byte_range_offsets() {
    let options = MediaPlaylistOptions {
        kind: PlaylistKind::Live,
        target_duration: 2,
        media_sequence: 7,
        discontinuity_sequence: Some(3),
        independent_segments: true,
        duration_precision: DurationPrecision::Microseconds,
    };
    let segment = |uri, discontinuity, initialization| MediaSegment {
        duration_seconds: 1.0,
        uri,
        byte_range: Some(ByteRange {
            start: 100,
            end: 199,
        }),
        discontinuity,
        initialization,
    };
    let map = |uri| {
        Some(Initialization {
            uri,
            byte_range: Some(ByteRange { start: 0, end: 99 }),
        })
    };
    let entries = [
        segment("seg-7.m4s".into(), false, map("init-0.mp4".into())),
        segment("seg-8.m4s".into(), true, map("init-8.mp4".into())),
    ];
    assert_eq!(
        write_media_playlist(options, entries.into_iter()).expect("playlist"),
        concat!(
            "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:2\n#EXT-X-MEDIA-SEQUENCE:7\n",
            "#EXT-X-DISCONTINUITY-SEQUENCE:3\n#EXT-X-INDEPENDENT-SEGMENTS\n",
            "#EXT-X-MAP:URI=\"init-0.mp4\",BYTERANGE=\"100@0\"\n",
            "#EXTINF:1.000000,\n#EXT-X-BYTERANGE:100@100\nseg-7.m4s\n",
            "#EXT-X-DISCONTINUITY\n#EXT-X-MAP:URI=\"init-8.mp4\",BYTERANGE=\"100@0\"\n",
            "#EXTINF:1.000000,\n#EXT-X-BYTERANGE:100@100\nseg-8.m4s\n",
        )
    );
}
