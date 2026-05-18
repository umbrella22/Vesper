use std::collections::HashMap;
use std::path::PathBuf;

use crate::{
    DownloadAssetIndex, DownloadAssetStream, DownloadByteRange, DownloadContentFormat,
    DownloadProfile, DownloadResourceRecord, DownloadSegmentRecord, DownloadSource,
    DownloadStreamKind, PlayerError, PlayerErrorCategory, PlayerErrorCode, PlayerResult,
};

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
        let manifest = self.client.fetch_text(manifest_uri)?;
        let mpd_type = xml_attr(&manifest, "MPD", "type");
        if mpd_type.as_deref().is_some_and(|value| value != "static") {
            return Err(planning_error(
                PlayerErrorCode::Unsupported,
                PlayerErrorCategory::Source,
                "DASH download planning requires a static MPD",
            ));
        }

        let representation = select_dash_representation(&manifest, profile)?;
        if let Some(template) = representation.segment_template.as_ref() {
            return self.build_dash_template_index(
                manifest_uri,
                &manifest,
                &representation,
                template,
            );
        }

        if let Some(base_url) = representation.base_url.as_deref() {
            return self.build_dash_segment_base_index(manifest_uri, &manifest, base_url);
        }

        Err(planning_error(
            PlayerErrorCode::Unsupported,
            PlayerErrorCategory::Source,
            "DASH MPD did not contain a supported SegmentTemplate or SegmentBase representation",
        ))
    }

    fn build_dash_template_index(
        &self,
        manifest_uri: &str,
        manifest: &str,
        representation: &DashRepresentation,
        template: &DashSegmentTemplate,
    ) -> PlayerResult<DownloadAssetIndex> {
        let duration_seconds = dash_duration_seconds(manifest).ok_or_else(|| {
            planning_error(
                PlayerErrorCode::InvalidSource,
                PlayerErrorCategory::Source,
                "DASH SegmentTemplate planning requires a finite MPD duration",
            )
        })?;
        if template.duration == 0 {
            return Err(planning_error(
                PlayerErrorCode::InvalidSource,
                PlayerErrorCategory::Source,
                "DASH SegmentTemplate duration must be greater than zero",
            ));
        }

        let segment_seconds = template.duration as f64 / template.timescale.max(1) as f64;
        let segment_count = (duration_seconds / segment_seconds).ceil().max(1.0) as u64;
        let mut resources = Vec::new();
        let mut segments = Vec::new();
        let mut total_size_bytes = 0_u64;
        let base_uri = representation
            .base_url
            .as_deref()
            .map(|base_url| resolve_uri(manifest_uri, base_url))
            .unwrap_or_else(|| manifest_uri.to_owned());

        if let Some(initialization) = template.initialization.as_deref() {
            let remote = resolve_uri(
                &base_uri,
                &expand_dash_template(initialization, representation, template.start_number),
            );
            let size = self.probe_required_size(&remote, None)?;
            total_size_bytes += size;
            resources.push(DownloadResourceRecord {
                resource_id: "dash-init".to_owned(),
                uri: remote,
                relative_path: Some(PathBuf::from("segments/init.mp4")),
                byte_range: None,
                generated_text: None,
                size_bytes: Some(size),
                etag: None,
                checksum: None,
            });
        }

        for index in 0..segment_count {
            let number = template.start_number + index;
            let remote = resolve_uri(
                &base_uri,
                &expand_dash_template(&template.media, representation, number),
            );
            let size = self.probe_required_size(&remote, None)?;
            total_size_bytes += size;
            segments.push(DownloadSegmentRecord {
                segment_id: format!("dash-segment-{number}"),
                uri: remote,
                relative_path: Some(PathBuf::from(format!("segments/seg-{number:05}.m4s"))),
                sequence: Some(number),
                byte_range: None,
                size_bytes: Some(size),
                checksum: None,
            });
        }

        resources.insert(
            0,
            DownloadResourceRecord {
                resource_id: "dash-manifest".to_owned(),
                uri: format!("vesper-generated://dash/{}", "manifest.mpd"),
                relative_path: Some(PathBuf::from("manifest.mpd")),
                byte_range: None,
                generated_text: Some(rewrite_dash_template_mpd(
                    manifest,
                    representation,
                    template,
                    segment_count,
                )),
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
            ..DownloadAssetIndex::default()
        })
    }

    fn build_dash_segment_base_index(
        &self,
        manifest_uri: &str,
        manifest: &str,
        base_url: &str,
    ) -> PlayerResult<DownloadAssetIndex> {
        let remote = resolve_uri(manifest_uri, base_url);
        let size = self.probe_required_size(&remote, None)?;
        let local_name = format!("media.{}", extension_from_uri(base_url, "mp4"));
        let manifest_text = rewrite_dash_segment_base_mpd(manifest, &local_name);

        Ok(DownloadAssetIndex {
            content_format: DownloadContentFormat::DashSegments,
            total_size_bytes: Some(size),
            resources: vec![
                DownloadResourceRecord {
                    resource_id: "dash-manifest".to_owned(),
                    uri: "vesper-generated://dash/manifest.mpd".to_owned(),
                    relative_path: Some(PathBuf::from("manifest.mpd")),
                    byte_range: None,
                    generated_text: Some(manifest_text),
                    size_bytes: None,
                    etag: None,
                    checksum: None,
                },
                DownloadResourceRecord {
                    resource_id: "dash-media".to_owned(),
                    uri: remote,
                    relative_path: Some(PathBuf::from(local_name)),
                    byte_range: None,
                    generated_text: None,
                    size_bytes: Some(size),
                    etag: None,
                    checksum: None,
                },
            ],
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
            let size = self.probe_required_size(clip_uri, None)?;
            total_size_bytes += size;
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
                total_size_bytes += size;
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
            let size = planner.probe_required_size(&segment.uri, segment.byte_range)?;
            total_size_bytes += size;
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
    let mut previous_range_end = 0_u64;
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
                .and_then(|value| parse_hls_byte_range(value, &mut previous_range_end));
            maps.push(HlsMap {
                uri: resolve_uri(manifest_uri, uri),
                byte_range,
            });
            continue;
        }
        if let Some(value) = line.strip_prefix("#EXT-X-BYTERANGE:") {
            pending_byte_range = parse_hls_byte_range(value.trim(), &mut previous_range_end);
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
        sequence += 1;
        segments.push(HlsSegment {
            uri: resolve_uri(manifest_uri, line),
            duration: pending_duration.take(),
            byte_range: pending_byte_range.take(),
            sequence,
        });
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

#[derive(Debug, Clone)]
struct DashRepresentation {
    id: String,
    bandwidth: Option<String>,
    base_url: Option<String>,
    segment_template: Option<DashSegmentTemplate>,
}

#[derive(Debug, Clone)]
struct DashSegmentTemplate {
    media: String,
    initialization: Option<String>,
    start_number: u64,
    timescale: u64,
    duration: u64,
}

fn select_dash_representation(
    manifest: &str,
    profile: &DownloadProfile,
) -> PlayerResult<DashRepresentation> {
    let mut inherited_template = find_segment_template(manifest);
    let inherited_base_url = inherited_dash_base_url(manifest);
    let mut candidates = Vec::new();
    for block in xml_blocks(manifest, "Representation") {
        let open_tag = block
            .split_once('>')
            .map(|(tag, _)| tag)
            .unwrap_or(block.as_str());
        let id = xml_attr_from_tag(open_tag, "id").unwrap_or_else(|| candidates.len().to_string());
        let bandwidth = xml_attr_from_tag(open_tag, "bandwidth");
        let base_url = xml_text(&block, "BaseURL").or_else(|| inherited_base_url.clone());
        let segment_template = find_segment_template(&block).or_else(|| inherited_template.clone());
        candidates.push(DashRepresentation {
            id,
            bandwidth,
            base_url,
            segment_template,
        });
    }
    if candidates.is_empty()
        && let Some(base_url) = xml_text(manifest, "BaseURL")
    {
        candidates.push(DashRepresentation {
            id: "0".to_owned(),
            bandwidth: None,
            base_url: Some(base_url),
            segment_template: inherited_template.take(),
        });
    }

    profile
        .variant_id
        .as_deref()
        .and_then(|variant_id| {
            candidates
                .iter()
                .find(|representation| representation.id == variant_id)
                .cloned()
        })
        .or_else(|| candidates.into_iter().next())
        .ok_or_else(|| {
            planning_error(
                PlayerErrorCode::InvalidSource,
                PlayerErrorCategory::Source,
                "DASH MPD did not contain any representations",
            )
        })
}

fn inherited_dash_base_url(manifest: &str) -> Option<String> {
    let representation_start = manifest.find("<Representation").unwrap_or(manifest.len());
    xml_text(&manifest[..representation_start], "BaseURL")
}

fn find_segment_template(input: &str) -> Option<DashSegmentTemplate> {
    let tag = find_xml_open_tag(input, "SegmentTemplate")?;
    let media = xml_attr_from_tag(tag, "media")?;
    let initialization = xml_attr_from_tag(tag, "initialization");
    let start_number = xml_attr_from_tag(tag, "startNumber")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1);
    let timescale = xml_attr_from_tag(tag, "timescale")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1);
    let duration = xml_attr_from_tag(tag, "duration")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    Some(DashSegmentTemplate {
        media,
        initialization,
        start_number,
        timescale,
        duration,
    })
}

fn rewrite_dash_template_mpd(
    manifest: &str,
    representation: &DashRepresentation,
    template: &DashSegmentTemplate,
    segment_count: u64,
) -> String {
    let duration = dash_duration_text(manifest).unwrap_or_else(|| "PT0S".to_owned());
    let bandwidth = representation.bandwidth.as_deref().unwrap_or("1");
    let initialization = template
        .initialization
        .as_ref()
        .map(|_| " initialization=\"segments/init.mp4\"")
        .unwrap_or_default();

    format!(
        "<MPD type=\"static\" mediaPresentationDuration=\"{duration}\" xmlns=\"urn:mpeg:dash:schema:mpd:2011\"><Period><AdaptationSet><Representation id=\"{}\" bandwidth=\"{bandwidth}\"><SegmentTemplate timescale=\"{}\" duration=\"{}\" startNumber=\"{}\"{initialization} media=\"segments/seg-$Number%05d$.m4s\" /></Representation></AdaptationSet></Period></MPD>\n<!-- plannedSegments={segment_count} -->\n",
        representation.id, template.timescale, template.duration, template.start_number
    )
}

fn rewrite_dash_segment_base_mpd(manifest: &str, local_name: &str) -> String {
    let duration = dash_duration_text(manifest).unwrap_or_else(|| "PT0S".to_owned());
    format!(
        "<MPD type=\"static\" mediaPresentationDuration=\"{duration}\" xmlns=\"urn:mpeg:dash:schema:mpd:2011\"><Period><AdaptationSet><Representation id=\"0\" bandwidth=\"1\"><BaseURL>{local_name}</BaseURL><SegmentBase /></Representation></AdaptationSet></Period></MPD>\n"
    )
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

fn dash_duration_text(manifest: &str) -> Option<String> {
    xml_attr(manifest, "MPD", "mediaPresentationDuration")
}

fn dash_duration_seconds(manifest: &str) -> Option<f64> {
    parse_iso8601_duration_seconds(&dash_duration_text(manifest)?)
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
        let parsed = number.parse::<f64>().ok()?;
        number.clear();
        match character {
            'H' => total += parsed * 3600.0,
            'M' => total += parsed * 60.0,
            'S' => total += parsed,
            _ => return None,
        }
    }
    Some(total)
}

fn xml_attr(input: &str, tag: &str, attr: &str) -> Option<String> {
    find_xml_open_tag(input, tag).and_then(|open_tag| xml_attr_from_tag(open_tag, attr))
}

fn find_xml_open_tag<'a>(input: &'a str, tag: &str) -> Option<&'a str> {
    let start = input.find(&format!("<{tag}"))?;
    let rest = &input[start..];
    let end = rest.find('>')?;
    Some(&rest[..=end])
}

fn xml_attr_from_tag(tag: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=");
    let start = tag.find(&needle)? + needle.len();
    let quote = tag[start..].chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let value_start = start + quote.len_utf8();
    let value_end = tag[value_start..].find(quote)? + value_start;
    Some(tag[value_start..value_end].to_owned())
}

fn xml_blocks(input: &str, tag: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = input;
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    while let Some(start) = rest.find(&open) {
        let candidate = &rest[start..];
        if let Some(close_start) = candidate.find(&close) {
            let end = close_start + close.len();
            blocks.push(candidate[..end].to_owned());
            rest = &candidate[end..];
        } else if let Some(open_end) = candidate.find("/>") {
            blocks.push(candidate[..open_end + 2].to_owned());
            rest = &candidate[open_end + 2..];
        } else {
            break;
        }
    }
    blocks
}

fn xml_text(input: &str, tag: &str) -> Option<String> {
    let open_start = input.find(&format!("<{tag}"))?;
    let after_open = &input[open_start..];
    let open_end = after_open.find('>')? + open_start + 1;
    let close_start = input[open_end..].find(&format!("</{tag}>"))? + open_end;
    Some(input[open_end..close_start].trim().to_owned())
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
    use super::{DownloadPlanner, DownloadPlanningClient};
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
            Ok(self.sizes.get(uri).copied())
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
            "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\nsegments/media-00001.ts\n#EXTINF:4,\nsegments/media-00002.ts\n#EXT-X-ENDLIST\n"
        );
        assert_eq!(index.streams.len(), 1);
        assert_eq!(index.streams[0].kind, DownloadStreamKind::Combined);
        assert!(
            index.streams[0]
                .resource_ids
                .contains(&"hls-master".to_owned())
        );
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
}
