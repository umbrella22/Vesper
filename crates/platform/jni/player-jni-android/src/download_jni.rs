use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use jni::errors::{Result as JniResult, ThrowRuntimeExAndDefault};
use jni::objects::{Global, JClass, JObject, JObjectArray, JString, JValue};
use jni::sys::{jboolean, jint, jlong, jobject, jobjectArray};
use jni::{Env, EnvUnowned, JavaVM};
use player_model::MediaSource;
use player_platform_android::{AndroidDownloadBridgeSession, AndroidDownloadCommand};
use player_plugin::ProcessorProgress;
use player_runtime::{
    DownloadAssetIndex, DownloadContentFormat, DownloadEvent, DownloadProfile,
    DownloadProgressSnapshot, DownloadResourceRecord, DownloadSegmentRecord, DownloadSource,
    DownloadTaskSnapshot, PlayerRuntimeError,
};

use crate::{
    HandleRegistry, PKG, error_category_from_ordinal, error_code_from_ordinal, field_sig, jni_name,
    lock_or_recover, method_sig, run_jni_entry, u64_to_jlong_saturating,
};

type AndroidJniDownloadSession = Arc<Mutex<AndroidDownloadBridgeSession>>;

#[derive(Debug)]
struct AndroidDownloadSessionConfig {
    auto_start: bool,
    run_post_processors_on_completion: bool,
    plugin_library_paths: Vec<PathBuf>,
}

struct JniDownloadExportProgress {
    java_vm: JavaVM,
    callback: Option<Global<JObject<'static>>>,
}

impl ProcessorProgress for JniDownloadExportProgress {
    fn on_progress(&self, ratio: f32) {
        let Some(callback) = self.callback.as_ref() else {
            return;
        };
        let _: JniResult<()> = self.java_vm.attach_current_thread_for_scope(|env| {
            env.call_method(
                callback.as_obj(),
                jni_name("onProgress"),
                method_sig("(F)V").method_signature(),
                &[JValue::Float(ratio)],
            )?;
            Ok(())
        });
    }

    fn is_cancelled(&self) -> bool {
        let Some(callback) = self.callback.as_ref() else {
            return false;
        };
        self.java_vm
            .attach_current_thread_for_scope(|env| {
                let value = env.call_method(
                    callback.as_obj(),
                    jni_name("isCancelled"),
                    method_sig("()Z").method_signature(),
                    &[],
                )?;
                value.z()
            })
            .unwrap_or(false)
    }
}

static DOWNLOAD_SESSIONS: OnceLock<Mutex<HandleRegistry<AndroidJniDownloadSession>>> =
    OnceLock::new();

fn download_sessions() -> &'static Mutex<HandleRegistry<AndroidJniDownloadSession>> {
    DOWNLOAD_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

fn invalid_download_handle_error() -> &'static str {
    "invalid android JNI download session handle"
}

fn with_download_session_mut<R>(
    env: &mut Env<'_>,
    handle: jlong,
    f: impl FnOnce(&mut AndroidDownloadBridgeSession) -> R,
) -> Option<R> {
    let session = {
        let guard = lock_or_recover(download_sessions());
        let Some(session) = guard.get(handle).cloned() else {
            let _ = env.throw_new(
                jni_name("java/lang/IllegalArgumentException"),
                jni_name(invalid_download_handle_error()),
            );
            return None;
        };
        session
    };

    // 持有 session 锁期间禁止回调 Java，避免同一 handle 发生重入阻塞。
    let mut session = lock_or_recover(session.as_ref());
    Some(f(&mut session))
}

fn with_download_session<R>(
    env: &mut Env<'_>,
    handle: jlong,
    f: impl FnOnce(&AndroidDownloadBridgeSession) -> R,
) -> Option<R> {
    let session = {
        let guard = lock_or_recover(download_sessions());
        let Some(session) = guard.get(handle).cloned() else {
            let _ = env.throw_new(
                jni_name("java/lang/IllegalArgumentException"),
                jni_name(invalid_download_handle_error()),
            );
            return None;
        };
        session
    };

    // 只读路径同样会持有 session 锁，闭包内不要触发会重入 JNI 的 Java 回调。
    let session = lock_or_recover(session.as_ref());
    Some(f(&session))
}

fn new_download_session(config: AndroidDownloadSessionConfig) -> Result<jlong, String> {
    let session = Arc::new(Mutex::new(
        AndroidDownloadBridgeSession::new_with_plugin_library_paths(
            config.auto_start,
            config.run_post_processors_on_completion,
            config.plugin_library_paths,
        )
        .map_err(|error| error.to_string())?,
    ));
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

fn bool_field(env: &mut Env<'_>, object: &JObject<'_>, field_name: &str) -> JniResult<bool> {
    env.get_field(
        object,
        jni_name(field_name),
        field_sig("Z").field_signature(),
    )?
    .z()
}

fn int_field(env: &mut Env<'_>, object: &JObject<'_>, field_name: &str) -> JniResult<jint> {
    env.get_field(
        object,
        jni_name(field_name),
        field_sig("I").field_signature(),
    )?
    .i()
}

fn long_field(env: &mut Env<'_>, object: &JObject<'_>, field_name: &str) -> JniResult<jlong> {
    env.get_field(
        object,
        jni_name(field_name),
        field_sig("J").field_signature(),
    )?
    .j()
}

fn string_field(
    env: &mut Env<'_>,
    object: &JObject<'_>,
    field_name: &str,
) -> JniResult<Option<String>> {
    let value = env
        .get_field(
            object,
            jni_name(field_name),
            field_sig("Ljava/lang/String;").field_signature(),
        )?
        .l()?;
    if value.is_null() {
        return Ok(None);
    }
    let value = unsafe { JString::from_raw(env, value.into_raw() as jni::sys::jstring) };
    Ok(Some(value.try_to_string(env)?))
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
        plugin_library_paths: string_array_field(env, &config, "pluginLibraryPaths")?
            .into_iter()
            .map(PathBuf::from)
            .collect(),
    })
}

fn download_source_from_java(env: &mut Env<'_>, source: JObject<'_>) -> JniResult<DownloadSource> {
    let source_uri = string_field(env, &source, "sourceUri")?.unwrap_or_default();
    let content_format = match int_field(env, &source, "contentFormatOrdinal")? {
        0 => DownloadContentFormat::HlsSegments,
        1 => DownloadContentFormat::DashSegments,
        2 => DownloadContentFormat::SingleFile,
        _ => DownloadContentFormat::Unknown,
    };
    let mut download_source = DownloadSource::new(MediaSource::new(source_uri), content_format);
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
        target_directory: string_field(env, &profile, "targetDirectory")?.map(PathBuf::from),
        allow_metered_network: bool_field(env, &profile, "allowMeteredNetwork")?,
    })
}

fn download_resource_record_from_java(
    env: &mut Env<'_>,
    resource: JObject<'_>,
) -> JniResult<DownloadResourceRecord> {
    Ok(DownloadResourceRecord {
        resource_id: string_field(env, &resource, "resourceId")?.unwrap_or_default(),
        uri: string_field(env, &resource, "uri")?.unwrap_or_default(),
        relative_path: string_field(env, &resource, "relativePath")?.map(PathBuf::from),
        size_bytes: bool_field(env, &resource, "hasSizeBytes")?
            .then_some(long_field(env, &resource, "sizeBytes")?.max(0) as u64),
        etag: string_field(env, &resource, "etag")?,
        checksum: string_field(env, &resource, "checksum")?,
    })
}

fn download_segment_record_from_java(
    env: &mut Env<'_>,
    segment: JObject<'_>,
) -> JniResult<DownloadSegmentRecord> {
    Ok(DownloadSegmentRecord {
        segment_id: string_field(env, &segment, "segmentId")?.unwrap_or_default(),
        uri: string_field(env, &segment, "uri")?.unwrap_or_default(),
        relative_path: string_field(env, &segment, "relativePath")?.map(PathBuf::from),
        sequence: bool_field(env, &segment, "hasSequence")?
            .then_some(long_field(env, &segment, "sequence")?.max(0) as u64),
        size_bytes: bool_field(env, &segment, "hasSizeBytes")?
            .then_some(long_field(env, &segment, "sizeBytes")?.max(0) as u64),
        checksum: string_field(env, &segment, "checksum")?,
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

fn download_asset_index_from_java(
    env: &mut Env<'_>,
    asset_index: JObject<'_>,
) -> JniResult<DownloadAssetIndex> {
    Ok(DownloadAssetIndex {
        content_format: match int_field(env, &asset_index, "contentFormatOrdinal")? {
            0 => DownloadContentFormat::HlsSegments,
            1 => DownloadContentFormat::DashSegments,
            2 => DownloadContentFormat::SingleFile,
            _ => DownloadContentFormat::Unknown,
        },
        version: string_field(env, &asset_index, "version")?,
        etag: string_field(env, &asset_index, "etag")?,
        checksum: string_field(env, &asset_index, "checksum")?,
        total_size_bytes: bool_field(env, &asset_index, "hasTotalSizeBytes")?
            .then_some(long_field(env, &asset_index, "totalSizeBytes")?.max(0) as u64),
        resources: download_resource_records_from_java(env, &asset_index)?,
        segments: download_segment_records_from_java(env, &asset_index)?,
        completed_path: string_field(env, &asset_index, "completedPath")?.map(PathBuf::from),
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
    env.new_object(
        class,
        method_sig("(Ljava/lang/String;ILjava/lang/String;)V").method_signature(),
        &[
            JValue::Object(&source_uri),
            JValue::Int(match source.content_format {
                DownloadContentFormat::HlsSegments => 0,
                DownloadContentFormat::DashSegments => 1,
                DownloadContentFormat::SingleFile => 2,
                DownloadContentFormat::Unknown => 3,
            }),
            JValue::Object(&manifest_uri),
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
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;Ljava/lang/String;Z)V",
        )
        .method_signature(),
        &[
            JValue::Object(&variant_id),
            JValue::Object(&preferred_audio_language),
            JValue::Object(&preferred_subtitle_language),
            JValue::Object(&selected_track_ids),
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
    env.new_object(
        class,
        method_sig(
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;ZJLjava/lang/String;Ljava/lang/String;)V",
        )
        .method_signature(),
        &[
            JValue::Object(&resource_id),
            JValue::Object(&uri),
            JValue::Object(&relative_path),
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
    env.new_object(
        class,
        method_sig(
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;ZJZJLjava/lang/String;)V",
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
            JValue::Bool(segment.size_bytes.is_some()),
            JValue::Long(u64_to_jlong_saturating(
                segment.size_bytes.unwrap_or_default(),
            )),
            JValue::Object(&checksum),
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
            "(ILjava/lang/String;Ljava/lang/String;Ljava/lang/String;ZJ[L{PKG}/NativeDownloadResourceRecord;[L{PKG}/NativeDownloadSegmentRecord;Ljava/lang/String;)V"
        ))
        .method_signature(),
        &[
            JValue::Int(match asset_index.content_format {
                DownloadContentFormat::HlsSegments => 0,
                DownloadContentFormat::DashSegments => 1,
                DownloadContentFormat::SingleFile => 2,
                DownloadContentFormat::Unknown => 3,
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
            JValue::Int(match task.status {
                player_runtime::DownloadTaskStatus::Queued => 0,
                player_runtime::DownloadTaskStatus::Preparing => 1,
                player_runtime::DownloadTaskStatus::Downloading => 2,
                player_runtime::DownloadTaskStatus::Paused => 3,
                player_runtime::DownloadTaskStatus::Completed => 4,
                player_runtime::DownloadTaskStatus::Failed => 5,
                player_runtime::DownloadTaskStatus::Removed => 6,
            }),
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
        AndroidDownloadCommand::Remove { task_id } => {
            let class = env.find_class(jni_name(format!("{PKG}/NativeDownloadCommand$Remove")))?;
            env.new_object(
                class,
                method_sig("(J)V").method_signature(),
                &[JValue::Long(u64_to_jlong_saturating(task_id.get()))],
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
        DownloadEvent::StateChanged(task) => {
            let class =
                env.find_class(jni_name(format!("{PKG}/NativeDownloadEvent$StateChanged")))?;
            let task = download_task_object(env, task)?;
            env.new_object(
                class,
                method_sig(&format!("(L{PKG}/NativeDownloadTask;)V")).method_signature(),
                &[JValue::Object(&task)],
            )
        }
        DownloadEvent::ProgressUpdated(task) => {
            let class = env.find_class(jni_name(format!(
                "{PKG}/NativeDownloadEvent$ProgressUpdated"
            )))?;
            let task = download_task_object(env, task)?;
            env.new_object(
                class,
                method_sig(&format!("(L{PKG}/NativeDownloadTask;)V")).method_signature(),
                &[JValue::Object(&task)],
            )
        }
    }
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
                match new_download_session(config) {
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
                let mut guard = lock_or_recover(download_sessions());
                guard.remove(session_handle);
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

fn mutate_download_task(
    mut unowned_env: EnvUnowned<'_>,
    session_handle: jlong,
    task_id: jlong,
    mutate: impl FnOnce(
        &mut AndroidDownloadBridgeSession,
        player_runtime::DownloadTaskId,
        Instant,
    ) -> player_runtime::PlayerRuntimeResult<Option<DownloadTaskSnapshot>>,
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
                let java_vm = env.get_java_vm()?;
                let callback = if progress_callback.is_null() {
                    None
                } else {
                    Some(env.new_global_ref(progress_callback)?)
                };
                let progress = JniDownloadExportProgress { java_vm, callback };
                let Some(result) = with_download_session(env, session_handle, |session| {
                    session.export_task_output(
                        player_runtime::DownloadTaskId::from_raw(task_id.max(0) as u64),
                        Some(PathBuf::from(output_path)),
                        &progress,
                    )
                }) else {
                    return Ok(false as jboolean);
                };
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
    code_ordinal: jint,
    category_ordinal: jint,
    retriable: jboolean,
    message: JString<'_>,
    _now_epoch_ms: jlong,
) -> jboolean {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jboolean> {
                let message = message.try_to_string(env)?;
                let error = PlayerRuntimeError::with_taxonomy(
                    error_code_from_ordinal(code_ordinal),
                    error_category_from_ordinal(category_ordinal),
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
) -> jobjectArray {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jobjectArray> {
                let Some(events) = with_download_session_mut(env, session_handle, |session| {
                    session.drain_events()
                }) else {
                    let event_class =
                        env.find_class(jni_name(format!("{PKG}/NativeDownloadEvent")))?;
                    let array: JObjectArray<'_> =
                        env.new_object_array(0, event_class, JObject::null())?;
                    return Ok(array.into_raw());
                };

                let event_class = env.find_class(jni_name(format!("{PKG}/NativeDownloadEvent")))?;
                let array: JObjectArray<'_> =
                    env.new_object_array(events.len() as i32, event_class, JObject::null())?;
                for (index, event) in events.iter().enumerate() {
                    let object = download_event_object(env, event)?;
                    array.set_element(env, index, object)?;
                }
                Ok(array.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}
