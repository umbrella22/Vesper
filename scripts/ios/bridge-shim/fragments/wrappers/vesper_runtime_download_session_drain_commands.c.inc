{
  if (out_commands == NULL) {
    return false;
  }

  PlayerFfiDownloadCommandList ffi_commands;
  PlayerFfiError ffi_error;
  memset(&ffi_commands, 0, sizeof(ffi_commands));
  memset(&ffi_error, 0, sizeof(ffi_error));

  PlayerFfiCallStatus status = player_ffi_download_session_drain_commands(
      handle,
      &ffi_commands,
      &ffi_error);
  if (status != PlayerFfiCallStatusOk) {
    player_ffi_error_free(&ffi_error);
    return false;
  }

  out_commands->len = ffi_commands.len;
  out_commands->commands = NULL;
  if (ffi_commands.len == 0) {
    player_ffi_download_command_list_free(&ffi_commands);
    return true;
  }
  if (ffi_commands.commands == NULL) {
    player_ffi_download_command_list_free(&ffi_commands);
    out_commands->len = 0;
    return false;
  }

  if (!can_allocate_items(ffi_commands.len, sizeof(VesperRuntimeDownloadCommand))) {
    player_ffi_download_command_list_free(&ffi_commands);
    out_commands->len = 0;
    return false;
  }

  out_commands->commands = calloc((size_t)ffi_commands.len, sizeof(VesperRuntimeDownloadCommand));
  if (out_commands->commands == NULL) {
    player_ffi_download_command_list_free(&ffi_commands);
    out_commands->len = 0;
    return false;
  }

  for (uintptr_t index = 0; index < ffi_commands.len; index += 1) {
    if (!runtime_download_command_from_ffi(
            &ffi_commands.commands[index],
            &out_commands->commands[index])) {
      vesper_runtime_download_command_list_free(out_commands);
      player_ffi_download_command_list_free(&ffi_commands);
      return false;
    }
  }
  player_ffi_download_command_list_free(&ffi_commands);
  return true;
}
