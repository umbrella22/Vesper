{
  if (config == NULL || out_handle == NULL) {
    return false;
  }

  PlayerFfiError ffi_error;
  PlayerFfiDownloadConfig ffi_config = ffi_download_config_from_runtime(config);
  memset(&ffi_error, 0, sizeof(ffi_error));
  return call_playlist_status(
      player_ffi_download_session_create(
          &ffi_config,
          out_handle,
          &ffi_error),
      &ffi_error);
}
