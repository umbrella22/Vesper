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
  out_events->dropped_events = ffi_events.dropped_events;
  if (ffi_events.len == 0) {
    player_ffi_download_event_list_free(&ffi_events);
    return true;
  }
  if (ffi_events.events == NULL) {
    uint64_t dropped_events =
        saturating_add_uintptr_to_u64(ffi_events.dropped_events, ffi_events.len);
    player_ffi_download_event_list_free(&ffi_events);
    out_events->len = 0;
    out_events->dropped_events = dropped_events;
    return true;
  }

  if (!can_allocate_items(ffi_events.len, sizeof(VesperRuntimeDownloadEvent))) {
    uint64_t dropped_events =
        saturating_add_uintptr_to_u64(ffi_events.dropped_events, ffi_events.len);
    player_ffi_download_event_list_free(&ffi_events);
    out_events->len = 0;
    out_events->dropped_events = dropped_events;
    return true;
  }

  out_events->events = calloc((size_t)ffi_events.len, sizeof(VesperRuntimeDownloadEvent));
  if (out_events->events == NULL) {
    uint64_t dropped_events =
        saturating_add_uintptr_to_u64(ffi_events.dropped_events, ffi_events.len);
    player_ffi_download_event_list_free(&ffi_events);
    out_events->len = 0;
    out_events->dropped_events = dropped_events;
    return true;
  }

  for (uintptr_t index = 0; index < ffi_events.len; index += 1) {
    if (!runtime_download_event_from_ffi(
            &ffi_events.events[index],
            &out_events->events[index])) {
      uint64_t dropped_events =
          saturating_add_uintptr_to_u64(ffi_events.dropped_events, ffi_events.len);
      vesper_runtime_download_event_list_free(out_events);
      player_ffi_download_event_list_free(&ffi_events);
      out_events->dropped_events = dropped_events;
      return true;
    }
  }
  player_ffi_download_event_list_free(&ffi_events);
  return true;
}
