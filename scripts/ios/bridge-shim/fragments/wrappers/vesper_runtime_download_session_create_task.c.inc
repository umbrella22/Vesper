{
  if (asset_id == NULL || source == NULL || profile == NULL || asset_index == NULL ||
      out_task_id == NULL) {
    return false;
  }

  PlayerFfiError ffi_error;
  PlayerFfiDownloadSource ffi_source = ffi_download_source_from_runtime(source);
  PlayerFfiDownloadProfile ffi_profile = ffi_download_profile_from_runtime(profile);
  PlayerFfiDownloadAssetIndex ffi_asset_index;
  if (!ffi_download_asset_index_from_runtime(asset_index, &ffi_asset_index)) {
    return false;
  }
  memset(&ffi_error, 0, sizeof(ffi_error));
  PlayerFfiCallStatus status = player_ffi_download_session_create_task(
      handle,
      asset_id,
      &ffi_source,
      &ffi_profile,
      &ffi_asset_index,
      out_task_id,
      &ffi_error);
  free_borrowed_ffi_download_asset_index(&ffi_asset_index);
  return call_playlist_status(status, &ffi_error);
}
