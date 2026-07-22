use crate::{
    dash::{DashAdaptationKind, DashAdaptationSet, DashManifest, DashRepresentation},
    error::{DashHlsError, DashHlsResult, SubtitleErrorDetails},
    hls::{
        bool_attr, ensure_line_value,
        model::{
            HlsAudioRendition, HlsMasterInput, HlsResolution, HlsSubtitleRendition, HlsVariant,
        },
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
    validate_subtitle_renditions(&input.subtitle_renditions)?;
    let mut subtitle_names_by_group = std::collections::HashMap::<&str, Vec<String>>::new();
    for rendition in &input.subtitle_renditions {
        let names = subtitle_names_by_group
            .entry(rendition.group_id.as_str())
            .or_default();
        let name = disambiguate_rendition_name(&rendition.name, names.iter().map(String::as_str));
        names.push(name.clone());
        let mut emitted = rendition.clone();
        emitted.name = name;
        append_subtitle_rendition(&mut output, &emitted)?;
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

    // Validate the complete subtitle declaration before any representation
    // support filtering or segment-addressing checks can hide an identity or
    // default collision.
    validate_dash_subtitle_declarations(period)?;

    let mut audio_renditions = Vec::new();
    let mut subtitle_renditions = Vec::new();
    let mut audio_codecs = Vec::new();
    let mut max_audio_bandwidth = 0_u64;
    let mut emitted_default_subtitle = false;
    for adaptation_set in period
        .adaptation_sets
        .iter()
        .filter(|set| set.kind == DashAdaptationKind::Audio)
    {
        for representation in &adaptation_set.representations {
            require_segment_addressing(representation)?;
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

    for adaptation_set in period
        .adaptation_sets
        .iter()
        .filter(|set| set.kind == DashAdaptationKind::Subtitle)
    {
        for representation in &adaptation_set.representations {
            require_segment_addressing(representation)?;
            if representation.id.trim().is_empty() {
                return Err(subtitle_identity_error(
                    "subtitle_track_identity_ambiguous",
                    None,
                    "subtitle Representation@id must not be empty",
                ));
            }
            let base_name = adaptation_set
                .label
                .as_deref()
                .or(adaptation_set.language.as_deref())
                .or(adaptation_set.id.as_deref())
                .unwrap_or("subtitles");
            let name = disambiguate_rendition_name(
                base_name,
                subtitle_renditions
                    .iter()
                    .map(|item: &HlsSubtitleRendition| item.name.as_str()),
            );
            let is_default = adaptation_set.is_default && !emitted_default_subtitle;
            emitted_default_subtitle |= is_default;
            subtitle_renditions.push(HlsSubtitleRendition {
                id: representation.id.clone(),
                group_id: "subtitles".to_owned(),
                name,
                uri: media_uri_for_representation(adaptation_set, representation),
                language: adaptation_set.language.clone(),
                is_default,
                autoselect: true,
                is_forced: adaptation_set.is_forced,
            });
        }
    }

    let mut ordered_video = period
        .adaptation_sets
        .iter()
        .filter(|set| set.kind == DashAdaptationKind::Video)
        .flat_map(|adaptation_set| {
            adaptation_set
                .representations
                .iter()
                .map(move |representation| (adaptation_set, representation))
        })
        .enumerate()
        .collect::<Vec<_>>();
    ordered_video.sort_by_key(|(index, (_, representation))| {
        dash_startup_video_sort_key(representation, *index, false)
    });

    let mut variants = Vec::new();
    for (_, (adaptation_set, representation)) in ordered_video {
        require_segment_addressing(representation)?;
        variants.push(hls_variant_from_dash_representation(
            representation,
            media_uri_for_representation(adaptation_set, representation),
            &audio_codecs,
            max_audio_bandwidth,
            (!audio_renditions.is_empty()).then_some("audio"),
            (!subtitle_renditions.is_empty()).then_some("subtitles"),
        )?);
    }

    if variants.is_empty() {
        for adaptation_set in period
            .adaptation_sets
            .iter()
            .filter(|set| set.kind == DashAdaptationKind::Audio)
        {
            for representation in &adaptation_set.representations {
                variants.push(hls_variant_from_dash_representation(
                    representation,
                    media_uri_for_representation(adaptation_set, representation),
                    &[],
                    0,
                    None,
                    (!subtitle_renditions.is_empty()).then_some("subtitles"),
                )?);
            }
        }
        audio_renditions.clear();
    }

    if variants.is_empty() {
        return Err(DashHlsError::UnsupportedMpd(
            "MPD does not contain supported audio or video representations".to_owned(),
        ));
    }

    let subtitle_renditions = if subtitle_renditions.is_empty() {
        subtitle_renditions
    } else {
        validate_subtitle_renditions(&subtitle_renditions)?;
        subtitle_renditions
    };

    Ok(HlsMasterInput {
        variants,
        audio_renditions,
        subtitle_renditions,
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

pub(crate) fn dash_startup_video_sort_key(
    representation: &DashRepresentation,
    index: usize,
    prefer_modern_video_codecs: bool,
) -> (u8, u8, u8, u8, u32, u64, u32, usize) {
    const STARTUP_MAX_HEIGHT: u32 = 720;
    const STARTUP_MAX_BANDWIDTH: u64 = 800_000;

    let codec_rank = startup_codec_rank(&representation.codecs, prefer_modern_video_codecs);
    let exceeds_startup_target = u8::from(
        representation
            .height
            .is_none_or(|height| height > STARTUP_MAX_HEIGHT)
            || representation
                .bandwidth
                .is_none_or(|bandwidth| bandwidth > STARTUP_MAX_BANDWIDTH),
    );
    let missing_bandwidth = u8::from(representation.bandwidth.is_none());
    (
        u8::from(codec_rank == u8::MAX),
        exceeds_startup_target,
        codec_rank,
        missing_bandwidth,
        representation.height.unwrap_or(u32::MAX),
        representation.bandwidth.unwrap_or(u64::MAX),
        representation.width.unwrap_or(u32::MAX),
        index,
    )
}

fn startup_codec_rank(value: &str, prefer_modern_video_codecs: bool) -> u8 {
    let mut best_rank = u8::MAX;
    for codec in value
        .split(',')
        .map(|codec| codec.trim().to_ascii_lowercase())
        .filter(|codec| !codec.is_empty())
    {
        let codec = codec.strip_prefix("video/").unwrap_or(&codec);
        let rank = if prefer_modern_video_codecs {
            if codec.starts_with("vvc1")
                || codec.starts_with("vvi1")
                || codec == "vvc"
                || codec == "h266"
            {
                0
            } else if codec.starts_with("av01") || codec == "av1" {
                1
            } else if codec.starts_with("hvc1")
                || codec.starts_with("hev1")
                || codec == "hevc"
                || codec == "h265"
            {
                2
            } else if codec.starts_with("avc1")
                || codec.starts_with("avc3")
                || codec == "avc"
                || codec == "h264"
            {
                3
            } else {
                u8::MAX
            }
        } else if codec.starts_with("avc1")
            || codec.starts_with("avc3")
            || codec == "avc"
            || codec == "h264"
        {
            0
        } else if codec.starts_with("hvc1")
            || codec.starts_with("hev1")
            || codec == "hevc"
            || codec == "h265"
        {
            1
        } else {
            u8::MAX
        };
        best_rank = best_rank.min(rank);
    }
    best_rank
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

fn append_subtitle_rendition(
    output: &mut String,
    rendition: &HlsSubtitleRendition,
) -> DashHlsResult<()> {
    let mut attrs = vec![
        "TYPE=SUBTITLES".to_owned(),
        format!(
            "GROUP-ID={}",
            quoted_attr(&rendition.group_id, "subtitle GROUP-ID")?
        ),
        format!("NAME={}", quoted_attr(&rendition.name, "subtitle NAME")?),
        format!("DEFAULT={}", bool_attr(rendition.is_default)),
        format!("AUTOSELECT={}", bool_attr(rendition.autoselect)),
        format!("FORCED={}", bool_attr(rendition.is_forced)),
        format!("URI={}", quoted_attr(&rendition.uri, "subtitle URI")?),
    ];
    if let Some(language) = &rendition.language {
        attrs.push(format!(
            "LANGUAGE={}",
            quoted_attr(language, "subtitle LANGUAGE")?
        ));
    }
    output.push_str("#EXT-X-MEDIA:");
    output.push_str(&attrs.join(","));
    output.push('\n');
    Ok(())
}

fn validate_subtitle_renditions(renditions: &[HlsSubtitleRendition]) -> DashHlsResult<()> {
    let mut ids = std::collections::HashSet::new();
    let mut defaults_by_group = std::collections::HashMap::<&str, usize>::new();
    for rendition in renditions {
        if rendition.id.trim().is_empty()
            || rendition.id.contains('\r')
            || rendition.id.contains('\n')
        {
            return Err(subtitle_identity_error(
                "subtitle_track_identity_ambiguous",
                None,
                "subtitle rendition id must be non-blank and single-line",
            ));
        }
        ensure_line_value(&rendition.group_id, "subtitle GROUP-ID")?;
        ensure_line_value(&rendition.name, "subtitle NAME")?;
        ensure_line_value(&rendition.uri, "subtitle URI")?;
        if !ids.insert(&rendition.id) {
            return Err(subtitle_identity_error(
                "subtitle_track_identity_ambiguous",
                Some(rendition.id.clone()),
                format!("duplicate subtitle rendition id `{}`", rendition.id),
            ));
        }
        if rendition.is_default {
            let count = defaults_by_group.entry(&rendition.group_id).or_default();
            *count += 1;
            if *count > 1 {
                return Err(subtitle_identity_error(
                    "subtitle_default_track_ambiguous",
                    None,
                    format!(
                        "subtitle group `{}` has multiple DEFAULT renditions",
                        rendition.group_id
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn validate_dash_subtitle_declarations(period: &crate::dash::DashPeriod) -> DashHlsResult<()> {
    let mut ids = std::collections::HashSet::new();
    let mut default_group_count = 0_u32;
    for adaptation_set in period
        .adaptation_sets
        .iter()
        .filter(|set| set.kind == DashAdaptationKind::Subtitle)
    {
        if adaptation_set.is_default && !adaptation_set.representations.is_empty() {
            default_group_count += 1;
        }
        for representation in &adaptation_set.representations {
            if representation.id.trim().is_empty() {
                return Err(subtitle_identity_error(
                    "subtitle_track_identity_ambiguous",
                    None,
                    "subtitle Representation@id must not be empty",
                ));
            }
            if !ids.insert(representation.id.clone()) {
                return Err(subtitle_identity_error(
                    "subtitle_track_identity_ambiguous",
                    Some(representation.id.clone()),
                    format!(
                        "duplicate subtitle Representation@id `{}`",
                        representation.id
                    ),
                ));
            }
        }
    }
    if default_group_count > 1 {
        return Err(subtitle_identity_error(
            "subtitle_default_track_ambiguous",
            None,
            format!("{default_group_count} default subtitle groups"),
        ));
    }
    Ok(())
}

fn subtitle_identity_error(
    code: &str,
    track_id: Option<String>,
    message: impl Into<String>,
) -> DashHlsError {
    DashHlsError::Subtitle {
        details: SubtitleErrorDetails::new(code, "identity", track_id, false, message),
    }
}

fn disambiguate_rendition_name<'a, I>(base_name: &str, existing: I) -> String
where
    I: Iterator<Item = &'a str> + Clone,
{
    if !existing.clone().any(|name| name == base_name) {
        return base_name.to_owned();
    }
    let mut suffix = 2_u32;
    loop {
        let candidate = format!("{base_name} ({suffix})");
        if !existing.clone().any(|name| name == candidate) {
            return candidate;
        }
        suffix += 1;
    }
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
    if let Some(subtitle_group_id) = &variant.subtitle_group_id {
        attrs.push(format!(
            "SUBTITLES={}",
            quoted_attr(subtitle_group_id, "variant SUBTITLES")?
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

fn require_segment_addressing(representation: &DashRepresentation) -> DashHlsResult<()> {
    if representation.segment_base.is_none() && representation.segment_template.is_none() {
        return Err(DashHlsError::UnsupportedMpd(format!(
            "Representation `{}` must use SegmentBase or SegmentTemplate for DASH-to-HLS bridge",
            representation.id
        )));
    }
    Ok(())
}

pub(crate) fn rendition_name(
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

pub(crate) fn hls_variant_from_dash_representation(
    representation: &DashRepresentation,
    uri: String,
    extra_codecs: &[String],
    extra_bandwidth: u64,
    audio_group: Option<&str>,
    subtitle_group: Option<&str>,
) -> DashHlsResult<HlsVariant> {
    let base_bandwidth = representation.bandwidth.ok_or_else(|| {
        DashHlsError::InvalidHlsInput(format!(
            "Representation `{}` is missing bandwidth",
            representation.id
        ))
    })?;
    let average_bandwidth = base_bandwidth.checked_add(extra_bandwidth).ok_or_else(|| {
        DashHlsError::InvalidHlsInput("HLS AVERAGE-BANDWIDTH overflows u64".to_owned())
    })?;
    let bandwidth = average_bandwidth
        .checked_add(average_bandwidth)
        .ok_or_else(|| DashHlsError::InvalidHlsInput("HLS BANDWIDTH overflows u64".to_owned()))?;
    let resolution = match (representation.width, representation.height) {
        (Some(width), Some(height)) if width > 0 && height > 0 => {
            Some(HlsResolution { width, height })
        }
        _ => None,
    };
    Ok(HlsVariant {
        uri,
        bandwidth,
        average_bandwidth: Some(average_bandwidth),
        codecs: combined_codecs(&representation.codecs, extra_codecs),
        resolution,
        frame_rate: representation.frame_rate.clone(),
        audio_group_id: audio_group.map(str::to_owned),
        subtitle_group_id: subtitle_group.map(str::to_owned),
        video_range: None,
    })
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
    use crate::dash::parse_mpd;
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
                subtitle_group_id: None,
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
            subtitle_renditions: Vec::new(),
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
            manifest_type: crate::dash::DashManifestType::Static,
            duration_ms: Some(1_000),
            min_buffer_time_ms: None,
            minimum_update_period_ms: None,
            time_shift_buffer_depth_ms: None,
            periods: vec![crate::dash::DashPeriod {
                id: Some("p0".to_owned()),
                adaptation_sets: vec![
                    DashAdaptationSet {
                        id: Some("video".to_owned()),
                        kind: DashAdaptationKind::Video,
                        mime_type: Some("video/mp4".to_owned()),
                        language: None,
                        label: None,
                        is_default: false,
                        is_forced: false,
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
                            segment_template: None,
                        }],
                    },
                    DashAdaptationSet {
                        id: Some("audio".to_owned()),
                        kind: DashAdaptationKind::Audio,
                        mime_type: Some("audio/mp4".to_owned()),
                        language: Some("ja".to_owned()),
                        label: None,
                        is_default: false,
                        is_forced: false,
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
                            segment_template: None,
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
        assert_eq!(input.variants[0].bandwidth, 1_856_000);
        assert_eq!(input.variants[0].average_bandwidth, Some(928_000));
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
                subtitle_group_id: None,
                video_range: None,
            }],
            ..HlsMasterInput::default()
        };

        let error = build_hls_master_playlist(&input).expect_err("invalid variant should fail");

        assert!(matches!(error, DashHlsError::InvalidHlsInput(_)));
    }

    #[test]
    fn public_builder_returns_typed_subtitle_identity_errors() {
        let mut input = master_input_with_subtitles(vec![subtitle_rendition("", false)]);
        let missing = build_hls_master_playlist(&input).expect_err("missing id");
        assert_eq!(
            missing
                .subtitle_details()
                .map(|details| details.code.as_str()),
            Some("subtitle_track_identity_ambiguous")
        );

        input.subtitle_renditions = vec![
            subtitle_rendition("sub-en", false),
            subtitle_rendition("sub-en", false),
        ];
        let duplicate = build_hls_master_playlist(&input).expect_err("duplicate id");
        assert_eq!(
            duplicate
                .subtitle_details()
                .and_then(|details| details.track_id.as_deref()),
            Some("sub-en")
        );
    }

    #[test]
    fn public_builder_returns_typed_multiple_default_error() {
        let input = master_input_with_subtitles(vec![
            subtitle_rendition("sub-en", true),
            subtitle_rendition("sub-zh", true),
        ]);

        let error = build_hls_master_playlist(&input).expect_err("multiple defaults");
        assert_eq!(
            error
                .subtitle_details()
                .map(|details| details.code.as_str()),
            Some("subtitle_default_track_ambiguous")
        );
    }

    #[test]
    fn dash_manifest_builder_returns_typed_subtitle_contract_errors() {
        let mut manifest = parse_mpd(
            r#"<?xml version="1.0"?>
<MPD type="static" mediaPresentationDuration="PT6S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="video-$Number$.m4s" duration="2000"/>
      <Representation id="video" bandwidth="800000" codecs="avc1.64001f"/>
    </AdaptationSet>
    <AdaptationSet id="sub-en" mimeType="text/vtt" lang="en">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.vtt" media="sub-$Number$.vtt" duration="2000"/>
      <Representation id="subtitle-en" bandwidth="256"/>
    </AdaptationSet>
    <AdaptationSet id="sub-zh" mimeType="text/vtt" lang="zh">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.vtt" media="sub-$Number$.vtt" duration="2000"/>
      <Representation id="subtitle-zh" bandwidth="256"/>
    </AdaptationSet>
  </Period>
</MPD>"#,
        )
        .expect("manifest");

        manifest.periods[0].adaptation_sets[2].representations[0].id = "subtitle-en".to_owned();
        let duplicate =
            build_hls_master_input_from_dash_manifest(&manifest, |_, representation| {
                format!("vesper-dash://media/session/{}", representation.id)
            })
            .expect_err("duplicate subtitle identities must fail");
        assert_eq!(
            duplicate
                .subtitle_details()
                .map(|details| details.code.as_str()),
            Some("subtitle_track_identity_ambiguous")
        );

        manifest.periods[0].adaptation_sets[2].representations[0].id = "subtitle-zh".to_owned();
        manifest.periods[0].adaptation_sets[1].is_default = true;
        manifest.periods[0].adaptation_sets[2].is_default = true;
        let multiple_defaults =
            build_hls_master_input_from_dash_manifest(&manifest, |_, representation| {
                format!("vesper-dash://media/session/{}", representation.id)
            })
            .expect_err("multiple default subtitles must fail");
        assert_eq!(
            multiple_defaults
                .subtitle_details()
                .map(|details| details.code.as_str()),
            Some("subtitle_default_track_ambiguous")
        );
    }

    #[test]
    fn dash_manifest_builder_validates_subtitles_before_segment_filtering() {
        let manifest = parse_mpd(
            r#"<?xml version="1.0"?>
<MPD type="static" mediaPresentationDuration="PT6S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="video-$Number$.m4s" duration="2000"/>
      <Representation id="video" bandwidth="800000" codecs="avc1.64001f"/>
    </AdaptationSet>
    <AdaptationSet id="sub-en" mimeType="text/vtt" lang="en">
      <Representation id="sub-en" bandwidth="256"/>
      <Representation id="sub-en" bandwidth="128"/>
    </AdaptationSet>
  </Period>
</MPD>"#,
        )
        .expect("manifest");

        let error = build_hls_master_input_from_dash_manifest(&manifest, |_, representation| {
            format!("vesper-dash://media/session/{}", representation.id)
        })
        .expect_err("identity validation must precede segment filtering");
        assert_eq!(
            error
                .subtitle_details()
                .map(|details| details.code.as_str()),
            Some("subtitle_track_identity_ambiguous")
        );
    }

    #[test]
    fn public_builder_disambiguates_duplicate_subtitle_names() {
        let input = master_input_with_subtitles(vec![
            subtitle_rendition("sub-en", false),
            subtitle_rendition("sub-zh", false),
        ]);

        let playlist = build_hls_master_playlist(&input).expect("playlist");
        assert!(playlist.contains("NAME=\"Main\""));
        assert!(playlist.contains("NAME=\"Main (2)\""));
    }

    fn master_input_with_subtitles(
        subtitle_renditions: Vec<HlsSubtitleRendition>,
    ) -> HlsMasterInput {
        HlsMasterInput {
            variants: vec![HlsVariant {
                uri: "video.m3u8".to_owned(),
                bandwidth: 800_000,
                average_bandwidth: None,
                codecs: "avc1.64001f".to_owned(),
                resolution: None,
                frame_rate: None,
                audio_group_id: None,
                subtitle_group_id: Some("subtitles".to_owned()),
                video_range: None,
            }],
            audio_renditions: Vec::new(),
            subtitle_renditions,
            independent_segments: true,
        }
    }

    fn subtitle_rendition(id: &str, is_default: bool) -> HlsSubtitleRendition {
        HlsSubtitleRendition {
            id: id.to_owned(),
            group_id: "subtitles".to_owned(),
            name: "Main".to_owned(),
            uri: format!("{id}.m3u8"),
            language: None,
            is_default,
            autoselect: true,
            is_forced: false,
        }
    }
}
