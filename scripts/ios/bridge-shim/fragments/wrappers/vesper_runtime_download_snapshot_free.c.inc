{
  if (snapshot == NULL) {
    return;
  }
  if (snapshot->tasks != NULL) {
    for (uintptr_t index = 0; index < snapshot->len; index += 1) {
      free_runtime_download_task_strings(&snapshot->tasks[index]);
    }
    free(snapshot->tasks);
  }
  memset(snapshot, 0, sizeof(*snapshot));
}
