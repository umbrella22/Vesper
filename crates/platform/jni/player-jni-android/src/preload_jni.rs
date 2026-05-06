use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use jni::errors::{Result as JniResult, ThrowRuntimeExAndDefault};
use jni::objects::{JClass, JObject, JObjectArray, JString, JValue};
use jni::sys::{jboolean, jint, jlong, jobject, jobjectArray};
use jni::{Env, EnvUnowned};
use player_model::MediaSource;
use player_platform_android::{AndroidPreloadBridgeSession, AndroidPreloadCommand};
use player_runtime::{
    InMemoryPreloadBudgetProvider, PlayerPreloadBudgetPolicy, PlayerRuntimeError, PreloadBudget,
    PreloadBudgetScope, PreloadCandidate, PreloadCandidateKind, PreloadConfig, PreloadPriority,
    PreloadSelectionHint, PreloadTaskId,
};

use crate::{
    HandleRegistry, PKG, error_category_from_ordinal, error_code_from_ordinal, field_sig, jni_name,
    lock_or_recover, method_sig, resolve_preload_budget_with_runtime, run_jni_entry,
    u64_to_jlong_saturating, u128_to_jlong_saturating,
};

type AndroidJniPreloadSession = Arc<Mutex<AndroidPreloadBridgeSession>>;

static PRELOAD_SESSIONS: OnceLock<Mutex<HandleRegistry<AndroidJniPreloadSession>>> =
    OnceLock::new();

fn preload_sessions() -> &'static Mutex<HandleRegistry<AndroidJniPreloadSession>> {
    PRELOAD_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

fn invalid_preload_handle_error() -> &'static str {
    "invalid android JNI preload session handle"
}

fn with_preload_session_mut<R>(
    env: &mut Env<'_>,
    handle: jlong,
    f: impl FnOnce(&mut AndroidPreloadBridgeSession) -> R,
) -> Option<R> {
    let session = {
        let guard = lock_or_recover(preload_sessions());
        let Some(session) = guard.get(handle).cloned() else {
            let _ = env.throw_new(
                jni_name("java/lang/IllegalArgumentException"),
                jni_name(invalid_preload_handle_error()),
            );
            return None;
        };
        session
    };

    // 持有 session 锁期间禁止回调 Java，避免同一预加载 session 发生重入阻塞。
    let mut session = lock_or_recover(session.as_ref());
    Some(f(&mut session))
}

fn new_preload_session(budget: PreloadBudget) -> Result<jlong, &'static str> {
    let session = Arc::new(Mutex::new(AndroidPreloadBridgeSession::new(
        InMemoryPreloadBudgetProvider::new(budget),
    )));
    let mut guard = lock_or_recover(preload_sessions());
    let handle = guard.insert(session);
    if handle == 0 {
        return Err("android JNI preload session registry overflow");
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

fn sparse_preload_budget_from_java(
    env: &mut Env<'_>,
    budget: JObject<'_>,
) -> JniResult<PlayerPreloadBudgetPolicy> {
    Ok(PlayerPreloadBudgetPolicy {
        max_concurrent_tasks: bool_field(env, &budget, "hasMaxConcurrentTasks")?
            .then_some(int_field(env, &budget, "maxConcurrentTasks")?.max(0) as u32),
        max_memory_bytes: bool_field(env, &budget, "hasMaxMemoryBytes")?
            .then_some(long_field(env, &budget, "maxMemoryBytes")?.max(0) as u64),
        max_disk_bytes: bool_field(env, &budget, "hasMaxDiskBytes")?
            .then_some(long_field(env, &budget, "maxDiskBytes")?.max(0) as u64),
        warmup_window: bool_field(env, &budget, "hasWarmupWindowMs")?.then_some(
            Duration::from_millis(long_field(env, &budget, "warmupWindowMs")?.max(0) as u64),
        ),
    })
}

fn resolved_preload_budget_from_java(
    env: &mut Env<'_>,
    budget: JObject<'_>,
) -> JniResult<PreloadBudget> {
    Ok(PreloadBudget {
        max_concurrent_tasks: int_field(env, &budget, "maxConcurrentTasks")?.max(0) as u32,
        max_memory_bytes: long_field(env, &budget, "maxMemoryBytes")?.max(0) as u64,
        max_disk_bytes: long_field(env, &budget, "maxDiskBytes")?.max(0) as u64,
        warmup_window: Duration::from_millis(
            long_field(env, &budget, "warmupWindowMs")?.max(0) as u64
        ),
    })
}

fn resolved_preload_budget_object<'local>(
    env: &mut Env<'local>,
    budget: &player_runtime::PlayerResolvedPreloadBudgetPolicy,
) -> JniResult<JObject<'local>> {
    let class = env.find_class(jni_name(format!("{PKG}/NativeResolvedPreloadBudgetPolicy")))?;
    env.new_object(
        class,
        method_sig("(IJJJ)V").method_signature(),
        &[
            JValue::Int(budget.max_concurrent_tasks.min(i32::MAX as u32) as jint),
            JValue::Long(u64_to_jlong_saturating(budget.max_memory_bytes)),
            JValue::Long(u64_to_jlong_saturating(budget.max_disk_bytes)),
            JValue::Long(u128_to_jlong_saturating(budget.warmup_window.as_millis())),
        ],
    )
}

fn preload_candidate_from_java(
    env: &mut Env<'_>,
    candidate: JObject<'_>,
) -> JniResult<PreloadCandidate> {
    let scope_kind = int_field(env, &candidate, "scopeKindOrdinal")?;
    let scope_id = string_field(env, &candidate, "scopeId")?;
    let scope = match scope_kind {
        1 => PreloadBudgetScope::Session(scope_id.unwrap_or_default()),
        2 => PreloadBudgetScope::Playlist(scope_id.unwrap_or_default()),
        _ => PreloadBudgetScope::App,
    };

    let source_uri = string_field(env, &candidate, "sourceUri")?.unwrap_or_default();
    let kind = match int_field(env, &candidate, "kindOrdinal")? {
        1 => PreloadCandidateKind::Neighbor,
        2 => PreloadCandidateKind::Recommended,
        3 => PreloadCandidateKind::Background,
        _ => PreloadCandidateKind::Current,
    };
    let selection_hint = match int_field(env, &candidate, "selectionHintOrdinal")? {
        1 => PreloadSelectionHint::CurrentItem,
        2 => PreloadSelectionHint::NeighborItem,
        3 => PreloadSelectionHint::RecommendedItem,
        4 => PreloadSelectionHint::BackgroundFill,
        _ => PreloadSelectionHint::None,
    };
    let priority = match int_field(env, &candidate, "priorityOrdinal")? {
        1 => PreloadPriority::High,
        2 => PreloadPriority::Normal,
        3 => PreloadPriority::Low,
        4 => PreloadPriority::Background,
        _ => PreloadPriority::Critical,
    };
    let has_ttl = bool_field(env, &candidate, "hasTtlMs")?;
    let ttl_ms = long_field(env, &candidate, "ttlMs")?.max(0) as u64;
    let has_warmup_window = bool_field(env, &candidate, "hasWarmupWindowMs")?;
    let warmup_window_ms = long_field(env, &candidate, "warmupWindowMs")?.max(0) as u64;

    Ok(PreloadCandidate {
        source: MediaSource::new(source_uri),
        scope,
        kind,
        selection_hint,
        config: PreloadConfig {
            priority,
            ttl: has_ttl.then_some(Duration::from_millis(ttl_ms)),
            expected_memory_bytes: long_field(env, &candidate, "expectedMemoryBytes")?.max(0)
                as u64,
            expected_disk_bytes: long_field(env, &candidate, "expectedDiskBytes")?.max(0) as u64,
            warmup_window: has_warmup_window.then_some(Duration::from_millis(warmup_window_ms)),
        },
    })
}

fn preload_task_object<'local>(
    env: &mut Env<'local>,
    task: &player_runtime::PreloadTaskSnapshot,
) -> JniResult<JObject<'local>> {
    let class = env.find_class(jni_name(format!("{PKG}/NativePreloadTask")))?;
    let source_uri = JObject::from(env.new_string(task.source.uri())?);
    let source_identity = JObject::from(env.new_string(task.source_identity.as_str())?);
    let cache_key = JObject::from(env.new_string(task.cache_key.as_str())?);
    let scope_id = match &task.scope {
        player_runtime::PreloadBudgetScope::App => JObject::null(),
        player_runtime::PreloadBudgetScope::Session(value)
        | player_runtime::PreloadBudgetScope::Playlist(value) => {
            JObject::from(env.new_string(value)?)
        }
    };
    let error_message = optional_java_string(
        env,
        task.error_summary
            .as_ref()
            .map(|summary| summary.message.as_str()),
    )?;

    let (scope_kind_ordinal, scope_id_object) = match &task.scope {
        player_runtime::PreloadBudgetScope::App => (0, JObject::null()),
        player_runtime::PreloadBudgetScope::Session(_) => (1, scope_id),
        player_runtime::PreloadBudgetScope::Playlist(_) => (2, scope_id),
    };

    env.new_object(
        class,
        method_sig("(JLjava/lang/String;Ljava/lang/String;Ljava/lang/String;ILjava/lang/String;IIIJJJZJIILjava/lang/String;)V")
            .method_signature(),
        &[
            JValue::Long(u64_to_jlong_saturating(task.task_id.get())),
            JValue::Object(&source_uri),
            JValue::Object(&source_identity),
            JValue::Object(&cache_key),
            JValue::Int(scope_kind_ordinal),
            JValue::Object(&scope_id_object),
            JValue::Int(match task.kind {
                player_runtime::PreloadCandidateKind::Current => 0,
                player_runtime::PreloadCandidateKind::Neighbor => 1,
                player_runtime::PreloadCandidateKind::Recommended => 2,
                player_runtime::PreloadCandidateKind::Background => 3,
            }),
            JValue::Int(match task.selection_hint {
                player_runtime::PreloadSelectionHint::None => 0,
                player_runtime::PreloadSelectionHint::CurrentItem => 1,
                player_runtime::PreloadSelectionHint::NeighborItem => 2,
                player_runtime::PreloadSelectionHint::RecommendedItem => 3,
                player_runtime::PreloadSelectionHint::BackgroundFill => 4,
            }),
            JValue::Int(match task.priority {
                player_runtime::PreloadPriority::Critical => 0,
                player_runtime::PreloadPriority::High => 1,
                player_runtime::PreloadPriority::Normal => 2,
                player_runtime::PreloadPriority::Low => 3,
                player_runtime::PreloadPriority::Background => 4,
            }),
            JValue::Long(u64_to_jlong_saturating(task.expected_memory_bytes)),
            JValue::Long(u64_to_jlong_saturating(task.expected_disk_bytes)),
            JValue::Long(u128_to_jlong_saturating(task.warmup_window.as_millis())),
            JValue::Bool(task.expires_at.is_some()),
            JValue::Long(
                task.expires_at
                    .and_then(|expires_at| expires_at.checked_duration_since(Instant::now()))
                    .map(|duration| u128_to_jlong_saturating(duration.as_millis()))
                    .unwrap_or_default(),
            ),
            JValue::Int(match task.status {
                player_runtime::PreloadTaskStatus::Planned => 0,
                player_runtime::PreloadTaskStatus::Active => 1,
                player_runtime::PreloadTaskStatus::Cancelled => 2,
                player_runtime::PreloadTaskStatus::Completed => 3,
                player_runtime::PreloadTaskStatus::Expired => 4,
                player_runtime::PreloadTaskStatus::Failed => 5,
            }),
            JValue::Int(task.error_summary.as_ref().map(|summary| summary.code as jint).unwrap_or_default()),
            JValue::Object(&error_message),
        ],
    )
}

pub(crate) fn preload_command_object<'local>(
    env: &mut Env<'local>,
    command: &AndroidPreloadCommand,
) -> JniResult<JObject<'local>> {
    match command {
        AndroidPreloadCommand::Start { task } => {
            let class = env.find_class(jni_name(format!("{PKG}/NativePreloadCommand$Start")))?;
            let task = preload_task_object(env, task)?;
            env.new_object(
                class,
                method_sig(&format!("(L{PKG}/NativePreloadTask;)V")).method_signature(),
                &[JValue::Object(&task)],
            )
        }
        AndroidPreloadCommand::Cancel { task_id } => {
            let class = env.find_class(jni_name(format!("{PKG}/NativePreloadCommand$Cancel")))?;
            env.new_object(
                class,
                method_sig("(J)V").method_signature(),
                &[JValue::Long(u64_to_jlong_saturating(task_id.get()))],
            )
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_createPreloadSession(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    preload_budget: JObject<'_>,
) -> jlong {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jlong> {
                let budget = resolved_preload_budget_from_java(env, preload_budget)?;
                match new_preload_session(budget) {
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
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_resolvePreloadBudget(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    preload_budget: JObject<'_>,
) -> jobject {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jobject> {
                let resolved = resolve_preload_budget_with_runtime(
                    sparse_preload_budget_from_java(env, preload_budget)?,
                );
                Ok(resolved_preload_budget_object(env, &resolved)?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_disposePreloadSession(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|_env| -> JniResult<()> {
                let mut guard = lock_or_recover(preload_sessions());
                guard.remove(session_handle);
                Ok(())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_planPreloadCandidates(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    candidates: jobjectArray,
    now_epoch_ms: jlong,
) -> jobjectArray {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jobjectArray> {
                let candidates_array = unsafe {
                    JObjectArray::<JObject<'_>>::from_raw(env, candidates as jobjectArray)
                };
                let len = candidates_array.len(env)?;
                let mut rust_candidates = Vec::with_capacity(len);
                for index in 0..len {
                    let candidate = candidates_array.get_element(env, index)?;
                    if !candidate.is_null() {
                        rust_candidates.push(preload_candidate_from_java(env, candidate)?);
                    }
                }

                let Some(task_ids) = with_preload_session_mut(env, session_handle, |session| {
                    session.plan(rust_candidates, Instant::now())
                }) else {
                    let long_class = env.find_class(jni_name("java/lang/Long"))?;
                    let empty: JObjectArray<'_> =
                        env.new_object_array(0, long_class, JObject::null())?;
                    return Ok(empty.into_raw());
                };

                let long_class = env.find_class(jni_name("java/lang/Long"))?;
                let array: JObjectArray<'_> =
                    env.new_object_array(task_ids.len() as i32, long_class, JObject::null())?;
                for (index, task_id) in task_ids.iter().enumerate() {
                    let boxed = env
                        .call_static_method(
                            jni_name("java/lang/Long"),
                            jni_name("valueOf"),
                            method_sig("(J)Ljava/lang/Long;").method_signature(),
                            &[JValue::Long(u64_to_jlong_saturating(task_id.get()))],
                        )?
                        .l()?;
                    array.set_element(env, index, boxed)?;
                }
                let _ = now_epoch_ms;
                Ok(array.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_drainPreloadCommands(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) -> jobjectArray {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jobjectArray> {
                let Some(commands) = with_preload_session_mut(env, session_handle, |session| {
                    session.drain_commands()
                }) else {
                    let command_class =
                        env.find_class(jni_name(format!("{PKG}/NativePreloadCommand")))?;
                    let array: JObjectArray<'_> =
                        env.new_object_array(0, command_class, JObject::null())?;
                    return Ok(array.into_raw());
                };

                let command_class =
                    env.find_class(jni_name(format!("{PKG}/NativePreloadCommand")))?;
                let array: JObjectArray<'_> =
                    env.new_object_array(commands.len() as i32, command_class, JObject::null())?;
                for (index, command) in commands.iter().enumerate() {
                    let object = preload_command_object(env, command)?;
                    array.set_element(env, index, object)?;
                }
                Ok(array.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_completePreloadTask(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    task_id: jlong,
) -> jboolean {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jboolean> {
                let Some(result) = with_preload_session_mut(env, session_handle, |session| {
                    session.complete(PreloadTaskId::from_raw(task_id.max(0) as u64))
                }) else {
                    return Ok(false as jboolean);
                };
                Ok(result.is_ok() as jboolean)
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_failPreloadTask(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    task_id: jlong,
    code_ordinal: jint,
    category_ordinal: jint,
    retriable: jboolean,
    message: JString<'_>,
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
                let Some(result) = with_preload_session_mut(env, session_handle, |session| {
                    session.fail(PreloadTaskId::from_raw(task_id.max(0) as u64), error)
                }) else {
                    return Ok(false as jboolean);
                };
                Ok(result.is_ok() as jboolean)
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}
