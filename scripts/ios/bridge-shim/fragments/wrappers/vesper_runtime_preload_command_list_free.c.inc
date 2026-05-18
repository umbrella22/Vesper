{
  if (commands == NULL) {
    return;
  }
  if (commands->commands != NULL) {
    for (uintptr_t index = 0; index < commands->len; index += 1) {
      free_runtime_preload_task_strings(&commands->commands[index].task);
    }
    free(commands->commands);
  }
  memset(commands, 0, sizeof(*commands));
}
