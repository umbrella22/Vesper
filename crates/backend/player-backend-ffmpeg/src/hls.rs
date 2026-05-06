use std::collections::HashMap;
use std::ffi::CString;
use std::ptr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result};
use ffmpeg_next as ffmpeg;
use player_model::{MediaSource, MediaSourceKind, MediaSourceProtocol};
use tracing::info;

use crate::input::FfmpegInputInterrupt;

const MAX_RESOLVED_HLS_SOURCE_CACHE_ENTRIES: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HlsAudioRendition {
    pub(crate) group_id: String,
    pub(crate) uri: String,
    pub(crate) is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HlsVariantInfo {
    pub(crate) audio_group_id: Option<String>,
    pub(crate) uri: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ResolvedRemoteHlsSources {
    pub(crate) audio_rendition_uri: Option<String>,
    pub(crate) video_variant_uri: Option<String>,
}

pub(crate) fn resolve_audio_decode_source(
    source: &MediaSource,
    interrupt_flag: Option<Arc<AtomicBool>>,
) -> Result<MediaSource> {
    if source.kind() != MediaSourceKind::Remote || source.protocol() != MediaSourceProtocol::Hls {
        return Ok(source.clone());
    }

    let Some(audio_rendition_uri) =
        resolve_remote_hls_audio_rendition_uri(source.uri(), interrupt_flag)?
    else {
        return Ok(source.clone());
    };

    if audio_rendition_uri != source.uri() {
        info!(
            source = source.uri(),
            audio_rendition_uri, "resolved remote HLS audio rendition playlist"
        );
        return Ok(MediaSource::new(audio_rendition_uri));
    }

    Ok(source.clone())
}

pub(crate) fn resolve_video_decode_source(
    source: &MediaSource,
    interrupt_flag: Option<Arc<AtomicBool>>,
) -> Result<MediaSource> {
    if source.kind() != MediaSourceKind::Remote || source.protocol() != MediaSourceProtocol::Hls {
        return Ok(source.clone());
    }

    let Some(video_variant_uri) =
        resolve_remote_hls_video_variant_uri(source.uri(), interrupt_flag)?
    else {
        return Ok(source.clone());
    };

    if video_variant_uri != source.uri() {
        info!(
            source = source.uri(),
            video_variant_uri, "resolved remote HLS video variant playlist"
        );
        return Ok(MediaSource::new(video_variant_uri));
    }

    Ok(source.clone())
}

fn resolve_remote_hls_audio_rendition_uri(
    manifest_uri: &str,
    interrupt_flag: Option<Arc<AtomicBool>>,
) -> Result<Option<String>> {
    Ok(resolve_remote_hls_sources(manifest_uri, interrupt_flag)?.audio_rendition_uri)
}

fn resolve_remote_hls_video_variant_uri(
    manifest_uri: &str,
    interrupt_flag: Option<Arc<AtomicBool>>,
) -> Result<Option<String>> {
    Ok(resolve_remote_hls_sources(manifest_uri, interrupt_flag)?.video_variant_uri)
}

fn resolve_remote_hls_sources(
    manifest_uri: &str,
    interrupt_flag: Option<Arc<AtomicBool>>,
) -> Result<ResolvedRemoteHlsSources> {
    if let Some(cached) = resolved_hls_source_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(manifest_uri)
        .cloned()
    {
        return Ok(cached);
    }

    let manifest_text = fetch_text_resource_via_ffmpeg(manifest_uri, interrupt_flag)
        .with_context(|| format!("failed to fetch remote HLS manifest: {manifest_uri}"))?;
    let resolved = resolve_hls_master_manifest_sources(manifest_uri, &manifest_text);

    let mut cache = resolved_hls_source_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if !cache.contains_key(manifest_uri) && cache.len() >= MAX_RESOLVED_HLS_SOURCE_CACHE_ENTRIES {
        cache.clear();
    }
    cache.insert(manifest_uri.to_owned(), resolved.clone());

    Ok(resolved)
}

fn resolved_hls_source_cache() -> &'static Mutex<HashMap<String, ResolvedRemoteHlsSources>> {
    static CACHE: OnceLock<Mutex<HashMap<String, ResolvedRemoteHlsSources>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn resolve_hls_master_manifest_sources(
    manifest_uri: &str,
    manifest_text: &str,
) -> ResolvedRemoteHlsSources {
    ResolvedRemoteHlsSources {
        audio_rendition_uri: select_hls_audio_rendition_uri(manifest_uri, manifest_text),
        video_variant_uri: select_hls_video_variant_uri(manifest_uri, manifest_text),
    }
}

fn fetch_text_resource_via_ffmpeg(
    uri: &str,
    interrupt_flag: Option<Arc<AtomicBool>>,
) -> Result<String> {
    let uri_cstr = CString::new(uri).context("resource URI contained an interior NUL byte")?;
    let interrupt = interrupt_flag.map(FfmpegInputInterrupt::new);
    let interrupt_callback = interrupt.as_ref().map(FfmpegInputInterrupt::callback);
    let interrupt_ptr = interrupt_callback
        .as_ref()
        .map(|callback| callback as *const _)
        .unwrap_or(ptr::null());
    let mut io_context = ptr::null_mut();
    let mut options = ffmpeg::Dictionary::new();
    options.set("rw_timeout", "15000000");
    let mut raw_options = unsafe { options.disown() };

    unsafe {
        let open_result = ffmpeg::ffi::avio_open2(
            &mut io_context,
            uri_cstr.as_ptr(),
            ffmpeg::ffi::AVIO_FLAG_READ,
            interrupt_ptr,
            &mut raw_options,
        );
        ffmpeg::Dictionary::own(raw_options);

        if open_result < 0 {
            return Err(anyhow::Error::new(ffmpeg::Error::from(open_result))
                .context(format!("failed to open FFmpeg IO for {uri}")));
        }

        let mut bytes = Vec::new();
        let mut buffer = [0u8; 8 * 1024];

        loop {
            let read_result =
                ffmpeg::ffi::avio_read(io_context, buffer.as_mut_ptr().cast(), buffer.len() as i32);

            if read_result == 0 || read_result == ffmpeg::ffi::AVERROR_EOF {
                break;
            }

            if read_result < 0 {
                ffmpeg::ffi::avio_closep(&mut io_context);
                return Err(anyhow::Error::new(ffmpeg::Error::from(read_result))
                    .context(format!("failed to read FFmpeg IO resource {uri}")));
            }

            bytes.extend_from_slice(&buffer[..read_result as usize]);
        }

        ffmpeg::ffi::avio_closep(&mut io_context);
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

pub(crate) fn select_hls_audio_rendition_uri(
    manifest_uri: &str,
    manifest_text: &str,
) -> Option<String> {
    let (audio_renditions, variants) = parse_hls_master_manifest(manifest_text);
    if audio_renditions.is_empty() {
        return None;
    }

    let preferred_group = variants
        .first()
        .and_then(|variant| variant.audio_group_id.as_deref());
    let selected = preferred_group
        .and_then(|group_id| choose_hls_audio_rendition(&audio_renditions, Some(group_id)))
        .or_else(|| choose_hls_audio_rendition(&audio_renditions, None))?;

    resolve_uri_relative_to(manifest_uri, &selected.uri)
}

pub(crate) fn select_hls_video_variant_uri(
    manifest_uri: &str,
    manifest_text: &str,
) -> Option<String> {
    let (_, variants) = parse_hls_master_manifest(manifest_text);
    let selected = variants.first()?;
    resolve_uri_relative_to(manifest_uri, &selected.uri)
}

pub(crate) fn parse_hls_master_manifest(
    manifest_text: &str,
) -> (Vec<HlsAudioRendition>, Vec<HlsVariantInfo>) {
    let mut audio_renditions = Vec::new();
    let mut variants = Vec::new();
    let mut pending_variant = None;

    for raw_line in manifest_text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(attributes) = line.strip_prefix("#EXT-X-MEDIA:") {
            let attributes = parse_hls_attribute_list(attributes);
            let media_type = attributes
                .get("TYPE")
                .map(|value| value.eq_ignore_ascii_case("AUDIO"))
                .unwrap_or(false);
            let Some(group_id) = attributes.get("GROUP-ID") else {
                continue;
            };
            let Some(uri) = attributes.get("URI") else {
                continue;
            };
            if !media_type {
                continue;
            }

            let is_default = attributes
                .get("DEFAULT")
                .map(|value| value.eq_ignore_ascii_case("YES"))
                .unwrap_or(false);
            audio_renditions.push(HlsAudioRendition {
                group_id: group_id.clone(),
                uri: uri.clone(),
                is_default,
            });
            continue;
        }

        if let Some(attributes) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            let attributes = parse_hls_attribute_list(attributes);
            pending_variant = Some(HlsVariantInfo {
                audio_group_id: attributes.get("AUDIO").cloned(),
                uri: String::new(),
            });
            continue;
        }

        if let Some(mut variant) = pending_variant.take() {
            if line.starts_with('#') {
                pending_variant = Some(variant);
                continue;
            }
            variant.uri = line.to_owned();
            variants.push(variant);
        }
    }

    (audio_renditions, variants)
}

fn choose_hls_audio_rendition<'a>(
    renditions: &'a [HlsAudioRendition],
    group_id: Option<&str>,
) -> Option<&'a HlsAudioRendition> {
    let candidates = renditions
        .iter()
        .filter(|rendition| group_id.is_none_or(|group| rendition.group_id == group));

    candidates
        .clone()
        .find(|rendition| rendition.is_default)
        .or_else(|| candidates.into_iter().next())
}

fn parse_hls_attribute_list(attributes: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in attributes.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            ',' if !in_quotes => {
                parse_hls_attribute_entry(&current, &mut values);
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    parse_hls_attribute_entry(&current, &mut values);
    values
}

fn parse_hls_attribute_entry(entry: &str, values: &mut HashMap<String, String>) {
    let trimmed = entry.trim();
    if trimmed.is_empty() {
        return;
    }

    let Some((key, value)) = trimmed.split_once('=') else {
        return;
    };
    let value = value.trim().trim_matches('"');
    values.insert(key.trim().to_owned(), value.to_owned());
}

pub(crate) fn resolve_uri_relative_to(base_uri: &str, reference: &str) -> Option<String> {
    let reference = reference.trim();
    if reference.is_empty() {
        return None;
    }

    if reference.contains("://") {
        return Some(reference.to_owned());
    }

    if reference.starts_with("//") {
        let (scheme, _) = base_uri.split_once("://")?;
        return Some(format!("{scheme}:{reference}"));
    }

    let base_uri = base_uri
        .split_once('#')
        .map(|(value, _)| value)
        .unwrap_or(base_uri);
    let base_uri = base_uri
        .split_once('?')
        .map(|(value, _)| value)
        .unwrap_or(base_uri);
    let (scheme, rest) = base_uri.split_once("://")?;
    let (authority, raw_path) = rest.split_once('/').unwrap_or((rest, ""));
    let base_path = format!("/{}", raw_path);
    let joined_path = if reference.starts_with('/') {
        reference.to_owned()
    } else {
        let base_dir = base_path
            .rsplit_once('/')
            .map(|(dir, _)| dir)
            .filter(|dir| !dir.is_empty())
            .unwrap_or("/");
        if base_dir.ends_with('/') {
            format!("{base_dir}{reference}")
        } else {
            format!("{base_dir}/{reference}")
        }
    };
    let normalized_path = normalize_url_path(&joined_path);

    Some(format!("{scheme}://{authority}{normalized_path}"))
}

fn normalize_url_path(path: &str) -> String {
    let mut segments = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            _ => segments.push(segment),
        }
    }

    if segments.is_empty() {
        "/".to_owned()
    } else {
        format!("/{}", segments.join("/"))
    }
}
