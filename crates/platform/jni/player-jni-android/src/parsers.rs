use std::time::Duration;

use jni::Env;
use jni::errors::Result as JniResult;
use jni::objects::{JObject, JObjectArray, JString};
use jni::sys::{jfloat, jint, jlong, jobjectArray};
use player_platform_android::AndroidExoPlaybackState;
use player_runtime::{
    MediaAbrMode, MediaAbrPolicy, MediaSourceKind, MediaSourceProtocol, MediaTrack,
    MediaTrackCatalog, MediaTrackKind, MediaTrackSelection, MediaTrackSelectionMode,
    MediaTrackSelectionSnapshot, PlayerBufferingPolicy, PlayerBufferingPreset, PlayerCachePolicy,
    PlayerCachePreset, PlayerErrorCategory, PlayerErrorCode, PlayerRetryBackoff, PlayerRetryPolicy,
    PlayerTrackPreferencePolicy,
};

use crate::{PKG, field_sig, jni_name};

pub(crate) fn error_code_from_jni_ordinal(jni_ordinal: jint) -> PlayerErrorCode {
    match jni_ordinal {
        0 => PlayerErrorCode::InvalidArgument,
        1 => PlayerErrorCode::InvalidState,
        2 => PlayerErrorCode::InvalidSource,
        3 => PlayerErrorCode::BackendFailure,
        4 => PlayerErrorCode::AudioOutputUnavailable,
        5 => PlayerErrorCode::DecodeFailure,
        6 => PlayerErrorCode::SeekFailure,
        7 => PlayerErrorCode::Unsupported,
        8 => PlayerErrorCode::CommandChannelClosed,
        9 => PlayerErrorCode::EventChannelClosed,
        10 => PlayerErrorCode::Cancelled,
        11 => PlayerErrorCode::Timeout,
        _ => PlayerErrorCode::BackendFailure,
    }
}

pub(crate) fn error_category_from_jni_ordinal(jni_ordinal: jint) -> PlayerErrorCategory {
    match jni_ordinal {
        0 => PlayerErrorCategory::Input,
        1 => PlayerErrorCategory::Source,
        2 => PlayerErrorCategory::Network,
        3 => PlayerErrorCategory::Decode,
        4 => PlayerErrorCategory::AudioOutput,
        5 => PlayerErrorCategory::Playback,
        6 => PlayerErrorCategory::Capability,
        7 => PlayerErrorCategory::Platform,
        _ => PlayerErrorCategory::Platform,
    }
}

pub(crate) fn exo_state_from_ordinal(ordinal: jint) -> AndroidExoPlaybackState {
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

pub(crate) fn track_selection_mode_from_ordinal(ordinal: jint) -> MediaTrackSelectionMode {
    match ordinal {
        1 => MediaTrackSelectionMode::Disabled,
        2 => MediaTrackSelectionMode::Track,
        _ => MediaTrackSelectionMode::Auto,
    }
}

pub(crate) fn abr_mode_from_ordinal(ordinal: jint) -> MediaAbrMode {
    match ordinal {
        1 => MediaAbrMode::Constrained,
        2 => MediaAbrMode::FixedTrack,
        _ => MediaAbrMode::Auto,
    }
}

pub(crate) fn source_kind_from_ordinal(ordinal: jint) -> MediaSourceKind {
    match ordinal {
        0 => MediaSourceKind::Local,
        _ => MediaSourceKind::Remote,
    }
}

pub(crate) fn source_protocol_from_ordinal(ordinal: jint) -> MediaSourceProtocol {
    match ordinal {
        1 => MediaSourceProtocol::File,
        2 => MediaSourceProtocol::Content,
        3 => MediaSourceProtocol::Progressive,
        4 => MediaSourceProtocol::Hls,
        5 => MediaSourceProtocol::Dash,
        _ => MediaSourceProtocol::Unknown,
    }
}

pub(crate) fn buffering_preset_from_ordinal(ordinal: jint) -> PlayerBufferingPreset {
    match ordinal {
        1 => PlayerBufferingPreset::Balanced,
        2 => PlayerBufferingPreset::Streaming,
        3 => PlayerBufferingPreset::Resilient,
        4 => PlayerBufferingPreset::LowLatency,
        _ => PlayerBufferingPreset::Default,
    }
}

pub(crate) fn retry_backoff_from_ordinal(ordinal: jint) -> PlayerRetryBackoff {
    match ordinal {
        0 => PlayerRetryBackoff::Fixed,
        2 => PlayerRetryBackoff::Exponential,
        _ => PlayerRetryBackoff::Linear,
    }
}

pub(crate) fn cache_preset_from_ordinal(ordinal: jint) -> PlayerCachePreset {
    match ordinal {
        1 => PlayerCachePreset::Disabled,
        2 => PlayerCachePreset::Streaming,
        3 => PlayerCachePreset::Resilient,
        _ => PlayerCachePreset::Default,
    }
}

pub(crate) fn string_from_java_object(
    env: &mut Env<'_>,
    object: JObject<'_>,
) -> JniResult<Option<String>> {
    if object.is_null() {
        return Ok(None);
    }

    let value = unsafe { JString::from_raw(env, object.into_raw() as jni::sys::jstring) };
    Ok(Some(value.try_to_string(env)?))
}

pub(crate) fn string_array_to_vec(
    env: &mut Env<'_>,
    values: JObjectArray<'_>,
) -> JniResult<Vec<String>> {
    if values.is_null() {
        return Ok(Vec::new());
    }

    let len = values.len(env)?;
    let mut result = Vec::with_capacity(len);
    for index in 0..len {
        let value = values.get_element(env, index)?;
        if let Some(value) = string_from_java_object(env, value)? {
            result.push(value);
        }
    }
    Ok(result)
}

pub(crate) fn string_field(
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

pub(crate) fn bool_field(
    env: &mut Env<'_>,
    object: &JObject<'_>,
    field_name: &str,
) -> JniResult<bool> {
    env.get_field(
        object,
        jni_name(field_name),
        field_sig("Z").field_signature(),
    )?
    .z()
}

pub(crate) fn int_field(
    env: &mut Env<'_>,
    object: &JObject<'_>,
    field_name: &str,
) -> JniResult<jint> {
    env.get_field(
        object,
        jni_name(field_name),
        field_sig("I").field_signature(),
    )?
    .i()
}

pub(crate) fn long_field(
    env: &mut Env<'_>,
    object: &JObject<'_>,
    field_name: &str,
) -> JniResult<jlong> {
    env.get_field(
        object,
        jni_name(field_name),
        field_sig("J").field_signature(),
    )?
    .j()
}

pub(crate) fn float_field(
    env: &mut Env<'_>,
    object: &JObject<'_>,
    field_name: &str,
) -> JniResult<jfloat> {
    env.get_field(
        object,
        jni_name(field_name),
        field_sig("F").field_signature(),
    )?
    .f()
}

pub(crate) fn parse_native_track(env: &mut Env<'_>, track: JObject<'_>) -> JniResult<MediaTrack> {
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

pub(crate) fn parse_native_track_catalog(
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

pub(crate) fn parse_native_track_selection(
    env: &mut Env<'_>,
    selection: JObject<'_>,
) -> JniResult<MediaTrackSelection> {
    Ok(MediaTrackSelection {
        mode: track_selection_mode_from_ordinal(int_field(env, &selection, "modeOrdinal")?),
        track_id: string_field(env, &selection, "trackId")?,
    })
}

pub(crate) fn parse_native_abr_policy(
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

pub(crate) fn parse_native_track_preferences(
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

pub(crate) fn parse_native_track_selection_snapshot(
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

pub(crate) fn parse_native_buffering_policy(
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

pub(crate) fn parse_native_retry_policy(
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

pub(crate) fn parse_native_cache_policy(
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
