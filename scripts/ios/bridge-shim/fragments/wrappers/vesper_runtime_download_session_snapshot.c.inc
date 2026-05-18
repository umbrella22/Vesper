{
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
