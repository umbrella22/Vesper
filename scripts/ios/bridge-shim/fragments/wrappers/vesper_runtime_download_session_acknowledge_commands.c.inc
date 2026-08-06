{
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  PlayerFfiCallStatus status = player_ffi_download_session_acknowledge_commands(
      handle,
      command_count,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }
  return true;
}
