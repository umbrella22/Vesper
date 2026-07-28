use super::*;

pub(crate) fn to_bridge_command(
    command: u32,
    position_ms: u64,
) -> Result<FfiCommand, PlayerFfiError> {
    match command {
        value if value == PlayerFfiCommandKind::Play as u32 => Ok(FfiCommand::Play),
        value if value == PlayerFfiCommandKind::Pause as u32 => Ok(FfiCommand::Pause),
        value if value == PlayerFfiCommandKind::TogglePause as u32 => Ok(FfiCommand::TogglePause),
        value if value == PlayerFfiCommandKind::SeekTo as u32 => {
            Ok(FfiCommand::SeekTo { position_ms })
        }
        value if value == PlayerFfiCommandKind::Stop as u32 => Ok(FfiCommand::Stop),
        value => Err(invalid_ordinal("command", value)),
    }
}

fn invalid_ordinal(field: &str, value: u32) -> PlayerFfiError {
    owned_api_error(
        PlayerFfiErrorCode::InvalidArgument,
        &format!("{field} had invalid value {value}"),
    )
}

pub(crate) fn read_optional_c_string(
    value: *const c_char,
    field_name: &str,
) -> Result<Option<String>, PlayerFfiError> {
    if value.is_null() {
        return Ok(None);
    }

    // SAFETY: caller validated the pointer is non-null and points to a null-terminated C string
    let text = unsafe { CStr::from_ptr(value) };
    let text = text.to_str().map_err(|_| {
        owned_api_error(
            PlayerFfiErrorCode::InvalidUtf8,
            &format!("{field_name} was not valid UTF-8"),
        )
    })?;
    Ok(Some(text.to_owned()))
}

pub(crate) fn read_track_selection(
    selection: *const PlayerFfiTrackSelection,
) -> Result<BridgeTrackSelection, PlayerFfiError> {
    // SAFETY: caller validated the raw pointer is non-null and valid for the duration of the FFI call
    let Some(selection) = (unsafe { selection.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "selection pointer was null",
        ));
    };

    Ok(BridgeTrackSelection {
        mode: match selection.mode {
            value if value == PlayerFfiTrackSelectionMode::Auto as u32 => {
                BridgeTrackSelectionMode::Auto
            }
            value if value == PlayerFfiTrackSelectionMode::Disabled as u32 => {
                BridgeTrackSelectionMode::Disabled
            }
            value if value == PlayerFfiTrackSelectionMode::Track as u32 => {
                BridgeTrackSelectionMode::Track
            }
            value => return Err(invalid_ordinal("selection.mode", value)),
        },
        track_id: read_optional_c_string(selection.track_id, "selection.track_id")?,
    })
}

pub(crate) fn read_abr_policy(
    policy: *const PlayerFfiAbrPolicy,
) -> Result<BridgeAbrPolicy, PlayerFfiError> {
    // SAFETY: caller validated the raw pointer is non-null and valid for the duration of the FFI call
    let Some(policy) = (unsafe { policy.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "policy pointer was null",
        ));
    };

    Ok(BridgeAbrPolicy {
        mode: match policy.mode {
            value if value == PlayerFfiAbrMode::Auto as u32 => BridgeAbrMode::Auto,
            value if value == PlayerFfiAbrMode::Constrained as u32 => BridgeAbrMode::Constrained,
            value if value == PlayerFfiAbrMode::FixedTrack as u32 => BridgeAbrMode::FixedTrack,
            value => return Err(invalid_ordinal("policy.mode", value)),
        },
        track_id: read_optional_c_string(policy.track_id, "policy.track_id")?,
        max_bit_rate: policy.has_max_bit_rate.then_some(policy.max_bit_rate),
        max_width: policy.has_max_width.then_some(policy.max_width),
        max_height: policy.has_max_height.then_some(policy.max_height),
    })
}

pub(crate) fn read_preload_budget(
    budget: *const PlayerFfiPreloadBudgetPolicy,
) -> Result<BridgePreloadBudgetPolicy, PlayerFfiError> {
    // SAFETY: caller validated the raw pointer is non-null and valid for the duration of the FFI call
    let Some(budget) = (unsafe { budget.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "preload budget pointer was null",
        ));
    };

    Ok(BridgePreloadBudgetPolicy {
        max_concurrent_tasks: budget
            .has_max_concurrent_tasks
            .then_some(budget.max_concurrent_tasks),
        max_memory_bytes: budget
            .has_max_memory_bytes
            .then_some(budget.max_memory_bytes),
        max_disk_bytes: budget.has_max_disk_bytes.then_some(budget.max_disk_bytes),
        warmup_window_ms: budget
            .has_warmup_window_ms
            .then_some(budget.warmup_window_ms),
    })
}

pub(crate) fn read_track_preferences(
    preferences: *const PlayerFfiTrackPreferences,
) -> Result<BridgeTrackPreferences, PlayerFfiError> {
    // SAFETY: caller validated the raw pointer is non-null and valid for the duration of the FFI call
    let Some(preferences) = (unsafe { preferences.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "track preferences pointer was null",
        ));
    };

    Ok(BridgeTrackPreferences {
        preferred_audio_language: read_optional_c_string(
            preferences.preferred_audio_language,
            "preferences.preferred_audio_language",
        )?,
        preferred_subtitle_language: read_optional_c_string(
            preferences.preferred_subtitle_language,
            "preferences.preferred_subtitle_language",
        )?,
        select_subtitles_by_default: preferences.select_subtitles_by_default,
        select_undetermined_subtitle_language: preferences.select_undetermined_subtitle_language,
        audio_selection: read_track_selection(&preferences.audio_selection)?,
        subtitle_selection: read_track_selection(&preferences.subtitle_selection)?,
        abr_policy: read_abr_policy(&preferences.abr_policy)?,
    })
}

pub(crate) fn read_buffering_policy(
    policy: *const PlayerFfiBufferingPolicy,
) -> Result<BridgeBufferingPolicy, PlayerFfiError> {
    // SAFETY: caller validated the raw pointer is non-null and valid for the duration of the FFI call
    let Some(policy) = (unsafe { policy.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "buffering policy pointer was null",
        ));
    };

    Ok(BridgeBufferingPolicy {
        preset: match policy.preset {
            value if value == PlayerFfiBufferingPreset::Default as u32 => {
                BridgeBufferingPreset::Default
            }
            value if value == PlayerFfiBufferingPreset::Balanced as u32 => {
                BridgeBufferingPreset::Balanced
            }
            value if value == PlayerFfiBufferingPreset::Streaming as u32 => {
                BridgeBufferingPreset::Streaming
            }
            value if value == PlayerFfiBufferingPreset::Resilient as u32 => {
                BridgeBufferingPreset::Resilient
            }
            value if value == PlayerFfiBufferingPreset::LowLatency as u32 => {
                BridgeBufferingPreset::LowLatency
            }
            value => return Err(invalid_ordinal("buffering.preset", value)),
        },
        min_buffer_ms: policy.has_min_buffer_ms.then_some(policy.min_buffer_ms),
        max_buffer_ms: policy.has_max_buffer_ms.then_some(policy.max_buffer_ms),
        buffer_for_playback_ms: policy
            .has_buffer_for_playback_ms
            .then_some(policy.buffer_for_playback_ms),
        buffer_for_rebuffer_ms: policy
            .has_buffer_for_rebuffer_ms
            .then_some(policy.buffer_for_rebuffer_ms),
    })
}

pub(crate) fn read_retry_policy(
    policy: *const PlayerFfiRetryPolicy,
) -> Result<BridgeRetryPolicy, PlayerFfiError> {
    // SAFETY: caller validated the raw pointer is non-null and valid for the duration of the FFI call
    let Some(policy) = (unsafe { policy.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "retry policy pointer was null",
        ));
    };

    Ok(BridgeRetryPolicy {
        max_attempts: if policy.uses_default_max_attempts {
            Some(3)
        } else if policy.has_max_attempts {
            Some(policy.max_attempts)
        } else {
            None
        },
        base_delay_ms: if policy.has_base_delay_ms {
            policy.base_delay_ms
        } else {
            1_000
        },
        max_delay_ms: if policy.has_max_delay_ms {
            policy.max_delay_ms
        } else {
            5_000
        },
        backoff: if policy.has_backoff {
            match policy.backoff {
                value if value == PlayerFfiRetryBackoff::Fixed as u32 => BridgeRetryBackoff::Fixed,
                value if value == PlayerFfiRetryBackoff::Linear as u32 => {
                    BridgeRetryBackoff::Linear
                }
                value if value == PlayerFfiRetryBackoff::Exponential as u32 => {
                    BridgeRetryBackoff::Exponential
                }
                value => return Err(invalid_ordinal("retry.backoff", value)),
            }
        } else {
            BridgeRetryBackoff::Linear
        },
    })
}

pub(crate) fn read_cache_policy(
    policy: *const PlayerFfiCachePolicy,
) -> Result<BridgeCachePolicy, PlayerFfiError> {
    // SAFETY: caller validated the raw pointer is non-null and valid for the duration of the FFI call
    let Some(policy) = (unsafe { policy.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "cache policy pointer was null",
        ));
    };

    Ok(BridgeCachePolicy {
        preset: match policy.preset {
            value if value == PlayerFfiCachePreset::Default as u32 => BridgeCachePreset::Default,
            value if value == PlayerFfiCachePreset::Disabled as u32 => BridgeCachePreset::Disabled,
            value if value == PlayerFfiCachePreset::Streaming as u32 => {
                BridgeCachePreset::Streaming
            }
            value if value == PlayerFfiCachePreset::Resilient as u32 => {
                BridgeCachePreset::Resilient
            }
            value => return Err(invalid_ordinal("cache.preset", value)),
        },
        max_memory_bytes: policy
            .has_max_memory_bytes
            .then_some(policy.max_memory_bytes),
        max_disk_bytes: policy.has_max_disk_bytes.then_some(policy.max_disk_bytes),
    })
}

pub(crate) fn read_uri(uri: *const c_char) -> Result<String, PlayerFfiError> {
    if uri.is_null() {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "uri pointer was null",
        ));
    }

    // SAFETY: caller validated the pointer is non-null and points to a null-terminated C string
    let uri = unsafe { CStr::from_ptr(uri) };
    let uri = uri
        .to_str()
        .map_err(|_| owned_api_error(PlayerFfiErrorCode::InvalidUtf8, "uri was not valid UTF-8"))?;

    Ok(uri.to_owned())
}
