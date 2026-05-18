{
  if (out_commands == NULL) {
    return false;
  }

  PlayerFfiPreloadCommandList ffi_commands;
  PlayerFfiError ffi_error;
  memset(&ffi_commands, 0, sizeof(ffi_commands));
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status =
      player_ffi_playlist_session_drain_preload_commands(
          handle,
          &ffi_commands,
          &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }

  out_commands->len = ffi_commands.len;
  out_commands->commands = NULL;
  if (ffi_commands.len == 0 || ffi_commands.commands == NULL) {
    player_ffi_preload_command_list_free(&ffi_commands);
    return true;
  }

  out_commands->commands = calloc(ffi_commands.len, sizeof(VesperRuntimePreloadCommand));
  if (out_commands->commands == NULL) {
    player_ffi_preload_command_list_free(&ffi_commands);
    out_commands->len = 0;
    return false;
  }

  for (uintptr_t index = 0; index < ffi_commands.len; index += 1) {
    PlayerFfiPreloadCommand *ffi_command = &ffi_commands.commands[index];
    VesperRuntimePreloadCommand *runtime_command = &out_commands->commands[index];
    runtime_command->kind = (VesperRuntimePreloadCommandKind)ffi_command->kind;
    runtime_command->task_id = ffi_command->task_id;
    runtime_command->task.task_id = ffi_command->task.task_id;
    runtime_command->task.source_uri = duplicate_string(ffi_command->task.source_uri);
    runtime_command->task.source_identity = duplicate_string(ffi_command->task.source_identity);
    runtime_command->task.cache_key = duplicate_string(ffi_command->task.cache_key);
    runtime_command->task.scope_kind =
        (VesperRuntimePreloadScopeKind)ffi_command->task.scope_kind;
    runtime_command->task.scope_id = duplicate_string(ffi_command->task.scope_id);
    runtime_command->task.candidate_kind =
        (VesperRuntimePreloadCandidateKind)ffi_command->task.candidate_kind;
    runtime_command->task.selection_hint =
        (VesperRuntimePreloadSelectionHint)ffi_command->task.selection_hint;
    runtime_command->task.priority =
        (VesperRuntimePreloadPriority)ffi_command->task.priority;
    runtime_command->task.status =
        (VesperRuntimePreloadTaskStatus)ffi_command->task.status;
    runtime_command->task.expected_memory_bytes =
        ffi_command->task.expected_memory_bytes;
    runtime_command->task.expected_disk_bytes =
        ffi_command->task.expected_disk_bytes;
    runtime_command->task.warmup_window_ms = ffi_command->task.warmup_window_ms;
    runtime_command->task.has_error = ffi_command->task.has_error;
    runtime_command->task.error_code = ffi_command->task.error_code;
    runtime_command->task.error_category = ffi_command->task.error_category;
    runtime_command->task.error_retriable = ffi_command->task.error_retriable;
    runtime_command->task.error_message =
        duplicate_string(ffi_command->task.error_message);
  }

  player_ffi_preload_command_list_free(&ffi_commands);
  return true;
}
