{
  if (out_json == NULL) {
    return false;
  }
  *out_json = NULL;
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  PlayerFfiCallStatus status = player_ffi_sequence_session_preload_intents_json(
      handle,
      wall_epoch_ms,
      out_json,
      &ffi_error);
  return call_runtime_sequence_json(status, &ffi_error, out_json);
}
