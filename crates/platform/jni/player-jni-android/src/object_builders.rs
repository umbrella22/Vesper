use jni::Env;
use jni::errors::Result as JniResult;
use jni::objects::{JObject, JValue};
use jni::sys::jint;
use player_platform_android::{
    AndroidHostCommand, AndroidHostEvent, AndroidHostSnapshot, AndroidHostTimelineKind,
};
use player_runtime::{
    MediaAbrMode, MediaAbrPolicy, MediaTrackSelection, MediaTrackSelectionMode,
    PlayerBufferingPolicy, PlayerBufferingPreset, PlayerCachePolicy, PlayerCachePreset,
    PlayerResolvedResiliencePolicy, PlayerRetryBackoff, PlayerRetryPolicy,
    PlayerTrackPreferencePolicy, PresentationState,
};

use crate::{
    PKG, field_sig, jni_name, method_sig, u64_to_jlong_saturating, u128_to_jlong_saturating,
};

pub(crate) fn boxed_long<'local>(
    env: &mut Env<'local>,
    value: Option<u64>,
) -> JniResult<JObject<'local>> {
    match value {
        Some(value) => env
            .call_static_method(
                jni_name("java/lang/Long"),
                jni_name("valueOf"),
                method_sig("(J)Ljava/lang/Long;").method_signature(),
                &[JValue::Long(u64_to_jlong_saturating(value))],
            )?
            .l(),
        None => Ok(JObject::null()),
    }
}

pub(crate) fn playback_state_object<'local>(
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

pub(crate) fn timeline_kind_object<'local>(
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

pub(crate) fn host_snapshot_object<'local>(
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
                    JValue::Long(u64_to_jlong_saturating(range.start_ms)),
                    JValue::Long(u64_to_jlong_saturating(range.end_ms)),
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
            JValue::Long(u64_to_jlong_saturating(snapshot.position_ms)),
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

pub(crate) fn host_event_object<'local>(
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
                &[JValue::Long(u64_to_jlong_saturating(*position_ms))],
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
                    JValue::Long(u64_to_jlong_saturating(*delay_ms)),
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

pub(crate) fn data_object_instance<'local>(
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

pub(crate) fn optional_java_string<'local>(
    env: &mut Env<'local>,
    value: Option<&str>,
) -> JniResult<JObject<'local>> {
    match value {
        Some(value) => Ok(JObject::from(env.new_string(value)?)),
        None => Ok(JObject::null()),
    }
}

pub(crate) fn track_selection_payload_object<'local>(
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

pub(crate) fn abr_policy_payload_object<'local>(
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
            JValue::Long(u64_to_jlong_saturating(max_bit_rate)),
            JValue::Bool(policy.max_width.is_some()),
            JValue::Int(max_width.min(i32::MAX as u32) as i32),
            JValue::Bool(policy.max_height.is_some()),
            JValue::Int(max_height.min(i32::MAX as u32) as i32),
        ],
    )
}

pub(crate) fn buffering_policy_object<'local>(
    env: &mut Env<'local>,
    policy: &PlayerBufferingPolicy,
) -> JniResult<JObject<'local>> {
    let class = env.find_class(jni_name(format!("{PKG}/NativeBufferingPolicy")))?;
    let min_buffer_ms = policy.min_buffer.map(|value| value.as_millis() as u64);
    let max_buffer_ms = policy.max_buffer.map(|value| value.as_millis() as u64);
    let buffer_for_playback_ms = policy
        .buffer_for_playback
        .map(|value| value.as_millis() as u64);
    let buffer_for_rebuffer_ms = policy
        .buffer_for_rebuffer
        .map(|value| value.as_millis() as u64);

    env.new_object(
        class,
        method_sig("(IZIZIZIZI)V").method_signature(),
        &[
            JValue::Int(match policy.preset {
                PlayerBufferingPreset::Default => 0,
                PlayerBufferingPreset::Balanced => 1,
                PlayerBufferingPreset::Streaming => 2,
                PlayerBufferingPreset::Resilient => 3,
                PlayerBufferingPreset::LowLatency => 4,
            }),
            JValue::Bool(min_buffer_ms.is_some()),
            JValue::Int(min_buffer_ms.unwrap_or_default().min(i32::MAX as u64) as jint),
            JValue::Bool(max_buffer_ms.is_some()),
            JValue::Int(max_buffer_ms.unwrap_or_default().min(i32::MAX as u64) as jint),
            JValue::Bool(buffer_for_playback_ms.is_some()),
            JValue::Int(
                buffer_for_playback_ms
                    .unwrap_or_default()
                    .min(i32::MAX as u64) as jint,
            ),
            JValue::Bool(buffer_for_rebuffer_ms.is_some()),
            JValue::Int(
                buffer_for_rebuffer_ms
                    .unwrap_or_default()
                    .min(i32::MAX as u64) as jint,
            ),
        ],
    )
}

pub(crate) fn retry_policy_object<'local>(
    env: &mut Env<'local>,
    policy: &PlayerRetryPolicy,
) -> JniResult<JObject<'local>> {
    let class = env.find_class(jni_name(format!("{PKG}/NativeRetryPolicy")))?;
    env.new_object(
        class,
        method_sig("(ZZIZJZJZI)V").method_signature(),
        &[
            JValue::Bool(false),
            JValue::Bool(policy.max_attempts.is_some()),
            JValue::Int(policy.max_attempts.unwrap_or_default().min(i32::MAX as u32) as jint),
            JValue::Bool(true),
            JValue::Long(u128_to_jlong_saturating(policy.base_delay.as_millis())),
            JValue::Bool(true),
            JValue::Long(u128_to_jlong_saturating(policy.max_delay.as_millis())),
            JValue::Bool(true),
            JValue::Int(match policy.backoff {
                PlayerRetryBackoff::Fixed => 0,
                PlayerRetryBackoff::Linear => 1,
                PlayerRetryBackoff::Exponential => 2,
            }),
        ],
    )
}

pub(crate) fn cache_policy_object<'local>(
    env: &mut Env<'local>,
    policy: &PlayerCachePolicy,
) -> JniResult<JObject<'local>> {
    let class = env.find_class(jni_name(format!("{PKG}/NativeCachePolicy")))?;
    env.new_object(
        class,
        method_sig("(IZJZJ)V").method_signature(),
        &[
            JValue::Int(match policy.preset {
                PlayerCachePreset::Default => 0,
                PlayerCachePreset::Disabled => 1,
                PlayerCachePreset::Streaming => 2,
                PlayerCachePreset::Resilient => 3,
            }),
            JValue::Bool(policy.max_memory_bytes.is_some()),
            JValue::Long(u64_to_jlong_saturating(
                policy.max_memory_bytes.unwrap_or_default(),
            )),
            JValue::Bool(policy.max_disk_bytes.is_some()),
            JValue::Long(u64_to_jlong_saturating(
                policy.max_disk_bytes.unwrap_or_default(),
            )),
        ],
    )
}

pub(crate) fn resolved_resilience_policy_object<'local>(
    env: &mut Env<'local>,
    policy: &PlayerResolvedResiliencePolicy,
) -> JniResult<JObject<'local>> {
    let class = env.find_class(jni_name(format!("{PKG}/NativeResolvedResiliencePolicy")))?;
    let buffering = buffering_policy_object(env, &policy.buffering_policy)?;
    let retry = retry_policy_object(env, &policy.retry_policy)?;
    let cache = cache_policy_object(env, &policy.cache_policy)?;
    env.new_object(
        class,
        method_sig(&format!(
            "(L{PKG}/NativeBufferingPolicy;L{PKG}/NativeRetryPolicy;L{PKG}/NativeCachePolicy;)V"
        ))
        .method_signature(),
        &[
            JValue::Object(&buffering),
            JValue::Object(&retry),
            JValue::Object(&cache),
        ],
    )
}

pub(crate) fn track_preferences_object<'local>(
    env: &mut Env<'local>,
    policy: &PlayerTrackPreferencePolicy,
) -> JniResult<JObject<'local>> {
    let class = env.find_class(jni_name(format!("{PKG}/NativeTrackPreferencePolicy")))?;
    let preferred_audio_language =
        optional_java_string(env, policy.preferred_audio_language.as_deref())?;
    let preferred_subtitle_language =
        optional_java_string(env, policy.preferred_subtitle_language.as_deref())?;
    let audio_selection = track_selection_payload_object(env, &policy.audio_selection)?;
    let subtitle_selection = track_selection_payload_object(env, &policy.subtitle_selection)?;
    let abr_policy = abr_policy_payload_object(env, &policy.abr_policy)?;
    env.new_object(
        class,
        method_sig(&format!(
            "(Ljava/lang/String;Ljava/lang/String;ZZL{PKG}/NativeTrackSelectionPayload;L{PKG}/NativeTrackSelectionPayload;L{PKG}/NativeAbrPolicyPayload;)V"
        ))
        .method_signature(),
        &[
            JValue::Object(&preferred_audio_language),
            JValue::Object(&preferred_subtitle_language),
            JValue::Bool(policy.select_subtitles_by_default),
            JValue::Bool(policy.select_undetermined_subtitle_language),
            JValue::Object(&audio_selection),
            JValue::Object(&subtitle_selection),
            JValue::Object(&abr_policy),
        ],
    )
}

pub(crate) fn native_command_object<'local>(
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
                &[JValue::Long(u64_to_jlong_saturating(*position_ms))],
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
