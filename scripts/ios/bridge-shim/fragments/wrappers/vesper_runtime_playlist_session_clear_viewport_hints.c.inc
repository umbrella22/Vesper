{
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_playlist_session_clear_viewport_hints(handle, &ffi_error),
      &ffi_error);
}
