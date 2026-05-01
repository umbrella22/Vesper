#include "include/VesperPlayerKitBridgeShim.h"

#include <stdbool.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>

typedef enum PlayerFfiCallStatus {
  PlayerFfiCallStatusOk = 0,
  PlayerFfiCallStatusError = 1,
} PlayerFfiCallStatus;

typedef enum PlayerFfiMediaSourceKind {
  PlayerFfiMediaSourceKindLocal = 0,
  PlayerFfiMediaSourceKindRemote = 1,
} PlayerFfiMediaSourceKind;

typedef enum PlayerFfiMediaSourceProtocol {
  PlayerFfiMediaSourceProtocolUnknown = 0,
  PlayerFfiMediaSourceProtocolFile = 1,
  PlayerFfiMediaSourceProtocolContent = 2,
  PlayerFfiMediaSourceProtocolProgressive = 3,
  PlayerFfiMediaSourceProtocolHls = 4,
  PlayerFfiMediaSourceProtocolDash = 5,
} PlayerFfiMediaSourceProtocol;

typedef enum PlayerFfiBufferingPreset {
  PlayerFfiBufferingPresetDefault = 0,
  PlayerFfiBufferingPresetBalanced = 1,
  PlayerFfiBufferingPresetStreaming = 2,
  PlayerFfiBufferingPresetResilient = 3,
  PlayerFfiBufferingPresetLowLatency = 4,
} PlayerFfiBufferingPreset;

typedef enum PlayerFfiRetryBackoff {
  PlayerFfiRetryBackoffFixed = 0,
  PlayerFfiRetryBackoffLinear = 1,
  PlayerFfiRetryBackoffExponential = 2,
} PlayerFfiRetryBackoff;

typedef enum PlayerFfiCachePreset {
  PlayerFfiCachePresetDefault = 0,
  PlayerFfiCachePresetDisabled = 1,
  PlayerFfiCachePresetStreaming = 2,
  PlayerFfiCachePresetResilient = 3,
} PlayerFfiCachePreset;

typedef enum PlayerFfiTrackSelectionMode {
  PlayerFfiTrackSelectionModeAuto = 0,
  PlayerFfiTrackSelectionModeDisabled = 1,
  PlayerFfiTrackSelectionModeTrack = 2,
} PlayerFfiTrackSelectionMode;

typedef enum PlayerFfiAbrMode {
  PlayerFfiAbrModeAuto = 0,
  PlayerFfiAbrModeConstrained = 1,
  PlayerFfiAbrModeFixedTrack = 2,
} PlayerFfiAbrMode;

typedef struct PlayerFfiBufferingPolicy {
  PlayerFfiBufferingPreset preset;
  bool has_min_buffer_ms;
  uint64_t min_buffer_ms;
  bool has_max_buffer_ms;
  uint64_t max_buffer_ms;
  bool has_buffer_for_playback_ms;
  uint64_t buffer_for_playback_ms;
  bool has_buffer_for_rebuffer_ms;
  uint64_t buffer_for_rebuffer_ms;
} PlayerFfiBufferingPolicy;

typedef struct PlayerFfiRetryPolicy {
  bool uses_default_max_attempts;
  bool has_max_attempts;
  uint32_t max_attempts;
  bool has_base_delay_ms;
  uint64_t base_delay_ms;
  bool has_max_delay_ms;
  uint64_t max_delay_ms;
  bool has_backoff;
  PlayerFfiRetryBackoff backoff;
} PlayerFfiRetryPolicy;

typedef struct PlayerFfiCachePolicy {
  PlayerFfiCachePreset preset;
  bool has_max_memory_bytes;
  uint64_t max_memory_bytes;
  bool has_max_disk_bytes;
  uint64_t max_disk_bytes;
} PlayerFfiCachePolicy;

typedef struct PlayerFfiResolvedResiliencePolicy {
  PlayerFfiBufferingPolicy buffering;
  PlayerFfiRetryPolicy retry;
  PlayerFfiCachePolicy cache;
} PlayerFfiResolvedResiliencePolicy;

typedef struct PlayerFfiPreloadBudgetPolicy {
  bool has_max_concurrent_tasks;
  uint32_t max_concurrent_tasks;
  bool has_max_memory_bytes;
  uint64_t max_memory_bytes;
  bool has_max_disk_bytes;
  uint64_t max_disk_bytes;
  bool has_warmup_window_ms;
  uint64_t warmup_window_ms;
} PlayerFfiPreloadBudgetPolicy;

typedef struct PlayerFfiResolvedPreloadBudgetPolicy {
  uint32_t max_concurrent_tasks;
  uint64_t max_memory_bytes;
  uint64_t max_disk_bytes;
  uint64_t warmup_window_ms;
} PlayerFfiResolvedPreloadBudgetPolicy;

typedef struct PlayerFfiTrackSelection {
  PlayerFfiTrackSelectionMode mode;
  char *track_id;
} PlayerFfiTrackSelection;

typedef struct PlayerFfiAbrPolicy {
  PlayerFfiAbrMode mode;
  char *track_id;
  bool has_max_bit_rate;
  uint64_t max_bit_rate;
  bool has_max_width;
  uint32_t max_width;
  bool has_max_height;
  uint32_t max_height;
} PlayerFfiAbrPolicy;

typedef struct PlayerFfiTrackPreferences {
  char *preferred_audio_language;
  char *preferred_subtitle_language;
  bool select_subtitles_by_default;
  bool select_undetermined_subtitle_language;
  PlayerFfiTrackSelection audio_selection;
  PlayerFfiTrackSelection subtitle_selection;
  PlayerFfiAbrPolicy abr_policy;
} PlayerFfiTrackPreferences;

typedef struct PlayerFfiError {
  int code;
  int category;
  bool retriable;
  char *message;
} PlayerFfiError;

extern PlayerFfiCallStatus player_ffi_resolve_resilience_policy(
    PlayerFfiMediaSourceKind source_kind,
    PlayerFfiMediaSourceProtocol source_protocol,
    const PlayerFfiBufferingPolicy *buffering_policy,
    const PlayerFfiRetryPolicy *retry_policy,
    const PlayerFfiCachePolicy *cache_policy,
    PlayerFfiResolvedResiliencePolicy *out_policy,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_resolve_preload_budget(
    const PlayerFfiPreloadBudgetPolicy *preload_budget,
    PlayerFfiResolvedPreloadBudgetPolicy *out_budget,
    PlayerFfiError *out_error);

typedef struct PlayerFfiPreloadCandidate {
  const char *source_uri;
  int scope_kind;
  const char *scope_id;
  int candidate_kind;
  int selection_hint;
  int priority;
  uint64_t expected_memory_bytes;
  uint64_t expected_disk_bytes;
  bool has_ttl_ms;
  uint64_t ttl_ms;
  bool has_warmup_window_ms;
  uint64_t warmup_window_ms;
} PlayerFfiPreloadCandidate;

typedef struct PlayerFfiPreloadTask {
  uint64_t task_id;
  char *source_uri;
  char *source_identity;
  char *cache_key;
  int scope_kind;
  char *scope_id;
  int candidate_kind;
  int selection_hint;
  int priority;
  int status;
  uint64_t expected_memory_bytes;
  uint64_t expected_disk_bytes;
  uint64_t warmup_window_ms;
  bool has_error;
  uint32_t error_code;
  uint32_t error_category;
  bool error_retriable;
  char *error_message;
} PlayerFfiPreloadTask;

typedef struct PlayerFfiPreloadCommand {
  int kind;
  PlayerFfiPreloadTask task;
  uint64_t task_id;
} PlayerFfiPreloadCommand;

typedef struct PlayerFfiPreloadCommandList {
  PlayerFfiPreloadCommand *commands;
  uintptr_t len;
} PlayerFfiPreloadCommandList;

typedef enum PlayerFfiPlaylistRepeatMode {
  PlayerFfiPlaylistRepeatModeOff = 0,
  PlayerFfiPlaylistRepeatModeOne = 1,
  PlayerFfiPlaylistRepeatModeAll = 2,
} PlayerFfiPlaylistRepeatMode;

typedef enum PlayerFfiPlaylistFailureStrategy {
  PlayerFfiPlaylistFailureStrategyPause = 0,
  PlayerFfiPlaylistFailureStrategySkipToNext = 1,
} PlayerFfiPlaylistFailureStrategy;

typedef enum PlayerFfiPlaylistViewportHintKind {
  PlayerFfiPlaylistViewportHintKindVisible = 0,
  PlayerFfiPlaylistViewportHintKindNearVisible = 1,
  PlayerFfiPlaylistViewportHintKindPrefetchOnly = 2,
  PlayerFfiPlaylistViewportHintKindHidden = 3,
} PlayerFfiPlaylistViewportHintKind;

typedef struct PlayerFfiPlaylistConfig {
  const char *playlist_id;
  uint32_t neighbor_previous;
  uint32_t neighbor_next;
  uint32_t preload_near_visible;
  uint32_t preload_prefetch_only;
  bool auto_advance;
  PlayerFfiPlaylistRepeatMode repeat_mode;
  PlayerFfiPlaylistFailureStrategy failure_strategy;
} PlayerFfiPlaylistConfig;

typedef struct PlayerFfiPlaylistQueueItem {
  const char *item_id;
  const char *source_uri;
  uint64_t expected_memory_bytes;
  uint64_t expected_disk_bytes;
  bool has_ttl_ms;
  uint64_t ttl_ms;
  bool has_warmup_window_ms;
  uint64_t warmup_window_ms;
} PlayerFfiPlaylistQueueItem;

typedef struct PlayerFfiPlaylistViewportHint {
  const char *item_id;
  PlayerFfiPlaylistViewportHintKind kind;
  uint32_t order;
} PlayerFfiPlaylistViewportHint;

typedef struct PlayerFfiPlaylistActiveItem {
  char *item_id;
  uint32_t index;
} PlayerFfiPlaylistActiveItem;

typedef struct PlayerFfiDownloadConfig {
  bool auto_start;
  bool run_post_processors_on_completion;
  char **plugin_library_paths;
  uintptr_t plugin_library_paths_len;
} PlayerFfiDownloadConfig;

typedef enum PlayerFfiDownloadContentFormat {
  PlayerFfiDownloadContentFormatHlsSegments = 0,
  PlayerFfiDownloadContentFormatDashSegments = 1,
  PlayerFfiDownloadContentFormatSingleFile = 2,
  PlayerFfiDownloadContentFormatUnknown = 3,
} PlayerFfiDownloadContentFormat;

typedef struct PlayerFfiDownloadSource {
  char *source_uri;
  PlayerFfiDownloadContentFormat content_format;
  char *manifest_uri;
} PlayerFfiDownloadSource;

typedef struct PlayerFfiDownloadProfile {
  char *variant_id;
  char *preferred_audio_language;
  char *preferred_subtitle_language;
  char **selected_track_ids;
  uintptr_t selected_track_ids_len;
  char *target_directory;
  bool allow_metered_network;
} PlayerFfiDownloadProfile;

typedef struct PlayerFfiDownloadResourceRecord {
  char *resource_id;
  char *uri;
  char *relative_path;
  bool has_size_bytes;
  uint64_t size_bytes;
  char *etag;
  char *checksum;
} PlayerFfiDownloadResourceRecord;

typedef struct PlayerFfiDownloadSegmentRecord {
  char *segment_id;
  char *uri;
  char *relative_path;
  bool has_sequence;
  uint64_t sequence;
  bool has_size_bytes;
  uint64_t size_bytes;
  char *checksum;
} PlayerFfiDownloadSegmentRecord;

typedef struct PlayerFfiDownloadAssetIndex {
  PlayerFfiDownloadContentFormat content_format;
  char *version;
  char *etag;
  char *checksum;
  bool has_total_size_bytes;
  uint64_t total_size_bytes;
  PlayerFfiDownloadResourceRecord *resources;
  uintptr_t resources_len;
  PlayerFfiDownloadSegmentRecord *segments;
  uintptr_t segments_len;
  char *completed_path;
} PlayerFfiDownloadAssetIndex;

typedef struct PlayerFfiDownloadProgressSnapshot {
  uint64_t received_bytes;
  bool has_total_bytes;
  uint64_t total_bytes;
  uint32_t received_segments;
  bool has_total_segments;
  uint32_t total_segments;
} PlayerFfiDownloadProgressSnapshot;

typedef enum PlayerFfiDownloadTaskStatus {
  PlayerFfiDownloadTaskStatusQueued = 0,
  PlayerFfiDownloadTaskStatusPreparing = 1,
  PlayerFfiDownloadTaskStatusDownloading = 2,
  PlayerFfiDownloadTaskStatusPaused = 3,
  PlayerFfiDownloadTaskStatusCompleted = 4,
  PlayerFfiDownloadTaskStatusFailed = 5,
  PlayerFfiDownloadTaskStatusRemoved = 6,
} PlayerFfiDownloadTaskStatus;

typedef struct PlayerFfiDownloadTask {
  uint64_t task_id;
  char *asset_id;
  PlayerFfiDownloadSource source;
  PlayerFfiDownloadProfile profile;
  PlayerFfiDownloadTaskStatus status;
  PlayerFfiDownloadProgressSnapshot progress;
  PlayerFfiDownloadAssetIndex asset_index;
  bool has_error;
  uint32_t error_code;
  uint32_t error_category;
  bool error_retriable;
  char *error_message;
} PlayerFfiDownloadTask;

typedef struct PlayerFfiDownloadSnapshot {
  PlayerFfiDownloadTask *tasks;
  uintptr_t len;
} PlayerFfiDownloadSnapshot;

typedef enum PlayerFfiDownloadCommandKind {
  PlayerFfiDownloadCommandKindStart = 0,
  PlayerFfiDownloadCommandKindPause = 1,
  PlayerFfiDownloadCommandKindResume = 2,
  PlayerFfiDownloadCommandKindRemove = 3,
} PlayerFfiDownloadCommandKind;

typedef struct PlayerFfiDownloadCommand {
  PlayerFfiDownloadCommandKind kind;
  PlayerFfiDownloadTask task;
  uint64_t task_id;
} PlayerFfiDownloadCommand;

typedef struct PlayerFfiDownloadCommandList {
  PlayerFfiDownloadCommand *commands;
  uintptr_t len;
} PlayerFfiDownloadCommandList;

typedef enum PlayerFfiDownloadEventKind {
  PlayerFfiDownloadEventKindCreated = 0,
  PlayerFfiDownloadEventKindStateChanged = 1,
  PlayerFfiDownloadEventKindProgressUpdated = 2,
} PlayerFfiDownloadEventKind;

typedef struct PlayerFfiDownloadEvent {
  PlayerFfiDownloadEventKind kind;
  PlayerFfiDownloadTask task;
} PlayerFfiDownloadEvent;

typedef struct PlayerFfiDownloadEventList {
  PlayerFfiDownloadEvent *events;
  uintptr_t len;
} PlayerFfiDownloadEventList;

typedef struct PlayerFfiDownloadExportCallbacks {
  void *context;
  void (*on_progress)(void *context, float ratio);
  bool (*is_cancelled)(void *context);
} PlayerFfiDownloadExportCallbacks;

extern PlayerFfiCallStatus player_ffi_preload_session_create(
    const PlayerFfiResolvedPreloadBudgetPolicy *preload_budget,
    uint64_t *out_handle,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_preload_session_plan(
    uint64_t handle,
    const PlayerFfiPreloadCandidate *candidates,
    uintptr_t candidates_len,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_preload_session_drain_commands(
    uint64_t handle,
    PlayerFfiPreloadCommandList *out_commands,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_preload_session_complete(
    uint64_t handle,
    uint64_t task_id,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_preload_session_fail(
    uint64_t handle,
    uint64_t task_id,
    uint32_t error_code,
    uint32_t error_category,
    bool retriable,
    const char *message,
    PlayerFfiError *out_error);

extern void player_ffi_preload_command_list_free(
    PlayerFfiPreloadCommandList *commands);

extern void player_ffi_preload_session_dispose(uint64_t handle);

extern PlayerFfiCallStatus player_ffi_playlist_session_create(
    const PlayerFfiPlaylistConfig *config,
    const PlayerFfiResolvedPreloadBudgetPolicy *preload_budget,
    uint64_t *out_handle,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_playlist_session_replace_queue(
    uint64_t handle,
    const PlayerFfiPlaylistQueueItem *queue,
    uintptr_t queue_len,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_playlist_session_update_viewport_hints(
    uint64_t handle,
    const PlayerFfiPlaylistViewportHint *hints,
    uintptr_t hints_len,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_playlist_session_clear_viewport_hints(
    uint64_t handle,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_playlist_session_advance_to_next(
    uint64_t handle,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_playlist_session_advance_to_previous(
    uint64_t handle,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_playlist_session_handle_playback_completed(
    uint64_t handle,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_playlist_session_handle_playback_failed(
    uint64_t handle,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_playlist_session_current_active_item(
    uint64_t handle,
    PlayerFfiPlaylistActiveItem *out_active_item,
    PlayerFfiError *out_error);

extern void player_ffi_playlist_active_item_free(
    PlayerFfiPlaylistActiveItem *item);

extern PlayerFfiCallStatus player_ffi_playlist_session_drain_preload_commands(
    uint64_t handle,
    PlayerFfiPreloadCommandList *out_commands,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_playlist_session_complete_preload_task(
    uint64_t handle,
    uint64_t task_id,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_playlist_session_fail_preload_task(
    uint64_t handle,
    uint64_t task_id,
    uint32_t error_code,
    uint32_t error_category,
    bool retriable,
    const char *message,
    PlayerFfiError *out_error);

extern void player_ffi_playlist_session_dispose(uint64_t handle);

extern PlayerFfiCallStatus player_ffi_download_session_create(
    const PlayerFfiDownloadConfig *config,
    uint64_t *out_handle,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_download_session_create_task(
    uint64_t handle,
    const char *asset_id,
    const PlayerFfiDownloadSource *source,
    const PlayerFfiDownloadProfile *profile,
    const PlayerFfiDownloadAssetIndex *asset_index,
    uint64_t *out_task_id,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_download_session_start_task(
    uint64_t handle,
    uint64_t task_id,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_download_session_pause_task(
    uint64_t handle,
    uint64_t task_id,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_download_session_resume_task(
    uint64_t handle,
    uint64_t task_id,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_download_session_update_progress(
    uint64_t handle,
    uint64_t task_id,
    uint64_t received_bytes,
    uint32_t received_segments,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_download_session_complete_task(
    uint64_t handle,
    uint64_t task_id,
    const char *completed_path,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_download_session_export_task(
    uint64_t handle,
    uint64_t task_id,
    const char *output_path,
    PlayerFfiDownloadExportCallbacks callbacks,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_download_session_fail_task(
    uint64_t handle,
    uint64_t task_id,
    uint32_t error_code,
    uint32_t error_category,
    bool retriable,
    const char *message,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_download_session_remove_task(
    uint64_t handle,
    uint64_t task_id,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_download_session_snapshot(
    uint64_t handle,
    PlayerFfiDownloadSnapshot *out_snapshot,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_download_session_drain_commands(
    uint64_t handle,
    PlayerFfiDownloadCommandList *out_commands,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_download_session_drain_events(
    uint64_t handle,
    PlayerFfiDownloadEventList *out_events,
    PlayerFfiError *out_error);

extern void player_ffi_download_snapshot_free(PlayerFfiDownloadSnapshot *snapshot);
extern void player_ffi_download_command_list_free(PlayerFfiDownloadCommandList *commands);
extern void player_ffi_download_event_list_free(PlayerFfiDownloadEventList *events);
extern void player_ffi_download_session_dispose(uint64_t handle);

extern PlayerFfiCallStatus player_ffi_resolve_track_preferences(
    const PlayerFfiTrackPreferences *track_preferences,
    PlayerFfiTrackPreferences *out_preferences,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_benchmark_session_create(
    char **plugin_library_paths,
    uintptr_t plugin_library_paths_len,
    uint64_t *out_handle,
    PlayerFfiError *out_error);

extern void player_ffi_benchmark_session_dispose(uint64_t handle);

extern PlayerFfiCallStatus player_ffi_benchmark_session_on_event_batch_json(
    uint64_t handle,
    const char *batch_json,
    char **out_report_json,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_benchmark_session_flush_json(
    uint64_t handle,
    char **out_report_json,
    PlayerFfiError *out_error);

extern void player_ffi_benchmark_report_string_free(char *value);

extern PlayerFfiCallStatus player_ffi_dash_bridge_execute_json(
    const char *request_json,
    char **out_json,
    PlayerFfiError *out_error);

extern void player_ffi_dash_bridge_string_free(char *value);

extern void player_ffi_error_free(PlayerFfiError *error);
extern void player_ffi_track_preferences_free(PlayerFfiTrackPreferences *track_preferences);

static uint64_t non_negative_u64(int64_t value) {
  return value > 0 ? (uint64_t)value : 0;
}

static uint32_t non_negative_u32(int32_t value) {
  return value > 0 ? (uint32_t)value : 0;
}

static char *duplicate_string(const char *value) {
  if (value == NULL) {
    return NULL;
  }
  return strdup(value);
}

bool vesper_runtime_resolve_resilience_policy(
    int source_kind_ordinal,
    int source_protocol_ordinal,
    const VesperRuntimeBufferingPolicy *buffering_policy,
    const VesperRuntimeRetryPolicy *retry_policy,
    const VesperRuntimeCachePolicy *cache_policy,
    VesperRuntimeResolvedResiliencePolicy *out_policy) {
  if (buffering_policy == NULL || retry_policy == NULL || cache_policy == NULL ||
      out_policy == NULL) {
    return false;
  }

  PlayerFfiBufferingPolicy ffi_buffering_policy = {
      .preset = (PlayerFfiBufferingPreset)buffering_policy->preset_ordinal,
      .has_min_buffer_ms = buffering_policy->has_min_buffer_ms,
      .min_buffer_ms = non_negative_u64(buffering_policy->min_buffer_ms),
      .has_max_buffer_ms = buffering_policy->has_max_buffer_ms,
      .max_buffer_ms = non_negative_u64(buffering_policy->max_buffer_ms),
      .has_buffer_for_playback_ms = buffering_policy->has_buffer_for_playback_ms,
      .buffer_for_playback_ms = non_negative_u64(buffering_policy->buffer_for_playback_ms),
      .has_buffer_for_rebuffer_ms = buffering_policy->has_buffer_for_rebuffer_ms,
      .buffer_for_rebuffer_ms = non_negative_u64(buffering_policy->buffer_for_rebuffer_ms),
  };
  PlayerFfiRetryPolicy ffi_retry_policy = {
      .uses_default_max_attempts = retry_policy->uses_default_max_attempts,
      .has_max_attempts = retry_policy->has_max_attempts,
      .max_attempts =
          retry_policy->max_attempts > 0 ? (uint32_t)retry_policy->max_attempts : 0,
      .has_base_delay_ms = retry_policy->has_base_delay_ms,
      .base_delay_ms = retry_policy->base_delay_ms,
      .has_max_delay_ms = retry_policy->has_max_delay_ms,
      .max_delay_ms = retry_policy->max_delay_ms,
      .has_backoff = retry_policy->has_backoff,
      .backoff = (PlayerFfiRetryBackoff)retry_policy->backoff_ordinal,
  };
  PlayerFfiCachePolicy ffi_cache_policy = {
      .preset = (PlayerFfiCachePreset)cache_policy->preset_ordinal,
      .has_max_memory_bytes = cache_policy->has_max_memory_bytes,
      .max_memory_bytes = non_negative_u64(cache_policy->max_memory_bytes),
      .has_max_disk_bytes = cache_policy->has_max_disk_bytes,
      .max_disk_bytes = non_negative_u64(cache_policy->max_disk_bytes),
  };
  PlayerFfiResolvedResiliencePolicy ffi_resolved_policy;
  PlayerFfiError ffi_error;
  memset(&ffi_resolved_policy, 0, sizeof(ffi_resolved_policy));
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_resolve_resilience_policy(
      (PlayerFfiMediaSourceKind)source_kind_ordinal,
      (PlayerFfiMediaSourceProtocol)source_protocol_ordinal,
      &ffi_buffering_policy,
      &ffi_retry_policy,
      &ffi_cache_policy,
      &ffi_resolved_policy,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }

  out_policy->buffering.preset_ordinal = ffi_resolved_policy.buffering.preset;
  out_policy->buffering.has_min_buffer_ms = ffi_resolved_policy.buffering.has_min_buffer_ms;
  out_policy->buffering.min_buffer_ms = (int64_t)ffi_resolved_policy.buffering.min_buffer_ms;
  out_policy->buffering.has_max_buffer_ms = ffi_resolved_policy.buffering.has_max_buffer_ms;
  out_policy->buffering.max_buffer_ms = (int64_t)ffi_resolved_policy.buffering.max_buffer_ms;
  out_policy->buffering.has_buffer_for_playback_ms =
      ffi_resolved_policy.buffering.has_buffer_for_playback_ms;
  out_policy->buffering.buffer_for_playback_ms =
      (int64_t)ffi_resolved_policy.buffering.buffer_for_playback_ms;
  out_policy->buffering.has_buffer_for_rebuffer_ms =
      ffi_resolved_policy.buffering.has_buffer_for_rebuffer_ms;
  out_policy->buffering.buffer_for_rebuffer_ms =
      (int64_t)ffi_resolved_policy.buffering.buffer_for_rebuffer_ms;

  out_policy->retry.uses_default_max_attempts =
      ffi_resolved_policy.retry.uses_default_max_attempts;
  out_policy->retry.has_max_attempts = ffi_resolved_policy.retry.has_max_attempts;
  out_policy->retry.max_attempts = (int32_t)ffi_resolved_policy.retry.max_attempts;
  out_policy->retry.has_base_delay_ms = ffi_resolved_policy.retry.has_base_delay_ms;
  out_policy->retry.base_delay_ms = ffi_resolved_policy.retry.base_delay_ms;
  out_policy->retry.has_max_delay_ms = ffi_resolved_policy.retry.has_max_delay_ms;
  out_policy->retry.max_delay_ms = ffi_resolved_policy.retry.max_delay_ms;
  out_policy->retry.has_backoff = ffi_resolved_policy.retry.has_backoff;
  out_policy->retry.backoff_ordinal = ffi_resolved_policy.retry.backoff;

  out_policy->cache.preset_ordinal = ffi_resolved_policy.cache.preset;
  out_policy->cache.has_max_memory_bytes = ffi_resolved_policy.cache.has_max_memory_bytes;
  out_policy->cache.max_memory_bytes = (int64_t)ffi_resolved_policy.cache.max_memory_bytes;
  out_policy->cache.has_max_disk_bytes = ffi_resolved_policy.cache.has_max_disk_bytes;
  out_policy->cache.max_disk_bytes = (int64_t)ffi_resolved_policy.cache.max_disk_bytes;
  return true;
}

bool vesper_runtime_resolve_preload_budget(
    const VesperRuntimePreloadBudgetPolicy *preload_budget,
    VesperRuntimeResolvedPreloadBudgetPolicy *out_budget) {
  if (preload_budget == NULL || out_budget == NULL) {
    return false;
  }

  PlayerFfiPreloadBudgetPolicy ffi_preload_budget = {
      .has_max_concurrent_tasks = preload_budget->has_max_concurrent_tasks,
      .max_concurrent_tasks = preload_budget->max_concurrent_tasks > 0
                                      ? (uint32_t)preload_budget->max_concurrent_tasks
                                      : 0,
      .has_max_memory_bytes = preload_budget->has_max_memory_bytes,
      .max_memory_bytes = non_negative_u64(preload_budget->max_memory_bytes),
      .has_max_disk_bytes = preload_budget->has_max_disk_bytes,
      .max_disk_bytes = non_negative_u64(preload_budget->max_disk_bytes),
      .has_warmup_window_ms = preload_budget->has_warmup_window_ms,
      .warmup_window_ms = non_negative_u64(preload_budget->warmup_window_ms),
  };
  PlayerFfiResolvedPreloadBudgetPolicy ffi_resolved_budget;
  PlayerFfiError ffi_error;
  memset(&ffi_resolved_budget, 0, sizeof(ffi_resolved_budget));
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_resolve_preload_budget(
      &ffi_preload_budget,
      &ffi_resolved_budget,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }

  out_budget->max_concurrent_tasks = ffi_resolved_budget.max_concurrent_tasks;
  out_budget->max_memory_bytes = (int64_t)ffi_resolved_budget.max_memory_bytes;
  out_budget->max_disk_bytes = (int64_t)ffi_resolved_budget.max_disk_bytes;
  out_budget->warmup_window_ms = ffi_resolved_budget.warmup_window_ms;
  return true;
}

bool vesper_runtime_resolve_track_preferences(
    const VesperRuntimeTrackPreferencePolicy *track_preferences,
    VesperRuntimeTrackPreferencePolicy *out_preferences) {
  if (track_preferences == NULL || out_preferences == NULL) {
    return false;
  }

  PlayerFfiTrackPreferences ffi_track_preferences = {
      .preferred_audio_language = track_preferences->preferred_audio_language,
      .preferred_subtitle_language = track_preferences->preferred_subtitle_language,
      .select_subtitles_by_default = track_preferences->select_subtitles_by_default,
      .select_undetermined_subtitle_language =
          track_preferences->select_undetermined_subtitle_language,
      .audio_selection =
          {
              .mode =
                  (PlayerFfiTrackSelectionMode)track_preferences->audio_selection.mode_ordinal,
              .track_id = (char *)track_preferences->audio_selection.track_id,
          },
      .subtitle_selection =
          {
              .mode =
                  (PlayerFfiTrackSelectionMode)track_preferences->subtitle_selection.mode_ordinal,
              .track_id = (char *)track_preferences->subtitle_selection.track_id,
          },
      .abr_policy =
          {
              .mode = (PlayerFfiAbrMode)track_preferences->abr_policy.mode_ordinal,
              .track_id = (char *)track_preferences->abr_policy.track_id,
              .has_max_bit_rate = track_preferences->abr_policy.has_max_bit_rate,
              .max_bit_rate = non_negative_u64(track_preferences->abr_policy.max_bit_rate),
              .has_max_width = track_preferences->abr_policy.has_max_width,
              .max_width = non_negative_u32(track_preferences->abr_policy.max_width),
              .has_max_height = track_preferences->abr_policy.has_max_height,
              .max_height = non_negative_u32(track_preferences->abr_policy.max_height),
          },
  };
  PlayerFfiTrackPreferences ffi_resolved_preferences;
  PlayerFfiError ffi_error;
  memset(&ffi_resolved_preferences, 0, sizeof(ffi_resolved_preferences));
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_resolve_track_preferences(
      &ffi_track_preferences,
      &ffi_resolved_preferences,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }

  out_preferences->preferred_audio_language =
      duplicate_string(ffi_resolved_preferences.preferred_audio_language);
  out_preferences->preferred_subtitle_language =
      duplicate_string(ffi_resolved_preferences.preferred_subtitle_language);
  out_preferences->select_subtitles_by_default =
      ffi_resolved_preferences.select_subtitles_by_default;
  out_preferences->select_undetermined_subtitle_language =
      ffi_resolved_preferences.select_undetermined_subtitle_language;
  out_preferences->audio_selection.mode_ordinal = ffi_resolved_preferences.audio_selection.mode;
  out_preferences->audio_selection.track_id =
      duplicate_string(ffi_resolved_preferences.audio_selection.track_id);
  out_preferences->subtitle_selection.mode_ordinal =
      ffi_resolved_preferences.subtitle_selection.mode;
  out_preferences->subtitle_selection.track_id =
      duplicate_string(ffi_resolved_preferences.subtitle_selection.track_id);
  out_preferences->abr_policy.mode_ordinal = ffi_resolved_preferences.abr_policy.mode;
  out_preferences->abr_policy.track_id =
      duplicate_string(ffi_resolved_preferences.abr_policy.track_id);
  out_preferences->abr_policy.has_max_bit_rate =
      ffi_resolved_preferences.abr_policy.has_max_bit_rate;
  out_preferences->abr_policy.max_bit_rate =
      (int64_t)ffi_resolved_preferences.abr_policy.max_bit_rate;
  out_preferences->abr_policy.has_max_width =
      ffi_resolved_preferences.abr_policy.has_max_width;
  out_preferences->abr_policy.max_width =
      (int32_t)ffi_resolved_preferences.abr_policy.max_width;
  out_preferences->abr_policy.has_max_height =
      ffi_resolved_preferences.abr_policy.has_max_height;
  out_preferences->abr_policy.max_height =
      (int32_t)ffi_resolved_preferences.abr_policy.max_height;

  player_ffi_track_preferences_free(&ffi_resolved_preferences);
  return true;
}

static void free_runtime_preload_task_strings(VesperRuntimePreloadTask *task) {
  if (task == NULL) {
    return;
  }
  free(task->source_uri);
  free(task->source_identity);
  free(task->cache_key);
  free(task->scope_id);
  free(task->error_message);
}

bool vesper_runtime_preload_session_create(
    const VesperRuntimeResolvedPreloadBudgetPolicy *preload_budget,
    uint64_t *out_handle) {
  if (preload_budget == NULL || out_handle == NULL) {
    return false;
  }

  PlayerFfiResolvedPreloadBudgetPolicy ffi_budget = {
      .max_concurrent_tasks = preload_budget->max_concurrent_tasks,
      .max_memory_bytes = non_negative_u64(preload_budget->max_memory_bytes),
      .max_disk_bytes = non_negative_u64(preload_budget->max_disk_bytes),
      .warmup_window_ms = preload_budget->warmup_window_ms,
  };
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_preload_session_create(
      &ffi_budget,
      out_handle,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }
  return true;
}

bool vesper_runtime_preload_session_plan(
    uint64_t handle,
    const VesperRuntimePreloadCandidate *candidates,
    uintptr_t candidates_len) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  PlayerFfiPreloadCandidate *ffi_candidates = NULL;

  if (candidates_len > 0) {
    if (candidates == NULL) {
      return false;
    }
    ffi_candidates = calloc(candidates_len, sizeof(PlayerFfiPreloadCandidate));
    if (ffi_candidates == NULL) {
      return false;
    }
    for (uintptr_t index = 0; index < candidates_len; index += 1) {
      ffi_candidates[index].source_uri = candidates[index].source_uri;
      ffi_candidates[index].scope_kind = (int)candidates[index].scope_kind;
      ffi_candidates[index].scope_id = candidates[index].scope_id;
      ffi_candidates[index].candidate_kind = (int)candidates[index].candidate_kind;
      ffi_candidates[index].selection_hint = (int)candidates[index].selection_hint;
      ffi_candidates[index].priority = (int)candidates[index].priority;
      ffi_candidates[index].expected_memory_bytes = candidates[index].expected_memory_bytes;
      ffi_candidates[index].expected_disk_bytes = candidates[index].expected_disk_bytes;
      ffi_candidates[index].has_ttl_ms = candidates[index].has_ttl_ms;
      ffi_candidates[index].ttl_ms = candidates[index].ttl_ms;
      ffi_candidates[index].has_warmup_window_ms = candidates[index].has_warmup_window_ms;
      ffi_candidates[index].warmup_window_ms = candidates[index].warmup_window_ms;
    }
  }

  PlayerFfiCallStatus status = player_ffi_preload_session_plan(
      handle,
      ffi_candidates,
      candidates_len,
      &ffi_error);
  free(ffi_candidates);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }
  return true;
}

bool vesper_runtime_preload_session_drain_commands(
    uint64_t handle,
    VesperRuntimePreloadCommandList *out_commands) {
  if (out_commands == NULL) {
    return false;
  }

  PlayerFfiPreloadCommandList ffi_commands;
  PlayerFfiError ffi_error;
  memset(&ffi_commands, 0, sizeof(ffi_commands));
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_preload_session_drain_commands(
      handle,
      &ffi_commands,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }

  out_commands->len = ffi_commands.len;
  out_commands->commands = NULL;
  if (ffi_commands.len == 0 || ffi_commands.commands == NULL) {
    player_ffi_preload_command_list_free(&ffi_commands);
    return true;
  }

  out_commands->commands = calloc(ffi_commands.len, sizeof(VesperRuntimePreloadCommand));
  if (out_commands->commands == NULL) {
    player_ffi_preload_command_list_free(&ffi_commands);
    out_commands->len = 0;
    return false;
  }

  for (uintptr_t index = 0; index < ffi_commands.len; index += 1) {
    PlayerFfiPreloadCommand *ffi_command = &ffi_commands.commands[index];
    VesperRuntimePreloadCommand *runtime_command = &out_commands->commands[index];
    runtime_command->kind = (VesperRuntimePreloadCommandKind)ffi_command->kind;
    runtime_command->task_id = ffi_command->task_id;
    runtime_command->task.task_id = ffi_command->task.task_id;
    runtime_command->task.source_uri = duplicate_string(ffi_command->task.source_uri);
    runtime_command->task.source_identity = duplicate_string(ffi_command->task.source_identity);
    runtime_command->task.cache_key = duplicate_string(ffi_command->task.cache_key);
    runtime_command->task.scope_kind = (VesperRuntimePreloadScopeKind)ffi_command->task.scope_kind;
    runtime_command->task.scope_id = duplicate_string(ffi_command->task.scope_id);
    runtime_command->task.candidate_kind =
        (VesperRuntimePreloadCandidateKind)ffi_command->task.candidate_kind;
    runtime_command->task.selection_hint =
        (VesperRuntimePreloadSelectionHint)ffi_command->task.selection_hint;
    runtime_command->task.priority = (VesperRuntimePreloadPriority)ffi_command->task.priority;
    runtime_command->task.status = (VesperRuntimePreloadTaskStatus)ffi_command->task.status;
    runtime_command->task.expected_memory_bytes = ffi_command->task.expected_memory_bytes;
    runtime_command->task.expected_disk_bytes = ffi_command->task.expected_disk_bytes;
    runtime_command->task.warmup_window_ms = ffi_command->task.warmup_window_ms;
    runtime_command->task.has_error = ffi_command->task.has_error;
    runtime_command->task.error_code = ffi_command->task.error_code;
    runtime_command->task.error_category = ffi_command->task.error_category;
    runtime_command->task.error_retriable = ffi_command->task.error_retriable;
    runtime_command->task.error_message = duplicate_string(ffi_command->task.error_message);
  }

  player_ffi_preload_command_list_free(&ffi_commands);
  return true;
}

bool vesper_runtime_preload_session_complete(
    uint64_t handle,
    uint64_t task_id) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  PlayerFfiCallStatus status = player_ffi_preload_session_complete(handle, task_id, &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }
  return true;
}

bool vesper_runtime_preload_session_fail(
    uint64_t handle,
    uint64_t task_id,
    uint32_t error_code,
    uint32_t error_category,
    bool retriable,
    const char *message) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  PlayerFfiCallStatus status = player_ffi_preload_session_fail(
      handle,
      task_id,
      error_code,
      error_category,
      retriable,
      message,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }
  return true;
}

void vesper_runtime_preload_command_list_free(
    VesperRuntimePreloadCommandList *commands) {
  if (commands == NULL) {
    return;
  }
  if (commands->commands != NULL) {
    for (uintptr_t index = 0; index < commands->len; index += 1) {
      free_runtime_preload_task_strings(&commands->commands[index].task);
    }
    free(commands->commands);
  }
  memset(commands, 0, sizeof(*commands));
}

void vesper_runtime_preload_session_dispose(uint64_t handle) {
  player_ffi_preload_session_dispose(handle);
}

static void free_runtime_playlist_active_item_strings(
    VesperRuntimePlaylistActiveItem *item) {
  if (item == NULL) {
    return;
  }
  free(item->item_id);
}

bool vesper_runtime_playlist_session_create(
    const VesperRuntimePlaylistConfig *config,
    const VesperRuntimeResolvedPreloadBudgetPolicy *preload_budget,
    uint64_t *out_handle) {
  if (config == NULL || preload_budget == NULL || out_handle == NULL) {
    return false;
  }

  PlayerFfiPlaylistConfig ffi_config = {
      .playlist_id = config->playlist_id,
      .neighbor_previous = config->neighbor_previous,
      .neighbor_next = config->neighbor_next,
      .preload_near_visible = config->preload_near_visible,
      .preload_prefetch_only = config->preload_prefetch_only,
      .auto_advance = config->auto_advance,
      .repeat_mode = (PlayerFfiPlaylistRepeatMode)config->repeat_mode,
      .failure_strategy =
          (PlayerFfiPlaylistFailureStrategy)config->failure_strategy,
  };
  PlayerFfiResolvedPreloadBudgetPolicy ffi_budget = {
      .max_concurrent_tasks = preload_budget->max_concurrent_tasks,
      .max_memory_bytes = non_negative_u64(preload_budget->max_memory_bytes),
      .max_disk_bytes = non_negative_u64(preload_budget->max_disk_bytes),
      .warmup_window_ms = preload_budget->warmup_window_ms,
  };
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_playlist_session_create(
      &ffi_config,
      &ffi_budget,
      out_handle,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }
  return true;
}

bool vesper_runtime_playlist_session_replace_queue(
    uint64_t handle,
    const VesperRuntimePlaylistQueueItem *queue,
    uintptr_t queue_len) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  PlayerFfiPlaylistQueueItem *ffi_queue = NULL;

  if (queue_len > 0) {
    if (queue == NULL) {
      return false;
    }
    ffi_queue = calloc(queue_len, sizeof(PlayerFfiPlaylistQueueItem));
    if (ffi_queue == NULL) {
      return false;
    }
    for (uintptr_t index = 0; index < queue_len; index += 1) {
      ffi_queue[index].item_id = queue[index].item_id;
      ffi_queue[index].source_uri = queue[index].source_uri;
      ffi_queue[index].expected_memory_bytes = queue[index].expected_memory_bytes;
      ffi_queue[index].expected_disk_bytes = queue[index].expected_disk_bytes;
      ffi_queue[index].has_ttl_ms = queue[index].has_ttl_ms;
      ffi_queue[index].ttl_ms = queue[index].ttl_ms;
      ffi_queue[index].has_warmup_window_ms = queue[index].has_warmup_window_ms;
      ffi_queue[index].warmup_window_ms = queue[index].warmup_window_ms;
    }
  }

  PlayerFfiCallStatus status = player_ffi_playlist_session_replace_queue(
      handle,
      ffi_queue,
      queue_len,
      &ffi_error);
  free(ffi_queue);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }
  return true;
}

bool vesper_runtime_playlist_session_update_viewport_hints(
    uint64_t handle,
    const VesperRuntimePlaylistViewportHint *hints,
    uintptr_t hints_len) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  PlayerFfiPlaylistViewportHint *ffi_hints = NULL;

  if (hints_len > 0) {
    if (hints == NULL) {
      return false;
    }
    ffi_hints = calloc(hints_len, sizeof(PlayerFfiPlaylistViewportHint));
    if (ffi_hints == NULL) {
      return false;
    }
    for (uintptr_t index = 0; index < hints_len; index += 1) {
      ffi_hints[index].item_id = hints[index].item_id;
      ffi_hints[index].kind = (PlayerFfiPlaylistViewportHintKind)hints[index].kind;
      ffi_hints[index].order = hints[index].order;
    }
  }

  PlayerFfiCallStatus status = player_ffi_playlist_session_update_viewport_hints(
      handle,
      ffi_hints,
      hints_len,
      &ffi_error);
  free(ffi_hints);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }
  return true;
}

static bool call_playlist_status(
    PlayerFfiCallStatus status,
    PlayerFfiError *ffi_error) {
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(ffi_error);
    return false;
  }
  return true;
}

bool vesper_runtime_playlist_session_clear_viewport_hints(
    uint64_t handle) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_playlist_session_clear_viewport_hints(handle, &ffi_error),
      &ffi_error);
}

bool vesper_runtime_playlist_session_advance_to_next(
    uint64_t handle) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_playlist_session_advance_to_next(handle, &ffi_error),
      &ffi_error);
}

bool vesper_runtime_playlist_session_advance_to_previous(
    uint64_t handle) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_playlist_session_advance_to_previous(handle, &ffi_error),
      &ffi_error);
}

bool vesper_runtime_playlist_session_handle_playback_completed(
    uint64_t handle) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_playlist_session_handle_playback_completed(handle, &ffi_error),
      &ffi_error);
}

bool vesper_runtime_playlist_session_handle_playback_failed(
    uint64_t handle) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_playlist_session_handle_playback_failed(handle, &ffi_error),
      &ffi_error);
}

bool vesper_runtime_playlist_session_current_active_item(
    uint64_t handle,
    VesperRuntimePlaylistActiveItem *out_active_item) {
  if (out_active_item == NULL) {
    return false;
  }

  PlayerFfiPlaylistActiveItem ffi_active_item;
  PlayerFfiError ffi_error;
  memset(&ffi_active_item, 0, sizeof(ffi_active_item));
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_playlist_session_current_active_item(
      handle,
      &ffi_active_item,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }

  out_active_item->item_id = duplicate_string(ffi_active_item.item_id);
  out_active_item->index = ffi_active_item.index;
  player_ffi_playlist_active_item_free(&ffi_active_item);
  return true;
}

bool vesper_runtime_playlist_session_drain_preload_commands(
    uint64_t handle,
    VesperRuntimePreloadCommandList *out_commands) {
  if (out_commands == NULL) {
    return false;
  }

  PlayerFfiPreloadCommandList ffi_commands;
  PlayerFfiError ffi_error;
  memset(&ffi_commands, 0, sizeof(ffi_commands));
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status =
      player_ffi_playlist_session_drain_preload_commands(
          handle,
          &ffi_commands,
          &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }

  out_commands->len = ffi_commands.len;
  out_commands->commands = NULL;
  if (ffi_commands.len == 0 || ffi_commands.commands == NULL) {
    player_ffi_preload_command_list_free(&ffi_commands);
    return true;
  }

  out_commands->commands = calloc(ffi_commands.len, sizeof(VesperRuntimePreloadCommand));
  if (out_commands->commands == NULL) {
    player_ffi_preload_command_list_free(&ffi_commands);
    out_commands->len = 0;
    return false;
  }

  for (uintptr_t index = 0; index < ffi_commands.len; index += 1) {
    PlayerFfiPreloadCommand *ffi_command = &ffi_commands.commands[index];
    VesperRuntimePreloadCommand *runtime_command = &out_commands->commands[index];
    runtime_command->kind = (VesperRuntimePreloadCommandKind)ffi_command->kind;
    runtime_command->task_id = ffi_command->task_id;
    runtime_command->task.task_id = ffi_command->task.task_id;
    runtime_command->task.source_uri = duplicate_string(ffi_command->task.source_uri);
    runtime_command->task.source_identity = duplicate_string(ffi_command->task.source_identity);
    runtime_command->task.cache_key = duplicate_string(ffi_command->task.cache_key);
    runtime_command->task.scope_kind =
        (VesperRuntimePreloadScopeKind)ffi_command->task.scope_kind;
    runtime_command->task.scope_id = duplicate_string(ffi_command->task.scope_id);
    runtime_command->task.candidate_kind =
        (VesperRuntimePreloadCandidateKind)ffi_command->task.candidate_kind;
    runtime_command->task.selection_hint =
        (VesperRuntimePreloadSelectionHint)ffi_command->task.selection_hint;
    runtime_command->task.priority =
        (VesperRuntimePreloadPriority)ffi_command->task.priority;
    runtime_command->task.status =
        (VesperRuntimePreloadTaskStatus)ffi_command->task.status;
    runtime_command->task.expected_memory_bytes =
        ffi_command->task.expected_memory_bytes;
    runtime_command->task.expected_disk_bytes =
        ffi_command->task.expected_disk_bytes;
    runtime_command->task.warmup_window_ms = ffi_command->task.warmup_window_ms;
    runtime_command->task.has_error = ffi_command->task.has_error;
    runtime_command->task.error_code = ffi_command->task.error_code;
    runtime_command->task.error_category = ffi_command->task.error_category;
    runtime_command->task.error_retriable = ffi_command->task.error_retriable;
    runtime_command->task.error_message =
        duplicate_string(ffi_command->task.error_message);
  }

  player_ffi_preload_command_list_free(&ffi_commands);
  return true;
}

bool vesper_runtime_playlist_session_complete_preload_task(
    uint64_t handle,
    uint64_t task_id) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_playlist_session_complete_preload_task(
          handle,
          task_id,
          &ffi_error),
      &ffi_error);
}

bool vesper_runtime_playlist_session_fail_preload_task(
    uint64_t handle,
    uint64_t task_id,
    uint32_t error_code,
    uint32_t error_category,
    bool retriable,
    const char *message) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_playlist_session_fail_preload_task(
          handle,
          task_id,
          error_code,
          error_category,
          retriable,
          message,
          &ffi_error),
      &ffi_error);
}

void vesper_runtime_playlist_active_item_free(
    VesperRuntimePlaylistActiveItem *item) {
  if (item == NULL) {
    return;
  }
  free_runtime_playlist_active_item_strings(item);
  memset(item, 0, sizeof(*item));
}

void vesper_runtime_playlist_session_dispose(uint64_t handle) {
  player_ffi_playlist_session_dispose(handle);
}

bool vesper_runtime_download_session_create(
    const VesperRuntimeDownloadConfig *config,
    uint64_t *out_handle) {
  if (config == NULL || out_handle == NULL) {
    return false;
  }

  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_download_session_create(
          (const PlayerFfiDownloadConfig *)config,
          out_handle,
          &ffi_error),
      &ffi_error);
}

bool vesper_runtime_download_session_create_task(
    uint64_t handle,
    const char *asset_id,
    const VesperRuntimeDownloadSource *source,
    const VesperRuntimeDownloadProfile *profile,
    const VesperRuntimeDownloadAssetIndex *asset_index,
    uint64_t *out_task_id) {
  if (asset_id == NULL || source == NULL || profile == NULL || asset_index == NULL ||
      out_task_id == NULL) {
    return false;
  }

  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_download_session_create_task(
          handle,
          asset_id,
          (const PlayerFfiDownloadSource *)source,
          (const PlayerFfiDownloadProfile *)profile,
          (const PlayerFfiDownloadAssetIndex *)asset_index,
          out_task_id,
          &ffi_error),
      &ffi_error);
}

bool vesper_runtime_download_session_start_task(
    uint64_t handle,
    uint64_t task_id) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_download_session_start_task(handle, task_id, &ffi_error),
      &ffi_error);
}

bool vesper_runtime_download_session_pause_task(
    uint64_t handle,
    uint64_t task_id) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_download_session_pause_task(handle, task_id, &ffi_error),
      &ffi_error);
}

bool vesper_runtime_download_session_resume_task(
    uint64_t handle,
    uint64_t task_id) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_download_session_resume_task(handle, task_id, &ffi_error),
      &ffi_error);
}

bool vesper_runtime_download_session_update_progress(
    uint64_t handle,
    uint64_t task_id,
    uint64_t received_bytes,
    uint32_t received_segments) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_download_session_update_progress(
          handle,
          task_id,
          received_bytes,
          received_segments,
          &ffi_error),
      &ffi_error);
}

bool vesper_runtime_download_session_complete_task(
    uint64_t handle,
    uint64_t task_id,
    const char *completed_path) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_download_session_complete_task(
          handle,
          task_id,
          completed_path,
          &ffi_error),
      &ffi_error);
}

bool vesper_runtime_download_session_export_task(
    uint64_t handle,
    uint64_t task_id,
    const char *output_path,
    VesperRuntimeDownloadExportCallbacks callbacks) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  PlayerFfiDownloadExportCallbacks ffi_callbacks = {
      .context = callbacks.context,
      .on_progress = callbacks.on_progress,
      .is_cancelled = callbacks.is_cancelled,
  };
  return call_playlist_status(
      player_ffi_download_session_export_task(
          handle,
          task_id,
          output_path,
          ffi_callbacks,
          &ffi_error),
      &ffi_error);
}

bool vesper_runtime_download_session_fail_task(
    uint64_t handle,
    uint64_t task_id,
    uint32_t error_code,
    uint32_t error_category,
    bool retriable,
    const char *message) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_download_session_fail_task(
          handle,
          task_id,
          error_code,
          error_category,
          retriable,
          message,
          &ffi_error),
      &ffi_error);
}

bool vesper_runtime_download_session_remove_task(
    uint64_t handle,
    uint64_t task_id) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_download_session_remove_task(handle, task_id, &ffi_error),
      &ffi_error);
}

bool vesper_runtime_download_session_snapshot(
    uint64_t handle,
    VesperRuntimeDownloadSnapshot *out_snapshot) {
  if (out_snapshot == NULL) {
    return false;
  }

  PlayerFfiDownloadSnapshot ffi_snapshot;
  PlayerFfiError ffi_error;
  memset(&ffi_snapshot, 0, sizeof(ffi_snapshot));
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_download_session_snapshot(
      handle,
      &ffi_snapshot,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }

  out_snapshot->tasks = (VesperRuntimeDownloadTask *)ffi_snapshot.tasks;
  out_snapshot->len = ffi_snapshot.len;
  return true;
}

bool vesper_runtime_download_session_drain_commands(
    uint64_t handle,
    VesperRuntimeDownloadCommandList *out_commands) {
  if (out_commands == NULL) {
    return false;
  }

  PlayerFfiDownloadCommandList ffi_commands;
  PlayerFfiError ffi_error;
  memset(&ffi_commands, 0, sizeof(ffi_commands));
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_download_session_drain_commands(
      handle,
      &ffi_commands,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }

  out_commands->commands = (VesperRuntimeDownloadCommand *)ffi_commands.commands;
  out_commands->len = ffi_commands.len;
  return true;
}

bool vesper_runtime_download_session_drain_events(
    uint64_t handle,
    VesperRuntimeDownloadEventList *out_events) {
  if (out_events == NULL) {
    return false;
  }

  PlayerFfiDownloadEventList ffi_events;
  PlayerFfiError ffi_error;
  memset(&ffi_events, 0, sizeof(ffi_events));
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_download_session_drain_events(
      handle,
      &ffi_events,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }

  out_events->events = (VesperRuntimeDownloadEvent *)ffi_events.events;
  out_events->len = ffi_events.len;
  return true;
}

void vesper_runtime_download_snapshot_free(
    VesperRuntimeDownloadSnapshot *snapshot) {
  player_ffi_download_snapshot_free((PlayerFfiDownloadSnapshot *)snapshot);
}

void vesper_runtime_download_command_list_free(
    VesperRuntimeDownloadCommandList *commands) {
  player_ffi_download_command_list_free((PlayerFfiDownloadCommandList *)commands);
}

void vesper_runtime_download_event_list_free(
    VesperRuntimeDownloadEventList *events) {
  player_ffi_download_event_list_free((PlayerFfiDownloadEventList *)events);
}

void vesper_runtime_download_session_dispose(uint64_t handle) {
  player_ffi_download_session_dispose(handle);
}

void vesper_runtime_track_preferences_free(
    VesperRuntimeTrackPreferencePolicy *track_preferences) {
  if (track_preferences == NULL) {
    return;
  }

  free(track_preferences->preferred_audio_language);
  free(track_preferences->preferred_subtitle_language);
  free((void *)track_preferences->audio_selection.track_id);
  free((void *)track_preferences->subtitle_selection.track_id);
  free((void *)track_preferences->abr_policy.track_id);
  memset(track_preferences, 0, sizeof(*track_preferences));
}

bool vesper_runtime_benchmark_sink_session_create(
    char **plugin_library_paths,
    uintptr_t plugin_library_paths_len,
    uint64_t *out_handle,
    char **out_error_message) {
  if (out_handle == NULL) {
    return false;
  }
  if (out_error_message != NULL) {
    *out_error_message = NULL;
  }
  *out_handle = 0;

  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_benchmark_session_create(
      plugin_library_paths,
      plugin_library_paths_len,
      out_handle,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    if (out_error_message != NULL) {
      *out_error_message = ffi_error.message;
      ffi_error.message = NULL;
    }
    player_ffi_error_free(&ffi_error);
    return false;
  }
  return *out_handle != 0;
}

void vesper_runtime_benchmark_sink_session_dispose(uint64_t handle) {
  player_ffi_benchmark_session_dispose(handle);
}

bool vesper_runtime_benchmark_sink_session_submit_json(
    uint64_t handle,
    const char *batch_json,
    char **out_report_json,
    char **out_error_message) {
  if (batch_json == NULL || out_report_json == NULL) {
    return false;
  }
  if (out_error_message != NULL) {
    *out_error_message = NULL;
  }
  *out_report_json = NULL;

  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_benchmark_session_on_event_batch_json(
      handle,
      batch_json,
      out_report_json,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    if (out_error_message != NULL) {
      *out_error_message = ffi_error.message;
      ffi_error.message = NULL;
    }
    player_ffi_error_free(&ffi_error);
    return false;
  }
  return *out_report_json != NULL;
}

bool vesper_runtime_benchmark_sink_session_flush_json(
    uint64_t handle,
    char **out_report_json,
    char **out_error_message) {
  if (out_report_json == NULL) {
    return false;
  }
  if (out_error_message != NULL) {
    *out_error_message = NULL;
  }
  *out_report_json = NULL;

  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_benchmark_session_flush_json(
      handle,
      out_report_json,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    if (out_error_message != NULL) {
      *out_error_message = ffi_error.message;
      ffi_error.message = NULL;
    }
    player_ffi_error_free(&ffi_error);
    return false;
  }
  return *out_report_json != NULL;
}

void vesper_runtime_benchmark_string_free(char *value) {
  player_ffi_benchmark_report_string_free(value);
}

bool vesper_dash_bridge_execute_json(
    const char *request_json,
    char **out_json,
    char **out_error_message) {
  if (request_json == NULL || out_json == NULL) {
    return false;
  }
  if (out_error_message != NULL) {
    *out_error_message = NULL;
  }
  *out_json = NULL;

  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_dash_bridge_execute_json(
      request_json,
      out_json,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    if (out_error_message != NULL) {
      *out_error_message = ffi_error.message;
      ffi_error.message = NULL;
    }
    player_ffi_error_free(&ffi_error);
    return false;
  }
  return *out_json != NULL;
}

void vesper_dash_bridge_string_free(char *value) {
  player_ffi_dash_bridge_string_free(value);
}
