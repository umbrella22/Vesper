use jni::EnvUnowned;
use jni::errors::{Result as JniResult, ThrowRuntimeExAndDefault};
use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jlong, jstring};
use serde_json::{Value, json};

use crate::run_jni_entry;

#[cfg(feature = "rtsp-bridge")]
mod bundled {
    use super::*;
    use player_rtsp_hls_bridge::{Bridge, Phase};
    use std::collections::HashMap;
    use std::sync::{
        LazyLock, Mutex, MutexGuard,
        atomic::{AtomicI64, Ordering},
    };

    static NEXT: AtomicI64 = AtomicI64::new(1);
    static SESSIONS: LazyLock<Mutex<HashMap<jlong, Bridge>>> = LazyLock::new(Mutex::default);

    fn sessions() -> MutexGuard<'static, HashMap<jlong, Bridge>> {
        SESSIONS.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn open(env: &mut jni::Env<'_>, source: &str) -> JniResult<jstring> {
        let bridge = match Bridge::start(source) {
            Ok(bridge) => bridge,
            Err(error) => {
                return Ok(env
                    .new_string(failed(error.code(), error.message()).to_string())?
                    .into_raw());
            }
        };
        let handle = NEXT.fetch_add(1, Ordering::Relaxed);
        let result = env.new_string(
            json!({
                "handle": handle,
                "playlistUri": bridge.playlist_url(),
            })
            .to_string(),
        )?;
        sessions().insert(handle, bridge);
        Ok(result.into_raw())
    }

    pub fn status(handle: jlong) -> Value {
        let Some(snapshot) = sessions().get(&handle).map(Bridge::snapshot) else {
            return closed();
        };
        let phase = match snapshot.phase {
            Phase::Connecting => "connecting",
            Phase::Reconnecting => "reconnecting",
            Phase::Ready => "ready",
            Phase::Failed(error) => return failed(error.code(), error.message()),
        };
        json!({
            "phase": phase,
            "audioOmitted": snapshot.audio_omitted,
            "damagedAccessUnits": snapshot.damaged_access_units,
            "reconnects": snapshot.reconnects,
            "mediaSequence": snapshot.media_sequence,
        })
    }

    pub fn close(handle: jlong) {
        let bridge = sessions().remove(&handle);
        drop(bridge);
    }
}

fn failed(code: &str, message: &str) -> Value {
    json!({"phase": "failed", "code": code, "message": message})
}

fn closed() -> Value {
    failed("rtsp_bridge_closed", "The RTSP bridge session is closed.")
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_umbrella22_vesper_player_android_VesperNativeRtspBridgeJni_open(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    source: JString<'_>,
    loopback_cleartext_permitted: jboolean,
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                #[cfg(feature = "rtsp-bridge")]
                {
                    if !loopback_cleartext_permitted {
                        return Ok(env.new_string(failed(
                            "rtsp_loopback_cleartext_denied",
                            "The app's Network Security Configuration must allow cleartext HTTP to 127.0.0.1 for the RTSP bridge.",
                        ).to_string())?.into_raw());
                    }
                    let source = source.try_to_string(env)?;
                    bundled::open(env, &source)
                }
                #[cfg(not(feature = "rtsp-bridge"))]
                {
                    let _ = (source, loopback_cleartext_permitted);
                    Ok(env.new_string(failed(
                    "rtsp_bridge_not_bundled",
                    "RTSP playback requires a Vesper Android build with the optional RTSP bridge.",
                ).to_string())?.into_raw())
                }
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_umbrella22_vesper_player_android_VesperNativeRtspBridgeJni_status(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                #[cfg(feature = "rtsp-bridge")]
                let state = bundled::status(handle);
                #[cfg(not(feature = "rtsp-bridge"))]
                let state = {
                    let _ = handle;
                    closed()
                };
                Ok(env.new_string(state.to_string())?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_umbrella22_vesper_player_android_VesperNativeRtspBridgeJni_close(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) {
    run_jni_entry(&mut unowned_env, |_| {
        #[cfg(feature = "rtsp-bridge")]
        bundled::close(handle);
        #[cfg(not(feature = "rtsp-bridge"))]
        let _ = handle;
    });
}
