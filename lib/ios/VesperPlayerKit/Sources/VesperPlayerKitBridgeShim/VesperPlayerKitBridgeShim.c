/* Auto-generated. Do not edit directly. */
#include "include/VesperPlayerKitBridgeShim.h"
#include <stdbool.h>
#include <stddef.h>
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
  PlayerFfiErrorCode code;
  PlayerFfiErrorCategory category;
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
  PlayerFfiErrorCode error_code;
  PlayerFfiErrorCategory error_category;
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
  PlayerFfiDownloadContentFormatFlvSegments = 2,
  PlayerFfiDownloadContentFormatSingleFile = 3,
  PlayerFfiDownloadContentFormatUnknown = 4,
} PlayerFfiDownloadContentFormat;

typedef enum PlayerFfiDownloadOutputFormat {
  PlayerFfiDownloadOutputFormatMp4 = 0,
  PlayerFfiDownloadOutputFormatMkv = 1,
  PlayerFfiDownloadOutputFormatOriginal = 2,
} PlayerFfiDownloadOutputFormat;

typedef enum PlayerFfiDownloadStreamKind {
  PlayerFfiDownloadStreamKindCombined = 0,
  PlayerFfiDownloadStreamKindVideo = 1,
  PlayerFfiDownloadStreamKindAudio = 2,
  PlayerFfiDownloadStreamKindSecondaryAudio = 3,
  PlayerFfiDownloadStreamKindSubtitle = 4,
  PlayerFfiDownloadStreamKindAuxiliary = 5,
} PlayerFfiDownloadStreamKind;

typedef struct PlayerFfiDownloadSource {
  char *source_uri;
  PlayerFfiDownloadContentFormat content_format;
  char *manifest_uri;
  char **header_names;
  char **header_values;
  uintptr_t headers_len;
} PlayerFfiDownloadSource;

typedef struct PlayerFfiDownloadProfile {
  char *variant_id;
  char *preferred_audio_language;
  char *preferred_subtitle_language;
  char **selected_track_ids;
  uintptr_t selected_track_ids_len;
  bool has_target_output_format;
  PlayerFfiDownloadOutputFormat target_output_format;
  char *target_directory;
  bool allow_metered_network;
} PlayerFfiDownloadProfile;

typedef struct PlayerFfiDownloadByteRange {
  uint64_t offset;
  uint64_t length;
} PlayerFfiDownloadByteRange;

typedef struct PlayerFfiDownloadResourceRecord {
  char *resource_id;
  char *uri;
  char *relative_path;
  bool has_byte_range;
  PlayerFfiDownloadByteRange byte_range;
  char *generated_text;
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
  bool has_byte_range;
  PlayerFfiDownloadByteRange byte_range;
  bool has_size_bytes;
  uint64_t size_bytes;
  char *checksum;
} PlayerFfiDownloadSegmentRecord;

typedef struct PlayerFfiDownloadAssetStream {
  char *stream_id;
  PlayerFfiDownloadStreamKind kind;
  char *language;
  char *codec;
  char *label;
  bool has_quality_rank;
  uint32_t quality_rank;
  char **resource_ids;
  uintptr_t resource_ids_len;
  char **segment_ids;
  uintptr_t segment_ids_len;
  char **metadata_keys;
  char **metadata_values;
  uintptr_t metadata_len;
} PlayerFfiDownloadAssetStream;

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
  PlayerFfiDownloadAssetStream *streams;
  uintptr_t streams_len;
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
  PlayerFfiErrorCode error_code;
  PlayerFfiErrorCategory error_category;
  bool error_retriable;
  char *error_message;
} PlayerFfiDownloadTask;

typedef struct PlayerFfiDownloadSnapshot {
  PlayerFfiDownloadTask *tasks;
  uintptr_t len;
} PlayerFfiDownloadSnapshot;

typedef enum PlayerFfiDownloadCommandKind {
  PlayerFfiDownloadCommandKindPrepare = 0,
  PlayerFfiDownloadCommandKindStart = 1,
  PlayerFfiDownloadCommandKindPause = 2,
  PlayerFfiDownloadCommandKindResume = 3,
  PlayerFfiDownloadCommandKindRemove = 4,
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
  PlayerFfiDownloadEventKindAssetIndexUpdated = 2,
  PlayerFfiDownloadEventKindProgressUpdated = 3,
} PlayerFfiDownloadEventKind;

typedef struct PlayerFfiDownloadEvent {
  PlayerFfiDownloadEventKind kind;
  PlayerFfiDownloadTask task;
  uint64_t task_id;
  PlayerFfiDownloadTaskStatus status;
  PlayerFfiDownloadProgressSnapshot progress;
  bool has_error;
  PlayerFfiErrorCode error_code;
  PlayerFfiErrorCategory error_category;
  bool error_retriable;
  char *error_message;
  char *completed_path;
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
    PlayerFfiErrorCode error_code,
    PlayerFfiErrorCategory error_category,
    bool retriable,
    const char *message,
    PlayerFfiError *out_error);

extern void player_ffi_preload_command_list_free(PlayerFfiPreloadCommandList *commands);

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

extern void player_ffi_playlist_active_item_free(PlayerFfiPlaylistActiveItem *item);

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
    PlayerFfiErrorCode error_code,
    PlayerFfiErrorCategory error_category,
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

extern PlayerFfiCallStatus player_ffi_download_session_restore_tasks(
    uint64_t handle,
    const PlayerFfiDownloadTask *tasks,
    uintptr_t tasks_len,
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

extern PlayerFfiCallStatus player_ffi_download_session_complete_preparation(
    uint64_t handle,
    uint64_t task_id,
    const PlayerFfiDownloadAssetIndex *asset_index,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_download_session_replace_task_plan(
    uint64_t handle,
    uint64_t task_id,
    const PlayerFfiDownloadSource *source,
    const PlayerFfiDownloadProfile *profile,
    const PlayerFfiDownloadAssetIndex *asset_index,
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
    PlayerFfiErrorCode error_code,
    PlayerFfiErrorCategory error_category,
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

extern PlayerFfiCallStatus player_ffi_mobile_plugin_diagnostics_json(
    const char *source_uri,
    uint32_t source_mode,
    char **source_plugin_library_paths,
    uintptr_t source_plugin_library_paths_len,
    const char *runtime_profile,
    uint32_t frame_mode,
    char **frame_plugin_library_paths,
    uintptr_t frame_plugin_library_paths_len,
    char **out_json,
    PlayerFfiError *out_error);

extern void player_ffi_mobile_plugin_diagnostics_string_free(char *value);

extern PlayerFfiCallStatus player_ffi_source_normalizer_resource_open(
    const char *source_uri,
    uint32_t source_mode,
    char **source_plugin_library_paths,
    uintptr_t source_plugin_library_paths_len,
    const char *runtime_profile,
    const char *output_root,
    bool force_normalized,
    uint64_t *out_handle,
    char **out_json,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_source_normalizer_resource_poll(
    uint64_t handle,
    char **out_json,
    PlayerFfiError *out_error);

extern void player_ffi_source_normalizer_resource_dispose(uint64_t handle);

extern PlayerFfiCallStatus player_ffi_dash_bridge_execute_json(
    const char *request_json,
    char **out_json,
    PlayerFfiError *out_error);

extern PlayerFfiCallStatus player_ffi_dash_bridge_parse_sidx(
    const uint8_t *data,
    uintptr_t data_len,
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

static bool can_allocate_items(uintptr_t len, size_t item_size) {
  return item_size > 0 && len <= (uintptr_t)(SIZE_MAX / item_size);
}

static bool duplicate_runtime_string(const char *value, char **out_value) {
  if (out_value == NULL) {
    return false;
  }
  *out_value = NULL;
  if (value == NULL) {
    return true;
  }
  *out_value = duplicate_string(value);
  return *out_value != NULL;
}

static void free_runtime_string_list(char **values, uintptr_t len) {
  if (values == NULL) {
    return;
  }
  for (uintptr_t index = 0; index < len; index += 1) {
    free(values[index]);
  }
  free(values);
}

static bool copy_ffi_string_list_to_runtime(
    char **values,
    uintptr_t len,
    char ***out_values) {
  if (out_values == NULL) {
    return false;
  }
  *out_values = NULL;
  if (len == 0) {
    return true;
  }
  if (values == NULL || !can_allocate_items(len, sizeof(char *))) {
    return false;
  }

  char **copied_values = calloc((size_t)len, sizeof(char *));
  if (copied_values == NULL) {
    return false;
  }

  for (uintptr_t index = 0; index < len; index += 1) {
    if (!duplicate_runtime_string(values[index], &copied_values[index])) {
      free_runtime_string_list(copied_values, len);
      return false;
    }
  }

  *out_values = copied_values;
  return true;
}

static PlayerFfiDownloadByteRange ffi_download_byte_range_from_runtime(
    VesperRuntimeDownloadByteRange byte_range) {
  PlayerFfiDownloadByteRange ffi_byte_range = {
      .offset = byte_range.offset,
      .length = byte_range.length,
  };
  return ffi_byte_range;
}

static VesperRuntimeDownloadByteRange runtime_download_byte_range_from_ffi(
    PlayerFfiDownloadByteRange byte_range) {
  VesperRuntimeDownloadByteRange runtime_byte_range = {
      .offset = byte_range.offset,
      .length = byte_range.length,
  };
  return runtime_byte_range;
}

static PlayerFfiDownloadConfig ffi_download_config_from_runtime(
    const VesperRuntimeDownloadConfig *config) {
  PlayerFfiDownloadConfig ffi_config = {
      .auto_start = config->auto_start,
      .run_post_processors_on_completion = config->run_post_processors_on_completion,
      .plugin_library_paths = config->plugin_library_paths,
      .plugin_library_paths_len = config->plugin_library_paths_len,
  };
  return ffi_config;
}

static PlayerFfiDownloadSource ffi_download_source_from_runtime(
    const VesperRuntimeDownloadSource *source) {
  PlayerFfiDownloadSource ffi_source = {
      .source_uri = source->source_uri,
      .content_format = (PlayerFfiDownloadContentFormat)source->content_format,
      .manifest_uri = source->manifest_uri,
      .header_names = source->header_names,
      .header_values = source->header_values,
      .headers_len = source->headers_len,
  };
  return ffi_source;
}

static PlayerFfiDownloadProfile ffi_download_profile_from_runtime(
    const VesperRuntimeDownloadProfile *profile) {
  PlayerFfiDownloadProfile ffi_profile = {
      .variant_id = profile->variant_id,
      .preferred_audio_language = profile->preferred_audio_language,
      .preferred_subtitle_language = profile->preferred_subtitle_language,
      .selected_track_ids = profile->selected_track_ids,
      .selected_track_ids_len = profile->selected_track_ids_len,
      .has_target_output_format = profile->has_target_output_format,
      .target_output_format = (PlayerFfiDownloadOutputFormat)profile->target_output_format,
      .target_directory = profile->target_directory,
      .allow_metered_network = profile->allow_metered_network,
  };
  return ffi_profile;
}

static PlayerFfiDownloadResourceRecord ffi_download_resource_from_runtime(
    const VesperRuntimeDownloadResourceRecord *resource) {
  PlayerFfiDownloadResourceRecord ffi_resource = {
      .resource_id = resource->resource_id,
      .uri = resource->uri,
      .relative_path = resource->relative_path,
      .has_byte_range = resource->has_byte_range,
      .byte_range = ffi_download_byte_range_from_runtime(resource->byte_range),
      .generated_text = resource->generated_text,
      .has_size_bytes = resource->has_size_bytes,
      .size_bytes = resource->size_bytes,
      .etag = resource->etag,
      .checksum = resource->checksum,
  };
  return ffi_resource;
}

static PlayerFfiDownloadSegmentRecord ffi_download_segment_from_runtime(
    const VesperRuntimeDownloadSegmentRecord *segment) {
  PlayerFfiDownloadSegmentRecord ffi_segment = {
      .segment_id = segment->segment_id,
      .uri = segment->uri,
      .relative_path = segment->relative_path,
      .has_sequence = segment->has_sequence,
      .sequence = segment->sequence,
      .has_byte_range = segment->has_byte_range,
      .byte_range = ffi_download_byte_range_from_runtime(segment->byte_range),
      .has_size_bytes = segment->has_size_bytes,
      .size_bytes = segment->size_bytes,
      .checksum = segment->checksum,
  };
  return ffi_segment;
}

static PlayerFfiDownloadAssetStream ffi_download_stream_from_runtime(
    const VesperRuntimeDownloadAssetStream *stream) {
  PlayerFfiDownloadAssetStream ffi_stream = {
      .stream_id = stream->stream_id,
      .kind = (PlayerFfiDownloadStreamKind)stream->kind,
      .language = stream->language,
      .codec = stream->codec,
      .label = stream->label,
      .has_quality_rank = stream->has_quality_rank,
      .quality_rank = stream->quality_rank,
      .resource_ids = stream->resource_ids,
      .resource_ids_len = stream->resource_ids_len,
      .segment_ids = stream->segment_ids,
      .segment_ids_len = stream->segment_ids_len,
      .metadata_keys = stream->metadata_keys,
      .metadata_values = stream->metadata_values,
      .metadata_len = stream->metadata_len,
  };
  return ffi_stream;
}

static void free_borrowed_ffi_download_asset_index(PlayerFfiDownloadAssetIndex *asset_index) {
  if (asset_index == NULL) {
    return;
  }
  free(asset_index->resources);
  free(asset_index->segments);
  free(asset_index->streams);
  memset(asset_index, 0, sizeof(*asset_index));
}

static bool copy_runtime_download_resources_to_ffi(
    const VesperRuntimeDownloadResourceRecord *resources,
    uintptr_t len,
    PlayerFfiDownloadResourceRecord **out_resources) {
  if (out_resources == NULL) {
    return false;
  }
  *out_resources = NULL;
  if (len == 0) {
    return true;
  }
  if (resources == NULL || !can_allocate_items(len, sizeof(PlayerFfiDownloadResourceRecord))) {
    return false;
  }

  PlayerFfiDownloadResourceRecord *ffi_resources =
      calloc((size_t)len, sizeof(PlayerFfiDownloadResourceRecord));
  if (ffi_resources == NULL) {
    return false;
  }
  for (uintptr_t index = 0; index < len; index += 1) {
    ffi_resources[index] = ffi_download_resource_from_runtime(&resources[index]);
  }
  *out_resources = ffi_resources;
  return true;
}

static bool copy_runtime_download_segments_to_ffi(
    const VesperRuntimeDownloadSegmentRecord *segments,
    uintptr_t len,
    PlayerFfiDownloadSegmentRecord **out_segments) {
  if (out_segments == NULL) {
    return false;
  }
  *out_segments = NULL;
  if (len == 0) {
    return true;
  }
  if (segments == NULL || !can_allocate_items(len, sizeof(PlayerFfiDownloadSegmentRecord))) {
    return false;
  }

  PlayerFfiDownloadSegmentRecord *ffi_segments =
      calloc((size_t)len, sizeof(PlayerFfiDownloadSegmentRecord));
  if (ffi_segments == NULL) {
    return false;
  }
  for (uintptr_t index = 0; index < len; index += 1) {
    ffi_segments[index] = ffi_download_segment_from_runtime(&segments[index]);
  }
  *out_segments = ffi_segments;
  return true;
}

static bool copy_runtime_download_streams_to_ffi(
    const VesperRuntimeDownloadAssetStream *streams,
    uintptr_t len,
    PlayerFfiDownloadAssetStream **out_streams) {
  if (out_streams == NULL) {
    return false;
  }
  *out_streams = NULL;
  if (len == 0) {
    return true;
  }
  if (streams == NULL || !can_allocate_items(len, sizeof(PlayerFfiDownloadAssetStream))) {
    return false;
  }

  PlayerFfiDownloadAssetStream *ffi_streams =
      calloc((size_t)len, sizeof(PlayerFfiDownloadAssetStream));
  if (ffi_streams == NULL) {
    return false;
  }
  for (uintptr_t index = 0; index < len; index += 1) {
    ffi_streams[index] = ffi_download_stream_from_runtime(&streams[index]);
  }
  *out_streams = ffi_streams;
  return true;
}

static bool ffi_download_asset_index_from_runtime(
    const VesperRuntimeDownloadAssetIndex *asset_index,
    PlayerFfiDownloadAssetIndex *out_asset_index) {
  if (asset_index == NULL || out_asset_index == NULL) {
    return false;
  }
  memset(out_asset_index, 0, sizeof(*out_asset_index));

  out_asset_index->content_format = (PlayerFfiDownloadContentFormat)asset_index->content_format;
  out_asset_index->version = asset_index->version;
  out_asset_index->etag = asset_index->etag;
  out_asset_index->checksum = asset_index->checksum;
  out_asset_index->has_total_size_bytes = asset_index->has_total_size_bytes;
  out_asset_index->total_size_bytes = asset_index->total_size_bytes;
  out_asset_index->resources_len = asset_index->resources_len;
  out_asset_index->segments_len = asset_index->segments_len;
  out_asset_index->streams_len = asset_index->streams_len;
  out_asset_index->completed_path = asset_index->completed_path;

  if (!copy_runtime_download_resources_to_ffi(
          asset_index->resources,
          asset_index->resources_len,
          &out_asset_index->resources) ||
      !copy_runtime_download_segments_to_ffi(
          asset_index->segments,
          asset_index->segments_len,
          &out_asset_index->segments) ||
      !copy_runtime_download_streams_to_ffi(
          asset_index->streams,
          asset_index->streams_len,
          &out_asset_index->streams)) {
    free_borrowed_ffi_download_asset_index(out_asset_index);
    return false;
  }
  return true;
}

static PlayerFfiDownloadProgressSnapshot ffi_download_progress_from_runtime(
    VesperRuntimeDownloadProgressSnapshot progress) {
  PlayerFfiDownloadProgressSnapshot ffi_progress = {
      .received_bytes = progress.received_bytes,
      .has_total_bytes = progress.has_total_bytes,
      .total_bytes = progress.total_bytes,
      .received_segments = progress.received_segments,
      .has_total_segments = progress.has_total_segments,
      .total_segments = progress.total_segments,
  };
  return ffi_progress;
}

static void free_borrowed_ffi_download_task(PlayerFfiDownloadTask *task) {
  if (task == NULL) {
    return;
  }
  free_borrowed_ffi_download_asset_index(&task->asset_index);
  memset(task, 0, sizeof(*task));
}

static bool ffi_download_task_from_runtime(
    const VesperRuntimeDownloadTask *task,
    PlayerFfiDownloadTask *out_task) {
  if (task == NULL || out_task == NULL) {
    return false;
  }
  memset(out_task, 0, sizeof(*out_task));

  out_task->task_id = task->task_id;
  out_task->asset_id = task->asset_id;
  out_task->source = ffi_download_source_from_runtime(&task->source);
  out_task->profile = ffi_download_profile_from_runtime(&task->profile);
  out_task->status = (PlayerFfiDownloadTaskStatus)task->status;
  out_task->progress = ffi_download_progress_from_runtime(task->progress);
  out_task->has_error = task->has_error;
  out_task->error_code = task->error_code;
  out_task->error_category = task->error_category;
  out_task->error_retriable = task->error_retriable;
  out_task->error_message = task->error_message;
  if (!ffi_download_asset_index_from_runtime(&task->asset_index, &out_task->asset_index)) {
    free_borrowed_ffi_download_task(out_task);
    return false;
  }
  return true;
}

static void free_borrowed_ffi_download_tasks(PlayerFfiDownloadTask *tasks, uintptr_t len) {
  if (tasks == NULL) {
    return;
  }
  for (uintptr_t index = 0; index < len; index += 1) {
    free_borrowed_ffi_download_task(&tasks[index]);
  }
  free(tasks);
}

static bool copy_runtime_download_tasks_to_ffi(
    const VesperRuntimeDownloadTask *tasks,
    uintptr_t len,
    PlayerFfiDownloadTask **out_tasks) {
  if (out_tasks == NULL) {
    return false;
  }
  *out_tasks = NULL;
  if (len == 0) {
    return true;
  }
  if (tasks == NULL || !can_allocate_items(len, sizeof(PlayerFfiDownloadTask))) {
    return false;
  }

  PlayerFfiDownloadTask *ffi_tasks = calloc((size_t)len, sizeof(PlayerFfiDownloadTask));
  if (ffi_tasks == NULL) {
    return false;
  }
  for (uintptr_t index = 0; index < len; index += 1) {
    if (!ffi_download_task_from_runtime(&tasks[index], &ffi_tasks[index])) {
      free_borrowed_ffi_download_tasks(ffi_tasks, len);
      return false;
    }
  }
  *out_tasks = ffi_tasks;
  return true;
}

static void free_runtime_download_source_strings(VesperRuntimeDownloadSource *source) {
  if (source == NULL) {
    return;
  }
  uintptr_t headers_len = source->headers_len;
  free(source->source_uri);
  free(source->manifest_uri);
  free_runtime_string_list(source->header_names, headers_len);
  free_runtime_string_list(source->header_values, headers_len);
  memset(source, 0, sizeof(*source));
}

static void free_runtime_download_profile_strings(VesperRuntimeDownloadProfile *profile) {
  if (profile == NULL) {
    return;
  }
  free(profile->variant_id);
  free(profile->preferred_audio_language);
  free(profile->preferred_subtitle_language);
  free_runtime_string_list(profile->selected_track_ids, profile->selected_track_ids_len);
  free(profile->target_directory);
  memset(profile, 0, sizeof(*profile));
}

static void free_runtime_download_resource_strings(VesperRuntimeDownloadResourceRecord *resource) {
  if (resource == NULL) {
    return;
  }
  free(resource->resource_id);
  free(resource->uri);
  free(resource->relative_path);
  free(resource->generated_text);
  free(resource->etag);
  free(resource->checksum);
  memset(resource, 0, sizeof(*resource));
}

static void free_runtime_download_segment_strings(VesperRuntimeDownloadSegmentRecord *segment) {
  if (segment == NULL) {
    return;
  }
  free(segment->segment_id);
  free(segment->uri);
  free(segment->relative_path);
  free(segment->checksum);
  memset(segment, 0, sizeof(*segment));
}

static void free_runtime_download_stream_strings(VesperRuntimeDownloadAssetStream *stream) {
  if (stream == NULL) {
    return;
  }
  free(stream->stream_id);
  free(stream->language);
  free(stream->codec);
  free(stream->label);
  free_runtime_string_list(stream->resource_ids, stream->resource_ids_len);
  free_runtime_string_list(stream->segment_ids, stream->segment_ids_len);
  free_runtime_string_list(stream->metadata_keys, stream->metadata_len);
  free_runtime_string_list(stream->metadata_values, stream->metadata_len);
  memset(stream, 0, sizeof(*stream));
}

static void free_runtime_download_asset_index_strings(VesperRuntimeDownloadAssetIndex *asset_index) {
  if (asset_index == NULL) {
    return;
  }
  free(asset_index->version);
  free(asset_index->etag);
  free(asset_index->checksum);
  if (asset_index->resources != NULL) {
    for (uintptr_t index = 0; index < asset_index->resources_len; index += 1) {
      free_runtime_download_resource_strings(&asset_index->resources[index]);
    }
    free(asset_index->resources);
  }
  if (asset_index->segments != NULL) {
    for (uintptr_t index = 0; index < asset_index->segments_len; index += 1) {
      free_runtime_download_segment_strings(&asset_index->segments[index]);
    }
    free(asset_index->segments);
  }
  if (asset_index->streams != NULL) {
    for (uintptr_t index = 0; index < asset_index->streams_len; index += 1) {
      free_runtime_download_stream_strings(&asset_index->streams[index]);
    }
    free(asset_index->streams);
  }
  free(asset_index->completed_path);
  memset(asset_index, 0, sizeof(*asset_index));
}

static void free_runtime_download_task_strings(VesperRuntimeDownloadTask *task) {
  if (task == NULL) {
    return;
  }
  free(task->asset_id);
  free_runtime_download_source_strings(&task->source);
  free_runtime_download_profile_strings(&task->profile);
  free_runtime_download_asset_index_strings(&task->asset_index);
  free(task->error_message);
  memset(task, 0, sizeof(*task));
}

static void free_runtime_download_command_strings(VesperRuntimeDownloadCommand *command) {
  if (command == NULL) {
    return;
  }
  free_runtime_download_task_strings(&command->task);
  memset(command, 0, sizeof(*command));
}

static void free_runtime_download_event_strings(VesperRuntimeDownloadEvent *event) {
  if (event == NULL) {
    return;
  }
  if (event->task != NULL) {
    free_runtime_download_task_strings(event->task);
    free(event->task);
  }
  free(event->state_error_message);
  free(event->state_completed_path);
  memset(event, 0, sizeof(*event));
}

static bool runtime_download_source_from_ffi(
    const PlayerFfiDownloadSource *source,
    VesperRuntimeDownloadSource *out_source) {
  if (source == NULL || out_source == NULL) {
    return false;
  }
  memset(out_source, 0, sizeof(*out_source));
  out_source->content_format = (VesperRuntimeDownloadContentFormat)source->content_format;
  out_source->headers_len = source->headers_len;
  if (!duplicate_runtime_string(source->source_uri, &out_source->source_uri) ||
      !duplicate_runtime_string(source->manifest_uri, &out_source->manifest_uri) ||
      !copy_ffi_string_list_to_runtime(
          source->header_names,
          source->headers_len,
          &out_source->header_names) ||
      !copy_ffi_string_list_to_runtime(
          source->header_values,
          source->headers_len,
          &out_source->header_values)) {
    free_runtime_download_source_strings(out_source);
    return false;
  }
  return true;
}

static bool runtime_download_profile_from_ffi(
    const PlayerFfiDownloadProfile *profile,
    VesperRuntimeDownloadProfile *out_profile) {
  if (profile == NULL || out_profile == NULL) {
    return false;
  }
  memset(out_profile, 0, sizeof(*out_profile));
  out_profile->selected_track_ids_len = profile->selected_track_ids_len;
  out_profile->has_target_output_format = profile->has_target_output_format;
  out_profile->target_output_format =
      (VesperRuntimeDownloadOutputFormat)profile->target_output_format;
  out_profile->allow_metered_network = profile->allow_metered_network;
  if (!duplicate_runtime_string(profile->variant_id, &out_profile->variant_id) ||
      !duplicate_runtime_string(
          profile->preferred_audio_language,
          &out_profile->preferred_audio_language) ||
      !duplicate_runtime_string(
          profile->preferred_subtitle_language,
          &out_profile->preferred_subtitle_language) ||
      !copy_ffi_string_list_to_runtime(
          profile->selected_track_ids,
          profile->selected_track_ids_len,
          &out_profile->selected_track_ids) ||
      !duplicate_runtime_string(profile->target_directory, &out_profile->target_directory)) {
    free_runtime_download_profile_strings(out_profile);
    return false;
  }
  return true;
}

static bool runtime_download_resource_from_ffi(
    const PlayerFfiDownloadResourceRecord *resource,
    VesperRuntimeDownloadResourceRecord *out_resource) {
  if (resource == NULL || out_resource == NULL) {
    return false;
  }
  memset(out_resource, 0, sizeof(*out_resource));
  out_resource->has_byte_range = resource->has_byte_range;
  out_resource->byte_range = runtime_download_byte_range_from_ffi(resource->byte_range);
  out_resource->generated_text = NULL;
  out_resource->has_size_bytes = resource->has_size_bytes;
  out_resource->size_bytes = resource->size_bytes;
  if (!duplicate_runtime_string(resource->resource_id, &out_resource->resource_id) ||
      !duplicate_runtime_string(resource->uri, &out_resource->uri) ||
      !duplicate_runtime_string(resource->relative_path, &out_resource->relative_path) ||
      !duplicate_runtime_string(resource->etag, &out_resource->etag) ||
      !duplicate_runtime_string(resource->checksum, &out_resource->checksum)) {
    free_runtime_download_resource_strings(out_resource);
    return false;
  }
  return true;
}

static bool runtime_download_segment_from_ffi(
    const PlayerFfiDownloadSegmentRecord *segment,
    VesperRuntimeDownloadSegmentRecord *out_segment) {
  if (segment == NULL || out_segment == NULL) {
    return false;
  }
  memset(out_segment, 0, sizeof(*out_segment));
  out_segment->has_sequence = segment->has_sequence;
  out_segment->sequence = segment->sequence;
  out_segment->has_byte_range = segment->has_byte_range;
  out_segment->byte_range = runtime_download_byte_range_from_ffi(segment->byte_range);
  out_segment->has_size_bytes = segment->has_size_bytes;
  out_segment->size_bytes = segment->size_bytes;
  if (!duplicate_runtime_string(segment->segment_id, &out_segment->segment_id) ||
      !duplicate_runtime_string(segment->uri, &out_segment->uri) ||
      !duplicate_runtime_string(segment->relative_path, &out_segment->relative_path) ||
      !duplicate_runtime_string(segment->checksum, &out_segment->checksum)) {
    free_runtime_download_segment_strings(out_segment);
    return false;
  }
  return true;
}

static bool runtime_download_stream_from_ffi(
    const PlayerFfiDownloadAssetStream *stream,
    VesperRuntimeDownloadAssetStream *out_stream) {
  if (stream == NULL || out_stream == NULL) {
    return false;
  }
  memset(out_stream, 0, sizeof(*out_stream));
  out_stream->kind = (VesperRuntimeDownloadStreamKind)stream->kind;
  out_stream->has_quality_rank = stream->has_quality_rank;
  out_stream->quality_rank = stream->quality_rank;
  out_stream->resource_ids_len = stream->resource_ids_len;
  out_stream->segment_ids_len = stream->segment_ids_len;
  out_stream->metadata_len = stream->metadata_len;
  if (!duplicate_runtime_string(stream->stream_id, &out_stream->stream_id) ||
      !duplicate_runtime_string(stream->language, &out_stream->language) ||
      !duplicate_runtime_string(stream->codec, &out_stream->codec) ||
      !duplicate_runtime_string(stream->label, &out_stream->label) ||
      !copy_ffi_string_list_to_runtime(
          stream->resource_ids,
          stream->resource_ids_len,
          &out_stream->resource_ids) ||
      !copy_ffi_string_list_to_runtime(
          stream->segment_ids,
          stream->segment_ids_len,
          &out_stream->segment_ids) ||
      !copy_ffi_string_list_to_runtime(
          stream->metadata_keys,
          stream->metadata_len,
          &out_stream->metadata_keys) ||
      !copy_ffi_string_list_to_runtime(
          stream->metadata_values,
          stream->metadata_len,
          &out_stream->metadata_values)) {
    free_runtime_download_stream_strings(out_stream);
    return false;
  }
  return true;
}

static bool copy_ffi_download_resources_to_runtime(
    const PlayerFfiDownloadResourceRecord *resources,
    uintptr_t len,
    VesperRuntimeDownloadResourceRecord **out_resources) {
  if (out_resources == NULL) {
    return false;
  }
  *out_resources = NULL;
  if (len == 0) {
    return true;
  }
  if (resources == NULL || !can_allocate_items(len, sizeof(VesperRuntimeDownloadResourceRecord))) {
    return false;
  }

  VesperRuntimeDownloadResourceRecord *runtime_resources =
      calloc((size_t)len, sizeof(VesperRuntimeDownloadResourceRecord));
  if (runtime_resources == NULL) {
    return false;
  }
  for (uintptr_t index = 0; index < len; index += 1) {
    if (!runtime_download_resource_from_ffi(&resources[index], &runtime_resources[index])) {
      for (uintptr_t cleanup_index = 0; cleanup_index < len; cleanup_index += 1) {
        free_runtime_download_resource_strings(&runtime_resources[cleanup_index]);
      }
      free(runtime_resources);
      return false;
    }
  }
  *out_resources = runtime_resources;
  return true;
}

static bool copy_ffi_download_segments_to_runtime(
    const PlayerFfiDownloadSegmentRecord *segments,
    uintptr_t len,
    VesperRuntimeDownloadSegmentRecord **out_segments) {
  if (out_segments == NULL) {
    return false;
  }
  *out_segments = NULL;
  if (len == 0) {
    return true;
  }
  if (segments == NULL || !can_allocate_items(len, sizeof(VesperRuntimeDownloadSegmentRecord))) {
    return false;
  }

  VesperRuntimeDownloadSegmentRecord *runtime_segments =
      calloc((size_t)len, sizeof(VesperRuntimeDownloadSegmentRecord));
  if (runtime_segments == NULL) {
    return false;
  }
  for (uintptr_t index = 0; index < len; index += 1) {
    if (!runtime_download_segment_from_ffi(&segments[index], &runtime_segments[index])) {
      for (uintptr_t cleanup_index = 0; cleanup_index < len; cleanup_index += 1) {
        free_runtime_download_segment_strings(&runtime_segments[cleanup_index]);
      }
      free(runtime_segments);
      return false;
    }
  }
  *out_segments = runtime_segments;
  return true;
}

static bool copy_ffi_download_streams_to_runtime(
    const PlayerFfiDownloadAssetStream *streams,
    uintptr_t len,
    VesperRuntimeDownloadAssetStream **out_streams) {
  if (out_streams == NULL) {
    return false;
  }
  *out_streams = NULL;
  if (len == 0) {
    return true;
  }
  if (streams == NULL || !can_allocate_items(len, sizeof(VesperRuntimeDownloadAssetStream))) {
    return false;
  }

  VesperRuntimeDownloadAssetStream *runtime_streams =
      calloc((size_t)len, sizeof(VesperRuntimeDownloadAssetStream));
  if (runtime_streams == NULL) {
    return false;
  }
  for (uintptr_t index = 0; index < len; index += 1) {
    if (!runtime_download_stream_from_ffi(&streams[index], &runtime_streams[index])) {
      for (uintptr_t cleanup_index = 0; cleanup_index < len; cleanup_index += 1) {
        free_runtime_download_stream_strings(&runtime_streams[cleanup_index]);
      }
      free(runtime_streams);
      return false;
    }
  }
  *out_streams = runtime_streams;
  return true;
}

static bool runtime_download_asset_index_from_ffi(
    const PlayerFfiDownloadAssetIndex *asset_index,
    VesperRuntimeDownloadAssetIndex *out_asset_index) {
  if (asset_index == NULL || out_asset_index == NULL) {
    return false;
  }
  memset(out_asset_index, 0, sizeof(*out_asset_index));
  out_asset_index->content_format =
      (VesperRuntimeDownloadContentFormat)asset_index->content_format;
  out_asset_index->has_total_size_bytes = asset_index->has_total_size_bytes;
  out_asset_index->total_size_bytes = asset_index->total_size_bytes;
  out_asset_index->resources_len = asset_index->resources_len;
  out_asset_index->segments_len = asset_index->segments_len;
  out_asset_index->streams_len = asset_index->streams_len;
  if (!duplicate_runtime_string(asset_index->version, &out_asset_index->version) ||
      !duplicate_runtime_string(asset_index->etag, &out_asset_index->etag) ||
      !duplicate_runtime_string(asset_index->checksum, &out_asset_index->checksum) ||
      !copy_ffi_download_resources_to_runtime(
          asset_index->resources,
          asset_index->resources_len,
          &out_asset_index->resources) ||
      !copy_ffi_download_segments_to_runtime(
          asset_index->segments,
          asset_index->segments_len,
          &out_asset_index->segments) ||
      !copy_ffi_download_streams_to_runtime(
          asset_index->streams,
          asset_index->streams_len,
          &out_asset_index->streams) ||
      !duplicate_runtime_string(asset_index->completed_path, &out_asset_index->completed_path)) {
    free_runtime_download_asset_index_strings(out_asset_index);
    return false;
  }
  return true;
}

static VesperRuntimeDownloadProgressSnapshot runtime_download_progress_from_ffi(
    PlayerFfiDownloadProgressSnapshot progress) {
  VesperRuntimeDownloadProgressSnapshot runtime_progress = {
      .received_bytes = progress.received_bytes,
      .has_total_bytes = progress.has_total_bytes,
      .total_bytes = progress.total_bytes,
      .received_segments = progress.received_segments,
      .has_total_segments = progress.has_total_segments,
      .total_segments = progress.total_segments,
  };
  return runtime_progress;
}

static bool runtime_download_task_from_ffi(
    const PlayerFfiDownloadTask *task,
    VesperRuntimeDownloadTask *out_task) {
  if (task == NULL || out_task == NULL) {
    return false;
  }
  memset(out_task, 0, sizeof(*out_task));
  out_task->task_id = task->task_id;
  out_task->status = (VesperRuntimeDownloadTaskStatus)task->status;
  out_task->progress = runtime_download_progress_from_ffi(task->progress);
  out_task->has_error = task->has_error;
  out_task->error_code = task->error_code;
  out_task->error_category = task->error_category;
  out_task->error_retriable = task->error_retriable;
  if (!duplicate_runtime_string(task->asset_id, &out_task->asset_id) ||
      !runtime_download_source_from_ffi(&task->source, &out_task->source) ||
      !runtime_download_profile_from_ffi(&task->profile, &out_task->profile) ||
      !runtime_download_asset_index_from_ffi(&task->asset_index, &out_task->asset_index) ||
      !duplicate_runtime_string(task->error_message, &out_task->error_message)) {
    free_runtime_download_task_strings(out_task);
    return false;
  }
  return true;
}

static bool runtime_download_command_from_ffi(
    const PlayerFfiDownloadCommand *command,
    VesperRuntimeDownloadCommand *out_command) {
  if (command == NULL || out_command == NULL) {
    return false;
  }
  memset(out_command, 0, sizeof(*out_command));
  out_command->kind = (VesperRuntimeDownloadCommandKind)command->kind;
  out_command->task_id = command->task_id;
  if (!runtime_download_task_from_ffi(&command->task, &out_command->task)) {
    free_runtime_download_command_strings(out_command);
    return false;
  }
  return true;
}

static bool runtime_download_event_from_ffi(
    const PlayerFfiDownloadEvent *event,
    VesperRuntimeDownloadEvent *out_event) {
  if (event == NULL || out_event == NULL) {
    return false;
  }
  memset(out_event, 0, sizeof(*out_event));
  out_event->kind = (VesperRuntimeDownloadEventKind)event->kind;
  if (event->kind == PlayerFfiDownloadEventKindCreated ||
      event->kind == PlayerFfiDownloadEventKindAssetIndexUpdated) {
    out_event->task = calloc(1, sizeof(VesperRuntimeDownloadTask));
    if (out_event->task == NULL ||
        !runtime_download_task_from_ffi(&event->task, out_event->task)) {
      free_runtime_download_event_strings(out_event);
      return false;
    }
  } else if (event->kind == PlayerFfiDownloadEventKindStateChanged) {
    out_event->task_id = event->task_id;
    out_event->state_status = (VesperRuntimeDownloadTaskStatus)event->status;
    out_event->state_progress = runtime_download_progress_from_ffi(event->progress);
    out_event->state_has_error = event->has_error;
    out_event->state_error_code = event->error_code;
    out_event->state_error_category = event->error_category;
    out_event->state_error_retriable = event->error_retriable;
    if (!duplicate_runtime_string(event->error_message, &out_event->state_error_message) ||
        !duplicate_runtime_string(event->completed_path, &out_event->state_completed_path)) {
      free_runtime_download_event_strings(out_event);
      return false;
    }
  } else if (event->kind == PlayerFfiDownloadEventKindProgressUpdated) {
    out_event->task_id = event->task_id;
    out_event->progress = runtime_download_progress_from_ffi(event->progress);
  }
  return true;
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
    PlayerFfiErrorCode error_code,
    PlayerFfiErrorCategory error_category,
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

void vesper_runtime_preload_command_list_free(VesperRuntimePreloadCommandList *commands) {
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

bool vesper_runtime_playlist_session_clear_viewport_hints(uint64_t handle) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_playlist_session_clear_viewport_hints(handle, &ffi_error),
      &ffi_error);
}

bool vesper_runtime_playlist_session_advance_to_next(uint64_t handle) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_playlist_session_advance_to_next(handle, &ffi_error),
      &ffi_error);
}

bool vesper_runtime_playlist_session_advance_to_previous(uint64_t handle) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_playlist_session_advance_to_previous(handle, &ffi_error),
      &ffi_error);
}

bool vesper_runtime_playlist_session_handle_playback_completed(uint64_t handle) {
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_playlist_session_handle_playback_completed(handle, &ffi_error),
      &ffi_error);
}

bool vesper_runtime_playlist_session_handle_playback_failed(uint64_t handle) {
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
    PlayerFfiErrorCode error_code,
    PlayerFfiErrorCategory error_category,
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

void vesper_runtime_playlist_active_item_free(VesperRuntimePlaylistActiveItem *item) {
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
  PlayerFfiDownloadConfig ffi_config = ffi_download_config_from_runtime(config);
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_download_session_create(
          &ffi_config,
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
  PlayerFfiDownloadSource ffi_source = ffi_download_source_from_runtime(source);
  PlayerFfiDownloadProfile ffi_profile = ffi_download_profile_from_runtime(profile);
  PlayerFfiDownloadAssetIndex ffi_asset_index;
  if (!ffi_download_asset_index_from_runtime(asset_index, &ffi_asset_index)) {
    return false;
  }
  memset(&ffi_error, 0, sizeof(ffi_error));
  PlayerFfiCallStatus status = player_ffi_download_session_create_task(
      handle,
      asset_id,
      &ffi_source,
      &ffi_profile,
      &ffi_asset_index,
      out_task_id,
      &ffi_error);
  free_borrowed_ffi_download_asset_index(&ffi_asset_index);
  return call_playlist_status(status, &ffi_error);
}

bool vesper_runtime_download_session_restore_tasks(
    uint64_t handle,
    const VesperRuntimeDownloadTask *tasks,
    size_t tasks_len) {
  if (tasks_len > 0 && tasks == NULL) {
    return false;
  }

  PlayerFfiError ffi_error;
  PlayerFfiDownloadTask *ffi_tasks = NULL;
  if (!copy_runtime_download_tasks_to_ffi(tasks, tasks_len, &ffi_tasks)) {
    return false;
  }
  memset(&ffi_error, 0, sizeof(ffi_error));
  PlayerFfiCallStatus status = player_ffi_download_session_restore_tasks(
      handle,
      ffi_tasks,
      (uintptr_t)tasks_len,
      &ffi_error);
  free_borrowed_ffi_download_tasks(ffi_tasks, tasks_len);
  return call_playlist_status(status, &ffi_error);
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

bool vesper_runtime_download_session_complete_preparation(
    uint64_t handle,
    uint64_t task_id,
    const VesperRuntimeDownloadAssetIndex *asset_index) {
  if (asset_index == NULL) {
    return false;
  }

  PlayerFfiError ffi_error;
  PlayerFfiDownloadAssetIndex ffi_asset_index;
  if (!ffi_download_asset_index_from_runtime(asset_index, &ffi_asset_index)) {
    return false;
  }
  memset(&ffi_error, 0, sizeof(ffi_error));
  PlayerFfiCallStatus status = player_ffi_download_session_complete_preparation(
      handle,
      task_id,
      &ffi_asset_index,
      &ffi_error);
  free_borrowed_ffi_download_asset_index(&ffi_asset_index);
  return call_playlist_status(status, &ffi_error);
}

bool vesper_runtime_download_session_replace_task_plan(
    uint64_t handle,
    uint64_t task_id,
    const VesperRuntimeDownloadSource *source,
    const VesperRuntimeDownloadProfile *profile,
    const VesperRuntimeDownloadAssetIndex *asset_index) {
  if (source == NULL || profile == NULL || asset_index == NULL) {
    return false;
  }

  PlayerFfiError ffi_error;
  PlayerFfiDownloadSource ffi_source = ffi_download_source_from_runtime(source);
  PlayerFfiDownloadProfile ffi_profile = ffi_download_profile_from_runtime(profile);
  PlayerFfiDownloadAssetIndex ffi_asset_index;
  if (!ffi_download_asset_index_from_runtime(asset_index, &ffi_asset_index)) {
    return false;
  }
  memset(&ffi_error, 0, sizeof(ffi_error));
  PlayerFfiCallStatus status = player_ffi_download_session_replace_task_plan(
      handle,
      task_id,
      &ffi_source,
      &ffi_profile,
      &ffi_asset_index,
      &ffi_error);
  free_borrowed_ffi_download_asset_index(&ffi_asset_index);
  return call_playlist_status(status, &ffi_error);
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
    PlayerFfiErrorCode error_code,
    PlayerFfiErrorCategory error_category,
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

  out_snapshot->len = ffi_snapshot.len;
  out_snapshot->tasks = NULL;
  if (ffi_snapshot.len == 0) {
    player_ffi_download_snapshot_free(&ffi_snapshot);
    return true;
  }
  if (ffi_snapshot.tasks == NULL) {
    player_ffi_download_snapshot_free(&ffi_snapshot);
    out_snapshot->len = 0;
    return false;
  }

  if (!can_allocate_items(ffi_snapshot.len, sizeof(VesperRuntimeDownloadTask))) {
    player_ffi_download_snapshot_free(&ffi_snapshot);
    out_snapshot->len = 0;
    return false;
  }

  out_snapshot->tasks = calloc((size_t)ffi_snapshot.len, sizeof(VesperRuntimeDownloadTask));
  if (out_snapshot->tasks == NULL) {
    player_ffi_download_snapshot_free(&ffi_snapshot);
    out_snapshot->len = 0;
    return false;
  }

  for (uintptr_t index = 0; index < ffi_snapshot.len; index += 1) {
    if (!runtime_download_task_from_ffi(
            &ffi_snapshot.tasks[index],
            &out_snapshot->tasks[index])) {
      vesper_runtime_download_snapshot_free(out_snapshot);
      player_ffi_download_snapshot_free(&ffi_snapshot);
      return false;
    }
  }
  player_ffi_download_snapshot_free(&ffi_snapshot);
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

  out_commands->len = ffi_commands.len;
  out_commands->commands = NULL;
  if (ffi_commands.len == 0) {
    player_ffi_download_command_list_free(&ffi_commands);
    return true;
  }
  if (ffi_commands.commands == NULL) {
    player_ffi_download_command_list_free(&ffi_commands);
    out_commands->len = 0;
    return false;
  }

  if (!can_allocate_items(ffi_commands.len, sizeof(VesperRuntimeDownloadCommand))) {
    player_ffi_download_command_list_free(&ffi_commands);
    out_commands->len = 0;
    return false;
  }

  out_commands->commands = calloc((size_t)ffi_commands.len, sizeof(VesperRuntimeDownloadCommand));
  if (out_commands->commands == NULL) {
    player_ffi_download_command_list_free(&ffi_commands);
    out_commands->len = 0;
    return false;
  }

  for (uintptr_t index = 0; index < ffi_commands.len; index += 1) {
    if (!runtime_download_command_from_ffi(
            &ffi_commands.commands[index],
            &out_commands->commands[index])) {
      vesper_runtime_download_command_list_free(out_commands);
      player_ffi_download_command_list_free(&ffi_commands);
      return false;
    }
  }
  player_ffi_download_command_list_free(&ffi_commands);
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

  out_events->len = ffi_events.len;
  out_events->events = NULL;
  if (ffi_events.len == 0) {
    player_ffi_download_event_list_free(&ffi_events);
    return true;
  }
  if (ffi_events.events == NULL) {
    player_ffi_download_event_list_free(&ffi_events);
    out_events->len = 0;
    return false;
  }

  if (!can_allocate_items(ffi_events.len, sizeof(VesperRuntimeDownloadEvent))) {
    player_ffi_download_event_list_free(&ffi_events);
    out_events->len = 0;
    return false;
  }

  out_events->events = calloc((size_t)ffi_events.len, sizeof(VesperRuntimeDownloadEvent));
  if (out_events->events == NULL) {
    player_ffi_download_event_list_free(&ffi_events);
    out_events->len = 0;
    return false;
  }

  for (uintptr_t index = 0; index < ffi_events.len; index += 1) {
    if (!runtime_download_event_from_ffi(
            &ffi_events.events[index],
            &out_events->events[index])) {
      vesper_runtime_download_event_list_free(out_events);
      player_ffi_download_event_list_free(&ffi_events);
      return false;
    }
  }
  player_ffi_download_event_list_free(&ffi_events);
  return true;
}

void vesper_runtime_download_snapshot_free(VesperRuntimeDownloadSnapshot *snapshot) {
  if (snapshot == NULL) {
    return;
  }
  if (snapshot->tasks != NULL) {
    for (uintptr_t index = 0; index < snapshot->len; index += 1) {
      free_runtime_download_task_strings(&snapshot->tasks[index]);
    }
    free(snapshot->tasks);
  }
  memset(snapshot, 0, sizeof(*snapshot));
}

void vesper_runtime_download_command_list_free(VesperRuntimeDownloadCommandList *commands) {
  if (commands == NULL) {
    return;
  }
  if (commands->commands != NULL) {
    for (uintptr_t index = 0; index < commands->len; index += 1) {
      free_runtime_download_command_strings(&commands->commands[index]);
    }
    free(commands->commands);
  }
  memset(commands, 0, sizeof(*commands));
}

void vesper_runtime_download_event_list_free(VesperRuntimeDownloadEventList *events) {
  if (events == NULL) {
    return;
  }
  if (events->events != NULL) {
    for (uintptr_t index = 0; index < events->len; index += 1) {
      free_runtime_download_event_strings(&events->events[index]);
    }
    free(events->events);
  }
  memset(events, 0, sizeof(*events));
}

void vesper_runtime_download_session_dispose(uint64_t handle) {
  player_ffi_download_session_dispose(handle);
}

void vesper_runtime_track_preferences_free(VesperRuntimeTrackPreferencePolicy *track_preferences) {
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

bool vesper_mobile_plugin_diagnostics_json(
    const char *source_uri,
    uint32_t source_mode,
    char **source_plugin_library_paths,
    uintptr_t source_plugin_library_paths_len,
    const char *runtime_profile,
    uint32_t frame_mode,
    char **frame_plugin_library_paths,
    uintptr_t frame_plugin_library_paths_len,
    char **out_json,
    char **out_error_message) {
  if (source_uri == NULL || out_json == NULL) {
    return false;
  }
  if (out_error_message != NULL) {
    *out_error_message = NULL;
  }
  *out_json = NULL;

  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_mobile_plugin_diagnostics_json(
      source_uri,
      source_mode,
      source_plugin_library_paths,
      source_plugin_library_paths_len,
      runtime_profile,
      frame_mode,
      frame_plugin_library_paths,
      frame_plugin_library_paths_len,
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

void vesper_mobile_plugin_diagnostics_string_free(char *value) {
  player_ffi_mobile_plugin_diagnostics_string_free(value);
}

bool vesper_source_normalizer_resource_open(
    const char *source_uri,
    uint32_t source_mode,
    char **source_plugin_library_paths,
    uintptr_t source_plugin_library_paths_len,
    const char *runtime_profile,
    const char *output_root,
    bool force_normalized,
    uint64_t *out_handle,
    char **out_json,
    char **out_error_message) {
  if (source_uri == NULL || output_root == NULL || out_handle == NULL || out_json == NULL) {
    return false;
  }
  if (out_error_message != NULL) {
    *out_error_message = NULL;
  }
  *out_handle = 0;
  *out_json = NULL;

  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_source_normalizer_resource_open(
      source_uri,
      source_mode,
      source_plugin_library_paths,
      source_plugin_library_paths_len,
      runtime_profile,
      output_root,
      force_normalized,
      out_handle,
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
  return *out_handle != 0 && *out_json != NULL;
}

bool vesper_source_normalizer_resource_poll(
    uint64_t handle,
    char **out_json,
    char **out_error_message) {
  if (handle == 0 || out_json == NULL) {
    return false;
  }
  if (out_error_message != NULL) {
    *out_error_message = NULL;
  }
  *out_json = NULL;

  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_source_normalizer_resource_poll(
      handle,
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

void vesper_source_normalizer_resource_dispose(uint64_t handle) {
  player_ffi_source_normalizer_resource_dispose(handle);
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

bool vesper_dash_bridge_parse_sidx(
    const uint8_t *data,
    uintptr_t data_len,
    char **out_json,
    char **out_error_message) {
  if ((data == NULL && data_len > 0) || out_json == NULL) {
    return false;
  }
  if (out_error_message != NULL) {
    *out_error_message = NULL;
  }
  *out_json = NULL;

  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_dash_bridge_parse_sidx(
      data,
      data_len,
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
