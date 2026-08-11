#![warn(clippy::undocumented_unsafe_blocks)]
#![allow(
    clippy::result_large_err,
    reason = "JNI entrypoints translate the shared player-runtime error contract at one boundary; boxing individual closure results would fragment that mapping"
)]

mod download_jni;
mod handles;
mod native_frame_presenter;
mod object_builders;
mod parsers;
mod playlist_jni;
mod plugin_registry_jni;
mod preload_jni;
mod sequence_jni;
mod sessions;

use std::time::Duration;

use jni::EnvUnowned;
use jni::errors::{Result as JniResult, ThrowRuntimeExAndDefault};
use jni::objects::{JClass, JObject, JObjectArray, JString};
use jni::signature::{RuntimeFieldSignature, RuntimeMethodSignature};
use jni::strings::JNIString;
use jni::sys::{jboolean, jfloat, jint, jlong, jobject, jobjectArray, jstring};
use player_platform_android::{AndroidExoPlaybackSnapshot, AndroidExoSeekableRange};
use player_platform_android::{
    AndroidNativeFramePipelineOpenConfig, AndroidNativeFramePipelineSession,
    AndroidNativeFramePresenterProfile, android_native_frame_pipeline_frame_json,
    android_native_frame_pipeline_open_json, android_native_frame_pipeline_status_json,
};
use player_platform_mobile::{
    MobileFrameProcessorConfiguration, MobileNativeFramePipelineConfiguration,
    MobileSourceNormalizerConfiguration, MobileSourceNormalizerRouteDecision,
    mobile_plugin_diagnostics_json, mobile_source_normalizer_resource_bypass_diagnostics_json,
    mobile_source_normalizer_resource_open_json, mobile_source_normalizer_resource_status_json,
    open_mobile_source_normalizer_resource_with_diagnostics,
    parse_mobile_native_plugin_artifacts_json,
};
use player_runtime::NativeFramePipelineMode;
use player_runtime::{
    FrameProcessorMode, MediaTrackSelection, PipelineEventHookReportBatch, PlayerError,
    PlayerErrorCategory, PlayerErrorCode, PlayerRuntimeCommand, SourceNormalizerMode,
    SubtitleErrorDetails,
};

pub(crate) const PKG: &str = "io/github/ikaros/vesper/player/android";

pub(crate) use handles::{
    HandleRegistry, lock_or_recover, run_jni_entry, u64_to_jlong_saturating,
    u128_to_jlong_saturating,
};
use native_frame_presenter::AndroidNativeWindowPresenterSink;
use object_builders::{
    host_event_object, host_snapshot_object, native_command_object,
    resolved_resilience_policy_object, timeline_object, track_preferences_object,
};
use parsers::{
    boxed_long_value, exo_state_from_ordinal, long_field, parse_native_abr_policy,
    parse_native_buffering_policy, parse_native_cache_policy, parse_native_retry_policy,
    parse_native_track_catalog, parse_native_track_preferences, parse_native_track_selection,
    parse_native_track_selection_snapshot, source_kind_from_ordinal, source_protocol_from_ordinal,
    string_field, string_from_java_object,
};
pub(crate) use parsers::{error_category_from_jni_ordinal, error_code_from_jni_ordinal};
use plugin_registry_jni::{clone_android_plugin_registry, parse_plugin_references};
pub(crate) use sessions::resolve_preload_budget_with_runtime;
use sessions::{
    dispose_benchmark_sink_session, dispose_native_frame_pipeline_session,
    dispose_source_normalizer_resource_session, new_native_frame_pipeline_session,
    new_session_with_plugin_registry, new_source_normalizer_resource_session,
    resolve_resilience_policy_with_runtime, resolve_track_preferences_with_runtime, sessions,
    with_benchmark_sink_session, with_native_frame_pipeline_session_mut, with_session_mut,
    with_session_mut_checked, with_source_normalizer_resource_session_mut,
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
    pipeline_event_hook_configuration: JObject<'_>,
) -> jlong {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jlong> {
                let source_uri = source_uri.try_to_string(env)?;
                let registry_handle = long_field(
                    env,
                    &pipeline_event_hook_configuration,
                    "pluginRegistryHandle",
                )?;
                let references_json = string_field(
                    env,
                    &pipeline_event_hook_configuration,
                    "pluginReferencesJson",
                )?
                .unwrap_or_else(|| "[]".to_owned());
                let references = match parse_plugin_references(&references_json) {
                    Ok(references) => references,
                    Err(message) => {
                        env.throw_new(
                            jni_name("java/lang/IllegalArgumentException"),
                            jni_name(message),
                        )?;
                        return Ok(0);
                    }
                };
                let registry = if references.is_empty() {
                    if registry_handle != 0 {
                        env.throw_new(
                            jni_name("java/lang/IllegalArgumentException"),
                            jni_name(
                                "Android plugin registry handle must be zero when playback event-hook references are empty",
                            ),
                        )?;
                        return Ok(0);
                    }
                    None
                } else {
                    if registry_handle == 0 {
                        env.throw_new(
                            jni_name("java/lang/IllegalArgumentException"),
                            jni_name(
                                "Android plugin registry handle is required for playback event-hook references",
                            ),
                        )?;
                        return Ok(0);
                    }
                    match clone_android_plugin_registry(registry_handle) {
                        Ok(registry) => Some(registry),
                        Err(message) => {
                            env.throw_new(
                                jni_name("java/lang/IllegalArgumentException"),
                                jni_name(message),
                            )?;
                            return Ok(0);
                        }
                    }
                };
                match new_session_with_plugin_registry(source_uri, registry, references) {
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
    source_plugin_artifacts_json: JString<'_>,
    runtime_profile: JObject<'_>,
    frame_mode_ordinal: jint,
    frame_plugin_artifacts_json: JString<'_>,
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                let source_uri = source_uri.try_to_string(env)?;
                let source_plugin_artifacts_json =
                    source_plugin_artifacts_json.try_to_string(env)?;
                let source_plugin_artifacts = match parse_mobile_native_plugin_artifacts_json(
                    &source_plugin_artifacts_json,
                ) {
                    Ok(artifacts) => artifacts,
                    Err(message) => {
                        env.throw_new(
                            jni_name("java/lang/IllegalArgumentException"),
                            jni_name(message),
                        )?;
                        return Ok(std::ptr::null_mut());
                    }
                };
                let frame_plugin_artifacts_json = frame_plugin_artifacts_json.try_to_string(env)?;
                let frame_plugin_artifacts =
                    match parse_mobile_native_plugin_artifacts_json(&frame_plugin_artifacts_json) {
                        Ok(artifacts) => artifacts,
                        Err(message) => {
                            env.throw_new(
                                jni_name("java/lang/IllegalArgumentException"),
                                jni_name(message),
                            )?;
                            return Ok(std::ptr::null_mut());
                        }
                    };
                let runtime_profile = string_from_java_object(env, runtime_profile)?;
                let diagnostics_json = mobile_plugin_diagnostics_json(
                    &player_model::MediaSource::new(source_uri),
                    &MobileSourceNormalizerConfiguration {
                        mode: source_normalizer_mode_from_ordinal(source_mode_ordinal),
                        plugin_artifacts: source_plugin_artifacts,
                        plugin_library_paths: Vec::new(),
                        native_plugin_loading_policy:
                            player_runtime::NativePluginLoadingPolicy::DenyRawPaths,
                        runtime_profile,
                    },
                    &MobileFrameProcessorConfiguration {
                        mode: frame_processor_mode_from_ordinal(frame_mode_ordinal),
                        plugin_artifacts: frame_plugin_artifacts,
                        plugin_library_paths: Vec::new(),
                        native_plugin_loading_policy:
                            player_runtime::NativePluginLoadingPolicy::DenyRawPaths,
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
    source_plugin_artifacts_json: JString<'_>,
    runtime_profile: JObject<'_>,
    output_root: JString<'_>,
    force_normalized: jboolean,
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                let source_uri = source_uri.try_to_string(env)?;
                let plugin_artifacts_json = source_plugin_artifacts_json.try_to_string(env)?;
                let plugin_artifacts =
                    match parse_mobile_native_plugin_artifacts_json(&plugin_artifacts_json) {
                        Ok(artifacts) => artifacts,
                        Err(message) => {
                            env.throw_new(
                                jni_name("java/lang/IllegalArgumentException"),
                                jni_name(message),
                            )?;
                            return Ok(std::ptr::null_mut());
                        }
                    };
                let runtime_profile = string_from_java_object(env, runtime_profile)?;
                let output_root = output_root.try_to_string(env)?;
                let decision = if force_normalized {
                    MobileSourceNormalizerRouteDecision::Force
                } else {
                    MobileSourceNormalizerRouteDecision::NativeFirst
                };
                let outcome = match open_mobile_source_normalizer_resource_with_diagnostics(
                    &player_model::MediaSource::new(source_uri),
                    &MobileSourceNormalizerConfiguration {
                        mode: source_normalizer_mode_from_ordinal(source_mode_ordinal),
                        plugin_artifacts,
                        plugin_library_paths: Vec::new(),
                        native_plugin_loading_policy:
                            player_runtime::NativePluginLoadingPolicy::DenyRawPaths,
                        runtime_profile,
                    },
                    output_root,
                    decision,
                ) {
                    Ok(outcome) => outcome,
                    Err(message) => {
                        env.throw_new(
                            jni_name("java/lang/IllegalStateException"),
                            jni_name(message),
                        )?;
                        return Ok(std::ptr::null_mut());
                    }
                };
                let opened = match outcome.opened {
                    Some(opened) => opened,
                    None => {
                        if outcome.diagnostics.is_empty() {
                            return Ok(std::ptr::null_mut());
                        }
                        let json = match mobile_source_normalizer_resource_bypass_diagnostics_json(
                            &outcome.diagnostics,
                        ) {
                            Ok(value) => value,
                            Err(message) => {
                                env.throw_new(
                                    jni_name("java/lang/IllegalStateException"),
                                    jni_name(message.to_string()),
                                )?;
                                return Ok(std::ptr::null_mut());
                            }
                        };
                        return Ok(env.new_string(json)?.into_raw());
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
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_openNativeFramePipeline(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    source_uri: JString<'_>,
    source_mode_ordinal: jint,
    source_plugin_artifacts_json: JString<'_>,
    runtime_profile: JObject<'_>,
    native_frame_mode: JString<'_>,
    decoder_plugin_artifacts_json: JString<'_>,
    avc_decoder_implementation_name: JObject<'_>,
    hevc_decoder_implementation_name: JObject<'_>,
    frame_processor_plugin_artifacts_json: JString<'_>,
    max_in_flight_frames: jint,
    presenter_profile: JString<'_>,
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                let source_uri = source_uri.try_to_string(env)?;
                let source_plugin_artifacts_json =
                    source_plugin_artifacts_json.try_to_string(env)?;
                let source_plugin_artifacts = match parse_mobile_native_plugin_artifacts_json(
                    &source_plugin_artifacts_json,
                ) {
                    Ok(artifacts) => artifacts,
                    Err(message) => {
                        env.throw_new(
                            jni_name("java/lang/IllegalArgumentException"),
                            jni_name(message),
                        )?;
                        return Ok(std::ptr::null_mut());
                    }
                };
                let runtime_profile = string_from_java_object(env, runtime_profile)?;
                let native_frame_mode = native_frame_mode.try_to_string(env)?;
                let native_frame_mode =
                    match native_frame_pipeline_mode_from_wire_name(&native_frame_mode) {
                        Ok(mode) => mode,
                        Err(message) => {
                            env.throw_new(
                                jni_name("java/lang/IllegalArgumentException"),
                                jni_name(message),
                            )?;
                            return Ok(std::ptr::null_mut());
                        }
                    };
                let presenter_profile = presenter_profile.try_to_string(env)?;
                let presenter_profile =
                    match native_frame_presenter_profile_from_wire_name(&presenter_profile) {
                        Ok(profile) => profile,
                        Err(message) => {
                            env.throw_new(
                                jni_name("java/lang/IllegalArgumentException"),
                                jni_name(message),
                            )?;
                            return Ok(std::ptr::null_mut());
                        }
                    };
                let decoder_plugin_artifacts_json =
                    decoder_plugin_artifacts_json.try_to_string(env)?;
                let decoder_plugin_artifacts =
                    match parse_mobile_native_plugin_artifacts_json(&decoder_plugin_artifacts_json)
                    {
                        Ok(artifacts) => artifacts,
                        Err(message) => {
                            env.throw_new(
                                jni_name("java/lang/IllegalArgumentException"),
                                jni_name(message),
                            )?;
                            return Ok(std::ptr::null_mut());
                        }
                    };
                let avc_decoder_implementation_name =
                    string_from_java_object(env, avc_decoder_implementation_name)?;
                let hevc_decoder_implementation_name =
                    string_from_java_object(env, hevc_decoder_implementation_name)?;
                let frame_processor_plugin_artifacts_json =
                    frame_processor_plugin_artifacts_json.try_to_string(env)?;
                let frame_processor_plugin_artifacts =
                    match parse_mobile_native_plugin_artifacts_json(
                        &frame_processor_plugin_artifacts_json,
                    ) {
                        Ok(artifacts) => artifacts,
                        Err(message) => {
                            env.throw_new(
                                jni_name("java/lang/IllegalArgumentException"),
                                jni_name(message),
                            )?;
                            return Ok(std::ptr::null_mut());
                        }
                    };
                let session = match AndroidNativeFramePipelineSession::open(
                    AndroidNativeFramePipelineOpenConfig {
                        source_uri,
                        source_normalizer: MobileSourceNormalizerConfiguration {
                            mode: source_normalizer_mode_from_ordinal(source_mode_ordinal),
                            plugin_artifacts: source_plugin_artifacts,
                            plugin_library_paths: Vec::new(),
                            native_plugin_loading_policy:
                                player_runtime::NativePluginLoadingPolicy::DenyRawPaths,
                            runtime_profile,
                        },
                        native_frame_pipeline: MobileNativeFramePipelineConfiguration {
                            mode: native_frame_mode,
                            decoder_plugin_artifacts,
                            decoder_plugin_library_paths: Vec::new(),
                            frame_processor_plugin_artifacts,
                            frame_processor_plugin_library_paths: Vec::new(),
                            native_plugin_loading_policy:
                                player_runtime::NativePluginLoadingPolicy::DenyRawPaths,
                            max_in_flight_frames: (max_in_flight_frames > 0)
                                .then_some(max_in_flight_frames as u32),
                        },
                        avc_decoder_implementation_name,
                        hevc_decoder_implementation_name,
                        presenter_profile,
                    },
                ) {
                    Ok(session) => session,
                    Err(error) => {
                        env.throw_new(
                            jni_name("java/lang/IllegalStateException"),
                            jni_name(error.message()),
                        )?;
                        return Ok(std::ptr::null_mut());
                    }
                };
                let handle = match new_native_frame_pipeline_session(session) {
                    Ok(handle) => handle,
                    Err(message) => {
                        env.throw_new(
                            jni_name("java/lang/IllegalStateException"),
                            jni_name(message),
                        )?;
                        return Ok(std::ptr::null_mut());
                    }
                };
                let Some(json) = with_native_frame_pipeline_session_mut(env, handle, |session| {
                    android_native_frame_pipeline_open_json(handle as u64, session)
                        .map_err(|error| error.to_string())
                }) else {
                    return Ok(std::ptr::null_mut());
                };
                Ok(env.new_string(json)?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_advanceNativeFramePipeline(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                let Some(json) = with_native_frame_pipeline_session_mut(env, handle, |session| {
                    let result = session
                        .advance()
                        .map_err(|error| error.message().to_owned())?;
                    let counters = session.status_wire(handle as u64, None).counters;
                    android_native_frame_pipeline_frame_json(result, counters)
                        .map_err(|error| error.to_string())
                }) else {
                    return Ok(std::ptr::null_mut());
                };
                Ok(env.new_string(json)?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_releaseNativeFramePipelineFrame(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    frame_handle: jlong,
    presented: jboolean,
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                let Some(json) = with_native_frame_pipeline_session_mut(env, handle, |session| {
                    session
                        .release_frame(frame_handle.max(0) as u64, presented)
                        .map_err(|error| error.message().to_owned())?;
                    android_native_frame_pipeline_status_json(
                        handle as u64,
                        session,
                        Some("released".to_owned()),
                    )
                    .map_err(|error| error.to_string())
                }) else {
                    return Ok(std::ptr::null_mut());
                };
                Ok(env.new_string(json)?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_attachNativeFramePipelineSurface(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    surface: JObject<'_>,
    surface_kind: JString<'_>,
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                let surface_kind = surface_kind.try_to_string(env)?;
                let surface_profile =
                    match native_frame_presenter_profile_from_wire_name(&surface_kind) {
                        Ok(profile) => profile,
                        Err(message) => {
                            env.throw_new(
                                jni_name("java/lang/IllegalArgumentException"),
                                jni_name(message),
                            )?;
                            return Ok(std::ptr::null_mut());
                        }
                    };
                let presenter_sink =
                    match AndroidNativeWindowPresenterSink::from_surface(env, &surface) {
                        Ok(sink) => sink,
                        Err(message) => {
                            env.throw_new(
                                jni_name("java/lang/IllegalStateException"),
                                jni_name(message),
                            )?;
                            return Ok(std::ptr::null_mut());
                        }
                    };
                let Some(json) = with_native_frame_pipeline_session_mut(env, handle, |session| {
                    session
                        .attach_presenter_surface(surface_profile)
                        .map_err(|error| error.message().to_owned())?;
                    session
                        .configure_presenter_sink(Box::new(presenter_sink))
                        .map_err(|error| error.message().to_owned())?;
                    android_native_frame_pipeline_status_json(
                        handle as u64,
                        session,
                        Some(
                            "presenter surface attached; ANativeWindow presenter context configured"
                                .to_owned(),
                        ),
                    )
                    .map_err(|error| error.to_string())
                }) else {
                    return Ok(std::ptr::null_mut());
                };
                Ok(env.new_string(json)?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_detachNativeFramePipelineSurface(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                let Some(json) = with_native_frame_pipeline_session_mut(env, handle, |session| {
                    session.detach_presenter_surface();
                    android_native_frame_pipeline_status_json(
                        handle as u64,
                        session,
                        Some("presenter surface detached".to_owned()),
                    )
                    .map_err(|error| error.to_string())
                }) else {
                    return Ok(std::ptr::null_mut());
                };
                Ok(env.new_string(json)?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_flushNativeFramePipeline(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                let Some(json) = with_native_frame_pipeline_session_mut(env, handle, |session| {
                    session
                        .flush()
                        .map_err(|error| error.message().to_owned())?;
                    android_native_frame_pipeline_status_json(
                        handle as u64,
                        session,
                        Some("flushed".to_owned()),
                    )
                    .map_err(|error| error.to_string())
                }) else {
                    return Ok(std::ptr::null_mut());
                };
                Ok(env.new_string(json)?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_seekNativeFramePipeline(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
    position_ms: jlong,
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                let Some(json) = with_native_frame_pipeline_session_mut(env, handle, |session| {
                    session
                        .seek(Duration::from_millis(position_ms.max(0) as u64))
                        .map_err(|error| error.message().to_owned())?;
                    android_native_frame_pipeline_status_json(
                        handle as u64,
                        session,
                        Some("seeked".to_owned()),
                    )
                    .map_err(|error| error.to_string())
                }) else {
                    return Ok(std::ptr::null_mut());
                };
                Ok(env.new_string(json)?.into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_closeNativeFramePipeline(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    handle: jlong,
) {
    run_jni_entry(&mut unowned_env, |_unowned_env| {
        dispose_native_frame_pipeline_session(handle);
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

fn native_frame_pipeline_mode_from_wire_name(
    value: &str,
) -> Result<NativeFramePipelineMode, String> {
    match value {
        "disabled" => Ok(NativeFramePipelineMode::Disabled),
        "diagnosticsOnly" => Ok(NativeFramePipelineMode::DiagnosticsOnly),
        "preferNativeFrame" => Ok(NativeFramePipelineMode::PreferNativeFrame),
        "requireNativeFrame" => Ok(NativeFramePipelineMode::RequireNativeFrame),
        other => Err(format!(
            "unknown Android native-frame mode wire name `{other}`"
        )),
    }
}

fn native_frame_presenter_profile_from_wire_name(
    value: &str,
) -> Result<AndroidNativeFramePresenterProfile, String> {
    match value {
        "SurfaceView" => Ok(AndroidNativeFramePresenterProfile::SurfaceView),
        "Surface" => Ok(AndroidNativeFramePresenterProfile::Surface),
        "SurfaceTexture" => Ok(AndroidNativeFramePresenterProfile::SurfaceTexture),
        other => Err(format!(
            "unknown Android native-frame presenter profile wire name `{other}`"
        )),
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
                // Remove the owner while holding the registry lock, then drop
                // it after unlocking. Closing a pipeline event worker can
                // join a thread and must not block unrelated JNI sessions.
                let session = {
                    let mut guard = lock_or_recover(sessions());
                    guard.remove(session_handle)
                };
                drop(session);
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
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_sampleTimeline(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    position_ms: jlong,
    duration_ms: jlong,
    is_live: jboolean,
    is_seekable: jboolean,
    seekable_start_ms: jlong,
    seekable_end_ms: jlong,
    live_edge_ms: jlong,
) -> jobject {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jobject> {
                let snapshot = AndroidExoPlaybackSnapshot {
                    playback_state: player_platform_android::AndroidExoPlaybackState::Ready,
                    play_when_ready: false,
                    playback_rate: 1.0,
                    position: Duration::from_millis(position_ms.max(0) as u64),
                    duration: (duration_ms >= 0).then(|| Duration::from_millis(duration_ms as u64)),
                    is_live,
                    is_seekable,
                    seekable_range: (seekable_start_ms >= 0
                        && seekable_end_ms >= seekable_start_ms)
                        .then(|| AndroidExoSeekableRange {
                            start: Duration::from_millis(seekable_start_ms as u64),
                            end: Duration::from_millis(seekable_end_ms as u64),
                        }),
                    live_edge: (live_edge_ms >= 0)
                        .then(|| Duration::from_millis(live_edge_ms as u64)),
                };
                let Some(timeline) = with_session_mut(env, session_handle, |session| {
                    session.sample_timeline(&snapshot)
                }) else {
                    return Ok(JObject::null().into_raw());
                };
                Ok(timeline_object(env, &timeline)?.into_raw())
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
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_reportFirstFrame(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    presentation_time_ms: jlong,
    width: jint,
    height: jint,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<()> {
                let _ = with_session_mut(env, session_handle, |session| {
                    session.report_first_frame(
                        Duration::from_millis(presentation_time_ms.max(0) as u64),
                        width.max(0) as u32,
                        height.max(0) as u32,
                    );
                });
                Ok(())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_flushPipelineEventHooks(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    timeout_ms: jlong,
) -> jboolean {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jboolean> {
                let Some(flushed) = with_session_mut(env, session_handle, |session| {
                    session
                        .flush_pipeline_event_hooks(Duration::from_millis(timeout_ms.max(0) as u64))
                }) else {
                    return Ok(false as jboolean);
                };
                Ok(flushed as jboolean)
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_closePipelineEventHooks(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) -> jboolean {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jboolean> {
                let Some(closed) = with_session_mut(env, session_handle, |session| {
                    session.close_pipeline_event_hooks()
                }) else {
                    return Ok(false as jboolean);
                };
                Ok(closed as jboolean)
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

fn pipeline_event_hook_reports_json(batch: PipelineEventHookReportBatch) -> String {
    let reports = batch
        .reports
        .into_iter()
        .map(|report| {
            let result = match report.result {
                Ok(outcome) => serde_json::json!({
                    "status": if outcome.accepted { "accepted" } else { "rejected" },
                    "outcome": outcome,
                }),
                Err(error) => serde_json::json!({
                    "status": "error",
                    "error": error,
                }),
            };
            serde_json::json!({
                "pluginId": report.reference.plugin_id(),
                "capabilityInstanceId": report.reference.capability_instance_id(),
                "transport": report.reference.transport(),
                "runId": report.run_id,
                "sessionId": report.session_id,
                "eventName": report.event_name,
                "result": result,
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "reports": reports,
        "droppedEvents": batch.dropped_events,
        "droppedReports": batch.dropped_reports,
        "dispatcherError": batch.dispatcher_error,
    })
    .to_string()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_drainPipelineEventHookReports(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                let Some(batch) = with_session_mut(env, session_handle, |session| {
                    session.drain_pipeline_event_hook_reports()
                }) else {
                    return Ok(std::ptr::null_mut());
                };
                Ok(env
                    .new_string(pipeline_event_hook_reports_json(batch))?
                    .into_raw())
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
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
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                if selection.is_null() {
                    return Ok(std::ptr::null_mut());
                }

                let selection = parse_native_track_selection(env, selection)?;
                let request = selection.clone();
                let result = with_session_mut_checked(session_handle, |session| {
                    session.dispatch_command(PlayerRuntimeCommand::SetSubtitleTrackSelection {
                        selection,
                    })
                });
                match result {
                    Ok(Ok(_)) => Ok(std::ptr::null_mut()),
                    Ok(Err(error)) => {
                        let json = subtitle_command_error_json(&request, &error);
                        Ok(env.new_string(json)?.into_raw())
                    }
                    Err(message) => {
                        let error = stale_subtitle_session_error(&request, message);
                        let json = subtitle_command_error_json(&request, &error);
                        Ok(env.new_string(json)?.into_raw())
                    }
                }
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

fn stale_subtitle_session_error(
    selection: &MediaTrackSelection,
    message: &'static str,
) -> PlayerError {
    PlayerError::new(PlayerErrorCode::Cancelled, message).with_subtitle_details(
        SubtitleErrorDetails::new(
            "subtitle_selection_cancelled",
            "selection",
            selection.track_id.clone(),
            true,
            message,
        ),
    )
}

fn subtitle_command_error_json(selection: &MediaTrackSelection, error: &PlayerError) -> String {
    let details = error
        .subtitle_details()
        .cloned()
        .unwrap_or_else(|| fallback_subtitle_error_details(selection, error));
    let mut payload = match serde_json::to_value(&details) {
        Ok(serde_json::Value::Object(payload)) => payload,
        _ => serde_json::Map::new(),
    };
    payload.insert(
        "domain".to_owned(),
        serde_json::Value::String("subtitle".to_owned()),
    );
    if !payload.contains_key("trackId")
        && let Some(track_id) = selection.track_id.as_ref()
    {
        payload.insert(
            "trackId".to_owned(),
            serde_json::Value::String(track_id.clone()),
        );
    }
    serde_json::Value::Object(payload).to_string()
}

fn fallback_subtitle_error_details(
    selection: &MediaTrackSelection,
    error: &PlayerError,
) -> SubtitleErrorDetails {
    let (code, retriable) = match error.code() {
        PlayerErrorCode::Timeout => ("subtitle_selection_timeout", true),
        PlayerErrorCode::Cancelled
        | PlayerErrorCode::CommandChannelClosed
        | PlayerErrorCode::EventChannelClosed => ("subtitle_selection_cancelled", true),
        PlayerErrorCode::Unsupported | PlayerErrorCode::BackendFailure => {
            ("subtitle_platform_track_unavailable", error.is_retriable())
        }
        PlayerErrorCode::InvalidArgument
        | PlayerErrorCode::InvalidState
        | PlayerErrorCode::InvalidSource
        | PlayerErrorCode::AudioOutputUnavailable
        | PlayerErrorCode::DecodeFailure
        | PlayerErrorCode::SeekFailure => ("subtitle_selection_mismatch", error.is_retriable()),
    };
    SubtitleErrorDetails::new(
        code,
        "selection",
        selection.track_id.clone(),
        retriable,
        error.message(),
    )
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_setAbrPolicy(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    policy: JObject<'_>,
    expected_catalog_revision: JObject<'_>,
) -> jstring {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<jstring> {
                if policy.is_null() {
                    return Ok(std::ptr::null_mut());
                }

                let policy = parse_native_abr_policy(env, policy)?;
                let expected_catalog_revision = boxed_long_value(env, expected_catalog_revision)?;
                let request = policy.clone();
                let result = with_session_mut_checked(session_handle, |session| {
                    session.dispatch_command(PlayerRuntimeCommand::SetAbrPolicy {
                        policy,
                        expected_catalog_revision,
                    })
                });
                match result {
                    Ok(Ok(_)) => Ok(std::ptr::null_mut()),
                    Ok(Err(error)) => {
                        let json = abr_policy_command_error_json(&request, &error);
                        Ok(env.new_string(json)?.into_raw())
                    }
                    Err(message) => {
                        let error = stale_abr_policy_session_error(message);
                        let json = abr_policy_command_error_json(&request, &error);
                        Ok(env.new_string(json)?.into_raw())
                    }
                }
            })
            .resolve::<ThrowRuntimeExAndDefault>()
    })
}

fn stale_abr_policy_session_error(message: &'static str) -> PlayerError {
    PlayerError::new(PlayerErrorCode::Cancelled, message)
}

fn abr_policy_command_error_json(
    policy: &player_runtime::MediaAbrPolicy,
    error: &PlayerError,
) -> String {
    let fixed_track_details = error.fixed_track_selection_details();
    let mut payload = match fixed_track_details {
        Some(details) => serde_json::to_value(details)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default(),
        None => serde_json::Map::new(),
    };
    if fixed_track_details.is_some() {
        payload.insert(
            "domain".to_owned(),
            serde_json::Value::String("fixedTrack".to_owned()),
        );
        if let Some(track_id) = policy.track_id.as_ref() {
            payload
                .entry("trackId".to_owned())
                .or_insert_with(|| serde_json::Value::String(track_id.clone()));
        }
    } else {
        payload.insert(
            "domain".to_owned(),
            serde_json::Value::String("abrPolicy".to_owned()),
        );
        payload.insert(
            "code".to_owned(),
            serde_json::Value::String(player_error_code_wire_name(error.code()).to_owned()),
        );
        payload.insert(
            "category".to_owned(),
            serde_json::Value::String(player_error_category_wire_name(error.category()).to_owned()),
        );
        payload.insert(
            "retriable".to_owned(),
            serde_json::Value::Bool(error.is_retriable()),
        );
    }
    payload.insert(
        "operation".to_owned(),
        serde_json::Value::String("setAbrPolicy".to_owned()),
    );
    payload.insert(
        "message".to_owned(),
        serde_json::Value::String(error.message().to_owned()),
    );
    serde_json::Value::Object(payload).to_string()
}

fn player_error_code_wire_name(code: PlayerErrorCode) -> &'static str {
    match code {
        PlayerErrorCode::InvalidArgument => "invalidArgument",
        PlayerErrorCode::InvalidState => "invalidState",
        PlayerErrorCode::InvalidSource => "invalidSource",
        PlayerErrorCode::BackendFailure => "backendFailure",
        PlayerErrorCode::AudioOutputUnavailable => "audioOutputUnavailable",
        PlayerErrorCode::DecodeFailure => "decodeFailure",
        PlayerErrorCode::SeekFailure => "seekFailure",
        PlayerErrorCode::Unsupported => "unsupported",
        PlayerErrorCode::CommandChannelClosed => "commandChannelClosed",
        PlayerErrorCode::EventChannelClosed => "eventChannelClosed",
        PlayerErrorCode::Cancelled => "cancelled",
        PlayerErrorCode::Timeout => "timeout",
    }
}

fn player_error_category_wire_name(category: PlayerErrorCategory) -> &'static str {
    match category {
        PlayerErrorCategory::Input => "input",
        PlayerErrorCategory::Source => "source",
        PlayerErrorCategory::Network => "network",
        PlayerErrorCategory::Decode => "decode",
        PlayerErrorCategory::AudioOutput => "audioOutput",
        PlayerErrorCategory::Playback => "playback",
        PlayerErrorCategory::Capability => "capability",
        PlayerErrorCategory::Platform => "platform",
    }
}

#[cfg(test)]
mod tests {
    use super::handles::next_generation;
    use super::{
        HandleRegistry, abr_policy_command_error_json, error_category_from_jni_ordinal,
        error_code_from_jni_ordinal, resolve_resilience_policy_with_runtime,
        resolve_track_preferences_with_runtime, stale_abr_policy_session_error,
        stale_subtitle_session_error, subtitle_command_error_json, u64_to_jlong_saturating,
        u128_to_jlong_saturating, with_session_mut_checked,
    };
    use player_runtime::{
        FixedTrackSelectionErrorDetails, MediaAbrMode, MediaAbrPolicy, MediaSourceKind,
        MediaSourceProtocol, MediaTrackSelection, PlayerBufferingPolicy, PlayerBufferingPreset,
        PlayerCachePolicy, PlayerCachePreset, PlayerError, PlayerErrorCategory, PlayerErrorCode,
        PlayerRetryBackoff, PlayerRetryPolicy, PlayerTrackPreferencePolicy, SubtitleErrorDetails,
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
    fn subtitle_json_preserves_typed_details_instead_of_selection_mode() {
        let selection = MediaTrackSelection::track("opaque-track");
        let error = PlayerError::new(PlayerErrorCode::Timeout, "generic timeout")
            .with_subtitle_details(
                SubtitleErrorDetails::new(
                    "subtitle_auto_candidate_unavailable",
                    "discovery",
                    Some("opaque-track".to_owned()),
                    true,
                    "no automatic subtitle candidate",
                )
                .with_transaction(Some(42), Some(9)),
            );
        let payload: serde_json::Value =
            serde_json::from_str(&subtitle_command_error_json(&selection, &error))
                .expect("subtitle error JSON");
        assert_eq!(payload["domain"], "subtitle");
        assert_eq!(payload["code"], "subtitle_auto_candidate_unavailable");
        assert_eq!(payload["phase"], "discovery");
        assert_eq!(payload["commandId"], 42);
        assert_eq!(payload["sourceEpoch"], 9);
    }

    #[test]
    fn subtitle_json_fallback_maps_runtime_code_without_request_mode() {
        let selection = MediaTrackSelection::auto();
        let error = PlayerError::new(PlayerErrorCode::Timeout, "timeout");
        let payload: serde_json::Value =
            serde_json::from_str(&subtitle_command_error_json(&selection, &error))
                .expect("subtitle error JSON");
        assert_eq!(payload["code"], "subtitle_selection_timeout");
        assert_eq!(payload["phase"], "selection");
    }

    #[test]
    fn abr_policy_json_keeps_non_fixed_command_errors_out_of_fixed_track_domain() {
        let policy = MediaAbrPolicy {
            mode: MediaAbrMode::Constrained,
            track_id: None,
            max_bit_rate: None,
            max_width: None,
            max_height: None,
        };
        let error = PlayerError::new(
            PlayerErrorCode::InvalidArgument,
            "constrained ABR requires at least one bitrate or size constraint",
        );
        let payload: serde_json::Value =
            serde_json::from_str(&abr_policy_command_error_json(&policy, &error))
                .expect("ABR policy error JSON");

        assert_eq!(payload["domain"], "abrPolicy");
        assert_eq!(payload["code"], "invalidArgument");
        assert_eq!(payload["category"], "input");
        assert_eq!(payload["retriable"], false);
        assert_eq!(payload["operation"], "setAbrPolicy");
        assert!(payload.get("trackId").is_none());
    }

    #[test]
    fn abr_policy_json_keeps_fixed_track_rejection_details_typed() {
        let policy = MediaAbrPolicy {
            mode: MediaAbrMode::FixedTrack,
            track_id: Some("video:4k".to_owned()),
            max_bit_rate: None,
            max_width: None,
            max_height: None,
        };
        let error = PlayerError::new(PlayerErrorCode::Unsupported, "track exceeds capabilities")
            .with_fixed_track_selection_details(FixedTrackSelectionErrorDetails::new(
                "trackExceedsCapabilities",
                Some("video:4k".to_owned()),
                Some(4),
                Some(5),
                "track exceeds capabilities",
            ));
        let payload: serde_json::Value =
            serde_json::from_str(&abr_policy_command_error_json(&policy, &error))
                .expect("fixed-track error JSON");

        assert_eq!(payload["domain"], "fixedTrack");
        assert_eq!(payload["code"], "trackExceedsCapabilities");
        assert_eq!(payload["trackId"], "video:4k");
        assert_eq!(payload["expectedCatalogRevision"], 4);
        assert_eq!(payload["actualCatalogRevision"], 5);
        assert_eq!(payload["operation"], "setAbrPolicy");
    }

    #[test]
    fn stale_abr_policy_session_error_remains_a_generic_command_failure() {
        let policy = MediaAbrPolicy::default();
        let error = stale_abr_policy_session_error("invalid android JNI session handle");
        let payload: serde_json::Value =
            serde_json::from_str(&abr_policy_command_error_json(&policy, &error))
                .expect("stale-session ABR error JSON");

        assert!(error.fixed_track_selection_details().is_none());
        assert_eq!(payload["domain"], "abrPolicy");
        assert_eq!(payload["code"], "cancelled");
        assert_eq!(payload["category"], "playback");
    }

    #[test]
    fn stale_session_lookup_is_checked_without_java_exception() {
        let handle = super::sessions::new_session_with_plugin_registry(
            "stale-test".to_owned(),
            None,
            Vec::new(),
        )
        .expect("session");
        {
            let mut guard = super::lock_or_recover(super::sessions::sessions());
            assert!(guard.remove(handle).is_some());
        }
        let result = with_session_mut_checked(handle, |_| ());
        assert_eq!(result, Err("invalid android JNI session handle"));

        let selection = MediaTrackSelection::track("stale-track");
        let error = stale_subtitle_session_error(&selection, "invalid android JNI session handle");
        let payload: serde_json::Value =
            serde_json::from_str(&subtitle_command_error_json(&selection, &error))
                .expect("subtitle error JSON");
        assert_eq!(payload["code"], "subtitle_selection_cancelled");
        assert_eq!(payload["trackId"], "stale-track");
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
