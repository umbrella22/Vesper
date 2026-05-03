use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use jni::errors::{Result as JniResult, ThrowRuntimeExAndDefault};
use jni::objects::{JClass, JObject, JObjectArray, JString, JValue};
use jni::sys::{jboolean, jint, jlong, jobject, jobjectArray};
use jni::{Env, EnvUnowned};
use player_core::MediaSource;
use player_platform_android::AndroidPlaylistBridgeSession;
use player_runtime::{
    PlayerRuntimeError, PlaylistCoordinatorConfig, PlaylistFailureStrategy, PlaylistNeighborWindow,
    PlaylistPreloadWindow, PlaylistQueueItem, PlaylistRepeatMode, PlaylistSwitchPolicy,
    PlaylistViewportHint, PlaylistViewportHintKind, PreloadBudget, PreloadTaskId,
};

use crate::{
    HandleRegistry, PKG, error_category_from_ordinal, error_code_from_ordinal, field_sig, jni_name,
    lock_or_recover, method_sig, preload_jni::preload_command_object, run_jni_entry,
};

type AndroidJniPlaylistSession = Arc<Mutex<AndroidPlaylistBridgeSession>>;

static PLAYLIST_SESSIONS: OnceLock<Mutex<HandleRegistry<AndroidJniPlaylistSession>>> =
    OnceLock::new();

fn playlist_sessions() -> &'static Mutex<HandleRegistry<AndroidJniPlaylistSession>> {
    PLAYLIST_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

fn invalid_playlist_handle_error() -> &'static str {
    "invalid android JNI playlist session handle"
}

fn with_playlist_session_mut<R>(
    env: &mut Env<'_>,
    handle: jlong,
    f: impl FnOnce(&mut AndroidPlaylistBridgeSession) -> R,
) -> Option<R> {
    let session = {
        let guard = lock_or_recover(playlist_sessions());
        let Some(session) = guard.get(handle).cloned() else {
            let _ = env.throw_new(
                jni_name("java/lang/IllegalArgumentException"),
                jni_name(invalid_playlist_handle_error()),
            );
            return None;
        };
        session
    };

    // 持有 session 锁期间不要回调 Java，也不要触发会重入 JNI 的析构链路。
    let mut session = lock_or_recover(session.as_ref());
    Some(f(&mut session))
}

fn with_playlist_session<R>(
    env: &mut Env<'_>,
    handle: jlong,
    f: impl FnOnce(&AndroidPlaylistBridgeSession) -> R,
) -> Option<R> {
    let session = {
        let guard = lock_or_recover(playlist_sessions());
        let Some(session) = guard.get(handle).cloned() else {
            let _ = env.throw_new(
                jni_name("java/lang/IllegalArgumentException"),
                jni_name(invalid_playlist_handle_error()),
            );
            return None;
        };
        session
    };

    let session = lock_or_recover(session.as_ref());
    Some(f(&session))
}

fn new_playlist_session(
    playlist_id: String,
    config: PlaylistCoordinatorConfig,
    budget: PreloadBudget,
) -> Result<jlong, &'static str> {
    let session = Arc::new(Mutex::new(AndroidPlaylistBridgeSession::new(
        playlist_id,
        config,
        budget,
    )));
    let mut guard = lock_or_recover(playlist_sessions());
    let handle = guard.insert(session);
    if handle == 0 {
        return Err("android JNI playlist session registry overflow");
    }
    Ok(handle)
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

fn playlist_config_from_java(
    env: &mut Env<'_>,
    config: JObject<'_>,
) -> JniResult<(String, PlaylistCoordinatorConfig)> {
    let playlist_id = string_field(env, &config, "playlistId")?
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "android-host-playlist".to_owned());

    Ok((
        playlist_id,
        PlaylistCoordinatorConfig {
            neighbor_window: PlaylistNeighborWindow {
                previous: int_field(env, &config, "neighborPrevious")?.max(0) as usize,
                next: int_field(env, &config, "neighborNext")?.max(0) as usize,
            },
            preload_window: PlaylistPreloadWindow {
                near_visible: int_field(env, &config, "preloadNearVisible")?.max(0) as usize,
                prefetch_only: int_field(env, &config, "preloadPrefetchOnly")?.max(0) as usize,
            },
            switch_policy: PlaylistSwitchPolicy {
                auto_advance: bool_field(env, &config, "autoAdvance")?,
                repeat_mode: match int_field(env, &config, "repeatModeOrdinal")? {
                    1 => PlaylistRepeatMode::One,
                    2 => PlaylistRepeatMode::All,
                    _ => PlaylistRepeatMode::Off,
                },
                failure_strategy: match int_field(env, &config, "failureStrategyOrdinal")? {
                    1 => PlaylistFailureStrategy::Pause,
                    _ => PlaylistFailureStrategy::SkipToNext,
                },
            },
        },
    ))
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

fn playlist_queue_item_from_java(
    env: &mut Env<'_>,
    item: JObject<'_>,
) -> JniResult<PlaylistQueueItem> {
    let item_id = string_field(env, &item, "itemId")?.unwrap_or_default();
    let source_uri = string_field(env, &item, "sourceUri")?.unwrap_or_default();

    Ok(
        PlaylistQueueItem::new(item_id, MediaSource::new(source_uri)).with_preload_profile(
            player_runtime::PlaylistItemPreloadProfile {
                expected_memory_bytes: long_field(env, &item, "expectedMemoryBytes")?.max(0) as u64,
                expected_disk_bytes: long_field(env, &item, "expectedDiskBytes")?.max(0) as u64,
                ttl: bool_field(env, &item, "hasTtlMs")?.then_some(Duration::from_millis(
                    long_field(env, &item, "ttlMs")?.max(0) as u64,
                )),
                warmup_window: bool_field(env, &item, "hasWarmupWindowMs")?.then_some(
                    Duration::from_millis(long_field(env, &item, "warmupWindowMs")?.max(0) as u64),
                ),
            },
        ),
    )
}

fn playlist_viewport_hint_from_java(
    env: &mut Env<'_>,
    hint: JObject<'_>,
) -> JniResult<PlaylistViewportHint> {
    let kind = match int_field(env, &hint, "kindOrdinal")? {
        0 => PlaylistViewportHintKind::Visible,
        1 => PlaylistViewportHintKind::NearVisible,
        2 => PlaylistViewportHintKind::PrefetchOnly,
        _ => PlaylistViewportHintKind::Hidden,
    };

    Ok(PlaylistViewportHint::new(
        string_field(env, &hint, "itemId")?.unwrap_or_default(),
        kind,
    )
    .with_order(int_field(env, &hint, "order")?.max(0) as u32))
}

fn playlist_active_item_object<'local>(
    env: &mut Env<'local>,
    item: &player_runtime::PlaylistActiveItem,
) -> JniResult<JObject<'local>> {
    let class = env.find_class(jni_name(format!("{PKG}/NativePlaylistActiveItem")))?;
    let item_id = JObject::from(env.new_string(item.item_id.as_str())?);
    env.new_object(
        class,
        method_sig("(Ljava/lang/String;I)V").method_signature(),
        &[
            JValue::Object(&item_id),
            JValue::Int(item.index.min(i32::MAX as usize) as jint),
        ],
    )
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_createPlaylistSession(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    config: JObject<'_>,
    preload_budget: JObject<'_>,
) -> jlong {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jlong> {
                let (playlist_id, config) = playlist_config_from_java(env, config)?;
                let budget = resolved_preload_budget_from_java(env, preload_budget)?;
                Ok(new_playlist_session(playlist_id, config, budget).unwrap_or_default())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_disposePlaylistSession(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|_env| -> JniResult<()> {
                let mut guard = lock_or_recover(playlist_sessions());
                guard.remove(session_handle);
                Ok(())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_replacePlaylistQueue(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    queue: jobjectArray,
    now_epoch_ms: jlong,
) -> jboolean {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jboolean> {
                let queue_array =
                    unsafe { JObjectArray::<JObject<'_>>::from_raw(env, queue as jobjectArray) };
                let len = queue_array.len(env)?;
                let mut rust_queue = Vec::with_capacity(len);
                for index in 0..len {
                    let item = queue_array.get_element(env, index)?;
                    if !item.is_null() {
                        rust_queue.push(playlist_queue_item_from_java(env, item)?);
                    }
                }

                let Some(()) = with_playlist_session_mut(env, session_handle, |session| {
                    session.replace_queue(rust_queue, Instant::now());
                }) else {
                    return Ok(false as jboolean);
                };
                let _ = now_epoch_ms;
                Ok(true as jboolean)
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_updatePlaylistViewportHints(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    hints: jobjectArray,
    now_epoch_ms: jlong,
) -> jboolean {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jboolean> {
                let hints_array =
                    unsafe { JObjectArray::<JObject<'_>>::from_raw(env, hints as jobjectArray) };
                let len = hints_array.len(env)?;
                let mut rust_hints = Vec::with_capacity(len);
                for index in 0..len {
                    let hint = hints_array.get_element(env, index)?;
                    if !hint.is_null() {
                        rust_hints.push(playlist_viewport_hint_from_java(env, hint)?);
                    }
                }

                let Some(()) = with_playlist_session_mut(env, session_handle, |session| {
                    session.update_viewport_hints(rust_hints, Instant::now());
                }) else {
                    return Ok(false as jboolean);
                };
                let _ = now_epoch_ms;
                Ok(true as jboolean)
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_clearPlaylistViewportHints(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    now_epoch_ms: jlong,
) -> jboolean {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jboolean> {
                let Some(()) = with_playlist_session_mut(env, session_handle, |session| {
                    session.clear_viewport_hints(Instant::now());
                }) else {
                    return Ok(false as jboolean);
                };
                let _ = now_epoch_ms;
                Ok(true as jboolean)
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

fn advance_playlist(
    mut unowned_env: EnvUnowned<'_>,
    session_handle: jlong,
    advance: impl FnOnce(&mut AndroidPlaylistBridgeSession, Instant),
) -> jboolean {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jboolean> {
                let Some(()) = with_playlist_session_mut(env, session_handle, |session| {
                    advance(session, Instant::now());
                }) else {
                    return Ok(false as jboolean);
                };
                Ok(true as jboolean)
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_advancePlaylistToNext(
    unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    _now_epoch_ms: jlong,
) -> jboolean {
    advance_playlist(unowned_env, session_handle, |session, now| {
        let _ = session.advance_to_next(now);
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_advancePlaylistToPrevious(
    unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    _now_epoch_ms: jlong,
) -> jboolean {
    advance_playlist(unowned_env, session_handle, |session, now| {
        let _ = session.advance_to_previous(now);
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_handlePlaylistPlaybackCompleted(
    unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    _now_epoch_ms: jlong,
) -> jboolean {
    advance_playlist(unowned_env, session_handle, |session, now| {
        let _ = session.handle_playback_completed(now);
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_handlePlaylistPlaybackFailed(
    unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    _now_epoch_ms: jlong,
) -> jboolean {
    advance_playlist(unowned_env, session_handle, |session, now| {
        let _ = session.handle_playback_failed(now);
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_currentPlaylistActiveItem(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) -> jobject {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jobject> {
                let active_item =
                    with_playlist_session(env, session_handle, |session| session.active_item())
                        .unwrap_or(None);

                let Some(active_item) = active_item else {
                    return Ok(JObject::null().into_raw());
                };

                Ok(playlist_active_item_object(env, &active_item)?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_drainPlaylistPreloadCommands(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) -> jobjectArray {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jobjectArray> {
                let Some(commands) = with_playlist_session_mut(env, session_handle, |session| {
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
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_completePlaylistPreloadTask(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    task_id: jlong,
) -> jboolean {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jboolean> {
                let Some(result) = with_playlist_session_mut(env, session_handle, |session| {
                    session.complete_preload_task(PreloadTaskId::from_raw(task_id.max(0) as u64))
                }) else {
                    return Ok(false as jboolean);
                };
                Ok(result.is_ok() as jboolean)
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_failPlaylistPreloadTask(
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
                let Some(result) = with_playlist_session_mut(env, session_handle, |session| {
                    session.fail_preload_task(PreloadTaskId::from_raw(task_id.max(0) as u64), error)
                }) else {
                    return Ok(false as jboolean);
                };
                Ok(result.is_ok() as jboolean)
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}
