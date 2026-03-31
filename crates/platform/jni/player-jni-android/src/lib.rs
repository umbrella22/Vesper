use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use jni::errors::{Result as JniResult, ThrowRuntimeExAndDefault};
use jni::objects::{JClass, JObject, JObjectArray, JString, JValue};
use jni::signature::{RuntimeFieldSignature, RuntimeMethodSignature};
use jni::strings::JNIString;
use jni::sys::{jboolean, jfloat, jint, jlong, jobject, jobjectArray};
use jni::{Env, EnvUnowned};
use player_platform_android::{
    AndroidExoPlaybackSnapshot, AndroidExoPlaybackState, AndroidHostBridgeSession,
    AndroidHostCommand, AndroidHostEvent, AndroidHostSnapshot, AndroidHostTimelineKind,
};
use player_runtime::{
    MediaAbrMode, MediaAbrPolicy, MediaTrack, MediaTrackCatalog, MediaTrackKind,
    MediaTrackSelection, MediaTrackSelectionMode, MediaTrackSelectionSnapshot,
    PlayerRuntimeCommand, PlayerRuntimeError, PlayerRuntimeErrorCategory, PlayerRuntimeErrorCode,
    PresentationState,
};

const PKG: &str = "io/github/ikaros/vesper/player/android";

struct AndroidJniSession {
    inner: AndroidHostBridgeSession,
}

static NEXT_SESSION_HANDLE: AtomicI64 = AtomicI64::new(1);
static SESSIONS: OnceLock<Mutex<HashMap<i64, AndroidJniSession>>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashMap<i64, AndroidJniSession>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn invalid_handle_error() -> &'static str {
    "invalid android JNI session handle"
}

fn jni_name(value: impl AsRef<str>) -> JNIString {
    JNIString::from(value.as_ref())
}

fn method_sig(value: &str) -> RuntimeMethodSignature {
    RuntimeMethodSignature::from_str(value).expect("static JNI method signature should parse")
}

fn field_sig(value: impl AsRef<str>) -> RuntimeFieldSignature {
    RuntimeFieldSignature::from_str(value.as_ref())
        .expect("static JNI field signature should parse")
}

fn with_session_mut<R>(
    env: &mut Env<'_>,
    handle: jlong,
    f: impl FnOnce(&mut AndroidJniSession) -> R,
) -> Option<R> {
    let Ok(mut guard) = sessions().lock() else {
        let _ = env.throw_new(
            jni_name("java/lang/IllegalStateException"),
            jni_name("failed to lock session registry"),
        );
        return None;
    };
    let Some(session) = guard.get_mut(&handle) else {
        let _ = env.throw_new(
            jni_name("java/lang/IllegalArgumentException"),
            jni_name(invalid_handle_error()),
        );
        return None;
    };
    Some(f(session))
}

fn new_session(source_uri: String) -> Result<jlong, &'static str> {
    let handle = NEXT_SESSION_HANDLE.fetch_add(1, Ordering::Relaxed);
    let session = AndroidJniSession {
        inner: AndroidHostBridgeSession::new(source_uri),
    };
    let Ok(mut guard) = sessions().lock() else {
        return Err("failed to lock session registry");
    };
    guard.insert(handle, session);
    Ok(handle)
}

fn boxed_long<'local>(env: &mut Env<'local>, value: Option<u64>) -> JniResult<JObject<'local>> {
    match value {
        Some(value) => env
            .call_static_method(
                jni_name("java/lang/Long"),
                jni_name("valueOf"),
                method_sig("(J)Ljava/lang/Long;").method_signature(),
                &[JValue::Long(value as i64)],
            )?
            .l(),
        None => Ok(JObject::null()),
    }
}

fn playback_state_object<'local>(
    env: &mut Env<'local>,
    state: PresentationState,
) -> JniResult<JObject<'local>> {
    let class = env.find_class(jni_name(format!("{PKG}/PlaybackStateUi")))?;
    let field = match state {
        PresentationState::Ready => "Ready",
        PresentationState::Playing => "Playing",
        PresentationState::Paused => "Paused",
        PresentationState::Finished => "Finished",
    };
    env.get_static_field(
        class,
        jni_name(field),
        field_sig(format!("L{PKG}/PlaybackStateUi;")).field_signature(),
    )?
    .l()
}

fn timeline_kind_object<'local>(
    env: &mut Env<'local>,
    kind: AndroidHostTimelineKind,
) -> JniResult<JObject<'local>> {
    let class = env.find_class(jni_name(format!("{PKG}/TimelineKind")))?;
    let field = match kind {
        AndroidHostTimelineKind::Vod => "Vod",
        AndroidHostTimelineKind::Live => "Live",
        AndroidHostTimelineKind::LiveDvr => "LiveDvr",
    };
    env.get_static_field(
        class,
        jni_name(field),
        field_sig(format!("L{PKG}/TimelineKind;")).field_signature(),
    )?
    .l()
}

fn host_snapshot_object<'local>(
    env: &mut Env<'local>,
    snapshot: &AndroidHostSnapshot,
) -> JniResult<JObject<'local>> {
    let seekable_range = match snapshot.seekable_range {
        Some(range) => {
            let class = env.find_class(jni_name(format!("{PKG}/SeekableRangeUi")))?;
            env.new_object(
                class,
                method_sig("(JJ)V").method_signature(),
                &[
                    JValue::Long(range.start_ms as i64),
                    JValue::Long(range.end_ms as i64),
                ],
            )?
        }
        None => JObject::null(),
    };

    let timeline_kind = timeline_kind_object(env, snapshot.timeline_kind)?;
    let live_edge = boxed_long(env, snapshot.live_edge_ms)?;
    let duration = boxed_long(env, snapshot.duration_ms)?;
    let timeline_class = env.find_class(jni_name(format!("{PKG}/TimelineUiState")))?;
    let timeline = env.new_object(
        timeline_class,
        method_sig(&format!(
            "(L{PKG}/TimelineKind;ZL{PKG}/SeekableRangeUi;Ljava/lang/Long;JLjava/lang/Long;)V"
        ))
        .method_signature(),
        &[
            JValue::Object(&timeline_kind),
            JValue::Bool(snapshot.is_seekable),
            JValue::Object(&seekable_range),
            JValue::Object(&live_edge),
            JValue::Long(snapshot.position_ms as i64),
            JValue::Object(&duration),
        ],
    )?;

    let playback_state = playback_state_object(env, snapshot.playback_state)?;
    let snapshot_class = env.find_class(jni_name(format!("{PKG}/NativeBridgeSnapshot")))?;
    env.new_object(
        snapshot_class,
        method_sig(&format!(
            "(L{PKG}/PlaybackStateUi;FZZL{PKG}/TimelineUiState;)V"
        ))
        .method_signature(),
        &[
            JValue::Object(&playback_state),
            JValue::Float(snapshot.playback_rate),
            JValue::Bool(snapshot.is_buffering),
            JValue::Bool(snapshot.is_interrupted),
            JValue::Object(&timeline),
        ],
    )
}

fn host_event_object<'local>(
    env: &mut Env<'local>,
    event: &AndroidHostEvent,
) -> JniResult<JObject<'local>> {
    match event {
        AndroidHostEvent::PlaybackStateChanged { state } => {
            let class = env.find_class(jni_name(format!(
                "{PKG}/NativeBridgeEvent$PlaybackStateChanged"
            )))?;
            let state = playback_state_object(env, *state)?;
            env.new_object(
                class,
                method_sig(&format!("(L{PKG}/PlaybackStateUi;)V")).method_signature(),
                &[JValue::Object(&state)],
            )
        }
        AndroidHostEvent::PlaybackRateChanged { rate } => {
            let class = env.find_class(jni_name(format!(
                "{PKG}/NativeBridgeEvent$PlaybackRateChanged"
            )))?;
            env.new_object(
                class,
                method_sig("(F)V").method_signature(),
                &[JValue::Float(*rate)],
            )
        }
        AndroidHostEvent::BufferingChanged { buffering } => {
            let class = env.find_class(jni_name(format!(
                "{PKG}/NativeBridgeEvent$BufferingChanged"
            )))?;
            env.new_object(
                class,
                method_sig("(Z)V").method_signature(),
                &[JValue::Bool(*buffering)],
            )
        }
        AndroidHostEvent::InterruptionChanged { interrupted } => {
            let class = env.find_class(jni_name(format!(
                "{PKG}/NativeBridgeEvent$InterruptionChanged"
            )))?;
            env.new_object(
                class,
                method_sig("(Z)V").method_signature(),
                &[JValue::Bool(*interrupted)],
            )
        }
        AndroidHostEvent::VideoSurfaceChanged { attached } => {
            let class = env.find_class(jni_name(format!(
                "{PKG}/NativeBridgeEvent$VideoSurfaceChanged"
            )))?;
            env.new_object(
                class,
                method_sig("(Z)V").method_signature(),
                &[JValue::Bool(*attached)],
            )
        }
        AndroidHostEvent::SeekCompleted { position_ms } => {
            let class =
                env.find_class(jni_name(format!("{PKG}/NativeBridgeEvent$SeekCompleted")))?;
            env.new_object(
                class,
                method_sig("(J)V").method_signature(),
                &[JValue::Long(*position_ms as i64)],
            )
        }
        AndroidHostEvent::RetryScheduled { attempt, delay_ms } => {
            let class =
                env.find_class(jni_name(format!("{PKG}/NativeBridgeEvent$RetryScheduled")))?;
            env.new_object(
                class,
                method_sig("(IJ)V").method_signature(),
                &[
                    JValue::Int(*attempt as jint),
                    JValue::Long(*delay_ms as i64),
                ],
            )
        }
        AndroidHostEvent::Ended => {
            let class = env.find_class(jni_name(format!("{PKG}/NativeBridgeEvent$Ended")))?;
            env.new_object(
                class,
                method_sig("(Z)V").method_signature(),
                &[JValue::Bool(true)],
            )
        }
        AndroidHostEvent::Error {
            code,
            category,
            retriable,
            message,
        } => {
            let class = env.find_class(jni_name(format!("{PKG}/NativeBridgeEvent$Error")))?;
            let message = env.new_string(format!("[{code:?}] {message}"))?;
            let message_object = JObject::from(message);
            env.new_object(
                class,
                method_sig("(Ljava/lang/String;IIZ)V").method_signature(),
                &[
                    JValue::Object(&message_object),
                    JValue::Int(*code as jint),
                    JValue::Int(*category as jint),
                    JValue::Bool(*retriable),
                ],
            )
        }
    }
}

fn data_object_instance<'local>(
    env: &mut Env<'local>,
    internal_name: &str,
) -> JniResult<JObject<'local>> {
    let class = env.find_class(jni_name(internal_name))?;
    env.get_static_field(
        class,
        jni_name("INSTANCE"),
        field_sig(format!("L{internal_name};")).field_signature(),
    )?
    .l()
}

fn track_selection_payload_object<'local>(
    env: &mut Env<'local>,
    selection: &MediaTrackSelection,
) -> JniResult<JObject<'local>> {
    let class = env.find_class(jni_name(format!("{PKG}/NativeTrackSelectionPayload")))?;
    let track_id = match selection.track_id.as_deref() {
        Some(track_id) => JObject::from(env.new_string(track_id)?),
        None => JObject::null(),
    };
    env.new_object(
        class,
        method_sig("(ILjava/lang/String;)V").method_signature(),
        &[
            JValue::Int(match selection.mode {
                MediaTrackSelectionMode::Auto => 0,
                MediaTrackSelectionMode::Disabled => 1,
                MediaTrackSelectionMode::Track => 2,
            }),
            JValue::Object(&track_id),
        ],
    )
}

fn abr_policy_payload_object<'local>(
    env: &mut Env<'local>,
    policy: &MediaAbrPolicy,
) -> JniResult<JObject<'local>> {
    let class = env.find_class(jni_name(format!("{PKG}/NativeAbrPolicyPayload")))?;
    let track_id = match policy.track_id.as_deref() {
        Some(track_id) => JObject::from(env.new_string(track_id)?),
        None => JObject::null(),
    };
    let max_bit_rate = policy.max_bit_rate.unwrap_or_default();
    let max_width = policy.max_width.unwrap_or_default();
    let max_height = policy.max_height.unwrap_or_default();
    env.new_object(
        class,
        method_sig("(ILjava/lang/String;ZJZIZI)V").method_signature(),
        &[
            JValue::Int(match policy.mode {
                MediaAbrMode::Auto => 0,
                MediaAbrMode::Constrained => 1,
                MediaAbrMode::FixedTrack => 2,
            }),
            JValue::Object(&track_id),
            JValue::Bool(policy.max_bit_rate.is_some()),
            JValue::Long(max_bit_rate.min(i64::MAX as u64) as i64),
            JValue::Bool(policy.max_width.is_some()),
            JValue::Int(max_width.min(i32::MAX as u32) as i32),
            JValue::Bool(policy.max_height.is_some()),
            JValue::Int(max_height.min(i32::MAX as u32) as i32),
        ],
    )
}

fn native_command_object<'local>(
    env: &mut Env<'local>,
    command: &AndroidHostCommand,
) -> JniResult<JObject<'local>> {
    match command {
        AndroidHostCommand::Play => {
            data_object_instance(env, &format!("{PKG}/NativePlayerCommand$Play"))
        }
        AndroidHostCommand::Pause => {
            data_object_instance(env, &format!("{PKG}/NativePlayerCommand$Pause"))
        }
        AndroidHostCommand::SeekTo { position_ms } => {
            let class = env.find_class(jni_name(format!("{PKG}/NativePlayerCommand$SeekTo")))?;
            env.new_object(
                class,
                method_sig("(J)V").method_signature(),
                &[JValue::Long(*position_ms as i64)],
            )
        }
        AndroidHostCommand::Stop => {
            data_object_instance(env, &format!("{PKG}/NativePlayerCommand$Stop"))
        }
        AndroidHostCommand::SetPlaybackRate { rate } => {
            let class = env.find_class(jni_name(format!(
                "{PKG}/NativePlayerCommand$SetPlaybackRate"
            )))?;
            env.new_object(
                class,
                method_sig("(F)V").method_signature(),
                &[JValue::Float(*rate)],
            )
        }
        AndroidHostCommand::SetVideoTrackSelection { selection } => {
            let class = env.find_class(jni_name(format!(
                "{PKG}/NativePlayerCommand$SetVideoTrackSelection"
            )))?;
            let selection = track_selection_payload_object(env, selection)?;
            env.new_object(
                class,
                method_sig(&format!("(L{PKG}/NativeTrackSelectionPayload;)V")).method_signature(),
                &[JValue::Object(&selection)],
            )
        }
        AndroidHostCommand::SetAudioTrackSelection { selection } => {
            let class = env.find_class(jni_name(format!(
                "{PKG}/NativePlayerCommand$SetAudioTrackSelection"
            )))?;
            let selection = track_selection_payload_object(env, selection)?;
            env.new_object(
                class,
                method_sig(&format!("(L{PKG}/NativeTrackSelectionPayload;)V")).method_signature(),
                &[JValue::Object(&selection)],
            )
        }
        AndroidHostCommand::SetSubtitleTrackSelection { selection } => {
            let class = env.find_class(jni_name(format!(
                "{PKG}/NativePlayerCommand$SetSubtitleTrackSelection"
            )))?;
            let selection = track_selection_payload_object(env, selection)?;
            env.new_object(
                class,
                method_sig(&format!("(L{PKG}/NativeTrackSelectionPayload;)V")).method_signature(),
                &[JValue::Object(&selection)],
            )
        }
        AndroidHostCommand::SetAbrPolicy { policy } => {
            let class =
                env.find_class(jni_name(format!("{PKG}/NativePlayerCommand$SetAbrPolicy")))?;
            let policy = abr_policy_payload_object(env, policy)?;
            env.new_object(
                class,
                method_sig(&format!("(L{PKG}/NativeAbrPolicyPayload;)V")).method_signature(),
                &[JValue::Object(&policy)],
            )
        }
    }
}

fn error_code_from_ordinal(ordinal: jint) -> PlayerRuntimeErrorCode {
    match ordinal {
        0 => PlayerRuntimeErrorCode::InvalidArgument,
        1 => PlayerRuntimeErrorCode::InvalidState,
        2 => PlayerRuntimeErrorCode::InvalidSource,
        3 => PlayerRuntimeErrorCode::BackendFailure,
        4 => PlayerRuntimeErrorCode::AudioOutputUnavailable,
        5 => PlayerRuntimeErrorCode::DecodeFailure,
        6 => PlayerRuntimeErrorCode::SeekFailure,
        7 => PlayerRuntimeErrorCode::Unsupported,
        _ => PlayerRuntimeErrorCode::BackendFailure,
    }
}

fn error_category_from_ordinal(ordinal: jint) -> PlayerRuntimeErrorCategory {
    match ordinal {
        0 => PlayerRuntimeErrorCategory::Input,
        1 => PlayerRuntimeErrorCategory::Source,
        2 => PlayerRuntimeErrorCategory::Network,
        3 => PlayerRuntimeErrorCategory::Decode,
        4 => PlayerRuntimeErrorCategory::AudioOutput,
        5 => PlayerRuntimeErrorCategory::Playback,
        6 => PlayerRuntimeErrorCategory::Capability,
        7 => PlayerRuntimeErrorCategory::Platform,
        _ => PlayerRuntimeErrorCategory::Platform,
    }
}

fn exo_state_from_ordinal(ordinal: jint) -> AndroidExoPlaybackState {
    match ordinal {
        1 => AndroidExoPlaybackState::Buffering,
        2 => AndroidExoPlaybackState::Ready,
        3 => AndroidExoPlaybackState::Ended,
        _ => AndroidExoPlaybackState::Idle,
    }
}

fn track_kind_from_ordinal(ordinal: jint) -> MediaTrackKind {
    match ordinal {
        1 => MediaTrackKind::Audio,
        2 => MediaTrackKind::Subtitle,
        _ => MediaTrackKind::Video,
    }
}

fn track_selection_mode_from_ordinal(ordinal: jint) -> MediaTrackSelectionMode {
    match ordinal {
        1 => MediaTrackSelectionMode::Disabled,
        2 => MediaTrackSelectionMode::Track,
        _ => MediaTrackSelectionMode::Auto,
    }
}

fn abr_mode_from_ordinal(ordinal: jint) -> MediaAbrMode {
    match ordinal {
        1 => MediaAbrMode::Constrained,
        2 => MediaAbrMode::FixedTrack,
        _ => MediaAbrMode::Auto,
    }
}

fn string_from_java_object(env: &mut Env<'_>, object: JObject<'_>) -> JniResult<Option<String>> {
    if object.is_null() {
        return Ok(None);
    }

    let value = unsafe { JString::from_raw(env, object.into_raw() as jni::sys::jstring) };
    Ok(Some(value.try_to_string(env)?))
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
    string_from_java_object(env, value)
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

fn float_field(env: &mut Env<'_>, object: &JObject<'_>, field_name: &str) -> JniResult<jfloat> {
    env.get_field(
        object,
        jni_name(field_name),
        field_sig("F").field_signature(),
    )?
    .f()
}

fn parse_native_track(env: &mut Env<'_>, track: JObject<'_>) -> JniResult<MediaTrack> {
    let has_bit_rate = bool_field(env, &track, "hasBitRate")?;
    let has_width = bool_field(env, &track, "hasWidth")?;
    let has_height = bool_field(env, &track, "hasHeight")?;
    let has_frame_rate = bool_field(env, &track, "hasFrameRate")?;
    let has_channels = bool_field(env, &track, "hasChannels")?;
    let has_sample_rate = bool_field(env, &track, "hasSampleRate")?;

    Ok(MediaTrack {
        id: string_field(env, &track, "id")?.unwrap_or_default(),
        kind: track_kind_from_ordinal(int_field(env, &track, "kindOrdinal")?),
        label: string_field(env, &track, "label")?,
        language: string_field(env, &track, "language")?,
        codec: string_field(env, &track, "codec")?,
        bit_rate: has_bit_rate.then_some(long_field(env, &track, "bitRate")? as u64),
        width: has_width.then_some(int_field(env, &track, "width")? as u32),
        height: has_height.then_some(int_field(env, &track, "height")? as u32),
        frame_rate: has_frame_rate.then_some(float_field(env, &track, "frameRate")? as f64),
        channels: has_channels.then_some(int_field(env, &track, "channels")? as u16),
        sample_rate: has_sample_rate.then_some(int_field(env, &track, "sampleRate")? as u32),
        is_default: bool_field(env, &track, "isDefault")?,
        is_forced: bool_field(env, &track, "isForced")?,
    })
}

fn parse_native_track_catalog(
    env: &mut Env<'_>,
    track_catalog: JObject<'_>,
) -> JniResult<MediaTrackCatalog> {
    let tracks_object = env
        .get_field(
            &track_catalog,
            jni_name("tracks"),
            field_sig(format!("[L{PKG}/NativeTrackInfo;")).field_signature(),
        )?
        .l()?;

    let mut tracks = Vec::new();
    if !tracks_object.is_null() {
        let tracks_array = unsafe {
            JObjectArray::<JObject<'_>>::from_raw(env, tracks_object.into_raw() as jobjectArray)
        };
        let len = tracks_array.len(env)?;
        for index in 0..len {
            let track = tracks_array.get_element(env, index)?;
            if !track.is_null() {
                tracks.push(parse_native_track(env, track)?);
            }
        }
    }

    Ok(MediaTrackCatalog {
        tracks,
        adaptive_video: bool_field(env, &track_catalog, "adaptiveVideo")?,
        adaptive_audio: bool_field(env, &track_catalog, "adaptiveAudio")?,
    })
}

fn parse_native_track_selection(
    env: &mut Env<'_>,
    selection: JObject<'_>,
) -> JniResult<MediaTrackSelection> {
    Ok(MediaTrackSelection {
        mode: track_selection_mode_from_ordinal(int_field(env, &selection, "modeOrdinal")?),
        track_id: string_field(env, &selection, "trackId")?,
    })
}

fn parse_native_abr_policy(
    env: &mut Env<'_>,
    abr_policy: JObject<'_>,
) -> JniResult<MediaAbrPolicy> {
    let has_max_bit_rate = bool_field(env, &abr_policy, "hasMaxBitRate")?;
    let has_max_width = bool_field(env, &abr_policy, "hasMaxWidth")?;
    let has_max_height = bool_field(env, &abr_policy, "hasMaxHeight")?;

    Ok(MediaAbrPolicy {
        mode: abr_mode_from_ordinal(int_field(env, &abr_policy, "modeOrdinal")?),
        track_id: string_field(env, &abr_policy, "trackId")?,
        max_bit_rate: has_max_bit_rate
            .then_some(long_field(env, &abr_policy, "maxBitRate")? as u64),
        max_width: has_max_width.then_some(int_field(env, &abr_policy, "maxWidth")? as u32),
        max_height: has_max_height.then_some(int_field(env, &abr_policy, "maxHeight")? as u32),
    })
}

fn parse_native_track_selection_snapshot(
    env: &mut Env<'_>,
    snapshot: JObject<'_>,
) -> JniResult<MediaTrackSelectionSnapshot> {
    let video = env
        .get_field(
            &snapshot,
            jni_name("video"),
            field_sig(format!("L{PKG}/NativeTrackSelectionPayload;")).field_signature(),
        )?
        .l()?;
    let audio = env
        .get_field(
            &snapshot,
            jni_name("audio"),
            field_sig(format!("L{PKG}/NativeTrackSelectionPayload;")).field_signature(),
        )?
        .l()?;
    let subtitle = env
        .get_field(
            &snapshot,
            jni_name("subtitle"),
            field_sig(format!("L{PKG}/NativeTrackSelectionPayload;")).field_signature(),
        )?
        .l()?;
    let abr_policy = env
        .get_field(
            &snapshot,
            jni_name("abrPolicy"),
            field_sig(format!("L{PKG}/NativeAbrPolicyPayload;")).field_signature(),
        )?
        .l()?;

    Ok(MediaTrackSelectionSnapshot {
        video: parse_native_track_selection(env, video)?,
        audio: parse_native_track_selection(env, audio)?,
        subtitle: parse_native_track_selection(env, subtitle)?,
        abr_policy: parse_native_abr_policy(env, abr_policy)?,
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_createSession(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    source_uri: JString<'_>,
) -> jlong {
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
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_disposeSession(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) {
    unowned_env
        .with_env(|_env| -> JniResult<()> {
            if let Ok(mut guard) = sessions().lock() {
                guard.remove(&session_handle);
            }
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_attachSurface(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    _surface: JObject<'_>,
    _surface_kind_ordinal: jint,
) {
    unowned_env
        .with_env(|env| -> JniResult<()> {
            let _ = with_session_mut(env, session_handle, |session| {
                session.inner.set_surface_attached(true);
            });
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_detachSurface(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) {
    unowned_env
        .with_env(|env| -> JniResult<()> {
            let _ = with_session_mut(env, session_handle, |session| {
                session.inner.set_surface_attached(false);
            });
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_pollSnapshot(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) -> jobject {
    unowned_env
        .with_env(|env| -> JniResult<jobject> {
            let Some(snapshot) =
                with_session_mut(env, session_handle, |session| session.inner.snapshot())
            else {
                return Ok(JObject::null().into_raw());
            };
            Ok(host_snapshot_object(env, &snapshot)?.into_raw())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_drainEvents(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) -> jobjectArray {
    unowned_env
        .with_env(|env| -> JniResult<jobjectArray> {
            let Some(events) =
                with_session_mut(env, session_handle, |session| session.inner.drain_events())
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
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_drainNativeCommands(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) -> jobjectArray {
    unowned_env
        .with_env(|env| -> JniResult<jobjectArray> {
            let Some(commands) = with_session_mut(env, session_handle, |session| {
                session.inner.drain_native_commands()
            }) else {
                return Ok(std::ptr::null_mut());
            };

            let command_class = env.find_class(jni_name(format!("{PKG}/NativePlayerCommand")))?;
            let array: JObjectArray<'_> =
                env.new_object_array(commands.len() as i32, command_class, JObject::null())?;
            for (index, command) in commands.iter().enumerate() {
                let object = native_command_object(env, command)?;
                array.set_element(env, index, object)?;
            }
            Ok(array.into_raw())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
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
                seekable_range: if seekable_start_ms >= 0 && seekable_end_ms >= seekable_start_ms {
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
                session.inner.apply_exo_snapshot(snapshot);
            });
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_applyTrackState(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    track_catalog: JObject<'_>,
    track_selection: JObject<'_>,
) {
    unowned_env
        .with_env(|env| -> JniResult<()> {
            if track_catalog.is_null() || track_selection.is_null() {
                return Ok(());
            }

            let track_catalog = parse_native_track_catalog(env, track_catalog)?;
            let track_selection = parse_native_track_selection_snapshot(env, track_selection)?;

            let _ = with_session_mut(env, session_handle, |session| {
                session
                    .inner
                    .report_media_info(track_catalog, track_selection);
            });
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_reportSeekCompleted(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    position_ms: jlong,
) {
    unowned_env
        .with_env(|env| -> JniResult<()> {
            let _ = with_session_mut(env, session_handle, |session| {
                session
                    .inner
                    .report_seek_completed(Duration::from_millis(position_ms.max(0) as u64));
            });
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_reportRetryScheduled(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    attempt: jint,
    delay_ms: jlong,
) {
    unowned_env
        .with_env(|env| -> JniResult<()> {
            let _ = with_session_mut(env, session_handle, |session| {
                session.inner.report_retry_scheduled(
                    attempt.max(0) as u32,
                    Duration::from_millis(delay_ms.max(0) as u64),
                );
            });
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_reportError(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    code_ordinal: jint,
    category_ordinal: jint,
    retriable: jboolean,
    message: JString<'_>,
) {
    unowned_env
        .with_env(|env| -> JniResult<()> {
            let message = message.try_to_string(env)?;
            let code = error_code_from_ordinal(code_ordinal);
            let category = error_category_from_ordinal(category_ordinal);
            let _ = with_session_mut(env, session_handle, |session| {
                session
                    .inner
                    .report_runtime_error(PlayerRuntimeError::with_taxonomy(
                        code, category, retriable, message,
                    ));
            });
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_play(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) {
    unowned_env
        .with_env(|env| -> JniResult<()> {
            let _ = with_session_mut(env, session_handle, |session| {
                let _ = session.inner.dispatch_command(PlayerRuntimeCommand::Play);
            });
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_pause(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) {
    unowned_env
        .with_env(|env| -> JniResult<()> {
            let _ = with_session_mut(env, session_handle, |session| {
                let _ = session.inner.dispatch_command(PlayerRuntimeCommand::Pause);
            });
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_stop(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
) {
    unowned_env
        .with_env(|env| -> JniResult<()> {
            let _ = with_session_mut(env, session_handle, |session| {
                let _ = session.inner.dispatch_command(PlayerRuntimeCommand::Stop);
            });
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_seekTo(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    position_ms: jlong,
) {
    unowned_env
        .with_env(|env| -> JniResult<()> {
            let _ = with_session_mut(env, session_handle, |session| {
                let _ = session
                    .inner
                    .dispatch_command(PlayerRuntimeCommand::SeekTo {
                        position: Duration::from_millis(position_ms.max(0) as u64),
                    });
            });
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_setPlaybackRate(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    rate: jfloat,
) {
    unowned_env
        .with_env(|env| -> JniResult<()> {
            let _ = with_session_mut(env, session_handle, |session| {
                let _ = session
                    .inner
                    .dispatch_command(PlayerRuntimeCommand::SetPlaybackRate { rate });
            });
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_setVideoTrackSelection(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    selection: JObject<'_>,
) {
    unowned_env
        .with_env(|env| -> JniResult<()> {
            if selection.is_null() {
                return Ok(());
            }

            let selection = parse_native_track_selection(env, selection)?;
            let _ = with_session_mut(env, session_handle, |session| {
                let _ = session
                    .inner
                    .dispatch_command(PlayerRuntimeCommand::SetVideoTrackSelection { selection });
            });
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_setAudioTrackSelection(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    selection: JObject<'_>,
) {
    unowned_env
        .with_env(|env| -> JniResult<()> {
            if selection.is_null() {
                return Ok(());
            }

            let selection = parse_native_track_selection(env, selection)?;
            let _ = with_session_mut(env, session_handle, |session| {
                let _ = session
                    .inner
                    .dispatch_command(PlayerRuntimeCommand::SetAudioTrackSelection { selection });
            });
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_setSubtitleTrackSelection(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    selection: JObject<'_>,
) {
    unowned_env
        .with_env(|env| -> JniResult<()> {
            if selection.is_null() {
                return Ok(());
            }

            let selection = parse_native_track_selection(env, selection)?;
            let _ = with_session_mut(env, session_handle, |session| {
                let _ = session.inner.dispatch_command(
                    PlayerRuntimeCommand::SetSubtitleTrackSelection { selection },
                );
            });
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_ikaros_vesper_player_android_VesperNativeJni_setAbrPolicy(
    mut unowned_env: EnvUnowned<'_>,
    _class: JClass<'_>,
    session_handle: jlong,
    policy: JObject<'_>,
) {
    unowned_env
        .with_env(|env| -> JniResult<()> {
            if policy.is_null() {
                return Ok(());
            }

            let policy = parse_native_abr_policy(env, policy)?;
            let _ = with_session_mut(env, session_handle, |session| {
                let _ = session
                    .inner
                    .dispatch_command(PlayerRuntimeCommand::SetAbrPolicy { policy });
            });
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}
