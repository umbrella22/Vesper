{
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_download_session_fail_task(
          handle,
          task_id,
          error_code,
          error_category,
          retriable,
          message,
          &ffi_error),
      &ffi_error);
}
