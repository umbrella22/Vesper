{
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
