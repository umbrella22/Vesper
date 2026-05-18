{
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  PlayerFfiDownloadExportCallbacks ffi_callbacks = {
      .context = callbacks.context,
      .on_progress = callbacks.on_progress,
      .is_cancelled = callbacks.is_cancelled,
  };
  return call_playlist_status(
      player_ffi_download_session_export_task(
          handle,
          task_id,
          output_path,
          ffi_callbacks,
          &ffi_error),
      &ffi_error);
}
