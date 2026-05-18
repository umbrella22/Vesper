{
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
