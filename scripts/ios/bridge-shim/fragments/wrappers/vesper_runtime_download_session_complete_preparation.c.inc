{
  if (asset_index == NULL) {
    return false;
  }

  PlayerFfiError ffi_error;
  PlayerFfiDownloadAssetIndex ffi_asset_index;
  if (!ffi_download_asset_index_from_runtime(asset_index, &ffi_asset_index)) {
    return false;
  }
  memset(&ffi_error, 0, sizeof(ffi_error));
  PlayerFfiCallStatus status = player_ffi_download_session_complete_preparation(
      handle,
      task_id,
      &ffi_asset_index,
      &ffi_error);
  free_borrowed_ffi_download_asset_index(&ffi_asset_index);
  return call_playlist_status(status, &ffi_error);
}
