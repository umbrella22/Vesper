use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    dash::{
        ByteRange, DashAdaptationKind, DashAdaptationSet, DashManifest, DashManifestType,
        DashRepresentation, DashSegmentBase, DashSegmentTemplate, parse_mpd_with_base_uri,
    },
    error::{DashHlsError, DashHlsResult},
    mp4::{SidxBox, parse_sidx, remove_top_level_sidx_boxes},
};

#[derive(Debug, Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
enum BridgeRequest {
    ParseManifest {
        mpd: String,
        manifest_url: String,
    },
    ParseSidx {
        data: Vec<u8>,
    },
    RemoveTopLevelSidx {
        data: Vec<u8>,
    },
    SelectedPlayableRepresentations {
        manifest: DashManifest,
        variant_policy: VariantPolicy,
        #[serde(default)]
        video_decode_capabilities: Option<Vec<VideoDecodeCapability>>,
    },
    BuildMasterPlaylist {
        manifest: DashManifest,
        variant_policy: VariantPolicy,
        media_urls: Vec<RenditionUrl>,
        #[serde(default)]
        video_decode_capabilities: Option<Vec<VideoDecodeCapability>>,
    },
    MediaSegments {
        segment_base: DashSegmentBase,
        sidx: SidxBox,
    },
    TemplateSegments {
        manifest_type: Option<DashManifestType>,
        duration_ms: Option<u64>,
        segment_template: DashSegmentTemplate,
    },
    BuildExternalMediaPlaylist {
        map: Option<HlsMap>,
        segments: Vec<HlsSegment>,
        playlist_kind: HlsPlaylistKind,
        media_sequence: Option<u64>,
    },
    ExpandTemplate {
        template: String,
        representation: Box<DashRepresentation>,
        number: Option<u64>,
        time: Option<u64>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VariantPolicy {
    All,
    StartupSingleVariant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VideoCodecFamily {
    Vvc,
    Av1,
    Hevc,
    Avc,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoDecodeCapability {
    pub rendition_id: String,
    pub codec_family: VideoCodecFamily,
    pub hardware_decode_supported: bool,
    #[serde(default)]
    pub decoder_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayableRepresentation {
    pub rendition_id: String,
    pub adaptation_set: DashAdaptationSet,
    pub representation: DashRepresentation,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectedPlayableResponse {
    audio: Vec<PlayableRepresentation>,
    video: Vec<PlayableRepresentation>,
    subtitles: Vec<PlayableRepresentation>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenditionUrl {
    rendition_id: String,
    url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MasterPlaylistResponse {
    playlist: String,
    selected: SelectedPlayableResponse,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaSegment {
    pub duration: f64,
    pub range: ByteRange,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateSegment {
    pub duration: f64,
    pub number: u64,
    pub time: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HlsMap {
    uri: String,
    byte_range: Option<ByteRange>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HlsSegment {
    duration: f64,
    uri: String,
    byte_range: Option<ByteRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum HlsPlaylistKind {
    Vod,
    Live,
}

/// Executes one JSON-encoded bridge operation and returns a JSON-encoded result.
///
/// The operation schema is intentionally coarse-grained so Swift, macOS, and future
/// platform hosts can reuse the same pure DASH logic without adding many FFI getters.
pub fn execute_json(request_json: &str) -> DashHlsResult<String> {
    let request: BridgeRequest = serde_json::from_str(request_json)
        .map_err(|error| DashHlsError::InvalidHlsInput(format!("invalid request JSON: {error}")))?;

    match request {
        BridgeRequest::ParseManifest { mpd, manifest_url } => {
            to_json(&parse_mpd_with_base_uri(&mpd, Some(&manifest_url))?)
        }
        BridgeRequest::ParseSidx { data } => to_json(&parse_sidx(&data)?),
        BridgeRequest::RemoveTopLevelSidx { data } => to_json(&remove_top_level_sidx_boxes(&data)?),
        BridgeRequest::SelectedPlayableRepresentations {
            manifest,
            variant_policy,
            video_decode_capabilities,
        } => to_json(&selected_playable_response(
            &manifest,
            variant_policy,
            video_decode_capabilities.as_deref(),
        )?),
        BridgeRequest::BuildMasterPlaylist {
            manifest,
            variant_policy,
            media_urls,
            video_decode_capabilities,
        } => {
            let prefer_modern_video_codecs = video_decode_capabilities.is_some();
            let selected = selected_playable_response(
                &manifest,
                variant_policy,
                video_decode_capabilities.as_deref(),
            )?;
            let media_urls = media_url_map(media_urls);
            let playlist =
                build_master_playlist(&selected, &media_urls, prefer_modern_video_codecs)?;
            to_json(&MasterPlaylistResponse { playlist, selected })
        }
        BridgeRequest::MediaSegments { segment_base, sidx } => {
            to_json(&media_segments(&segment_base, &sidx)?)
        }
        BridgeRequest::TemplateSegments {
            manifest_type,
            duration_ms,
            segment_template,
        } => to_json(&template_segments(
            manifest_type,
            duration_ms,
            &segment_template,
        )?),
        BridgeRequest::BuildExternalMediaPlaylist {
            map,
            segments,
            playlist_kind,
            media_sequence,
        } => to_json(&build_external_media_playlist(
            map.as_ref(),
            &segments,
            playlist_kind,
            media_sequence,
        )?),
        BridgeRequest::ExpandTemplate {
            template,
            representation,
            number,
            time,
        } => to_json(&expand_template(&template, &representation, number, time)?),
    }
}

fn to_json<T: Serialize>(value: &T) -> DashHlsResult<String> {
    serde_json::to_string(value)
        .map_err(|error| DashHlsError::InvalidHlsInput(format!("failed to encode JSON: {error}")))
}

fn selected_playable_response(
    manifest: &DashManifest,
    variant_policy: VariantPolicy,
    video_decode_capabilities: Option<&[VideoDecodeCapability]>,
) -> DashHlsResult<SelectedPlayableResponse> {
    let mut audio = Vec::new();
    let mut video = Vec::new();
    let mut subtitles = Vec::new();
    for item in playable_representations(manifest)? {
        match item.adaptation_set.kind {
            DashAdaptationKind::Audio => audio.push(item),
            DashAdaptationKind::Video => video.push(item),
            DashAdaptationKind::Subtitle => subtitles.push(item),
            DashAdaptationKind::Unknown => {}
        }
    }
    let source_has_video = !video.is_empty();
    let prefer_modern_video_codecs = video_decode_capabilities.is_some();
    video = filter_hardware_decodable_video(video, video_decode_capabilities)?;
    if source_has_video && video.is_empty() {
        return Err(DashHlsError::UnsupportedMpd(
            "MPD has video representations, but none are hardware-decodable on this device"
                .to_owned(),
        ));
    }

    if audio.is_empty() && video.is_empty() {
        return Err(DashHlsError::UnsupportedMpd(
            "MPD has no SegmentBase or SegmentTemplate audio/video representations".to_owned(),
        ));
    }

    if variant_policy == VariantPolicy::StartupSingleVariant {
        if let Some(first_audio) = audio.first().cloned() {
            audio = vec![first_audio];
        }
        if let Some(selected_video) =
            startup_video_representation_for_policy(&video, prefer_modern_video_codecs).cloned()
        {
            video = vec![selected_video];
        }
    }

    Ok(SelectedPlayableResponse {
        audio,
        video,
        subtitles,
    })
}

fn filter_hardware_decodable_video(
    video: Vec<PlayableRepresentation>,
    video_decode_capabilities: Option<&[VideoDecodeCapability]>,
) -> DashHlsResult<Vec<PlayableRepresentation>> {
    let Some(capabilities) = video_decode_capabilities else {
        return Ok(video);
    };
    if video.is_empty() {
        return Ok(video);
    }

    let capabilities_by_rendition_id = capabilities
        .iter()
        .map(|capability| (capability.rendition_id.as_str(), capability))
        .collect::<HashMap<_, _>>();
    let mut filtered = Vec::new();
    let mut unsupported = Vec::new();
    for item in video {
        match capabilities_by_rendition_id.get(item.rendition_id.as_str()) {
            Some(capability) if capability.hardware_decode_supported => filtered.push(item),
            Some(capability) => unsupported.push(format!(
                "{}:{}:{}",
                item.rendition_id,
                item.representation.codecs,
                video_codec_family_wire_name(capability.codec_family)
            )),
            None => unsupported.push(format!(
                "{}:{}:missing-capability",
                item.rendition_id, item.representation.codecs
            )),
        }
    }
    if filtered.is_empty() {
        return Err(DashHlsError::UnsupportedMpd(format!(
            "MPD has no hardware-decodable video representation; rejected={}",
            unsupported.join(",")
        )));
    }
    Ok(filtered)
}

fn playable_representations(manifest: &DashManifest) -> DashHlsResult<Vec<PlayableRepresentation>> {
    let period = match manifest.periods.as_slice() {
        [period] => period,
        _ => {
            return Err(DashHlsError::UnsupportedMpd(
                "multi-period DASH is not supported".to_owned(),
            ));
        }
    };

    let mut used_ids: HashMap<String, u32> = HashMap::new();
    let mut playable = Vec::new();
    for (adaptation_index, adaptation_set) in period.adaptation_sets.iter().enumerate() {
        if !matches!(
            adaptation_set.kind,
            DashAdaptationKind::Audio | DashAdaptationKind::Video | DashAdaptationKind::Subtitle
        ) {
            continue;
        }
        for (representation_index, representation) in
            adaptation_set.representations.iter().enumerate()
        {
            if representation.segment_base.is_none() && representation.segment_template.is_none() {
                continue;
            }
            let fallback_id = format!(
                "{}-{adaptation_index}-{representation_index}",
                adaptation_kind_id(adaptation_set.kind)
            );
            let base_id = if representation.id.is_empty() {
                fallback_id
            } else {
                representation.id.clone()
            };
            let seen_count = used_ids.get(&base_id).copied().unwrap_or(0);
            used_ids.insert(base_id.clone(), seen_count + 1);
            let rendition_id = if seen_count == 0 {
                base_id
            } else {
                format!("{}-{}", base_id, seen_count + 1)
            };
            playable.push(PlayableRepresentation {
                rendition_id,
                adaptation_set: adaptation_set.clone(),
                representation: representation.clone(),
            });
        }
    }

    if !playable
        .iter()
        .any(|item| item.adaptation_set.kind != DashAdaptationKind::Subtitle)
    {
        return Err(DashHlsError::UnsupportedMpd(
            "MPD has no SegmentBase or SegmentTemplate audio/video representations".to_owned(),
        ));
    }
    Ok(playable)
}

fn adaptation_kind_id(kind: DashAdaptationKind) -> &'static str {
    match kind {
        DashAdaptationKind::Video => "video",
        DashAdaptationKind::Audio => "audio",
        DashAdaptationKind::Subtitle => "subtitle",
        DashAdaptationKind::Unknown => "unknown",
    }
}

fn startup_video_representation(
    video: &[PlayableRepresentation],
) -> Option<&PlayableRepresentation> {
    startup_video_representation_for_policy(video, false)
}

fn startup_video_representation_for_policy(
    video: &[PlayableRepresentation],
    prefer_modern_video_codecs: bool,
) -> Option<&PlayableRepresentation> {
    video
        .iter()
        .enumerate()
        .min_by_key(|(index, item)| startup_video_score(item, *index, prefer_modern_video_codecs))
        .map(|(_, item)| item)
}

fn startup_video_score(
    item: &PlayableRepresentation,
    index: usize,
    prefer_modern_video_codecs: bool,
) -> (u8, u8, u8, u8, u32, u64, u32, usize) {
    const STARTUP_MAX_HEIGHT: u32 = 720;
    const STARTUP_MAX_BANDWIDTH: u64 = 800_000;

    let representation = &item.representation;
    let codec_family = video_codec_family(&representation.codecs);
    let codec_rank = video_codec_startup_rank(codec_family, prefer_modern_video_codecs);
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

fn video_codec_startup_rank(family: VideoCodecFamily, prefer_modern_video_codecs: bool) -> u8 {
    if prefer_modern_video_codecs {
        match family {
            VideoCodecFamily::Vvc => 0,
            VideoCodecFamily::Av1 => 1,
            VideoCodecFamily::Hevc => 2,
            VideoCodecFamily::Avc => 3,
            VideoCodecFamily::Unknown => u8::MAX,
        }
    } else {
        match family {
            VideoCodecFamily::Avc => 0,
            VideoCodecFamily::Hevc => 1,
            VideoCodecFamily::Vvc | VideoCodecFamily::Av1 | VideoCodecFamily::Unknown => u8::MAX,
        }
    }
}

fn video_codec_family(value: &str) -> VideoCodecFamily {
    for codec in value
        .split(',')
        .map(|codec| codec.trim().to_ascii_lowercase())
        .filter(|codec| !codec.is_empty())
    {
        let codec = codec
            .strip_prefix("video/")
            .map(str::to_owned)
            .unwrap_or(codec);
        if codec.starts_with("vvc1")
            || codec.starts_with("vvi1")
            || codec == "vvc"
            || codec == "h266"
        {
            return VideoCodecFamily::Vvc;
        }
        if codec.starts_with("av01") || codec == "av1" {
            return VideoCodecFamily::Av1;
        }
        if codec.starts_with("hvc1")
            || codec.starts_with("hev1")
            || codec == "hevc"
            || codec == "h265"
        {
            return VideoCodecFamily::Hevc;
        }
        if codec.starts_with("avc1")
            || codec.starts_with("avc3")
            || codec == "avc"
            || codec == "h264"
        {
            return VideoCodecFamily::Avc;
        }
    }
    VideoCodecFamily::Unknown
}

fn video_codec_family_wire_name(family: VideoCodecFamily) -> &'static str {
    match family {
        VideoCodecFamily::Vvc => "vvc",
        VideoCodecFamily::Av1 => "av1",
        VideoCodecFamily::Hevc => "hevc",
        VideoCodecFamily::Avc => "avc",
        VideoCodecFamily::Unknown => "unknown",
    }
}

fn media_url_map(entries: Vec<RenditionUrl>) -> HashMap<String, String> {
    entries
        .into_iter()
        .map(|entry| (entry.rendition_id, entry.url))
        .collect()
}

fn build_master_playlist(
    selected: &SelectedPlayableResponse,
    media_urls: &HashMap<String, String>,
    prefer_modern_video_codecs: bool,
) -> DashHlsResult<String> {
    let mut lines = vec![
        "#EXTM3U".to_owned(),
        "#EXT-X-VERSION:7".to_owned(),
        "#EXT-X-INDEPENDENT-SEGMENTS".to_owned(),
    ];

    if !selected.audio.is_empty() && !selected.video.is_empty() {
        for (index, item) in selected.audio.iter().enumerate() {
            let name = item
                .adaptation_set
                .language
                .as_deref()
                .or(item.adaptation_set.id.as_deref())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("audio-{}", index + 1));
            let media_url = required_media_url(media_urls, &item.rendition_id)?;
            let mut attrs = format!(
                "TYPE=AUDIO,GROUP-ID=\"audio\",NAME=\"{}\",DEFAULT={},AUTOSELECT=YES,URI=\"{}\"",
                escape_attribute(&name),
                if index == 0 { "YES" } else { "NO" },
                escape_attribute(media_url)
            );
            if let Some(language) = &item.adaptation_set.language {
                attrs.push_str(&format!(",LANGUAGE=\"{}\"", escape_attribute(language)));
            }
            lines.push(format!("#EXT-X-MEDIA:{attrs}"));
        }
    }

    if !selected.subtitles.is_empty() {
        for (index, item) in selected.subtitles.iter().enumerate() {
            let name = item
                .adaptation_set
                .language
                .as_deref()
                .or(item.adaptation_set.id.as_deref())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("subtitles-{}", index + 1));
            let media_url = required_media_url(media_urls, &item.rendition_id)?;
            let mut attrs = format!(
                "TYPE=SUBTITLES,GROUP-ID=\"subtitles\",NAME=\"{}\",DEFAULT=NO,AUTOSELECT=YES,FORCED=NO,URI=\"{}\"",
                escape_attribute(&name),
                escape_attribute(media_url)
            );
            if let Some(language) = &item.adaptation_set.language {
                attrs.push_str(&format!(",LANGUAGE=\"{}\"", escape_attribute(language)));
            }
            lines.push(format!("#EXT-X-MEDIA:{attrs}"));
        }
    }

    if selected.video.is_empty() {
        for item in &selected.audio {
            append_variant_lines(
                &mut lines,
                item,
                &[],
                0,
                None,
                (!selected.subtitles.is_empty()).then_some("subtitles"),
                media_urls,
            )?;
        }
    } else {
        let audio_codecs = unique_codecs(
            selected
                .audio
                .iter()
                .map(|item| item.representation.codecs.as_str()),
        );
        let max_audio_bandwidth = selected
            .audio
            .iter()
            .filter_map(|item| item.representation.bandwidth)
            .max()
            .unwrap_or(0);
        let mut ordered_video = selected.video.iter().enumerate().collect::<Vec<_>>();
        ordered_video.sort_by_key(|(index, item)| {
            startup_video_score(item, *index, prefer_modern_video_codecs)
        });
        for (_, item) in ordered_video {
            append_variant_lines(
                &mut lines,
                item,
                &audio_codecs,
                max_audio_bandwidth,
                (!selected.audio.is_empty()).then_some("audio"),
                (!selected.subtitles.is_empty()).then_some("subtitles"),
                media_urls,
            )?;
        }
    }

    lines.push(String::new());
    Ok(lines.join("\n"))
}

fn append_variant_lines(
    lines: &mut Vec<String>,
    item: &PlayableRepresentation,
    extra_codecs: &[String],
    extra_bandwidth: u64,
    audio_group: Option<&str>,
    subtitle_group: Option<&str>,
    media_urls: &HashMap<String, String>,
) -> DashHlsResult<()> {
    let base_bandwidth = item.representation.bandwidth.ok_or_else(|| {
        DashHlsError::InvalidMpd(format!(
            "Representation {} is missing bandwidth",
            item.representation.id
        ))
    })?;
    let average_bandwidth = base_bandwidth.checked_add(extra_bandwidth).ok_or_else(|| {
        DashHlsError::InvalidHlsInput("HLS AVERAGE-BANDWIDTH overflows u64".to_owned())
    })?;
    let peak_bandwidth = average_bandwidth
        .checked_add(average_bandwidth)
        .ok_or_else(|| DashHlsError::InvalidHlsInput("HLS BANDWIDTH overflows u64".to_owned()))?;
    let mut attrs = vec![
        format!("BANDWIDTH={peak_bandwidth}"),
        format!("AVERAGE-BANDWIDTH={average_bandwidth}"),
    ];
    if let (Some(width), Some(height)) = (item.representation.width, item.representation.height)
        && width > 0
        && height > 0
    {
        attrs.push(format!("RESOLUTION={width}x{height}"));
    }
    if let Some(frame_rate) = item
        .representation
        .frame_rate
        .as_deref()
        .and_then(format_frame_rate)
    {
        attrs.push(format!("FRAME-RATE={frame_rate}"));
    }
    let mut codec_values = vec![item.representation.codecs.as_str()];
    codec_values.extend(extra_codecs.iter().map(String::as_str));
    let codecs = unique_codecs(codec_values).join(",");
    if !codecs.is_empty() {
        attrs.push(format!("CODECS=\"{}\"", escape_attribute(&codecs)));
    }
    if let Some(audio_group) = audio_group {
        attrs.push(format!("AUDIO=\"{}\"", escape_attribute(audio_group)));
    }
    if let Some(subtitle_group) = subtitle_group {
        attrs.push(format!(
            "SUBTITLES=\"{}\"",
            escape_attribute(subtitle_group)
        ));
    }

    lines.push(format!("#EXT-X-STREAM-INF:{}", attrs.join(",")));
    lines.push(required_media_url(media_urls, &item.rendition_id)?.to_owned());
    Ok(())
}

fn required_media_url<'a>(
    media_urls: &'a HashMap<String, String>,
    rendition_id: &str,
) -> DashHlsResult<&'a str> {
    media_urls
        .get(rendition_id)
        .map(String::as_str)
        .ok_or_else(|| {
            DashHlsError::InvalidHlsInput(format!(
                "missing HLS media URL for rendition {rendition_id}"
            ))
        })
}

fn unique_codecs<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut codecs = Vec::new();
    for value in values {
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
    codecs
}

fn format_frame_rate(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let rate = if let Some((numerator, denominator)) = trimmed.split_once('/') {
        let numerator: f64 = numerator.trim().parse().ok()?;
        let denominator: f64 = denominator.trim().parse().ok()?;
        if denominator == 0.0 {
            return None;
        }
        numerator / denominator
    } else {
        trimmed.parse().ok()?
    };
    (rate.is_finite() && rate > 0.0).then(|| format_decimal(rate))
}

pub fn media_segments(
    segment_base: &DashSegmentBase,
    sidx: &SidxBox,
) -> DashHlsResult<Vec<MediaSegment>> {
    if sidx.references.is_empty() {
        return Err(DashHlsError::InvalidMp4(
            "sidx must contain at least one reference".to_owned(),
        ));
    }
    if sidx.timescale == 0 {
        return Err(DashHlsError::InvalidMp4(
            "sidx timescale must be non-zero".to_owned(),
        ));
    }
    let mut offset = checked_add(
        checked_add(segment_base.index_range.end, 1, "sidx media offset")?,
        sidx.first_offset,
        "sidx media offset",
    )?;
    let mut segments = Vec::new();
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
        let end = checked_add(
            offset,
            u64::from(reference.referenced_size) - 1,
            "sidx media byte range",
        )?;
        segments.push(MediaSegment {
            duration: f64::from(reference.subsegment_duration) / f64::from(sidx.timescale),
            range: ByteRange::new(offset, end),
        });
        offset = checked_add(end, 1, "sidx next media offset")?;
    }
    Ok(segments)
}

pub fn template_segments(
    manifest_type: Option<DashManifestType>,
    duration_ms: Option<u64>,
    segment_template: &DashSegmentTemplate,
) -> DashHlsResult<Vec<TemplateSegment>> {
    if !segment_template.timeline.is_empty() {
        return timeline_template_segments(duration_ms, segment_template);
    }
    if manifest_type == Some(DashManifestType::Dynamic) {
        return Err(DashHlsError::UnsupportedMpd(
            "dynamic SegmentTemplate without SegmentTimeline is not supported".to_owned(),
        ));
    }
    let declared_duration = segment_template.duration.ok_or_else(|| {
        DashHlsError::UnsupportedMpd(
            "SegmentTemplate without SegmentTimeline requires duration".to_owned(),
        )
    })?;
    let duration_ms = duration_ms.filter(|value| *value > 0).ok_or_else(|| {
        DashHlsError::UnsupportedMpd(
            "SegmentTemplate requires mediaPresentationDuration".to_owned(),
        )
    })?;
    let total_duration = duration_ms as f64 / 1_000.0;
    let segment_duration = normalized_fixed_template_duration(
        declared_duration as f64 / segment_template.timescale as f64,
    );
    if !total_duration.is_finite() || !segment_duration.is_finite() || segment_duration <= 0.0 {
        return Err(DashHlsError::InvalidMpd(
            "invalid SegmentTemplate duration".to_owned(),
        ));
    }
    let segment_count = (total_duration / segment_duration).ceil();
    if !segment_count.is_finite() || segment_count <= 0.0 || segment_count > usize::MAX as f64 {
        return Err(DashHlsError::InvalidMpd(
            "invalid SegmentTemplate segment count".to_owned(),
        ));
    }
    let segment_count = segment_count as usize;
    let mut segments = Vec::with_capacity(segment_count);
    for index in 0..segment_count {
        let number = checked_add(
            segment_template.start_number,
            index as u64,
            "SegmentTemplate segment number",
        )?;
        let remaining = total_duration - (index as f64 * segment_duration);
        let duration = segment_duration.min(remaining);
        if !duration.is_finite() || duration <= 0.0 {
            return Err(DashHlsError::InvalidMpd(
                "invalid SegmentTemplate segment duration".to_owned(),
            ));
        }
        segments.push(TemplateSegment {
            duration,
            number,
            time: None,
        });
    }
    Ok(segments)
}

fn timeline_template_segments(
    duration_ms: Option<u64>,
    segment_template: &DashSegmentTemplate,
) -> DashHlsResult<Vec<TemplateSegment>> {
    let timeline_end = timeline_end_tick(duration_ms, segment_template)?;
    let mut next_start = None;
    let mut segment_index = 0_u64;
    let mut segments = Vec::new();

    for (entry_index, entry) in segment_template.timeline.iter().enumerate() {
        let entry_start = entry.start_time.or(next_start).unwrap_or(0);
        let next_explicit_start = segment_template.timeline[entry_index + 1..]
            .iter()
            .find_map(|entry| entry.start_time);
        let repeat_count =
            expanded_timeline_repeat_count(entry, entry_start, next_explicit_start, timeline_end)?;
        if repeat_count == 0 {
            next_start = Some(entry_start);
            continue;
        }

        let mut current_start = entry_start;
        for _ in 0..repeat_count {
            if let Some(timeline_end) = timeline_end
                && current_start >= timeline_end
            {
                break;
            }
            let unclipped_end =
                checked_add(current_start, entry.duration, "SegmentTimeline segment end")?;
            let clipped_end = min_timeline_end(unclipped_end, timeline_end, next_explicit_start);
            if clipped_end <= current_start {
                break;
            }
            let number = checked_add(
                segment_template.start_number,
                segment_index,
                "SegmentTemplate segment number",
            )?;
            let duration = (clipped_end - current_start) as f64 / segment_template.timescale as f64;
            if !duration.is_finite() || duration <= 0.0 {
                return Err(DashHlsError::InvalidMpd(
                    "invalid SegmentTimeline segment duration".to_owned(),
                ));
            }
            segments.push(TemplateSegment {
                duration,
                number,
                time: Some(current_start),
            });
            segment_index = checked_add(segment_index, 1, "SegmentTimeline segment index")?;
            current_start = unclipped_end;
        }
        next_start = Some(current_start);
    }

    if segments.is_empty() {
        return Err(DashHlsError::InvalidMpd(
            "SegmentTimeline produced no media segments".to_owned(),
        ));
    }
    Ok(segments)
}

fn expanded_timeline_repeat_count(
    entry: &crate::dash::DashSegmentTimelineEntry,
    entry_start: u64,
    next_explicit_start: Option<u64>,
    timeline_end: Option<u64>,
) -> DashHlsResult<usize> {
    if entry.repeat_count >= 0 {
        return usize::try_from(entry.repeat_count as u64 + 1).map_err(|_| {
            DashHlsError::InvalidMpd("SegmentTimeline repeat count exceeds usize".to_owned())
        });
    }

    let boundary = if let Some(next_explicit_start) = next_explicit_start {
        if next_explicit_start <= entry_start {
            return Err(DashHlsError::InvalidMpd(
                "SegmentTimeline next S@t must be greater than current time".to_owned(),
            ));
        }
        next_explicit_start
    } else if let Some(timeline_end) = timeline_end {
        if timeline_end <= entry_start {
            return Ok(0);
        }
        timeline_end
    } else {
        return Err(DashHlsError::UnsupportedMpd(
            "SegmentTimeline r=-1 requires next S@t or mediaPresentationDuration".to_owned(),
        ));
    };

    usize::try_from(ceil_div(boundary - entry_start, entry.duration)).map_err(|_| {
        DashHlsError::InvalidMpd("SegmentTimeline expanded repeat count exceeds usize".to_owned())
    })
}

fn timeline_end_tick(
    duration_ms: Option<u64>,
    segment_template: &DashSegmentTemplate,
) -> DashHlsResult<Option<u64>> {
    let Some(duration_ms) = duration_ms.filter(|value| *value > 0) else {
        return Ok(None);
    };
    let end_tick = (duration_ms as f64 * segment_template.timescale as f64 / 1_000.0).round();
    if !end_tick.is_finite() || end_tick < 0.0 || end_tick > u64::MAX as f64 {
        return Err(DashHlsError::InvalidMpd(
            "SegmentTimeline media duration exceeds UInt64".to_owned(),
        ));
    }
    checked_add(
        segment_template.presentation_time_offset,
        end_tick as u64,
        "SegmentTimeline media end",
    )
    .map(Some)
}

fn min_timeline_end(
    value: u64,
    timeline_end: Option<u64>,
    next_explicit_start: Option<u64>,
) -> u64 {
    let mut result = value;
    if let Some(timeline_end) = timeline_end {
        result = result.min(timeline_end);
    }
    if let Some(next_explicit_start) = next_explicit_start {
        result = result.min(next_explicit_start);
    }
    result
}

fn ceil_div(value: u64, divisor: u64) -> u64 {
    if divisor == 0 {
        return 0;
    }
    let quotient = value / divisor;
    if value.is_multiple_of(divisor) {
        quotient
    } else {
        quotient + 1
    }
}

fn normalized_fixed_template_duration(value: f64) -> f64 {
    if !value.is_finite() || value <= 0.0 {
        return value;
    }
    let rounded = value.round();
    let tolerance = 0.010_f64.max(value * 0.005);
    if rounded > 0.0 && (rounded - value).abs() <= tolerance {
        rounded
    } else {
        value
    }
}

fn build_external_media_playlist(
    map: Option<&HlsMap>,
    segments: &[HlsSegment],
    playlist_kind: HlsPlaylistKind,
    media_sequence: Option<u64>,
) -> DashHlsResult<String> {
    if segments.is_empty() {
        return Err(DashHlsError::InvalidMp4(
            "media playlist must contain at least one segment".to_owned(),
        ));
    }
    let target_duration = segments
        .iter()
        .map(|segment| segment.duration)
        .fold(1.0_f64, f64::max)
        .ceil()
        .max(1.0) as u64;
    let mut lines = vec![
        "#EXTM3U".to_owned(),
        "#EXT-X-VERSION:7".to_owned(),
        format!("#EXT-X-TARGETDURATION:{target_duration}"),
        format!("#EXT-X-MEDIA-SEQUENCE:{}", media_sequence.unwrap_or(1)),
        "#EXT-X-INDEPENDENT-SEGMENTS".to_owned(),
    ];
    if playlist_kind == HlsPlaylistKind::Vod {
        lines.push("#EXT-X-PLAYLIST-TYPE:VOD".to_owned());
    }
    if let Some(map) = map {
        if let Some(byte_range) = &map.byte_range {
            lines.push(format!(
                "#EXT-X-MAP:URI=\"{}\",BYTERANGE=\"{}@{}\"",
                escape_attribute(&map.uri),
                byte_range_len(byte_range, "map byte range")?,
                byte_range.start
            ));
        } else {
            lines.push(format!("#EXT-X-MAP:URI=\"{}\"", escape_attribute(&map.uri)));
        }
    }

    for segment in segments {
        if !segment.duration.is_finite() || segment.duration <= 0.0 {
            return Err(DashHlsError::InvalidHlsInput(
                "segment duration must be finite and positive".to_owned(),
            ));
        }
        lines.push(format!("#EXTINF:{},", format_decimal(segment.duration)));
        if let Some(byte_range) = &segment.byte_range {
            lines.push(format!(
                "#EXT-X-BYTERANGE:{}@{}",
                byte_range_len(byte_range, "segment byte range")?,
                byte_range.start
            ));
        }
        lines.push(segment.uri.clone());
    }
    if playlist_kind == HlsPlaylistKind::Vod {
        lines.push("#EXT-X-ENDLIST".to_owned());
    }
    lines.push(String::new());
    Ok(lines.join("\n"))
}

fn expand_template(
    template: &str,
    representation: &DashRepresentation,
    number: Option<u64>,
    time: Option<u64>,
) -> DashHlsResult<String> {
    let mut output = String::new();
    let mut cursor = 0;
    while cursor < template.len() {
        let Some(relative_index) = template[cursor..].find('$') else {
            output.push_str(&template[cursor..]);
            break;
        };
        let token_start_marker = cursor + relative_index;
        output.push_str(&template[cursor..token_start_marker]);
        let token_start = token_start_marker + 1;
        if token_start >= template.len() {
            return Err(DashHlsError::InvalidMpd(
                "unterminated SegmentTemplate token".to_owned(),
            ));
        }
        if template[token_start..].starts_with('$') {
            output.push('$');
            cursor = token_start + 1;
            continue;
        }
        let Some(token_end_relative) = template[token_start..].find('$') else {
            return Err(DashHlsError::InvalidMpd(
                "unterminated SegmentTemplate token".to_owned(),
            ));
        };
        let token_end = token_start + token_end_relative;
        output.push_str(&expand_token(
            &template[token_start..token_end],
            representation,
            number,
            time,
        )?);
        cursor = token_end + 1;
    }
    Ok(output)
}

fn expand_token(
    token: &str,
    representation: &DashRepresentation,
    number: Option<u64>,
    time: Option<u64>,
) -> DashHlsResult<String> {
    let (name, format) = token
        .split_once('%')
        .map(|(name, format)| (name, Some(format!("%{format}"))))
        .unwrap_or((token, None));
    match name {
        "RepresentationID" => {
            if format.is_some() {
                return Err(DashHlsError::UnsupportedMpd(
                    "SegmentTemplate RepresentationID formatting is not supported".to_owned(),
                ));
            }
            Ok(representation.id.clone())
        }
        "Number" => format_template_number(
            number.ok_or_else(|| {
                DashHlsError::InvalidMpd("SegmentTemplate Number is not available".to_owned())
            })?,
            format.as_deref(),
        ),
        "Bandwidth" => format_template_number(
            representation.bandwidth.ok_or_else(|| {
                DashHlsError::InvalidMpd(
                    "SegmentTemplate Bandwidth requires representation bandwidth".to_owned(),
                )
            })?,
            format.as_deref(),
        ),
        "Time" => format_template_number(
            time.ok_or_else(|| {
                DashHlsError::InvalidMpd("SegmentTemplate Time requires SegmentTimeline".to_owned())
            })?,
            format.as_deref(),
        ),
        _ => Err(DashHlsError::UnsupportedMpd(format!(
            "unsupported SegmentTemplate token {name}"
        ))),
    }
}

fn format_template_number(value: u64, format: Option<&str>) -> DashHlsResult<String> {
    let Some(format) = format else {
        return Ok(value.to_string());
    };
    let Some(rest) = format.strip_prefix('%') else {
        return Err(DashHlsError::InvalidMpd(format!(
            "invalid SegmentTemplate format {format}"
        )));
    };
    let mut chars = rest.chars().peekable();
    let padding = if matches!(chars.peek(), Some('0')) {
        chars.next();
        '0'
    } else {
        ' '
    };
    let mut width_text = String::new();
    while let Some(ch) = chars.peek().copied() {
        if ch.is_ascii_digit() {
            width_text.push(ch);
            chars.next();
        } else {
            break;
        }
    }
    let Some(specifier) = chars.next() else {
        return Err(DashHlsError::UnsupportedMpd(format!(
            "unsupported SegmentTemplate format {format}"
        )));
    };
    if !matches!(specifier, 'd' | 'i' | 'u') || chars.next().is_some() {
        return Err(DashHlsError::UnsupportedMpd(format!(
            "unsupported SegmentTemplate format {format}"
        )));
    }
    let raw = value.to_string();
    let width = width_text.parse::<usize>().unwrap_or(0);
    if width <= raw.len() {
        return Ok(raw);
    }
    Ok(format!(
        "{}{}",
        padding.to_string().repeat(width - raw.len()),
        raw
    ))
}

fn checked_add(lhs: u64, rhs: u64, field: &str) -> DashHlsResult<u64> {
    lhs.checked_add(rhs)
        .ok_or_else(|| DashHlsError::InvalidMp4(format!("{field} overflows UInt64")))
}

fn byte_range_len(range: &ByteRange, field: &str) -> DashHlsResult<u64> {
    range
        .len()
        .ok_or_else(|| DashHlsError::InvalidHlsInput(format!("invalid byte range for {field}")))
}

fn escape_attribute(value: &str) -> String {
    value.replace('"', "%22").replace(['\n', '\r'], "")
}

fn format_decimal(value: f64) -> String {
    format!("{value:.3}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_video_prefers_low_cost_supported_variant() {
        let video = vec![
            playable_video(
                "avc-2160",
                "avc1.640033",
                Some(20_000_000),
                Some(3840),
                Some(2160),
            ),
            playable_video(
                "avc-360",
                "avc1.4d401e",
                Some(109_000),
                Some(640),
                Some(360),
            ),
            playable_video(
                "avc-720",
                "avc1.4d401f",
                Some(800_000),
                Some(1280),
                Some(720),
            ),
        ];

        let selected = startup_video_representation(&video).expect("startup video");

        assert_eq!(selected.rendition_id, "avc-360");
    }

    #[test]
    fn startup_video_keeps_supported_codec_before_unknown_low_variant() {
        let video = vec![
            playable_video(
                "vp9-360",
                "vp09.00.10.08",
                Some(90_000),
                Some(640),
                Some(360),
            ),
            playable_video(
                "avc-720",
                "avc1.4d401f",
                Some(800_000),
                Some(1280),
                Some(720),
            ),
        ];

        let selected = startup_video_representation(&video).expect("startup video");

        assert_eq!(selected.rendition_id, "avc-720");
    }

    #[test]
    fn startup_video_keeps_avc_first_without_decode_capabilities() {
        let video = vec![
            playable_video(
                "av1-720",
                "av01.0.05M.08",
                Some(760_000),
                Some(1280),
                Some(720),
            ),
            playable_video(
                "avc-720",
                "avc1.4d401f",
                Some(800_000),
                Some(1280),
                Some(720),
            ),
        ];

        let selected = startup_video_representation(&video).expect("startup video");

        assert_eq!(selected.rendition_id, "avc-720");
    }

    #[test]
    fn startup_video_prefers_newer_hardware_codec_within_startup_target() {
        let video = vec![
            playable_video(
                "hevc-720",
                "hvc1.1.6.L93.B0",
                Some(800_000),
                Some(1280),
                Some(720),
            ),
            playable_video(
                "av1-720",
                "av01.0.05M.08",
                Some(780_000),
                Some(1280),
                Some(720),
            ),
        ];

        let selected =
            startup_video_representation_for_policy(&video, true).expect("startup video");

        assert_eq!(selected.rendition_id, "av1-720");
    }

    #[test]
    fn startup_video_keeps_startup_cost_before_codec_efficiency() {
        let video = vec![
            playable_video(
                "av1-2160",
                "av01.0.13M.10",
                Some(16_000_000),
                Some(3840),
                Some(2160),
            ),
            playable_video(
                "avc-360",
                "avc1.4d401e",
                Some(180_000),
                Some(640),
                Some(360),
            ),
        ];

        let selected = startup_video_representation(&video).expect("startup video");

        assert_eq!(selected.rendition_id, "avc-360");
    }

    #[test]
    fn hardware_decode_capabilities_downgrade_unsupported_av1_to_hevc() {
        let video = vec![
            playable_video(
                "av1-720",
                "av01.0.05M.08",
                Some(760_000),
                Some(1280),
                Some(720),
            ),
            playable_video(
                "hevc-720",
                "hvc1.1.6.L93.B0",
                Some(800_000),
                Some(1280),
                Some(720),
            ),
            playable_video(
                "avc-720",
                "avc1.4d401f",
                Some(800_000),
                Some(1280),
                Some(720),
            ),
        ];
        let capabilities = vec![
            video_decode_capability("av1-720", VideoCodecFamily::Av1, false),
            video_decode_capability("hevc-720", VideoCodecFamily::Hevc, true),
            video_decode_capability("avc-720", VideoCodecFamily::Avc, true),
        ];

        let filtered = filter_hardware_decodable_video(video, Some(&capabilities))
            .expect("filtered hardware video");
        let selected =
            startup_video_representation_for_policy(&filtered, true).expect("startup video");

        assert_eq!(
            filtered
                .iter()
                .map(|item| item.rendition_id.as_str())
                .collect::<Vec<_>>(),
            vec!["hevc-720", "avc-720"]
        );
        assert_eq!(selected.rendition_id, "hevc-720");
    }

    #[test]
    fn hardware_decode_capabilities_fail_when_every_video_is_software_only() {
        let video = vec![playable_video(
            "av1-720",
            "av01.0.05M.08",
            Some(760_000),
            Some(1280),
            Some(720),
        )];
        let capabilities = vec![video_decode_capability(
            "av1-720",
            VideoCodecFamily::Av1,
            false,
        )];

        let error = filter_hardware_decodable_video(video, Some(&capabilities))
            .expect_err("software-only source should fail");

        assert!(error.to_string().contains("hardware-decodable"));
    }

    #[test]
    fn video_codec_family_detects_modern_codec_tokens() {
        assert_eq!(video_codec_family("vvc1.1.L123"), VideoCodecFamily::Vvc);
        assert_eq!(video_codec_family("av01.0.05M.08"), VideoCodecFamily::Av1);
        assert_eq!(video_codec_family("video/av01"), VideoCodecFamily::Av1);
        assert_eq!(
            video_codec_family("hvc1.1.6.L93.B0"),
            VideoCodecFamily::Hevc
        );
        assert_eq!(
            video_codec_family("hev1.1.6.L93.B0"),
            VideoCodecFamily::Hevc
        );
        assert_eq!(video_codec_family("avc1.4d401f"), VideoCodecFamily::Avc);
        assert_eq!(video_codec_family("mp4a.40.2"), VideoCodecFamily::Unknown);
    }

    #[test]
    fn master_playlist_orders_startup_video_first() {
        let selected = SelectedPlayableResponse {
            audio: Vec::new(),
            video: vec![
                playable_video(
                    "avc-2160",
                    "avc1.640033",
                    Some(20_000_000),
                    Some(3840),
                    Some(2160),
                ),
                playable_video(
                    "avc-360",
                    "avc1.4d401e",
                    Some(109_000),
                    Some(640),
                    Some(360),
                ),
                playable_video(
                    "avc-720",
                    "avc1.4d401f",
                    Some(800_000),
                    Some(1280),
                    Some(720),
                ),
            ],
            subtitles: Vec::new(),
        };
        let media_urls = HashMap::from([
            (
                "avc-2160".to_owned(),
                "vesper-dash://media/avc-2160.m3u8".to_owned(),
            ),
            (
                "avc-360".to_owned(),
                "vesper-dash://media/avc-360.m3u8".to_owned(),
            ),
            (
                "avc-720".to_owned(),
                "vesper-dash://media/avc-720.m3u8".to_owned(),
            ),
        ]);

        let playlist =
            build_master_playlist(&selected, &media_urls, false).expect("master playlist");
        let variant_urls = playlist
            .lines()
            .filter(|line| line.starts_with("vesper-dash://media/"))
            .collect::<Vec<_>>();

        assert_eq!(
            variant_urls,
            vec![
                "vesper-dash://media/avc-360.m3u8",
                "vesper-dash://media/avc-720.m3u8",
                "vesper-dash://media/avc-2160.m3u8",
            ]
        );
    }

    #[test]
    fn expands_segment_template_timeline() {
        let template = DashSegmentTemplate {
            timescale: 1_000,
            duration: None,
            start_number: 7,
            presentation_time_offset: 5_000,
            initialization: Some("init.mp4".to_owned()),
            media: "chunk-$Time%05d$-$Number$.m4s".to_owned(),
            timeline: vec![
                crate::dash::DashSegmentTimelineEntry {
                    start_time: Some(5_000),
                    duration: 2_000,
                    repeat_count: 2,
                },
                crate::dash::DashSegmentTimelineEntry {
                    start_time: None,
                    duration: 1_000,
                    repeat_count: 0,
                },
            ],
        };

        let segments = template_segments(Some(DashManifestType::Static), Some(7_000), &template)
            .expect("segments");

        assert_eq!(
            segments,
            vec![
                TemplateSegment {
                    duration: 2.0,
                    number: 7,
                    time: Some(5_000)
                },
                TemplateSegment {
                    duration: 2.0,
                    number: 8,
                    time: Some(7_000)
                },
                TemplateSegment {
                    duration: 2.0,
                    number: 9,
                    time: Some(9_000)
                },
                TemplateSegment {
                    duration: 1.0,
                    number: 10,
                    time: Some(11_000)
                },
            ]
        );
    }

    fn playable_video(
        id: &str,
        codecs: &str,
        bandwidth: Option<u64>,
        width: Option<u32>,
        height: Option<u32>,
    ) -> PlayableRepresentation {
        PlayableRepresentation {
            rendition_id: id.to_owned(),
            adaptation_set: DashAdaptationSet {
                id: None,
                kind: DashAdaptationKind::Video,
                mime_type: Some("video/mp4".to_owned()),
                language: None,
                representations: Vec::new(),
            },
            representation: DashRepresentation {
                id: id.to_owned(),
                base_url: format!("{id}.m4s"),
                mime_type: "video/mp4".to_owned(),
                codecs: codecs.to_owned(),
                bandwidth,
                width,
                height,
                frame_rate: None,
                audio_sampling_rate: None,
                segment_base: None,
                segment_template: Some(DashSegmentTemplate {
                    timescale: 1_000,
                    duration: Some(2_000),
                    start_number: 1,
                    presentation_time_offset: 0,
                    initialization: Some("init.mp4".to_owned()),
                    media: "chunk-$Number$.m4s".to_owned(),
                    timeline: Vec::new(),
                }),
            },
        }
    }

    fn video_decode_capability(
        rendition_id: &str,
        codec_family: VideoCodecFamily,
        hardware_decode_supported: bool,
    ) -> VideoDecodeCapability {
        VideoDecodeCapability {
            rendition_id: rendition_id.to_owned(),
            codec_family,
            hardware_decode_supported,
            decoder_name: None,
        }
    }
}
