use super::*;

pub(crate) fn to_bridge_command(command: PlayerFfiCommandKind, position_ms: u64) -> FfiCommand {
    match command {
        PlayerFfiCommandKind::Play => FfiCommand::Play,
        PlayerFfiCommandKind::Pause => FfiCommand::Pause,
        PlayerFfiCommandKind::TogglePause => FfiCommand::TogglePause,
        PlayerFfiCommandKind::SeekTo => FfiCommand::SeekTo { position_ms },
        PlayerFfiCommandKind::Stop => FfiCommand::Stop,
    }
}

pub(crate) fn read_optional_c_string(
    value: *const c_char,
    field_name: &str,
) -> Result<Option<String>, PlayerFfiError> {
    if value.is_null() {
        return Ok(None);
    }

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
    let Some(selection) = (unsafe { selection.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "selection pointer was null",
        ));
    };

    Ok(BridgeTrackSelection {
        mode: match selection.mode {
            PlayerFfiTrackSelectionMode::Auto => BridgeTrackSelectionMode::Auto,
            PlayerFfiTrackSelectionMode::Disabled => BridgeTrackSelectionMode::Disabled,
            PlayerFfiTrackSelectionMode::Track => BridgeTrackSelectionMode::Track,
        },
        track_id: read_optional_c_string(selection.track_id, "selection.track_id")?,
    })
}

pub(crate) fn read_abr_policy(
    policy: *const PlayerFfiAbrPolicy,
) -> Result<BridgeAbrPolicy, PlayerFfiError> {
    let Some(policy) = (unsafe { policy.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "policy pointer was null",
        ));
    };

    Ok(BridgeAbrPolicy {
        mode: match policy.mode {
            PlayerFfiAbrMode::Auto => BridgeAbrMode::Auto,
            PlayerFfiAbrMode::Constrained => BridgeAbrMode::Constrained,
            PlayerFfiAbrMode::FixedTrack => BridgeAbrMode::FixedTrack,
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
    let Some(policy) = (unsafe { policy.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "buffering policy pointer was null",
        ));
    };

    Ok(BridgeBufferingPolicy {
        preset: policy.preset.into(),
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
            policy.backoff.into()
        } else {
            BridgeRetryBackoff::Linear
        },
    })
}

pub(crate) fn read_cache_policy(
    policy: *const PlayerFfiCachePolicy,
) -> Result<BridgeCachePolicy, PlayerFfiError> {
    let Some(policy) = (unsafe { policy.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "cache policy pointer was null",
        ));
    };

    Ok(BridgeCachePolicy {
        preset: policy.preset.into(),
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

    let uri = unsafe { CStr::from_ptr(uri) };
    let uri = uri
        .to_str()
        .map_err(|_| owned_api_error(PlayerFfiErrorCode::InvalidUtf8, "uri was not valid UTF-8"))?;

    Ok(uri.to_owned())
}
