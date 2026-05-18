static void free_runtime_preload_task_strings(VesperRuntimePreloadTask *task) {
  if (task == NULL) {
    return;
  }
  free(task->source_uri);
  free(task->source_identity);
  free(task->cache_key);
  free(task->scope_id);
  free(task->error_message);
}
