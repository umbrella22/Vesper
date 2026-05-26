#![warn(clippy::undocumented_unsafe_blocks)]

mod download_jni;
mod handles;
mod object_builders;
mod parsers;
mod playlist_jni;
mod preload_jni;
mod sessions;

use std::path::PathBuf;
use std::time::Duration;

use jni::EnvUnowned;
use jni::errors::{Result as JniResult, ThrowRuntimeExAndDefault};
use jni::objects::{JClass, JObject, JObjectArray, JString};
use jni::signature::{RuntimeFieldSignature, RuntimeMethodSignature};
use jni::strings::JNIString;
use jni::sys::{jboolean, jfloat, jint, jlong, jobject, jobjectArray, jstring};
use player_platform_android::AndroidExoPlaybackSnapshot;
use player_platform_mobile::{
    MobileFrameProcessorConfiguration, MobileSourceNormalizerConfiguration,
    MobileSourceNormalizerRouteDecision, mobile_plugin_diagnostics_json,
    mobile_source_normalizer_resource_open_json, mobile_source_normalizer_resource_status_json,
    open_mobile_source_normalizer_resource,
};
use player_runtime::{FrameProcessorMode, PlayerError, PlayerRuntimeCommand, SourceNormalizerMode};

pub(crate) const PKG: &str = "io/github/ikaros/vesper/player/android";

pub(crate) use handles::{
    HandleRegistry, lock_or_recover, run_jni_entry, u64_to_jlong_saturating,
    u128_to_jlong_saturating,
};
use object_builders::{
    host_event_object, host_snapshot_object, native_command_object,
    resolved_resilience_policy_object, track_preferences_object,
};
pub(crate) use parsers::{error_category_from_jni_ordinal, error_code_from_jni_ordinal};
use parsers::{
    exo_state_from_ordinal, parse_native_abr_policy, parse_native_buffering_policy,
    parse_native_cache_policy, parse_native_retry_policy, parse_native_track_catalog,
    parse_native_track_preferences, parse_native_track_selection,
    parse_native_track_selection_snapshot, source_kind_from_ordinal, source_protocol_from_ordinal,
    string_array_to_vec, string_from_java_object,
};
pub(crate) use sessions::resolve_preload_budget_with_runtime;
use sessions::{
    dispose_benchmark_sink_session, dispose_source_normalizer_resource_session,
    new_benchmark_sink_session, new_session, new_source_normalizer_resource_session,
    resolve_resilience_policy_with_runtime, resolve_track_preferences_with_runtime, sessions,
    with_benchmark_sink_session, with_session_mut, with_source_normalizer_resource_session_mut,
};

pub(crate) fn jni_name(value: impl AsRef<str>) -> JNIString {
    JNIString::from(value.as_ref())
}

pub(crate) fn method_sig(value: &str) -> RuntimeMethodSignature {
    match RuntimeMethodSignature::from_str(value) {
        Ok(signature) => signature,
        Err(_) => RuntimeMethodSignature::from(jni::jni_sig!("()V")),
    }
}

pub(crate) fn field_sig(value: impl AsRef<str>) -> RuntimeFieldSignature {
    match RuntimeFieldSignature::from_str(value.as_ref()) {
        Ok(signature) => signature,
        Err(_) => RuntimeFieldSignature::from(jni::jni_sig!("J")),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_createSession(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    source_uri: JString<'_>,
) -> jlong {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jlong> {
                let source_uri = source_uri.try_to_string(env)?;
                match new_session(source_uri) {
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
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_createBenchmarkSinkSession(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    plugin_library_paths: JObjectArray<'_>,
) -> jlong {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jlong> {
                let paths = string_array_to_vec(env, plugin_library_paths)?;
                match new_benchmark_sink_session(paths) {
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
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_probeMobilePlugins(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    source_uri: JString<'_>,
    source_mode_ordinal: jint,
    source_plugin_library_paths: JObjectArray<'_>,
    runtime_profile: JObject<'_>,
    frame_mode_ordinal: jint,
    frame_plugin_library_paths: JObjectArray<'_>,
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                let source_uri = source_uri.try_to_string(env)?;
                let source_plugin_library_paths =
                    string_array_to_vec(env, source_plugin_library_paths)?
                        .into_iter()
                        .map(PathBuf::from)
                        .collect();
                let frame_plugin_library_paths =
                    string_array_to_vec(env, frame_plugin_library_paths)?
                        .into_iter()
                        .map(PathBuf::from)
                        .collect();
                let runtime_profile = string_from_java_object(env, runtime_profile)?;
                let diagnostics_json = mobile_plugin_diagnostics_json(
                    &player_model::MediaSource::new(source_uri),
                    &MobileSourceNormalizerConfiguration {
                        mode: source_normalizer_mode_from_ordinal(source_mode_ordinal),
                        plugin_library_paths: source_plugin_library_paths,
                        runtime_profile,
                    },
                    &MobileFrameProcessorConfiguration {
                        mode: frame_processor_mode_from_ordinal(frame_mode_ordinal),
                        plugin_library_paths: frame_plugin_library_paths,
                    },
                )
                .unwrap_or_else(|_| "[]".to_owned());
                Ok(env.new_string(diagnostics_json)?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_openSourceNormalizerResource(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    source_uri: JString<'_>,
    source_mode_ordinal: jint,
    source_plugin_library_paths: JObjectArray<'_>,
    runtime_profile: JObject<'_>,
    output_root: JString<'_>,
    force_normalized: jboolean,
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                let source_uri = source_uri.try_to_string(env)?;
                let plugin_library_paths = string_array_to_vec(env, source_plugin_library_paths)?
                    .into_iter()
                    .map(PathBuf::from)
                    .collect();
                let runtime_profile = string_from_java_object(env, runtime_profile)?;
                let output_root = output_root.try_to_string(env)?;
                let decision = if force_normalized {
                    MobileSourceNormalizerRouteDecision::Force
                } else {
                    MobileSourceNormalizerRouteDecision::NativeFirst
                };
                let opened = match open_mobile_source_normalizer_resource(
                    &player_model::MediaSource::new(source_uri),
                    &MobileSourceNormalizerConfiguration {
                        mode: source_normalizer_mode_from_ordinal(source_mode_ordinal),
                        plugin_library_paths,
                        runtime_profile,
                    },
                    output_root,
                    decision,
                ) {
                    Ok(Some(opened)) => opened,
                    Ok(None) => return Ok(std::ptr::null_mut()),
                    Err(message) => {
                        env.throw_new(
                            jni_name("java/lang/IllegalStateException"),
                            jni_name(message),
                        )?;
                        return Ok(std::ptr::null_mut());
                    }
                };
                let handle = match new_source_normalizer_resource_session(opened) {
                    Ok(handle) => handle,
                    Err(message) => {
                        env.throw_new(
                            jni_name("java/lang/IllegalStateException"),
                            jni_name(message),
                        )?;
                        return Ok(std::ptr::null_mut());
                    }
                };
                let Some(json) =
                    with_source_normalizer_resource_session_mut(env, handle, |opened| {
                        mobile_source_normalizer_resource_open_json(handle as u64, opened, None)
                            .map_err(|error| error.to_string())
                    })
                else {
                    return Ok(std::ptr::null_mut());
                };
                Ok(env.new_string(json)?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_pollSourceNormalizerResource(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                let Some(json) =
                    with_source_normalizer_resource_session_mut(env, handle, |opened| {
                        let status = opened.session.poll().map_err(|error| error.to_string())?;
                        opened.status = status;
                        mobile_source_normalizer_resource_status_json(handle as u64, opened, None)
                            .map_err(|error| error.to_string())
                    })
                else {
                    return Ok(std::ptr::null_mut());
                };
                Ok(env.new_string(json)?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_disposeSourceNormalizerResource(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) {
    run_jni_entry(&mut unowned_env, |_unowned_env| {
        dispose_source_normalizer_resource_session(handle);
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_disposeBenchmarkSinkSession(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) {
    run_jni_entry(&mut unowned_env, |_unowned_env| {
        dispose_benchmark_sink_session(handle);
    })
}

fn source_normalizer_mode_from_ordinal(ordinal: jint) -> SourceNormalizerMode {
    match ordinal {
        1 => SourceNormalizerMode::DiagnosticsOnly,
        2 => SourceNormalizerMode::PreflightOnly,
        3 => SourceNormalizerMode::PreferNormalized,
        4 => SourceNormalizerMode::RequireNormalized,
        _ => SourceNormalizerMode::Disabled,
    }
}

fn frame_processor_mode_from_ordinal(ordinal: jint) -> FrameProcessorMode {
    match ordinal {
        1 => FrameProcessorMode::DiagnosticsOnly,
        _ => FrameProcessorMode::Disabled,
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_submitBenchmarkSinkEvents(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    batch_json: JString<'_>,
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                let batch_json = batch_json.try_to_string(env)?;
                let Some(report_json) = with_benchmark_sink_session(env, handle, |session| {
                    session
                        .on_event_batch_report_json(&batch_json)
                        .map_err(|error| error.to_string())
                }) else {
                    return Ok(std::ptr::null_mut());
                };
                Ok(env.new_string(report_json)?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_flushBenchmarkSinkSession(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                let Some(report_json) = with_benchmark_sink_session(env, handle, |session| {
                    session.flush_json().map_err(|error| error.to_string())
                }) else {
                    return Ok(std::ptr::null_mut());
                };
                Ok(env.new_string(report_json)?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_resolveResiliencePolicy(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    source_kind_ordinal: jint,
    source_protocol_ordinal: jint,
    buffering_policy: JObject<'_>,
    retry_policy: JObject<'_>,
    cache_policy: JObject<'_>,
) -> jobject {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jobject> {
                let resolved = resolve_resilience_policy_with_runtime(
                    source_kind_from_ordinal(source_kind_ordinal),
                    source_protocol_from_ordinal(source_protocol_ordinal),
                    parse_native_buffering_policy(env, buffering_policy)?,
                    parse_native_retry_policy(env, retry_policy)?,
                    parse_native_cache_policy(env, cache_policy)?,
                );
                Ok(resolved_resilience_policy_object(env, &resolved)?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_resolveTrackPreferences(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    track_preferences: JObject<'_>,
) -> jobject {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jobject> {
                let resolved = resolve_track_preferences_with_runtime(
                    parse_native_track_preferences(env, track_preferences)?,
                );
                Ok(track_preferences_object(env, &resolved)?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_disposeSession(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|_env| -> JniResult<()> {
                let mut guard = lock_or_recover(sessions());
                guard.remove(session_handle);
                Ok(())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_attachSurface(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    _surface: JObject<'_>,
    _surface_kind_ordinal: jint,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<()> {
                let _ = with_session_mut(env, session_handle, |session| {
                    session.set_surface_attached(true);
                });
                Ok(())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_detachSurface(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<()> {
                let _ = with_session_mut(env, session_handle, |session| {
                    session.set_surface_attached(false);
                });
                Ok(())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_pollSnapshot(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) -> jobject {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jobject> {
                let Some(snapshot) =
                    with_session_mut(env, session_handle, |session| session.snapshot())
                else {
                    return Ok(JObject::null().into_raw());
                };
                Ok(host_snapshot_object(env, &snapshot)?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_drainEvents(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) -> jobjectArray {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jobjectArray> {
                let Some(events) =
                    with_session_mut(env, session_handle, |session| session.drain_events())
                else {
                    return Ok(std::ptr::null_mut());
                };

                let event_class = env.find_class(jni_name(format!("{PKG}/NativeBridgeEvent")))?;
                let array: JObjectArray<'_> =
                    env.new_object_array(events.len() as i32, event_class, JObject::null())?;
                for (index, event) in events.iter().enumerate() {
                    let object = host_event_object(env, event)?;
                    array.set_element(env, index, object)?;
                }
                Ok(array.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_drainNativeCommands(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) -> jobjectArray {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jobjectArray> {
                let Some(commands) = with_session_mut(env, session_handle, |session| {
                    session.drain_native_commands()
                }) else {
                    return Ok(std::ptr::null_mut());
                };

                let command_class =
                    env.find_class(jni_name(format!("{PKG}/NativePlayerCommand")))?;
                let array: JObjectArray<'_> =
                    env.new_object_array(commands.len() as i32, command_class, JObject::null())?;
                for (index, command) in commands.iter().enumerate() {
                    let object = native_command_object(env, command)?;
                    array.set_element(env, index, object)?;
                }
                Ok(array.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_applyExoSnapshot(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    playback_state_ordinal: jint,
    play_when_ready: jboolean,
    playback_rate: jfloat,
    position_ms: jlong,
    duration_ms: jlong,
    is_live: jboolean,
    is_seekable: jboolean,
    seekable_start_ms: jlong,
    seekable_end_ms: jlong,
    live_edge_ms: jlong,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<()> {
                let snapshot = AndroidExoPlaybackSnapshot {
                    playback_state: exo_state_from_ordinal(playback_state_ordinal),
                    play_when_ready,
                    playback_rate,
                    position: Duration::from_millis(position_ms.max(0) as u64),
                    duration: if duration_ms >= 0 {
                        Some(Duration::from_millis(duration_ms as u64))
                    } else {
                        None
                    },
                    is_live,
                    is_seekable,
                    seekable_range: if seekable_start_ms >= 0
                        && seekable_end_ms >= seekable_start_ms
                    {
                        Some(player_platform_android::AndroidExoSeekableRange {
                            start: Duration::from_millis(seekable_start_ms as u64),
                            end: Duration::from_millis(seekable_end_ms as u64),
                        })
                    } else {
                        None
                    },
                    live_edge: if live_edge_ms >= 0 {
                        Some(Duration::from_millis(live_edge_ms as u64))
                    } else {
                        None
                    },
                };
                let _ = with_session_mut(env, session_handle, |session| {
                    session.apply_exo_snapshot(snapshot);
                });
                Ok(())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_applyTrackState(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    track_catalog: JObject<'_>,
    track_selection: JObject<'_>,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<()> {
                if track_catalog.is_null() || track_selection.is_null() {
                    return Ok(());
                }

                let track_catalog = parse_native_track_catalog(env, track_catalog)?;
                let track_selection = parse_native_track_selection_snapshot(env, track_selection)?;

                let _ = with_session_mut(env, session_handle, |session| {
                    session.report_media_info(track_catalog, track_selection);
                });
                Ok(())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_reportSeekCompleted(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    position_ms: jlong,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<()> {
                let _ = with_session_mut(env, session_handle, |session| {
                    session.report_seek_completed(Duration::from_millis(position_ms.max(0) as u64));
                });
                Ok(())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_reportRetryScheduled(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    attempt: jint,
    delay_ms: jlong,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<()> {
                let _ = with_session_mut(env, session_handle, |session| {
                    session.report_retry_scheduled(
                        attempt.max(0) as u32,
                        Duration::from_millis(delay_ms.max(0) as u64),
                    );
                });
                Ok(())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_reportError(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    code_jni_ordinal: jint,
    category_jni_ordinal: jint,
    retriable: jboolean,
    message: JString<'_>,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<()> {
                let message = message.try_to_string(env)?;
                let code = error_code_from_jni_ordinal(code_jni_ordinal);
                let category = error_category_from_jni_ordinal(category_jni_ordinal);
                let _ = with_session_mut(env, session_handle, |session| {
                    session.report_player_error(PlayerError::with_taxonomy(
                        code, category, retriable, message,
                    ));
                });
                Ok(())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_play(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<()> {
                let _ = with_session_mut(env, session_handle, |session| {
                    let _ = session.dispatch_command(PlayerRuntimeCommand::Play);
                });
                Ok(())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_pause(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<()> {
                let _ = with_session_mut(env, session_handle, |session| {
                    let _ = session.dispatch_command(PlayerRuntimeCommand::Pause);
                });
                Ok(())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_stop(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<()> {
                let _ = with_session_mut(env, session_handle, |session| {
                    let _ = session.dispatch_command(PlayerRuntimeCommand::Stop);
                });
                Ok(())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_seekTo(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    position_ms: jlong,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<()> {
                let _ = with_session_mut(env, session_handle, |session| {
                    let _ = session.dispatch_command(PlayerRuntimeCommand::SeekTo {
                        position: Duration::from_millis(position_ms.max(0) as u64),
                    });
                });
                Ok(())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_setPlaybackRate(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    rate: jfloat,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<()> {
                let _ = with_session_mut(env, session_handle, |session| {
                    let _ =
                        session.dispatch_command(PlayerRuntimeCommand::SetPlaybackRate { rate });
                });
                Ok(())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_setVideoTrackSelection(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    selection: JObject<'_>,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<()> {
                if selection.is_null() {
                    return Ok(());
                }

                let selection = parse_native_track_selection(env, selection)?;
                let _ = with_session_mut(env, session_handle, |session| {
                    let _ =
                        session.dispatch_command(PlayerRuntimeCommand::SetVideoTrackSelection {
                            selection,
                        });
                });
                Ok(())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_setAudioTrackSelection(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    selection: JObject<'_>,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<()> {
                if selection.is_null() {
                    return Ok(());
                }

                let selection = parse_native_track_selection(env, selection)?;
                let _ = with_session_mut(env, session_handle, |session| {
                    let _ =
                        session.dispatch_command(PlayerRuntimeCommand::SetAudioTrackSelection {
                            selection,
                        });
                });
                Ok(())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_setSubtitleTrackSelection(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    selection: JObject<'_>,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<()> {
                if selection.is_null() {
                    return Ok(());
                }

                let selection = parse_native_track_selection(env, selection)?;
                let _ = with_session_mut(env, session_handle, |session| {
                    let _ =
                        session.dispatch_command(PlayerRuntimeCommand::SetSubtitleTrackSelection {
                            selection,
                        });
                });
                Ok(())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_setAbrPolicy(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    policy: JObject<'_>,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<()> {
                if policy.is_null() {
                    return Ok(());
                }

                let policy = parse_native_abr_policy(env, policy)?;
                let _ = with_session_mut(env, session_handle, |session| {
                    let _ = session.dispatch_command(PlayerRuntimeCommand::SetAbrPolicy { policy });
                });
                Ok(())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    });
}

#[cfg(test)]
mod tests {
    use super::handles::next_generation;
    use super::{
        HandleRegistry, error_category_from_jni_ordinal, error_code_from_jni_ordinal,
        resolve_resilience_policy_with_runtime, resolve_track_preferences_with_runtime,
        u64_to_jlong_saturating, u128_to_jlong_saturating,
    };
    use player_runtime::{
        MediaAbrMode, MediaAbrPolicy, MediaSourceKind, MediaSourceProtocol, MediaTrackSelection,
        PlayerBufferingPolicy, PlayerBufferingPreset, PlayerCachePolicy, PlayerCachePreset,
        PlayerErrorCategory, PlayerErrorCode, PlayerRetryBackoff, PlayerRetryPolicy,
        PlayerTrackPreferencePolicy,
    };
    use std::time::Duration;

    #[test]
    fn handle_registry_reuses_slot_with_new_generation_and_rejects_stale_handle() {
        let mut registry = HandleRegistry::default();
        let first = registry.insert(11_u32);

        assert_eq!(registry.get(first), Some(&11));
        assert_eq!(registry.remove(first), Some(11));

        let second = registry.insert(22_u32);
        assert_ne!(first, second);
        assert!(registry.get(first).is_none());
        assert_eq!(registry.get(second), Some(&22));
    }

    #[test]
    fn handle_registry_truncates_trailing_empty_slots() {
        let mut registry = HandleRegistry::default();
        let first = registry.insert(11_u32);
        let second = registry.insert(22_u32);

        assert_eq!(registry.slots.len(), 2);
        assert_eq!(registry.remove(second), Some(22));
        assert_eq!(registry.slots.len(), 1);
        assert!(registry.free_slots.is_empty());

        assert_eq!(registry.remove(first), Some(11));
        assert!(registry.slots.is_empty());
        assert!(registry.free_slots.is_empty());
    }

    #[test]
    fn handle_registry_preserves_interior_free_slot_after_tail_compaction() {
        let mut registry = HandleRegistry::default();
        let first = registry.insert(11_u32);
        let second = registry.insert(22_u32);
        let third = registry.insert(33_u32);

        assert_eq!(registry.remove(first), Some(11));
        assert_eq!(registry.remove(third), Some(33));
        assert_eq!(registry.slots.len(), 2);
        assert_eq!(registry.get(first), None);
        assert_eq!(registry.get(second), Some(&22));

        let fourth = registry.insert(44_u32);
        assert_eq!(registry.slots.len(), 2);
        assert_ne!(fourth, first);
        assert_eq!(registry.get(fourth), Some(&44));
        assert_eq!(registry.get(second), Some(&22));
    }

    #[test]
    fn handle_registry_rejects_zero_handle() {
        let registry = HandleRegistry::<u32>::default();

        assert!(registry.get(0_i64).is_none());
    }

    #[test]
    fn handle_registry_generation_wrap_skips_zero() {
        assert_eq!(next_generation(u32::MAX), 1);
        assert_eq!(next_generation(41), 42);
    }

    #[test]
    fn error_code_jni_ordinals_preserve_stable_values() {
        let cases = [
            (0, PlayerErrorCode::InvalidArgument),
            (1, PlayerErrorCode::InvalidState),
            (2, PlayerErrorCode::InvalidSource),
            (3, PlayerErrorCode::BackendFailure),
            (4, PlayerErrorCode::AudioOutputUnavailable),
            (5, PlayerErrorCode::DecodeFailure),
            (6, PlayerErrorCode::SeekFailure),
            (7, PlayerErrorCode::Unsupported),
            (8, PlayerErrorCode::CommandChannelClosed),
            (9, PlayerErrorCode::EventChannelClosed),
            (10, PlayerErrorCode::Cancelled),
            (11, PlayerErrorCode::Timeout),
        ];

        for (ordinal, code) in cases {
            assert_eq!(error_code_from_jni_ordinal(ordinal), code);
        }
        assert_eq!(
            error_code_from_jni_ordinal(99),
            PlayerErrorCode::BackendFailure
        );
    }

    #[test]
    fn error_category_jni_ordinals_preserve_stable_values() {
        let cases = [
            (0, PlayerErrorCategory::Input),
            (1, PlayerErrorCategory::Source),
            (2, PlayerErrorCategory::Network),
            (3, PlayerErrorCategory::Decode),
            (4, PlayerErrorCategory::AudioOutput),
            (5, PlayerErrorCategory::Playback),
            (6, PlayerErrorCategory::Capability),
            (7, PlayerErrorCategory::Platform),
        ];

        for (ordinal, category) in cases {
            assert_eq!(error_category_from_jni_ordinal(ordinal), category);
        }
        assert_eq!(
            error_category_from_jni_ordinal(99),
            PlayerErrorCategory::Platform
        );
    }

    #[test]
    fn jlong_saturating_helpers_clamp_large_unsigned_values() {
        assert_eq!(u64_to_jlong_saturating(123), 123);
        assert_eq!(u64_to_jlong_saturating(u64::MAX), i64::MAX);
        assert_eq!(u128_to_jlong_saturating(456), 456);
        assert_eq!(u128_to_jlong_saturating(u128::MAX), i64::MAX);
    }

    #[test]
    fn runtime_resolved_policy_uses_hls_defaults_for_android_jni_bridge() {
        let resolved = resolve_resilience_policy_with_runtime(
            MediaSourceKind::Remote,
            MediaSourceProtocol::Hls,
            PlayerBufferingPolicy::default(),
            PlayerRetryPolicy::default(),
            PlayerCachePolicy::default(),
        );

        assert_eq!(
            resolved.buffering_policy.preset,
            PlayerBufferingPreset::Resilient
        );
        assert_eq!(
            resolved.buffering_policy.min_buffer,
            Some(Duration::from_millis(20_000))
        );
        assert_eq!(resolved.cache_policy.preset, PlayerCachePreset::Resilient);
        assert_eq!(
            resolved.cache_policy.max_disk_bytes,
            Some(384 * 1024 * 1024)
        );
        assert_eq!(resolved.retry_policy.max_attempts, Some(3));
        assert_eq!(resolved.retry_policy.backoff, PlayerRetryBackoff::Linear);
    }

    #[test]
    fn runtime_resolved_policy_preserves_retry_overrides_for_android_jni_bridge() {
        let resolved = resolve_resilience_policy_with_runtime(
            MediaSourceKind::Remote,
            MediaSourceProtocol::Progressive,
            PlayerBufferingPolicy::default(),
            PlayerRetryPolicy {
                max_attempts: None,
                base_delay: Duration::from_millis(2_000),
                max_delay: Duration::from_millis(9_000),
                backoff: PlayerRetryBackoff::Exponential,
            },
            PlayerCachePolicy::default(),
        );

        assert_eq!(resolved.retry_policy.max_attempts, None);
        assert_eq!(
            resolved.retry_policy.base_delay,
            Duration::from_millis(2_000)
        );
        assert_eq!(
            resolved.retry_policy.max_delay,
            Duration::from_millis(9_000)
        );
        assert_eq!(
            resolved.retry_policy.backoff,
            PlayerRetryBackoff::Exponential
        );
        assert_eq!(resolved.cache_policy.preset, PlayerCachePreset::Streaming);
    }

    #[test]
    fn runtime_resolved_track_preferences_normalize_blank_values_for_android_jni_bridge() {
        let resolved = resolve_track_preferences_with_runtime(PlayerTrackPreferencePolicy {
            preferred_audio_language: Some("  en-US ".to_owned()),
            preferred_subtitle_language: Some(" ".to_owned()),
            select_subtitles_by_default: true,
            select_undetermined_subtitle_language: true,
            audio_selection: MediaTrackSelection::track(" "),
            subtitle_selection: MediaTrackSelection::track(" subtitle:eng "),
            abr_policy: MediaAbrPolicy {
                mode: MediaAbrMode::FixedTrack,
                track_id: Some(" ".to_owned()),
                max_bit_rate: Some(4_000_000),
                max_width: Some(1_920),
                max_height: Some(1_080),
            },
        });

        assert_eq!(resolved.preferred_audio_language.as_deref(), Some("en-US"));
        assert_eq!(resolved.preferred_subtitle_language, None);
        assert_eq!(resolved.audio_selection, MediaTrackSelection::auto());
        assert_eq!(
            resolved.subtitle_selection,
            MediaTrackSelection::track("subtitle:eng")
        );
        assert_eq!(resolved.abr_policy, MediaAbrPolicy::default());
    }

    #[test]
    fn runtime_resolved_track_preferences_preserve_valid_constraints_for_android_jni_bridge() {
        let resolved = resolve_track_preferences_with_runtime(PlayerTrackPreferencePolicy {
            preferred_audio_language: Some("ja".to_owned()),
            preferred_subtitle_language: Some("zh-Hans".to_owned()),
            select_subtitles_by_default: true,
            select_undetermined_subtitle_language: false,
            audio_selection: MediaTrackSelection::auto(),
            subtitle_selection: MediaTrackSelection::disabled(),
            abr_policy: MediaAbrPolicy {
                mode: MediaAbrMode::Constrained,
                track_id: Some("ignored".to_owned()),
                max_bit_rate: Some(3_500_000),
                max_width: None,
                max_height: Some(1_080),
            },
        });

        assert_eq!(resolved.preferred_audio_language.as_deref(), Some("ja"));
        assert_eq!(
            resolved.preferred_subtitle_language.as_deref(),
            Some("zh-Hans")
        );
        assert_eq!(resolved.subtitle_selection, MediaTrackSelection::disabled());
        assert_eq!(
            resolved.abr_policy,
            MediaAbrPolicy {
                mode: MediaAbrMode::Constrained,
                track_id: None,
                max_bit_rate: Some(3_500_000),
                max_width: None,
                max_height: Some(1_080),
            }
        );
    }
}
