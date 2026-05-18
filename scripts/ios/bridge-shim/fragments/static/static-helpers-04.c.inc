static bool call_playlist_status(
    PlayerFfiCallStatus status,
    PlayerFfiError *ffi_error) {
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(ffi_error);
    return false;
  }
  return true;
}
