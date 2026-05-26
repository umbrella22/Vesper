use std::any::Any;
use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::ptr;
use std::slice;
use std::sync::Arc;
use std::time::{Duration, Instant};

use player_model::MediaSource;
use player_platform_ios::{IosDownloadCommand, IosPreloadCommand};
use player_plugin::OutputFormat;
use player_runtime::{
    DownloadAssetId, DownloadAssetIndex, DownloadAssetStream, DownloadByteRange,
    DownloadContentFormat, DownloadErrorSummary, DownloadEvent, DownloadProfile,
    DownloadProgressSnapshot, DownloadResourceRecord, DownloadSegmentRecord, DownloadSource,
    DownloadStreamKind, DownloadTaskId, DownloadTaskSnapshot, DownloadTaskStatus, MediaAbrMode,
    MediaAbrPolicy, MediaSourceKind, MediaSourceProtocol, MediaTrackSelection,
    MediaTrackSelectionMode, PlayerBufferingPolicy, PlayerBufferingPreset, PlayerCachePolicy,
    PlayerCachePreset, PlayerError, PlayerErrorCategory, PlayerErrorCode,
    PlayerPreloadBudgetPolicy, PlayerRetryBackoff, PlayerRetryPolicy, PlayerTrackPreferencePolicy,
    PlaylistActiveItem, PlaylistCoordinatorConfig, PlaylistFailureStrategy, PlaylistNeighborWindow,
    PlaylistPreloadWindow, PlaylistQueueItem, PlaylistRepeatMode, PlaylistSwitchPolicy,
    PlaylistViewportHint, PlaylistViewportHintKind, PreloadBudgetScope, PreloadCandidate,
    PreloadCandidateKind, PreloadConfig, PreloadPriority, PreloadSelectionHint,
    PreloadTaskSnapshot,
};

use crate::*;

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
    selection: &PlayerFfiTrackSelection,
) -> Result<MediaTrackSelection, PlayerFfiError> {
    Ok(MediaTrackSelection {
        mode: selection.mode.into(),
        track_id: read_optional_c_string(selection.track_id, "selection.track_id")?,
    })
}

pub(crate) fn read_abr_policy(
    policy: &PlayerFfiAbrPolicy,
) -> Result<MediaAbrPolicy, PlayerFfiError> {
    Ok(MediaAbrPolicy {
        mode: policy.mode.into(),
        track_id: read_optional_c_string(policy.track_id, "policy.track_id")?,
        max_bit_rate: policy.has_max_bit_rate.then_some(policy.max_bit_rate),
        max_width: policy.has_max_width.then_some(policy.max_width),
        max_height: policy.has_max_height.then_some(policy.max_height),
    })
}

pub(crate) fn read_preload_budget(
    budget: *const PlayerFfiPreloadBudgetPolicy,
) -> Result<PlayerPreloadBudgetPolicy, PlayerFfiError> {
    let Some(budget) = (unsafe { budget.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "preload budget pointer was null",
        ));
    };

    Ok(PlayerPreloadBudgetPolicy {
        max_concurrent_tasks: budget
            .has_max_concurrent_tasks
            .then_some(budget.max_concurrent_tasks),
        max_memory_bytes: budget
            .has_max_memory_bytes
            .then_some(budget.max_memory_bytes),
        max_disk_bytes: budget.has_max_disk_bytes.then_some(budget.max_disk_bytes),
        warmup_window: budget
            .has_warmup_window_ms
            .then_some(Duration::from_millis(budget.warmup_window_ms)),
    })
}

pub(crate) fn read_preload_candidate(
    candidate: &PlayerFfiPreloadCandidate,
) -> Result<PreloadCandidate, PlayerFfiError> {
    let source_uri = read_optional_c_string(candidate.source_uri, "candidate.source_uri")?
        .ok_or_else(|| {
            owned_api_error(
                PlayerFfiErrorCode::NullPointer,
                "candidate.source_uri was null",
            )
        })?;
    let scope_id = read_optional_c_string(candidate.scope_id, "candidate.scope_id")?;
    let scope = match candidate.scope_kind {
        PlayerFfiPreloadScopeKind::App => PreloadBudgetScope::App,
        PlayerFfiPreloadScopeKind::Session => {
            PreloadBudgetScope::Session(scope_id.unwrap_or_default())
        }
        PlayerFfiPreloadScopeKind::Playlist => {
            PreloadBudgetScope::Playlist(scope_id.unwrap_or_default())
        }
    };

    Ok(PreloadCandidate {
        source: MediaSource::new(source_uri),
        scope,
        kind: candidate.candidate_kind.into(),
        selection_hint: candidate.selection_hint.into(),
        config: PreloadConfig {
            priority: candidate.priority.into(),
            ttl: candidate
                .has_ttl_ms
                .then_some(Duration::from_millis(candidate.ttl_ms)),
            expected_memory_bytes: candidate.expected_memory_bytes,
            expected_disk_bytes: candidate.expected_disk_bytes,
            warmup_window: candidate
                .has_warmup_window_ms
                .then_some(Duration::from_millis(candidate.warmup_window_ms)),
        },
    })
}

pub(crate) fn read_download_config(
    config: *const PlayerFfiDownloadConfig,
) -> Result<ResolvedDownloadConfig, PlayerFfiError> {
    let Some(config) = (unsafe { config.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "download config pointer was null",
        ));
    };
    Ok(ResolvedDownloadConfig {
        auto_start: config.auto_start,
        run_post_processors_on_completion: config.run_post_processors_on_completion,
        plugin_library_paths: read_string_list(
            config.plugin_library_paths,
            config.plugin_library_paths_len,
            "config.plugin_library_paths",
        )?
        .into_iter()
        .map(PathBuf::from)
        .collect(),
    })
}

pub(crate) fn read_download_source(
    source: *const PlayerFfiDownloadSource,
) -> Result<DownloadSource, PlayerFfiError> {
    let Some(source) = (unsafe { source.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "download source pointer was null",
        ));
    };
    let source_uri =
        read_optional_c_string(source.source_uri, "source.source_uri")?.ok_or_else(|| {
            owned_api_error(
                PlayerFfiErrorCode::NullPointer,
                "source.source_uri was null",
            )
        })?;

    let header_names = read_string_list(
        source.header_names,
        source.headers_len,
        "source.header_names",
    )?;
    let header_values = read_string_list(
        source.header_values,
        source.headers_len,
        "source.header_values",
    )?;
    let mut download_source =
        DownloadSource::new(MediaSource::new(source_uri), source.content_format.into())
            .with_request_headers(header_names.into_iter().zip(header_values));
    if let Some(manifest_uri) = read_optional_c_string(source.manifest_uri, "source.manifest_uri")?
        && !manifest_uri.is_empty()
    {
        download_source = download_source.with_manifest_uri(manifest_uri);
    }
    Ok(download_source)
}

pub(crate) fn read_string_list(
    values: *mut *mut c_char,
    len: usize,
    field_name: &str,
) -> Result<Vec<String>, PlayerFfiError> {
    if len == 0 {
        return Ok(Vec::new());
    }
    if values.is_null() {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            &format!("{field_name} was null"),
        ));
    }

    let values = unsafe { slice::from_raw_parts(values, len) };
    values
        .iter()
        .map(|value| read_optional_c_string(*value as *const c_char, field_name))
        .collect::<Result<Vec<_>, _>>()
        .map(|values| values.into_iter().flatten().collect())
}

pub(crate) fn read_download_profile(
    profile: *const PlayerFfiDownloadProfile,
) -> Result<DownloadProfile, PlayerFfiError> {
    let Some(profile) = (unsafe { profile.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "download profile pointer was null",
        ));
    };

    Ok(DownloadProfile {
        variant_id: read_optional_c_string(profile.variant_id, "profile.variant_id")?,
        preferred_audio_language: read_optional_c_string(
            profile.preferred_audio_language,
            "profile.preferred_audio_language",
        )?,
        preferred_subtitle_language: read_optional_c_string(
            profile.preferred_subtitle_language,
            "profile.preferred_subtitle_language",
        )?,
        selected_track_ids: read_string_list(
            profile.selected_track_ids,
            profile.selected_track_ids_len,
            "profile.selected_track_ids",
        )?,
        target_output_format: profile
            .has_target_output_format
            .then(|| OutputFormat::from(profile.target_output_format)),
        target_directory: read_optional_c_string(
            profile.target_directory,
            "profile.target_directory",
        )?
        .map(PathBuf::from),
        allow_metered_network: profile.allow_metered_network,
    })
}

pub(crate) fn read_download_resource_record(
    resource: &PlayerFfiDownloadResourceRecord,
) -> Result<DownloadResourceRecord, PlayerFfiError> {
    Ok(DownloadResourceRecord {
        resource_id: read_optional_c_string(resource.resource_id, "resource.resource_id")?
            .ok_or_else(|| {
                owned_api_error(
                    PlayerFfiErrorCode::NullPointer,
                    "resource.resource_id was null",
                )
            })?,
        uri: read_optional_c_string(resource.uri, "resource.uri")?.ok_or_else(|| {
            owned_api_error(PlayerFfiErrorCode::NullPointer, "resource.uri was null")
        })?,
        relative_path: read_optional_c_string(resource.relative_path, "resource.relative_path")?
            .map(PathBuf::from),
        byte_range: resource.has_byte_range.then_some(DownloadByteRange {
            offset: resource.byte_range.offset,
            length: resource.byte_range.length,
        }),
        generated_text: read_optional_c_string(resource.generated_text, "resource.generated_text")?,
        size_bytes: resource.has_size_bytes.then_some(resource.size_bytes),
        etag: read_optional_c_string(resource.etag, "resource.etag")?,
        checksum: read_optional_c_string(resource.checksum, "resource.checksum")?,
    })
}

pub(crate) fn read_download_segment_record(
    segment: &PlayerFfiDownloadSegmentRecord,
) -> Result<DownloadSegmentRecord, PlayerFfiError> {
    Ok(DownloadSegmentRecord {
        segment_id: read_optional_c_string(segment.segment_id, "segment.segment_id")?.ok_or_else(
            || {
                owned_api_error(
                    PlayerFfiErrorCode::NullPointer,
                    "segment.segment_id was null",
                )
            },
        )?,
        uri: read_optional_c_string(segment.uri, "segment.uri")?.ok_or_else(|| {
            owned_api_error(PlayerFfiErrorCode::NullPointer, "segment.uri was null")
        })?,
        relative_path: read_optional_c_string(segment.relative_path, "segment.relative_path")?
            .map(PathBuf::from),
        sequence: segment.has_sequence.then_some(segment.sequence),
        byte_range: segment.has_byte_range.then_some(DownloadByteRange {
            offset: segment.byte_range.offset,
            length: segment.byte_range.length,
        }),
        size_bytes: segment.has_size_bytes.then_some(segment.size_bytes),
        checksum: read_optional_c_string(segment.checksum, "segment.checksum")?,
    })
}

pub(crate) fn read_download_asset_stream(
    stream: &PlayerFfiDownloadAssetStream,
) -> Result<DownloadAssetStream, PlayerFfiError> {
    let resource_ids = read_string_list(
        stream.resource_ids,
        stream.resource_ids_len,
        "stream.resource_ids",
    )?;
    let segment_ids = read_string_list(
        stream.segment_ids,
        stream.segment_ids_len,
        "stream.segment_ids",
    )?;
    let metadata_keys = read_string_list(
        stream.metadata_keys,
        stream.metadata_len,
        "stream.metadata_keys",
    )?;
    let metadata_values = read_string_list(
        stream.metadata_values,
        stream.metadata_len,
        "stream.metadata_values",
    )?;
    if metadata_keys.len() != metadata_values.len() {
        return Err(owned_api_error(
            PlayerFfiErrorCode::InvalidArgument,
            "stream metadata keys and values had different lengths",
        ));
    }

    Ok(DownloadAssetStream {
        stream_id: read_optional_c_string(stream.stream_id, "stream.stream_id")?.ok_or_else(
            || owned_api_error(PlayerFfiErrorCode::NullPointer, "stream.stream_id was null"),
        )?,
        kind: stream.kind.into(),
        language: read_optional_c_string(stream.language, "stream.language")?,
        codec: read_optional_c_string(stream.codec, "stream.codec")?,
        label: read_optional_c_string(stream.label, "stream.label")?,
        quality_rank: stream.has_quality_rank.then_some(stream.quality_rank),
        resource_ids,
        segment_ids,
        metadata: metadata_keys.into_iter().zip(metadata_values).collect(),
    })
}

pub(crate) fn read_download_asset_index(
    asset_index: *const PlayerFfiDownloadAssetIndex,
) -> Result<DownloadAssetIndex, PlayerFfiError> {
    let Some(asset_index) = (unsafe { asset_index.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "download asset_index pointer was null",
        ));
    };

    let resources = if asset_index.resources_len == 0 {
        Vec::new()
    } else {
        if asset_index.resources.is_null() {
            return Err(owned_api_error(
                PlayerFfiErrorCode::NullPointer,
                "asset_index.resources was null",
            ));
        }
        unsafe { slice::from_raw_parts(asset_index.resources, asset_index.resources_len) }
            .iter()
            .map(read_download_resource_record)
            .collect::<Result<Vec<_>, _>>()?
    };

    let segments = if asset_index.segments_len == 0 {
        Vec::new()
    } else {
        if asset_index.segments.is_null() {
            return Err(owned_api_error(
                PlayerFfiErrorCode::NullPointer,
                "asset_index.segments was null",
            ));
        }
        unsafe { slice::from_raw_parts(asset_index.segments, asset_index.segments_len) }
            .iter()
            .map(read_download_segment_record)
            .collect::<Result<Vec<_>, _>>()?
    };

    let streams = if asset_index.streams_len == 0 {
        Vec::new()
    } else {
        if asset_index.streams.is_null() {
            return Err(owned_api_error(
                PlayerFfiErrorCode::NullPointer,
                "asset_index.streams was null",
            ));
        }
        unsafe { slice::from_raw_parts(asset_index.streams, asset_index.streams_len) }
            .iter()
            .map(read_download_asset_stream)
            .collect::<Result<Vec<_>, _>>()?
    };

    Ok(DownloadAssetIndex {
        content_format: asset_index.content_format.into(),
        version: read_optional_c_string(asset_index.version, "asset_index.version")?,
        etag: read_optional_c_string(asset_index.etag, "asset_index.etag")?,
        checksum: read_optional_c_string(asset_index.checksum, "asset_index.checksum")?,
        total_size_bytes: asset_index
            .has_total_size_bytes
            .then_some(asset_index.total_size_bytes),
        resources,
        segments,
        streams,
        completed_path: read_optional_c_string(
            asset_index.completed_path,
            "asset_index.completed_path",
        )?
        .map(PathBuf::from),
    })
}

pub(crate) fn read_download_progress(
    progress: &PlayerFfiDownloadProgressSnapshot,
) -> DownloadProgressSnapshot {
    DownloadProgressSnapshot {
        received_bytes: progress.received_bytes,
        total_bytes: progress.has_total_bytes.then_some(progress.total_bytes),
        received_segments: progress.received_segments,
        total_segments: progress
            .has_total_segments
            .then_some(progress.total_segments),
    }
}

pub(crate) fn read_download_task(
    task: &PlayerFfiDownloadTask,
    now: Instant,
) -> Result<DownloadTaskSnapshot, PlayerFfiError> {
    let asset_id = read_optional_c_string(task.asset_id, "task.asset_id")?.ok_or_else(|| {
        owned_api_error(PlayerFfiErrorCode::NullPointer, "task.asset_id was null")
    })?;
    let error_summary = if task.has_error {
        Some(DownloadErrorSummary {
            code: task.error_code.into(),
            category: task.error_category.into(),
            retriable: task.error_retriable,
            message: read_optional_c_string(task.error_message, "task.error_message")?
                .unwrap_or_else(|| "download failed".to_owned()),
        })
    } else {
        None
    };

    Ok(DownloadTaskSnapshot {
        task_id: DownloadTaskId::from_raw(task.task_id),
        asset_id: DownloadAssetId::new(asset_id),
        source: read_download_source(&task.source)?,
        profile: read_download_profile(&task.profile)?,
        status: DownloadTaskStatus::from(task.status),
        progress: read_download_progress(&task.progress),
        asset_index: Arc::new(read_download_asset_index(&task.asset_index)?),
        created_at: now,
        updated_at: now,
        error_summary,
    })
}

pub(crate) fn read_playlist_config(
    config: *const PlayerFfiPlaylistConfig,
) -> Result<(String, PlaylistCoordinatorConfig), PlayerFfiError> {
    let Some(config) = (unsafe { config.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "playlist config pointer was null",
        ));
    };

    let playlist_id = read_optional_c_string(config.playlist_id, "config.playlist_id")?
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "ios-host-playlist".to_owned());

    Ok((
        playlist_id,
        PlaylistCoordinatorConfig {
            neighbor_window: PlaylistNeighborWindow {
                previous: config.neighbor_previous as usize,
                next: config.neighbor_next as usize,
            },
            preload_window: PlaylistPreloadWindow {
                near_visible: config.preload_near_visible as usize,
                prefetch_only: config.preload_prefetch_only as usize,
            },
            switch_policy: PlaylistSwitchPolicy {
                auto_advance: config.auto_advance,
                repeat_mode: match config.repeat_mode {
                    PlayerFfiPlaylistRepeatMode::Off => PlaylistRepeatMode::Off,
                    PlayerFfiPlaylistRepeatMode::One => PlaylistRepeatMode::One,
                    PlayerFfiPlaylistRepeatMode::All => PlaylistRepeatMode::All,
                },
                failure_strategy: match config.failure_strategy {
                    PlayerFfiPlaylistFailureStrategy::Pause => PlaylistFailureStrategy::Pause,
                    PlayerFfiPlaylistFailureStrategy::SkipToNext => {
                        PlaylistFailureStrategy::SkipToNext
                    }
                },
            },
        },
    ))
}

pub(crate) fn read_playlist_queue_item(
    item: &PlayerFfiPlaylistQueueItem,
) -> Result<PlaylistQueueItem, PlayerFfiError> {
    let item_id = read_optional_c_string(item.item_id, "item.item_id")?
        .ok_or_else(|| owned_api_error(PlayerFfiErrorCode::NullPointer, "item.item_id was null"))?;
    let source_uri =
        read_optional_c_string(item.source_uri, "item.source_uri")?.ok_or_else(|| {
            owned_api_error(PlayerFfiErrorCode::NullPointer, "item.source_uri was null")
        })?;

    Ok(
        PlaylistQueueItem::new(item_id, MediaSource::new(source_uri)).with_preload_profile(
            player_runtime::PlaylistItemPreloadProfile {
                expected_memory_bytes: item.expected_memory_bytes,
                expected_disk_bytes: item.expected_disk_bytes,
                ttl: item
                    .has_ttl_ms
                    .then_some(Duration::from_millis(item.ttl_ms)),
                warmup_window: item
                    .has_warmup_window_ms
                    .then_some(Duration::from_millis(item.warmup_window_ms)),
            },
        ),
    )
}

pub(crate) fn read_playlist_viewport_hint(
    hint: &PlayerFfiPlaylistViewportHint,
) -> Result<PlaylistViewportHint, PlayerFfiError> {
    let item_id = read_optional_c_string(hint.item_id, "hint.item_id")?
        .ok_or_else(|| owned_api_error(PlayerFfiErrorCode::NullPointer, "hint.item_id was null"))?;
    let kind = match hint.kind {
        PlayerFfiPlaylistViewportHintKind::Visible => PlaylistViewportHintKind::Visible,
        PlayerFfiPlaylistViewportHintKind::NearVisible => PlaylistViewportHintKind::NearVisible,
        PlayerFfiPlaylistViewportHintKind::PrefetchOnly => PlaylistViewportHintKind::PrefetchOnly,
        PlayerFfiPlaylistViewportHintKind::Hidden => PlaylistViewportHintKind::Hidden,
    };

    Ok(PlaylistViewportHint::new(item_id, kind).with_order(hint.order))
}

pub(crate) fn player_error_to_ffi(error: PlayerError) -> PlayerFfiError {
    let (code, category) = map_player_error(&error);
    PlayerFfiError {
        code,
        category,
        retriable: error.is_retriable(),
        message: into_c_string_ptr(error.message().to_owned()),
    }
}

pub(crate) fn map_player_error(
    error: &PlayerError,
) -> (PlayerFfiErrorCode, PlayerFfiErrorCategory) {
    (
        error_code_to_ffi(error.code()),
        error_category_to_ffi(error.category()),
    )
}

pub(crate) fn error_code_to_ffi(code: PlayerErrorCode) -> PlayerFfiErrorCode {
    match code {
        PlayerErrorCode::InvalidArgument => PlayerFfiErrorCode::InvalidArgument,
        PlayerErrorCode::InvalidState => PlayerFfiErrorCode::InvalidState,
        PlayerErrorCode::InvalidSource => PlayerFfiErrorCode::InvalidSource,
        PlayerErrorCode::BackendFailure => PlayerFfiErrorCode::BackendFailure,
        PlayerErrorCode::AudioOutputUnavailable => PlayerFfiErrorCode::AudioOutputUnavailable,
        PlayerErrorCode::DecodeFailure => PlayerFfiErrorCode::DecodeFailure,
        PlayerErrorCode::SeekFailure => PlayerFfiErrorCode::SeekFailure,
        PlayerErrorCode::Unsupported => PlayerFfiErrorCode::Unsupported,
        PlayerErrorCode::CommandChannelClosed => PlayerFfiErrorCode::CommandChannelClosed,
        PlayerErrorCode::EventChannelClosed => PlayerFfiErrorCode::EventChannelClosed,
        PlayerErrorCode::Cancelled => PlayerFfiErrorCode::Cancelled,
        PlayerErrorCode::Timeout => PlayerFfiErrorCode::Timeout,
    }
}

pub(crate) fn error_category_to_ffi(category: PlayerErrorCategory) -> PlayerFfiErrorCategory {
    match category {
        PlayerErrorCategory::Input => PlayerFfiErrorCategory::Input,
        PlayerErrorCategory::Source => PlayerFfiErrorCategory::Source,
        PlayerErrorCategory::Network => PlayerFfiErrorCategory::Network,
        PlayerErrorCategory::Decode => PlayerFfiErrorCategory::Decode,
        PlayerErrorCategory::AudioOutput => PlayerFfiErrorCategory::AudioOutput,
        PlayerErrorCategory::Playback => PlayerFfiErrorCategory::Playback,
        PlayerErrorCategory::Capability => PlayerFfiErrorCategory::Capability,
        PlayerErrorCategory::Platform => PlayerFfiErrorCategory::Platform,
    }
}

impl From<PlayerFfiErrorCode> for PlayerErrorCode {
    fn from(value: PlayerFfiErrorCode) -> Self {
        match value {
            PlayerFfiErrorCode::InvalidState => PlayerErrorCode::InvalidState,
            PlayerFfiErrorCode::InvalidSource => PlayerErrorCode::InvalidSource,
            PlayerFfiErrorCode::BackendFailure => PlayerErrorCode::BackendFailure,
            PlayerFfiErrorCode::AudioOutputUnavailable => PlayerErrorCode::AudioOutputUnavailable,
            PlayerFfiErrorCode::DecodeFailure => PlayerErrorCode::DecodeFailure,
            PlayerFfiErrorCode::SeekFailure => PlayerErrorCode::SeekFailure,
            PlayerFfiErrorCode::Unsupported => PlayerErrorCode::Unsupported,
            PlayerFfiErrorCode::CommandChannelClosed => PlayerErrorCode::CommandChannelClosed,
            PlayerFfiErrorCode::EventChannelClosed => PlayerErrorCode::EventChannelClosed,
            PlayerFfiErrorCode::Cancelled => PlayerErrorCode::Cancelled,
            PlayerFfiErrorCode::Timeout => PlayerErrorCode::Timeout,
            PlayerFfiErrorCode::None
            | PlayerFfiErrorCode::NullPointer
            | PlayerFfiErrorCode::InvalidUtf8
            | PlayerFfiErrorCode::InvalidArgument => PlayerErrorCode::InvalidArgument,
        }
    }
}

impl From<PlayerFfiErrorCategory> for PlayerErrorCategory {
    fn from(value: PlayerFfiErrorCategory) -> Self {
        match value {
            PlayerFfiErrorCategory::Source => PlayerErrorCategory::Source,
            PlayerFfiErrorCategory::Network => PlayerErrorCategory::Network,
            PlayerFfiErrorCategory::Decode => PlayerErrorCategory::Decode,
            PlayerFfiErrorCategory::AudioOutput => PlayerErrorCategory::AudioOutput,
            PlayerFfiErrorCategory::Playback => PlayerErrorCategory::Playback,
            PlayerFfiErrorCategory::Capability => PlayerErrorCategory::Capability,
            PlayerFfiErrorCategory::Platform => PlayerErrorCategory::Platform,
            PlayerFfiErrorCategory::Input => PlayerErrorCategory::Input,
        }
    }
}

pub(crate) fn preload_task_to_ffi(task: PreloadTaskSnapshot) -> PlayerFfiPreloadTask {
    let (scope_kind, scope_id) = match task.scope {
        PreloadBudgetScope::App => (PlayerFfiPreloadScopeKind::App, ptr::null_mut()),
        PreloadBudgetScope::Session(value) => {
            (PlayerFfiPreloadScopeKind::Session, into_c_string_ptr(value))
        }
        PreloadBudgetScope::Playlist(value) => (
            PlayerFfiPreloadScopeKind::Playlist,
            into_c_string_ptr(value),
        ),
    };
    let (has_error, error_code, error_category, error_retriable, error_message) =
        match task.error_summary {
            Some(error) => (
                true,
                error_code_to_ffi(error.code),
                error_category_to_ffi(error.category),
                error.retriable,
                into_c_string_ptr(error.message),
            ),
            None => (
                false,
                PlayerFfiErrorCode::None,
                PlayerFfiErrorCategory::Platform,
                false,
                ptr::null_mut(),
            ),
        };

    PlayerFfiPreloadTask {
        task_id: task.task_id.get(),
        source_uri: into_c_string_ptr(task.source.uri().to_owned()),
        source_identity: into_c_string_ptr(task.source_identity.as_str().to_owned()),
        cache_key: into_c_string_ptr(task.cache_key.as_str().to_owned()),
        scope_kind,
        scope_id,
        candidate_kind: task.kind.into(),
        selection_hint: task.selection_hint.into(),
        priority: task.priority.into(),
        status: task.status.into(),
        expected_memory_bytes: task.expected_memory_bytes,
        expected_disk_bytes: task.expected_disk_bytes,
        warmup_window_ms: duration_to_millis_u64(task.warmup_window),
        has_error,
        error_code,
        error_category,
        error_retriable,
        error_message,
    }
}

pub(crate) fn into_c_string_list(values: Vec<String>) -> (*mut *mut c_char, usize) {
    let len = values.len();
    if len == 0 {
        return (ptr::null_mut(), 0);
    }

    let ptrs = values
        .into_iter()
        .map(into_c_string_ptr)
        .collect::<Vec<_>>()
        .into_boxed_slice();
    (Box::into_raw(ptrs) as *mut *mut c_char, len)
}

pub(crate) fn download_source_to_ffi(source: DownloadSource) -> PlayerFfiDownloadSource {
    let (header_names, header_values): (Vec<_>, Vec<_>) =
        source.request_headers.into_iter().unzip();
    let (header_names, headers_len) = into_c_string_list(header_names);
    let (header_values, header_values_len) = into_c_string_list(header_values);
    debug_assert_eq!(headers_len, header_values_len);
    PlayerFfiDownloadSource {
        source_uri: into_c_string_ptr(source.source.uri().to_owned()),
        content_format: source.content_format.into(),
        manifest_uri: source
            .manifest_uri
            .map(into_c_string_ptr)
            .unwrap_or(ptr::null_mut()),
        header_names,
        header_values,
        headers_len,
    }
}

pub(crate) fn download_profile_to_ffi(profile: DownloadProfile) -> PlayerFfiDownloadProfile {
    let (selected_track_ids, selected_track_ids_len) =
        into_c_string_list(profile.selected_track_ids);
    PlayerFfiDownloadProfile {
        variant_id: profile
            .variant_id
            .map(into_c_string_ptr)
            .unwrap_or(ptr::null_mut()),
        preferred_audio_language: profile
            .preferred_audio_language
            .map(into_c_string_ptr)
            .unwrap_or(ptr::null_mut()),
        preferred_subtitle_language: profile
            .preferred_subtitle_language
            .map(into_c_string_ptr)
            .unwrap_or(ptr::null_mut()),
        selected_track_ids,
        selected_track_ids_len,
        has_target_output_format: profile.target_output_format.is_some(),
        target_output_format: profile
            .target_output_format
            .map(PlayerFfiDownloadOutputFormat::from)
            .unwrap_or_default(),
        target_directory: profile
            .target_directory
            .map(|path| into_c_string_ptr(path.to_string_lossy().into_owned()))
            .unwrap_or(ptr::null_mut()),
        allow_metered_network: profile.allow_metered_network,
    }
}

pub(crate) fn download_resource_record_to_ffi(
    resource: DownloadResourceRecord,
) -> PlayerFfiDownloadResourceRecord {
    PlayerFfiDownloadResourceRecord {
        resource_id: into_c_string_ptr(resource.resource_id),
        uri: into_c_string_ptr(resource.uri),
        relative_path: resource
            .relative_path
            .map(|path| into_c_string_ptr(path.to_string_lossy().into_owned()))
            .unwrap_or(ptr::null_mut()),
        has_byte_range: resource.byte_range.is_some(),
        byte_range: resource
            .byte_range
            .map(PlayerFfiDownloadByteRange::from)
            .unwrap_or_default(),
        generated_text: ptr::null_mut(),
        has_size_bytes: resource.size_bytes.is_some(),
        size_bytes: resource.size_bytes.unwrap_or_default(),
        etag: resource
            .etag
            .map(into_c_string_ptr)
            .unwrap_or(ptr::null_mut()),
        checksum: resource
            .checksum
            .map(into_c_string_ptr)
            .unwrap_or(ptr::null_mut()),
    }
}

pub(crate) fn download_segment_record_to_ffi(
    segment: DownloadSegmentRecord,
) -> PlayerFfiDownloadSegmentRecord {
    PlayerFfiDownloadSegmentRecord {
        segment_id: into_c_string_ptr(segment.segment_id),
        uri: into_c_string_ptr(segment.uri),
        relative_path: segment
            .relative_path
            .map(|path| into_c_string_ptr(path.to_string_lossy().into_owned()))
            .unwrap_or(ptr::null_mut()),
        has_sequence: segment.sequence.is_some(),
        sequence: segment.sequence.unwrap_or_default(),
        has_byte_range: segment.byte_range.is_some(),
        byte_range: segment
            .byte_range
            .map(PlayerFfiDownloadByteRange::from)
            .unwrap_or_default(),
        has_size_bytes: segment.size_bytes.is_some(),
        size_bytes: segment.size_bytes.unwrap_or_default(),
        checksum: segment
            .checksum
            .map(into_c_string_ptr)
            .unwrap_or(ptr::null_mut()),
    }
}

pub(crate) fn download_asset_stream_to_ffi(
    stream: DownloadAssetStream,
) -> PlayerFfiDownloadAssetStream {
    let (resource_ids, resource_ids_len) = into_c_string_list(stream.resource_ids);
    let (segment_ids, segment_ids_len) = into_c_string_list(stream.segment_ids);
    let (metadata_keys, metadata_values): (Vec<_>, Vec<_>) = stream.metadata.into_iter().unzip();
    let (metadata_keys, metadata_len) = into_c_string_list(metadata_keys);
    let (metadata_values, metadata_values_len) = into_c_string_list(metadata_values);
    debug_assert_eq!(metadata_len, metadata_values_len);

    PlayerFfiDownloadAssetStream {
        stream_id: into_c_string_ptr(stream.stream_id),
        kind: stream.kind.into(),
        language: stream
            .language
            .map(into_c_string_ptr)
            .unwrap_or(ptr::null_mut()),
        codec: stream
            .codec
            .map(into_c_string_ptr)
            .unwrap_or(ptr::null_mut()),
        label: stream
            .label
            .map(into_c_string_ptr)
            .unwrap_or(ptr::null_mut()),
        has_quality_rank: stream.quality_rank.is_some(),
        quality_rank: stream.quality_rank.unwrap_or_default(),
        resource_ids,
        resource_ids_len,
        segment_ids,
        segment_ids_len,
        metadata_keys,
        metadata_values,
        metadata_len,
    }
}

pub(crate) fn download_asset_index_to_ffi(
    asset_index: &DownloadAssetIndex,
) -> PlayerFfiDownloadAssetIndex {
    let resources = asset_index
        .resources
        .iter()
        .cloned()
        .map(download_resource_record_to_ffi)
        .collect::<Vec<_>>();
    let resources_len = resources.len();
    let resources = if resources_len == 0 {
        ptr::null_mut()
    } else {
        Box::into_raw(resources.into_boxed_slice()) as *mut PlayerFfiDownloadResourceRecord
    };

    let segments = asset_index
        .segments
        .iter()
        .cloned()
        .map(download_segment_record_to_ffi)
        .collect::<Vec<_>>();
    let segments_len = segments.len();
    let segments = if segments_len == 0 {
        ptr::null_mut()
    } else {
        Box::into_raw(segments.into_boxed_slice()) as *mut PlayerFfiDownloadSegmentRecord
    };

    let streams = asset_index
        .streams
        .iter()
        .cloned()
        .map(download_asset_stream_to_ffi)
        .collect::<Vec<_>>();
    let streams_len = streams.len();
    let streams = if streams_len == 0 {
        ptr::null_mut()
    } else {
        Box::into_raw(streams.into_boxed_slice()) as *mut PlayerFfiDownloadAssetStream
    };

    PlayerFfiDownloadAssetIndex {
        content_format: asset_index.content_format.into(),
        version: asset_index
            .version
            .as_ref()
            .map(|version| into_c_string_ptr(version.clone()))
            .unwrap_or(ptr::null_mut()),
        etag: asset_index
            .etag
            .as_ref()
            .map(|etag| into_c_string_ptr(etag.clone()))
            .unwrap_or(ptr::null_mut()),
        checksum: asset_index
            .checksum
            .as_ref()
            .map(|checksum| into_c_string_ptr(checksum.clone()))
            .unwrap_or(ptr::null_mut()),
        has_total_size_bytes: asset_index.total_size_bytes.is_some(),
        total_size_bytes: asset_index.total_size_bytes.unwrap_or_default(),
        resources,
        resources_len,
        segments,
        segments_len,
        streams,
        streams_len,
        completed_path: asset_index
            .completed_path
            .as_ref()
            .map(|path| into_c_string_ptr(path.to_string_lossy().into_owned()))
            .unwrap_or(ptr::null_mut()),
    }
}

pub(crate) fn into_optional_c_string_path(path: Option<PathBuf>) -> *mut c_char {
    path.map(|path| into_c_string_ptr(path.to_string_lossy().into_owned()))
        .unwrap_or(ptr::null_mut())
}

pub(crate) fn download_progress_to_ffi(
    progress: DownloadProgressSnapshot,
) -> PlayerFfiDownloadProgressSnapshot {
    PlayerFfiDownloadProgressSnapshot {
        received_bytes: progress.received_bytes,
        has_total_bytes: progress.total_bytes.is_some(),
        total_bytes: progress.total_bytes.unwrap_or_default(),
        received_segments: progress.received_segments,
        has_total_segments: progress.total_segments.is_some(),
        total_segments: progress.total_segments.unwrap_or_default(),
    }
}

pub(crate) fn download_task_to_ffi(task: DownloadTaskSnapshot) -> PlayerFfiDownloadTask {
    let (has_error, error_code, error_category, error_retriable, error_message) =
        download_error_to_ffi_fields(task.error_summary);

    PlayerFfiDownloadTask {
        task_id: task.task_id.get(),
        asset_id: into_c_string_ptr(task.asset_id.as_str().to_owned()),
        source: download_source_to_ffi(task.source),
        profile: download_profile_to_ffi(task.profile),
        status: task.status.into(),
        progress: download_progress_to_ffi(task.progress),
        asset_index: download_asset_index_to_ffi(&task.asset_index),
        has_error,
        error_code,
        error_category,
        error_retriable,
        error_message,
    }
}

pub(crate) fn download_error_to_ffi_fields(
    error: Option<DownloadErrorSummary>,
) -> (
    bool,
    PlayerFfiErrorCode,
    PlayerFfiErrorCategory,
    bool,
    *mut c_char,
) {
    match error {
        Some(error) => (
            true,
            error_code_to_ffi(error.code),
            error_category_to_ffi(error.category),
            error.retriable,
            into_c_string_ptr(error.message),
        ),
        None => (
            false,
            PlayerFfiErrorCode::None,
            PlayerFfiErrorCategory::Platform,
            false,
            ptr::null_mut(),
        ),
    }
}

pub(crate) fn playlist_active_item_to_ffi(item: PlaylistActiveItem) -> PlayerFfiPlaylistActiveItem {
    PlayerFfiPlaylistActiveItem {
        item_id: into_c_string_ptr(item.item_id.as_str().to_owned()),
        index: item.index.min(u32::MAX as usize) as u32,
    }
}

pub(crate) fn preload_command_free(command: &mut PlayerFfiPreloadCommand) {
    preload_task_free(&mut command.task);
    *command = PlayerFfiPreloadCommand::default();
}

pub(crate) fn download_command_free(command: &mut PlayerFfiDownloadCommand) {
    download_task_free(&mut command.task);
    *command = PlayerFfiDownloadCommand::default();
}

pub(crate) fn download_event_free(event: &mut PlayerFfiDownloadEvent) {
    download_task_free(&mut event.task);
    free_c_string(&mut event.error_message);
    free_c_string(&mut event.completed_path);
    *event = PlayerFfiDownloadEvent::default();
}

pub(crate) fn preload_task_free(task: &mut PlayerFfiPreloadTask) {
    free_c_string(&mut task.source_uri);
    free_c_string(&mut task.source_identity);
    free_c_string(&mut task.cache_key);
    free_c_string(&mut task.scope_id);
    free_c_string(&mut task.error_message);
    *task = PlayerFfiPreloadTask::default();
}

pub(crate) fn free_c_string_list(values: &mut *mut *mut c_char, len: &mut usize) {
    if !(*values).is_null() && *len > 0 {
        let items = unsafe { Vec::from_raw_parts(*values, *len, *len) };
        for mut value in items {
            free_c_string(&mut value);
        }
    }
    *values = ptr::null_mut();
    *len = 0;
}

pub(crate) fn download_profile_free(profile: &mut PlayerFfiDownloadProfile) {
    free_c_string(&mut profile.variant_id);
    free_c_string(&mut profile.preferred_audio_language);
    free_c_string(&mut profile.preferred_subtitle_language);
    free_c_string_list(
        &mut profile.selected_track_ids,
        &mut profile.selected_track_ids_len,
    );
    free_c_string(&mut profile.target_directory);
    *profile = PlayerFfiDownloadProfile::default();
}

pub(crate) fn download_source_free(source: &mut PlayerFfiDownloadSource) {
    free_c_string(&mut source.source_uri);
    free_c_string(&mut source.manifest_uri);
    let mut header_values_len = source.headers_len;
    free_c_string_list(&mut source.header_names, &mut source.headers_len);
    free_c_string_list(&mut source.header_values, &mut header_values_len);
    *source = PlayerFfiDownloadSource::default();
}

pub(crate) fn download_resource_record_free(resource: &mut PlayerFfiDownloadResourceRecord) {
    free_c_string(&mut resource.resource_id);
    free_c_string(&mut resource.uri);
    free_c_string(&mut resource.relative_path);
    free_c_string(&mut resource.generated_text);
    free_c_string(&mut resource.etag);
    free_c_string(&mut resource.checksum);
    *resource = PlayerFfiDownloadResourceRecord::default();
}

pub(crate) fn download_segment_record_free(segment: &mut PlayerFfiDownloadSegmentRecord) {
    free_c_string(&mut segment.segment_id);
    free_c_string(&mut segment.uri);
    free_c_string(&mut segment.relative_path);
    free_c_string(&mut segment.checksum);
    *segment = PlayerFfiDownloadSegmentRecord::default();
}

pub(crate) fn download_asset_stream_free(stream: &mut PlayerFfiDownloadAssetStream) {
    free_c_string(&mut stream.stream_id);
    free_c_string(&mut stream.language);
    free_c_string(&mut stream.codec);
    free_c_string(&mut stream.label);
    free_c_string_list(&mut stream.resource_ids, &mut stream.resource_ids_len);
    free_c_string_list(&mut stream.segment_ids, &mut stream.segment_ids_len);
    let mut metadata_values_len = stream.metadata_len;
    free_c_string_list(&mut stream.metadata_keys, &mut stream.metadata_len);
    free_c_string_list(&mut stream.metadata_values, &mut metadata_values_len);
    *stream = PlayerFfiDownloadAssetStream::default();
}

pub(crate) fn download_asset_index_free(asset_index: &mut PlayerFfiDownloadAssetIndex) {
    free_c_string(&mut asset_index.version);
    free_c_string(&mut asset_index.etag);
    free_c_string(&mut asset_index.checksum);
    free_c_string(&mut asset_index.completed_path);

    if !asset_index.resources.is_null() && asset_index.resources_len > 0 {
        let resources = unsafe {
            Vec::from_raw_parts(
                asset_index.resources,
                asset_index.resources_len,
                asset_index.resources_len,
            )
        };
        for mut resource in resources {
            download_resource_record_free(&mut resource);
        }
    }
    if !asset_index.segments.is_null() && asset_index.segments_len > 0 {
        let segments = unsafe {
            Vec::from_raw_parts(
                asset_index.segments,
                asset_index.segments_len,
                asset_index.segments_len,
            )
        };
        for mut segment in segments {
            download_segment_record_free(&mut segment);
        }
    }
    if !asset_index.streams.is_null() && asset_index.streams_len > 0 {
        let streams = unsafe {
            Vec::from_raw_parts(
                asset_index.streams,
                asset_index.streams_len,
                asset_index.streams_len,
            )
        };
        for mut stream in streams {
            download_asset_stream_free(&mut stream);
        }
    }
    *asset_index = PlayerFfiDownloadAssetIndex::default();
}

pub(crate) fn download_task_free(task: &mut PlayerFfiDownloadTask) {
    free_c_string(&mut task.asset_id);
    download_source_free(&mut task.source);
    download_profile_free(&mut task.profile);
    download_asset_index_free(&mut task.asset_index);
    free_c_string(&mut task.error_message);
    *task = PlayerFfiDownloadTask::default();
}

pub(crate) fn read_track_preferences(
    preferences: *const PlayerFfiTrackPreferences,
) -> Result<PlayerTrackPreferencePolicy, PlayerFfiError> {
    let Some(preferences) = (unsafe { preferences.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "track preferences pointer was null",
        ));
    };

    Ok(PlayerTrackPreferencePolicy {
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
) -> Result<PlayerBufferingPolicy, PlayerFfiError> {
    let Some(policy) = (unsafe { policy.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "buffering policy pointer was null",
        ));
    };

    Ok(PlayerBufferingPolicy {
        preset: policy.preset.into(),
        min_buffer: policy
            .has_min_buffer_ms
            .then_some(Duration::from_millis(policy.min_buffer_ms)),
        max_buffer: policy
            .has_max_buffer_ms
            .then_some(Duration::from_millis(policy.max_buffer_ms)),
        buffer_for_playback: policy
            .has_buffer_for_playback_ms
            .then_some(Duration::from_millis(policy.buffer_for_playback_ms)),
        buffer_for_rebuffer: policy
            .has_buffer_for_rebuffer_ms
            .then_some(Duration::from_millis(policy.buffer_for_rebuffer_ms)),
    })
}

pub(crate) fn read_retry_policy(
    policy: *const PlayerFfiRetryPolicy,
) -> Result<PlayerRetryPolicy, PlayerFfiError> {
    let Some(policy) = (unsafe { policy.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "retry policy pointer was null",
        ));
    };

    Ok(PlayerRetryPolicy {
        max_attempts: if policy.uses_default_max_attempts {
            Some(3)
        } else if policy.has_max_attempts {
            Some(policy.max_attempts)
        } else {
            None
        },
        base_delay: if policy.has_base_delay_ms {
            Duration::from_millis(policy.base_delay_ms)
        } else {
            Duration::from_millis(1_000)
        },
        max_delay: if policy.has_max_delay_ms {
            Duration::from_millis(policy.max_delay_ms)
        } else {
            Duration::from_millis(5_000)
        },
        backoff: if policy.has_backoff {
            policy.backoff.into()
        } else {
            PlayerRetryBackoff::Linear
        },
    })
}

pub(crate) fn read_cache_policy(
    policy: *const PlayerFfiCachePolicy,
) -> Result<PlayerCachePolicy, PlayerFfiError> {
    let Some(policy) = (unsafe { policy.as_ref() }) else {
        return Err(owned_api_error(
            PlayerFfiErrorCode::NullPointer,
            "cache policy pointer was null",
        ));
    };

    Ok(PlayerCachePolicy {
        preset: policy.preset.into(),
        max_memory_bytes: policy
            .has_max_memory_bytes
            .then_some(policy.max_memory_bytes),
        max_disk_bytes: policy.has_max_disk_bytes.then_some(policy.max_disk_bytes),
    })
}

pub(crate) fn owned_api_error(code: PlayerFfiErrorCode, message: &str) -> PlayerFfiError {
    PlayerFfiError {
        code,
        category: api_error_category(code),
        retriable: false,
        message: into_c_string_ptr(message.to_owned()),
    }
}

pub(crate) fn api_error_category(code: PlayerFfiErrorCode) -> PlayerFfiErrorCategory {
    match code {
        PlayerFfiErrorCode::NullPointer
        | PlayerFfiErrorCode::InvalidUtf8
        | PlayerFfiErrorCode::InvalidArgument => PlayerFfiErrorCategory::Input,
        PlayerFfiErrorCode::InvalidState
        | PlayerFfiErrorCode::SeekFailure
        | PlayerFfiErrorCode::CommandChannelClosed
        | PlayerFfiErrorCode::EventChannelClosed
        | PlayerFfiErrorCode::Cancelled
        | PlayerFfiErrorCode::Timeout => PlayerFfiErrorCategory::Playback,
        PlayerFfiErrorCode::InvalidSource => PlayerFfiErrorCategory::Source,
        PlayerFfiErrorCode::AudioOutputUnavailable => PlayerFfiErrorCategory::AudioOutput,
        PlayerFfiErrorCode::DecodeFailure => PlayerFfiErrorCategory::Decode,
        PlayerFfiErrorCode::Unsupported => PlayerFfiErrorCategory::Capability,
        PlayerFfiErrorCode::BackendFailure | PlayerFfiErrorCode::None => {
            PlayerFfiErrorCategory::Platform
        }
    }
}

pub(crate) fn into_c_string_ptr(value: String) -> *mut c_char {
    CString::new(value).unwrap_or_default().into_raw()
}

pub(crate) fn free_c_string(value: &mut *mut c_char) {
    if value.is_null() {
        return;
    }

    unsafe {
        let raw = ptr::replace(value, ptr::null_mut());
        if !raw.is_null() {
            let _ = CString::from_raw(raw);
        }
    }
}

pub(crate) fn ffi_call(
    out_error: *mut PlayerFfiError,
    f: impl FnOnce() -> PlayerFfiCallStatus,
) -> PlayerFfiCallStatus {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(status) => {
            if status == PlayerFfiCallStatus::Ok {
                write_success(out_error);
            }
            status
        }
        Err(payload) => {
            write_error(out_error, owned_panic_error(payload));
            PlayerFfiCallStatus::Error
        }
    }
}

pub(crate) fn ffi_void(f: impl FnOnce()) {
    let _ = catch_unwind(AssertUnwindSafe(f));
}

pub(crate) fn owned_panic_error(payload: Box<dyn Any + Send>) -> PlayerFfiError {
    let message = panic_payload_message(payload.as_ref());
    owned_api_error(
        PlayerFfiErrorCode::BackendFailure,
        &format!("player_ffi caught Rust panic: {message}"),
    )
}

pub(crate) fn panic_payload_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        return (*message).to_owned();
    }

    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }

    "unknown panic payload".to_owned()
}

pub(crate) fn write_error(out_error: *mut PlayerFfiError, mut error: PlayerFfiError) {
    if out_error.is_null() {
        free_c_string(&mut error.message);
        return;
    }

    unsafe {
        ptr::write(out_error, error);
    }
}

pub(crate) fn write_success(out_error: *mut PlayerFfiError) {
    if out_error.is_null() {
        return;
    }

    unsafe {
        ptr::write(out_error, PlayerFfiError::default());
    }
}

impl From<PlayerFfiMediaSourceKind> for MediaSourceKind {
    fn from(value: PlayerFfiMediaSourceKind) -> Self {
        match value {
            PlayerFfiMediaSourceKind::Local => Self::Local,
            PlayerFfiMediaSourceKind::Remote => Self::Remote,
        }
    }
}

impl From<PlayerFfiMediaSourceProtocol> for MediaSourceProtocol {
    fn from(value: PlayerFfiMediaSourceProtocol) -> Self {
        match value {
            PlayerFfiMediaSourceProtocol::Unknown => Self::Unknown,
            PlayerFfiMediaSourceProtocol::File => Self::File,
            PlayerFfiMediaSourceProtocol::Content => Self::Content,
            PlayerFfiMediaSourceProtocol::Progressive => Self::Progressive,
            PlayerFfiMediaSourceProtocol::Hls => Self::Hls,
            PlayerFfiMediaSourceProtocol::Dash => Self::Dash,
        }
    }
}

impl From<PlayerFfiBufferingPreset> for PlayerBufferingPreset {
    fn from(value: PlayerFfiBufferingPreset) -> Self {
        match value {
            PlayerFfiBufferingPreset::Default => Self::Default,
            PlayerFfiBufferingPreset::Balanced => Self::Balanced,
            PlayerFfiBufferingPreset::Streaming => Self::Streaming,
            PlayerFfiBufferingPreset::Resilient => Self::Resilient,
            PlayerFfiBufferingPreset::LowLatency => Self::LowLatency,
        }
    }
}

impl From<PlayerBufferingPreset> for PlayerFfiBufferingPreset {
    fn from(value: PlayerBufferingPreset) -> Self {
        match value {
            PlayerBufferingPreset::Default => Self::Default,
            PlayerBufferingPreset::Balanced => Self::Balanced,
            PlayerBufferingPreset::Streaming => Self::Streaming,
            PlayerBufferingPreset::Resilient => Self::Resilient,
            PlayerBufferingPreset::LowLatency => Self::LowLatency,
        }
    }
}

impl From<PlayerFfiRetryBackoff> for PlayerRetryBackoff {
    fn from(value: PlayerFfiRetryBackoff) -> Self {
        match value {
            PlayerFfiRetryBackoff::Fixed => Self::Fixed,
            PlayerFfiRetryBackoff::Linear => Self::Linear,
            PlayerFfiRetryBackoff::Exponential => Self::Exponential,
        }
    }
}

impl From<PlayerRetryBackoff> for PlayerFfiRetryBackoff {
    fn from(value: PlayerRetryBackoff) -> Self {
        match value {
            PlayerRetryBackoff::Fixed => Self::Fixed,
            PlayerRetryBackoff::Linear => Self::Linear,
            PlayerRetryBackoff::Exponential => Self::Exponential,
        }
    }
}

impl From<PlayerFfiCachePreset> for PlayerCachePreset {
    fn from(value: PlayerFfiCachePreset) -> Self {
        match value {
            PlayerFfiCachePreset::Default => Self::Default,
            PlayerFfiCachePreset::Disabled => Self::Disabled,
            PlayerFfiCachePreset::Streaming => Self::Streaming,
            PlayerFfiCachePreset::Resilient => Self::Resilient,
        }
    }
}

impl From<PlayerCachePreset> for PlayerFfiCachePreset {
    fn from(value: PlayerCachePreset) -> Self {
        match value {
            PlayerCachePreset::Default => Self::Default,
            PlayerCachePreset::Disabled => Self::Disabled,
            PlayerCachePreset::Streaming => Self::Streaming,
            PlayerCachePreset::Resilient => Self::Resilient,
        }
    }
}

impl From<PlayerFfiTrackSelectionMode> for MediaTrackSelectionMode {
    fn from(value: PlayerFfiTrackSelectionMode) -> Self {
        match value {
            PlayerFfiTrackSelectionMode::Auto => Self::Auto,
            PlayerFfiTrackSelectionMode::Disabled => Self::Disabled,
            PlayerFfiTrackSelectionMode::Track => Self::Track,
        }
    }
}

impl From<MediaTrackSelectionMode> for PlayerFfiTrackSelectionMode {
    fn from(value: MediaTrackSelectionMode) -> Self {
        match value {
            MediaTrackSelectionMode::Auto => Self::Auto,
            MediaTrackSelectionMode::Disabled => Self::Disabled,
            MediaTrackSelectionMode::Track => Self::Track,
        }
    }
}

impl From<PlayerFfiAbrMode> for MediaAbrMode {
    fn from(value: PlayerFfiAbrMode) -> Self {
        match value {
            PlayerFfiAbrMode::Auto => Self::Auto,
            PlayerFfiAbrMode::Constrained => Self::Constrained,
            PlayerFfiAbrMode::FixedTrack => Self::FixedTrack,
        }
    }
}

impl From<PlayerFfiPreloadCandidateKind> for PreloadCandidateKind {
    fn from(value: PlayerFfiPreloadCandidateKind) -> Self {
        match value {
            PlayerFfiPreloadCandidateKind::Current => Self::Current,
            PlayerFfiPreloadCandidateKind::Neighbor => Self::Neighbor,
            PlayerFfiPreloadCandidateKind::Recommended => Self::Recommended,
            PlayerFfiPreloadCandidateKind::Background => Self::Background,
        }
    }
}

impl From<PlayerFfiPreloadSelectionHint> for PreloadSelectionHint {
    fn from(value: PlayerFfiPreloadSelectionHint) -> Self {
        match value {
            PlayerFfiPreloadSelectionHint::None => Self::None,
            PlayerFfiPreloadSelectionHint::CurrentItem => Self::CurrentItem,
            PlayerFfiPreloadSelectionHint::NeighborItem => Self::NeighborItem,
            PlayerFfiPreloadSelectionHint::RecommendedItem => Self::RecommendedItem,
            PlayerFfiPreloadSelectionHint::BackgroundFill => Self::BackgroundFill,
        }
    }
}

impl From<PlayerFfiPreloadPriority> for PreloadPriority {
    fn from(value: PlayerFfiPreloadPriority) -> Self {
        match value {
            PlayerFfiPreloadPriority::Critical => Self::Critical,
            PlayerFfiPreloadPriority::High => Self::High,
            PlayerFfiPreloadPriority::Normal => Self::Normal,
            PlayerFfiPreloadPriority::Low => Self::Low,
            PlayerFfiPreloadPriority::Background => Self::Background,
        }
    }
}

impl From<player_runtime::PreloadCandidateKind> for PlayerFfiPreloadCandidateKind {
    fn from(value: player_runtime::PreloadCandidateKind) -> Self {
        match value {
            player_runtime::PreloadCandidateKind::Current => Self::Current,
            player_runtime::PreloadCandidateKind::Neighbor => Self::Neighbor,
            player_runtime::PreloadCandidateKind::Recommended => Self::Recommended,
            player_runtime::PreloadCandidateKind::Background => Self::Background,
        }
    }
}

impl From<player_runtime::PreloadSelectionHint> for PlayerFfiPreloadSelectionHint {
    fn from(value: player_runtime::PreloadSelectionHint) -> Self {
        match value {
            player_runtime::PreloadSelectionHint::None => Self::None,
            player_runtime::PreloadSelectionHint::CurrentItem => Self::CurrentItem,
            player_runtime::PreloadSelectionHint::NeighborItem => Self::NeighborItem,
            player_runtime::PreloadSelectionHint::RecommendedItem => Self::RecommendedItem,
            player_runtime::PreloadSelectionHint::BackgroundFill => Self::BackgroundFill,
        }
    }
}

impl From<player_runtime::PreloadPriority> for PlayerFfiPreloadPriority {
    fn from(value: player_runtime::PreloadPriority) -> Self {
        match value {
            player_runtime::PreloadPriority::Critical => Self::Critical,
            player_runtime::PreloadPriority::High => Self::High,
            player_runtime::PreloadPriority::Normal => Self::Normal,
            player_runtime::PreloadPriority::Low => Self::Low,
            player_runtime::PreloadPriority::Background => Self::Background,
        }
    }
}

impl From<player_runtime::PreloadTaskStatus> for PlayerFfiPreloadTaskStatus {
    fn from(value: player_runtime::PreloadTaskStatus) -> Self {
        match value {
            player_runtime::PreloadTaskStatus::Planned => Self::Planned,
            player_runtime::PreloadTaskStatus::Active => Self::Active,
            player_runtime::PreloadTaskStatus::Cancelled => Self::Cancelled,
            player_runtime::PreloadTaskStatus::Completed => Self::Completed,
            player_runtime::PreloadTaskStatus::Expired => Self::Expired,
            player_runtime::PreloadTaskStatus::Failed => Self::Failed,
        }
    }
}

impl From<PlayerFfiDownloadContentFormat> for DownloadContentFormat {
    fn from(value: PlayerFfiDownloadContentFormat) -> Self {
        match value {
            PlayerFfiDownloadContentFormat::HlsSegments => Self::HlsSegments,
            PlayerFfiDownloadContentFormat::DashSegments => Self::DashSegments,
            PlayerFfiDownloadContentFormat::FlvSegments => Self::FlvSegments,
            PlayerFfiDownloadContentFormat::SingleFile => Self::SingleFile,
            PlayerFfiDownloadContentFormat::Unknown => Self::Unknown,
        }
    }
}

impl From<DownloadContentFormat> for PlayerFfiDownloadContentFormat {
    fn from(value: DownloadContentFormat) -> Self {
        match value {
            DownloadContentFormat::HlsSegments => Self::HlsSegments,
            DownloadContentFormat::DashSegments => Self::DashSegments,
            DownloadContentFormat::FlvSegments => Self::FlvSegments,
            DownloadContentFormat::SingleFile => Self::SingleFile,
            DownloadContentFormat::Unknown => Self::Unknown,
        }
    }
}

impl From<PlayerFfiDownloadOutputFormat> for OutputFormat {
    fn from(value: PlayerFfiDownloadOutputFormat) -> Self {
        match value {
            PlayerFfiDownloadOutputFormat::Mp4 => Self::Mp4,
            PlayerFfiDownloadOutputFormat::Mkv => Self::Mkv,
            PlayerFfiDownloadOutputFormat::Original => Self::Original,
        }
    }
}

impl From<OutputFormat> for PlayerFfiDownloadOutputFormat {
    fn from(value: OutputFormat) -> Self {
        match value {
            OutputFormat::Mp4 => Self::Mp4,
            OutputFormat::Mkv => Self::Mkv,
            OutputFormat::Original => Self::Original,
        }
    }
}

impl From<PlayerFfiDownloadStreamKind> for DownloadStreamKind {
    fn from(value: PlayerFfiDownloadStreamKind) -> Self {
        match value {
            PlayerFfiDownloadStreamKind::Combined => Self::Combined,
            PlayerFfiDownloadStreamKind::Video => Self::Video,
            PlayerFfiDownloadStreamKind::Audio => Self::Audio,
            PlayerFfiDownloadStreamKind::SecondaryAudio => Self::SecondaryAudio,
            PlayerFfiDownloadStreamKind::Subtitle => Self::Subtitle,
            PlayerFfiDownloadStreamKind::Auxiliary => Self::Auxiliary,
        }
    }
}

impl From<DownloadStreamKind> for PlayerFfiDownloadStreamKind {
    fn from(value: DownloadStreamKind) -> Self {
        match value {
            DownloadStreamKind::Combined => Self::Combined,
            DownloadStreamKind::Video => Self::Video,
            DownloadStreamKind::Audio => Self::Audio,
            DownloadStreamKind::SecondaryAudio => Self::SecondaryAudio,
            DownloadStreamKind::Subtitle => Self::Subtitle,
            DownloadStreamKind::Auxiliary => Self::Auxiliary,
        }
    }
}

impl From<DownloadByteRange> for PlayerFfiDownloadByteRange {
    fn from(value: DownloadByteRange) -> Self {
        Self {
            offset: value.offset,
            length: value.length,
        }
    }
}

impl From<DownloadTaskStatus> for PlayerFfiDownloadTaskStatus {
    fn from(value: DownloadTaskStatus) -> Self {
        match value {
            DownloadTaskStatus::Queued => Self::Queued,
            DownloadTaskStatus::Preparing => Self::Preparing,
            DownloadTaskStatus::Downloading => Self::Downloading,
            DownloadTaskStatus::Paused => Self::Paused,
            DownloadTaskStatus::Completed => Self::Completed,
            DownloadTaskStatus::Failed => Self::Failed,
            DownloadTaskStatus::Removed => Self::Removed,
        }
    }
}

impl From<PlayerFfiDownloadTaskStatus> for DownloadTaskStatus {
    fn from(value: PlayerFfiDownloadTaskStatus) -> Self {
        match value {
            PlayerFfiDownloadTaskStatus::Queued => Self::Queued,
            PlayerFfiDownloadTaskStatus::Preparing => Self::Preparing,
            PlayerFfiDownloadTaskStatus::Downloading => Self::Downloading,
            PlayerFfiDownloadTaskStatus::Paused => Self::Paused,
            PlayerFfiDownloadTaskStatus::Completed => Self::Completed,
            PlayerFfiDownloadTaskStatus::Failed => Self::Failed,
            PlayerFfiDownloadTaskStatus::Removed => Self::Removed,
        }
    }
}

impl From<IosPreloadCommand> for PlayerFfiPreloadCommand {
    fn from(value: IosPreloadCommand) -> Self {
        match value {
            IosPreloadCommand::Start { task } => Self {
                kind: PlayerFfiPreloadCommandKind::Start,
                task: preload_task_to_ffi(task),
                task_id: 0,
            },
            IosPreloadCommand::Cancel { task_id } => Self {
                kind: PlayerFfiPreloadCommandKind::Cancel,
                task: PlayerFfiPreloadTask::default(),
                task_id: task_id.get(),
            },
        }
    }
}

impl From<IosDownloadCommand> for PlayerFfiDownloadCommand {
    fn from(value: IosDownloadCommand) -> Self {
        match value {
            IosDownloadCommand::Prepare { task } => Self {
                kind: PlayerFfiDownloadCommandKind::Prepare,
                task: download_task_to_ffi(task),
                task_id: 0,
            },
            IosDownloadCommand::Start { task } => Self {
                kind: PlayerFfiDownloadCommandKind::Start,
                task: download_task_to_ffi(task),
                task_id: 0,
            },
            IosDownloadCommand::Pause { task_id } => Self {
                kind: PlayerFfiDownloadCommandKind::Pause,
                task: PlayerFfiDownloadTask::default(),
                task_id: task_id.get(),
            },
            IosDownloadCommand::Resume { task } => Self {
                kind: PlayerFfiDownloadCommandKind::Resume,
                task: download_task_to_ffi(task),
                task_id: 0,
            },
            IosDownloadCommand::Remove { task_id } => Self {
                kind: PlayerFfiDownloadCommandKind::Remove,
                task: PlayerFfiDownloadTask::default(),
                task_id: task_id.get(),
            },
        }
    }
}

impl From<DownloadEvent> for PlayerFfiDownloadEvent {
    fn from(value: DownloadEvent) -> Self {
        match value {
            DownloadEvent::Created(task) => {
                let task_id = task.task_id.get();
                Self {
                    kind: PlayerFfiDownloadEventKind::Created,
                    task: download_task_to_ffi(task),
                    task_id,
                    ..Self::default()
                }
            }
            DownloadEvent::StateChanged(patch) => {
                let (has_error, error_code, error_category, error_retriable, error_message) =
                    download_error_to_ffi_fields(patch.error_summary);
                Self {
                    kind: PlayerFfiDownloadEventKind::StateChanged,
                    task_id: patch.task_id.get(),
                    status: patch.status.into(),
                    progress: download_progress_to_ffi(patch.progress),
                    has_error,
                    error_code,
                    error_category,
                    error_retriable,
                    error_message,
                    completed_path: into_optional_c_string_path(patch.completed_path),
                    ..Self::default()
                }
            }
            DownloadEvent::AssetIndexUpdated(task) => {
                let task_id = task.task_id.get();
                Self {
                    kind: PlayerFfiDownloadEventKind::AssetIndexUpdated,
                    task: download_task_to_ffi(task),
                    task_id,
                    ..Self::default()
                }
            }
            DownloadEvent::ProgressUpdated(patch) => Self {
                kind: PlayerFfiDownloadEventKind::ProgressUpdated,
                task_id: patch.task_id.get(),
                progress: download_progress_to_ffi(patch.progress),
                ..Self::default()
            },
        }
    }
}

impl From<MediaAbrMode> for PlayerFfiAbrMode {
    fn from(value: MediaAbrMode) -> Self {
        match value {
            MediaAbrMode::Auto => Self::Auto,
            MediaAbrMode::Constrained => Self::Constrained,
            MediaAbrMode::FixedTrack => Self::FixedTrack,
        }
    }
}

impl From<player_runtime::PlayerResolvedResiliencePolicy> for PlayerFfiResolvedResiliencePolicy {
    fn from(value: player_runtime::PlayerResolvedResiliencePolicy) -> Self {
        Self {
            buffering: PlayerFfiBufferingPolicy {
                preset: value.buffering_policy.preset.into(),
                has_min_buffer_ms: value.buffering_policy.min_buffer.is_some(),
                min_buffer_ms: value
                    .buffering_policy
                    .min_buffer
                    .map(duration_to_millis_u64)
                    .unwrap_or_default(),
                has_max_buffer_ms: value.buffering_policy.max_buffer.is_some(),
                max_buffer_ms: value
                    .buffering_policy
                    .max_buffer
                    .map(duration_to_millis_u64)
                    .unwrap_or_default(),
                has_buffer_for_playback_ms: value.buffering_policy.buffer_for_playback.is_some(),
                buffer_for_playback_ms: value
                    .buffering_policy
                    .buffer_for_playback
                    .map(duration_to_millis_u64)
                    .unwrap_or_default(),
                has_buffer_for_rebuffer_ms: value.buffering_policy.buffer_for_rebuffer.is_some(),
                buffer_for_rebuffer_ms: value
                    .buffering_policy
                    .buffer_for_rebuffer
                    .map(duration_to_millis_u64)
                    .unwrap_or_default(),
            },
            retry: PlayerFfiRetryPolicy {
                uses_default_max_attempts: value.retry_policy.max_attempts == Some(3),
                has_max_attempts: value.retry_policy.max_attempts.is_some(),
                max_attempts: value.retry_policy.max_attempts.unwrap_or_default(),
                has_base_delay_ms: true,
                base_delay_ms: duration_to_millis_u64(value.retry_policy.base_delay),
                has_max_delay_ms: true,
                max_delay_ms: duration_to_millis_u64(value.retry_policy.max_delay),
                has_backoff: true,
                backoff: value.retry_policy.backoff.into(),
            },
            cache: PlayerFfiCachePolicy {
                preset: value.cache_policy.preset.into(),
                has_max_memory_bytes: value.cache_policy.max_memory_bytes.is_some(),
                max_memory_bytes: value.cache_policy.max_memory_bytes.unwrap_or_default(),
                has_max_disk_bytes: value.cache_policy.max_disk_bytes.is_some(),
                max_disk_bytes: value.cache_policy.max_disk_bytes.unwrap_or_default(),
            },
        }
    }
}

impl From<player_runtime::PlayerResolvedPreloadBudgetPolicy>
    for PlayerFfiResolvedPreloadBudgetPolicy
{
    fn from(value: player_runtime::PlayerResolvedPreloadBudgetPolicy) -> Self {
        Self {
            max_concurrent_tasks: value.max_concurrent_tasks,
            max_memory_bytes: value.max_memory_bytes,
            max_disk_bytes: value.max_disk_bytes,
            warmup_window_ms: duration_to_millis_u64(value.warmup_window),
        }
    }
}

impl From<PlayerTrackPreferencePolicy> for PlayerFfiTrackPreferences {
    fn from(value: PlayerTrackPreferencePolicy) -> Self {
        Self {
            preferred_audio_language: value
                .preferred_audio_language
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            preferred_subtitle_language: value
                .preferred_subtitle_language
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            select_subtitles_by_default: value.select_subtitles_by_default,
            select_undetermined_subtitle_language: value.select_undetermined_subtitle_language,
            audio_selection: value.audio_selection.into(),
            subtitle_selection: value.subtitle_selection.into(),
            abr_policy: value.abr_policy.into(),
        }
    }
}

impl From<MediaTrackSelection> for PlayerFfiTrackSelection {
    fn from(value: MediaTrackSelection) -> Self {
        Self {
            mode: value.mode.into(),
            track_id: value
                .track_id
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
        }
    }
}

impl From<MediaAbrPolicy> for PlayerFfiAbrPolicy {
    fn from(value: MediaAbrPolicy) -> Self {
        Self {
            mode: value.mode.into(),
            track_id: value
                .track_id
                .map(into_c_string_ptr)
                .unwrap_or(ptr::null_mut()),
            has_max_bit_rate: value.max_bit_rate.is_some(),
            max_bit_rate: value.max_bit_rate.unwrap_or_default(),
            has_max_width: value.max_width.is_some(),
            max_width: value.max_width.unwrap_or_default(),
            has_max_height: value.max_height.is_some(),
            max_height: value.max_height.unwrap_or_default(),
        }
    }
}

pub(crate) fn duration_to_millis_u64(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}
