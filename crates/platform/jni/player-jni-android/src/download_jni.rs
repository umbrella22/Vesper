use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use jni::errors::{Result as JniResult, ThrowRuntimeExAndDefault};
use jni::objects::{Global, JClass, JObject, JObjectArray, JString, JValue};
use jni::sys::{jboolean, jint, jlong, jobject, jobjectArray};
use jni::{Env, EnvUnowned, JavaVM};
use player_model::MediaSource;
use player_platform_android::{AndroidDownloadBridgeSession, AndroidDownloadCommand};
use player_plugin::{OutputFormat, ProcessorProgress};
use player_plugin_loader::PluginRegistry;
use player_runtime::{
    DownloadAssetId, DownloadAssetIndex, DownloadAssetStream, DownloadByteRange,
    DownloadContentFormat, DownloadErrorSummary, DownloadEvent, DownloadEventBatch,
    DownloadProfile, DownloadProgressSnapshot, DownloadResourceRecord, DownloadSegmentRecord,
    DownloadSource, DownloadStreamKind, DownloadTaskId, DownloadTaskSnapshot, DownloadTaskStatus,
    PlayerError,
};

use crate::plugin_registry_jni::{clone_android_plugin_registry, parse_plugin_references};
use crate::{
    HandleRegistry, PKG, error_category_from_jni_ordinal, error_code_from_jni_ordinal, field_sig,
    jni_name, lock_or_recover, method_sig,
    parsers::{bool_field, int_field, long_field, string_field},
    run_jni_entry, u64_to_jlong_saturating,
};

const EXPORT_CALLBACK_QUEUE_CAPACITY: usize = 32;
const EXPORT_CALLBACK_WAIT_TIMEOUT: Duration = Duration::from_secs(2);

struct PersistentBoundedWorker<M> {
    sender: Mutex<Option<mpsc::SyncSender<M>>>,
    finished: Mutex<mpsc::Receiver<()>>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl<M: Send + 'static> PersistentBoundedWorker<M> {
    fn spawn(
        name: &str,
        capacity: usize,
        run: impl FnOnce(mpsc::Receiver<M>) + Send + 'static,
    ) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(capacity.max(1));
        let (finished_tx, finished_rx) = mpsc::sync_channel(1);
        let join = std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(receiver)));
                let _ = finished_tx.send(());
            })?;
        Ok(Self {
            sender: Mutex::new(Some(sender)),
            finished: Mutex::new(finished_rx),
            join: Mutex::new(Some(join)),
        })
    }

    fn try_send(&self, message: M) -> Result<(), mpsc::TrySendError<M>> {
        let sender = lock_or_recover(&self.sender).clone();
        match sender {
            Some(sender) => sender.try_send(message),
            None => Err(mpsc::TrySendError::Disconnected(message)),
        }
    }

    fn send_timeout(&self, message: M, timeout: Duration) -> Result<(), M> {
        let deadline = Instant::now() + timeout;
        let mut pending = message;
        loop {
            match self.try_send(pending) {
                Ok(()) => return Ok(()),
                Err(mpsc::TrySendError::Disconnected(message)) => return Err(message),
                Err(mpsc::TrySendError::Full(message)) => {
                    pending = message;
                    if Instant::now() >= deadline {
                        return Err(pending);
                    }
                    std::thread::park_timeout(
                        deadline
                            .saturating_duration_since(Instant::now())
                            .min(Duration::from_millis(5)),
                    );
                }
            }
        }
    }
}

impl<M> Drop for PersistentBoundedWorker<M> {
    fn drop(&mut self) {
        lock_or_recover(&self.sender).take();
        let finished = lock_or_recover(&self.finished)
            .recv_timeout(EXPORT_CALLBACK_WAIT_TIMEOUT)
            .is_ok();
        let join = lock_or_recover(&self.join).take();
        if finished && let Some(join) = join {
            let _ = join.join();
        }
    }
}

struct AndroidJniDownloadSession {
    bridge: Mutex<AndroidDownloadBridgeSession>,
    export_worker: Arc<JniDownloadExportCallbackWorker>,
    _registry: Option<Arc<PluginRegistry>>,
}

type SharedAndroidJniDownloadSession = Arc<AndroidJniDownloadSession>;

#[derive(Debug)]
struct AndroidDownloadSessionConfig {
    auto_start: bool,
    run_post_processors_on_completion: bool,
    registry_handle: jlong,
    post_download_references_json: String,
    event_hook_references_json: String,
}

struct JniDownloadExportProgress {
    worker: Arc<JniDownloadExportCallbackWorker>,
    callback: Option<Arc<Global<JObject<'static>>>>,
}

enum JniDownloadExportCallbackRequest {
    Progress {
        callback: Arc<Global<JObject<'static>>>,
        ratio: f32,
    },
    IsCancelled {
        callback: Arc<Global<JObject<'static>>>,
        reply: mpsc::SyncSender<bool>,
    },
    Barrier {
        reply: mpsc::SyncSender<()>,
    },
}

struct JniDownloadExportCallbackWorker {
    worker: PersistentBoundedWorker<JniDownloadExportCallbackRequest>,
}

impl JniDownloadExportCallbackWorker {
    fn new(java_vm: JavaVM) -> Result<Self, String> {
        let worker = PersistentBoundedWorker::spawn(
            "vesper-jni-export-cb",
            EXPORT_CALLBACK_QUEUE_CAPACITY,
            move |receiver| {
                let result: JniResult<()> = java_vm.attach_current_thread(|env| {
                    while let Ok(request) = receiver.recv() {
                        match request {
                            JniDownloadExportCallbackRequest::Progress { callback, ratio } => {
                                if env
                                    .call_method(
                                        callback.as_obj(),
                                        jni_name("onProgress"),
                                        method_sig("(F)V").method_signature(),
                                        &[JValue::Float(ratio)],
                                    )
                                    .is_err()
                                {
                                    env.exception_clear();
                                }
                            }
                            JniDownloadExportCallbackRequest::IsCancelled { callback, reply } => {
                                let cancelled = env
                                    .call_method(
                                        callback.as_obj(),
                                        jni_name("isCancelled"),
                                        method_sig("()Z").method_signature(),
                                        &[],
                                    )
                                    .and_then(|value| value.z())
                                    .unwrap_or_else(|_| {
                                        env.exception_clear();
                                        true
                                    });
                                let _ = reply.send(cancelled);
                            }
                            JniDownloadExportCallbackRequest::Barrier { reply } => {
                                let _ = reply.send(());
                            }
                        }
                    }
                    Ok(())
                });
                if let Err(error) = result {
                    eprintln!("vesper export progress worker failed: {error}");
                }
            },
        )
        .map_err(|error| format!("failed to start Android export callback worker: {error}"))?;
        Ok(Self { worker })
    }

    fn on_progress(&self, callback: Arc<Global<JObject<'static>>>, ratio: f32) {
        let _ = self
            .worker
            .try_send(JniDownloadExportCallbackRequest::Progress { callback, ratio });
    }

    fn is_cancelled(&self, callback: Arc<Global<JObject<'static>>>) -> bool {
        let (reply, response) = mpsc::sync_channel(1);
        if self
            .worker
            .send_timeout(
                JniDownloadExportCallbackRequest::IsCancelled { callback, reply },
                EXPORT_CALLBACK_WAIT_TIMEOUT,
            )
            .is_err()
        {
            return true;
        }
        response
            .recv_timeout(EXPORT_CALLBACK_WAIT_TIMEOUT)
            .unwrap_or(true)
    }

    fn flush(&self) {
        let (reply, response) = mpsc::sync_channel(1);
        if self
            .worker
            .send_timeout(
                JniDownloadExportCallbackRequest::Barrier { reply },
                EXPORT_CALLBACK_WAIT_TIMEOUT,
            )
            .is_ok()
        {
            let _ = response.recv_timeout(EXPORT_CALLBACK_WAIT_TIMEOUT);
        }
    }
}

impl ProcessorProgress for JniDownloadExportProgress {
    fn on_progress(&self, ratio: f32) {
        if let Some(callback) = self.callback.clone() {
            self.worker.on_progress(callback, ratio);
        }
    }

    fn is_cancelled(&self) -> bool {
        self.callback
            .clone()
            .is_some_and(|callback| self.worker.is_cancelled(callback))
    }
}

static DOWNLOAD_SESSIONS: OnceLock<Mutex<HandleRegistry<SharedAndroidJniDownloadSession>>> =
    OnceLock::new();

fn download_sessions() -> &'static Mutex<HandleRegistry<SharedAndroidJniDownloadSession>> {
    DOWNLOAD_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

fn invalid_download_handle_error() -> &'static str {
    "invalid android JNI download session handle"
}

fn download_session(env: &mut Env<'_>, handle: jlong) -> Option<SharedAndroidJniDownloadSession> {
    let guard = lock_or_recover(download_sessions());
    let Some(session) = guard.get(handle).cloned() else {
        let _ = env.throw_new(
            jni_name("java/lang/IllegalArgumentException"),
            jni_name(invalid_download_handle_error()),
        );
        return None;
    };
    Some(session)
}

fn with_download_session_mut<R>(
    env: &mut Env<'_>,
    handle: jlong,
    f: impl FnOnce(&mut AndroidDownloadBridgeSession) -> R,
) -> Option<R> {
    let session = download_session(env, handle)?;

    // Do not call back into Java while the session lock is held; the same handle could reenter.
    let mut session = lock_or_recover(&session.bridge);
    Some(f(&mut session))
}

fn with_download_session<R>(
    env: &mut Env<'_>,
    handle: jlong,
    f: impl FnOnce(&AndroidDownloadBridgeSession) -> R,
) -> Option<R> {
    let session = download_session(env, handle)?;

    // Read-only paths still hold the session lock; closures must not trigger reentrant JNI callbacks.
    let session = lock_or_recover(&session.bridge);
    Some(f(&session))
}

fn new_download_session(
    config: AndroidDownloadSessionConfig,
    java_vm: JavaVM,
) -> Result<jlong, String> {
    let post_download_references = parse_plugin_references(&config.post_download_references_json)?;
    let event_hook_references = parse_plugin_references(&config.event_hook_references_json)?;
    let has_references = !post_download_references.is_empty() || !event_hook_references.is_empty();
    let registry = if has_references {
        if config.registry_handle == 0 {
            return Err(
                "Android plugin registry handle is required for download plugin references"
                    .to_owned(),
            );
        }
        Some(clone_android_plugin_registry(config.registry_handle).map_err(str::to_owned)?)
    } else {
        if config.registry_handle != 0 {
            return Err(
                "Android plugin registry handle must be zero when download plugin references are empty"
                    .to_owned(),
            );
        }
        None
    };
    let empty_registry = PluginRegistry::default();
    let bridge = AndroidDownloadBridgeSession::new_with_plugin_registry(
        config.auto_start,
        config.run_post_processors_on_completion,
        registry.as_deref().unwrap_or(&empty_registry),
        post_download_references,
        event_hook_references,
    )
    .map_err(|error| error.to_string())?;
    let export_worker = Arc::new(JniDownloadExportCallbackWorker::new(java_vm)?);
    let session = Arc::new(AndroidJniDownloadSession {
        bridge: Mutex::new(bridge),
        export_worker,
        _registry: registry,
    });
    let mut guard = lock_or_recover(download_sessions());
    let handle = guard.insert(session);
    if handle == 0 {
        return Err("android JNI download session registry overflow".to_owned());
    }
    Ok(handle)
}

fn optional_java_string<'local>(
    env: &mut Env<'local>,
    value: Option<&str>,
) -> JniResult<JObject<'local>> {
    match value {
        Some(value) => Ok(JObject::from(env.new_string(value)?)),
        None => Ok(JObject::null()),
    }
}

fn string_array_field(
    env: &mut Env<'_>,
    object: &JObject<'_>,
    field_name: &str,
) -> JniResult<Vec<String>> {
    let value = env
        .get_field(
            object,
            jni_name(field_name),
            field_sig("[Ljava/lang/String;").field_signature(),
        )?
        .l()?;
    if value.is_null() {
        return Ok(Vec::new());
    }

    // SAFETY: `value` is a local reference whose raw pointer was just produced by
    // `into_raw()` on the same thread-attached `env`. `JObjectArray::from_raw` re-wraps
    // the raw `jobjectArray` without dereferencing foreign memory; the borrow is bounded
    // by `env`'s local frame and the local reference is released when `value` drops.
    let array =
        unsafe { JObjectArray::<JString<'_>>::from_raw(env, value.into_raw() as jobjectArray) };
    let len = array.len(env)?;
    let mut values = Vec::with_capacity(len);
    for index in 0..len {
        let value = array.get_element(env, index)?;
        if !value.is_null() {
            values.push(value.try_to_string(env)?);
        }
    }
    Ok(values)
}

fn object_field<'local>(
    env: &mut Env<'local>,
    object: &JObject<'local>,
    field_name: &str,
    class_name: &str,
) -> JniResult<Option<JObject<'local>>> {
    let value = env
        .get_field(
            object,
            jni_name(field_name),
            field_sig(format!("L{class_name};")).field_signature(),
        )?
        .l()?;
    if value.is_null() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

fn download_config_from_java(
    env: &mut Env<'_>,
    config: JObject<'_>,
) -> JniResult<AndroidDownloadSessionConfig> {
    Ok(AndroidDownloadSessionConfig {
        auto_start: bool_field(env, &config, "autoStart")?,
        run_post_processors_on_completion: bool_field(
            env,
            &config,
            "runPostProcessorsOnCompletion",
        )?,
        registry_handle: long_field(env, &config, "pluginRegistryHandle")?,
        post_download_references_json: string_field(
            env,
            &config,
            "postDownloadPluginReferencesJson",
        )?
        .unwrap_or_else(|| "[]".to_owned()),
        event_hook_references_json: string_field(env, &config, "eventHookPluginReferencesJson")?
            .unwrap_or_else(|| "[]".to_owned()),
    })
}

fn download_source_from_java(env: &mut Env<'_>, source: JObject<'_>) -> JniResult<DownloadSource> {
    let source_uri = string_field(env, &source, "sourceUri")?.unwrap_or_default();
    let content_format = match int_field(env, &source, "contentFormatOrdinal")? {
        0 => DownloadContentFormat::HlsSegments,
        1 => DownloadContentFormat::DashSegments,
        2 => DownloadContentFormat::FlvSegments,
        3 => DownloadContentFormat::SingleFile,
        _ => DownloadContentFormat::Unknown,
    };
    let header_names = string_array_field(env, &source, "headerNames")?;
    let header_values = string_array_field(env, &source, "headerValues")?;
    let mut download_source = DownloadSource::new(MediaSource::new(source_uri), content_format)
        .with_request_headers(header_names.into_iter().zip(header_values));
    if let Some(manifest_uri) = string_field(env, &source, "manifestUri")?
        && !manifest_uri.is_empty()
    {
        download_source = download_source.with_manifest_uri(manifest_uri);
    }
    Ok(download_source)
}

fn download_profile_from_java(
    env: &mut Env<'_>,
    profile: JObject<'_>,
) -> JniResult<DownloadProfile> {
    Ok(DownloadProfile {
        variant_id: string_field(env, &profile, "variantId")?,
        preferred_audio_language: string_field(env, &profile, "preferredAudioLanguage")?,
        preferred_subtitle_language: string_field(env, &profile, "preferredSubtitleLanguage")?,
        selected_track_ids: string_array_field(env, &profile, "selectedTrackIds")?,
        target_output_format: match int_field(env, &profile, "targetOutputFormatOrdinal")? {
            0 => Some(OutputFormat::Mp4),
            1 => Some(OutputFormat::Mkv),
            2 => Some(OutputFormat::Original),
            _ => None,
        },
        target_directory: string_field(env, &profile, "targetDirectory")?.map(PathBuf::from),
        allow_metered_network: bool_field(env, &profile, "allowMeteredNetwork")?,
    })
}

fn download_resource_record_from_java<'local>(
    env: &mut Env<'local>,
    resource: JObject<'local>,
) -> JniResult<DownloadResourceRecord> {
    let byte_range = object_field(
        env,
        &resource,
        "byteRange",
        &format!("{PKG}/NativeDownloadByteRange"),
    )?
    .map(|byte_range| download_byte_range_from_java(env, byte_range))
    .transpose()?;
    Ok(DownloadResourceRecord {
        resource_id: string_field(env, &resource, "resourceId")?.unwrap_or_default(),
        uri: string_field(env, &resource, "uri")?.unwrap_or_default(),
        relative_path: string_field(env, &resource, "relativePath")?.map(PathBuf::from),
        byte_range,
        generated_text: string_field(env, &resource, "generatedText")?,
        size_bytes: bool_field(env, &resource, "hasSizeBytes")?
            .then_some(long_field(env, &resource, "sizeBytes")?.max(0) as u64),
        etag: string_field(env, &resource, "etag")?,
        checksum: string_field(env, &resource, "checksum")?,
    })
}

fn download_segment_record_from_java<'local>(
    env: &mut Env<'local>,
    segment: JObject<'local>,
) -> JniResult<DownloadSegmentRecord> {
    let byte_range = object_field(
        env,
        &segment,
        "byteRange",
        &format!("{PKG}/NativeDownloadByteRange"),
    )?
    .map(|byte_range| download_byte_range_from_java(env, byte_range))
    .transpose()?;
    Ok(DownloadSegmentRecord {
        segment_id: string_field(env, &segment, "segmentId")?.unwrap_or_default(),
        uri: string_field(env, &segment, "uri")?.unwrap_or_default(),
        relative_path: string_field(env, &segment, "relativePath")?.map(PathBuf::from),
        sequence: bool_field(env, &segment, "hasSequence")?
            .then_some(long_field(env, &segment, "sequence")?.max(0) as u64),
        byte_range,
        size_bytes: bool_field(env, &segment, "hasSizeBytes")?
            .then_some(long_field(env, &segment, "sizeBytes")?.max(0) as u64),
        checksum: string_field(env, &segment, "checksum")?,
    })
}

fn download_byte_range_from_java<'local>(
    env: &mut Env<'local>,
    byte_range: JObject<'local>,
) -> JniResult<DownloadByteRange> {
    Ok(DownloadByteRange {
        offset: long_field(env, &byte_range, "offset")?.max(0) as u64,
        length: long_field(env, &byte_range, "length")?.max(0) as u64,
    })
}

fn download_resource_records_from_java(
    env: &mut Env<'_>,
    object: &JObject<'_>,
) -> JniResult<Vec<DownloadResourceRecord>> {
    let value = env
        .get_field(
            object,
            jni_name("resources"),
            field_sig(format!("[L{PKG}/NativeDownloadResourceRecord;")).field_signature(),
        )?
        .l()?;
    if value.is_null() {
        return Ok(Vec::new());
    }

    // SAFETY: `value` is a local reference whose raw pointer was just produced by
    // `into_raw()` on the same thread-attached `env`. `JObjectArray::from_raw` re-wraps
    // the raw `jobjectArray` without dereferencing foreign memory; the borrow is bounded
    // by `env`'s local frame and the local reference is released when `value` drops.
    let array =
        unsafe { JObjectArray::<JObject<'_>>::from_raw(env, value.into_raw() as jobjectArray) };
    let len = array.len(env)?;
    let mut resources = Vec::with_capacity(len);
    for index in 0..len {
        let resource = array.get_element(env, index)?;
        if !resource.is_null() {
            resources.push(download_resource_record_from_java(env, resource)?);
        }
    }
    Ok(resources)
}

fn download_segment_records_from_java(
    env: &mut Env<'_>,
    object: &JObject<'_>,
) -> JniResult<Vec<DownloadSegmentRecord>> {
    let value = env
        .get_field(
            object,
            jni_name("segments"),
            field_sig(format!("[L{PKG}/NativeDownloadSegmentRecord;")).field_signature(),
        )?
        .l()?;
    if value.is_null() {
        return Ok(Vec::new());
    }

    // SAFETY: `value` is a local reference whose raw pointer was just produced by
    // `into_raw()` on the same thread-attached `env`. `JObjectArray::from_raw` re-wraps
    // the raw `jobjectArray` without dereferencing foreign memory; the borrow is bounded
    // by `env`'s local frame and the local reference is released when `value` drops.
    let array =
        unsafe { JObjectArray::<JObject<'_>>::from_raw(env, value.into_raw() as jobjectArray) };
    let len = array.len(env)?;
    let mut segments = Vec::with_capacity(len);
    for index in 0..len {
        let segment = array.get_element(env, index)?;
        if !segment.is_null() {
            segments.push(download_segment_record_from_java(env, segment)?);
        }
    }
    Ok(segments)
}

fn download_asset_stream_from_java(
    env: &mut Env<'_>,
    stream: JObject<'_>,
) -> JniResult<DownloadAssetStream> {
    let metadata_keys = string_array_field(env, &stream, "metadataKeys")?;
    let metadata_values = string_array_field(env, &stream, "metadataValues")?;
    let metadata = metadata_keys
        .into_iter()
        .zip(metadata_values)
        .collect::<HashMap<_, _>>();
    Ok(DownloadAssetStream {
        stream_id: string_field(env, &stream, "streamId")?.unwrap_or_default(),
        kind: match int_field(env, &stream, "kindOrdinal")? {
            1 => DownloadStreamKind::Video,
            2 => DownloadStreamKind::Audio,
            3 => DownloadStreamKind::SecondaryAudio,
            4 => DownloadStreamKind::Subtitle,
            5 => DownloadStreamKind::Auxiliary,
            _ => DownloadStreamKind::Combined,
        },
        language: string_field(env, &stream, "language")?,
        codec: string_field(env, &stream, "codec")?,
        label: string_field(env, &stream, "label")?,
        quality_rank: bool_field(env, &stream, "hasQualityRank")?
            .then_some(int_field(env, &stream, "qualityRank")?.max(0) as u32),
        resource_ids: string_array_field(env, &stream, "resourceIds")?,
        segment_ids: string_array_field(env, &stream, "segmentIds")?,
        metadata,
    })
}

fn download_asset_streams_from_java(
    env: &mut Env<'_>,
    object: &JObject<'_>,
) -> JniResult<Vec<DownloadAssetStream>> {
    let value = env
        .get_field(
            object,
            jni_name("streams"),
            field_sig(format!("[L{PKG}/NativeDownloadAssetStream;")).field_signature(),
        )?
        .l()?;
    if value.is_null() {
        return Ok(Vec::new());
    }

    // SAFETY: `value` is a local reference whose raw pointer was just produced by
    // `into_raw()` on the same thread-attached `env`. `JObjectArray::from_raw` re-wraps
    // the raw `jobjectArray` without dereferencing foreign memory; the borrow is bounded
    // by `env`'s local frame and the local reference is released when `value` drops.
    let array =
        unsafe { JObjectArray::<JObject<'_>>::from_raw(env, value.into_raw() as jobjectArray) };
    let len = array.len(env)?;
    let mut streams = Vec::with_capacity(len);
    for index in 0..len {
        let stream = array.get_element(env, index)?;
        if !stream.is_null() {
            streams.push(download_asset_stream_from_java(env, stream)?);
        }
    }
    Ok(streams)
}

fn download_asset_index_from_java(
    env: &mut Env<'_>,
    asset_index: JObject<'_>,
) -> JniResult<DownloadAssetIndex> {
    Ok(DownloadAssetIndex {
        content_format: match int_field(env, &asset_index, "contentFormatOrdinal")? {
            0 => DownloadContentFormat::HlsSegments,
            1 => DownloadContentFormat::DashSegments,
            2 => DownloadContentFormat::FlvSegments,
            3 => DownloadContentFormat::SingleFile,
            _ => DownloadContentFormat::Unknown,
        },
        version: string_field(env, &asset_index, "version")?,
        etag: string_field(env, &asset_index, "etag")?,
        checksum: string_field(env, &asset_index, "checksum")?,
        total_size_bytes: bool_field(env, &asset_index, "hasTotalSizeBytes")?
            .then_some(long_field(env, &asset_index, "totalSizeBytes")?.max(0) as u64),
        resources: download_resource_records_from_java(env, &asset_index)?,
        segments: download_segment_records_from_java(env, &asset_index)?,
        streams: download_asset_streams_from_java(env, &asset_index)?,
        completed_path: string_field(env, &asset_index, "completedPath")?.map(PathBuf::from),
    })
}

fn download_progress_from_java(
    env: &mut Env<'_>,
    progress: JObject<'_>,
) -> JniResult<DownloadProgressSnapshot> {
    Ok(DownloadProgressSnapshot {
        received_bytes: long_field(env, &progress, "receivedBytes")?.max(0) as u64,
        total_bytes: bool_field(env, &progress, "hasTotalBytes")?
            .then_some(long_field(env, &progress, "totalBytes")?.max(0) as u64),
        received_segments: int_field(env, &progress, "receivedSegments")?.max(0) as u32,
        total_segments: bool_field(env, &progress, "hasTotalSegments")?
            .then_some(int_field(env, &progress, "totalSegments")?.max(0) as u32),
    })
}

fn download_task_from_java<'local>(
    env: &mut Env<'local>,
    task: JObject<'local>,
    now: Instant,
) -> JniResult<DownloadTaskSnapshot> {
    let source = object_field(env, &task, "source", &format!("{PKG}/NativeDownloadSource"))?
        .ok_or_else(|| jni::errors::Error::NullPtr("task.source"))?;
    let profile = object_field(
        env,
        &task,
        "profile",
        &format!("{PKG}/NativeDownloadProfile"),
    )?
    .ok_or_else(|| jni::errors::Error::NullPtr("task.profile"))?;
    let progress = object_field(
        env,
        &task,
        "progress",
        &format!("{PKG}/NativeDownloadProgress"),
    )?
    .ok_or_else(|| jni::errors::Error::NullPtr("task.progress"))?;
    let asset_index = object_field(
        env,
        &task,
        "assetIndex",
        &format!("{PKG}/NativeDownloadAssetIndex"),
    )?
    .ok_or_else(|| jni::errors::Error::NullPtr("task.assetIndex"))?;
    let error_summary = if bool_field(env, &task, "hasError")? {
        Some(DownloadErrorSummary {
            code: error_code_from_jni_ordinal(int_field(env, &task, "errorCodeOrdinal")?),
            category: error_category_from_jni_ordinal(int_field(
                env,
                &task,
                "errorCategoryOrdinal",
            )?),
            retriable: bool_field(env, &task, "errorRetriable")?,
            message: string_field(env, &task, "errorMessage")?
                .unwrap_or_else(|| "download failed".to_owned()),
        })
    } else {
        None
    };

    Ok(DownloadTaskSnapshot {
        task_id: DownloadTaskId::from_raw(long_field(env, &task, "taskId")?.max(0) as u64),
        asset_id: DownloadAssetId::new(string_field(env, &task, "assetId")?.unwrap_or_default()),
        source: download_source_from_java(env, source)?,
        profile: download_profile_from_java(env, profile)?,
        status: match int_field(env, &task, "statusOrdinal")? {
            0 => DownloadTaskStatus::Queued,
            1 => DownloadTaskStatus::Preparing,
            2 => DownloadTaskStatus::Downloading,
            3 => DownloadTaskStatus::Paused,
            4 => DownloadTaskStatus::Completed,
            5 => DownloadTaskStatus::Failed,
            6 => DownloadTaskStatus::Removed,
            _ => DownloadTaskStatus::Queued,
        },
        progress: download_progress_from_java(env, progress)?,
        asset_index: Arc::new(download_asset_index_from_java(env, asset_index)?),
        created_at: now,
        updated_at: now,
        error_summary,
    })
}

fn java_string_array_object<'local>(
    env: &mut Env<'local>,
    values: &[String],
) -> JniResult<JObject<'local>> {
    let string_class = env.find_class(jni_name("java/lang/String"))?;
    let array: JObjectArray<'_> =
        env.new_object_array(values.len() as i32, string_class, JObject::null())?;
    for (index, value) in values.iter().enumerate() {
        let value = JObject::from(env.new_string(value.as_str())?);
        array.set_element(env, index, value)?;
    }
    Ok(array.into())
}

fn download_source_object<'local>(
    env: &mut Env<'local>,
    source: &DownloadSource,
) -> JniResult<JObject<'local>> {
    let class = env.find_class(jni_name(format!("{PKG}/NativeDownloadSource")))?;
    let source_uri = JObject::from(env.new_string(source.source.uri())?);
    let manifest_uri = optional_java_string(env, source.manifest_uri.as_deref())?;
    let header_entries = source.request_headers.iter().collect::<Vec<_>>();
    let header_names = java_string_array_object(
        env,
        &header_entries
            .iter()
            .map(|(name, _)| (*name).clone())
            .collect::<Vec<_>>(),
    )?;
    let header_values = java_string_array_object(
        env,
        &header_entries
            .iter()
            .map(|(_, value)| (*value).clone())
            .collect::<Vec<_>>(),
    )?;
    env.new_object(
        class,
        method_sig(
            "(Ljava/lang/String;ILjava/lang/String;[Ljava/lang/String;[Ljava/lang/String;)V",
        )
        .method_signature(),
        &[
            JValue::Object(&source_uri),
            JValue::Int(match source.content_format {
                DownloadContentFormat::HlsSegments => 0,
                DownloadContentFormat::DashSegments => 1,
                DownloadContentFormat::FlvSegments => 2,
                DownloadContentFormat::SingleFile => 3,
                DownloadContentFormat::Unknown => 4,
            }),
            JValue::Object(&manifest_uri),
            JValue::Object(&header_names),
            JValue::Object(&header_values),
        ],
    )
}

fn download_profile_object<'local>(
    env: &mut Env<'local>,
    profile: &DownloadProfile,
) -> JniResult<JObject<'local>> {
    let class = env.find_class(jni_name(format!("{PKG}/NativeDownloadProfile")))?;
    let variant_id = optional_java_string(env, profile.variant_id.as_deref())?;
    let preferred_audio_language =
        optional_java_string(env, profile.preferred_audio_language.as_deref())?;
    let preferred_subtitle_language =
        optional_java_string(env, profile.preferred_subtitle_language.as_deref())?;
    let selected_track_ids = java_string_array_object(env, &profile.selected_track_ids)?;
    let target_directory = optional_java_string(
        env,
        profile
            .target_directory
            .as_ref()
            .and_then(|path| path.to_str()),
    )?;
    env.new_object(
        class,
        method_sig(
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;ILjava/lang/String;Z)V",
        )
        .method_signature(),
        &[
            JValue::Object(&variant_id),
            JValue::Object(&preferred_audio_language),
            JValue::Object(&preferred_subtitle_language),
            JValue::Object(&selected_track_ids),
            JValue::Int(match profile.target_output_format {
                Some(OutputFormat::Mp4) => 0,
                Some(OutputFormat::Mkv) => 1,
                Some(OutputFormat::Original) => 2,
                None => -1,
            }),
            JValue::Object(&target_directory),
            JValue::Bool(profile.allow_metered_network),
        ],
    )
}

fn download_resource_record_object<'local>(
    env: &mut Env<'local>,
    resource: &DownloadResourceRecord,
) -> JniResult<JObject<'local>> {
    let class = env.find_class(jni_name(format!("{PKG}/NativeDownloadResourceRecord")))?;
    let resource_id = JObject::from(env.new_string(resource.resource_id.as_str())?);
    let uri = JObject::from(env.new_string(resource.uri.as_str())?);
    let relative_path = optional_java_string(
        env,
        resource
            .relative_path
            .as_ref()
            .and_then(|path| path.to_str()),
    )?;
    let etag = optional_java_string(env, resource.etag.as_deref())?;
    let checksum = optional_java_string(env, resource.checksum.as_deref())?;
    let byte_range = optional_download_byte_range_object(env, resource.byte_range)?;
    let generated_text = JObject::null();
    env.new_object(
        class,
        method_sig(
            &format!(
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;L{PKG}/NativeDownloadByteRange;Ljava/lang/String;ZJLjava/lang/String;Ljava/lang/String;)V"
            ),
        )
        .method_signature(),
        &[
            JValue::Object(&resource_id),
            JValue::Object(&uri),
            JValue::Object(&relative_path),
            JValue::Object(&byte_range),
            JValue::Object(&generated_text),
            JValue::Bool(resource.size_bytes.is_some()),
            JValue::Long(u64_to_jlong_saturating(
                resource.size_bytes.unwrap_or_default(),
            )),
            JValue::Object(&etag),
            JValue::Object(&checksum),
        ],
    )
}

fn download_segment_record_object<'local>(
    env: &mut Env<'local>,
    segment: &DownloadSegmentRecord,
) -> JniResult<JObject<'local>> {
    let class = env.find_class(jni_name(format!("{PKG}/NativeDownloadSegmentRecord")))?;
    let segment_id = JObject::from(env.new_string(segment.segment_id.as_str())?);
    let uri = JObject::from(env.new_string(segment.uri.as_str())?);
    let relative_path = optional_java_string(
        env,
        segment
            .relative_path
            .as_ref()
            .and_then(|path| path.to_str()),
    )?;
    let checksum = optional_java_string(env, segment.checksum.as_deref())?;
    let byte_range = optional_download_byte_range_object(env, segment.byte_range)?;
    env.new_object(
        class,
        method_sig(
            &format!(
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;ZJL{PKG}/NativeDownloadByteRange;ZJLjava/lang/String;)V"
            ),
        )
        .method_signature(),
        &[
            JValue::Object(&segment_id),
            JValue::Object(&uri),
            JValue::Object(&relative_path),
            JValue::Bool(segment.sequence.is_some()),
            JValue::Long(u64_to_jlong_saturating(
                segment.sequence.unwrap_or_default(),
            )),
            JValue::Object(&byte_range),
            JValue::Bool(segment.size_bytes.is_some()),
            JValue::Long(u64_to_jlong_saturating(
                segment.size_bytes.unwrap_or_default(),
            )),
            JValue::Object(&checksum),
        ],
    )
}

fn optional_download_byte_range_object<'local>(
    env: &mut Env<'local>,
    byte_range: Option<DownloadByteRange>,
) -> JniResult<JObject<'local>> {
    let Some(byte_range) = byte_range else {
        return Ok(JObject::null());
    };
    let class = env.find_class(jni_name(format!("{PKG}/NativeDownloadByteRange")))?;
    env.new_object(
        class,
        method_sig("(JJ)V").method_signature(),
        &[
            JValue::Long(u64_to_jlong_saturating(byte_range.offset)),
            JValue::Long(u64_to_jlong_saturating(byte_range.length)),
        ],
    )
}

fn download_stream_kind_ordinal(kind: DownloadStreamKind) -> jint {
    match kind {
        DownloadStreamKind::Combined => 0,
        DownloadStreamKind::Video => 1,
        DownloadStreamKind::Audio => 2,
        DownloadStreamKind::SecondaryAudio => 3,
        DownloadStreamKind::Subtitle => 4,
        DownloadStreamKind::Auxiliary => 5,
    }
}

fn download_asset_stream_object<'local>(
    env: &mut Env<'local>,
    stream: &DownloadAssetStream,
) -> JniResult<JObject<'local>> {
    let class = env.find_class(jni_name(format!("{PKG}/NativeDownloadAssetStream")))?;
    let stream_id = optional_java_string(env, Some(stream.stream_id.as_str()))?;
    let language = optional_java_string(env, stream.language.as_deref())?;
    let codec = optional_java_string(env, stream.codec.as_deref())?;
    let label = optional_java_string(env, stream.label.as_deref())?;
    let resource_ids = java_string_array_object(env, &stream.resource_ids)?;
    let segment_ids = java_string_array_object(env, &stream.segment_ids)?;
    let (metadata_keys, metadata_values): (Vec<_>, Vec<_>) = stream
        .metadata
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .unzip();
    let metadata_keys = java_string_array_object(env, &metadata_keys)?;
    let metadata_values = java_string_array_object(env, &metadata_values)?;
    env.new_object(
        class,
        method_sig(
            "(Ljava/lang/String;ILjava/lang/String;Ljava/lang/String;Ljava/lang/String;ZI[Ljava/lang/String;[Ljava/lang/String;[Ljava/lang/String;[Ljava/lang/String;)V",
        )
        .method_signature(),
        &[
            JValue::Object(&stream_id),
            JValue::Int(download_stream_kind_ordinal(stream.kind)),
            JValue::Object(&language),
            JValue::Object(&codec),
            JValue::Object(&label),
            JValue::Bool(stream.quality_rank.is_some()),
            JValue::Int(stream.quality_rank.unwrap_or_default() as jint),
            JValue::Object(&resource_ids),
            JValue::Object(&segment_ids),
            JValue::Object(&metadata_keys),
            JValue::Object(&metadata_values),
        ],
    )
}

fn download_asset_index_object<'local>(
    env: &mut Env<'local>,
    asset_index: &DownloadAssetIndex,
) -> JniResult<JObject<'local>> {
    let resource_class = env.find_class(jni_name(format!("{PKG}/NativeDownloadResourceRecord")))?;
    let resources_array: JObjectArray<'_> = env.new_object_array(
        asset_index.resources.len() as i32,
        resource_class,
        JObject::null(),
    )?;
    for (index, resource) in asset_index.resources.iter().enumerate() {
        let object = download_resource_record_object(env, resource)?;
        resources_array.set_element(env, index, object)?;
    }

    let segment_class = env.find_class(jni_name(format!("{PKG}/NativeDownloadSegmentRecord")))?;
    let segments_array: JObjectArray<'_> = env.new_object_array(
        asset_index.segments.len() as i32,
        segment_class,
        JObject::null(),
    )?;
    for (index, segment) in asset_index.segments.iter().enumerate() {
        let object = download_segment_record_object(env, segment)?;
        segments_array.set_element(env, index, object)?;
    }

    let stream_class = env.find_class(jni_name(format!("{PKG}/NativeDownloadAssetStream")))?;
    let streams_array: JObjectArray<'_> = env.new_object_array(
        asset_index.streams.len() as i32,
        stream_class,
        JObject::null(),
    )?;
    for (index, stream) in asset_index.streams.iter().enumerate() {
        let object = download_asset_stream_object(env, stream)?;
        streams_array.set_element(env, index, object)?;
    }

    let class = env.find_class(jni_name(format!("{PKG}/NativeDownloadAssetIndex")))?;
    let version = optional_java_string(env, asset_index.version.as_deref())?;
    let etag = optional_java_string(env, asset_index.etag.as_deref())?;
    let checksum = optional_java_string(env, asset_index.checksum.as_deref())?;
    let completed_path = optional_java_string(
        env,
        asset_index
            .completed_path
            .as_ref()
            .and_then(|path| path.to_str()),
    )?;
    env.new_object(
        class,
        method_sig(&format!(
            "(ILjava/lang/String;Ljava/lang/String;Ljava/lang/String;ZJ[L{PKG}/NativeDownloadResourceRecord;[L{PKG}/NativeDownloadSegmentRecord;[L{PKG}/NativeDownloadAssetStream;Ljava/lang/String;)V"
        ))
        .method_signature(),
        &[
            JValue::Int(match asset_index.content_format {
                DownloadContentFormat::HlsSegments => 0,
                DownloadContentFormat::DashSegments => 1,
                DownloadContentFormat::FlvSegments => 2,
                DownloadContentFormat::SingleFile => 3,
                DownloadContentFormat::Unknown => 4,
            }),
            JValue::Object(&version),
            JValue::Object(&etag),
            JValue::Object(&checksum),
            JValue::Bool(asset_index.total_size_bytes.is_some()),
            JValue::Long(u64_to_jlong_saturating(
                asset_index.total_size_bytes.unwrap_or_default(),
            )),
            JValue::Object(&resources_array),
            JValue::Object(&segments_array),
            JValue::Object(&streams_array),
            JValue::Object(&completed_path),
        ],
    )
}

fn download_progress_object<'local>(
    env: &mut Env<'local>,
    progress: &DownloadProgressSnapshot,
) -> JniResult<JObject<'local>> {
    let class = env.find_class(jni_name(format!("{PKG}/NativeDownloadProgress")))?;
    env.new_object(
        class,
        method_sig("(JZJIZI)V").method_signature(),
        &[
            JValue::Long(u64_to_jlong_saturating(progress.received_bytes)),
            JValue::Bool(progress.total_bytes.is_some()),
            JValue::Long(u64_to_jlong_saturating(
                progress.total_bytes.unwrap_or_default(),
            )),
            JValue::Int(progress.received_segments.min(i32::MAX as u32) as jint),
            JValue::Bool(progress.total_segments.is_some()),
            JValue::Int(
                progress
                    .total_segments
                    .unwrap_or_default()
                    .min(i32::MAX as u32) as jint,
            ),
        ],
    )
}

fn download_status_ordinal(status: DownloadTaskStatus) -> jint {
    match status {
        DownloadTaskStatus::Queued => 0,
        DownloadTaskStatus::Preparing => 1,
        DownloadTaskStatus::Downloading => 2,
        DownloadTaskStatus::Paused => 3,
        DownloadTaskStatus::Completed => 4,
        DownloadTaskStatus::Failed => 5,
        DownloadTaskStatus::Removed => 6,
    }
}

fn download_task_object<'local>(
    env: &mut Env<'local>,
    task: &DownloadTaskSnapshot,
) -> JniResult<JObject<'local>> {
    let class = env.find_class(jni_name(format!("{PKG}/NativeDownloadTask")))?;
    let asset_id = JObject::from(env.new_string(task.asset_id.as_str())?);
    let source = download_source_object(env, &task.source)?;
    let profile = download_profile_object(env, &task.profile)?;
    let progress = download_progress_object(env, &task.progress)?;
    let asset_index = download_asset_index_object(env, &task.asset_index)?;
    let error_message = optional_java_string(
        env,
        task.error_summary
            .as_ref()
            .map(|summary| summary.message.as_str()),
    )?;
    env.new_object(
        class,
        method_sig(&format!(
            "(JLjava/lang/String;L{PKG}/NativeDownloadSource;L{PKG}/NativeDownloadProfile;IL{PKG}/NativeDownloadProgress;L{PKG}/NativeDownloadAssetIndex;ZIIZLjava/lang/String;)V"
        ))
        .method_signature(),
        &[
            JValue::Long(u64_to_jlong_saturating(task.task_id.get())),
            JValue::Object(&asset_id),
            JValue::Object(&source),
            JValue::Object(&profile),
            JValue::Int(download_status_ordinal(task.status)),
            JValue::Object(&progress),
            JValue::Object(&asset_index),
            JValue::Bool(task.error_summary.is_some()),
            JValue::Int(
                task.error_summary
                    .as_ref()
                    .map(|summary| summary.code as jint)
                    .unwrap_or_default(),
            ),
            JValue::Int(
                task.error_summary
                    .as_ref()
                    .map(|summary| summary.category as jint)
                    .unwrap_or_default(),
            ),
            JValue::Bool(
                task.error_summary
                    .as_ref()
                    .map(|summary| summary.retriable)
                    .unwrap_or(false),
            ),
            JValue::Object(&error_message),
        ],
    )
}

fn download_command_object<'local>(
    env: &mut Env<'local>,
    command: &AndroidDownloadCommand,
) -> JniResult<JObject<'local>> {
    match command {
        AndroidDownloadCommand::Prepare { task } => {
            let class = env.find_class(jni_name(format!("{PKG}/NativeDownloadCommand$Prepare")))?;
            let task = download_task_object(env, task)?;
            env.new_object(
                class,
                method_sig(&format!("(L{PKG}/NativeDownloadTask;)V")).method_signature(),
                &[JValue::Object(&task)],
            )
        }
        AndroidDownloadCommand::Start { task } => {
            let class = env.find_class(jni_name(format!("{PKG}/NativeDownloadCommand$Start")))?;
            let task = download_task_object(env, task)?;
            env.new_object(
                class,
                method_sig(&format!("(L{PKG}/NativeDownloadTask;)V")).method_signature(),
                &[JValue::Object(&task)],
            )
        }
        AndroidDownloadCommand::Pause { task_id } => {
            let class = env.find_class(jni_name(format!("{PKG}/NativeDownloadCommand$Pause")))?;
            env.new_object(
                class,
                method_sig("(J)V").method_signature(),
                &[JValue::Long(u64_to_jlong_saturating(task_id.get()))],
            )
        }
        AndroidDownloadCommand::Resume { task } => {
            let class = env.find_class(jni_name(format!("{PKG}/NativeDownloadCommand$Resume")))?;
            let task = download_task_object(env, task)?;
            env.new_object(
                class,
                method_sig(&format!("(L{PKG}/NativeDownloadTask;)V")).method_signature(),
                &[JValue::Object(&task)],
            )
        }
        AndroidDownloadCommand::Remove { task } => {
            let class = env.find_class(jni_name(format!("{PKG}/NativeDownloadCommand$Remove")))?;
            let task = download_task_object(env, task)?;
            env.new_object(
                class,
                method_sig(&format!("(L{PKG}/NativeDownloadTask;)V")).method_signature(),
                &[JValue::Object(&task)],
            )
        }
    }
}

fn download_event_object<'local>(
    env: &mut Env<'local>,
    event: &DownloadEvent,
) -> JniResult<JObject<'local>> {
    match event {
        DownloadEvent::Created(task) => {
            let class = env.find_class(jni_name(format!("{PKG}/NativeDownloadEvent$Created")))?;
            let task = download_task_object(env, task)?;
            env.new_object(
                class,
                method_sig(&format!("(L{PKG}/NativeDownloadTask;)V")).method_signature(),
                &[JValue::Object(&task)],
            )
        }
        DownloadEvent::StateChanged(patch) => {
            let class =
                env.find_class(jni_name(format!("{PKG}/NativeDownloadEvent$StateChanged")))?;
            let progress = download_progress_object(env, &patch.progress)?;
            let error_message = optional_java_string(
                env,
                patch
                    .error_summary
                    .as_ref()
                    .map(|summary| summary.message.as_str()),
            )?;
            let completed_path = optional_java_string(
                env,
                patch.completed_path.as_ref().and_then(|path| path.to_str()),
            )?;
            env.new_object(
                class,
                method_sig(&format!(
                    "(JIL{PKG}/NativeDownloadProgress;ZIIZLjava/lang/String;Ljava/lang/String;)V"
                ))
                .method_signature(),
                &[
                    JValue::Long(u64_to_jlong_saturating(patch.task_id.get())),
                    JValue::Int(download_status_ordinal(patch.status)),
                    JValue::Object(&progress),
                    JValue::Bool(patch.error_summary.is_some()),
                    JValue::Int(
                        patch
                            .error_summary
                            .as_ref()
                            .map(|summary| summary.code as jint)
                            .unwrap_or_default(),
                    ),
                    JValue::Int(
                        patch
                            .error_summary
                            .as_ref()
                            .map(|summary| summary.category as jint)
                            .unwrap_or_default(),
                    ),
                    JValue::Bool(
                        patch
                            .error_summary
                            .as_ref()
                            .map(|summary| summary.retriable)
                            .unwrap_or(false),
                    ),
                    JValue::Object(&error_message),
                    JValue::Object(&completed_path),
                ],
            )
        }
        DownloadEvent::AssetIndexUpdated(task) => {
            let class = env.find_class(jni_name(format!(
                "{PKG}/NativeDownloadEvent$AssetIndexUpdated"
            )))?;
            let task = download_task_object(env, task)?;
            env.new_object(
                class,
                method_sig(&format!("(L{PKG}/NativeDownloadTask;)V")).method_signature(),
                &[JValue::Object(&task)],
            )
        }
        DownloadEvent::ProgressUpdated(patch) => {
            let class = env.find_class(jni_name(format!(
                "{PKG}/NativeDownloadEvent$ProgressUpdated"
            )))?;
            let progress = download_progress_object(env, &patch.progress)?;
            env.new_object(
                class,
                method_sig(&format!("(JL{PKG}/NativeDownloadProgress;)V")).method_signature(),
                &[
                    JValue::Long(u64_to_jlong_saturating(patch.task_id.get())),
                    JValue::Object(&progress),
                ],
            )
        }
    }
}

fn download_event_batch_object<'local>(
    env: &mut Env<'local>,
    batch: &DownloadEventBatch,
) -> JniResult<JObject<'local>> {
    let event_class = env.find_class(jni_name(format!("{PKG}/NativeDownloadEvent")))?;
    let events: JObjectArray<'_, JObject<'_>> =
        env.new_object_array(batch.events.len() as i32, event_class, JObject::null())?;
    for (index, event) in batch.events.iter().enumerate() {
        let object = download_event_object(env, event)?;
        events.set_element(env, index, object)?;
    }
    let batch_class = env.find_class(jni_name(format!("{PKG}/NativeDownloadEventBatch")))?;
    env.new_object(
        batch_class,
        method_sig(&format!("([L{PKG}/NativeDownloadEvent;J)V")).method_signature(),
        &[
            JValue::Object(&events),
            JValue::Long(u64_to_jlong_saturating(batch.dropped_events)),
        ],
    )
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_createDownloadSession(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    config: JObject<'_>,
) -> jlong {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jlong> {
                let config = download_config_from_java(env, config)?;
                let java_vm = env.get_java_vm()?;
                match new_download_session(config, java_vm) {
                    Ok(handle) => Ok(handle),
                    Err(message) => {
                        env.throw_new(
                            jni_name("java/lang/IllegalStateException"),
                            jni_name(message),
                        )?;
                        Ok(0)
                    }
                }
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_disposeDownloadSession(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|_env| -> JniResult<()> {
                // Remove the entry under the registry lock, but drop the
                // returned `Arc<Mutex<AndroidDownloadBridgeSession>>` *after*
                // releasing the registry lock. The session wraps a
                // `DownloadManager` whose post-processor chain may invoke plugin
                // `destroy` FFI on Drop; dropping it under the lock would
                // serialize every download session process-wide.
                let session = {
                    let mut guard = lock_or_recover(download_sessions());
                    guard.remove(session_handle)
                };
                drop(session);
                Ok(())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_createDownloadTask(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    asset_id: JString<'_>,
    source: JObject<'_>,
    profile: JObject<'_>,
    asset_index: JObject<'_>,
    _now_epoch_ms: jlong,
) -> jlong {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jlong> {
                let asset_id = asset_id.try_to_string(env)?;
                let source = download_source_from_java(env, source)?;
                let profile = download_profile_from_java(env, profile)?;
                let asset_index = download_asset_index_from_java(env, asset_index)?;
                let Some(result) = with_download_session_mut(env, session_handle, |session| {
                    session.create_task(asset_id, source, profile, asset_index, Instant::now())
                }) else {
                    return Ok(0);
                };
                Ok(result
                    .map(|task_id| u64_to_jlong_saturating(task_id.get()))
                    .unwrap_or_default())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_restoreDownloadTasks(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    tasks: JObjectArray<'_, JObject<'_>>,
    _now_epoch_ms: jlong,
) -> jboolean {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jboolean> {
                let len = tasks.len(env)?;
                let now = Instant::now();
                let mut restored_tasks = Vec::with_capacity(len);
                for index in 0..len {
                    let task = tasks.get_element(env, index)?;
                    if !task.is_null() {
                        restored_tasks.push(download_task_from_java(env, task, now)?);
                    }
                }
                let Some(result) = with_download_session_mut(env, session_handle, |session| {
                    session.restore_tasks(restored_tasks, now)
                }) else {
                    return Ok(false as jboolean);
                };
                Ok(result.is_ok() as jboolean)
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

fn mutate_download_task(
    mut unowned_env: EnvUnowned<'_>,
    session_handle: jlong,
    task_id: jlong,
    mutate: impl FnOnce(
        &mut AndroidDownloadBridgeSession,
        player_runtime::DownloadTaskId,
        Instant,
    ) -> player_runtime::PlayerResult<Option<DownloadTaskSnapshot>>,
) -> jboolean {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jboolean> {
                let Some(result) = with_download_session_mut(env, session_handle, |session| {
                    mutate(
                        session,
                        player_runtime::DownloadTaskId::from_raw(task_id.max(0) as u64),
                        Instant::now(),
                    )
                }) else {
                    return Ok(false as jboolean);
                };
                Ok(result.is_ok() as jboolean)
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_startDownloadTask(
    unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    task_id: jlong,
    _now_epoch_ms: jlong,
) -> jboolean {
    mutate_download_task(
        unowned_env,
        session_handle,
        task_id,
        |session, task_id, now| session.start_task(task_id, now),
    )
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_pauseDownloadTask(
    unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    task_id: jlong,
    _now_epoch_ms: jlong,
) -> jboolean {
    mutate_download_task(
        unowned_env,
        session_handle,
        task_id,
        |session, task_id, now| session.pause_task(task_id, now),
    )
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_resumeDownloadTask(
    unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    task_id: jlong,
    _now_epoch_ms: jlong,
) -> jboolean {
    mutate_download_task(
        unowned_env,
        session_handle,
        task_id,
        |session, task_id, now| session.resume_task(task_id, now),
    )
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_updateDownloadTaskProgress(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    task_id: jlong,
    received_bytes: jlong,
    received_segments: jint,
    _now_epoch_ms: jlong,
) -> jboolean {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jboolean> {
                let Some(result) = with_download_session_mut(env, session_handle, |session| {
                    session.update_progress(
                        player_runtime::DownloadTaskId::from_raw(task_id.max(0) as u64),
                        received_bytes.max(0) as u64,
                        received_segments.max(0) as u32,
                        Instant::now(),
                    )
                }) else {
                    return Ok(false as jboolean);
                };
                Ok(result.is_ok() as jboolean)
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_completeDownloadPreparation(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    task_id: jlong,
    asset_index: JObject<'_>,
    _now_epoch_ms: jlong,
) -> jboolean {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jboolean> {
                let asset_index = download_asset_index_from_java(env, asset_index)?;
                let Some(result) = with_download_session_mut(env, session_handle, |session| {
                    session.complete_preparation(
                        player_runtime::DownloadTaskId::from_raw(task_id.max(0) as u64),
                        asset_index,
                        Instant::now(),
                    )
                }) else {
                    return Ok(false as jboolean);
                };
                Ok(result.is_ok() as jboolean)
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_replaceDownloadTaskPlan(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    task_id: jlong,
    source: JObject<'_>,
    profile: JObject<'_>,
    asset_index: JObject<'_>,
    _now_epoch_ms: jlong,
) -> jboolean {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jboolean> {
                let source = download_source_from_java(env, source)?;
                let profile = download_profile_from_java(env, profile)?;
                let asset_index = download_asset_index_from_java(env, asset_index)?;
                let Some(result) = with_download_session_mut(env, session_handle, |session| {
                    session.replace_task_plan(
                        player_runtime::DownloadTaskId::from_raw(task_id.max(0) as u64),
                        source,
                        profile,
                        asset_index,
                        Instant::now(),
                    )
                }) else {
                    return Ok(false as jboolean);
                };
                Ok(result.is_ok() as jboolean)
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_completeDownloadTask(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    task_id: jlong,
    completed_path: JString<'_>,
    _now_epoch_ms: jlong,
) -> jboolean {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jboolean> {
                let completed_path = completed_path.try_to_string(env)?;
                let completed_path =
                    (!completed_path.trim().is_empty()).then_some(PathBuf::from(completed_path));
                let Some(result) = with_download_session_mut(env, session_handle, |session| {
                    session.complete_task(
                        player_runtime::DownloadTaskId::from_raw(task_id.max(0) as u64),
                        completed_path,
                        Instant::now(),
                    )
                }) else {
                    return Ok(false as jboolean);
                };
                Ok(result.is_ok() as jboolean)
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_exportDownloadTask(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    task_id: jlong,
    output_path: JString<'_>,
    progress_callback: JObject<'_>,
) -> jboolean {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jboolean> {
                let output_path = output_path.try_to_string(env)?;
                let callback = if progress_callback.is_null() {
                    None
                } else {
                    Some(Arc::new(env.new_global_ref(progress_callback)?))
                };
                let Some(session) = download_session(env, session_handle) else {
                    return Ok(false as jboolean);
                };
                let progress = JniDownloadExportProgress {
                    worker: session.export_worker.clone(),
                    callback,
                };
                let export_plan = {
                    let session = lock_or_recover(&session.bridge);
                    session.prepare_export_task_output(
                        player_runtime::DownloadTaskId::from_raw(task_id.max(0) as u64),
                        Some(PathBuf::from(output_path)),
                    )
                };
                let result = export_plan.and_then(|plan| plan.execute(&progress));
                progress.worker.flush();
                match result {
                    Ok(_) => Ok(true as jboolean),
                    Err(error) => {
                        env.throw_new(
                            jni_name("java/lang/IllegalStateException"),
                            jni_name(error.to_string()),
                        )?;
                        Ok(false as jboolean)
                    }
                }
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_failDownloadTask(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    task_id: jlong,
    code_jni_ordinal: jint,
    category_jni_ordinal: jint,
    retriable: jboolean,
    message: JString<'_>,
    _now_epoch_ms: jlong,
) -> jboolean {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jboolean> {
                let message = message.try_to_string(env)?;
                let error = PlayerError::with_taxonomy(
                    error_code_from_jni_ordinal(code_jni_ordinal),
                    error_category_from_jni_ordinal(category_jni_ordinal),
                    (retriable as u8) != 0,
                    message,
                );
                let Some(result) = with_download_session_mut(env, session_handle, |session| {
                    session.fail_task(
                        player_runtime::DownloadTaskId::from_raw(task_id.max(0) as u64),
                        error,
                        Instant::now(),
                    )
                }) else {
                    return Ok(false as jboolean);
                };
                Ok(result.is_ok() as jboolean)
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_removeDownloadTask(
    unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    task_id: jlong,
    _now_epoch_ms: jlong,
) -> jboolean {
    mutate_download_task(
        unowned_env,
        session_handle,
        task_id,
        |session, task_id, now| session.remove_task(task_id, now),
    )
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_pollDownloadSnapshot(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) -> jobject {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jobject> {
                let snapshot =
                    with_download_session(env, session_handle, |session| session.snapshot());
                let Some(snapshot) = snapshot else {
                    return Ok(JObject::null().into_raw());
                };

                let task_class = env.find_class(jni_name(format!("{PKG}/NativeDownloadTask")))?;
                let tasks_array: JObjectArray<'_> =
                    env.new_object_array(snapshot.tasks.len() as i32, task_class, JObject::null())?;
                for (index, task) in snapshot.tasks.iter().enumerate() {
                    let object = download_task_object(env, task)?;
                    tasks_array.set_element(env, index, object)?;
                }

                let class = env.find_class(jni_name(format!("{PKG}/NativeDownloadSnapshot")))?;
                let snapshot = env.new_object(
                    class,
                    method_sig(&format!("([L{PKG}/NativeDownloadTask;)V")).method_signature(),
                    &[JValue::Object(&tasks_array)],
                )?;
                Ok(snapshot.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_drainDownloadCommands(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) -> jobjectArray {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jobjectArray> {
                let Some(commands) = with_download_session_mut(env, session_handle, |session| {
                    session.drain_commands()
                }) else {
                    let command_class =
                        env.find_class(jni_name(format!("{PKG}/NativeDownloadCommand")))?;
                    let array: JObjectArray<'_> =
                        env.new_object_array(0, command_class, JObject::null())?;
                    return Ok(array.into_raw());
                };

                let command_class =
                    env.find_class(jni_name(format!("{PKG}/NativeDownloadCommand")))?;
                let array: JObjectArray<'_> =
                    env.new_object_array(commands.len() as i32, command_class, JObject::null())?;
                for (index, command) in commands.iter().enumerate() {
                    let object = download_command_object(env, command)?;
                    array.set_element(env, index, object)?;
                }
                Ok(array.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_drainDownloadEvents(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) -> jobject {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jobject> {
                let Some(batch) = with_download_session_mut(env, session_handle, |session| {
                    session.drain_event_batch()
                }) else {
                    return Ok(JObject::null().into_raw());
                };
                Ok(download_event_batch_object(env, &batch)?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::PersistentBoundedWorker;

    #[test]
    fn persistent_worker_reuses_one_thread_and_bounds_queued_work() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (processed_tx, processed_rx) = mpsc::channel();
        let worker = PersistentBoundedWorker::spawn("vesper-jni-worker-test", 1, move |receiver| {
            while let Ok(value) = receiver.recv() {
                let thread_id = std::thread::current().id();
                if value == 1 {
                    entered_tx.send(()).expect("entered");
                    release_rx.recv().expect("release");
                }
                processed_tx.send((value, thread_id)).expect("processed");
            }
        })
        .expect("worker");

        worker.try_send(1).expect("running task");
        entered_rx.recv().expect("worker entered first task");
        worker.try_send(2).expect("queued task");
        assert!(matches!(
            worker.try_send(3),
            Err(mpsc::TrySendError::Full(3))
        ));
        release_tx.send(()).expect("release worker");

        let first = processed_rx.recv().expect("first result");
        let second = processed_rx.recv().expect("second result");
        assert_eq!((first.0, second.0), (1, 2));
        assert_eq!(first.1, second.1);
        drop(worker);
    }
}
