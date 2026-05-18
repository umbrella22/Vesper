{
  if (events == NULL) {
    return;
  }
  if (events->events != NULL) {
    for (uintptr_t index = 0; index < events->len; index += 1) {
      free_runtime_download_event_strings(&events->events[index]);
    }
    free(events->events);
  }
  memset(events, 0, sizeof(*events));
}
