use crate::{
    dash::{ByteRange, DashSegmentBase},
    error::{DashHlsError, DashHlsResult},
    hls::{
        byte_range_attr, ensure_line_value,
        model::{HlsMediaInput, HlsMediaSegment},
        quoted_attr,
    },
    mp4::SidxBox,
};

pub fn build_hls_media_playlist(input: &HlsMediaInput) -> DashHlsResult<String> {
    ensure_line_value(&input.uri, "media URI")?;
    let init_range = byte_range_attr(&input.initialization, "initialization")?;
    if input.segments.is_empty() {
        return Err(DashHlsError::InvalidHlsInput(
            "media playlist must contain at least one segment".to_owned(),
        ));
    }

    let target_duration = target_duration(&input.segments)?;
    let mut output = format!(
        concat!(
            "#EXTM3U\n",
            "#EXT-X-VERSION:7\n",
            "#EXT-X-TARGETDURATION:{}\n",
            "#EXT-X-MEDIA-SEQUENCE:0\n",
        ),
        target_duration
    );
    if input.independent_segments {
        output.push_str("#EXT-X-INDEPENDENT-SEGMENTS\n");
    }
    output.push_str("#EXT-X-PLAYLIST-TYPE:VOD\n");
    output.push_str(&format!(
        "#EXT-X-MAP:URI={},BYTERANGE=\"{}\"\n",
        quoted_attr(&input.uri, "media URI")?,
        init_range
    ));

    for segment in &input.segments {
        if !segment.duration_seconds.is_finite() || segment.duration_seconds <= 0.0 {
            return Err(DashHlsError::InvalidHlsInput(
                "segment duration must be finite and positive".to_owned(),
            ));
        }
        output.push_str(&format!("#EXTINF:{:.3},\n", segment.duration_seconds));
        output.push_str(&format!(
            "#EXT-X-BYTERANGE:{}\n",
            byte_range_attr(&segment.byte_range, "media segment")?
        ));
        output.push_str(&input.uri);
        output.push('\n');
    }
    output.push_str("#EXT-X-ENDLIST\n");

    Ok(output)
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

fn target_duration(segments: &[HlsMediaSegment]) -> DashHlsResult<u64> {
    let mut max_duration = 0.0_f64;
    for segment in segments {
        if !segment.duration_seconds.is_finite() || segment.duration_seconds <= 0.0 {
            return Err(DashHlsError::InvalidHlsInput(
                "segment duration must be finite and positive".to_owned(),
            ));
        }
        max_duration = max_duration.max(segment.duration_seconds);
    }
    Ok(max_duration.ceil().max(1.0) as u64)
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
