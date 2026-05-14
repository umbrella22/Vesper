#![warn(clippy::undocumented_unsafe_blocks)]

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use ffmpeg::{Rational, codec, encoder, format, media};
use ffmpeg_next as ffmpeg;
use jni::errors::Result as JniResult;
use jni::objects::{JByteArray, JClass, JObject, JString, JValue};
use jni::signature::RuntimeMethodSignature;
use jni::strings::JNIString;
use jni::sys::{jint, jlong, jobject, jstring};
use jni::{Env, EnvUnowned};
use serde::Deserialize;

const PKG: &str = "io/github/ikaros/vesper/player/android/relay/ffmpeg";
const MAX_MANIFEST_PROBE_BYTES: usize = 1024 * 1024;
const DEFAULT_REMUX_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenRequest {
    session_id: String,
    source_uri: String,
    #[serde(default)]
    source_label: Option<String>,
    #[serde(default)]
    source_protocol: Option<String>,
    fallback_format: FallbackFormat,
    #[serde(default)]
    resource_path: String,
    #[serde(default)]
    range: Option<RangeRequest>,
    #[serde(default)]
    source_headers: HashMap<String, String>,
    #[serde(default)]
    request_headers: HashMap<String, String>,
    #[serde(default = "default_true")]
    enable_range_cache: bool,
    #[serde(default)]
    debug_diagnostics: bool,
    #[serde(default)]
    route_id: Option<String>,
    #[serde(default)]
    route_name: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum FallbackFormat {
    MpegTs,
    Hls,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RangeRequest {
    start: Option<u64>,
    end: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
struct ResolvedRange {
    start: u64,
    end: u64,
}

#[derive(Debug)]
struct RelayError {
    code: &'static str,
    status: i32,
    message: String,
    details: Vec<(String, String)>,
}

struct SessionCache {
    root_dir: PathBuf,
    state: Mutex<SessionState>,
}

#[derive(Default)]
struct SessionState {
    mpeg_ts_path: Option<PathBuf>,
    hls_playlist_path: Option<PathBuf>,
}

enum NativeStream {
    File(File),
    LimitedFile { file: File, remaining: u64 },
}

impl Read for NativeStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self {
            NativeStream::File(file) => file.read(buffer),
            NativeStream::LimitedFile { file, remaining } => {
                if *remaining == 0 {
                    return Ok(0);
                }
                let max_read = buffer.len().min(*remaining as usize);
                let read = file.read(&mut buffer[..max_read])?;
                *remaining = remaining.saturating_sub(read as u64);
                Ok(read)
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_relay_ffmpeg_VesperRelayFfmpegNative_runtimeMetadata(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
) -> jstring {
    let metadata = serde_json::json!({
        "profileHash": profile_hash(),
        "configureMetadata": configure_metadata(),
        "engine": "vesper-relay-ffmpeg",
        "status": "available",
    })
    .to_string();
    let mut output = std::ptr::null_mut();
    let _ = unowned_env.with_env(|env| -> JniResult<()> {
        output = env.new_string(metadata)?.into_raw();
        Ok(())
    });
    output
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_relay_ffmpeg_VesperRelayFfmpegNative_open(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    request_json: JString<'_>,
) -> jobject {
    let mut output = std::ptr::null_mut();
    let _ = unowned_env.with_env(|env| -> JniResult<()> {
        let request = match decode_request(env, request_json) {
            Ok(request) => request,
            Err(error) => {
                output = open_result_object(
                    env,
                    OpenResultFields {
                        handle: 0,
                        status: 400,
                        content_type: "application/octet-stream",
                        content_length: -1,
                        headers: Vec::new(),
                        error_code: Some("ffmpeg_open_failed"),
                        error_message: Some(&error.message),
                        error_details: error.details,
                    },
                )?
                .into_raw();
                return Ok(());
            }
        };

        output = match open_stream(&request) {
            Ok(opened) => open_result_object(
                env,
                OpenResultFields {
                    handle: opened.handle,
                    status: opened.status,
                    content_type: &opened.content_type,
                    content_length: opened.content_length,
                    headers: opened.headers,
                    error_code: None,
                    error_message: None,
                    error_details: Vec::new(),
                },
            )?,
            Err(error) => open_result_object(
                env,
                OpenResultFields {
                    handle: 0,
                    status: error.status,
                    content_type: "application/octet-stream",
                    content_length: -1,
                    headers: Vec::new(),
                    error_code: Some(error.code),
                    error_message: Some(&error.message),
                    error_details: error.details,
                },
            )?,
        }
        .into_raw();
        Ok(())
    });
    output
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_relay_ffmpeg_VesperRelayFfmpegNative_read(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    buffer: jni::sys::jbyteArray,
    length: jint,
) -> jint {
    let mut output = -1;
    let _ = unowned_env.with_env(|env| -> JniResult<()> {
        if handle == 0 || length <= 0 || buffer.is_null() {
            output = 0;
            return Ok(());
        }

        let array = {
            // SAFETY: `buffer` is the byte array passed by the current JNI
            // frame to this native method and is only borrowed for this call.
            unsafe { JByteArray::from_raw(env, buffer) }
        };
        let array_length = array.len(env).unwrap_or(0);
        let target_length = (length as usize).min(array_length);
        if target_length == 0 {
            output = 0;
            return Ok(());
        }

        let mut bytes = vec![0u8; target_length];
        let read = {
            let mut streams = streams().lock().unwrap_or_else(|error| error.into_inner());
            let Some(stream) = streams.get_mut(&handle) else {
                output = -1;
                return Ok(());
            };
            stream.read(&mut bytes).unwrap_or_default()
        };

        if read == 0 {
            output = -1;
            return Ok(());
        }

        let jbytes: Vec<i8> = bytes[..read].iter().map(|byte| *byte as i8).collect();
        array.set_region(env, 0, &jbytes)?;
        output = read as jint;
        Ok(())
    });
    output
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_relay_ffmpeg_VesperRelayFfmpegNative_close(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) {
    streams()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .remove(&handle);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_relay_ffmpeg_VesperRelayFfmpegNative_invalidate(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_id: JString<'_>,
) {
    let _ = unowned_env.with_env(|env| -> JniResult<()> {
        let session_id = session_id.try_to_string(env)?.to_string();
        if let Some(cache) = sessions()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&session_id)
        {
            let _ = fs::remove_dir_all(&cache.root_dir);
        }
        Ok(())
    });
}

struct OpenedStream {
    handle: i64,
    status: i32,
    content_type: String,
    content_length: i64,
    headers: Vec<(String, String)>,
}

struct OpenResultFields<'a> {
    handle: i64,
    status: i32,
    content_type: &'a str,
    content_length: i64,
    headers: Vec<(String, String)>,
    error_code: Option<&'a str>,
    error_message: Option<&'a str>,
    error_details: Vec<(String, String)>,
}

fn decode_request(env: &mut Env<'_>, request_json: JString<'_>) -> Result<OpenRequest, RelayError> {
    let value = request_json
        .try_to_string(env)
        .map_err(|error| {
            relay_error(
                "ffmpeg_open_failed",
                400,
                "Failed to decode request JSON.",
                [("jniError", error.to_string())],
            )
        })?
        .to_string();
    serde_json::from_str(&value).map_err(|error| {
        relay_error(
            "ffmpeg_open_failed",
            400,
            "Failed to parse request JSON.",
            [("jsonError", error.to_string())],
        )
    })
}

fn open_stream(request: &OpenRequest) -> Result<OpenedStream, RelayError> {
    validate_request(request)?;
    initialize_ffmpeg()?;
    reject_encrypted_dash(request)?;

    let session = session_cache(request)?;
    let path = {
        let mut state = session
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match request.fallback_format {
            FallbackFormat::MpegTs => ensure_mpeg_ts_cache(request, &session.root_dir, &mut state)?,
            FallbackFormat::Hls => ensure_hls_cache(request, &session.root_dir, &mut state)?,
        }
    };

    let target_path = target_resource_path(request, &session.root_dir, &path)?;
    let content_type = content_type_for_request(request, &target_path);
    open_cached_file(request, target_path, content_type)
}

fn validate_request(request: &OpenRequest) -> Result<(), RelayError> {
    if request.session_id.trim().is_empty() {
        return Err(relay_error(
            "ffmpeg_open_failed",
            400,
            "Relay remux request did not include a session id.",
            Vec::<(String, String)>::new(),
        ));
    }
    if request.source_uri.trim().is_empty() {
        return Err(relay_error(
            "ffmpeg_open_failed",
            400,
            "Relay remux request did not include a source URI.",
            request.base_details(),
        ));
    }
    if matches!(request.fallback_format, FallbackFormat::MpegTs) && !request.enable_range_cache {
        return Err(relay_error(
            "range_not_ready",
            416,
            "MPEG-TS fallback requires relay-managed range cache.",
            request.base_details(),
        ));
    }
    Ok(())
}

fn initialize_ffmpeg() -> Result<(), RelayError> {
    ffmpeg::init().map_err(|error| {
        relay_error(
            "ffmpeg_open_failed",
            503,
            "Failed to initialize FFmpeg.",
            [("ffmpegError", error.to_string())],
        )
    })
}

fn session_cache(request: &OpenRequest) -> Result<Arc<SessionCache>, RelayError> {
    let mut sessions = sessions().lock().unwrap_or_else(|error| error.into_inner());
    if let Some(existing) = sessions.get(&request.session_id) {
        return Ok(existing.clone());
    }

    let root_dir = std::env::temp_dir()
        .join("vesper-relay-ffmpeg")
        .join(safe_file_component(&request.session_id));
    fs::create_dir_all(&root_dir).map_err(|error| {
        relay_error(
            "ffmpeg_open_failed",
            503,
            "Failed to create relay remux cache directory.",
            request
                .base_details()
                .into_iter()
                .chain([("ioError".to_owned(), error.to_string())]),
        )
    })?;

    let cache = Arc::new(SessionCache {
        root_dir,
        state: Mutex::new(SessionState::default()),
    });
    sessions.insert(request.session_id.clone(), cache.clone());
    Ok(cache)
}

fn ensure_mpeg_ts_cache(
    request: &OpenRequest,
    root_dir: &Path,
    state: &mut SessionState,
) -> Result<PathBuf, RelayError> {
    if let Some(path) = state.mpeg_ts_path.as_ref().filter(|path| path.is_file()) {
        return Ok(path.clone());
    }

    ensure_muxer("mpegts", request)?;
    ensure_demuxer_if_needed("dash", request)?;
    let output_path = root_dir.join("media.ts");
    let started_at = Instant::now();
    remux_to_file(request, &output_path, OutputKind::MpegTs)?;
    if started_at.elapsed() > DEFAULT_REMUX_TIMEOUT {
        return Err(relay_error(
            "remux_timeout",
            504,
            "FFmpeg relay remux exceeded the timeout budget.",
            request.base_details().into_iter().chain([(
                "elapsedMillis".to_owned(),
                started_at.elapsed().as_millis().to_string(),
            )]),
        ));
    }
    state.mpeg_ts_path = Some(output_path.clone());
    Ok(output_path)
}

fn ensure_hls_cache(
    request: &OpenRequest,
    root_dir: &Path,
    state: &mut SessionState,
) -> Result<PathBuf, RelayError> {
    if let Some(path) = state
        .hls_playlist_path
        .as_ref()
        .filter(|path| path.is_file())
    {
        return Ok(path.clone());
    }

    ensure_muxer("hls", request)?;
    ensure_demuxer_if_needed("dash", request)?;
    clean_hls_outputs(root_dir);
    let output_path = root_dir.join("playlist.m3u8");
    let started_at = Instant::now();
    remux_to_file(request, &output_path, OutputKind::Hls)?;
    rewrite_hls_playlist(&output_path, root_dir, request)?;
    if started_at.elapsed() > DEFAULT_REMUX_TIMEOUT {
        return Err(relay_error(
            "remux_timeout",
            504,
            "FFmpeg relay HLS fallback exceeded the timeout budget.",
            request.base_details().into_iter().chain([(
                "elapsedMillis".to_owned(),
                started_at.elapsed().as_millis().to_string(),
            )]),
        ));
    }
    state.hls_playlist_path = Some(output_path.clone());
    Ok(output_path)
}

fn rewrite_hls_playlist(
    playlist_path: &Path,
    root_dir: &Path,
    request: &OpenRequest,
) -> Result<(), RelayError> {
    let playlist = fs::read_to_string(playlist_path).map_err(|error| {
        relay_error(
            "ffmpeg_open_failed",
            503,
            "Failed to read generated HLS fallback playlist.",
            request
                .base_details()
                .into_iter()
                .chain([("ioError".to_owned(), error.to_string())]),
        )
    })?;
    let root = root_dir.to_string_lossy();
    let rewritten = playlist
        .lines()
        .map(|line| {
            if line.starts_with('#') || line.trim().is_empty() {
                return line.to_owned();
            }
            let without_root = line.strip_prefix(root.as_ref()).unwrap_or(line);
            Path::new(without_root)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(without_root)
                .trim_start_matches('/')
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(playlist_path, format!("{rewritten}\n")).map_err(|error| {
        relay_error(
            "ffmpeg_open_failed",
            503,
            "Failed to rewrite generated HLS fallback playlist.",
            request
                .base_details()
                .into_iter()
                .chain([("ioError".to_owned(), error.to_string())]),
        )
    })
}

fn clean_hls_outputs(root_dir: &Path) {
    if let Ok(entries) = fs::read_dir(root_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("ts") || extension.eq_ignore_ascii_case("m3u8")
                })
            {
                let _ = fs::remove_file(path);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum OutputKind {
    MpegTs,
    Hls,
}

fn remux_to_file(
    request: &OpenRequest,
    output_path: &Path,
    kind: OutputKind,
) -> Result<(), RelayError> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            relay_error(
                "ffmpeg_open_failed",
                503,
                "Failed to create relay output directory.",
                request
                    .base_details()
                    .into_iter()
                    .chain([("ioError".to_owned(), error.to_string())]),
            )
        })?;
    }
    let _ = fs::remove_file(output_path);

    let input_options = input_dictionary(request);
    let mut input_context = format::input_with_dictionary(&request.source_uri, input_options)
        .map_err(|error| {
            relay_error(
                "ffmpeg_open_failed",
                503,
                "Failed to open DASH source with FFmpeg.",
                request
                    .base_details()
                    .into_iter()
                    .chain([("ffmpegError".to_owned(), error.to_string())]),
            )
        })?;

    let output_path_string = output_path.to_string_lossy().into_owned();
    let mut output_context = match kind {
        OutputKind::MpegTs => format::output_as(&output_path_string, "mpegts"),
        OutputKind::Hls => format::output_as(&output_path_string, "hls"),
    }
    .map_err(|error| {
        relay_error(
            "ffmpeg_muxer_missing",
            503,
            "Failed to create FFmpeg relay output.",
            request
                .base_details()
                .into_iter()
                .chain([("ffmpegError".to_owned(), error.to_string())]),
        )
    })?;

    let mut stream_mapping = vec![-1; input_context.nb_streams() as usize];
    let mut input_time_bases = vec![Rational(0, 1); input_context.nb_streams() as usize];
    let mut output_stream_index = 0;

    for (input_stream_index, input_stream) in input_context.streams().enumerate() {
        let medium = input_stream.parameters().medium();
        if medium != media::Type::Audio && medium != media::Type::Video {
            continue;
        }

        stream_mapping[input_stream_index] = output_stream_index;
        input_time_bases[input_stream_index] = input_stream.time_base();
        output_stream_index += 1;

        let mut output_stream = output_context
            .add_stream(encoder::find(codec::Id::None))
            .map_err(|error| {
                relay_error(
                    "ffmpeg_open_failed",
                    503,
                    "Failed to add FFmpeg relay output stream.",
                    request
                        .base_details()
                        .into_iter()
                        .chain([("ffmpegError".to_owned(), error.to_string())]),
                )
            })?;
        output_stream.set_parameters(input_stream.parameters());
        // SAFETY: FFmpeg requires codec_tag to be cleared after copying codec
        // parameters into a different muxer; the stream owns these parameters.
        unsafe {
            (*output_stream.parameters().as_mut_ptr()).codec_tag = 0;
        }
    }

    if output_stream_index == 0 {
        return Err(relay_error(
            "ffmpeg_open_failed",
            415,
            "DASH source does not contain audio or video streams that can be remuxed.",
            request.base_details(),
        ));
    }

    output_context.set_metadata(input_context.metadata().to_owned());
    match kind {
        OutputKind::MpegTs => output_context.write_header(),
        OutputKind::Hls => {
            let mut options = ffmpeg::Dictionary::new();
            let segment_pattern = output_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("segment_%05d.ts")
                .to_string_lossy()
                .into_owned();
            options.set("hls_segment_filename", &segment_pattern);
            options.set("hls_time", "6");
            options.set("hls_playlist_type", "vod");
            options.set("hls_segment_type", "mpegts");
            output_context.write_header_with(options).map(|_| ())
        }
    }
    .map_err(|error| {
        relay_error(
            "ffmpeg_open_failed",
            503,
            "Failed to write FFmpeg relay output header.",
            request
                .base_details()
                .into_iter()
                .chain([("ffmpegError".to_owned(), error.to_string())]),
        )
    })?;

    for (stream, mut packet) in input_context.packets() {
        let input_stream_index = stream.index();
        let output_stream_index = stream_mapping[input_stream_index];
        if output_stream_index < 0 {
            continue;
        }
        let output_stream = output_context
            .stream(output_stream_index as usize)
            .ok_or_else(|| {
                relay_error(
                    "ffmpeg_open_failed",
                    503,
                    "FFmpeg relay output stream disappeared during remux.",
                    request.base_details(),
                )
            })?;
        packet.rescale_ts(
            input_time_bases[input_stream_index],
            output_stream.time_base(),
        );
        packet.set_position(-1);
        packet.set_stream(output_stream_index as usize);
        packet
            .write_interleaved(&mut output_context)
            .map_err(|error| {
                relay_error(
                    "ffmpeg_open_failed",
                    503,
                    "Failed to write FFmpeg relay packet.",
                    request
                        .base_details()
                        .into_iter()
                        .chain([("ffmpegError".to_owned(), error.to_string())]),
                )
            })?;
    }

    output_context.write_trailer().map_err(|error| {
        relay_error(
            "ffmpeg_open_failed",
            503,
            "Failed to finalize FFmpeg relay output.",
            request
                .base_details()
                .into_iter()
                .chain([("ffmpegError".to_owned(), error.to_string())]),
        )
    })
}

fn target_resource_path(
    request: &OpenRequest,
    root_dir: &Path,
    primary_path: &Path,
) -> Result<PathBuf, RelayError> {
    match request.fallback_format {
        FallbackFormat::MpegTs => Ok(primary_path.to_path_buf()),
        FallbackFormat::Hls => {
            let resource = request.resource_path.rsplit('/').next().unwrap_or_default();
            if resource.ends_with(".m3u8") || resource.is_empty() {
                return Ok(primary_path.to_path_buf());
            }
            let file_name = safe_file_component(resource);
            let path = root_dir.join(file_name);
            if path.is_file() {
                Ok(path)
            } else {
                Err(relay_error(
                    "range_not_ready",
                    404,
                    "Requested HLS fallback segment is not available.",
                    request
                        .base_details()
                        .into_iter()
                        .chain([("resourcePath".to_owned(), request.resource_path.clone())]),
                ))
            }
        }
    }
}

fn content_type_for_request(request: &OpenRequest, path: &Path) -> String {
    if request.fallback_format == FallbackFormat::MpegTs {
        return "video/mp2t".to_owned();
    }
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("m3u8"))
    {
        "application/vnd.apple.mpegurl".to_owned()
    } else {
        "video/mp2t".to_owned()
    }
}

fn open_cached_file(
    request: &OpenRequest,
    path: PathBuf,
    content_type: String,
) -> Result<OpenedStream, RelayError> {
    let total = fs::metadata(&path)
        .map_err(|error| {
            relay_error(
                "range_not_ready",
                416,
                "Adapted media cache is not ready.",
                request
                    .base_details()
                    .into_iter()
                    .chain([("ioError".to_owned(), error.to_string())]),
            )
        })?
        .len();

    let range = match request.range {
        Some(range) => Some(resolve_range(range, total).ok_or_else(|| {
            relay_error(
                "range_not_ready",
                416,
                "Requested adapted range is not available.",
                request.base_details().into_iter().chain([
                    ("range".to_owned(), range.to_header_value()),
                    ("availableLength".to_owned(), total.to_string()),
                ]),
            )
        })?),
        None => None,
    };

    let mut file = File::open(&path).map_err(|error| {
        relay_error(
            "range_not_ready",
            416,
            "Failed to open adapted media cache.",
            request
                .base_details()
                .into_iter()
                .chain([("ioError".to_owned(), error.to_string())]),
        )
    })?;

    let (status, content_length, headers, stream) = if let Some(range) = range {
        file.seek(SeekFrom::Start(range.start)).map_err(|error| {
            relay_error(
                "range_not_ready",
                416,
                "Failed to seek adapted media cache.",
                request
                    .base_details()
                    .into_iter()
                    .chain([("ioError".to_owned(), error.to_string())]),
            )
        })?;
        let length = range.end - range.start + 1;
        (
            206,
            length as i64,
            vec![(
                "Content-Range".to_owned(),
                format!("bytes {}-{}/{}", range.start, range.end, total),
            )],
            NativeStream::LimitedFile {
                file,
                remaining: length,
            },
        )
    } else {
        (200, total as i64, Vec::new(), NativeStream::File(file))
    };

    let handle = next_handle();
    streams()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(handle, stream);

    let mut response_headers = headers;
    response_headers.push((
        "X-Vesper-FFmpeg-Profile-Hash".to_owned(),
        profile_hash().to_owned(),
    ));
    if request.debug_diagnostics {
        response_headers.push((
            "X-Vesper-FFmpeg-Configure-Metadata".to_owned(),
            configure_metadata().to_owned(),
        ));
    }

    Ok(OpenedStream {
        handle,
        status,
        content_type,
        content_length,
        headers: response_headers,
    })
}

fn resolve_range(range: RangeRequest, total: u64) -> Option<ResolvedRange> {
    if total == 0 {
        return None;
    }
    let (start, end) = match (range.start, range.end) {
        (Some(start), Some(end)) => (start, end.min(total - 1)),
        (Some(start), None) => (start, total - 1),
        (None, Some(suffix_length)) if suffix_length > 0 => {
            (total.saturating_sub(suffix_length), total - 1)
        }
        _ => return None,
    };
    if start >= total || end < start {
        return None;
    }
    Some(ResolvedRange { start, end })
}

fn ensure_muxer(name: &'static str, request: &OpenRequest) -> Result<(), RelayError> {
    let c_name = CString::new(name).map_err(|_| {
        relay_error(
            "ffmpeg_muxer_missing",
            503,
            "FFmpeg muxer name is invalid.",
            request.base_details(),
        )
    })?;
    // SAFETY: `c_name` is a live NUL-terminated string and FFmpeg only reads
    // it during this lookup.
    let muxer = unsafe {
        ffmpeg::ffi::av_guess_format(c_name.as_ptr(), std::ptr::null(), std::ptr::null())
    };
    if muxer.is_null() {
        return Err(relay_error(
            "ffmpeg_muxer_missing",
            503,
            "Required FFmpeg muxer is missing from the runtime profile.",
            request
                .base_details()
                .into_iter()
                .chain([("muxer".to_owned(), name.to_owned())]),
        ));
    }
    Ok(())
}

fn ensure_demuxer_if_needed(name: &'static str, request: &OpenRequest) -> Result<(), RelayError> {
    if !request
        .source_protocol
        .as_deref()
        .is_some_and(|protocol| protocol.eq_ignore_ascii_case("dash"))
        && !request.source_uri.to_ascii_lowercase().contains(".mpd")
    {
        return Ok(());
    }
    let c_name = CString::new(name).map_err(|_| {
        relay_error(
            "ffmpeg_open_failed",
            503,
            "FFmpeg demuxer name is invalid.",
            request.base_details(),
        )
    })?;
    // SAFETY: `c_name` is a live NUL-terminated string and FFmpeg only reads
    // it during this lookup.
    let demuxer = unsafe { ffmpeg::ffi::av_find_input_format(c_name.as_ptr()) };
    if demuxer.is_null() {
        return Err(relay_error(
            "ffmpeg_open_failed",
            503,
            "Required FFmpeg DASH demuxer is missing from the runtime profile.",
            request
                .base_details()
                .into_iter()
                .chain([("demuxer".to_owned(), name.to_owned())]),
        ));
    }
    Ok(())
}

fn reject_encrypted_dash(request: &OpenRequest) -> Result<(), RelayError> {
    let looks_like_dash = request
        .source_protocol
        .as_deref()
        .is_some_and(|protocol| protocol.eq_ignore_ascii_case("dash"))
        || request.source_uri.to_ascii_lowercase().contains(".mpd");
    if !looks_like_dash {
        return Ok(());
    }

    let manifest = fetch_text_resource(request)?;
    let normalized = manifest.to_ascii_lowercase();
    if normalized.contains("<contentprotection")
        || normalized.contains("cenc:default_kid")
        || normalized.contains("urn:mpeg:dash:mp4protection")
        || normalized.contains("com.widevine.alpha")
        || normalized.contains("com.microsoft.playready")
    {
        return Err(relay_error(
            "unsupported_encrypted_dash",
            415,
            "Encrypted DASH content cannot be remuxed for DLNA fallback.",
            request.base_details(),
        ));
    }
    Ok(())
}

fn fetch_text_resource(request: &OpenRequest) -> Result<String, RelayError> {
    if !request.source_uri.starts_with("http://") && !request.source_uri.starts_with("https://") {
        return fs::read_to_string(&request.source_uri).map_err(|error| {
            relay_error(
                "ffmpeg_open_failed",
                503,
                "Failed to read DASH manifest for encryption detection.",
                request
                    .base_details()
                    .into_iter()
                    .chain([("ioError".to_owned(), error.to_string())]),
            )
        });
    }

    let uri_cstr = CString::new(request.source_uri.as_str()).map_err(|_| {
        relay_error(
            "ffmpeg_open_failed",
            400,
            "Source URI contained an interior NUL byte.",
            request.base_details(),
        )
    })?;
    let mut io_context = std::ptr::null_mut();
    let options = input_dictionary(request);
    // SAFETY: the dictionary is passed to FFmpeg exactly once and reclaimed
    // with `Dictionary::own` immediately after `avio_open2` returns.
    let mut raw_options = unsafe { options.disown() };

    // SAFETY: all pointers passed to FFmpeg are either owned local variables or
    // FFmpeg-allocated output slots. They remain valid for the duration of the
    // synchronous avio open/read/close sequence below.
    unsafe {
        let open_result = ffmpeg::ffi::avio_open2(
            &mut io_context,
            uri_cstr.as_ptr(),
            ffmpeg::ffi::AVIO_FLAG_READ,
            std::ptr::null(),
            &mut raw_options,
        );
        let _ = ffmpeg::Dictionary::own(raw_options);
        if open_result < 0 {
            return Err(relay_error(
                "ffmpeg_open_failed",
                503,
                "Failed to open DASH manifest for encryption detection.",
                request.base_details().into_iter().chain([(
                    "ffmpegError".to_owned(),
                    ffmpeg::Error::from(open_result).to_string(),
                )]),
            ));
        }

        let mut bytes = Vec::new();
        let mut buffer = [0u8; 8192];
        while bytes.len() < MAX_MANIFEST_PROBE_BYTES {
            let read =
                ffmpeg::ffi::avio_read(io_context, buffer.as_mut_ptr().cast(), buffer.len() as i32);
            if read == 0 || read == ffmpeg::ffi::AVERROR_EOF {
                break;
            }
            if read < 0 {
                ffmpeg::ffi::avio_closep(&mut io_context);
                return Err(relay_error(
                    "ffmpeg_open_failed",
                    503,
                    "Failed to read DASH manifest for encryption detection.",
                    request.base_details().into_iter().chain([(
                        "ffmpegError".to_owned(),
                        ffmpeg::Error::from(read).to_string(),
                    )]),
                ));
            }
            bytes.extend_from_slice(&buffer[..read as usize]);
        }
        ffmpeg::ffi::avio_closep(&mut io_context);
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

fn input_dictionary(request: &OpenRequest) -> ffmpeg::Dictionary<'static> {
    let mut options = ffmpeg::Dictionary::new();
    options.set("rw_timeout", "15000000");
    options.set("reconnect", "1");
    options.set("reconnect_streamed", "1");
    options.set("reconnect_delay_max", "2");

    let headers = http_headers_for_ffmpeg(request);
    if !headers.is_empty() {
        options.set("headers", &headers);
    }
    options
}

fn http_headers_for_ffmpeg(request: &OpenRequest) -> String {
    let mut headers = String::new();
    for (name, value) in request
        .source_headers
        .iter()
        .chain(request.request_headers.iter())
    {
        if name.is_empty()
            || value.is_empty()
            || name.eq_ignore_ascii_case("range")
            || name.eq_ignore_ascii_case("host")
            || name.eq_ignore_ascii_case("connection")
        {
            continue;
        }
        headers.push_str(name);
        headers.push_str(": ");
        headers.push_str(value);
        headers.push_str("\r\n");
    }
    headers
}

fn open_result_object<'local>(
    env: &mut Env<'local>,
    fields: OpenResultFields<'_>,
) -> jni::errors::Result<JObject<'local>> {
    let class = env.find_class(jni_name(format!("{PKG}/VesperRelayFfmpegOpenResult")))?;
    let content_type = JObject::from(env.new_string(fields.content_type)?);
    let headers = string_map_object(env, fields.headers)?;
    let error_code = optional_string(env, fields.error_code)?;
    let error_message = optional_string(env, fields.error_message)?;
    let error_details = string_map_object(env, fields.error_details)?;
    env.new_object(
        class,
        method_sig("(JILjava/lang/String;JLjava/util/Map;Ljava/lang/String;Ljava/lang/String;Ljava/util/Map;)V")
            .method_signature(),
        &[
            JValue::Long(fields.handle),
            JValue::Int(fields.status),
            JValue::Object(&content_type),
            JValue::Long(fields.content_length),
            JValue::Object(&headers),
            JValue::Object(&error_code),
            JValue::Object(&error_message),
            JValue::Object(&error_details),
        ],
    )
}

fn string_map_object<'local>(
    env: &mut Env<'local>,
    entries: Vec<(String, String)>,
) -> jni::errors::Result<JObject<'local>> {
    let map_class = env.find_class(jni_name("java/util/HashMap"))?;
    let map = env.new_object(map_class, method_sig("()V").method_signature(), &[])?;
    for (key, value) in entries {
        let key = JObject::from(env.new_string(key)?);
        let value = JObject::from(env.new_string(value)?);
        let _ = env.call_method(
            &map,
            jni_name("put"),
            method_sig("(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;")
                .method_signature(),
            &[JValue::Object(&key), JValue::Object(&value)],
        )?;
    }
    Ok(map)
}

fn optional_string<'local>(
    env: &mut Env<'local>,
    value: Option<&str>,
) -> jni::errors::Result<JObject<'local>> {
    match value {
        Some(value) => env.new_string(value).map(JObject::from),
        None => Ok(JObject::null()),
    }
}

fn sessions() -> &'static Mutex<HashMap<String, Arc<SessionCache>>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, Arc<SessionCache>>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn streams() -> &'static Mutex<HashMap<i64, NativeStream>> {
    static STREAMS: OnceLock<Mutex<HashMap<i64, NativeStream>>> = OnceLock::new();
    STREAMS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_handle() -> i64 {
    static NEXT_HANDLE: AtomicI64 = AtomicI64::new(1);
    NEXT_HANDLE.fetch_add(1, Ordering::Relaxed)
}

fn relay_error<K, V, I>(
    code: &'static str,
    status: i32,
    message: impl Into<String>,
    details: I,
) -> RelayError
where
    K: Into<String>,
    V: Into<String>,
    I: IntoIterator<Item = (K, V)>,
{
    let mut detail_entries: Vec<(String, String)> = details
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect();
    detail_entries.push(("profileHash".to_owned(), profile_hash().to_owned()));
    if !configure_metadata().is_empty() {
        detail_entries.push((
            "ffmpegConfigureMetadata".to_owned(),
            configure_metadata().to_owned(),
        ));
    }
    RelayError {
        code,
        status,
        message: message.into(),
        details: detail_entries,
    }
}

fn safe_file_component(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    if output.is_empty() {
        "media".to_owned()
    } else {
        output
    }
}

fn profile_hash() -> &'static str {
    option_env!("VESPER_FFMPEG_PROFILE_HASH").unwrap_or("unknown")
}

fn configure_metadata() -> &'static str {
    option_env!("VESPER_FFMPEG_CONFIGURE_METADATA").unwrap_or("")
}

fn default_true() -> bool {
    true
}

impl OpenRequest {
    fn base_details(&self) -> Vec<(String, String)> {
        let mut details = vec![
            ("sessionId".to_owned(), self.session_id.clone()),
            (
                "fallbackFormat".to_owned(),
                format!("{:?}", self.fallback_format),
            ),
            ("resourcePath".to_owned(), self.resource_path.clone()),
            ("sourceUri".to_owned(), self.source_uri.clone()),
        ];
        if let Some(label) = self.source_label.as_ref() {
            details.push(("sourceLabel".to_owned(), label.clone()));
        }
        if let Some(route_id) = self.route_id.as_ref() {
            details.push(("routeId".to_owned(), route_id.clone()));
        }
        if let Some(route_name) = self.route_name.as_ref() {
            details.push(("routeName".to_owned(), route_name.clone()));
        }
        details
    }
}

impl RangeRequest {
    fn to_header_value(self) -> String {
        format!(
            "bytes={}-{}",
            self.start
                .map(|value| value.to_string())
                .unwrap_or_default(),
            self.end.map(|value| value.to_string()).unwrap_or_default()
        )
    }
}

fn jni_name(value: impl AsRef<str>) -> JNIString {
    JNIString::from(value.as_ref())
}

fn method_sig(value: &str) -> RuntimeMethodSignature {
    match RuntimeMethodSignature::from_str(value) {
        Ok(signature) => signature,
        Err(_) => RuntimeMethodSignature::from(jni::jni_sig!("()V")),
    }
}

#[allow(dead_code)]
fn ffmpeg_error_text(code: i32) -> String {
    let mut buffer = [0i8; 256];
    // SAFETY: `buffer` is a valid writable stack buffer and FFmpeg writes a
    // NUL-terminated error string of at most the provided length.
    let result = unsafe { ffmpeg::ffi::av_strerror(code, buffer.as_mut_ptr(), buffer.len()) };
    if result < 0 {
        return code.to_string();
    }
    // SAFETY: `av_strerror` writes a NUL-terminated string into `buffer` when
    // it succeeds.
    unsafe { CStr::from_ptr(buffer.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::{RangeRequest, resolve_range, safe_file_component};

    #[test]
    fn resolves_standard_and_suffix_ranges() {
        let range = resolve_range(
            RangeRequest {
                start: Some(2),
                end: Some(5),
            },
            10,
        )
        .expect("range");
        assert_eq!(range.start, 2);
        assert_eq!(range.end, 5);

        let suffix = resolve_range(
            RangeRequest {
                start: None,
                end: Some(4),
            },
            10,
        )
        .expect("suffix");
        assert_eq!(suffix.start, 6);
        assert_eq!(suffix.end, 9);
    }

    #[test]
    fn rejects_unsatisfied_ranges() {
        assert!(
            resolve_range(
                RangeRequest {
                    start: Some(99),
                    end: Some(100),
                },
                10,
            )
            .is_none()
        );
    }

    #[test]
    fn sanitizes_session_path_components() {
        assert_eq!(safe_file_component("../abc:def"), ".._abc_def");
    }
}
