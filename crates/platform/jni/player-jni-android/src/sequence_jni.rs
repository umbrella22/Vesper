use std::ptr;
use std::sync::{Arc, Mutex, OnceLock};

use jni::EnvUnowned;
use jni::errors::{Result as JniResult, ThrowRuntimeExAndDefault};
use jni::objects::{JClass, JString};
use jni::sys::{jint, jlong, jstring};
use player_platform_android::AndroidSequenceBridgeSession;

use crate::{HandleRegistry, jni_name, lock_or_recover, run_jni_entry};

type AndroidJniSequenceSession = Arc<Mutex<AndroidSequenceBridgeSession>>;

static SEQUENCE_SESSIONS: OnceLock<Mutex<HandleRegistry<AndroidJniSequenceSession>>> =
    OnceLock::new();

fn sequence_sessions() -> &'static Mutex<HandleRegistry<AndroidJniSequenceSession>> {
    SEQUENCE_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

fn with_sequence_session_mut<R>(
    env: &mut jni::Env<'_>,
    handle: jlong,
    f: impl FnOnce(&mut AndroidSequenceBridgeSession) -> R,
) -> Option<R> {
    let session = {
        let guard = lock_or_recover(sequence_sessions());
        let Some(session) = guard.get(handle).cloned() else {
            let _ = env.throw_new(
                jni_name("java/lang/IllegalArgumentException"),
                jni_name("invalid Android sequence session handle"),
            );
            return None;
        };
        session
    };
    let mut session = lock_or_recover(session.as_ref());
    Some(f(&mut session))
}

fn with_sequence_session<R>(
    env: &mut jni::Env<'_>,
    handle: jlong,
    f: impl FnOnce(&AndroidSequenceBridgeSession) -> R,
) -> Option<R> {
    let session = {
        let guard = lock_or_recover(sequence_sessions());
        let Some(session) = guard.get(handle).cloned() else {
            let _ = env.throw_new(
                jni_name("java/lang/IllegalArgumentException"),
                jni_name("invalid Android sequence session handle"),
            );
            return None;
        };
        session
    };
    let session = lock_or_recover(session.as_ref());
    Some(f(&session))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_createSequenceSession(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    config_json: JString<'_>,
) -> jlong {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jlong> {
                let config_json = config_json.try_to_string(env)?;
                let session = match AndroidSequenceBridgeSession::from_config_json(&config_json) {
                    Ok(session) => session,
                    Err(error) => {
                        env.throw_new(
                            jni_name("java/lang/IllegalArgumentException"),
                            jni_name(format!("{}: {}", error.code, error.message)),
                        )?;
                        return Ok(0);
                    }
                };
                let mut sessions = lock_or_recover(sequence_sessions());
                Ok(sessions.insert(Arc::new(Mutex::new(session))))
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_disposeSequenceSession(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|_env| -> JniResult<()> {
                let mut sessions = lock_or_recover(sequence_sessions());
                let _ = sessions.remove(session_handle);
                Ok(())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_executeSequenceCommand(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    command_json: JString<'_>,
    wall_epoch_ms: jlong,
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                let command_json = command_json.try_to_string(env)?;
                let Some(response) = with_sequence_session_mut(env, session_handle, |session| {
                    session.execute_json(&command_json, wall_epoch_ms.max(0) as u64)
                }) else {
                    return Ok(ptr::null_mut());
                };
                Ok(env.new_string(response)?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_sequenceSnapshot(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                let Some(response) =
                    with_sequence_session(env, session_handle, |session| session.snapshot_json())
                else {
                    return Ok(ptr::null_mut());
                };
                Ok(env.new_string(response)?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_drainSequenceEvents(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    max_count: jint,
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                let Some(response) = with_sequence_session_mut(env, session_handle, |session| {
                    session.drain_events_json(max_count.max(0) as usize)
                }) else {
                    return Ok(ptr::null_mut());
                };
                Ok(env.new_string(response)?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_sequencePreloadIntents(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    wall_epoch_ms: jlong,
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                let Some(response) = with_sequence_session(env, session_handle, |session| {
                    session.preload_intents_json(wall_epoch_ms.max(0) as u64)
                }) else {
                    return Ok(ptr::null_mut());
                };
                Ok(env.new_string(response)?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}
