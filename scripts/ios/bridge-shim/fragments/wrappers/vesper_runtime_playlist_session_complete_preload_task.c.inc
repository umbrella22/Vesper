{
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_playlist_session_complete_preload_task(
          handle,
          task_id,
          &ffi_error),
      &ffi_error);
}
