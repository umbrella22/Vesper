{
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_download_session_remove_task(handle, task_id, &ffi_error),
      &ffi_error);
}
