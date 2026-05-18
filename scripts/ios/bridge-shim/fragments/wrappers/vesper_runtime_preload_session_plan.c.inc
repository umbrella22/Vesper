{
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
