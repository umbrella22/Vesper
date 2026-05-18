{
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
