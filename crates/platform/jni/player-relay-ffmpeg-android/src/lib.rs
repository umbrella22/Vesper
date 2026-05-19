#![warn(clippy::undocumented_unsafe_blocks)]

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::fs::{self, File};
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use ffmpeg::{Rational, codec, encoder, format, media};
use ffmpeg_next as ffmpeg;
use jni::errors::Result as JniResult;
use jni::objects::{JByteArray, JClass, JObject, JString, JValue};
use jni::signature::RuntimeMethodSignature;
use jni::strings::JNIString;
use jni::sys::{jint, jlong, jobject, jstring};
use jni::{Env, EnvUnowned};
use serde::Deserialize;

const PKG: &str = "io/github/ikaros/vesper/player/android/external/internal/relay/ffmpeg";
const HOST_PREPARED_DASH_INPUT_MODE: &str = "host_prepared_dash_fmp4_tracks";
const DEFAULT_REMUX_TIMEOUT: Duration = Duration::from_secs(90);
const MPEG_TS_RANGE_WAIT: Duration = Duration::from_secs(5);
const HLS_SEGMENT_WAIT: Duration = Duration::from_secs(10);
const STALE_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const HLS_PLAYLIST_CONTENT_TYPE: &str = "application/x-mpegURL";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenRequest {
    session_id: String,
    #[serde(default)]
    input_mode: Option<String>,
    #[serde(default)]
    tracks: Vec<PreparedTrack>,
    #[serde(default)]
    source_uri_hash: Option<String>,
    #[serde(default)]
    source_label: Option<String>,
    fallback_format: FallbackFormat,
    #[serde(default)]
    resource_path: String,
    #[serde(default)]
    range: Option<RangeRequest>,
    #[serde(default = "default_true")]
    enable_range_cache: bool,
    #[serde(default)]
    debug_diagnostics: bool,
    #[serde(default)]
    route_id: Option<String>,
    #[serde(default)]
    route_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreparedTrack {
    kind: String,
    pipe_path: String,
    media_id: String,
    #[serde(default)]
    mime_type: Option<String>,
    #[serde(default)]
    codecs: Option<String>,
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

#[derive(Debug, Clone)]
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
    mpeg_ts_cache: Option<Arc<GrowingCache>>,
    hls_cache: Option<Arc<GrowingCache>>,
}

struct GrowingCache {
    path: PathBuf,
    state: Mutex<GrowingCacheState>,
    ready: Condvar,
}

struct GrowingCacheState {
    available_len: u64,
    complete: bool,
    error: Option<RelayError>,
    last_progress: Instant,
}

impl GrowingCache {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            state: Mutex::new(GrowingCacheState {
                available_len: 0,
                complete: false,
                error: None,
                last_progress: Instant::now(),
            }),
            ready: Condvar::new(),
        }
    }

    fn refresh_available_len(&self) {
        let Ok(metadata) = fs::metadata(&self.path) else {
            return;
        };
        let available_len = metadata.len();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if available_len > state.available_len {
            state.available_len = available_len;
            state.last_progress = Instant::now();
            self.ready.notify_all();
        }
    }

    fn mark_complete(&self) {
        self.refresh_available_len();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.complete = true;
        state.last_progress = Instant::now();
        self.ready.notify_all();
    }

    fn mark_error(&self, error: RelayError) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.error = Some(error);
        state.last_progress = Instant::now();
        self.ready.notify_all();
    }
}

enum NativeStream {
    Bytes(Cursor<Vec<u8>>),
    File(File),
    LimitedFile {
        file: File,
        remaining: u64,
    },
    GrowingFile {
        file: File,
        cache: Arc<GrowingCache>,
        position: u64,
        remaining: Option<u64>,
    },
}

impl Read for NativeStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self {
            NativeStream::Bytes(cursor) => cursor.read(buffer),
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
            NativeStream::GrowingFile {
                file,
                cache,
                position,
                remaining,
            } => read_growing_file(file, cache, position, remaining, buffer),
        }
    }
}

fn read_growing_file(
    file: &mut File,
    cache: &GrowingCache,
    position: &mut u64,
    remaining: &mut Option<u64>,
    buffer: &mut [u8],
) -> std::io::Result<usize> {
    if buffer.is_empty() || remaining.as_ref().is_some_and(|remaining| *remaining == 0) {
        return Ok(0);
    }

    loop {
        cache.refresh_available_len();
        let mut state = cache
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let readable_end = remaining
            .map(|remaining| position.saturating_add(remaining))
            .map(|end| end.min(state.available_len))
            .unwrap_or(state.available_len);
        if *position < readable_end {
            let max_read = (readable_end - *position) as usize;
            let max_read = max_read.min(buffer.len());
            file.seek(SeekFrom::Start(*position))?;
            drop(state);
            let read = file.read(&mut buffer[..max_read])?;
            if read > 0 {
                *position += read as u64;
                if let Some(remaining_bytes) = remaining.as_mut() {
                    *remaining_bytes = remaining_bytes.saturating_sub(read as u64);
                }
                return Ok(read);
            }
            return Ok(0);
        } else if state.complete || state.error.is_some() {
            return Ok(0);
        }

        if Instant::now().duration_since(state.last_progress) >= DEFAULT_REMUX_TIMEOUT {
            return Ok(0);
        }
        let (next_state, _) = cache
            .ready
            .wait_timeout(state, Duration::from_millis(250))
            .unwrap_or_else(|error| error.into_inner());
        state = next_state;
        drop(state);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_external_internal_relay_ffmpeg_VesperRelayFfmpegNative_runtimeMetadata(
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
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_external_internal_relay_ffmpeg_VesperRelayFfmpegNative_open(
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
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_external_internal_relay_ffmpeg_VesperRelayFfmpegNative_prewarm(
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

        output = match prewarm_stream(&request) {
            Ok(()) => open_result_object(
                env,
                OpenResultFields {
                    handle: 0,
                    status: 202,
                    content_type: "application/octet-stream",
                    content_length: -1,
                    headers: Vec::new(),
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
/// # Safety
///
/// This function is called by the JVM through JNI. The JVM must pass a valid
/// `jbyteArray` that belongs to the current JNI frame when `buffer` is non-null.
pub unsafe extern "system" fn Java_io_github_ikaros_vesper_player_android_external_internal_relay_ffmpeg_VesperRelayFfmpegNative_read(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    buffer: jni::sys::jbyteArray,
    offset: jint,
    length: jint,
) -> jint {
    let mut output = -1;
    let _ = unowned_env.with_env(|env| -> JniResult<()> {
        if handle == 0 || length <= 0 || buffer.is_null() {
            output = 0;
            return Ok(());
        }
        if offset < 0 {
            output = 0;
            return Ok(());
        }

        let array = {
            // SAFETY: `buffer` is the byte array passed by the current JNI
            // frame to this native method and is only borrowed for this call.
            unsafe { JByteArray::from_raw(env, buffer) }
        };
        let array_length = array.len(env).unwrap_or(0);
        let offset = offset as usize;
        let length = length as usize;
        let Some(end) = offset.checked_add(length) else {
            output = 0;
            return Ok(());
        };
        if end > array_length {
            output = 0;
            return Ok(());
        }
        let target_length = length;
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
        array.set_region(env, offset as jint, &jbytes)?;
        output = read as jint;
        Ok(())
    });
    output
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_external_internal_relay_ffmpeg_VesperRelayFfmpegNative_close(
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
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_external_internal_relay_ffmpeg_VesperRelayFfmpegNative_invalidate(
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

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_external_internal_relay_ffmpeg_VesperRelayDashBridgeNative_nativeExecuteJson(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    request_json: JString<'_>,
) -> jstring {
    let mut output = std::ptr::null_mut();
    let _ = unowned_env.with_env(|env| -> JniResult<()> {
        let request = match request_json.try_to_string(env) {
            Ok(request) => request.to_string(),
            Err(error) => {
                throw_dash_bridge_exception(
                    env,
                    &format!("Failed to decode DASH bridge request: {error}"),
                )?;
                return Ok(());
            }
        };
        match player_dash_hls_bridge::ops::execute_json(&request) {
            Ok(response) => {
                output = env.new_string(response)?.into_raw();
            }
            Err(error) => {
                throw_dash_bridge_exception(env, &error.to_string())?;
            }
        }
        Ok(())
    });
    output
}

fn throw_dash_bridge_exception(env: &mut Env<'_>, message: &str) -> JniResult<()> {
    env.throw_new(
        jni_name("java/lang/IllegalArgumentException"),
        JNIString::from(message),
    )
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

    let session = session_cache(request)?;
    match request.fallback_format {
        FallbackFormat::MpegTs => {
            let cache = {
                let mut state = session
                    .state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                ensure_mpeg_ts_cache(request, &session.root_dir, &mut state)?
            };
            open_growing_cache_file(request, cache, "video/mp2t")
        }
        FallbackFormat::Hls => {
            let cache = {
                let mut state = session
                    .state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                ensure_hls_cache(request, &session.root_dir, &mut state)?
            };
            open_hls_cache_resource(request, &session.root_dir, cache)
        }
    }
}

fn prewarm_stream(request: &OpenRequest) -> Result<(), RelayError> {
    validate_request(request)?;
    initialize_ffmpeg()?;

    let session = session_cache(request)?;
    let mut state = session
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    match request.fallback_format {
        FallbackFormat::MpegTs => {
            let _ = ensure_mpeg_ts_cache(request, &session.root_dir, &mut state)?;
        }
        FallbackFormat::Hls => {
            let _ = ensure_hls_cache(request, &session.root_dir, &mut state)?;
        }
    }
    Ok(())
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
    if request.input_mode.as_deref() != Some(HOST_PREPARED_DASH_INPUT_MODE) {
        return Err(relay_error(
            "unsupported_dash_layout",
            415,
            "Relay remux requires host-prepared DASH fMP4 track input.",
            request.base_details(),
        ));
    }
    if request.tracks.is_empty() {
        return Err(relay_error(
            "unsupported_dash_layout",
            415,
            "Relay remux request did not include host-prepared tracks.",
            request.base_details(),
        ));
    }
    for track in &request.tracks {
        if track.pipe_path.trim().is_empty() || track.kind.trim().is_empty() {
            return Err(relay_error(
                "unsupported_dash_layout",
                415,
                "Relay remux host-prepared track is missing a kind or FIFO path.",
                request.base_details().into_iter().chain(track.details()),
            ));
        }
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
    cleanup_stale_caches_once();

    let mut sessions = sessions().lock().unwrap_or_else(|error| error.into_inner());
    if let Some(existing) = sessions.get(&request.session_id) {
        return Ok(existing.clone());
    }

    let root_dir = relay_cache_root().join(safe_file_component(&request.session_id));
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
) -> Result<Arc<GrowingCache>, RelayError> {
    if let Some(cache) = state.mpeg_ts_cache.as_ref() {
        return Ok(cache.clone());
    }

    ensure_muxer("mpegts", request)?;
    let output_path = root_dir.join("media.ts");
    let cache = Arc::new(GrowingCache::new(output_path));
    spawn_remux_worker(request.clone(), cache.clone(), OutputKind::MpegTs)?;
    state.mpeg_ts_cache = Some(cache.clone());
    Ok(cache)
}

fn ensure_hls_cache(
    request: &OpenRequest,
    root_dir: &Path,
    state: &mut SessionState,
) -> Result<Arc<GrowingCache>, RelayError> {
    if let Some(cache) = state.hls_cache.as_ref() {
        return Ok(cache.clone());
    }

    ensure_muxer("hls", request)?;
    clean_hls_outputs(root_dir);
    let output_path = root_dir.join("playlist.m3u8");
    let cache = Arc::new(GrowingCache::new(output_path));
    spawn_remux_worker(request.clone(), cache.clone(), OutputKind::Hls)?;
    state.hls_cache = Some(cache.clone());
    Ok(cache)
}

fn spawn_remux_worker(
    request: OpenRequest,
    cache: Arc<GrowingCache>,
    kind: OutputKind,
) -> Result<(), RelayError> {
    let request_for_error = request.clone();
    thread::Builder::new()
        .name("vesper-relay-ffmpeg-remux".to_owned())
        .spawn(move || {
            let result = remux_to_file(&request, &cache.path, kind, Some(cache.as_ref()));
            match result {
                Ok(()) => cache.mark_complete(),
                Err(error) => cache.mark_error(error),
            }
        })
        .map(|_| ())
        .map_err(|error| {
            relay_error(
                "ffmpeg_open_failed",
                503,
                "Failed to start FFmpeg relay remux worker.",
                request_for_error
                    .base_details()
                    .into_iter()
                    .chain([("ioError".to_owned(), error.to_string())]),
            )
        })
}

fn relay_cache_root() -> PathBuf {
    std::env::temp_dir().join("vesper-relay-ffmpeg")
}

fn cleanup_stale_caches_once() {
    static CLEANED: OnceLock<()> = OnceLock::new();
    CLEANED.get_or_init(|| {
        let active_sessions = sessions()
            .lock()
            .map(|sessions| {
                sessions
                    .keys()
                    .map(|session_id| safe_file_component(session_id))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        cleanup_stale_caches_in(&relay_cache_root(), STALE_CACHE_TTL, &active_sessions);
    });
}

fn cleanup_stale_caches_in(root: &Path, ttl: Duration, active_sessions: &[String]) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if active_sessions.iter().any(|active| active == name) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if now.duration_since(modified).is_ok_and(|age| age >= ttl) {
            let _ = fs::remove_dir_all(path);
        }
    }
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

struct PreparedInput {
    context: format::context::Input,
    track: PreparedTrack,
    stream_mapping: Vec<i32>,
    input_time_bases: Vec<Rational>,
    pending: Option<PendingPacket>,
    eof: bool,
}

struct PendingPacket {
    packet: ffmpeg::Packet,
    input_stream_index: usize,
    sort_ts_us: i128,
}

fn remux_to_file(
    request: &OpenRequest,
    output_path: &Path,
    kind: OutputKind,
    progress_cache: Option<&GrowingCache>,
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
    enable_incremental_output(&mut output_context);

    let mut inputs = open_prepared_inputs(request)?;
    let mut output_stream_index = 0;

    for input in &mut inputs {
        let mut stream_mapping = vec![-1; input.context.nb_streams() as usize];
        let mut input_time_bases = vec![Rational(0, 1); input.context.nb_streams() as usize];
        for (input_stream_index, input_stream) in input.context.streams().enumerate() {
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
                            .chain(input.track.details())
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
        input.stream_mapping = stream_mapping;
        input.input_time_bases = input_time_bases;
    }

    if output_stream_index == 0 {
        return Err(relay_error(
            "ffmpeg_open_failed",
            415,
            "DASH source does not contain audio or video streams that can be remuxed.",
            request.base_details(),
        ));
    }

    if let Some(first_input) = inputs.first() {
        output_context.set_metadata(first_input.context.metadata().to_owned());
    }
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
            options.set("hls_time", "4");
            options.set("hls_list_size", "0");
            options.set("hls_playlist_type", "event");
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
    flush_output_context(&mut output_context);
    if let Some(cache) = progress_cache {
        cache.refresh_available_len();
    }

    let output_time_bases = (0..output_stream_index)
        .map(|index| {
            output_context
                .stream(index as usize)
                .map(|stream| stream.time_base())
                .unwrap_or(Rational(0, 1))
        })
        .collect::<Vec<_>>();

    for input in &mut inputs {
        read_next_pending_packet(request, input)?;
    }

    while let Some(input_index) = select_next_input(&inputs) {
        let pending = inputs[input_index].pending.take().ok_or_else(|| {
            relay_error(
                "ffmpeg_open_failed",
                503,
                "FFmpeg relay packet scheduler lost a pending packet.",
                request.base_details(),
            )
        })?;
        write_pending_packet(
            request,
            &mut inputs[input_index],
            pending,
            &output_time_bases,
            &mut output_context,
        )?;
        flush_output_context(&mut output_context);
        if let Some(cache) = progress_cache {
            cache.refresh_available_len();
        }
        read_next_pending_packet(request, &mut inputs[input_index])?;
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
    })?;
    flush_output_context(&mut output_context);
    if let Some(cache) = progress_cache {
        cache.refresh_available_len();
    }
    Ok(())
}

fn open_prepared_inputs(request: &OpenRequest) -> Result<Vec<PreparedInput>, RelayError> {
    let mut inputs = Vec::with_capacity(request.tracks.len());
    for track in &request.tracks {
        let input_context = format::input(&track.pipe_path).map_err(|error| {
            relay_error(
                "ffmpeg_open_failed",
                503,
                "Failed to open host-prepared DASH track with FFmpeg.",
                request
                    .base_details()
                    .into_iter()
                    .chain(track.details())
                    .chain([("ffmpegError".to_owned(), error.to_string())]),
            )
        })?;
        inputs.push(PreparedInput {
            context: input_context,
            track: track.clone(),
            stream_mapping: Vec::new(),
            input_time_bases: Vec::new(),
            pending: None,
            eof: false,
        });
    }
    Ok(inputs)
}

fn read_next_pending_packet(
    request: &OpenRequest,
    input: &mut PreparedInput,
) -> Result<(), RelayError> {
    if input.eof {
        return Ok(());
    }

    loop {
        let mut packet = ffmpeg::Packet::empty();
        match packet.read(&mut input.context) {
            Ok(()) => {
                let input_stream_index = packet.stream();
                if input_stream_index >= input.stream_mapping.len() {
                    continue;
                }
                if input.stream_mapping[input_stream_index] < 0 {
                    continue;
                }
                let sort_ts_us =
                    packet_sort_timestamp_us(&packet, input.input_time_bases[input_stream_index]);
                input.pending = Some(PendingPacket {
                    packet,
                    input_stream_index,
                    sort_ts_us,
                });
                return Ok(());
            }
            Err(ffmpeg::Error::Eof) => {
                input.eof = true;
                return Ok(());
            }
            Err(error) => {
                return Err(relay_error(
                    "ffmpeg_open_failed",
                    503,
                    "Failed to read host-prepared DASH packet with FFmpeg.",
                    request
                        .base_details()
                        .into_iter()
                        .chain(input.track.details())
                        .chain([("ffmpegError".to_owned(), error.to_string())]),
                ));
            }
        }
    }
}

fn select_next_input(inputs: &[PreparedInput]) -> Option<usize> {
    inputs
        .iter()
        .enumerate()
        .filter_map(|(index, input)| {
            input
                .pending
                .as_ref()
                .map(|pending| (index, pending.sort_ts_us))
        })
        .min_by_key(|(index, sort_ts_us)| (*sort_ts_us, *index))
        .map(|(index, _)| index)
}

fn write_pending_packet(
    request: &OpenRequest,
    input: &mut PreparedInput,
    pending: PendingPacket,
    output_time_bases: &[Rational],
    output_context: &mut format::context::Output,
) -> Result<(), RelayError> {
    let output_stream_index = input.stream_mapping[pending.input_stream_index];
    if output_stream_index < 0 {
        return Ok(());
    }
    let output_stream_index = output_stream_index as usize;
    let output_time_base = output_time_bases
        .get(output_stream_index)
        .copied()
        .unwrap_or(Rational(0, 1));
    let mut packet = pending.packet;
    packet.rescale_ts(
        input.input_time_bases[pending.input_stream_index],
        output_time_base,
    );
    packet.set_position(-1);
    packet.set_stream(output_stream_index);
    packet.write_interleaved(output_context).map_err(|error| {
        relay_error(
            "ffmpeg_open_failed",
            503,
            "Failed to write FFmpeg relay packet.",
            request
                .base_details()
                .into_iter()
                .chain(input.track.details())
                .chain([("ffmpegError".to_owned(), error.to_string())]),
        )
    })
}

fn packet_sort_timestamp_us(packet: &ffmpeg::Packet, time_base: Rational) -> i128 {
    let timestamp = packet.dts().or_else(|| packet.pts()).unwrap_or(0) as i128;
    let numerator = time_base.0 as i128;
    let denominator = (time_base.1 as i128).max(1);
    timestamp
        .saturating_mul(numerator)
        .saturating_mul(1_000_000)
        / denominator
}

fn enable_incremental_output(output_context: &mut format::context::Output) {
    // SAFETY: `output_context` owns a live AVFormatContext. The flags changed
    // here are public libavformat fields intended to request packet flushing.
    unsafe {
        let context = output_context.as_mut_ptr();
        if !context.is_null() {
            (*context).flags |= ffmpeg::ffi::AVFMT_FLAG_FLUSH_PACKETS;
            (*context).flush_packets = 1;
        }
    }
}

fn flush_output_context(output_context: &mut format::context::Output) {
    // SAFETY: `output_context` owns a live AVFormatContext. Its `pb` pointer is
    // managed by FFmpeg and may be null for muxers without an AVIOContext.
    unsafe {
        let context = output_context.as_mut_ptr();
        if !context.is_null() {
            let io_context = (*context).pb;
            if !io_context.is_null() {
                ffmpeg::ffi::avio_flush(io_context);
            }
        }
    }
}

fn open_growing_cache_file(
    request: &OpenRequest,
    cache: Arc<GrowingCache>,
    content_type: &str,
) -> Result<OpenedStream, RelayError> {
    if let Some(range) = request.range {
        return open_growing_cache_range(request, cache, content_type, range, MPEG_TS_RANGE_WAIT);
    }

    let (available_len, complete) =
        wait_for_initial_cache_bytes(request, &cache, DEFAULT_REMUX_TIMEOUT)?;
    let file = File::open(&cache.path).map_err(|error| {
        relay_error(
            "range_not_ready",
            416,
            "Failed to open adapted media cache.",
            streaming_details(request, complete, available_len, None)
                .into_iter()
                .chain([("ioError".to_owned(), error.to_string())]),
        )
    })?;
    let handle = next_handle();
    streams()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(
            handle,
            NativeStream::GrowingFile {
                file,
                cache,
                position: 0,
                remaining: None,
            },
        );

    Ok(OpenedStream {
        handle,
        status: 200,
        content_type: content_type.to_owned(),
        content_length: if complete { available_len as i64 } else { -1 },
        headers: response_headers(Vec::new(), request, complete, available_len),
    })
}

fn open_growing_cache_range(
    request: &OpenRequest,
    cache: Arc<GrowingCache>,
    content_type: &str,
    range: RangeRequest,
    wait_timeout: Duration,
) -> Result<OpenedStream, RelayError> {
    let resolved = wait_for_cache_range(request, &cache, range, wait_timeout)?;
    let mut file = File::open(&cache.path).map_err(|error| {
        relay_error(
            "range_not_ready",
            416,
            "Failed to open adapted media cache.",
            streaming_details(
                request,
                resolved.complete,
                resolved.available_len,
                Some(range.to_header_value()),
            )
            .into_iter()
            .chain([("ioError".to_owned(), error.to_string())]),
        )
    })?;
    file.seek(SeekFrom::Start(resolved.range.start))
        .map_err(|error| {
            relay_error(
                "range_not_ready",
                416,
                "Failed to seek adapted media cache.",
                streaming_details(
                    request,
                    resolved.complete,
                    resolved.available_len,
                    Some(range.to_header_value()),
                )
                .into_iter()
                .chain([("ioError".to_owned(), error.to_string())]),
            )
        })?;

    let length = resolved.range.end - resolved.range.start + 1;
    let total = resolved
        .complete_len
        .map(|total| total.to_string())
        .unwrap_or_else(|| "*".to_owned());
    let content_range = format!(
        "bytes {}-{}/{}",
        resolved.range.start, resolved.range.end, total
    );
    let handle = next_handle();
    streams()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(
            handle,
            NativeStream::GrowingFile {
                file,
                cache,
                position: resolved.range.start,
                remaining: Some(length),
            },
        );

    Ok(OpenedStream {
        handle,
        status: 206,
        content_type: content_type.to_owned(),
        content_length: length as i64,
        headers: response_headers(
            vec![("Content-Range".to_owned(), content_range)],
            request,
            resolved.complete,
            resolved.available_len,
        ),
    })
}

struct CacheRangeResolution {
    range: ResolvedRange,
    available_len: u64,
    complete_len: Option<u64>,
    complete: bool,
}

fn wait_for_initial_cache_bytes(
    request: &OpenRequest,
    cache: &GrowingCache,
    wait_timeout: Duration,
) -> Result<(u64, bool), RelayError> {
    let deadline = Instant::now() + wait_timeout;
    loop {
        cache.refresh_available_len();
        let mut state = cache
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(error) = state.error.clone() {
            return Err(error);
        }
        if state.available_len > 0 || state.complete {
            return Ok((state.available_len, state.complete));
        }
        let now = Instant::now();
        if now >= deadline || now.duration_since(state.last_progress) >= wait_timeout {
            return Err(relay_error(
                "remux_timeout",
                504,
                "FFmpeg relay remux did not produce readable output in time.",
                streaming_details(request, state.complete, state.available_len, None),
            ));
        }
        let wait_for = deadline.saturating_duration_since(now);
        let (next_state, _) = cache
            .ready
            .wait_timeout(state, wait_for.min(Duration::from_secs(1)))
            .unwrap_or_else(|error| error.into_inner());
        state = next_state;
        drop(state);
    }
}

fn wait_for_cache_range(
    request: &OpenRequest,
    cache: &GrowingCache,
    range: RangeRequest,
    wait_timeout: Duration,
) -> Result<CacheRangeResolution, RelayError> {
    let requested = range.to_header_value();
    let deadline = Instant::now() + wait_timeout;
    loop {
        cache.refresh_available_len();
        let mut state = cache
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(error) = state.error.clone() {
            return Err(error);
        }
        if let Some(resolution) = resolve_cache_range(range, state.available_len, state.complete) {
            return Ok(resolution);
        }
        if state.complete || Instant::now() >= deadline {
            return Err(relay_error(
                "range_not_ready",
                416,
                "Requested adapted range is not available.",
                streaming_details(
                    request,
                    state.complete,
                    state.available_len,
                    Some(requested.clone()),
                ),
            ));
        }
        let wait_for = deadline.saturating_duration_since(Instant::now());
        let (next_state, _) = cache
            .ready
            .wait_timeout(state, wait_for.min(Duration::from_millis(250)))
            .unwrap_or_else(|error| error.into_inner());
        state = next_state;
        drop(state);
    }
}

fn resolve_cache_range(
    range: RangeRequest,
    available_len: u64,
    complete: bool,
) -> Option<CacheRangeResolution> {
    if complete {
        return resolve_range(range, available_len).map(|range| CacheRangeResolution {
            range,
            available_len,
            complete_len: Some(available_len),
            complete,
        });
    }
    if available_len == 0 {
        return None;
    }
    match (range.start, range.end) {
        (Some(start), Some(end)) if end >= start && end < available_len => {
            Some(CacheRangeResolution {
                range: ResolvedRange { start, end },
                available_len,
                complete_len: None,
                complete,
            })
        }
        (Some(start), None) if start < available_len => Some(CacheRangeResolution {
            range: ResolvedRange {
                start,
                end: available_len - 1,
            },
            available_len,
            complete_len: None,
            complete,
        }),
        _ => None,
    }
}

fn open_hls_cache_resource(
    request: &OpenRequest,
    root_dir: &Path,
    cache: Arc<GrowingCache>,
) -> Result<OpenedStream, RelayError> {
    let resource = request.resource_path.rsplit('/').next().unwrap_or_default();
    if resource.is_empty() || resource.ends_with(".m3u8") {
        let (playlist, available_len, complete) =
            wait_for_hls_playlist_snapshot(request, root_dir, &cache, DEFAULT_REMUX_TIMEOUT)?;
        return open_bytes_stream(
            request,
            playlist,
            HLS_PLAYLIST_CONTENT_TYPE,
            Vec::new(),
            complete,
            available_len,
        );
    }

    let segment_name = safe_file_component(resource);
    let segment_path = root_dir.join(&segment_name);
    wait_for_hls_segment(request, &cache, &segment_path, &segment_name)?;
    open_cached_file(request, segment_path, "video/mp2t".to_owned())
}

fn wait_for_hls_playlist_snapshot(
    request: &OpenRequest,
    root_dir: &Path,
    cache: &GrowingCache,
    wait_timeout: Duration,
) -> Result<(Vec<u8>, u64, bool), RelayError> {
    let deadline = Instant::now() + wait_timeout;
    loop {
        cache.refresh_available_len();
        let state = cache
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(error) = state.error.clone() {
            return Err(error);
        }
        let available_len = state.available_len;
        let complete = state.complete;
        drop(state);
        if let Some(playlist) =
            hls_playlist_snapshot(root_dir, &cache.path, !complete).map_err(|error| {
                relay_error(
                    "ffmpeg_open_failed",
                    503,
                    "Failed to read generated HLS fallback playlist.",
                    streaming_details(request, complete, available_len, None)
                        .into_iter()
                        .chain([("ioError".to_owned(), error.to_string())]),
                )
            })?
        {
            return Ok((playlist, available_len, complete));
        }

        let mut state = cache
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(error) = state.error.clone() {
            return Err(error);
        }
        if state.complete || Instant::now() >= deadline {
            return Err(relay_error(
                "remux_timeout",
                504,
                "FFmpeg relay HLS fallback did not produce a playlist segment in time.",
                streaming_details(request, state.complete, state.available_len, None),
            ));
        }
        let wait_for = deadline.saturating_duration_since(Instant::now());
        let (next_state, _) = cache
            .ready
            .wait_timeout(state, wait_for.min(Duration::from_millis(250)))
            .unwrap_or_else(|error| error.into_inner());
        state = next_state;
        drop(state);
    }
}

fn hls_playlist_snapshot(
    root_dir: &Path,
    playlist_path: &Path,
    require_first_segment: bool,
) -> std::io::Result<Option<Vec<u8>>> {
    let playlist = match fs::read_to_string(playlist_path) {
        Ok(playlist) if !playlist.trim().is_empty() => playlist,
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let root = root_dir.to_string_lossy();
    let mut first_segment_ready = !require_first_segment;
    let rewritten = playlist
        .lines()
        .map(|line| {
            if line.starts_with('#') || line.trim().is_empty() {
                return line.to_owned();
            }
            let without_root = line.strip_prefix(root.as_ref()).unwrap_or(line);
            let file_name = Path::new(without_root)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(without_root)
                .trim_start_matches('/')
                .to_owned();
            if !first_segment_ready {
                let segment_path = root_dir.join(&file_name);
                first_segment_ready = fs::metadata(segment_path)
                    .map(|metadata| metadata.len() > 0)
                    .unwrap_or(false);
            }
            file_name
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !first_segment_ready {
        return Ok(None);
    }
    Ok(Some(format!("{rewritten}\n").into_bytes()))
}

fn wait_for_hls_segment(
    request: &OpenRequest,
    cache: &GrowingCache,
    segment_path: &Path,
    segment_name: &str,
) -> Result<(), RelayError> {
    let deadline = Instant::now() + HLS_SEGMENT_WAIT;
    loop {
        if fs::metadata(segment_path)
            .map(|metadata| metadata.len() > 0)
            .unwrap_or(false)
        {
            return Ok(());
        }
        let mut state = cache
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(error) = state.error.clone() {
            return Err(error);
        }
        if state.complete || Instant::now() >= deadline {
            return Err(relay_error(
                "range_not_ready",
                404,
                "Requested HLS fallback segment is not available.",
                streaming_details(request, state.complete, state.available_len, None)
                    .into_iter()
                    .chain([("segmentName".to_owned(), segment_name.to_owned())]),
            ));
        }
        let wait_for = deadline.saturating_duration_since(Instant::now());
        let (next_state, _) = cache
            .ready
            .wait_timeout(state, wait_for.min(Duration::from_millis(250)))
            .unwrap_or_else(|error| error.into_inner());
        state = next_state;
        drop(state);
    }
}

fn open_bytes_stream(
    request: &OpenRequest,
    bytes: Vec<u8>,
    content_type: &str,
    headers: Vec<(String, String)>,
    complete: bool,
    available_len: u64,
) -> Result<OpenedStream, RelayError> {
    let content_length = bytes.len() as i64;
    let handle = next_handle();
    streams()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(handle, NativeStream::Bytes(Cursor::new(bytes)));
    Ok(OpenedStream {
        handle,
        status: 200,
        content_type: content_type.to_owned(),
        content_length,
        headers: response_headers(headers, request, complete, available_len),
    })
}

fn response_headers(
    mut headers: Vec<(String, String)>,
    request: &OpenRequest,
    complete: bool,
    available_len: u64,
) -> Vec<(String, String)> {
    headers.push((
        "X-Vesper-FFmpeg-Profile-Hash".to_owned(),
        profile_hash().to_owned(),
    ));
    headers.push((
        "X-Vesper-Remux-Cache-State".to_owned(),
        cache_state_name(complete).to_owned(),
    ));
    headers.push((
        "X-Vesper-Remux-Available-Length".to_owned(),
        available_len.to_string(),
    ));
    if request.debug_diagnostics {
        headers.push((
            "X-Vesper-FFmpeg-Configure-Metadata".to_owned(),
            configure_metadata().to_owned(),
        ));
    }
    headers
}

fn streaming_details(
    request: &OpenRequest,
    complete: bool,
    available_len: u64,
    requested_range: Option<String>,
) -> Vec<(String, String)> {
    request
        .base_details()
        .into_iter()
        .chain([
            (
                "cacheState".to_owned(),
                cache_state_name(complete).to_owned(),
            ),
            ("availableLength".to_owned(), available_len.to_string()),
        ])
        .chain(requested_range.map(|range| ("requestedRange".to_owned(), range)))
        .collect()
}

fn cache_state_name(complete: bool) -> &'static str {
    if complete { "complete" } else { "active" }
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

    Ok(OpenedStream {
        handle,
        status,
        content_type,
        content_length,
        headers: response_headers(headers, request, true, total),
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
    if output.is_empty() || output == "." || output == ".." {
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
            (
                "inputMode".to_owned(),
                self.input_mode
                    .clone()
                    .unwrap_or_else(|| "missing".to_owned()),
            ),
        ];
        if let Some(source_uri_hash) = self.source_uri_hash.as_ref() {
            details.push(("sourceUriHash".to_owned(), source_uri_hash.clone()));
        }
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

impl PreparedTrack {
    fn details(&self) -> Vec<(String, String)> {
        let mut details = vec![
            ("trackKind".to_owned(), self.kind.clone()),
            ("mediaId".to_owned(), self.media_id.clone()),
            ("pipePath".to_owned(), self.pipe_path.clone()),
        ];
        if let Some(mime_type) = self.mime_type.as_ref() {
            details.push(("mimeType".to_owned(), mime_type.clone()));
        }
        if let Some(codecs) = self.codecs.as_ref() {
            details.push(("codecs".to_owned(), codecs.clone()));
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
    let mut buffer = [0 as std::ffi::c_char; 256];
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
    use std::fs;
    use std::io::Read;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use serde_json::json;

    use super::{
        FallbackFormat, GrowingCache, HOST_PREPARED_DASH_INPUT_MODE, OpenRequest, PreparedTrack,
        RangeRequest, cleanup_stale_caches_in, hls_playlist_snapshot, open_growing_cache_file,
        open_growing_cache_range, packet_sort_timestamp_us, prewarm_stream, resolve_range,
        safe_file_component, sessions, streams, validate_request,
    };

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
        assert_eq!(safe_file_component(".."), "media");
        assert_eq!(safe_file_component("."), "media");
    }

    #[test]
    fn stale_cleanup_removes_old_inactive_cache_dirs() {
        let root = unique_temp_dir("stale-cleanup");
        let stale = root.join("stale-session");
        let active = root.join("active-session");
        fs::create_dir_all(&stale).expect("stale dir");
        fs::create_dir_all(&active).expect("active dir");

        cleanup_stale_caches_in(&root, Duration::ZERO, &[String::from("active-session")]);

        assert!(!stale.exists());
        assert!(active.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn growing_cache_active_get_omits_content_length_and_reads_available_bytes() {
        let root = unique_temp_dir("growing-get");
        let path = root.join("media.ts");
        fs::write(&path, b"abcd").expect("media");
        let cache = Arc::new(GrowingCache::new(path));
        cache.refresh_available_len();

        let request = test_request(None);
        let opened = open_growing_cache_file(&request, cache, "video/mp2t").expect("open");

        assert_eq!(opened.status, 200);
        assert_eq!(opened.content_length, -1);
        assert_eq!(
            opened
                .headers
                .iter()
                .find(|(name, _)| name == "X-Vesper-Remux-Cache-State")
                .map(|(_, value)| value.as_str()),
            Some("active"),
        );
        let mut buffer = [0u8; 4];
        let read = streams()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get_mut(&opened.handle)
            .expect("stream")
            .read(&mut buffer)
            .expect("read");
        assert_eq!(read, 4);
        assert_eq!(&buffer, b"abcd");
        streams()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&opened.handle);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn growing_cache_future_range_reports_not_ready() {
        let root = unique_temp_dir("future-range");
        let path = root.join("media.ts");
        fs::write(&path, b"abcd").expect("media");
        let cache = Arc::new(GrowingCache::new(path));
        cache.refresh_available_len();
        let range = RangeRequest {
            start: Some(8),
            end: Some(9),
        };
        let request = test_request(Some(range));

        let error = match open_growing_cache_range(
            &request,
            cache,
            "video/mp2t",
            range,
            Duration::from_millis(1),
        ) {
            Ok(_) => panic!("future range should not be ready"),
            Err(error) => error,
        };

        assert_eq!(error.code, "range_not_ready");
        assert!(
            error
                .details
                .contains(&("requestedRange".to_owned(), "bytes=8-9".to_owned(),))
        );
        assert!(
            error
                .details
                .contains(&("availableLength".to_owned(), "4".to_owned(),))
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn completed_growing_cache_range_returns_content_range() {
        let root = unique_temp_dir("completed-range");
        let path = root.join("media.ts");
        fs::write(&path, b"abcd").expect("media");
        let cache = Arc::new(GrowingCache::new(path));
        cache.mark_complete();
        let request = test_request(Some(RangeRequest {
            start: Some(1),
            end: Some(2),
        }));

        let opened = open_growing_cache_file(&request, cache, "video/mp2t").expect("open");

        assert_eq!(opened.status, 206);
        assert_eq!(opened.content_length, 2);
        assert!(
            opened
                .headers
                .contains(&("Content-Range".to_owned(), "bytes 1-2/4".to_owned(),))
        );
        streams()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&opened.handle);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn hls_playlist_snapshot_uses_relative_segments_and_preserves_end_marker() {
        let root = unique_temp_dir("hls-snapshot");
        let segment = root.join("segment_00000.ts");
        let playlist = root.join("playlist.m3u8");
        fs::write(&segment, b"segment").expect("segment");
        fs::write(
            &playlist,
            format!(
                "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\n{}\n#EXT-X-ENDLIST\n",
                segment.display()
            ),
        )
        .expect("playlist");

        let snapshot = hls_playlist_snapshot(&root, &playlist, true)
            .expect("snapshot")
            .expect("ready");
        let snapshot = String::from_utf8(snapshot).expect("utf8");

        assert!(snapshot.contains("segment_00000.ts"));
        assert!(!snapshot.contains(root.to_string_lossy().as_ref()));
        assert!(snapshot.contains("#EXT-X-ENDLIST"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn host_prepared_request_json_round_trips() {
        let request: OpenRequest = serde_json::from_str(
            r#"{
              "sessionId":"session",
              "inputMode":"host_prepared_dash_fmp4_tracks",
              "tracks":[{"kind":"video","pipePath":"/tmp/video.fifo","mediaId":"video0","mimeType":"video/mp4","codecs":"avc1.640028"}],
              "sourceUriHash":"abc123",
              "fallbackFormat":"mpeg_ts",
              "resourcePath":"Episode.ts",
              "enableRangeCache":true
            }"#,
        )
        .expect("request");

        assert_eq!(
            request.input_mode.as_deref(),
            Some(HOST_PREPARED_DASH_INPUT_MODE)
        );
        assert_eq!(request.tracks.len(), 1);
        assert_eq!(request.tracks[0].pipe_path, "/tmp/video.fifo");
        validate_request(&request).expect("valid");
    }

    #[test]
    fn dash_bridge_json_operations_execute_through_relay_crate() {
        let manifest_json = player_dash_hls_bridge::ops::execute_json(
            &json!({
                "operation": "parse_manifest",
                "mpd": r#"
                    <MPD type="static" mediaPresentationDuration="PT9S">
                      <Period>
                        <AdaptationSet mimeType="video/mp4">
                          <Representation id="v1" codecs="avc1.640028">
                            <SegmentTemplate timescale="1" duration="4" startNumber="5"
                              initialization="init-$RepresentationID$.mp4"
                              media="seg-$Number$.m4s" />
                          </Representation>
                        </AdaptationSet>
                      </Period>
                    </MPD>
                "#,
                "manifestUrl": "https://example.com/video/manifest.mpd",
            })
            .to_string(),
        )
        .expect("parse manifest");
        let manifest: serde_json::Value =
            serde_json::from_str(&manifest_json).expect("manifest json");
        assert_eq!(manifest["type"], "static");

        let sidx_json = player_dash_hls_bridge::ops::execute_json(
            &json!({
                "operation": "parse_sidx",
                "data": sidx_box_bytes(),
            })
            .to_string(),
        )
        .expect("parse sidx");
        let sidx: serde_json::Value = serde_json::from_str(&sidx_json).expect("sidx json");
        assert_eq!(sidx["references"][0]["referencedSize"], 100);

        let segments_json = player_dash_hls_bridge::ops::execute_json(
            &json!({
                "operation": "media_segments",
                "segmentBase": {
                    "initialization": {"start": 0, "end": 99},
                    "indexRange": {"start": 100, "end": 199},
                },
                "sidx": sidx,
            })
            .to_string(),
        )
        .expect("media segments");
        let segments: serde_json::Value =
            serde_json::from_str(&segments_json).expect("segments json");
        assert_eq!(segments[0]["range"]["start"], 200);
        assert_eq!(segments[0]["range"]["end"], 299);

        let template_segments_json = player_dash_hls_bridge::ops::execute_json(
            &json!({
                "operation": "template_segments",
                "manifestType": "static",
                "durationMs": 9_000,
                "segmentTemplate": {
                    "timescale": 1,
                    "duration": 4,
                    "startNumber": 5,
                    "presentationTimeOffset": 0,
                    "initialization": "init-$RepresentationID$.mp4",
                    "media": "seg-$Number$.m4s",
                    "timeline": [],
                },
            })
            .to_string(),
        )
        .expect("template segments");
        let template_segments: serde_json::Value =
            serde_json::from_str(&template_segments_json).expect("template segments json");
        assert_eq!(template_segments.as_array().expect("array").len(), 3);
        assert_eq!(template_segments[0]["number"], 5);
    }

    #[test]
    fn rejects_missing_host_prepared_tracks() {
        let mut request = test_request(None);
        request.tracks.clear();

        let error = validate_request(&request).expect_err("invalid");

        assert_eq!(error.code, "unsupported_dash_layout");
    }

    #[test]
    fn prewarm_stream_validates_request() {
        let request = test_request(None);

        prewarm_stream(&request).expect("prewarm");

        let session_key = request.session_id.clone();
        let sessions = sessions().lock().unwrap_or_else(|error| error.into_inner());
        assert!(sessions.contains_key(&session_key));
    }

    #[test]
    fn packet_sort_timestamp_uses_dts_before_pts() {
        let mut packet = ffmpeg_next::Packet::empty();
        packet.set_pts(Some(90));
        packet.set_dts(Some(45));

        assert_eq!(
            packet_sort_timestamp_us(&packet, ffmpeg_next::Rational(1, 90)),
            500_000
        );
    }

    fn test_request(range: Option<RangeRequest>) -> OpenRequest {
        OpenRequest {
            session_id: "session".to_owned(),
            input_mode: Some(HOST_PREPARED_DASH_INPUT_MODE.to_owned()),
            tracks: vec![PreparedTrack {
                kind: "video".to_owned(),
                pipe_path: "/tmp/video.fifo".to_owned(),
                media_id: "video0".to_owned(),
                mime_type: Some("video/mp4".to_owned()),
                codecs: Some("avc1.640028".to_owned()),
            }],
            source_uri_hash: Some("sourcehash".to_owned()),
            source_label: Some("Episode".to_owned()),
            fallback_format: FallbackFormat::MpegTs,
            resource_path: "Episode.ts".to_owned(),
            range,
            enable_range_cache: true,
            debug_diagnostics: false,
            route_id: Some("route".to_owned()),
            route_name: Some("Living Room".to_owned()),
        }
    }

    fn unique_temp_dir(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "vesper-relay-ffmpeg-test-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temp dir");
        path
    }

    fn sidx_box_bytes() -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend([0, 0, 0, 0]);
        payload.extend(1_u32.to_be_bytes());
        payload.extend(1_000_u32.to_be_bytes());
        payload.extend(0_u32.to_be_bytes());
        payload.extend(0_u32.to_be_bytes());
        payload.extend(0_u16.to_be_bytes());
        payload.extend(1_u16.to_be_bytes());
        payload.extend(100_u32.to_be_bytes());
        payload.extend(4_000_u32.to_be_bytes());
        payload.extend(0x9000_0000_u32.to_be_bytes());

        let mut output = Vec::new();
        output.extend((8 + payload.len() as u32).to_be_bytes());
        output.extend(*b"sidx");
        output.extend(payload);
        output
    }
}
