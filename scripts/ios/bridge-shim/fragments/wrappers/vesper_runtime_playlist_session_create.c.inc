{
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
