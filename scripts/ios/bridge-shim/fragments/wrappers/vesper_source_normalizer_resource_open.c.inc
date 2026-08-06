{
  if (source_uri == NULL || source_plugin_artifacts_json == NULL || output_root == NULL ||
      out_handle == NULL || out_json == NULL) {
    return false;
  }
  if (out_error_message != NULL) {
    *out_error_message = NULL;
  }
  *out_handle = 0;
  *out_json = NULL;

  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_source_normalizer_resource_open(
      source_uri,
      source_mode,
      source_plugin_artifacts_json,
      runtime_profile,
      output_root,
      force_normalized,
      out_handle,
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
  return *out_handle != 0 && *out_json != NULL;
}
