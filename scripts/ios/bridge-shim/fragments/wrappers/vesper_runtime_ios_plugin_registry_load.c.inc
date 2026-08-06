{
  if (out_handle == NULL) {
    return false;
  }
  if (out_error_message != NULL) {
    *out_error_message = NULL;
  }
  *out_handle = 0;

  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  PlayerFfiCallStatus status = player_ffi_ios_plugin_registry_load(
      plan_handle,
      resolved_frameworks_json,
      resolved_frameworks_json_len,
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
