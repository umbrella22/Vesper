use player_dash_hls_bridge::{
    DashHlsError,
    dash::{DashAdaptationKind, DashManifestType, DashRepresentation, parse_mpd_with_base_uri},
    hls::{build_hls_master_input_from_dash_manifest, build_hls_master_playlist},
    ops::{expand_template, template_segments},
};
use serde_json::{Value, json};
use url::Url;

use crate::failure;

// This preview accepts complete, explicitly listed fMP4 windows, not a wall-clock
// availability model. Limits bound materialization and each synchronous ABI call.
const MAX_WINDOW_SEGMENTS: usize = 256;
const MAX_MPD_BYTES: usize = 256 * 1024;
const MAX_OUTPUT_BYTES: usize = 900 * 1024;

#[derive(Clone, PartialEq, Eq)]
struct Segment {
    number: u64,
    time: u64,
    end: u64,
    upstream: String,
}

#[derive(Clone, PartialEq, Eq)]
struct TrackIdentity {
    representation: DashRepresentation,
    initialization: String,
}

#[derive(Clone)]
struct Track {
    identity: TrackIdentity,
    segments: Vec<Segment>,
}

/// One source/activation-owned, CPU-only dynamic DASH projection session.
#[derive(Default)]
pub struct DashLive {
    identity: Option<(String, Option<String>)>,
    tracks: Vec<Track>,
    target_durations: Option<[u64; 2]>,
}

fn unsupported() -> Value {
    failure(
        "unsupported",
        "Dynamic DASH requires one clear Period, one AVC video and one AAC audio representation, and finite continuous SegmentTimeline windows",
    )
}

fn http_uri(base: &str, relative: &str) -> Result<String, Value> {
    let uri = Url::parse(base)
        .and_then(|base| base.join(relative))
        .map_err(|_| unsupported())?;
    if !matches!(uri.scheme(), "http" | "https")
        || !uri.username().is_empty()
        || uri.password().is_some()
        || uri.fragment().is_some()
        || uri.as_str().len() > 2048
        || (base.starts_with("https:") && uri.scheme() != "https")
    {
        return Err(unsupported());
    }
    Ok(uri.to_string())
}

fn validate_xml(mpd: &str) -> Result<(), Value> {
    let mut reader = quick_xml::Reader::from_str(mpd);
    loop {
        use quick_xml::events::Event;
        match reader.read_event() {
            Ok(Event::Start(tag) | Event::Empty(tag)) => {
                if matches!(
                    tag.local_name().as_ref(),
                    "ContentProtection"
                        | "SegmentList"
                        | "SegmentBase"
                        | "EssentialProperty"
                        | "SubRepresentation"
                ) {
                    return Err(unsupported());
                }
                for attribute in tag.attributes() {
                    let attribute = attribute.map_err(|_| unsupported())?;
                    // These attributes require timing/partial-segment semantics
                    // that the shared manifest model deliberately does not own.
                    if matches!(
                        attribute.key.as_ref(),
                        "availabilityTimeOffset" | "availabilityTimeComplete" | "href"
                    ) || attribute.key.as_ref().ends_with(":href")
                        || (tag.local_name().as_ref() == "Period"
                            && matches!(attribute.key.as_ref(), "start" | "duration"))
                    {
                        return Err(unsupported());
                    }
                }
            }
            Ok(Event::DocType(_)) | Err(_) => return Err(unsupported()),
            Ok(Event::Eof) => return Ok(()),
            _ => {}
        }
    }
}

fn track(representation: &DashRepresentation) -> Result<Track, Value> {
    let template = representation
        .segment_template
        .as_ref()
        .ok_or_else(unsupported)?;
    if representation.segment_base.is_some()
        || template.timescale == 0
        || template.start_number == 0
        || template.presentation_time_offset != 0
        || template.timeline.is_empty()
        || template.timeline[0].start_time.is_none()
    {
        return Err(unsupported());
    }
    let mut count = 0_usize;
    let mut next_time = template.timeline[0].start_time.ok_or_else(unsupported)?;
    let mut ticks = Vec::new();
    for entry in &template.timeline {
        let repeats = usize::try_from(entry.repeat_count)
            .map_err(|_| unsupported())?
            .checked_add(1)
            .ok_or_else(unsupported)?;
        count = count.checked_add(repeats).ok_or_else(unsupported)?;
        if count > MAX_WINDOW_SEGMENTS {
            return Err(failure(
                "capacity",
                "Dynamic DASH window exceeds 256 segments per track",
            ));
        }
        if entry.start_time.unwrap_or(next_time) != next_time
            || entry.duration == 0
            || u128::from(entry.duration) * 1000 < u128::from(template.timescale) * 100
            || u128::from(entry.duration) > u128::from(template.timescale) * 30
        {
            return Err(unsupported());
        }
        for _ in 0..repeats {
            let end = next_time
                .checked_add(entry.duration)
                .ok_or_else(unsupported)?;
            ticks.push((next_time, end));
            next_time = end;
        }
    }
    if count < 3 {
        return Err(unsupported());
    }
    let expanded = template_segments(Some(DashManifestType::Dynamic), None, template)
        .map_err(|_| unsupported())?;
    if expanded.len() != ticks.len() {
        return Err(unsupported());
    }
    let initialization = expand_template(
        template.initialization.as_deref().ok_or_else(unsupported)?,
        representation,
        None,
        None,
    )
    .map_err(|_| unsupported())?;
    let initialization = http_uri(&representation.base_url, &initialization)?;
    let mut segments = Vec::with_capacity(expanded.len());
    for (segment, (time, end)) in expanded.iter().zip(ticks) {
        if segment.time != Some(time) {
            return Err(unsupported());
        }
        let resource = expand_template(
            &template.media,
            representation,
            Some(segment.number),
            segment.time,
        )
        .map_err(|_| unsupported())?;
        segments.push(Segment {
            number: segment.number,
            time,
            end,
            upstream: http_uri(&representation.base_url, &resource)?,
        });
    }
    // Only the moving window is mutable. Decoder configuration, PTO, templates,
    // initialization and resource authority must stay stable within an epoch.
    let mut identity = representation.clone();
    if let Some(template) = identity.segment_template.as_mut() {
        template.start_number = 0;
        template.timeline.clear();
    }
    Ok(Track {
        identity: TrackIdentity {
            representation: identity,
            initialization,
        },
        segments,
    })
}

fn compatible(previous: &Track, next: &Track) -> bool {
    if previous.identity != next.identity {
        return false;
    }
    let (Some(old_first), Some(old_last), Some(new_first), Some(new_last)) = (
        previous.segments.first(),
        previous.segments.last(),
        next.segments.first(),
        next.segments.last(),
    ) else {
        return false;
    };
    if new_first.number < old_first.number || new_last.number < old_last.number {
        return false;
    }
    let mut overlap = false;
    for segment in &next.segments {
        if let Some(old) = previous
            .segments
            .iter()
            .find(|old| old.number == segment.number)
        {
            if old != segment {
                return false;
            }
            overlap = true;
        }
    }
    overlap
        || (old_last.number.checked_add(1) == Some(new_first.number)
            && old_last.end == new_first.time)
}

fn aligned(video: u64, video_scale: u64, audio: u64, audio_scale: u64) -> bool {
    // AAC boundaries can differ by a frame. Compare original clocks, without
    // independently normalizing the tracks or rounding identity-bearing ticks.
    let difference = (u128::from(video) * u128::from(audio_scale))
        .abs_diff(u128::from(audio) * u128::from(video_scale));
    difference <= u128::from(video_scale) * u128::from(audio_scale) / 20
}

impl DashLive {
    pub(crate) fn execute(&mut self, value: &Value) -> Result<Value, Value> {
        let mpd = value["mpd"].as_str().ok_or_else(unsupported)?;
        let manifest_url = value["manifestUrl"].as_str().ok_or_else(unsupported)?;
        if mpd.len() > MAX_MPD_BYTES {
            return Err(failure("capacity", "Live DASH manifest exceeds 256 KiB"));
        }
        http_uri(manifest_url, "")?;
        let manifest =
            parse_mpd_with_base_uri(mpd, Some(manifest_url)).map_err(|_| unsupported())?;
        if value["op"] == "classify" {
            return Ok(json!({"dynamic":manifest.manifest_type == DashManifestType::Dynamic}));
        }
        if value["op"] != "refresh" {
            return Err(failure("unsupported", "Unknown live DASH operation"));
        }
        let root = value["virtualRoot"]
            .as_str()
            .filter(|root| root.ends_with('/'))
            .ok_or_else(unsupported)?;
        http_uri(root, "")?;
        validate_xml(mpd)?;
        if manifest.manifest_type != DashManifestType::Dynamic || manifest.duration_ms.is_some() {
            return Err(unsupported());
        }
        let refresh_ms = manifest
            .minimum_update_period_ms
            .filter(|value| (1000..=30000).contains(value))
            .ok_or_else(unsupported)?;
        let [period] = manifest.periods.as_slice() else {
            return Err(unsupported());
        };
        if period.adaptation_sets.len() != 2 {
            return Err(unsupported());
        }
        let mut ordered = Vec::new();
        for kind in [DashAdaptationKind::Video, DashAdaptationKind::Audio] {
            let sets: Vec<_> = period
                .adaptation_sets
                .iter()
                .filter(|set| set.kind == kind)
                .collect();
            let [set] = sets.as_slice() else {
                return Err(unsupported());
            };
            let [representation] = set.representations.as_slice() else {
                return Err(unsupported());
            };
            let supported = match kind {
                DashAdaptationKind::Video => {
                    representation.mime_type == "video/mp4"
                        && (representation.codecs.starts_with("avc1.")
                            || representation.codecs.starts_with("avc3."))
                }
                DashAdaptationKind::Audio => {
                    representation.mime_type == "audio/mp4" && representation.codecs == "mp4a.40.2"
                }
                _ => false,
            };
            if !supported
                || representation.id.is_empty()
                || representation.id.len() > 128
                || representation
                    .bandwidth
                    .is_none_or(|value| value == 0 || value > 9_007_199_254_740_991)
            {
                return Err(unsupported());
            }
            ordered.push(track(representation)?);
        }
        let (video, audio) = (&ordered[0], &ordered[1]);
        let video_scale = video
            .identity
            .representation
            .segment_template
            .as_ref()
            .ok_or_else(unsupported)?
            .timescale;
        let audio_scale = audio
            .identity
            .representation
            .segment_template
            .as_ref()
            .ok_or_else(unsupported)?
            .timescale;
        if !aligned(
            video.segments[0].time,
            video_scale,
            audio.segments[0].time,
            audio_scale,
        ) || !aligned(
            video.segments.last().ok_or_else(unsupported)?.end,
            video_scale,
            audio.segments.last().ok_or_else(unsupported)?.end,
            audio_scale,
        ) {
            return Err(unsupported());
        }
        let identity = (root.to_owned(), period.id.clone());
        if self
            .identity
            .as_ref()
            .is_some_and(|previous| previous != &identity)
            || (!self.tracks.is_empty()
                && !self
                    .tracks
                    .iter()
                    .zip(&ordered)
                    .all(|(old, new)| compatible(old, new)))
        {
            return Err(failure(
                "resource_changed",
                "Live DASH track identity or segment sequence changed within the activation",
            ));
        }
        let master_input = build_hls_master_input_from_dash_manifest(&manifest, |set, _| {
            format!(
                "{root}{}.m3u8",
                if set.kind == DashAdaptationKind::Video {
                    "video"
                } else {
                    "audio"
                }
            )
        })
        .map_err(|_| unsupported())?;
        let master = build_hls_master_playlist(&master_input).map_err(|_| unsupported())?;
        let mut playlists = vec![json!({"uri":format!("{root}master.m3u8"), "text":master})];
        let mut resources = Vec::new();
        let mut sequences = Vec::new();
        let mut targets = [0; 2];
        for (index, track) in ordered.iter().enumerate() {
            let name = if index == 0 { "video" } else { "audio" };
            let initialization = format!("{root}{name}/init.mp4");
            resources
                .push(json!({"uri":initialization,"upstreamUri":track.identity.initialization}));
            let scale = track
                .identity
                .representation
                .segment_template
                .as_ref()
                .ok_or_else(unsupported)?
                .timescale;
            let mut segments = Vec::new();
            let mut longest = 1.0_f64;
            for segment in &track.segments {
                let uri = format!("{root}{name}/segment-{}.m4s", segment.time);
                let duration = (segment.end - segment.time) as f64 / scale as f64;
                longest = longest.max(duration);
                segments.push(json!({"uri":uri,"duration":duration}));
                resources.push(json!({"uri":uri,"upstreamUri":segment.upstream}));
            }
            let first_number = track.segments[0].number;
            let target = self
                .target_durations
                .map_or_else(|| longest.ceil() as u64, |targets| targets[index]);
            targets[index] = target;
            let request = json!({"operation":"build_external_media_playlist","map":{"uri":initialization},
                "segments":segments,"playlistKind":"live","mediaSequence":first_number,"targetDuration":target});
            let result = player_dash_hls_bridge::ops::execute_json(&request.to_string()).map_err(
                |error| match error {
                    DashHlsError::TargetDurationExceeded { .. } => {
                        failure("resource_changed", &error.to_string())
                    }
                    _ => unsupported(),
                },
            )?;
            let playlist: String = serde_json::from_str(&result).map_err(|_| unsupported())?;
            playlists.push(json!({"uri":format!("{root}{name}.m3u8"),"text":playlist}));
            sequences.push(first_number.to_string());
        }
        let output = json!({"playlists":playlists,"resources":resources,"refreshMs":refresh_ms,"mediaSequences":sequences});
        if output.to_string().len() > MAX_OUTPUT_BYTES {
            return Err(failure("capacity", "Live DASH projection exceeds 900 KiB"));
        }
        self.identity = Some(identity);
        self.tracks = ordered;
        self.target_durations = Some(targets);
        Ok(output)
    }
}
