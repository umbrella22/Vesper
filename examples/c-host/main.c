#include <stdio.h>
#include <string.h>

#include "../../include/player_ffi.h"

static void print_error(const char* context, PlayerFfiError* error) {
  if (error == NULL) {
    return;
  }

  fprintf(stderr, "%s failed: code=%d", context, (int)error->code);
  if (error->message != NULL) {
    fprintf(stderr, " message=%s", error->message);
  }
  fprintf(stderr, "\n");
  player_ffi_error_free(error);
}

int main(int argc, char** argv) {
  const char* source = argc > 1 ? argv[1] : "fixtures/media/tiny-h264-aac.m4v";
  PlayerFfiInitializerHandle initializer = {0};
  PlayerFfiHandle player = {0};
  PlayerFfiError error = {0};
  PlayerFfiMediaInfo media_info = {0};
  PlayerFfiStartup startup = {0};
  bool has_initial_frame = false;
  PlayerFfiVideoFrame initial_frame = {0};
  PlayerFfiSnapshot snapshot = {0};
  PlayerFfiEventList events = {0};
  bool applied = false;

  if (player_ffi_initializer_probe_uri(source, &initializer, &error) !=
      PLAYER_FFI_CALL_STATUS_OK) {
    print_error("probe_uri", &error);
    return 1;
  }

  if (player_ffi_initializer_media_info(initializer, &media_info, &error) !=
      PLAYER_FFI_CALL_STATUS_OK) {
    print_error("media_info", &error);
    player_ffi_initializer_destroy(initializer, &error);
    return 1;
  }

  printf("media: source=%s duration_ms=%llu video_streams=%zu audio_streams=%zu\n",
         media_info.source_uri != NULL ? media_info.source_uri : "(null)",
         (unsigned long long)media_info.duration_ms,
         media_info.video_streams,
         media_info.audio_streams);
  player_ffi_media_info_free(&media_info);

  if (player_ffi_initializer_initialize(
          initializer,
          &player,
          &has_initial_frame,
          &initial_frame,
          &startup,
          &error) !=
      PLAYER_FFI_CALL_STATUS_OK) {
    print_error("initialize", &error);
    return 1;
  }

  if (has_initial_frame) {
    printf("startup: ffmpeg_initialized=%d initial_frame=%ux%u pts=%llums\n",
           startup.ffmpeg_initialized ? 1 : 0,
           initial_frame.width,
           initial_frame.height,
           (unsigned long long)initial_frame.presentation_time_ms);
  } else {
    printf("startup: ffmpeg_initialized=%d initial_frame=none\n",
           startup.ffmpeg_initialized ? 1 : 0);
  }
  player_ffi_startup_free(&startup);
  if (has_initial_frame) {
    player_ffi_video_frame_free(&initial_frame);
  }

  if (player_ffi_player_set_playback_rate(
          player, 2.0f, &applied, &snapshot, &error) !=
      PLAYER_FFI_CALL_STATUS_OK) {
    print_error("set_playback_rate(2.0)", &error);
    player_ffi_player_destroy(player, &error);
    return 1;
  }

  printf("rate applied=%d playback_rate=%.2f position_ms=%llu\n",
         applied ? 1 : 0,
         snapshot.playback_rate,
         (unsigned long long)snapshot.progress.position_ms);
  player_ffi_snapshot_free(&snapshot);

  if (player_ffi_player_dispatch(player,
                                 PLAYER_FFI_COMMAND_KIND_PLAY,
                                 0,
                                 &applied,
                                 NULL,
                                 &snapshot,
                                 &error) != PLAYER_FFI_CALL_STATUS_OK) {
    print_error("dispatch(play)", &error);
    player_ffi_player_destroy(player, &error);
    return 1;
  }

  printf("play applied=%d state=%d position_ms=%llu\n",
         applied ? 1 : 0,
         (int)snapshot.state,
         (unsigned long long)snapshot.progress.position_ms);
  player_ffi_snapshot_free(&snapshot);

  if (player_ffi_player_drain_events(player, &events, &error) !=
      PLAYER_FFI_CALL_STATUS_OK) {
    print_error("drain_events", &error);
    player_ffi_player_destroy(player, &error);
    return 1;
  }

  printf("events: %zu\n", events.len);
  player_ffi_event_list_free(&events);
  player_ffi_player_destroy(player, &error);
  return 0;
}
