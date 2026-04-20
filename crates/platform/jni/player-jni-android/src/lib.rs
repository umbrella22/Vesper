mod download_jni;
mod playlist_jni;
mod preload_jni;

use std::any::Any;
use std::borrow::Borrow;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
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
use player_policy_resolver::{
    resolve_preload_budget as resolve_preload_budget_via_shared_resolver,
    resolve_resilience_policy as resolve_resilience_policy_via_shared_resolver,
    resolve_track_preferences as resolve_track_preferences_via_shared_resolver,
};
use player_runtime::{
    MediaAbrMode, MediaAbrPolicy, MediaSourceKind, MediaSourceProtocol, MediaTrack,
    MediaTrackCatalog, MediaTrackKind, MediaTrackSelection, MediaTrackSelectionMode,
    MediaTrackSelectionSnapshot, PlayerBufferingPolicy, PlayerBufferingPreset, PlayerCachePolicy,
    PlayerCachePreset, PlayerPreloadBudgetPolicy, PlayerResolvedPreloadBudgetPolicy,
    PlayerResolvedResiliencePolicy, PlayerRetryBackoff, PlayerRetryPolicy, PlayerRuntimeCommand,
    PlayerRuntimeError, PlayerRuntimeErrorCategory, PlayerRuntimeErrorCode,
    PlayerTrackPreferencePolicy, PresentationState,
};

pub(crate) const PKG: &str = "io/github/ikaros/vesper/player/android";

#[derive(Debug)]
pub(crate) struct HandleRegistry<T> {
    slots: Vec<HandleSlot<T>>,
    free_slots: Vec<u32>,
    next_generation_seed: u32,
}

#[derive(Debug)]
struct HandleSlot<T> {
    generation: u32,
    value: Option<T>,
}

impl<T> Default for HandleRegistry<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free_slots: Vec::new(),
            next_generation_seed: 0,
        }
    }
}

impl<T> HandleRegistry<T> {
    fn allocate_generation(&mut self) -> u32 {
        let generation = next_generation(self.next_generation_seed);
        self.next_generation_seed = generation;
        generation
    }

    pub(crate) fn insert(&mut self, value: T) -> i64 {
        let generation = self.allocate_generation();
        if let Some(slot_index) = self.free_slots.pop() {
            let slot = &mut self.slots[slot_index as usize];
            slot.generation = generation;
            slot.value = Some(value);
            return encode_handle(slot_index, generation);
        }

        debug_assert!(
            self.slots.len() < u32::MAX as usize,
            "HandleRegistry exhausted u32 slot space"
        );
        if self.slots.len() >= u32::MAX as usize {
            return 0;
        }

        let slot_index = self.slots.len() as u32;
        self.slots.push(HandleSlot {
            generation,
            value: Some(value),
        });
        encode_handle(slot_index, generation)
    }

    pub(crate) fn get(&self, handle: impl Borrow<i64>) -> Option<&T> {
        let (slot_index, generation) = decode_handle(*handle.borrow())?;
        let slot = self.slots.get(slot_index as usize)?;
        (slot.generation == generation)
            .then_some(slot.value.as_ref())
            .flatten()
    }

    pub(crate) fn remove(&mut self, handle: impl Borrow<i64>) -> Option<T> {
        let (slot_index, generation) = decode_handle(*handle.borrow())?;
        let slot = self.slots.get_mut(slot_index as usize)?;
        if slot.generation != generation {
            return None;
        }
        let value = slot.value.take()?;
        self.free_slots.push(slot_index);
        self.compact_tail();
        Some(value)
    }

    fn compact_tail(&mut self) {
        let Some(last_used_index) = self.slots.iter().rposition(|slot| slot.value.is_some()) else {
            self.slots.clear();
            self.free_slots.clear();
            return;
        };

        let new_len = last_used_index + 1;
        if new_len == self.slots.len() {
            return;
        }

        self.slots.truncate(new_len);
        self.free_slots
            .retain(|slot_index| (*slot_index as usize) < new_len);
    }
}

fn encode_handle(slot_index: u32, generation: u32) -> i64 {
    let slot_id = u64::from(slot_index) + 1;
    let raw = (slot_id << 32) | u64::from(generation.max(1));
    raw as i64
}

fn decode_handle(handle: i64) -> Option<(u32, u32)> {
    if handle == 0 {
        return None;
    }
    let raw = handle as u64;
    let slot_id = (raw >> 32) as u32;
    let generation = raw as u32;
    if slot_id == 0 || generation == 0 {
        return None;
    }
    Some((slot_id - 1, generation))
}

fn next_generation(generation: u32) -> u32 {
    generation.wrapping_add(1).max(1)
}

pub(crate) fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return format!("Rust panic crossed JNI boundary: {message}");
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return format!("Rust panic crossed JNI boundary: {message}");
    }
    "Rust panic crossed JNI boundary".to_owned()
}

fn throw_panic_exception(unowned_env: &mut EnvUnowned<'_>, message: &str) {
    let _ = unowned_env
        .with_env(|env| -> JniResult<()> {
            env.throw_new(jni_name("java/lang/RuntimeException"), jni_name(message))?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}

pub(crate) fn run_jni_entry<T: Default>(
    unowned_env: &mut EnvUnowned<'_>,
    f: impl FnOnce(&mut EnvUnowned<'_>) -> T,
) -> T {
    match catch_unwind(AssertUnwindSafe(|| f(unowned_env))) {
        Ok(value) => value,
        Err(payload) => {
            let message = panic_message(payload.as_ref());
            throw_panic_exception(unowned_env, &message);
            T::default()
        }
    }
}

pub(crate) fn u64_to_jlong_saturating(value: u64) -> jlong {
    value.min(i64::MAX as u64) as jlong
}

pub(crate) fn u128_to_jlong_saturating(value: u128) -> jlong {
    value.min(i64::MAX as u128) as jlong
}

type AndroidJniSession = Arc<Mutex<AndroidHostBridgeSession>>;

static SESSIONS: OnceLock<Mutex<HandleRegistry<AndroidJniSession>>> = OnceLock::new();

fn sessions() -> &'static Mutex<HandleRegistry<AndroidJniSession>> {
    SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

fn invalid_handle_error() -> &'static str {
    "invalid android JNI session handle"
}

pub(crate) fn jni_name(value: impl AsRef<str>) -> JNIString {
    JNIString::from(value.as_ref())
}

pub(crate) fn method_sig(value: &str) -> RuntimeMethodSignature {
    match RuntimeMethodSignature::from_str(value) {
        Ok(signature) => signature,
        Err(_) => unreachable!("static JNI method signature should parse"),
    }
}

pub(crate) fn field_sig(value: impl AsRef<str>) -> RuntimeFieldSignature {
    match RuntimeFieldSignature::from_str(value.as_ref()) {
        Ok(signature) => signature,
        Err(_) => unreachable!("static JNI field signature should parse"),
    }
}

fn with_session_mut<R>(
    env: &mut Env<'_>,
    handle: jlong,
    f: impl FnOnce(&mut AndroidHostBridgeSession) -> R,
) -> Option<R> {
    let session = {
        let guard = lock_or_recover(sessions());
        let Some(session) = guard.get(&handle).cloned() else {
            let _ = env.throw_new(
                jni_name("java/lang/IllegalArgumentException"),
                jni_name(invalid_handle_error()),
            );
            return None;
        };
        session
    };

    // 持有 session 锁期间禁止调用任何 `env.call_*`，也不要触发可能重入 JNI 的析构路径。
    let mut session = lock_or_recover(session.as_ref());
    Some(f(&mut session))
}

fn new_session(source_uri: String) -> Result<jlong, &'static str> {
    let session = Arc::new(Mutex::new(AndroidHostBridgeSession::new(source_uri)));
    let mut guard = lock_or_recover(sessions());
    let handle = guard.insert(session);
    if handle == 0 {
        return Err("android JNI session registry overflow");
    }
    Ok(handle)
}

fn boxed_long<'local>(env: &mut Env<'local>, value: Option<u64>) -> JniResult<JObject<'local>> {
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

fn optional_java_string<'local>(
    env: &mut Env<'local>,
    value: Option<&str>,
) -> JniResult<JObject<'local>> {
    match value {
        Some(value) => Ok(JObject::from(env.new_string(value)?)),
        None => Ok(JObject::null()),
    }
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
            JValue::Long(u64_to_jlong_saturating(max_bit_rate)),
            JValue::Bool(policy.max_width.is_some()),
            JValue::Int(max_width.min(i32::MAX as u32) as i32),
            JValue::Bool(policy.max_height.is_some()),
            JValue::Int(max_height.min(i32::MAX as u32) as i32),
        ],
    )
}

fn buffering_policy_object<'local>(
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

fn retry_policy_object<'local>(
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

fn cache_policy_object<'local>(
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

fn resolved_resilience_policy_object<'local>(
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

fn track_preferences_object<'local>(
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

pub(crate) fn error_code_from_ordinal(ordinal: jint) -> PlayerRuntimeErrorCode {
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

pub(crate) fn error_category_from_ordinal(ordinal: jint) -> PlayerRuntimeErrorCategory {
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

fn source_kind_from_ordinal(ordinal: jint) -> MediaSourceKind {
    match ordinal {
        0 => MediaSourceKind::Local,
        _ => MediaSourceKind::Remote,
    }
}

fn source_protocol_from_ordinal(ordinal: jint) -> MediaSourceProtocol {
    match ordinal {
        1 => MediaSourceProtocol::File,
        2 => MediaSourceProtocol::Content,
        3 => MediaSourceProtocol::Progressive,
        4 => MediaSourceProtocol::Hls,
        5 => MediaSourceProtocol::Dash,
        _ => MediaSourceProtocol::Unknown,
    }
}

fn buffering_preset_from_ordinal(ordinal: jint) -> PlayerBufferingPreset {
    match ordinal {
        1 => PlayerBufferingPreset::Balanced,
        2 => PlayerBufferingPreset::Streaming,
        3 => PlayerBufferingPreset::Resilient,
        4 => PlayerBufferingPreset::LowLatency,
        _ => PlayerBufferingPreset::Default,
    }
}

fn retry_backoff_from_ordinal(ordinal: jint) -> PlayerRetryBackoff {
    match ordinal {
        0 => PlayerRetryBackoff::Fixed,
        2 => PlayerRetryBackoff::Exponential,
        _ => PlayerRetryBackoff::Linear,
    }
}

fn cache_preset_from_ordinal(ordinal: jint) -> PlayerCachePreset {
    match ordinal {
        1 => PlayerCachePreset::Disabled,
        2 => PlayerCachePreset::Streaming,
        3 => PlayerCachePreset::Resilient,
        _ => PlayerCachePreset::Default,
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

fn parse_native_track_preferences(
    env: &mut Env<'_>,
    preferences: JObject<'_>,
) -> JniResult<PlayerTrackPreferencePolicy> {
    let audio_selection = env
        .get_field(
            &preferences,
            jni_name("audioSelection"),
            field_sig(format!("L{PKG}/NativeTrackSelectionPayload;")).field_signature(),
        )?
        .l()?;
    let subtitle_selection = env
        .get_field(
            &preferences,
            jni_name("subtitleSelection"),
            field_sig(format!("L{PKG}/NativeTrackSelectionPayload;")).field_signature(),
        )?
        .l()?;
    let abr_policy = env
        .get_field(
            &preferences,
            jni_name("abrPolicy"),
            field_sig(format!("L{PKG}/NativeAbrPolicyPayload;")).field_signature(),
        )?
        .l()?;

    Ok(PlayerTrackPreferencePolicy {
        preferred_audio_language: string_field(env, &preferences, "preferredAudioLanguage")?,
        preferred_subtitle_language: string_field(env, &preferences, "preferredSubtitleLanguage")?,
        select_subtitles_by_default: bool_field(env, &preferences, "selectSubtitlesByDefault")?,
        select_undetermined_subtitle_language: bool_field(
            env,
            &preferences,
            "selectUndeterminedSubtitleLanguage",
        )?,
        audio_selection: parse_native_track_selection(env, audio_selection)?,
        subtitle_selection: parse_native_track_selection(env, subtitle_selection)?,
        abr_policy: parse_native_abr_policy(env, abr_policy)?,
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

fn parse_native_buffering_policy(
    env: &mut Env<'_>,
    policy: JObject<'_>,
) -> JniResult<PlayerBufferingPolicy> {
    let has_min_buffer_ms = bool_field(env, &policy, "hasMinBufferMs")?;
    let has_max_buffer_ms = bool_field(env, &policy, "hasMaxBufferMs")?;
    let has_buffer_for_playback_ms = bool_field(env, &policy, "hasBufferForPlaybackMs")?;
    let has_buffer_for_rebuffer_ms =
        bool_field(env, &policy, "hasBufferForPlaybackAfterRebufferMs")?;

    Ok(PlayerBufferingPolicy {
        preset: buffering_preset_from_ordinal(int_field(env, &policy, "presetOrdinal")?),
        min_buffer: has_min_buffer_ms.then_some(Duration::from_millis(int_field(
            env,
            &policy,
            "minBufferMs",
        )? as u64)),
        max_buffer: has_max_buffer_ms.then_some(Duration::from_millis(int_field(
            env,
            &policy,
            "maxBufferMs",
        )? as u64)),
        buffer_for_playback: has_buffer_for_playback_ms.then_some(Duration::from_millis(
            int_field(env, &policy, "bufferForPlaybackMs")? as u64,
        )),
        buffer_for_rebuffer: has_buffer_for_rebuffer_ms.then_some(Duration::from_millis(
            int_field(env, &policy, "bufferForPlaybackAfterRebufferMs")? as u64,
        )),
    })
}

fn parse_native_retry_policy(
    env: &mut Env<'_>,
    policy: JObject<'_>,
) -> JniResult<PlayerRetryPolicy> {
    let uses_default_max_attempts = bool_field(env, &policy, "usesDefaultMaxAttempts")?;
    let has_max_attempts = bool_field(env, &policy, "hasMaxAttempts")?;
    let has_base_delay_ms = bool_field(env, &policy, "hasBaseDelayMs")?;
    let has_max_delay_ms = bool_field(env, &policy, "hasMaxDelayMs")?;
    let has_backoff = bool_field(env, &policy, "hasBackoff")?;

    Ok(PlayerRetryPolicy {
        max_attempts: if uses_default_max_attempts {
            Some(3)
        } else if has_max_attempts {
            Some(int_field(env, &policy, "maxAttempts")? as u32)
        } else {
            None
        },
        base_delay: if has_base_delay_ms {
            Duration::from_millis(long_field(env, &policy, "baseDelayMs")? as u64)
        } else {
            Duration::from_millis(1_000)
        },
        max_delay: if has_max_delay_ms {
            Duration::from_millis(long_field(env, &policy, "maxDelayMs")? as u64)
        } else {
            Duration::from_millis(5_000)
        },
        backoff: if has_backoff {
            retry_backoff_from_ordinal(int_field(env, &policy, "backoffOrdinal")?)
        } else {
            PlayerRetryBackoff::Linear
        },
    })
}

fn parse_native_cache_policy(
    env: &mut Env<'_>,
    policy: JObject<'_>,
) -> JniResult<PlayerCachePolicy> {
    let has_max_memory_bytes = bool_field(env, &policy, "hasMaxMemoryBytes")?;
    let has_max_disk_bytes = bool_field(env, &policy, "hasMaxDiskBytes")?;

    Ok(PlayerCachePolicy {
        preset: cache_preset_from_ordinal(int_field(env, &policy, "presetOrdinal")?),
        max_memory_bytes: has_max_memory_bytes
            .then_some(long_field(env, &policy, "maxMemoryBytes")? as u64),
        max_disk_bytes: has_max_disk_bytes
            .then_some(long_field(env, &policy, "maxDiskBytes")? as u64),
    })
}

fn resolve_resilience_policy_with_runtime(
    source_kind: MediaSourceKind,
    source_protocol: MediaSourceProtocol,
    buffering_policy: PlayerBufferingPolicy,
    retry_policy: PlayerRetryPolicy,
    cache_policy: PlayerCachePolicy,
) -> PlayerResolvedResiliencePolicy {
    resolve_resilience_policy_via_shared_resolver(
        source_kind,
        source_protocol,
        buffering_policy,
        retry_policy,
        cache_policy,
    )
}

fn resolve_track_preferences_with_runtime(
    track_preferences: PlayerTrackPreferencePolicy,
) -> PlayerTrackPreferencePolicy {
    resolve_track_preferences_via_shared_resolver(track_preferences)
}

pub(crate) fn resolve_preload_budget_with_runtime(
    preload_budget: PlayerPreloadBudgetPolicy,
) -> PlayerResolvedPreloadBudgetPolicy {
    resolve_preload_budget_via_shared_resolver(preload_budget)
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
                guard.remove(&session_handle);
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
    code_ordinal: jint,
    category_ordinal: jint,
    retriable: jboolean,
    message: JString<'_>,
) {
    run_jni_entry(&mut unowned_env, |unowned_env| {
        unowned_env
            .with_env(|env| -> JniResult<()> {
                let message = message.try_to_string(env)?;
                let code = error_code_from_ordinal(code_ordinal);
                let category = error_category_from_ordinal(category_ordinal);
                let _ = with_session_mut(env, session_handle, |session| {
                    session.report_runtime_error(PlayerRuntimeError::with_taxonomy(
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
    use super::{
        HandleRegistry, MediaAbrMode, MediaAbrPolicy, MediaSourceKind, MediaSourceProtocol,
        MediaTrackSelection, PlayerBufferingPolicy, PlayerBufferingPreset, PlayerCachePolicy,
        PlayerCachePreset, PlayerRetryBackoff, PlayerRetryPolicy, PlayerTrackPreferencePolicy,
        resolve_resilience_policy_with_runtime, resolve_track_preferences_with_runtime,
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
