{
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  PlayerFfiPlaylistQueueItem *ffi_queue = NULL;

  if (queue_len > 0) {
    if (queue == NULL) {
      return false;
    }
    ffi_queue = calloc(queue_len, sizeof(PlayerFfiPlaylistQueueItem));
    if (ffi_queue == NULL) {
      return false;
    }
    for (uintptr_t index = 0; index < queue_len; index += 1) {
      ffi_queue[index].item_id = queue[index].item_id;
      ffi_queue[index].source_uri = queue[index].source_uri;
      ffi_queue[index].expected_memory_bytes = queue[index].expected_memory_bytes;
      ffi_queue[index].expected_disk_bytes = queue[index].expected_disk_bytes;
      ffi_queue[index].has_ttl_ms = queue[index].has_ttl_ms;
      ffi_queue[index].ttl_ms = queue[index].ttl_ms;
      ffi_queue[index].has_warmup_window_ms = queue[index].has_warmup_window_ms;
      ffi_queue[index].warmup_window_ms = queue[index].warmup_window_ms;
    }
  }

  PlayerFfiCallStatus status = player_ffi_playlist_session_replace_queue(
      handle,
      ffi_queue,
      queue_len,
      &ffi_error);
  free(ffi_queue);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }
  return true;
}
