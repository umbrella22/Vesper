use crate::{
    dash::{DashAdaptationKind, DashAdaptationSet, DashManifest, DashRepresentation},
    error::{DashHlsError, DashHlsResult},
    hls::{
        bool_attr, ensure_line_value,
        model::{HlsAudioRendition, HlsMasterInput, HlsResolution, HlsVariant},
        quoted_attr,
    },
};

pub fn build_hls_master_playlist(input: &HlsMasterInput) -> DashHlsResult<String> {
    if input.variants.is_empty() {
        return Err(DashHlsError::InvalidHlsInput(
            "master playlist must contain at least one variant".to_owned(),
        ));
    }

    let mut output = String::from("#EXTM3U\n#EXT-X-VERSION:7\n");
    if input.independent_segments {
        output.push_str("#EXT-X-INDEPENDENT-SEGMENTS\n");
    }

    for rendition in &input.audio_renditions {
        append_audio_rendition(&mut output, rendition)?;
    }
    for variant in &input.variants {
        append_variant(&mut output, variant)?;
    }

    Ok(output)
}

pub fn build_hls_master_input_from_dash_manifest<F>(
    manifest: &DashManifest,
    media_uri_for_representation: F,
) -> DashHlsResult<HlsMasterInput>
where
    F: Fn(&DashAdaptationSet, &DashRepresentation) -> String,
{
    let period = match manifest.periods.as_slice() {
        [period] => period,
        [] => {
            return Err(DashHlsError::UnsupportedMpd(
                "MPD must contain one Period".to_owned(),
            ));
        }
        _ => {
            return Err(DashHlsError::UnsupportedMpd(
                "multi-period DASH is not supported by the HLS bridge MVP".to_owned(),
            ));
        }
    };

    let mut audio_renditions = Vec::new();
    let mut audio_codecs = Vec::new();
    let mut max_audio_bandwidth = 0_u64;
    for adaptation_set in period
        .adaptation_sets
        .iter()
        .filter(|set| set.kind == DashAdaptationKind::Audio)
    {
        for representation in &adaptation_set.representations {
            require_segment_base(representation)?;
            if !representation.codecs.is_empty() {
                push_unique_codec(&mut audio_codecs, &representation.codecs);
            }
            if let Some(bandwidth) = representation.bandwidth {
                max_audio_bandwidth = max_audio_bandwidth.max(bandwidth);
            }

            audio_renditions.push(HlsAudioRendition {
                group_id: "audio".to_owned(),
                name: rendition_name(adaptation_set, representation, audio_renditions.len()),
                uri: media_uri_for_representation(adaptation_set, representation),
                language: adaptation_set.language.clone(),
                is_default: audio_renditions.is_empty(),
                autoselect: true,
                channels: None,
            });
        }
    }

    let mut variants = Vec::new();
    for adaptation_set in period
        .adaptation_sets
        .iter()
        .filter(|set| set.kind == DashAdaptationKind::Video)
    {
        for representation in &adaptation_set.representations {
            require_segment_base(representation)?;
            let bandwidth = representation.bandwidth.ok_or_else(|| {
                DashHlsError::InvalidHlsInput(format!(
                    "video Representation `{}` is missing bandwidth",
                    representation.id
                ))
            })?;
            let bandwidth = bandwidth.checked_add(max_audio_bandwidth).ok_or_else(|| {
                DashHlsError::InvalidHlsInput("variant BANDWIDTH overflows u64".to_owned())
            })?;
            let resolution = match (representation.width, representation.height) {
                (Some(width), Some(height)) => Some(HlsResolution { width, height }),
                _ => None,
            };
            variants.push(HlsVariant {
                uri: media_uri_for_representation(adaptation_set, representation),
                bandwidth,
                average_bandwidth: None,
                codecs: combined_codecs(&representation.codecs, &audio_codecs),
                resolution,
                frame_rate: representation.frame_rate.clone(),
                audio_group_id: (!audio_renditions.is_empty()).then(|| "audio".to_owned()),
                video_range: None,
            });
        }
    }

    if variants.is_empty() {
        for adaptation_set in period
            .adaptation_sets
            .iter()
            .filter(|set| set.kind == DashAdaptationKind::Audio)
        {
            for representation in &adaptation_set.representations {
                let bandwidth = representation.bandwidth.ok_or_else(|| {
                    DashHlsError::InvalidHlsInput(format!(
                        "audio Representation `{}` is missing bandwidth",
                        representation.id
                    ))
                })?;
                variants.push(HlsVariant {
                    uri: media_uri_for_representation(adaptation_set, representation),
                    bandwidth,
                    average_bandwidth: None,
                    codecs: representation.codecs.clone(),
                    resolution: None,
                    frame_rate: None,
                    audio_group_id: None,
                    video_range: None,
                });
            }
        }
        audio_renditions.clear();
    }

    if variants.is_empty() {
        return Err(DashHlsError::UnsupportedMpd(
            "MPD does not contain supported audio or video representations".to_owned(),
        ));
    }

    Ok(HlsMasterInput {
        variants,
        audio_renditions,
        independent_segments: true,
    })
}

pub fn format_hls_frame_rate(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    let rate = if let Some((numerator, denominator)) = value.split_once('/') {
        let numerator: f64 = numerator.trim().parse().ok()?;
        let denominator: f64 = denominator.trim().parse().ok()?;
        if denominator == 0.0 {
            return None;
        }
        numerator / denominator
    } else {
        value.parse().ok()?
    };

    (rate.is_finite() && rate > 0.0).then(|| format!("{rate:.3}"))
}

fn append_audio_rendition(output: &mut String, rendition: &HlsAudioRendition) -> DashHlsResult<()> {
    let mut attrs = vec![
        "TYPE=AUDIO".to_owned(),
        format!(
            "GROUP-ID={}",
            quoted_attr(&rendition.group_id, "audio GROUP-ID")?
        ),
        format!("NAME={}", quoted_attr(&rendition.name, "audio NAME")?),
        format!("DEFAULT={}", bool_attr(rendition.is_default)),
        format!("AUTOSELECT={}", bool_attr(rendition.autoselect)),
        format!("URI={}", quoted_attr(&rendition.uri, "audio URI")?),
    ];

    if let Some(language) = &rendition.language {
        attrs.push(format!(
            "LANGUAGE={}",
            quoted_attr(language, "audio LANGUAGE")?
        ));
    }
    if let Some(channels) = &rendition.channels {
        attrs.push(format!(
            "CHANNELS={}",
            quoted_attr(channels, "audio CHANNELS")?
        ));
    }

    output.push_str("#EXT-X-MEDIA:");
    output.push_str(&attrs.join(","));
    output.push('\n');
    Ok(())
}

fn append_variant(output: &mut String, variant: &HlsVariant) -> DashHlsResult<()> {
    ensure_line_value(&variant.uri, "variant URI")?;
    if variant.bandwidth == 0 {
        return Err(DashHlsError::InvalidHlsInput(
            "variant BANDWIDTH must be non-zero".to_owned(),
        ));
    }

    let mut attrs = vec![format!("BANDWIDTH={}", variant.bandwidth)];
    if let Some(average_bandwidth) = variant.average_bandwidth {
        if average_bandwidth == 0 {
            return Err(DashHlsError::InvalidHlsInput(
                "variant AVERAGE-BANDWIDTH must be non-zero".to_owned(),
            ));
        }
        attrs.push(format!("AVERAGE-BANDWIDTH={average_bandwidth}"));
    }
    if let Some(resolution) = variant.resolution {
        if resolution.width == 0 || resolution.height == 0 {
            return Err(DashHlsError::InvalidHlsInput(
                "variant RESOLUTION dimensions must be non-zero".to_owned(),
            ));
        }
        attrs.push(format!(
            "RESOLUTION={}x{}",
            resolution.width, resolution.height
        ));
    }
    if let Some(frame_rate) = &variant.frame_rate {
        let frame_rate = format_hls_frame_rate(frame_rate).ok_or_else(|| {
            DashHlsError::InvalidHlsInput("variant FRAME-RATE is invalid".to_owned())
        })?;
        attrs.push(format!("FRAME-RATE={frame_rate}"));
    }
    if !variant.codecs.is_empty() {
        attrs.push(format!(
            "CODECS={}",
            quoted_attr(&variant.codecs, "variant CODECS")?
        ));
    }
    if let Some(audio_group_id) = &variant.audio_group_id {
        attrs.push(format!(
            "AUDIO={}",
            quoted_attr(audio_group_id, "variant AUDIO")?
        ));
    }
    if let Some(video_range) = &variant.video_range {
        attrs.push(format!("VIDEO-RANGE={}", video_range_attr(video_range)?));
    }

    output.push_str("#EXT-X-STREAM-INF:");
    output.push_str(&attrs.join(","));
    output.push('\n');
    output.push_str(&variant.uri);
    output.push('\n');
    Ok(())
}

fn require_segment_base(representation: &DashRepresentation) -> DashHlsResult<()> {
    if representation.segment_base.is_none() {
        return Err(DashHlsError::UnsupportedMpd(format!(
            "Representation `{}` must use SegmentBase for DASH-to-HLS bridge MVP",
            representation.id
        )));
    }
    Ok(())
}

fn rendition_name(
    adaptation_set: &DashAdaptationSet,
    representation: &DashRepresentation,
    index: usize,
) -> String {
    let prefix = adaptation_set
        .language
        .clone()
        .or_else(|| adaptation_set.id.clone())
        .unwrap_or_else(|| format!("audio-{}", index + 1));
    format!("{prefix}-{}", representation.id)
}

fn combined_codecs(primary: &str, extras: &[String]) -> String {
    let mut codecs = Vec::new();
    push_unique_codec(&mut codecs, primary);
    for codec in extras {
        push_unique_codec(&mut codecs, codec);
    }
    codecs.join(",")
}

fn push_unique_codec(codecs: &mut Vec<String>, value: &str) {
    for codec in value
        .split(',')
        .map(str::trim)
        .filter(|codec| !codec.is_empty())
    {
        if !codecs.iter().any(|existing| existing == codec) {
            codecs.push(codec.to_owned());
        }
    }
}

fn video_range_attr(value: &str) -> DashHlsResult<String> {
    ensure_line_value(value, "variant VIDEO-RANGE")?;
    let value = value.trim().to_ascii_uppercase();
    if matches!(value.as_str(), "SDR" | "PQ" | "HLG") {
        Ok(value)
    } else {
        Err(DashHlsError::InvalidHlsInput(
            "variant VIDEO-RANGE must be SDR, PQ, or HLG".to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hls::model::{HlsMasterInput, HlsResolution};

    #[test]
    fn builds_master_playlist_with_audio_group() {
        let input = HlsMasterInput {
            variants: vec![HlsVariant {
                uri: "video/720p.m3u8".to_owned(),
                bandwidth: 800_000,
                average_bandwidth: Some(760_000),
                codecs: "avc1.64001f,mp4a.40.2".to_owned(),
                resolution: Some(HlsResolution {
                    width: 1280,
                    height: 720,
                }),
                frame_rate: Some("30000/1001".to_owned()),
                audio_group_id: Some("audio-main".to_owned()),
                video_range: Some("PQ".to_owned()),
            }],
            audio_renditions: vec![HlsAudioRendition {
                group_id: "audio-main".to_owned(),
                name: "Main".to_owned(),
                uri: "audio/main.m3u8".to_owned(),
                language: Some("ja".to_owned()),
                is_default: true,
                autoselect: true,
                channels: Some("2".to_owned()),
            }],
            independent_segments: true,
        };

        let playlist = build_hls_master_playlist(&input).expect("playlist");

        assert_eq!(
            playlist,
            concat!(
                "#EXTM3U\n",
                "#EXT-X-VERSION:7\n",
                "#EXT-X-INDEPENDENT-SEGMENTS\n",
                "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio-main\",NAME=\"Main\",DEFAULT=YES,AUTOSELECT=YES,URI=\"audio/main.m3u8\",LANGUAGE=\"ja\",CHANNELS=\"2\"\n",
                "#EXT-X-STREAM-INF:BANDWIDTH=800000,AVERAGE-BANDWIDTH=760000,RESOLUTION=1280x720,FRAME-RATE=29.970,CODECS=\"avc1.64001f,mp4a.40.2\",AUDIO=\"audio-main\",VIDEO-RANGE=PQ\n",
                "video/720p.m3u8\n",
            )
        );
    }

    #[test]
    fn derives_master_input_from_single_period_dash_manifest() {
        let manifest = DashManifest {
            duration_ms: Some(1_000),
            min_buffer_time_ms: None,
            periods: vec![crate::dash::DashPeriod {
                id: Some("p0".to_owned()),
                adaptation_sets: vec![
                    DashAdaptationSet {
                        id: Some("video".to_owned()),
                        kind: DashAdaptationKind::Video,
                        mime_type: Some("video/mp4".to_owned()),
                        language: None,
                        representations: vec![DashRepresentation {
                            id: "v1".to_owned(),
                            base_url: "video.m4s".to_owned(),
                            mime_type: "video/mp4".to_owned(),
                            codecs: "avc1.64001f".to_owned(),
                            bandwidth: Some(800_000),
                            width: Some(1280),
                            height: Some(720),
                            frame_rate: Some("30000/1001".to_owned()),
                            audio_sampling_rate: None,
                            segment_base: Some(crate::dash::DashSegmentBase {
                                initialization: crate::dash::ByteRange::new(0, 99),
                                index_range: crate::dash::ByteRange::new(100, 199),
                            }),
                        }],
                    },
                    DashAdaptationSet {
                        id: Some("audio".to_owned()),
                        kind: DashAdaptationKind::Audio,
                        mime_type: Some("audio/mp4".to_owned()),
                        language: Some("ja".to_owned()),
                        representations: vec![DashRepresentation {
                            id: "a1".to_owned(),
                            base_url: "audio.m4s".to_owned(),
                            mime_type: "audio/mp4".to_owned(),
                            codecs: "mp4a.40.2".to_owned(),
                            bandwidth: Some(128_000),
                            width: None,
                            height: None,
                            frame_rate: None,
                            audio_sampling_rate: Some("48000".to_owned()),
                            segment_base: Some(crate::dash::DashSegmentBase {
                                initialization: crate::dash::ByteRange::new(0, 49),
                                index_range: crate::dash::ByteRange::new(50, 99),
                            }),
                        }],
                    },
                ],
            }],
        };

        let input = build_hls_master_input_from_dash_manifest(&manifest, |_, representation| {
            format!("vesper-dash://media/session/{}", representation.id)
        })
        .expect("master input");

        assert_eq!(input.audio_renditions.len(), 1);
        assert_eq!(input.audio_renditions[0].group_id, "audio");
        assert_eq!(input.audio_renditions[0].language.as_deref(), Some("ja"));
        assert_eq!(input.variants.len(), 1);
        assert_eq!(input.variants[0].bandwidth, 928_000);
        assert_eq!(input.variants[0].codecs, "avc1.64001f,mp4a.40.2");
        assert_eq!(input.variants[0].audio_group_id.as_deref(), Some("audio"));
    }

    #[test]
    fn rejects_variant_without_bandwidth() {
        let input = HlsMasterInput {
            variants: vec![HlsVariant {
                uri: "video.m3u8".to_owned(),
                bandwidth: 0,
                average_bandwidth: None,
                codecs: String::new(),
                resolution: None,
                frame_rate: None,
                audio_group_id: None,
                video_range: None,
            }],
            ..HlsMasterInput::default()
        };

        let error = build_hls_master_playlist(&input).expect_err("invalid variant should fail");

        assert!(matches!(error, DashHlsError::InvalidHlsInput(_)));
    }
}
