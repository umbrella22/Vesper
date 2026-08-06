{
  if (source_uri == NULL || source_plugin_artifacts_json == NULL ||
      decoder_plugin_artifacts_json == NULL || frame_plugin_artifacts_json == NULL ||
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

  PlayerFfiCallStatus status = player_ffi_ios_native_frame_pipeline_open(
      source_uri,
      source_mode,
      source_plugin_artifacts_json,
      runtime_profile,
      native_frame_mode,
      decoder_plugin_artifacts_json,
      frame_plugin_artifacts_json,
      max_in_flight_frames,
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
