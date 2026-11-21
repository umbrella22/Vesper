use player_hls_playlist::{
    DurationPrecision, Initialization, MediaPlaylistOptions, MediaSegment, PlaylistKind,
    write_media_playlist,
};

use crate::{
    dash::{ByteRange, DashSegmentBase},
    error::{DashHlsError, DashHlsResult},
    hls::{
        media_playlist_error,
        model::{HlsMediaInput, HlsMediaSegment},
        playlist_byte_range, vod_target_duration,
    },
    mp4::SidxBox,
};

pub fn build_hls_media_playlist(input: &HlsMediaInput) -> DashHlsResult<String> {
    let segments = input
        .segments
        .iter()
        .enumerate()
        .map(|(index, segment)| MediaSegment {
            duration_seconds: segment.duration_seconds,
            uri: input.uri.as_str().into(),
            byte_range: Some(playlist_byte_range(&segment.byte_range)),
            discontinuity: false,
            initialization: (index == 0).then(|| Initialization {
                uri: input.uri.as_str().into(),
                byte_range: Some(playlist_byte_range(&input.initialization)),
            }),
        });
    write_media_playlist(
        MediaPlaylistOptions {
            kind: PlaylistKind::Vod,
            target_duration: vod_target_duration(input.segments.iter().map(|s| s.duration_seconds)),
            media_sequence: 0,
            discontinuity_sequence: None,
            independent_segments: input.independent_segments,
            duration_precision: DurationPrecision::Milliseconds,
        },
        segments,
    )
    .map_err(media_playlist_error)
}

pub fn build_hls_media_input_from_sidx(
    uri: impl Into<String>,
    segment_base: &DashSegmentBase,
    sidx: &SidxBox,
) -> DashHlsResult<HlsMediaInput> {
    if sidx.timescale == 0 {
        return Err(DashHlsError::InvalidMp4(
            "sidx timescale must be non-zero".to_owned(),
        ));
    }
    if sidx.references.is_empty() {
        return Err(DashHlsError::InvalidMp4(
            "sidx must contain at least one reference".to_owned(),
        ));
    }

    let mut offset = segment_base
        .index_range
        .end
        .checked_add(1)
        .and_then(|value| value.checked_add(sidx.first_offset))
        .ok_or_else(|| {
            DashHlsError::InvalidMp4("sidx first media offset overflows u64".to_owned())
        })?;
    let mut segments = Vec::with_capacity(sidx.references.len());
    for reference in &sidx.references {
        if reference.reference_type != 0 {
            return Err(DashHlsError::UnsupportedMp4(
                "hierarchical sidx references are not supported".to_owned(),
            ));
        }
        if reference.referenced_size == 0 {
            return Err(DashHlsError::InvalidMp4(
                "sidx reference size must be non-zero".to_owned(),
            ));
        }
        if reference.subsegment_duration == 0 {
            return Err(DashHlsError::InvalidMp4(
                "sidx subsegment duration must be non-zero".to_owned(),
            ));
        }

        let end = offset
            .checked_add(u64::from(reference.referenced_size))
            .and_then(|value| value.checked_sub(1))
            .ok_or_else(|| {
                DashHlsError::InvalidMp4("sidx media byte range overflows u64".to_owned())
            })?;
        segments.push(HlsMediaSegment {
            duration_seconds: f64::from(reference.subsegment_duration) / f64::from(sidx.timescale),
            byte_range: ByteRange::new(offset, end),
        });
        offset = end.checked_add(1).ok_or_else(|| {
            DashHlsError::InvalidMp4("sidx next media offset overflows u64".to_owned())
        })?;
    }

    Ok(HlsMediaInput {
        uri: uri.into(),
        initialization: segment_base.initialization.clone(),
        segments,
        independent_segments: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mp4::SidxReference;

    #[test]
    fn builds_media_playlist() {
        let input = HlsMediaInput {
            uri: "https://cdn.example.com/video.m4s".to_owned(),
            initialization: ByteRange::new(0, 99),
            segments: vec![
                HlsMediaSegment {
                    duration_seconds: 2.0,
                    byte_range: ByteRange::new(120, 219),
                },
                HlsMediaSegment {
                    duration_seconds: 3.5,
                    byte_range: ByteRange::new(220, 369),
                },
            ],
            independent_segments: true,
        };

        let playlist = build_hls_media_playlist(&input).expect("playlist");

        assert_eq!(
            playlist,
            concat!(
                "#EXTM3U\n",
                "#EXT-X-VERSION:7\n",
                "#EXT-X-TARGETDURATION:4\n",
                "#EXT-X-MEDIA-SEQUENCE:0\n",
                "#EXT-X-INDEPENDENT-SEGMENTS\n",
                "#EXT-X-PLAYLIST-TYPE:VOD\n",
                "#EXT-X-MAP:URI=\"https://cdn.example.com/video.m4s\",BYTERANGE=\"100@0\"\n",
                "#EXTINF:2.000,\n",
                "#EXT-X-BYTERANGE:100@120\n",
                "https://cdn.example.com/video.m4s\n",
                "#EXTINF:3.500,\n",
                "#EXT-X-BYTERANGE:150@220\n",
                "https://cdn.example.com/video.m4s\n",
                "#EXT-X-ENDLIST\n",
            )
        );
    }

    #[test]
    fn derives_media_input_from_segment_base_and_sidx() {
        let segment_base = DashSegmentBase {
            initialization: ByteRange::new(0, 999),
            index_range: ByteRange::new(1_000, 1_199),
        };
        let sidx = SidxBox {
            timescale: 1_000,
            earliest_presentation_time: 0,
            first_offset: 10,
            references: vec![
                SidxReference {
                    reference_type: 0,
                    referenced_size: 100,
                    subsegment_duration: 2_000,
                    starts_with_sap: true,
                    sap_type: 1,
                    sap_delta_time: 0,
                },
                SidxReference {
                    reference_type: 0,
                    referenced_size: 150,
                    subsegment_duration: 3_500,
                    starts_with_sap: true,
                    sap_type: 1,
                    sap_delta_time: 0,
                },
            ],
        };

        let input = build_hls_media_input_from_sidx("video.m4s", &segment_base, &sidx)
            .expect("media input");

        assert_eq!(input.initialization, ByteRange::new(0, 999));
        assert_eq!(input.segments[0].byte_range, ByteRange::new(1_210, 1_309));
        assert_eq!(input.segments[0].duration_seconds, 2.0);
        assert_eq!(input.segments[1].byte_range, ByteRange::new(1_310, 1_459));
        assert_eq!(input.segments[1].duration_seconds, 3.5);
    }

    #[test]
    fn rejects_hierarchical_sidx_references_for_media_playlist_input() {
        let segment_base = DashSegmentBase {
            initialization: ByteRange::new(0, 9),
            index_range: ByteRange::new(10, 19),
        };
        let sidx = SidxBox {
            timescale: 1_000,
            earliest_presentation_time: 0,
            first_offset: 0,
            references: vec![SidxReference {
                reference_type: 1,
                referenced_size: 10,
                subsegment_duration: 1_000,
                starts_with_sap: true,
                sap_type: 1,
                sap_delta_time: 0,
            }],
        };

        let error = build_hls_media_input_from_sidx("video.m4s", &segment_base, &sidx)
            .expect_err("hierarchical references should fail");

        assert!(matches!(error, DashHlsError::UnsupportedMp4(_)));
    }
}
