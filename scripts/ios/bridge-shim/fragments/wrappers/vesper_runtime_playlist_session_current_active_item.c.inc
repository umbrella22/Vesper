{
  if (out_active_item == NULL) {
    return false;
  }

  PlayerFfiPlaylistActiveItem ffi_active_item;
  PlayerFfiError ffi_error;
  memset(&ffi_active_item, 0, sizeof(ffi_active_item));
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_playlist_session_current_active_item(
      handle,
      &ffi_active_item,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }

  out_active_item->item_id = duplicate_string(ffi_active_item.item_id);
  out_active_item->index = ffi_active_item.index;
  player_ffi_playlist_active_item_free(&ffi_active_item);
  return true;
}
