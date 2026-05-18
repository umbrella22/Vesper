static void free_runtime_playlist_active_item_strings(
    VesperRuntimePlaylistActiveItem *item) {
  if (item == NULL) {
    return;
  }
  free(item->item_id);
}
