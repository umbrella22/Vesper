#ifndef VESPER_PLAYER_KIT_BRIDGE_SHIM_H
#define VESPER_PLAYER_KIT_BRIDGE_SHIM_H

#include <stdbool.h>
#include <stdint.h>

typedef struct VesperRuntimeBufferingPolicy {
  int preset_ordinal;
  bool has_min_buffer_ms;
  int64_t min_buffer_ms;
  bool has_max_buffer_ms;
  int64_t max_buffer_ms;
  bool has_buffer_for_playback_ms;
  int64_t buffer_for_playback_ms;
  bool has_buffer_for_rebuffer_ms;
  int64_t buffer_for_rebuffer_ms;
} VesperRuntimeBufferingPolicy;

typedef struct VesperRuntimeRetryPolicy {
  bool uses_default_max_attempts;
  bool has_max_attempts;
  int32_t max_attempts;
  bool has_base_delay_ms;
  uint64_t base_delay_ms;
  bool has_max_delay_ms;
  uint64_t max_delay_ms;
  bool has_backoff;
  int backoff_ordinal;
} VesperRuntimeRetryPolicy;

typedef struct VesperRuntimeCachePolicy {
  int preset_ordinal;
  bool has_max_memory_bytes;
  int64_t max_memory_bytes;
  bool has_max_disk_bytes;
  int64_t max_disk_bytes;
} VesperRuntimeCachePolicy;

typedef struct VesperRuntimeResolvedResiliencePolicy {
  VesperRuntimeBufferingPolicy buffering;
  VesperRuntimeRetryPolicy retry;
  VesperRuntimeCachePolicy cache;
} VesperRuntimeResolvedResiliencePolicy;

typedef struct VesperRuntimePreloadBudgetPolicy {
  bool has_max_concurrent_tasks;
  uint32_t max_concurrent_tasks;
  bool has_max_memory_bytes;
  int64_t max_memory_bytes;
  bool has_max_disk_bytes;
  int64_t max_disk_bytes;
  bool has_warmup_window_ms;
  int64_t warmup_window_ms;
} VesperRuntimePreloadBudgetPolicy;

typedef struct VesperRuntimeResolvedPreloadBudgetPolicy {
  uint32_t max_concurrent_tasks;
  int64_t max_memory_bytes;
  int64_t max_disk_bytes;
  uint64_t warmup_window_ms;
} VesperRuntimeResolvedPreloadBudgetPolicy;

typedef enum VesperRuntimePreloadScopeKind {
  VesperRuntimePreloadScopeKindApp = 0,
  VesperRuntimePreloadScopeKindSession = 1,
  VesperRuntimePreloadScopeKindPlaylist = 2,
} VesperRuntimePreloadScopeKind;

typedef enum VesperRuntimePreloadCandidateKind {
  VesperRuntimePreloadCandidateKindCurrent = 0,
  VesperRuntimePreloadCandidateKindNeighbor = 1,
  VesperRuntimePreloadCandidateKindRecommended = 2,
  VesperRuntimePreloadCandidateKindBackground = 3,
} VesperRuntimePreloadCandidateKind;

typedef enum VesperRuntimePreloadSelectionHint {
  VesperRuntimePreloadSelectionHintNone = 0,
  VesperRuntimePreloadSelectionHintCurrentItem = 1,
  VesperRuntimePreloadSelectionHintNeighborItem = 2,
  VesperRuntimePreloadSelectionHintRecommendedItem = 3,
  VesperRuntimePreloadSelectionHintBackgroundFill = 4,
} VesperRuntimePreloadSelectionHint;

typedef enum VesperRuntimePreloadPriority {
  VesperRuntimePreloadPriorityCritical = 0,
  VesperRuntimePreloadPriorityHigh = 1,
  VesperRuntimePreloadPriorityNormal = 2,
  VesperRuntimePreloadPriorityLow = 3,
  VesperRuntimePreloadPriorityBackground = 4,
} VesperRuntimePreloadPriority;

typedef enum VesperRuntimePreloadTaskStatus {
  VesperRuntimePreloadTaskStatusPlanned = 0,
  VesperRuntimePreloadTaskStatusActive = 1,
  VesperRuntimePreloadTaskStatusCancelled = 2,
  VesperRuntimePreloadTaskStatusCompleted = 3,
  VesperRuntimePreloadTaskStatusExpired = 4,
  VesperRuntimePreloadTaskStatusFailed = 5,
} VesperRuntimePreloadTaskStatus;

typedef struct VesperRuntimePreloadCandidate {
  const char *source_uri;
  VesperRuntimePreloadScopeKind scope_kind;
  const char *scope_id;
  VesperRuntimePreloadCandidateKind candidate_kind;
  VesperRuntimePreloadSelectionHint selection_hint;
  VesperRuntimePreloadPriority priority;
  uint64_t expected_memory_bytes;
  uint64_t expected_disk_bytes;
  bool has_ttl_ms;
  uint64_t ttl_ms;
  bool has_warmup_window_ms;
  uint64_t warmup_window_ms;
} VesperRuntimePreloadCandidate;

typedef struct VesperRuntimePreloadTask {
  uint64_t task_id;
  char *source_uri;
  char *source_identity;
  char *cache_key;
  VesperRuntimePreloadScopeKind scope_kind;
  char *scope_id;
  VesperRuntimePreloadCandidateKind candidate_kind;
  VesperRuntimePreloadSelectionHint selection_hint;
  VesperRuntimePreloadPriority priority;
  VesperRuntimePreloadTaskStatus status;
  uint64_t expected_memory_bytes;
  uint64_t expected_disk_bytes;
  uint64_t warmup_window_ms;
  bool has_error;
  uint32_t error_code;
  uint32_t error_category;
  bool error_retriable;
  char *error_message;
} VesperRuntimePreloadTask;

typedef enum VesperRuntimePreloadCommandKind {
  VesperRuntimePreloadCommandKindStart = 0,
  VesperRuntimePreloadCommandKindCancel = 1,
} VesperRuntimePreloadCommandKind;

typedef struct VesperRuntimePreloadCommand {
  VesperRuntimePreloadCommandKind kind;
  VesperRuntimePreloadTask task;
  uint64_t task_id;
} VesperRuntimePreloadCommand;

typedef struct VesperRuntimePreloadCommandList {
  VesperRuntimePreloadCommand *commands;
  uintptr_t len;
} VesperRuntimePreloadCommandList;

typedef enum VesperRuntimePlaylistRepeatMode {
  VesperRuntimePlaylistRepeatModeOff = 0,
  VesperRuntimePlaylistRepeatModeOne = 1,
  VesperRuntimePlaylistRepeatModeAll = 2,
} VesperRuntimePlaylistRepeatMode;

typedef enum VesperRuntimePlaylistFailureStrategy {
  VesperRuntimePlaylistFailureStrategyPause = 0,
  VesperRuntimePlaylistFailureStrategySkipToNext = 1,
} VesperRuntimePlaylistFailureStrategy;

typedef enum VesperRuntimePlaylistViewportHintKind {
  VesperRuntimePlaylistViewportHintKindVisible = 0,
  VesperRuntimePlaylistViewportHintKindNearVisible = 1,
  VesperRuntimePlaylistViewportHintKindPrefetchOnly = 2,
  VesperRuntimePlaylistViewportHintKindHidden = 3,
} VesperRuntimePlaylistViewportHintKind;

typedef struct VesperRuntimePlaylistConfig {
  const char *playlist_id;
  uint32_t neighbor_previous;
  uint32_t neighbor_next;
  uint32_t preload_near_visible;
  uint32_t preload_prefetch_only;
  bool auto_advance;
  VesperRuntimePlaylistRepeatMode repeat_mode;
  VesperRuntimePlaylistFailureStrategy failure_strategy;
} VesperRuntimePlaylistConfig;

typedef struct VesperRuntimePlaylistQueueItem {
  const char *item_id;
  const char *source_uri;
  uint64_t expected_memory_bytes;
  uint64_t expected_disk_bytes;
  bool has_ttl_ms;
  uint64_t ttl_ms;
  bool has_warmup_window_ms;
  uint64_t warmup_window_ms;
} VesperRuntimePlaylistQueueItem;

typedef struct VesperRuntimePlaylistViewportHint {
  const char *item_id;
  VesperRuntimePlaylistViewportHintKind kind;
  uint32_t order;
} VesperRuntimePlaylistViewportHint;

typedef struct VesperRuntimePlaylistActiveItem {
  char *item_id;
  uint32_t index;
} VesperRuntimePlaylistActiveItem;

typedef struct VesperRuntimeDownloadConfig {
  bool auto_start;
  bool run_post_processors_on_completion;
  char **plugin_library_paths;
  uintptr_t plugin_library_paths_len;
} VesperRuntimeDownloadConfig;

typedef enum VesperRuntimeDownloadContentFormat {
  VesperRuntimeDownloadContentFormatHlsSegments = 0,
  VesperRuntimeDownloadContentFormatDashSegments = 1,
  VesperRuntimeDownloadContentFormatSingleFile = 2,
  VesperRuntimeDownloadContentFormatUnknown = 3,
} VesperRuntimeDownloadContentFormat;

typedef struct VesperRuntimeDownloadSource {
  char *source_uri;
  VesperRuntimeDownloadContentFormat content_format;
  char *manifest_uri;
} VesperRuntimeDownloadSource;

typedef struct VesperRuntimeDownloadProfile {
  char *variant_id;
  char *preferred_audio_language;
  char *preferred_subtitle_language;
  char **selected_track_ids;
  uintptr_t selected_track_ids_len;
  char *target_directory;
  bool allow_metered_network;
} VesperRuntimeDownloadProfile;

typedef struct VesperRuntimeDownloadResourceRecord {
  char *resource_id;
  char *uri;
  char *relative_path;
  bool has_size_bytes;
  uint64_t size_bytes;
  char *etag;
  char *checksum;
} VesperRuntimeDownloadResourceRecord;

typedef struct VesperRuntimeDownloadSegmentRecord {
  char *segment_id;
  char *uri;
  char *relative_path;
  bool has_sequence;
  uint64_t sequence;
  bool has_size_bytes;
  uint64_t size_bytes;
  char *checksum;
} VesperRuntimeDownloadSegmentRecord;

typedef struct VesperRuntimeDownloadAssetIndex {
  VesperRuntimeDownloadContentFormat content_format;
  char *version;
  char *etag;
  char *checksum;
  bool has_total_size_bytes;
  uint64_t total_size_bytes;
  VesperRuntimeDownloadResourceRecord *resources;
  uintptr_t resources_len;
  VesperRuntimeDownloadSegmentRecord *segments;
  uintptr_t segments_len;
  char *completed_path;
} VesperRuntimeDownloadAssetIndex;

typedef struct VesperRuntimeDownloadProgressSnapshot {
  uint64_t received_bytes;
  bool has_total_bytes;
  uint64_t total_bytes;
  uint32_t received_segments;
  bool has_total_segments;
  uint32_t total_segments;
} VesperRuntimeDownloadProgressSnapshot;

typedef enum VesperRuntimeDownloadTaskStatus {
  VesperRuntimeDownloadTaskStatusQueued = 0,
  VesperRuntimeDownloadTaskStatusPreparing = 1,
  VesperRuntimeDownloadTaskStatusDownloading = 2,
  VesperRuntimeDownloadTaskStatusPaused = 3,
  VesperRuntimeDownloadTaskStatusCompleted = 4,
  VesperRuntimeDownloadTaskStatusFailed = 5,
  VesperRuntimeDownloadTaskStatusRemoved = 6,
} VesperRuntimeDownloadTaskStatus;

typedef struct VesperRuntimeDownloadTask {
  uint64_t task_id;
  char *asset_id;
  VesperRuntimeDownloadSource source;
  VesperRuntimeDownloadProfile profile;
  VesperRuntimeDownloadTaskStatus status;
  VesperRuntimeDownloadProgressSnapshot progress;
  VesperRuntimeDownloadAssetIndex asset_index;
  bool has_error;
  uint32_t error_code;
  uint32_t error_category;
  bool error_retriable;
  char *error_message;
} VesperRuntimeDownloadTask;

typedef struct VesperRuntimeDownloadSnapshot {
  VesperRuntimeDownloadTask *tasks;
  uintptr_t len;
} VesperRuntimeDownloadSnapshot;

typedef enum VesperRuntimeDownloadCommandKind {
  VesperRuntimeDownloadCommandKindStart = 0,
  VesperRuntimeDownloadCommandKindPause = 1,
  VesperRuntimeDownloadCommandKindResume = 2,
  VesperRuntimeDownloadCommandKindRemove = 3,
} VesperRuntimeDownloadCommandKind;

typedef struct VesperRuntimeDownloadCommand {
  VesperRuntimeDownloadCommandKind kind;
  VesperRuntimeDownloadTask task;
  uint64_t task_id;
} VesperRuntimeDownloadCommand;

typedef struct VesperRuntimeDownloadCommandList {
  VesperRuntimeDownloadCommand *commands;
  uintptr_t len;
} VesperRuntimeDownloadCommandList;

typedef enum VesperRuntimeDownloadEventKind {
  VesperRuntimeDownloadEventKindCreated = 0,
  VesperRuntimeDownloadEventKindStateChanged = 1,
  VesperRuntimeDownloadEventKindProgressUpdated = 2,
} VesperRuntimeDownloadEventKind;

typedef struct VesperRuntimeDownloadEvent {
  VesperRuntimeDownloadEventKind kind;
  VesperRuntimeDownloadTask task;
} VesperRuntimeDownloadEvent;

typedef struct VesperRuntimeDownloadEventList {
  VesperRuntimeDownloadEvent *events;
  uintptr_t len;
} VesperRuntimeDownloadEventList;

typedef struct VesperRuntimeDownloadExportCallbacks {
  void *context;
  void (*on_progress)(void *context, float ratio);
  bool (*is_cancelled)(void *context);
} VesperRuntimeDownloadExportCallbacks;

typedef struct VesperRuntimeTrackSelection {
  int mode_ordinal;
  const char *track_id;
} VesperRuntimeTrackSelection;

typedef struct VesperRuntimeAbrPolicy {
  int mode_ordinal;
  const char *track_id;
  bool has_max_bit_rate;
  int64_t max_bit_rate;
  bool has_max_width;
  int32_t max_width;
  bool has_max_height;
  int32_t max_height;
} VesperRuntimeAbrPolicy;

typedef struct VesperRuntimeTrackPreferencePolicy {
  char *preferred_audio_language;
  char *preferred_subtitle_language;
  bool select_subtitles_by_default;
  bool select_undetermined_subtitle_language;
  VesperRuntimeTrackSelection audio_selection;
  VesperRuntimeTrackSelection subtitle_selection;
  VesperRuntimeAbrPolicy abr_policy;
} VesperRuntimeTrackPreferencePolicy;

bool vesper_runtime_resolve_resilience_policy(
    int source_kind_ordinal,
    int source_protocol_ordinal,
    const VesperRuntimeBufferingPolicy *buffering_policy,
    const VesperRuntimeRetryPolicy *retry_policy,
    const VesperRuntimeCachePolicy *cache_policy,
    VesperRuntimeResolvedResiliencePolicy *out_policy);

bool vesper_runtime_resolve_preload_budget(
    const VesperRuntimePreloadBudgetPolicy *preload_budget,
    VesperRuntimeResolvedPreloadBudgetPolicy *out_budget);

bool vesper_runtime_preload_session_create(
    const VesperRuntimeResolvedPreloadBudgetPolicy *preload_budget,
    uint64_t *out_handle);

bool vesper_runtime_preload_session_plan(
    uint64_t handle,
    const VesperRuntimePreloadCandidate *candidates,
    uintptr_t candidates_len);

bool vesper_runtime_preload_session_drain_commands(
    uint64_t handle,
    VesperRuntimePreloadCommandList *out_commands);

bool vesper_runtime_preload_session_complete(
    uint64_t handle,
    uint64_t task_id);

bool vesper_runtime_preload_session_fail(
    uint64_t handle,
    uint64_t task_id,
    uint32_t error_code,
    uint32_t error_category,
    bool retriable,
    const char *message);

void vesper_runtime_preload_command_list_free(
    VesperRuntimePreloadCommandList *commands);

void vesper_runtime_preload_session_dispose(uint64_t handle);

bool vesper_runtime_playlist_session_create(
    const VesperRuntimePlaylistConfig *config,
    const VesperRuntimeResolvedPreloadBudgetPolicy *preload_budget,
    uint64_t *out_handle);

bool vesper_runtime_playlist_session_replace_queue(
    uint64_t handle,
    const VesperRuntimePlaylistQueueItem *queue,
    uintptr_t queue_len);

bool vesper_runtime_playlist_session_update_viewport_hints(
    uint64_t handle,
    const VesperRuntimePlaylistViewportHint *hints,
    uintptr_t hints_len);

bool vesper_runtime_playlist_session_clear_viewport_hints(
    uint64_t handle);

bool vesper_runtime_playlist_session_advance_to_next(
    uint64_t handle);

bool vesper_runtime_playlist_session_advance_to_previous(
    uint64_t handle);

bool vesper_runtime_playlist_session_handle_playback_completed(
    uint64_t handle);

bool vesper_runtime_playlist_session_handle_playback_failed(
    uint64_t handle);

bool vesper_runtime_playlist_session_current_active_item(
    uint64_t handle,
    VesperRuntimePlaylistActiveItem *out_active_item);

bool vesper_runtime_playlist_session_drain_preload_commands(
    uint64_t handle,
    VesperRuntimePreloadCommandList *out_commands);

bool vesper_runtime_playlist_session_complete_preload_task(
    uint64_t handle,
    uint64_t task_id);

bool vesper_runtime_playlist_session_fail_preload_task(
    uint64_t handle,
    uint64_t task_id,
    uint32_t error_code,
    uint32_t error_category,
    bool retriable,
    const char *message);

void vesper_runtime_playlist_active_item_free(
    VesperRuntimePlaylistActiveItem *item);

void vesper_runtime_playlist_session_dispose(uint64_t handle);

bool vesper_runtime_download_session_create(
    const VesperRuntimeDownloadConfig *config,
    uint64_t *out_handle);

bool vesper_runtime_download_session_create_task(
    uint64_t handle,
    const char *asset_id,
    const VesperRuntimeDownloadSource *source,
    const VesperRuntimeDownloadProfile *profile,
    const VesperRuntimeDownloadAssetIndex *asset_index,
    uint64_t *out_task_id);

bool vesper_runtime_download_session_start_task(
    uint64_t handle,
    uint64_t task_id);

bool vesper_runtime_download_session_pause_task(
    uint64_t handle,
    uint64_t task_id);

bool vesper_runtime_download_session_resume_task(
    uint64_t handle,
    uint64_t task_id);

bool vesper_runtime_download_session_update_progress(
    uint64_t handle,
    uint64_t task_id,
    uint64_t received_bytes,
    uint32_t received_segments);

bool vesper_runtime_download_session_complete_task(
    uint64_t handle,
    uint64_t task_id,
    const char *completed_path);

bool vesper_runtime_download_session_export_task(
    uint64_t handle,
    uint64_t task_id,
    const char *output_path,
    VesperRuntimeDownloadExportCallbacks callbacks);

bool vesper_runtime_download_session_fail_task(
    uint64_t handle,
    uint64_t task_id,
    uint32_t error_code,
    uint32_t error_category,
    bool retriable,
    const char *message);

bool vesper_runtime_download_session_remove_task(
    uint64_t handle,
    uint64_t task_id);

bool vesper_runtime_download_session_snapshot(
    uint64_t handle,
    VesperRuntimeDownloadSnapshot *out_snapshot);

bool vesper_runtime_download_session_drain_commands(
    uint64_t handle,
    VesperRuntimeDownloadCommandList *out_commands);

bool vesper_runtime_download_session_drain_events(
    uint64_t handle,
    VesperRuntimeDownloadEventList *out_events);

void vesper_runtime_download_snapshot_free(
    VesperRuntimeDownloadSnapshot *snapshot);

void vesper_runtime_download_command_list_free(
    VesperRuntimeDownloadCommandList *commands);

void vesper_runtime_download_event_list_free(
    VesperRuntimeDownloadEventList *events);

void vesper_runtime_download_session_dispose(uint64_t handle);

bool vesper_runtime_resolve_track_preferences(
    const VesperRuntimeTrackPreferencePolicy *track_preferences,
    VesperRuntimeTrackPreferencePolicy *out_preferences);

void vesper_runtime_track_preferences_free(
    VesperRuntimeTrackPreferencePolicy *track_preferences);

bool vesper_runtime_benchmark_sink_session_create(
    char **plugin_library_paths,
    uintptr_t plugin_library_paths_len,
    uint64_t *out_handle,
    char **out_error_message);

void vesper_runtime_benchmark_sink_session_dispose(uint64_t handle);

bool vesper_runtime_benchmark_sink_session_submit_json(
    uint64_t handle,
    const char *batch_json,
    char **out_report_json,
    char **out_error_message);

bool vesper_runtime_benchmark_sink_session_flush_json(
    uint64_t handle,
    char **out_report_json,
    char **out_error_message);

void vesper_runtime_benchmark_string_free(char *value);

bool vesper_dash_bridge_execute_json(
    const char *request_json,
    char **out_json,
    char **out_error_message);

void vesper_dash_bridge_string_free(char *value);

#endif
