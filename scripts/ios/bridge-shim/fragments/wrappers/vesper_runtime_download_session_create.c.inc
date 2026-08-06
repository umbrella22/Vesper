{
  if (out_error_message != NULL) {
    *out_error_message = NULL;
  }
  if (config == NULL || out_handle == NULL) {
    return false;
  }
  *out_handle = 0;

  PlayerFfiError ffi_error;
  PlayerFfiDownloadConfig ffi_config = ffi_download_config_from_runtime(config);
  memset(&ffi_error, 0, sizeof(ffi_error));
  PlayerFfiCallStatus status = player_ffi_download_session_create(
      &ffi_config,
      out_handle,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    if (out_error_message != NULL) {
      *out_error_message = ffi_error.message;
      ffi_error.message = NULL;
    }
    player_ffi_error_free(&ffi_error);
    return false;
  }
  return *out_handle != 0;
}
