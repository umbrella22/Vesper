{
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  PlayerFfiPlaylistViewportHint *ffi_hints = NULL;

  if (hints_len > 0) {
    if (hints == NULL) {
      return false;
    }
    ffi_hints = calloc(hints_len, sizeof(PlayerFfiPlaylistViewportHint));
    if (ffi_hints == NULL) {
      return false;
    }
    for (uintptr_t index = 0; index < hints_len; index += 1) {
      ffi_hints[index].item_id = hints[index].item_id;
      ffi_hints[index].kind = (PlayerFfiPlaylistViewportHintKind)hints[index].kind;
      ffi_hints[index].order = hints[index].order;
    }
  }

  PlayerFfiCallStatus status = player_ffi_playlist_session_update_viewport_hints(
      handle,
      ffi_hints,
      hints_len,
      &ffi_error);
  free(ffi_hints);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }
  return true;
}
