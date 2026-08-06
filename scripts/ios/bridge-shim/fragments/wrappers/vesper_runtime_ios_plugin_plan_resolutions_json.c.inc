{
  if (out_json == NULL) {
    return false;
  }
  *out_json = NULL;
  if (out_error_message != NULL) {
    *out_error_message = NULL;
  }

  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  PlayerFfiCallStatus status = player_ffi_ios_plugin_plan_resolutions_json(
      handle,
      out_json,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    if (out_error_message != NULL) {
      *out_error_message = ffi_error.message;
      ffi_error.message = NULL;
    }
    player_ffi_error_free(&ffi_error);
    return false;
  }
  return *out_json != NULL;
}
