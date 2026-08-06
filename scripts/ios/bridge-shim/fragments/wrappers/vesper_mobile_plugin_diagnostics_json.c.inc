{
  if (source_uri == NULL || source_plugin_artifacts_json == NULL ||
      frame_plugin_artifacts_json == NULL || out_json == NULL) {
    return false;
  }
  if (out_error_message != NULL) {
    *out_error_message = NULL;
  }
  *out_json = NULL;

  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_mobile_plugin_diagnostics_json(
      source_uri,
      source_mode,
      source_plugin_artifacts_json,
      runtime_profile,
      frame_mode,
      frame_plugin_artifacts_json,
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
