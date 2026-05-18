{
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_download_session_update_progress(
          handle,
          task_id,
          received_bytes,
          received_segments,
          &ffi_error),
      &ffi_error);
}
