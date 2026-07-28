use std::collections::HashMap;
use std::path::PathBuf;

use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};

use crate::{
    DownloadAssetIndex, DownloadAssetStream, DownloadByteRange, DownloadContentFormat,
    DownloadProfile, DownloadResourceRecord, DownloadSegmentRecord, DownloadSource,
    DownloadStreamKind, PlayerError, PlayerErrorCategory, PlayerErrorCode, PlayerResult,
};

/// Maximum number of segments/clips the planner is willing to materialize from a
/// single manifest. Manifests (HLS media playlists, FLV ffconcat lists, and DASH
/// SegmentTemplate expansions) are untrusted media input; without a cap, a
/// malicious or malformed manifest with millions of `#EXTINF`/URI lines (or a
/// pathological SegmentTemplate duration) could drive `Vec` allocations into the
/// multi-GB range and trigger a probe-request amplification storm (one HEAD per
/// segment). 100k comfortably exceeds any realistic multi-day VOD presentation
/// at sub-second granularity while keeping worst-case memory bounded. This
/// mirrors the cap already enforced in `player-dash-hls-bridge::ops::template_segments`.
const MAX_PLANNED_SEGMENTS: usize = 100_000;

fn ensure_segment_capacity(current_len: usize) -> PlayerResult<()> {
    if current_len >= MAX_PLANNED_SEGMENTS {
        return Err(planning_error(
            PlayerErrorCode::InvalidSource,
            PlayerErrorCategory::Source,
            format!(
                "download planning refused to expand more than {MAX_PLANNED_SEGMENTS} segments from a single manifest"
            ),
        ));
    }
    Ok(())
}

pub trait DownloadPlanningClient {
    fn fetch_text(&self, uri: &str) -> PlayerResult<String>;

    fn content_length(&self, uri: &str) -> PlayerResult<Option<u64>>;
}

#[derive(Debug)]
pub struct DownloadPlanner<C> {
    client: C,
}

impl<C> DownloadPlanner<C> {
    pub fn new(client: C) -> Self {
        Self { client }
    }

    pub fn client(&self) -> &C {
        &self.client
    }

    pub fn into_client(self) -> C {
        self.client
    }
}

impl<C> DownloadPlanner<C>
where
    C: DownloadPlanningClient,
{
    pub fn plan(
        &self,
        source: &DownloadSource,
        profile: &DownloadProfile,
    ) -> PlayerResult<DownloadAssetIndex> {
        match source.content_format {
            DownloadContentFormat::HlsSegments => self.plan_hls(source, profile),
            DownloadContentFormat::DashSegments => self.plan_dash(source, profile),
            DownloadContentFormat::FlvSegments => self.plan_flv_segments(source),
            DownloadContentFormat::SingleFile => self.plan_single_file(source),
            DownloadContentFormat::Unknown => Err(planning_error(
                PlayerErrorCode::Unsupported,
                PlayerErrorCategory::Capability,
                "download planner cannot plan an unknown content format",
            )),
        }
    }

    fn plan_hls(
        &self,
        source: &DownloadSource,
        profile: &DownloadProfile,
    ) -> PlayerResult<DownloadAssetIndex> {
        let manifest_uri = source
            .manifest_uri
            .as_deref()
            .unwrap_or(source.source.uri());
        let manifest = self.client.fetch_text(manifest_uri)?;

        if manifest.contains("#EXT-X-STREAM-INF") {
            self.plan_hls_master(manifest_uri, &manifest, profile)
        } else {
            let media = parse_hls_media_playlist(manifest_uri, &manifest)?;
            build_hls_media_asset_index(self, "index.m3u8", vec![("media", media)])
        }
    }

    fn plan_hls_master(
        &self,
        manifest_uri: &str,
        manifest: &str,
        profile: &DownloadProfile,
    ) -> PlayerResult<DownloadAssetIndex> {
        let master = parse_hls_master_playlist(manifest_uri, manifest)?;
        let variant = select_hls_variant(&master.variants, profile).ok_or_else(|| {
            planning_error(
                PlayerErrorCode::InvalidSource,
                PlayerErrorCategory::Source,
                "HLS master playlist did not contain a playable variant",
            )
        })?;
        let variant_text = self.client.fetch_text(&variant.uri)?;
        let variant_media = parse_hls_media_playlist(&variant.uri, &variant_text)?;

        let mut media = vec![("video", variant_media)];
        let selected_audio = select_hls_audio(&master.audio, profile);
        if let Some(audio) = selected_audio {
            let audio_text = self.client.fetch_text(&audio.uri)?;
            media.push(("audio", parse_hls_media_playlist(&audio.uri, &audio_text)?));
        }

        let mut index = build_hls_media_asset_index(self, "index.m3u8", media)?;
        let media_resource_ids = index
            .resources
            .iter()
            .filter(|resource| {
                resource
                    .relative_path
                    .as_ref()
                    .is_some_and(|path| path.extension().is_some_and(|ext| ext == "m3u8"))
            })
            .filter_map(|resource| {
                resource
                    .relative_path
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .and_then(|name| name.to_str())
                    .map(str::to_owned)
            })
            .filter(|name| name != "index.m3u8")
            .collect::<Vec<_>>();

        let master_text = rewrite_hls_master(&variant.attributes, &media_resource_ids);
        if let Some(master_resource) = index
            .resources
            .iter_mut()
            .find(|resource| resource.resource_id == "hls-master")
        {
            master_resource.generated_text = Some(master_text);
        }
        Ok(index)
    }

    fn plan_dash(
        &self,
        source: &DownloadSource,
        profile: &DownloadProfile,
    ) -> PlayerResult<DownloadAssetIndex> {
        let manifest_uri = source
            .manifest_uri
            .as_deref()
            .unwrap_or(source.source.uri());
        let manifest_text = self.client.fetch_text(manifest_uri)?;
        let manifest = parse_dash_manifest(&manifest_text)?;
        if manifest
            .mpd_type
            .as_deref()
            .is_some_and(|value| !value.eq_ignore_ascii_case("static"))
        {
            return Err(planning_error(
                PlayerErrorCode::Unsupported,
                PlayerErrorCategory::Source,
                "DASH download planning requires a static MPD",
            ));
        }

        let selected = select_dash_representations(&manifest, manifest_uri, profile)?;
        self.build_dash_index(&manifest, &selected)
    }

    fn build_dash_index(
        &self,
        manifest: &DashManifest,
        selected: &[DashSelectedRepresentation],
    ) -> PlayerResult<DownloadAssetIndex> {
        let mut resources = Vec::new();
        let mut segments = Vec::new();
        let mut streams = Vec::new();
        let mut total_size_bytes = 0_u64;

        for item in selected {
            let stream_key = dash_stream_key(item);
            let mut resource_ids = vec!["dash-manifest".to_owned()];
            let mut segment_ids = Vec::new();

            if let Some(template) = item.segment_template.as_ref() {
                if let Some(initialization) = template.initialization.as_deref() {
                    let remote = resolve_uri(
                        &item.base_uri,
                        &expand_dash_template(
                            initialization,
                            &item.representation,
                            template.start_number,
                        ),
                    );
                    let size = self.probe_required_size(&remote, None)?;
                    add_total_size(&mut total_size_bytes, size)?;
                    let resource_id = format!("dash-{stream_key}-init");
                    resource_ids.push(resource_id.clone());
                    resources.push(DownloadResourceRecord {
                        resource_id,
                        uri: remote,
                        relative_path: Some(PathBuf::from(format!(
                            "segments/{stream_key}/init.mp4"
                        ))),
                        byte_range: None,
                        generated_text: None,
                        size_bytes: Some(size),
                        etag: None,
                        checksum: None,
                    });
                }

                for index in 0..item.segment_count {
                    ensure_segment_capacity(segments.len())?;
                    let number = template.start_number.checked_add(index).ok_or_else(|| {
                        planning_error(
                            PlayerErrorCode::InvalidSource,
                            PlayerErrorCategory::Source,
                            "DASH SegmentTemplate segment number overflowed u64",
                        )
                    })?;
                    let remote = resolve_uri(
                        &item.base_uri,
                        &expand_dash_template(&template.media, &item.representation, number),
                    );
                    let size = self.probe_required_size(&remote, None)?;
                    add_total_size(&mut total_size_bytes, size)?;
                    let segment_id = format!("dash-{stream_key}-segment-{number}");
                    segment_ids.push(segment_id.clone());
                    segments.push(DownloadSegmentRecord {
                        segment_id,
                        uri: remote,
                        relative_path: Some(PathBuf::from(format!(
                            "segments/{stream_key}/seg-{number:05}.m4s"
                        ))),
                        sequence: Some(number),
                        byte_range: None,
                        size_bytes: Some(size),
                        checksum: None,
                    });
                }
            } else {
                let remote = item.base_uri.clone();
                let size = self.probe_required_size(&remote, None)?;
                add_total_size(&mut total_size_bytes, size)?;
                let extension = extension_from_uri(&remote, "mp4");
                let resource_id = format!("dash-{stream_key}-media");
                resource_ids.push(resource_id.clone());
                resources.push(DownloadResourceRecord {
                    resource_id,
                    uri: remote,
                    relative_path: Some(PathBuf::from(format!("media/{stream_key}.{extension}"))),
                    byte_range: None,
                    generated_text: None,
                    size_bytes: Some(size),
                    etag: None,
                    checksum: None,
                });
            }

            let mut metadata = HashMap::new();
            metadata.insert(
                "representationId".to_owned(),
                item.representation.id.clone(),
            );
            streams.push(DownloadAssetStream {
                stream_id: stream_key,
                kind: item.kind,
                language: item.language.clone(),
                codec: item.representation.codecs.clone(),
                label: None,
                quality_rank: None,
                resource_ids,
                segment_ids,
                metadata,
            });
        }

        resources.insert(
            0,
            DownloadResourceRecord {
                resource_id: "dash-manifest".to_owned(),
                uri: "vesper-generated://dash/manifest.mpd".to_owned(),
                relative_path: Some(PathBuf::from("manifest.mpd")),
                byte_range: None,
                generated_text: Some(rewrite_dash_mpd(manifest, selected)),
                size_bytes: None,
                etag: None,
                checksum: None,
            },
        );

        Ok(DownloadAssetIndex {
            content_format: DownloadContentFormat::DashSegments,
            total_size_bytes: Some(total_size_bytes),
            resources,
            segments,
            streams,
            ..DownloadAssetIndex::default()
        })
    }

    fn plan_flv_segments(&self, source: &DownloadSource) -> PlayerResult<DownloadAssetIndex> {
        let uri = source
            .manifest_uri
            .as_deref()
            .unwrap_or(source.source.uri());
        let clip_uris = if extension_from_uri(uri, "flv").eq_ignore_ascii_case("flv") {
            vec![uri.to_owned()]
        } else {
            parse_flv_clip_manifest(uri, &self.client.fetch_text(uri)?)?
        };

        if clip_uris.is_empty() {
            return Err(planning_error(
                PlayerErrorCode::InvalidSource,
                PlayerErrorCategory::Source,
                "FLV clip manifest did not contain any clip URI",
            ));
        }

        let mut total_size_bytes = 0_u64;
        let mut concat = String::from("ffconcat version 1.0\n");
        let mut segments = Vec::with_capacity(clip_uris.len());
        for (index, clip_uri) in clip_uris.iter().enumerate() {
            ensure_segment_capacity(segments.len())?;
            let size = self.probe_required_size(clip_uri, None)?;
            add_total_size(&mut total_size_bytes, size)?;
            let sequence = index as u64 + 1;
            let local_path = PathBuf::from(format!(
                "clips/clip-{sequence:05}.{}",
                extension_from_uri(clip_uri, "flv")
            ));
            concat.push_str(&format!(
                "file '{}'\n",
                escape_ffconcat_path(&local_path.to_string_lossy())
            ));
            segments.push(DownloadSegmentRecord {
                segment_id: format!("flv-clip-{sequence}"),
                uri: clip_uri.clone(),
                relative_path: Some(local_path),
                sequence: Some(sequence),
                byte_range: None,
                size_bytes: Some(size),
                checksum: None,
            });
        }

        Ok(DownloadAssetIndex {
            content_format: DownloadContentFormat::FlvSegments,
            total_size_bytes: Some(total_size_bytes),
            resources: vec![DownloadResourceRecord {
                resource_id: "flv-concat".to_owned(),
                uri: "vesper-generated://flv/manifest.ffconcat".to_owned(),
                relative_path: Some(PathBuf::from("manifest.ffconcat")),
                byte_range: None,
                generated_text: Some(concat),
                size_bytes: None,
                etag: None,
                checksum: None,
            }],
            segments,
            ..DownloadAssetIndex::default()
        })
    }

    fn plan_single_file(&self, source: &DownloadSource) -> PlayerResult<DownloadAssetIndex> {
        let uri = source
            .manifest_uri
            .as_deref()
            .unwrap_or(source.source.uri());
        let size = self.probe_required_size(uri, None)?;
        Ok(DownloadAssetIndex {
            content_format: DownloadContentFormat::SingleFile,
            total_size_bytes: Some(size),
            resources: vec![DownloadResourceRecord {
                resource_id: "single-file".to_owned(),
                uri: uri.to_owned(),
                relative_path: Some(PathBuf::from(format!(
                    "media.{}",
                    extension_from_uri(uri, "bin")
                ))),
                byte_range: None,
                generated_text: None,
                size_bytes: Some(size),
                etag: None,
                checksum: None,
            }],
            ..DownloadAssetIndex::default()
        })
    }

    fn probe_required_size(
        &self,
        uri: &str,
        byte_range: Option<DownloadByteRange>,
    ) -> PlayerResult<u64> {
        if let Some(byte_range) = byte_range {
            return Ok(byte_range.length);
        }
        self.client.content_length(uri)?.ok_or_else(|| {
            planning_error(
                PlayerErrorCode::InvalidSource,
                PlayerErrorCategory::Network,
                format!("remote resource `{uri}` did not expose a stable content length"),
            )
        })
    }
}

fn build_hls_media_asset_index<C>(
    planner: &DownloadPlanner<C>,
    manifest_path: &str,
    media_playlists: Vec<(&str, HlsMediaPlaylist)>,
) -> PlayerResult<DownloadAssetIndex>
where
    C: DownloadPlanningClient,
{
    let mut resources = vec![DownloadResourceRecord {
        resource_id: "hls-master".to_owned(),
        uri: format!("vesper-generated://hls/{manifest_path}"),
        relative_path: Some(PathBuf::from(manifest_path)),
        byte_range: None,
        generated_text: None,
        size_bytes: None,
        etag: None,
        checksum: None,
    }];
    let mut segments = Vec::new();
    let mut streams = Vec::new();
    let mut map_resources = HashMap::<String, (String, PathBuf)>::new();
    let mut total_size_bytes = 0_u64;

    for (media_id, playlist) in &media_playlists {
        let mut stream_resource_ids = Vec::new();
        let mut stream_segment_ids = Vec::new();
        let playlist_path = if media_playlists.len() == 1 && manifest_path == "index.m3u8" {
            PathBuf::from("index.m3u8")
        } else {
            PathBuf::from(format!("{media_id}.m3u8"))
        };
        let mut local_maps = HashMap::<String, PathBuf>::new();
        for (map_index, map) in playlist.maps.iter().enumerate() {
            let key = format!("{}:{:?}", map.uri, map.byte_range);
            if let Some((resource_id, relative_path)) = map_resources.get(&key) {
                local_maps.insert(key, relative_path.clone());
                stream_resource_ids.push(resource_id.clone());
            } else {
                let size = planner.probe_required_size(&map.uri, map.byte_range)?;
                add_total_size(&mut total_size_bytes, size)?;
                let relative_path = PathBuf::from(format!(
                    "segments/{media_id}-init-{map_index}.{}",
                    extension_from_uri(&map.uri, "mp4")
                ));
                let resource_id = format!("hls-{media_id}-init-{map_index}");
                resources.push(DownloadResourceRecord {
                    resource_id: resource_id.clone(),
                    uri: map.uri.clone(),
                    relative_path: Some(relative_path.clone()),
                    byte_range: map.byte_range,
                    generated_text: None,
                    size_bytes: Some(size),
                    etag: None,
                    checksum: None,
                });
                stream_resource_ids.push(resource_id.clone());
                map_resources.insert(key.clone(), (resource_id, relative_path.clone()));
                local_maps.insert(key, relative_path);
            }
        }

        for segment in &playlist.segments {
            ensure_segment_capacity(segments.len())?;
            let size = planner.probe_required_size(&segment.uri, segment.byte_range)?;
            add_total_size(&mut total_size_bytes, size)?;
            let segment_id = format!("hls-{media_id}-{}", segment.sequence);
            segments.push(DownloadSegmentRecord {
                segment_id: segment_id.clone(),
                uri: segment.uri.clone(),
                relative_path: Some(PathBuf::from(format!(
                    "segments/{media_id}-{:05}.{}",
                    segment.sequence,
                    extension_from_uri(&segment.uri, "ts")
                ))),
                sequence: Some(segment.sequence),
                byte_range: segment.byte_range,
                size_bytes: Some(size),
                checksum: None,
            });
            stream_segment_ids.push(segment_id);
        }

        let media_text = rewrite_hls_media(media_id, playlist, &local_maps);
        let playlist_resource_id = format!("hls-{media_id}-playlist");
        resources.push(DownloadResourceRecord {
            resource_id: playlist_resource_id.clone(),
            uri: format!("vesper-generated://hls/{}", playlist_path.display()),
            relative_path: Some(playlist_path),
            byte_range: None,
            generated_text: Some(media_text),
            size_bytes: None,
            etag: None,
            checksum: None,
        });
        stream_resource_ids.push(playlist_resource_id);
        streams.push(DownloadAssetStream {
            stream_id: (*media_id).to_owned(),
            kind: hls_stream_kind(media_id, media_playlists.len()),
            language: None,
            codec: None,
            label: Some((*media_id).to_owned()),
            quality_rank: None,
            resource_ids: stream_resource_ids,
            segment_ids: stream_segment_ids,
            metadata: HashMap::new(),
        });
    }

    if media_playlists.len() == 1
        && let Some(media_playlist) = resources
            .iter()
            .position(|resource| resource.resource_id.ends_with("-playlist"))
    {
        let media_resource = resources.remove(media_playlist);
        let media_resource_id = media_resource.resource_id;
        resources[0].generated_text = media_resource.generated_text;
        for stream in &mut streams {
            for resource_id in &mut stream.resource_ids {
                if resource_id == &media_resource_id {
                    *resource_id = "hls-master".to_owned();
                }
            }
        }
    }

    Ok(DownloadAssetIndex {
        content_format: DownloadContentFormat::HlsSegments,
        total_size_bytes: Some(total_size_bytes),
        resources,
        segments,
        streams,
        ..DownloadAssetIndex::default()
    })
}

fn add_total_size(total_size_bytes: &mut u64, size: u64) -> PlayerResult<()> {
    *total_size_bytes = total_size_bytes.checked_add(size).ok_or_else(|| {
        planning_error(
            PlayerErrorCode::InvalidSource,
            PlayerErrorCategory::Source,
            "download asset size exceeds u64 range",
        )
    })?;
    Ok(())
}

fn hls_stream_kind(media_id: &str, media_count: usize) -> DownloadStreamKind {
    if media_count == 1 {
        return DownloadStreamKind::Combined;
    }
    if media_id.eq_ignore_ascii_case("audio") {
        DownloadStreamKind::Audio
    } else if media_id.eq_ignore_ascii_case("video") {
        DownloadStreamKind::Video
    } else {
        DownloadStreamKind::Auxiliary
    }
}

#[derive(Debug, Clone)]
struct HlsMasterPlaylist {
    variants: Vec<HlsVariant>,
    audio: Vec<HlsRendition>,
}

#[derive(Debug, Clone)]
struct HlsVariant {
    uri: String,
    attributes: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct HlsRendition {
    uri: String,
    attributes: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct HlsMediaPlaylist {
    target_duration: Option<String>,
    version: Option<String>,
    maps: Vec<HlsMap>,
    segments: Vec<HlsSegment>,
}

#[derive(Debug, Clone)]
struct HlsMap {
    uri: String,
    byte_range: Option<DownloadByteRange>,
}

#[derive(Debug, Clone)]
struct HlsSegment {
    uri: String,
    duration: Option<String>,
    byte_range: Option<DownloadByteRange>,
    sequence: u64,
}

fn parse_hls_master_playlist(
    manifest_uri: &str,
    manifest: &str,
) -> PlayerResult<HlsMasterPlaylist> {
    let mut variants = Vec::new();
    let mut audio = Vec::new();
    let mut pending_variant = None;

    for line in manifest
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(attributes) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            pending_variant = Some(parse_hls_attributes(attributes));
            continue;
        }
        if let Some(attributes) = line.strip_prefix("#EXT-X-MEDIA:") {
            let attributes = parse_hls_attributes(attributes);
            if attributes
                .get("TYPE")
                .is_some_and(|kind| kind.eq_ignore_ascii_case("AUDIO"))
                && let Some(uri) = attributes.get("URI")
            {
                audio.push(HlsRendition {
                    uri: resolve_uri(manifest_uri, uri),
                    attributes,
                });
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if let Some(attributes) = pending_variant.take() {
            variants.push(HlsVariant {
                uri: resolve_uri(manifest_uri, line),
                attributes,
            });
        }
    }

    Ok(HlsMasterPlaylist { variants, audio })
}

fn parse_hls_media_playlist(manifest_uri: &str, manifest: &str) -> PlayerResult<HlsMediaPlaylist> {
    let mut target_duration = None;
    let mut version = None;
    let mut end_list = false;
    let mut playlist_type_vod = false;
    let mut maps = Vec::new();
    let mut segments = Vec::new();
    let mut pending_duration = None;
    let mut pending_byte_range = None;
    let mut previous_map_range_end = 0_u64;
    let mut previous_segment_range_end = 0_u64;
    let mut sequence = 0_u64;

    for line in manifest
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(value) = line.strip_prefix("#EXT-X-TARGETDURATION:") {
            target_duration = Some(value.trim().to_owned());
            continue;
        }
        if let Some(value) = line.strip_prefix("#EXT-X-VERSION:") {
            version = Some(value.trim().to_owned());
            continue;
        }
        if let Some(value) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            sequence = parse_hls_media_sequence(value.trim())?;
            continue;
        }
        if line == "#EXT-X-ENDLIST" {
            end_list = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("#EXT-X-PLAYLIST-TYPE:") {
            playlist_type_vod = value.trim().eq_ignore_ascii_case("VOD");
            continue;
        }
        if let Some(value) = line.strip_prefix("#EXT-X-MAP:") {
            let attributes = parse_hls_attributes(value);
            let Some(uri) = attributes.get("URI") else {
                return Err(planning_error(
                    PlayerErrorCode::InvalidSource,
                    PlayerErrorCategory::Source,
                    "HLS EXT-X-MAP was missing URI",
                ));
            };
            let byte_range = attributes
                .get("BYTERANGE")
                .and_then(|value| parse_hls_byte_range(value, &mut previous_map_range_end));
            maps.push(HlsMap {
                uri: resolve_uri(manifest_uri, uri),
                byte_range,
            });
            continue;
        }
        if let Some(value) = line.strip_prefix("#EXT-X-BYTERANGE:") {
            pending_byte_range =
                parse_hls_byte_range(value.trim(), &mut previous_segment_range_end);
            continue;
        }
        if let Some(value) = line.strip_prefix("#EXTINF:") {
            pending_duration = Some(
                value
                    .split_once(',')
                    .map(|(duration, _)| duration)
                    .unwrap_or(value)
                    .trim()
                    .to_owned(),
            );
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        ensure_segment_capacity(segments.len())?;
        segments.push(HlsSegment {
            uri: resolve_uri(manifest_uri, line),
            duration: pending_duration.take(),
            byte_range: pending_byte_range.take(),
            sequence,
        });
        sequence = next_hls_media_sequence(sequence)?;
    }

    if !end_list && !playlist_type_vod {
        return Err(planning_error(
            PlayerErrorCode::Unsupported,
            PlayerErrorCategory::Source,
            "HLS download planning requires a VOD playlist or EXT-X-ENDLIST",
        ));
    }
    if segments.is_empty() {
        return Err(planning_error(
            PlayerErrorCode::InvalidSource,
            PlayerErrorCategory::Source,
            "HLS media playlist did not contain any segments",
        ));
    }

    Ok(HlsMediaPlaylist {
        target_duration,
        version,
        maps,
        segments,
    })
}

fn select_hls_variant<'a>(
    variants: &'a [HlsVariant],
    profile: &DownloadProfile,
) -> Option<&'a HlsVariant> {
    profile
        .variant_id
        .as_deref()
        .and_then(|variant_id| {
            variants.iter().find(|variant| {
                variant.uri == variant_id
                    || variant
                        .attributes
                        .get("NAME")
                        .is_some_and(|name| name == variant_id)
            })
        })
        .or_else(|| variants.first())
}

fn select_hls_audio<'a>(
    audio: &'a [HlsRendition],
    profile: &DownloadProfile,
) -> Option<&'a HlsRendition> {
    profile
        .preferred_audio_language
        .as_deref()
        .and_then(|language| {
            audio.iter().find(|rendition| {
                rendition
                    .attributes
                    .get("LANGUAGE")
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(language))
            })
        })
        .or_else(|| {
            audio.iter().find(|rendition| {
                rendition
                    .attributes
                    .get("DEFAULT")
                    .is_some_and(|value| value.eq_ignore_ascii_case("YES"))
            })
        })
        .or_else(|| audio.first())
}

fn rewrite_hls_master(
    variant_attributes: &HashMap<String, String>,
    media_resource_ids: &[String],
) -> String {
    let audio_playlist = media_resource_ids
        .iter()
        .find(|path| path.starts_with("audio"))
        .cloned();
    let video_playlist = media_resource_ids
        .iter()
        .find(|path| path.starts_with("video"))
        .or_else(|| media_resource_ids.first())
        .cloned()
        .unwrap_or_else(|| "video.m3u8".to_owned());

    let bandwidth = variant_attributes
        .get("BANDWIDTH")
        .cloned()
        .unwrap_or_else(|| "1".to_owned());
    let mut text = "#EXTM3U\n#EXT-X-VERSION:3\n".to_owned();
    if let Some(audio_playlist) = audio_playlist.as_deref() {
        text.push_str(
            "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio\",NAME=\"audio\",DEFAULT=YES,AUTOSELECT=YES,URI=\"",
        );
        text.push_str(audio_playlist);
        text.push_str("\"\n");
        text.push_str(&format!(
            "#EXT-X-STREAM-INF:BANDWIDTH={bandwidth},AUDIO=\"audio\"\n"
        ));
    } else {
        text.push_str(&format!("#EXT-X-STREAM-INF:BANDWIDTH={bandwidth}\n"));
    }
    text.push_str(&video_playlist);
    text.push('\n');
    text
}

fn rewrite_hls_media(
    media_id: &str,
    playlist: &HlsMediaPlaylist,
    local_maps: &HashMap<String, PathBuf>,
) -> String {
    let mut text = "#EXTM3U\n".to_owned();
    text.push_str(&format!(
        "#EXT-X-VERSION:{}\n",
        playlist.version.as_deref().unwrap_or("3")
    ));
    text.push_str("#EXT-X-PLAYLIST-TYPE:VOD\n");
    if let Some(target_duration) = playlist.target_duration.as_deref() {
        text.push_str(&format!("#EXT-X-TARGETDURATION:{target_duration}\n"));
    }
    if let Some(map) = playlist.maps.last() {
        let key = format!("{}:{:?}", map.uri, map.byte_range);
        if let Some(path) = local_maps.get(&key) {
            text.push_str(&format!("#EXT-X-MAP:URI=\"{}\"\n", path.display()));
        }
    }
    for segment in &playlist.segments {
        text.push_str(&format!(
            "#EXTINF:{},\nsegments/{media_id}-{:05}.{}\n",
            segment.duration.as_deref().unwrap_or("0"),
            segment.sequence,
            extension_from_uri(&segment.uri, "ts")
        ));
    }
    text.push_str("#EXT-X-ENDLIST\n");
    text
}

fn parse_hls_attributes(input: &str) -> HashMap<String, String> {
    split_quoted(input, ',')
        .into_iter()
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((
                key.trim().to_owned(),
                value.trim().trim_matches('"').to_owned(),
            ))
        })
        .collect()
}

fn parse_hls_byte_range(value: &str, previous_range_end: &mut u64) -> Option<DownloadByteRange> {
    let (length, offset) = value
        .split_once('@')
        .map(|(length, offset)| (length.trim(), Some(offset.trim())))
        .unwrap_or((value.trim(), None));
    let length = length.parse::<u64>().ok()?;
    let offset = offset
        .and_then(|offset| offset.parse::<u64>().ok())
        .unwrap_or(*previous_range_end);
    *previous_range_end = offset.saturating_add(length);
    Some(DownloadByteRange { offset, length })
}

fn parse_hls_media_sequence(value: &str) -> PlayerResult<u64> {
    value.parse::<u64>().map_err(|_| {
        planning_error(
            PlayerErrorCode::InvalidSource,
            PlayerErrorCategory::Source,
            "HLS EXT-X-MEDIA-SEQUENCE must be a non-negative integer",
        )
    })
}

fn next_hls_media_sequence(sequence: u64) -> PlayerResult<u64> {
    sequence.checked_add(1).ok_or_else(|| {
        planning_error(
            PlayerErrorCode::InvalidSource,
            PlayerErrorCategory::Source,
            "HLS EXT-X-MEDIA-SEQUENCE overflowed u64",
        )
    })
}

#[derive(Debug, Clone, Default)]
struct DashManifest {
    mpd_type: Option<String>,
    duration_text: Option<String>,
    base_url: Option<String>,
    segment_template: DashSegmentTemplateFields,
    has_segment_base: bool,
    periods: Vec<DashPeriod>,
}

#[derive(Debug, Clone, Default)]
struct DashPeriod {
    base_url: Option<String>,
    segment_template: DashSegmentTemplateFields,
    has_segment_base: bool,
    adaptation_sets: Vec<DashAdaptationSet>,
}

#[derive(Debug, Clone, Default)]
struct DashAdaptationSet {
    content_type: Option<String>,
    mime_type: Option<String>,
    language: Option<String>,
    base_url: Option<String>,
    segment_template: DashSegmentTemplateFields,
    has_segment_base: bool,
    representations: Vec<DashRepresentation>,
}

#[derive(Debug, Clone, Default)]
struct DashRepresentation {
    id: String,
    bandwidth: Option<String>,
    mime_type: Option<String>,
    codecs: Option<String>,
    base_url: Option<String>,
    segment_template: DashSegmentTemplateFields,
    has_segment_base: bool,
}

#[derive(Debug, Clone, Default)]
struct DashSegmentTemplateFields {
    media: Option<String>,
    initialization: Option<String>,
    start_number: Option<u64>,
    timescale: Option<u64>,
    duration: Option<u64>,
}

impl DashSegmentTemplateFields {
    fn merged_with(&self, child: &Self) -> Self {
        Self {
            media: child.media.clone().or_else(|| self.media.clone()),
            initialization: child
                .initialization
                .clone()
                .or_else(|| self.initialization.clone()),
            start_number: child.start_number.or(self.start_number),
            timescale: child.timescale.or(self.timescale),
            duration: child.duration.or(self.duration),
        }
    }

    fn effective(&self) -> Option<DashSegmentTemplate> {
        Some(DashSegmentTemplate {
            media: self.media.clone()?,
            initialization: self.initialization.clone(),
            start_number: self.start_number.unwrap_or(1),
            timescale: self.timescale.unwrap_or(1),
            duration: self.duration.unwrap_or(0),
        })
    }
}

#[derive(Debug, Clone)]
struct DashSegmentTemplate {
    media: String,
    initialization: Option<String>,
    start_number: u64,
    timescale: u64,
    duration: u64,
}

#[derive(Debug, Clone)]
struct DashSelectedRepresentation {
    adaptation_index: usize,
    kind: DownloadStreamKind,
    language: Option<String>,
    mime_type: Option<String>,
    representation: DashRepresentation,
    base_uri: String,
    segment_template: Option<DashSegmentTemplate>,
    segment_count: u64,
}

#[derive(Debug, Clone, Copy)]
enum DashBaseUrlTarget {
    Manifest,
    Period,
    AdaptationSet,
    Representation,
}

fn parse_dash_manifest(input: &str) -> PlayerResult<DashManifest> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(true);
    let mut manifest = DashManifest::default();
    let mut current_period = None;
    let mut current_adaptation = None;
    let mut current_representation = None;
    let mut base_url_capture: Option<(DashBaseUrlTarget, String)> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => process_dash_open_element(
                &reader,
                &start,
                false,
                &mut manifest,
                &mut current_period,
                &mut current_adaptation,
                &mut current_representation,
                &mut base_url_capture,
            )?,
            Ok(Event::Empty(start)) => process_dash_open_element(
                &reader,
                &start,
                true,
                &mut manifest,
                &mut current_period,
                &mut current_adaptation,
                &mut current_representation,
                &mut base_url_capture,
            )?,
            Ok(Event::Text(text)) => {
                if let Some((_, value)) = base_url_capture.as_mut() {
                    let decoded = text
                        .decode()
                        .map_err(|error| dash_xml_error(error.to_string()))?;
                    let unescaped = quick_xml::escape::unescape(decoded.as_ref())
                        .map_err(|error| dash_xml_error(error.to_string()))?;
                    value.push_str(unescaped.as_ref());
                }
            }
            Ok(Event::CData(text)) => {
                if let Some((_, value)) = base_url_capture.as_mut() {
                    let decoded = text
                        .decode()
                        .map_err(|error| dash_xml_error(error.to_string()))?;
                    value.push_str(decoded.as_ref());
                }
            }
            Ok(Event::End(end)) => match end.local_name().as_ref() {
                b"BaseURL" => {
                    if let Some((target, value)) = base_url_capture.take() {
                        assign_dash_base_url(
                            target,
                            value,
                            &mut manifest,
                            &mut current_period,
                            &mut current_adaptation,
                            &mut current_representation,
                        )?;
                    }
                }
                b"Representation" => {
                    let representation = current_representation.take().ok_or_else(|| {
                        dash_xml_error("unexpected closing Representation element")
                    })?;
                    current_adaptation
                        .as_mut()
                        .ok_or_else(|| dash_xml_error("Representation is outside AdaptationSet"))?
                        .representations
                        .push(representation);
                }
                b"AdaptationSet" => {
                    let adaptation = current_adaptation.take().ok_or_else(|| {
                        dash_xml_error("unexpected closing AdaptationSet element")
                    })?;
                    current_period
                        .as_mut()
                        .ok_or_else(|| dash_xml_error("AdaptationSet is outside Period"))?
                        .adaptation_sets
                        .push(adaptation);
                }
                b"Period" => {
                    let period = current_period
                        .take()
                        .ok_or_else(|| dash_xml_error("unexpected closing Period element"))?;
                    manifest.periods.push(period);
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(dash_xml_error(error.to_string())),
        }
    }

    if current_period.is_some()
        || current_adaptation.is_some()
        || current_representation.is_some()
        || base_url_capture.is_some()
    {
        return Err(dash_xml_error("DASH MPD ended with an incomplete element"));
    }
    Ok(manifest)
}

#[allow(clippy::too_many_arguments)]
fn process_dash_open_element(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    is_empty: bool,
    manifest: &mut DashManifest,
    current_period: &mut Option<DashPeriod>,
    current_adaptation: &mut Option<DashAdaptationSet>,
    current_representation: &mut Option<DashRepresentation>,
    base_url_capture: &mut Option<(DashBaseUrlTarget, String)>,
) -> PlayerResult<()> {
    match start.local_name().as_ref() {
        b"MPD" => {
            manifest.mpd_type = dash_event_attribute(reader, start, b"type")?;
            manifest.duration_text =
                dash_event_attribute(reader, start, b"mediaPresentationDuration")?;
        }
        b"Period" => {
            if current_period.is_some() {
                return Err(dash_xml_error(
                    "nested DASH Period elements are not supported",
                ));
            }
            let period = DashPeriod::default();
            if is_empty {
                manifest.periods.push(period);
            } else {
                *current_period = Some(period);
            }
        }
        b"AdaptationSet" => {
            if current_adaptation.is_some() {
                return Err(dash_xml_error(
                    "nested DASH AdaptationSet elements are not supported",
                ));
            }
            if current_period.is_none() {
                return Err(dash_xml_error("AdaptationSet is outside Period"));
            }
            let adaptation = DashAdaptationSet {
                content_type: dash_event_attribute(reader, start, b"contentType")?,
                mime_type: dash_event_attribute(reader, start, b"mimeType")?,
                language: dash_event_attribute(reader, start, b"lang")?,
                ..DashAdaptationSet::default()
            };
            if is_empty {
                current_period
                    .as_mut()
                    .ok_or_else(|| dash_xml_error("AdaptationSet is outside Period"))?
                    .adaptation_sets
                    .push(adaptation);
            } else {
                *current_adaptation = Some(adaptation);
            }
        }
        b"Representation" => {
            if current_representation.is_some() {
                return Err(dash_xml_error(
                    "nested DASH Representation elements are not supported",
                ));
            }
            let representation_index = current_adaptation
                .as_ref()
                .ok_or_else(|| dash_xml_error("Representation is outside AdaptationSet"))?
                .representations
                .len();
            let representation = DashRepresentation {
                id: dash_event_attribute(reader, start, b"id")?
                    .unwrap_or_else(|| representation_index.to_string()),
                bandwidth: dash_event_attribute(reader, start, b"bandwidth")?,
                mime_type: dash_event_attribute(reader, start, b"mimeType")?,
                codecs: dash_event_attribute(reader, start, b"codecs")?,
                ..DashRepresentation::default()
            };
            if is_empty {
                current_adaptation
                    .as_mut()
                    .ok_or_else(|| dash_xml_error("Representation is outside AdaptationSet"))?
                    .representations
                    .push(representation);
            } else {
                *current_representation = Some(representation);
            }
        }
        b"BaseURL" if !is_empty => {
            let target = if current_representation.is_some() {
                DashBaseUrlTarget::Representation
            } else if current_adaptation.is_some() {
                DashBaseUrlTarget::AdaptationSet
            } else if current_period.is_some() {
                DashBaseUrlTarget::Period
            } else {
                DashBaseUrlTarget::Manifest
            };
            *base_url_capture = Some((target, String::new()));
        }
        b"SegmentTemplate" => {
            let fields = DashSegmentTemplateFields {
                media: dash_event_attribute(reader, start, b"media")?,
                initialization: dash_event_attribute(reader, start, b"initialization")?,
                start_number: dash_u64_attribute(reader, start, b"startNumber")?,
                timescale: dash_u64_attribute(reader, start, b"timescale")?,
                duration: dash_u64_attribute(reader, start, b"duration")?,
            };
            if let Some(representation) = current_representation.as_mut() {
                representation.segment_template = fields;
            } else if let Some(adaptation) = current_adaptation.as_mut() {
                adaptation.segment_template = fields;
            } else if let Some(period) = current_period.as_mut() {
                period.segment_template = fields;
            } else {
                manifest.segment_template = fields;
            }
        }
        b"SegmentBase" => {
            if let Some(representation) = current_representation.as_mut() {
                representation.has_segment_base = true;
            } else if let Some(adaptation) = current_adaptation.as_mut() {
                adaptation.has_segment_base = true;
            } else if let Some(period) = current_period.as_mut() {
                period.has_segment_base = true;
            } else {
                manifest.has_segment_base = true;
            }
        }
        _ => {}
    }
    Ok(())
}

fn dash_event_attribute(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    name: &[u8],
) -> PlayerResult<Option<String>> {
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|error| dash_xml_error(error.to_string()))?;
        if attribute.key.local_name().as_ref() == name {
            return attribute
                .decode_and_unescape_value(reader.decoder())
                .map(|value| Some(value.into_owned()))
                .map_err(|error| dash_xml_error(error.to_string()));
        }
    }
    Ok(None)
}

fn dash_u64_attribute(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    name: &[u8],
) -> PlayerResult<Option<u64>> {
    let Some(value) = dash_event_attribute(reader, start, name)? else {
        return Ok(None);
    };
    value.parse::<u64>().map(Some).map_err(|_| {
        dash_xml_error(format!(
            "DASH {} attribute must be a non-negative integer",
            String::from_utf8_lossy(name)
        ))
    })
}

fn assign_dash_base_url(
    target: DashBaseUrlTarget,
    value: String,
    manifest: &mut DashManifest,
    current_period: &mut Option<DashPeriod>,
    current_adaptation: &mut Option<DashAdaptationSet>,
    current_representation: &mut Option<DashRepresentation>,
) -> PlayerResult<()> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(());
    }
    let slot = match target {
        DashBaseUrlTarget::Manifest => &mut manifest.base_url,
        DashBaseUrlTarget::Period => {
            &mut current_period
                .as_mut()
                .ok_or_else(|| dash_xml_error("Period BaseURL is outside Period"))?
                .base_url
        }
        DashBaseUrlTarget::AdaptationSet => {
            &mut current_adaptation
                .as_mut()
                .ok_or_else(|| dash_xml_error("AdaptationSet BaseURL is outside AdaptationSet"))?
                .base_url
        }
        DashBaseUrlTarget::Representation => {
            &mut current_representation
                .as_mut()
                .ok_or_else(|| dash_xml_error("Representation BaseURL is outside Representation"))?
                .base_url
        }
    };
    if slot.is_none() {
        *slot = Some(value.to_owned());
    }
    Ok(())
}

fn dash_xml_error(message: impl Into<String>) -> PlayerError {
    planning_error(
        PlayerErrorCode::InvalidSource,
        PlayerErrorCategory::Source,
        format!("invalid DASH MPD: {}", message.into()),
    )
}

fn select_dash_representations(
    manifest: &DashManifest,
    manifest_uri: &str,
    profile: &DownloadProfile,
) -> PlayerResult<Vec<DashSelectedRepresentation>> {
    if manifest.periods.len() != 1 {
        return Err(planning_error(
            PlayerErrorCode::Unsupported,
            PlayerErrorCategory::Source,
            "DASH download planning currently requires exactly one Period",
        ));
    }
    let period = &manifest.periods[0];
    let manifest_base = resolve_optional_dash_base(manifest_uri, manifest.base_url.as_deref());
    let period_base = resolve_optional_dash_base(&manifest_base, period.base_url.as_deref());
    let period_template = manifest
        .segment_template
        .merged_with(&period.segment_template);
    let duration_seconds = manifest
        .duration_text
        .as_deref()
        .and_then(parse_iso8601_duration_seconds);
    let mut total_segment_count = 0_u64;
    let mut selected = Vec::new();

    for (adaptation_index, adaptation) in period.adaptation_sets.iter().enumerate() {
        let Some(kind) = dash_adaptation_stream_kind(adaptation) else {
            continue;
        };
        let Some(representation) = select_dash_adaptation_representation(adaptation, kind, profile)
        else {
            return Err(planning_error(
                PlayerErrorCode::InvalidSource,
                PlayerErrorCategory::Source,
                "DASH audio/video AdaptationSet did not contain a Representation",
            ));
        };
        let adaptation_template = period_template.merged_with(&adaptation.segment_template);
        let segment_template = adaptation_template
            .merged_with(&representation.segment_template)
            .effective();
        let has_segment_base = manifest.has_segment_base
            || period.has_segment_base
            || adaptation.has_segment_base
            || representation.has_segment_base;
        let has_explicit_base_url = manifest.base_url.is_some()
            || period.base_url.is_some()
            || adaptation.base_url.is_some()
            || representation.base_url.is_some();
        if segment_template.is_none() && !(has_segment_base && has_explicit_base_url) {
            return Err(planning_error(
                PlayerErrorCode::Unsupported,
                PlayerErrorCategory::Source,
                format!(
                    "DASH Representation `{}` does not provide a supported SegmentTemplate or SegmentBase",
                    representation.id
                ),
            ));
        }

        let adaptation_base =
            resolve_optional_dash_base(&period_base, adaptation.base_url.as_deref());
        let base_uri =
            resolve_optional_dash_base(&adaptation_base, representation.base_url.as_deref());
        let segment_count = if let Some(template) = segment_template.as_ref() {
            if template.duration == 0 || template.timescale == 0 {
                return Err(planning_error(
                    PlayerErrorCode::InvalidSource,
                    PlayerErrorCategory::Source,
                    "DASH SegmentTemplate duration and timescale must be greater than zero",
                ));
            }
            let duration_seconds = duration_seconds.ok_or_else(|| {
                planning_error(
                    PlayerErrorCode::InvalidSource,
                    PlayerErrorCategory::Source,
                    "DASH SegmentTemplate planning requires a finite MPD duration",
                )
            })?;
            let segment_seconds = template.duration as f64 / template.timescale as f64;
            dash_segment_count(duration_seconds, segment_seconds)?
        } else {
            0
        };
        total_segment_count = total_segment_count
            .checked_add(segment_count)
            .ok_or_else(|| {
                planning_error(
                    PlayerErrorCode::InvalidSource,
                    PlayerErrorCategory::Source,
                    "DASH aggregate segment count overflowed u64",
                )
            })?;
        if total_segment_count > MAX_PLANNED_SEGMENTS as u64 {
            return Err(planning_error(
                PlayerErrorCode::InvalidSource,
                PlayerErrorCategory::Source,
                format!(
                    "DASH download planning refused to expand more than {MAX_PLANNED_SEGMENTS} segments across selected representations"
                ),
            ));
        }

        selected.push(DashSelectedRepresentation {
            adaptation_index,
            kind,
            language: adaptation.language.clone(),
            mime_type: representation
                .mime_type
                .clone()
                .or_else(|| adaptation.mime_type.clone()),
            representation: representation.clone(),
            base_uri,
            segment_template,
            segment_count,
        });
    }

    if selected.is_empty() {
        return Err(planning_error(
            PlayerErrorCode::InvalidSource,
            PlayerErrorCategory::Source,
            "DASH MPD did not contain a supported audio/video Representation",
        ));
    }
    Ok(selected)
}

fn select_dash_adaptation_representation<'a>(
    adaptation: &'a DashAdaptationSet,
    kind: DownloadStreamKind,
    profile: &DownloadProfile,
) -> Option<&'a DashRepresentation> {
    if kind == DownloadStreamKind::Video
        && let Some(variant_id) = profile.variant_id.as_deref()
        && let Some(representation) = adaptation
            .representations
            .iter()
            .find(|representation| representation.id == variant_id)
    {
        return Some(representation);
    }
    adaptation.representations.first()
}

fn dash_adaptation_stream_kind(adaptation: &DashAdaptationSet) -> Option<DownloadStreamKind> {
    for value in adaptation
        .content_type
        .iter()
        .chain(adaptation.mime_type.iter())
        .chain(
            adaptation
                .representations
                .iter()
                .filter_map(|representation| representation.mime_type.as_ref()),
        )
    {
        let value = value.to_ascii_lowercase();
        if value == "video" || value.starts_with("video/") {
            return Some(DownloadStreamKind::Video);
        }
        if value == "audio" || value.starts_with("audio/") {
            return Some(DownloadStreamKind::Audio);
        }
        if value == "text" || value.starts_with("text/") {
            return None;
        }
    }
    for codecs in adaptation
        .representations
        .iter()
        .filter_map(|representation| representation.codecs.as_deref())
    {
        let codecs = codecs.to_ascii_lowercase();
        if ["mp4a", "opus", "vorbis", "ac-3", "ec-3"]
            .iter()
            .any(|codec| codecs.contains(codec))
        {
            return Some(DownloadStreamKind::Audio);
        }
        if ["avc", "hvc", "hev", "av01", "vp9", "vp09"]
            .iter()
            .any(|codec| codecs.contains(codec))
        {
            return Some(DownloadStreamKind::Video);
        }
        if codecs.contains("wvtt") || codecs.contains("stpp") {
            return None;
        }
    }
    Some(DownloadStreamKind::Video)
}

fn resolve_optional_dash_base(base: &str, child: Option<&str>) -> String {
    child
        .map(|child| resolve_uri(base, child))
        .unwrap_or_else(|| base.to_owned())
}

fn dash_stream_key(item: &DashSelectedRepresentation) -> String {
    let prefix = match item.kind {
        DownloadStreamKind::Audio | DownloadStreamKind::SecondaryAudio => "audio",
        DownloadStreamKind::Video => "video",
        DownloadStreamKind::Subtitle => "subtitle",
        DownloadStreamKind::Combined => "combined",
        DownloadStreamKind::Auxiliary => "auxiliary",
    };
    format!("{prefix}-{}", item.adaptation_index)
}

fn rewrite_dash_mpd(manifest: &DashManifest, selected: &[DashSelectedRepresentation]) -> String {
    let duration = manifest.duration_text.as_deref().unwrap_or("PT0S");
    let mut output = format!(
        "<MPD type=\"static\" mediaPresentationDuration=\"{}\" xmlns=\"urn:mpeg:dash:schema:mpd:2011\"><Period>",
        escape_xml_attribute(duration)
    );
    for item in selected {
        let stream_key = dash_stream_key(item);
        let content_type = match item.kind {
            DownloadStreamKind::Audio | DownloadStreamKind::SecondaryAudio => "audio",
            DownloadStreamKind::Video => "video",
            DownloadStreamKind::Subtitle => "text",
            DownloadStreamKind::Combined | DownloadStreamKind::Auxiliary => "application",
        };
        output.push_str(&format!(
            "<AdaptationSet contentType=\"{content_type}\"{}{}><Representation id=\"{}\" bandwidth=\"{}\"{}>",
            item.mime_type
                .as_deref()
                .map(|value| format!(" mimeType=\"{}\"", escape_xml_attribute(value)))
                .unwrap_or_default(),
            item.language
                .as_deref()
                .map(|value| format!(" lang=\"{}\"", escape_xml_attribute(value)))
                .unwrap_or_default(),
            escape_xml_attribute(&item.representation.id),
            escape_xml_attribute(item.representation.bandwidth.as_deref().unwrap_or("1")),
            item.representation
                .codecs
                .as_deref()
                .map(|value| format!(" codecs=\"{}\"", escape_xml_attribute(value)))
                .unwrap_or_default(),
        ));
        if let Some(template) = item.segment_template.as_ref() {
            let initialization = template
                .initialization
                .as_ref()
                .map(|_| format!(" initialization=\"segments/{stream_key}/init.mp4\""))
                .unwrap_or_default();
            output.push_str(&format!(
                "<SegmentTemplate timescale=\"{}\" duration=\"{}\" startNumber=\"{}\"{initialization} media=\"segments/{stream_key}/seg-$Number%05d$.m4s\" />",
                template.timescale, template.duration, template.start_number
            ));
        } else {
            let extension = extension_from_uri(&item.base_uri, "mp4");
            output.push_str(&format!(
                "<BaseURL>media/{stream_key}.{}</BaseURL><SegmentBase />",
                escape_xml_text(&extension)
            ));
        }
        output.push_str("</Representation></AdaptationSet>");
    }
    output.push_str("</Period></MPD>\n");
    output
}

fn escape_xml_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn escape_xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn expand_dash_template(
    template: &str,
    representation: &DashRepresentation,
    number: u64,
) -> String {
    let value = template.replace("$RepresentationID$", &representation.id);
    replace_dash_number_token(&value, number)
}

fn replace_dash_number_token(value: &str, number: u64) -> String {
    let mut output = value.replace("$Number$", &number.to_string());
    while let Some(start) = output.find("$Number%") {
        let Some(end_offset) = output[start + "$Number%".len()..].find("$") else {
            break;
        };
        let token_end = start + "$Number%".len() + end_offset + 1;
        let format_spec = &output[start + "$Number%".len()..token_end - 1];
        let width = format_spec
            .strip_suffix('d')
            .and_then(|value| value.strip_prefix('0'))
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        output.replace_range(start..token_end, &format!("{number:0width$}"));
    }
    output
}

fn dash_segment_count(duration_seconds: f64, segment_seconds: f64) -> PlayerResult<u64> {
    if !duration_seconds.is_finite()
        || !segment_seconds.is_finite()
        || duration_seconds <= 0.0
        || segment_seconds <= 0.0
    {
        return Err(planning_error(
            PlayerErrorCode::InvalidSource,
            PlayerErrorCategory::Source,
            "DASH SegmentTemplate planning requires finite positive duration values",
        ));
    }
    let segment_count = (duration_seconds / segment_seconds).ceil().max(1.0);
    if segment_count > u64::MAX as f64 {
        return Err(planning_error(
            PlayerErrorCode::InvalidSource,
            PlayerErrorCategory::Source,
            "DASH SegmentTemplate segment count exceeds u64 range",
        ));
    }
    let segment_count = segment_count as u64;
    // Bound the segment count up front. The caller iterates `0..segment_count`
    // and probes each segment over HTTP; an unbounded count from a pathological
    // SegmentTemplate duration would otherwise drive a multi-GB allocation and
    // a probe-request amplification storm before the per-iteration cap inside
    // the loop fires. 100k mirrors MAX_PLANNED_SEGMENTS.
    if segment_count > MAX_PLANNED_SEGMENTS as u64 {
        return Err(planning_error(
            PlayerErrorCode::InvalidSource,
            PlayerErrorCategory::Source,
            format!(
                "DASH SegmentTemplate planning refused to expand more than {MAX_PLANNED_SEGMENTS} segments"
            ),
        ));
    }
    Ok(segment_count)
}

fn parse_iso8601_duration_seconds(value: &str) -> Option<f64> {
    let value = value.strip_prefix("PT")?;
    let mut number = String::new();
    let mut total = 0.0;
    for character in value.chars() {
        if character.is_ascii_digit() || character == '.' {
            number.push(character);
            continue;
        }
        if number.is_empty() {
            return None;
        }
        let parsed = number.parse::<f64>().ok()?;
        if !parsed.is_finite() {
            return None;
        }
        number.clear();
        match character {
            'H' => total += parsed * 3600.0,
            'M' => total += parsed * 60.0,
            'S' => total += parsed,
            _ => return None,
        }
    }
    if !number.is_empty() || total <= 0.0 || !total.is_finite() {
        return None;
    }
    Some(total)
}

fn split_quoted(input: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut in_quotes = false;
    let mut start = 0;
    for (index, character) in input.char_indices() {
        if character == '"' {
            in_quotes = !in_quotes;
        } else if character == delimiter && !in_quotes {
            parts.push(input[start..index].trim());
            start = index + character.len_utf8();
        }
    }
    parts.push(input[start..].trim());
    parts
}

fn resolve_uri(base: &str, reference: &str) -> String {
    let reference = reference.trim();
    if reference.contains("://") || reference.starts_with("data:") {
        return reference.to_owned();
    }
    if reference.starts_with('/') {
        if let Some((scheme, rest)) = base.split_once("://")
            && let Some(host_end) = rest.find('/')
        {
            return format!("{scheme}://{}{}", &rest[..host_end], reference);
        }
    }
    let base_without_query = base.split_once('?').map(|(path, _)| path).unwrap_or(base);
    let prefix = base_without_query
        .rsplit_once('/')
        .map(|(prefix, _)| prefix)
        .unwrap_or(base_without_query);
    format!("{prefix}/{reference}")
}

fn extension_from_uri(uri: &str, default_extension: &str) -> String {
    let path = uri
        .split_once('?')
        .map(|(path, _)| path)
        .unwrap_or(uri)
        .split_once('#')
        .map(|(path, _)| path)
        .unwrap_or(uri);
    path.rsplit_once('.')
        .map(|(_, extension)| extension)
        .filter(|extension| {
            !extension.is_empty()
                && extension
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
        })
        .unwrap_or(default_extension)
        .to_owned()
}

fn parse_flv_clip_manifest(base_uri: &str, manifest: &str) -> PlayerResult<Vec<String>> {
    let mut clips = Vec::new();
    for line in manifest.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || line.eq_ignore_ascii_case("ffconcat version 1.0")
        {
            continue;
        }

        let raw_uri = if let Some(rest) = line.strip_prefix("file ") {
            rest.trim().trim_matches('"').trim_matches('\'')
        } else {
            line
        };
        if raw_uri.is_empty() {
            continue;
        }
        ensure_segment_capacity(clips.len())?;
        clips.push(resolve_uri(base_uri, raw_uri));
    }

    Ok(clips)
}

fn escape_ffconcat_path(path: &str) -> String {
    path.replace('\'', "'\\''")
}

fn planning_error(
    code: PlayerErrorCode,
    category: PlayerErrorCategory,
    message: impl Into<String>,
) -> PlayerError {
    PlayerError::with_category(code, category, message)
}

#[cfg(test)]
mod tests {
    use super::{DownloadPlanner, DownloadPlanningClient, parse_iso8601_duration_seconds};
    use crate::{
        DownloadByteRange, DownloadContentFormat, DownloadProfile, DownloadSource,
        DownloadStreamKind, PlayerError, PlayerErrorCategory, PlayerErrorCode,
    };
    use player_model::MediaSource;
    use std::collections::HashMap;

    #[derive(Debug, Default)]
    struct FakeClient {
        text: HashMap<String, String>,
        sizes: HashMap<String, u64>,
        default_size: Option<u64>,
    }

    impl FakeClient {
        fn with_text(mut self, uri: &str, text: &str) -> Self {
            self.text.insert(uri.to_owned(), text.to_owned());
            self
        }

        fn with_size(mut self, uri: &str, size: u64) -> Self {
            self.sizes.insert(uri.to_owned(), size);
            self
        }

        fn with_default_size(mut self, size: u64) -> Self {
            self.default_size = Some(size);
            self
        }
    }

    impl DownloadPlanningClient for FakeClient {
        fn fetch_text(&self, uri: &str) -> Result<String, PlayerError> {
            self.text.get(uri).cloned().ok_or_else(|| {
                PlayerError::with_category(
                    PlayerErrorCode::InvalidSource,
                    PlayerErrorCategory::Network,
                    format!("missing text fixture for {uri}"),
                )
            })
        }

        fn content_length(&self, uri: &str) -> Result<Option<u64>, PlayerError> {
            Ok(self.sizes.get(uri).copied().or(self.default_size))
        }
    }

    fn hls_source(uri: &str) -> DownloadSource {
        DownloadSource::new(MediaSource::new(uri), DownloadContentFormat::HlsSegments)
            .with_manifest_uri(uri)
    }

    #[test]
    fn hls_media_playlist_plans_segments_and_total_size() {
        let client = FakeClient::default()
            .with_text(
                "https://cdn.test/video/main.m3u8",
                "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-ENDLIST\n#EXTINF:4,\nseg1.ts\n#EXTINF:4,\nseg2.ts\n",
            )
            .with_size("https://cdn.test/video/seg1.ts", 100)
            .with_size("https://cdn.test/video/seg2.ts", 150);
        let planner = DownloadPlanner::new(client);

        let index = planner
            .plan(
                &hls_source("https://cdn.test/video/main.m3u8"),
                &DownloadProfile::default(),
            )
            .expect("hls plan");

        assert_eq!(index.total_size_bytes, Some(250));
        assert_eq!(index.segments.len(), 2);
        assert_eq!(
            index.resources[0]
                .generated_text
                .as_ref()
                .expect("manifest"),
            "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\nsegments/media-00000.ts\n#EXTINF:4,\nsegments/media-00001.ts\n#EXT-X-ENDLIST\n"
        );
        assert_eq!(index.streams.len(), 1);
        assert_eq!(index.streams[0].kind, DownloadStreamKind::Combined);
        assert!(
            index.streams[0]
                .resource_ids
                .contains(&"hls-master".to_owned())
        );
        assert_eq!(index.segments[0].sequence, Some(0));
        assert_eq!(index.segments[1].sequence, Some(1));
        assert!(
            index.resources[0]
                .generated_text
                .as_ref()
                .expect("manifest")
                .contains("segments/media-00000.ts")
        );
    }

    #[test]
    fn hls_media_sequence_is_preserved_from_playlist() {
        let client = FakeClient::default()
            .with_text(
                "https://cdn.test/video/sequence.m3u8",
                "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:42\n#EXT-X-ENDLIST\n#EXTINF:4,\nseg42.ts\n#EXTINF:4,\nseg43.ts\n",
            )
            .with_size("https://cdn.test/video/seg42.ts", 100)
            .with_size("https://cdn.test/video/seg43.ts", 150);
        let planner = DownloadPlanner::new(client);

        let index = planner
            .plan(
                &hls_source("https://cdn.test/video/sequence.m3u8"),
                &DownloadProfile::default(),
            )
            .expect("hls plan");

        assert_eq!(index.segments[0].sequence, Some(42));
        assert_eq!(index.segments[1].sequence, Some(43));
        assert!(
            index.resources[0]
                .generated_text
                .as_ref()
                .expect("manifest")
                .contains("segments/media-00042.ts")
        );
    }

    #[test]
    fn hls_media_sequence_rejects_malformed_values() {
        let client = FakeClient::default().with_text(
            "https://cdn.test/video/bad-sequence.m3u8",
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:not-a-number\n#EXT-X-ENDLIST\n#EXTINF:4,\nseg.ts\n",
        );
        let planner = DownloadPlanner::new(client);

        let error = planner
            .plan(
                &hls_source("https://cdn.test/video/bad-sequence.m3u8"),
                &DownloadProfile::default(),
            )
            .expect_err("malformed media sequence should fail");

        assert_eq!(error.code(), PlayerErrorCode::InvalidSource);
    }

    #[test]
    fn hls_master_playlist_includes_selected_audio_playlist() {
        let client = FakeClient::default()
            .with_text(
                "https://cdn.test/master.m3u8",
                "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",LANGUAGE=\"en\",DEFAULT=YES,URI=\"audio/en.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=2000,AUDIO=\"a\"\nvideo/main.m3u8\n",
            )
            .with_text(
                "https://cdn.test/video/main.m3u8",
                "#EXTM3U\n#EXT-X-ENDLIST\n#EXTINF:4,\nseg.ts\n",
            )
            .with_text(
                "https://cdn.test/audio/en.m3u8",
                "#EXTM3U\n#EXT-X-ENDLIST\n#EXTINF:4,\naudio.aac\n",
            )
            .with_size("https://cdn.test/video/seg.ts", 200)
            .with_size("https://cdn.test/audio/audio.aac", 50);
        let planner = DownloadPlanner::new(client);

        let index = planner
            .plan(
                &hls_source("https://cdn.test/master.m3u8"),
                &DownloadProfile::default(),
            )
            .expect("hls master plan");

        assert_eq!(index.total_size_bytes, Some(250));
        assert_eq!(index.segments.len(), 2);
        assert!(index.resources.iter().any(|resource| {
            resource
                .relative_path
                .as_deref()
                .is_some_and(|path| path == "audio.m3u8")
        }));
        assert!(
            index.resources[0]
                .generated_text
                .as_ref()
                .is_some_and(|text| text.contains("AUDIO=\"audio\""))
        );
        assert_eq!(index.streams.len(), 2);
        assert!(
            index
                .streams
                .iter()
                .any(|stream| stream.kind == DownloadStreamKind::Video)
        );
        assert!(
            index
                .streams
                .iter()
                .any(|stream| stream.kind == DownloadStreamKind::Audio)
        );
    }

    #[test]
    fn hls_shared_map_is_rewritten_into_each_media_playlist() {
        let client = FakeClient::default()
            .with_text(
                "https://cdn.test/master.m3u8",
                "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",LANGUAGE=\"en\",DEFAULT=YES,URI=\"audio.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=2000,AUDIO=\"a\"\nvideo.m3u8\n",
            )
            .with_text(
                "https://cdn.test/video.m3u8",
                "#EXTM3U\n#EXT-X-ENDLIST\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4,\nvideo.m4s\n",
            )
            .with_text(
                "https://cdn.test/audio.m3u8",
                "#EXTM3U\n#EXT-X-ENDLIST\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4,\naudio.m4s\n",
            )
            .with_size("https://cdn.test/init.mp4", 10)
            .with_size("https://cdn.test/video.m4s", 20)
            .with_size("https://cdn.test/audio.m4s", 5);
        let planner = DownloadPlanner::new(client);

        let index = planner
            .plan(
                &hls_source("https://cdn.test/master.m3u8"),
                &DownloadProfile::default(),
            )
            .expect("hls master plan");

        assert_eq!(index.total_size_bytes, Some(35));
        assert_eq!(
            index
                .resources
                .iter()
                .filter(|resource| resource.resource_id.contains("-init-"))
                .count(),
            1
        );
        for playlist_name in ["video.m3u8", "audio.m3u8"] {
            let playlist = index
                .resources
                .iter()
                .find(|resource| resource.relative_path.as_deref() == Some(playlist_name.as_ref()))
                .and_then(|resource| resource.generated_text.as_deref())
                .expect("generated media playlist");
            assert!(
                playlist.contains("#EXT-X-MAP:URI=\"segments/video-init-0.mp4\""),
                "{playlist_name} should reference the shared init segment"
            );
        }
        assert!(
            index
                .streams
                .iter()
                .all(|stream| stream.resource_ids.iter().any(|id| id.contains("-init-")))
        );
    }

    #[test]
    fn hls_byte_ranges_count_declared_range_lengths() {
        let client = FakeClient::default().with_text(
            "https://cdn.test/ranges.m3u8",
            "#EXTM3U\n#EXT-X-ENDLIST\n#EXTINF:4,\n#EXT-X-BYTERANGE:10@5\nmedia.ts\n#EXTINF:4,\n#EXT-X-BYTERANGE:12\nmedia.ts\n",
        );
        let planner = DownloadPlanner::new(client);

        let index = planner
            .plan(
                &hls_source("https://cdn.test/ranges.m3u8"),
                &DownloadProfile::default(),
            )
            .expect("range hls plan");

        assert_eq!(index.total_size_bytes, Some(22));
        assert_eq!(
            index.segments[0].byte_range,
            Some(DownloadByteRange {
                offset: 5,
                length: 10
            })
        );
        assert_eq!(
            index.segments[1].byte_range,
            Some(DownloadByteRange {
                offset: 15,
                length: 12
            })
        );
    }

    #[test]
    fn hls_map_byterange_does_not_seed_segment_byterange_offset() {
        let client = FakeClient::default().with_text(
            "https://cdn.test/map-ranges.m3u8",
            "#EXTM3U\n#EXT-X-ENDLIST\n#EXT-X-MAP:URI=\"media.mp4\",BYTERANGE=\"100@900\"\n#EXTINF:4,\n#EXT-X-BYTERANGE:10@5\nmedia.mp4\n#EXTINF:4,\n#EXT-X-BYTERANGE:12\nmedia.mp4\n",
        );
        let planner = DownloadPlanner::new(client);

        let index = planner
            .plan(
                &hls_source("https://cdn.test/map-ranges.m3u8"),
                &DownloadProfile::default(),
            )
            .expect("range hls plan");

        let init = index
            .resources
            .iter()
            .find(|resource| resource.resource_id.contains("-init-"))
            .expect("init resource");
        assert_eq!(
            init.byte_range,
            Some(DownloadByteRange {
                offset: 900,
                length: 100
            })
        );
        assert_eq!(
            index.segments[1].byte_range,
            Some(DownloadByteRange {
                offset: 15,
                length: 12
            })
        );
    }

    #[test]
    fn hls_live_playlist_is_rejected() {
        let client = FakeClient::default().with_text(
            "https://cdn.test/live.m3u8",
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\nseg.ts\n",
        );
        let planner = DownloadPlanner::new(client);

        let error = planner
            .plan(
                &hls_source("https://cdn.test/live.m3u8"),
                &DownloadProfile::default(),
            )
            .expect_err("live playlist should fail");

        assert_eq!(error.code(), PlayerErrorCode::Unsupported);
    }

    #[test]
    fn dash_segment_template_plans_finite_static_mpd() {
        let mpd = r#"<MPD type="static" mediaPresentationDuration="PT6S"><Period><AdaptationSet><Representation id="v1" bandwidth="1000"><SegmentTemplate timescale="1" duration="2" startNumber="1" initialization="init-$RepresentationID$.mp4" media="chunk-$Number%05d$.m4s" /></Representation></AdaptationSet></Period></MPD>"#;
        let client = FakeClient::default()
            .with_text("https://cdn.test/manifest.mpd", mpd)
            .with_size("https://cdn.test/init-v1.mp4", 10)
            .with_size("https://cdn.test/chunk-00001.m4s", 20)
            .with_size("https://cdn.test/chunk-00002.m4s", 30)
            .with_size("https://cdn.test/chunk-00003.m4s", 40);
        let planner = DownloadPlanner::new(client);
        let source = DownloadSource::new(
            MediaSource::new("https://cdn.test/manifest.mpd"),
            DownloadContentFormat::DashSegments,
        )
        .with_manifest_uri("https://cdn.test/manifest.mpd");

        let index = planner
            .plan(&source, &DownloadProfile::default())
            .expect("dash template plan");

        assert_eq!(index.total_size_bytes, Some(100));
        assert_eq!(index.segments.len(), 3);
        assert_eq!(index.resources[1].size_bytes, Some(10));
    }

    #[test]
    fn dash_plans_selected_video_and_each_audio_adaptation_with_inheritance() {
        let mpd = r#"
<MPD type="static" mediaPresentationDuration="PT4S">
  <BaseURL>root/</BaseURL>
  <Period>
    <BaseURL>period/</BaseURL>
    <SegmentTemplate timescale="1" duration="2" startNumber="7" />
    <AdaptationSet contentType="video" mimeType="video/mp4">
      <BaseURL>video/</BaseURL>
      <SegmentTemplate initialization="init-$RepresentationID$.mp4" media="chunk-$Number$.m4s" />
      <Representation id="v-first" bandwidth="1000" />
      <Representation id="v-choice" bandwidth="2000">
        <BaseURL>selected/</BaseURL>
      </Representation>
    </AdaptationSet>
    <AdaptationSet contentType="audio" mimeType="audio/mp4" lang="en">
      <BaseURL>audio/</BaseURL>
      <SegmentTemplate initialization="init-$RepresentationID$.mp4" media="chunk-$Number$.m4s" />
      <Representation id="a-first" bandwidth="128" />
      <Representation id="a-second" bandwidth="256" />
    </AdaptationSet>
  </Period>
</MPD>
"#;
        let client = FakeClient::default()
            .with_text("https://cdn.test/base/manifest.mpd", mpd)
            // The legacy string parser incorrectly treats this BaseURL as a
            // single SegmentBase resource. Keeping it sized makes the test fail
            // on the missing audio/video plan instead of fixture setup.
            .with_size("https://cdn.test/base/selected/", 1)
            .with_size(
                "https://cdn.test/base/root/period/video/selected/init-v-choice.mp4",
                10,
            )
            .with_size(
                "https://cdn.test/base/root/period/video/selected/chunk-7.m4s",
                20,
            )
            .with_size(
                "https://cdn.test/base/root/period/video/selected/chunk-8.m4s",
                30,
            )
            .with_size(
                "https://cdn.test/base/root/period/audio/init-a-first.mp4",
                11,
            )
            .with_size("https://cdn.test/base/root/period/audio/chunk-7.m4s", 21)
            .with_size("https://cdn.test/base/root/period/audio/chunk-8.m4s", 31);
        let planner = DownloadPlanner::new(client);
        let source = DownloadSource::new(
            MediaSource::new("https://cdn.test/base/manifest.mpd"),
            DownloadContentFormat::DashSegments,
        );
        let profile = DownloadProfile {
            variant_id: Some("v-choice".to_owned()),
            ..DownloadProfile::default()
        };

        let index = planner.plan(&source, &profile).expect("complete DASH plan");

        assert_eq!(index.total_size_bytes, Some(123));
        assert_eq!(index.segments.len(), 4);
        assert_eq!(index.streams.len(), 2);
        assert_eq!(index.streams[0].kind, DownloadStreamKind::Video);
        assert_eq!(index.streams[1].kind, DownloadStreamKind::Audio);
        assert_eq!(
            index.segments[0].uri,
            "https://cdn.test/base/root/period/video/selected/chunk-7.m4s"
        );
        assert_eq!(
            index.segments[2].uri,
            "https://cdn.test/base/root/period/audio/chunk-7.m4s"
        );
        let local_mpd = index.resources[0]
            .generated_text
            .as_deref()
            .expect("generated MPD");
        assert!(local_mpd.contains("contentType=\"video\""));
        assert!(local_mpd.contains("contentType=\"audio\""));
        assert!(local_mpd.contains("id=\"v-choice\""));
        assert!(!local_mpd.contains("id=\"v-first\""));
    }

    #[test]
    fn dash_segment_limit_is_shared_across_selected_representations() {
        let mpd = r#"
<MPD type="static" mediaPresentationDuration="PT100002S">
  <Period>
    <AdaptationSet contentType="video">
      <SegmentTemplate timescale="1" duration="2" media="video-$Number$.m4s" />
      <Representation id="video" />
    </AdaptationSet>
    <AdaptationSet contentType="audio">
      <SegmentTemplate timescale="1" duration="2" media="audio-$Number$.m4s" />
      <Representation id="audio" />
    </AdaptationSet>
  </Period>
</MPD>
"#;
        let client = FakeClient::default()
            .with_text("https://cdn.test/manifest.mpd", mpd)
            .with_default_size(1);
        let planner = DownloadPlanner::new(client);
        let source = DownloadSource::new(
            MediaSource::new("https://cdn.test/manifest.mpd"),
            DownloadContentFormat::DashSegments,
        );

        let error = planner
            .plan(&source, &DownloadProfile::default())
            .expect_err("combined DASH segment count must be bounded");

        assert_eq!(error.code(), PlayerErrorCode::InvalidSource);
        assert!(error.message().contains("100000"));
    }

    #[test]
    fn dash_variant_id_never_selects_an_audio_representation() {
        let mpd = r#"
<MPD type="static" mediaPresentationDuration="PT2S">
  <Period>
    <AdaptationSet contentType="video">
      <SegmentTemplate duration="2" media="video-$RepresentationID$-$Number$.m4s" />
      <Representation id="v-first" />
      <Representation id="v-second" />
    </AdaptationSet>
    <AdaptationSet contentType="audio">
      <SegmentTemplate duration="2" media="audio-$RepresentationID$-$Number$.m4s" />
      <Representation id="a-first" />
      <Representation id="a-second" />
    </AdaptationSet>
  </Period>
</MPD>
"#;
        let client = FakeClient::default()
            .with_text("https://cdn.test/manifest.mpd", mpd)
            .with_default_size(1);
        let planner = DownloadPlanner::new(client);
        let source = DownloadSource::new(
            MediaSource::new("https://cdn.test/manifest.mpd"),
            DownloadContentFormat::DashSegments,
        );
        let profile = DownloadProfile {
            variant_id: Some("a-second".to_owned()),
            ..DownloadProfile::default()
        };

        let index = planner.plan(&source, &profile).expect("DASH plan");

        assert_eq!(
            index.streams[0].metadata.get("representationId"),
            Some(&"v-first".to_owned())
        );
        assert_eq!(
            index.streams[1].metadata.get("representationId"),
            Some(&"a-first".to_owned())
        );
    }

    #[test]
    fn dash_duration_rejects_malformed_iso8601_values() {
        for value in ["PT", "PT0S", "PT1H2", "PTMS", "P1D"] {
            assert_eq!(parse_iso8601_duration_seconds(value), None, "{value}");
        }
        assert_eq!(parse_iso8601_duration_seconds("PT1H2M3.5S"), Some(3723.5));
    }

    #[test]
    fn dash_segment_template_inherits_base_url_before_representation() {
        let mpd = r#"<MPD type="static" mediaPresentationDuration="PT4S"><Period><AdaptationSet><BaseURL>media/</BaseURL><Representation id="v1" bandwidth="1000"><SegmentTemplate timescale="1" duration="2" startNumber="1" initialization="init-$RepresentationID$.mp4" media="chunk-$Number$.m4s" /></Representation></AdaptationSet></Period></MPD>"#;
        let client = FakeClient::default()
            .with_text("https://cdn.test/base/manifest.mpd", mpd)
            .with_size("https://cdn.test/base/media/init-v1.mp4", 10)
            .with_size("https://cdn.test/base/media/chunk-1.m4s", 20)
            .with_size("https://cdn.test/base/media/chunk-2.m4s", 30);
        let planner = DownloadPlanner::new(client);
        let source = DownloadSource::new(
            MediaSource::new("https://cdn.test/base/manifest.mpd"),
            DownloadContentFormat::DashSegments,
        )
        .with_manifest_uri("https://cdn.test/base/manifest.mpd");

        let index = planner
            .plan(&source, &DownloadProfile::default())
            .expect("dash template plan");

        assert_eq!(index.total_size_bytes, Some(60));
        assert_eq!(
            index.resources[1].uri,
            "https://cdn.test/base/media/init-v1.mp4"
        );
        assert_eq!(
            index.segments[0].uri,
            "https://cdn.test/base/media/chunk-1.m4s"
        );
    }

    #[test]
    fn dash_dynamic_mpd_is_rejected() {
        let client = FakeClient::default().with_text(
            "https://cdn.test/live.mpd",
            r#"<MPD type="dynamic"><Period /></MPD>"#,
        );
        let planner = DownloadPlanner::new(client);
        let source = DownloadSource::new(
            MediaSource::new("https://cdn.test/live.mpd"),
            DownloadContentFormat::DashSegments,
        );

        let error = planner
            .plan(&source, &DownloadProfile::default())
            .expect_err("dynamic MPD should fail");

        assert_eq!(error.code(), PlayerErrorCode::Unsupported);
    }

    #[test]
    fn dash_segment_base_plans_single_media_resource() {
        let mpd = r#"<MPD type="static" mediaPresentationDuration="PT10S"><Period><AdaptationSet><Representation id="v1"><BaseURL>video.mp4</BaseURL><SegmentBase indexRange="0-99" /></Representation></AdaptationSet></Period></MPD>"#;
        let client = FakeClient::default()
            .with_text("https://cdn.test/base/manifest.mpd", mpd)
            .with_size("https://cdn.test/base/video.mp4", 1024);
        let planner = DownloadPlanner::new(client);
        let source = DownloadSource::new(
            MediaSource::new("https://cdn.test/base/manifest.mpd"),
            DownloadContentFormat::DashSegments,
        );

        let index = planner
            .plan(&source, &DownloadProfile::default())
            .expect("dash segment base plan");

        assert_eq!(index.total_size_bytes, Some(1024));
        assert_eq!(index.resources.len(), 2);
        assert!(index.resources[0].generated_text.is_some());
    }

    #[test]
    fn flv_single_clip_plans_concat_manifest_and_clip() {
        let client = FakeClient::default().with_size("https://cdn.test/video.flv", 4096);
        let planner = DownloadPlanner::new(client);
        let source = DownloadSource::new(
            MediaSource::new("https://cdn.test/video.flv"),
            DownloadContentFormat::FlvSegments,
        );

        let index = planner
            .plan(&source, &DownloadProfile::default())
            .expect("flv plan");

        assert_eq!(index.total_size_bytes, Some(4096));
        assert_eq!(index.resources.len(), 1);
        assert_eq!(index.segments.len(), 1);
        assert_eq!(
            index.resources[0].relative_path,
            Some("manifest.ffconcat".into())
        );
    }

    #[test]
    fn flv_manifest_plans_multiple_clips() {
        let client = FakeClient::default()
            .with_text(
                "https://cdn.test/video/clips.ffconcat",
                "ffconcat version 1.0\nfile 'part-1.flv'\nfile 'part-2.flv'\n",
            )
            .with_size("https://cdn.test/video/part-1.flv", 100)
            .with_size("https://cdn.test/video/part-2.flv", 150);
        let planner = DownloadPlanner::new(client);
        let source = DownloadSource::new(
            MediaSource::new("https://cdn.test/video/clips.ffconcat"),
            DownloadContentFormat::FlvSegments,
        );

        let index = planner
            .plan(&source, &DownloadProfile::default())
            .expect("flv clip manifest plan");

        assert_eq!(index.total_size_bytes, Some(250));
        assert_eq!(index.segments.len(), 2);
        assert_eq!(
            index.resources[0].generated_text.as_ref().expect("concat"),
            "ffconcat version 1.0\nfile 'clips/clip-00001.flv'\nfile 'clips/clip-00002.flv'\n"
        );
    }

    #[test]
    fn missing_content_length_fails_strict_planning() {
        let client = FakeClient::default().with_text(
            "https://cdn.test/main.m3u8",
            "#EXTM3U\n#EXT-X-ENDLIST\n#EXTINF:4,\nseg.ts\n",
        );
        let planner = DownloadPlanner::new(client);

        let error = planner
            .plan(
                &hls_source("https://cdn.test/main.m3u8"),
                &DownloadProfile::default(),
            )
            .expect_err("missing content length should fail");

        assert_eq!(error.category(), PlayerErrorCategory::Network);
    }

    // Regression: a malicious HLS media playlist with millions of segment URI
    // lines must not drive unbounded Vec allocation or a probe-request storm.
    // The planner must refuse to expand beyond MAX_PLANNED_SEGMENTS.
    #[test]
    fn refuses_oversized_hls_media_playlist() {
        // Build a VOD playlist with MAX_PLANNED_SEGMENTS + 1 segment lines. We
        // do NOT register sizes (planning would fail on the first probe anyway),
        // because the parser-side cap fires before any probe is issued.
        let mut manifest = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-ENDLIST\n");
        for sequence in 0..=super::MAX_PLANNED_SEGMENTS {
            manifest.push_str(&format!("#EXTINF:1,\nseg-{sequence}.ts\n"));
        }
        let client = FakeClient::default().with_text("https://cdn.test/main.m3u8", &manifest);
        let planner = DownloadPlanner::new(client);

        let error = planner
            .plan(
                &hls_source("https://cdn.test/main.m3u8"),
                &DownloadProfile::default(),
            )
            .expect_err("oversized HLS playlist must be rejected");
        assert_eq!(error.category(), PlayerErrorCategory::Source);
        assert_eq!(error.code(), PlayerErrorCode::InvalidSource);
    }

    // Regression: a pathological DASH SegmentTemplate that would expand to more
    // than MAX_PLANNED_SEGMENTS segments must be rejected up front, before any
    // probe is issued.
    #[test]
    fn refuses_oversized_dash_segment_template() {
        // duration_seconds huge, segment_seconds tiny => segment_count huge.
        let error = super::dash_segment_count(3_600_000_000.0, 0.001)
            .expect_err("pathological DASH template must be rejected");
        assert_eq!(error.category(), PlayerErrorCategory::Source);
        assert_eq!(error.code(), PlayerErrorCode::InvalidSource);
    }

    // Regression: an FLV ffconcat manifest with too many clip URIs must be
    // rejected by the same cap.
    #[test]
    fn refuses_oversized_flv_clip_manifest() {
        let mut manifest = String::from("ffconcat version 1.0\n");
        for sequence in 0..=super::MAX_PLANNED_SEGMENTS {
            manifest.push_str(&format!("file 'clip-{sequence}.flv'\n"));
        }
        let client = FakeClient::default().with_text("https://cdn.test/main.ffconcat", &manifest);
        let planner = DownloadPlanner::new(client);

        let source = DownloadSource::new(
            MediaSource::new("https://cdn.test/main.ffconcat"),
            DownloadContentFormat::FlvSegments,
        )
        .with_manifest_uri("https://cdn.test/main.ffconcat");
        let error = planner
            .plan(&source, &DownloadProfile::default())
            .expect_err("oversized FLV clip manifest must be rejected");
        assert_eq!(error.category(), PlayerErrorCategory::Source);
        assert_eq!(error.code(), PlayerErrorCode::InvalidSource);
    }
}
