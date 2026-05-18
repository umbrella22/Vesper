{
  if (item == NULL) {
    return;
  }
  free_runtime_playlist_active_item_strings(item);
  memset(item, 0, sizeof(*item));
}
