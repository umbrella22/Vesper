{
  PlayerFfiError ffi_error;
  memset(&ffi_error, 0, sizeof(ffi_error));
  ffi_error.message = value;
  player_ffi_error_free(&ffi_error);
}
