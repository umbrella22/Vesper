{
  if (out_events == NULL) {
    return false;
  }

  PlayerFfiDownloadEventList ffi_events;
  PlayerFfiError ffi_error;
  memset(&ffi_events, 0, sizeof(ffi_events));
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_download_session_drain_events(
      handle,
      &ffi_events,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }

  out_events->len = ffi_events.len;
  out_events->events = NULL;
  if (ffi_events.len == 0) {
    player_ffi_download_event_list_free(&ffi_events);
    return true;
  }
  if (ffi_events.events == NULL) {
    player_ffi_download_event_list_free(&ffi_events);
    out_events->len = 0;
    return false;
  }

  if (!can_allocate_items(ffi_events.len, sizeof(VesperRuntimeDownloadEvent))) {
    player_ffi_download_event_list_free(&ffi_events);
    out_events->len = 0;
    return false;
  }

  out_events->events = calloc((size_t)ffi_events.len, sizeof(VesperRuntimeDownloadEvent));
  if (out_events->events == NULL) {
    player_ffi_download_event_list_free(&ffi_events);
    out_events->len = 0;
    return false;
  }

  for (uintptr_t index = 0; index < ffi_events.len; index += 1) {
    if (!runtime_download_event_from_ffi(
            &ffi_events.events[index],
            &out_events->events[index])) {
      vesper_runtime_download_event_list_free(out_events);
      player_ffi_download_event_list_free(&ffi_events);
      return false;
    }
  }
  player_ffi_download_event_list_free(&ffi_events);
  return true;
}
