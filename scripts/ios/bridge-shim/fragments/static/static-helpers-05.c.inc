static bool call_runtime_sequence_json(
    PlayerFfiCallStatus status,
    PlayerFfiError *ffi_error,
    char **out_json) {
  if (status == PlayerFfiCallStatusOk) {
    return true;
  }
  if (out_json != NULL && *out_json != NULL) {
    player_ffi_sequence_string_free(*out_json);
    *out_json = NULL;
  }
  player_ffi_error_free(ffi_error);
  return false;
}
