{
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
