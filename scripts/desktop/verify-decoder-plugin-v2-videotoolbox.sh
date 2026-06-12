#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/desktop.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROFILE="debug"
MODE="loader"
LIBRARY_PATH_OVERRIDE="${VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH:-}"
FRAME_PROCESSOR_LIBRARY_PATH_OVERRIDE="${VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH:-}"
SOURCE_PATH_OVERRIDE="${VESPER_DECODER_VIDEOTOOLBOX_SOURCE:-}"

usage() {
  cat <<EOF >&2
Usage: $(basename "$0") [debug|release] [loader|playback|basic-player|all]

Examples:
  $(basename "$0")
  $(basename "$0") debug loader
  $(basename "$0") debug playback
  $(basename "$0") debug basic-player
  $(basename "$0") release all
EOF
}

for token in "$@"; do
  case "$token" in
    debug|release)
      PROFILE="$token"
      ;;
    loader|playback|basic-player|all)
      MODE="$token"
      ;;
    *)
      usage
      exit 1
      ;;
  esac
done

needs_frame_processor_verification() {
  [[ "$MODE" == "playback" || "$MODE" == "basic-player" || "$MODE" == "all" ]]
}

build_decoder_plugin() {
  if [[ -n "$LIBRARY_PATH_OVERRIDE" ]]; then
    return 0
  fi

  if [[ "$PROFILE" == "release" ]]; then
    cargo build -p player-decoder-videotoolbox --release
  else
    cargo build -p player-decoder-videotoolbox
  fi
}

build_frame_processor_plugin() {
  if [[ -n "$FRAME_PROCESSOR_LIBRARY_PATH_OVERRIDE" ]]; then
    return 0
  fi

  if [[ "$PROFILE" == "release" ]]; then
    cargo build -p player-frame-processor-diagnostic --release
  else
    cargo build -p player-frame-processor-diagnostic
  fi
}

resolve_smoke_source() {
  local target_dir="$1"
  local generated="$target_dir/videotoolbox-smoke-h264.mp4"

  if [[ -n "$SOURCE_PATH_OVERRIDE" ]]; then
    if [[ ! -f "$SOURCE_PATH_OVERRIDE" ]]; then
      echo "VESPER_DECODER_VIDEOTOOLBOX_SOURCE points to a missing file: $SOURCE_PATH_OVERRIDE" >&2
      exit 1
    fi
    printf '%s\n' "$SOURCE_PATH_OVERRIDE"
    return 0
  fi

  if [[ -f "$generated" ]]; then
    printf '%s\n' "$generated"
    return 0
  fi

  if ! command -v ffmpeg >/dev/null 2>&1; then
    echo "ffmpeg is required to generate the VideoToolbox smoke source; install ffmpeg or set VESPER_DECODER_VIDEOTOOLBOX_SOURCE." >&2
    exit 1
  fi

  mkdir -p "$target_dir"
  ffmpeg \
    -hide_banner \
    -loglevel error \
    -y \
    -f lavfi \
    -i testsrc2=size=320x180:rate=24:duration=2 \
    -c:v libx264 \
    -profile:v baseline \
    -level:v 3.1 \
    -pix_fmt yuv420p \
    -movflags +faststart \
    "$generated"
  printf '%s\n' "$generated"
}

run_loader_test() {
  cargo test \
    -p player-plugin-loader \
    tests::decoder_tests::dynamic_loader_opens_real_videotoolbox_decoder_shared_library \
    -- \
    --ignored \
    --exact
}

run_macos_runtime_test() {
  cargo test \
    -p player-platform-macos \
    tests::macos_runtime_diagnostics_loads_real_videotoolbox_decoder_library \
    -- \
    --ignored \
    --exact
}

run_headless_decode_test() {
  cargo test \
    -p player-platform-macos \
    tests::macos_videotoolbox_decoder_decodes_ffmpeg_packets_headless \
    -- \
    --ignored \
    --exact
}

run_headless_lifecycle_test() {
  cargo test \
    -p player-platform-macos \
    tests::macos_videotoolbox_decoder_flush_seek_and_eof_headless \
    -- \
    --ignored \
    --exact
}

run_source_switch_cleanup_test() {
  cargo test \
    -p player-platform-macos \
    tests::macos_native_frame_source_switch_releases_old_source_and_decodes_new_source \
    -- \
    --exact
}

run_source_normalizer_lease_cleanup_test() {
  cargo test \
    -p player-platform-macos \
    tests::source_normalizer_packet_source_drop_after_backpressure_has_no_outstanding_lease \
    -- \
    --exact
}

run_playback_test() {
  cargo test \
    -p player-platform-macos \
    tests::macos_native_frame_decoder_plugin_runtime_probes_with_surface \
    -- \
    --ignored \
    --exact
}

run_playback_fallback_test() {
  cargo test \
    -p player-platform-macos \
    tests::macos_native_frame_runtime_reopens_as_software_after_presenter_failure \
    -- \
    --ignored \
    --exact
}

run_frame_processor_test() {
  cargo test \
    -p player-platform-macos \
    tests::macos_native_frame_runtime_loads_frame_processor_diagnostic_plugin \
    -- \
    --ignored \
    --exact
}

run_strict_frame_processor_runtime_test() {
  VESPER_FRAME_PROCESSOR_DIAGNOSTIC_MODE=unsupported-handle \
    cargo test \
    -p player-platform-macos \
    tests::macos_native_frame_strict_frame_processor_failure_does_not_fallback_to_software \
    -- \
    --ignored \
    --exact
}

run_strict_frame_processor_host_test() {
  VESPER_FRAME_PROCESSOR_DIAGNOSTIC_MODE=unsupported-handle \
    cargo test \
    -p player-platform-macos \
    tests::macos_host_strict_frame_processor_failure_forwards_software_error_message \
    -- \
    --ignored \
    --exact
}

build_basic_player() {
  if [[ "$PROFILE" == "release" ]]; then
    cargo build -p basic-player --release
  else
    cargo build -p basic-player
  fi
}

require_basic_player_log_entry() {
  local log_file="$1"
  local pattern="$2"
  local description="$3"

  if ! grep -Fq "$pattern" "$log_file"; then
    echo "basic-player smoke did not report $description." >&2
    echo "Log: $log_file" >&2
    tail -n 80 "$log_file" >&2 || true
    exit 1
  fi
}

run_basic_player_smoke_test() {
  local target_dir="$1"
  local timeout_seconds="${VESPER_BASIC_PLAYER_SMOKE_TIMEOUT_SECONDS:-60}"
  local basic_player_path="$target_dir/$PROFILE/basic-player"
  local log_file
  local pid
  local deadline
  local process_status=0

  case "$timeout_seconds" in
    ''|*[!0-9]*)
      echo "VESPER_BASIC_PLAYER_SMOKE_TIMEOUT_SECONDS must be a positive integer." >&2
      exit 1
      ;;
  esac

  build_basic_player
  if [[ ! -x "$basic_player_path" ]]; then
    echo "Could not find built basic-player binary: $basic_player_path" >&2
    exit 1
  fi

  log_file="$(mktemp "${TMPDIR:-/tmp}/vesper-basic-player-videotoolbox.XXXXXX").log"
  echo "Running basic-player VideoToolbox smoke; log: $log_file"

  env \
    VESPER_DECODER_PLUGIN_VIDEO_MODE=native-frame \
    VESPER_DECODER_PLUGIN_PATHS="$VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH" \
    VESPER_FRAME_PROCESSOR_MODE=prefer-processed \
    VESPER_FRAME_PROCESSOR_PLUGIN_PATHS="$VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH" \
    VESPER_FRAME_PROCESSOR_DIAGNOSTIC_MODE=noop \
    VESPER_FRAME_PROCESSOR_DEBUG=1 \
    VESPER_FRAME_PROCESSOR_DEBUG_WINDOW="${VESPER_FRAME_PROCESSOR_DEBUG_WINDOW:-24}" \
    VESPER_PLAYBACK_DEBUG=1 \
    VESPER_PLAYBACK_DEBUG_WINDOW="${VESPER_PLAYBACK_DEBUG_WINDOW:-24}" \
    VESPER_BASIC_PLAYER_SMOKE_SCRIPT=1 \
    "$basic_player_path" "$VESPER_DECODER_VIDEOTOOLBOX_SOURCE" \
    >"$log_file" 2>&1 &
  pid=$!
  deadline=$((SECONDS + timeout_seconds))

  while kill -0 "$pid" 2>/dev/null && [[ "$SECONDS" -lt "$deadline" ]]; do
    if grep -Fq "player playback ended" "$log_file"; then
      break
    fi
    if grep -Fq "desktop launch failed" "$log_file" || grep -Fq "panicked at" "$log_file"; then
      break
    fi
    sleep 1
  done

  if kill -0 "$pid" 2>/dev/null; then
    kill -INT "$pid" 2>/dev/null || true
    sleep 2
    if kill -0 "$pid" 2>/dev/null; then
      kill -TERM "$pid" 2>/dev/null || true
    fi
  fi

  set +e
  wait "$pid" 2>/dev/null
  process_status=$?
  set -e

  require_basic_player_log_entry "$log_file" "initialized desktop player" "runtime initialization"
  require_basic_player_log_entry "$log_file" "selected sdkManagedNativeFrame route" "sdkManagedNativeFrame route selection"
  require_basic_player_log_entry "$log_file" "supports_external_video_surface=true" "external macOS video surface support"
  require_basic_player_log_entry "$log_file" "frame processor plugins: 1/1 supported" "diagnostic FrameProcessor support"
  require_basic_player_log_entry "$log_file" "macOS frame processor debug summary" "FrameProcessor debug summary"
  require_basic_player_log_entry "$log_file" "basic-player smoke script observed playback" "scripted playback observation"
  require_basic_player_log_entry "$log_file" "basic-player smoke script showed overlay" "scripted overlay refresh"
  require_basic_player_log_entry "$log_file" "basic-player smoke script paused playback" "scripted pause"
  require_basic_player_log_entry "$log_file" "basic-player smoke script resumed playback" "scripted resume"
  require_basic_player_log_entry "$log_file" "basic-player smoke script seeked to midpoint" "scripted seek"
  require_basic_player_log_entry "$log_file" "basic-player smoke script changed rate" "scripted playback-rate update"
  require_basic_player_log_entry "$log_file" "player playback ended" "playback completion"

  if grep -Eq 'deadline_misses=[1-9][0-9]*' "$log_file"; then
    echo "basic-player smoke reported FrameProcessor deadline misses." >&2
    echo "Log: $log_file" >&2
    grep -E 'deadline_misses=[1-9][0-9]*|macOS frame processor debug summary' "$log_file" >&2 || true
    exit 1
  fi
  if grep -Eq 'dropped_outputs=[1-9][0-9]*' "$log_file"; then
    echo "basic-player smoke reported dropped FrameProcessor outputs." >&2
    echo "Log: $log_file" >&2
    grep -E 'dropped_outputs=[1-9][0-9]*|macOS frame processor debug summary' "$log_file" >&2 || true
    exit 1
  fi

  case "$process_status" in
    0|130|143)
      ;;
    *)
      echo "basic-player smoke exited unexpectedly with status $process_status." >&2
      echo "Log: $log_file" >&2
      tail -n 80 "$log_file" >&2 || true
      exit 1
      ;;
  esac

  echo "basic-player VideoToolbox smoke passed; log: $log_file"
}

main() {
  local library_name
  local frame_processor_library_name
  local frame_processor_plugin_path
  local target_dir
  local plugin_path
  local smoke_source

  if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "VideoToolbox decoder verification only runs on macOS." >&2
    exit 1
  fi

  library_name="$(vesper_desktop_shared_library_name vesper_decoder_videotoolbox)"
  target_dir="$(vesper_desktop_target_dir)"

  build_decoder_plugin
  plugin_path="$(vesper_desktop_resolve_plugin_path "$library_name" "$target_dir" "$PROFILE" "$LIBRARY_PATH_OVERRIDE" VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH player-decoder-videotoolbox)"
  export VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH="$plugin_path"

  echo "Using VideoToolbox decoder plugin: $VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH"

  smoke_source="$(resolve_smoke_source "$target_dir")"
  export VESPER_DECODER_VIDEOTOOLBOX_SOURCE="$smoke_source"
  echo "Using VideoToolbox smoke source: $VESPER_DECODER_VIDEOTOOLBOX_SOURCE"

  if needs_frame_processor_verification; then
    frame_processor_library_name="$(vesper_desktop_shared_library_name vesper_frame_processor_diagnostic)"
    build_frame_processor_plugin
    frame_processor_plugin_path="$(vesper_desktop_resolve_plugin_path "$frame_processor_library_name" "$target_dir" "$PROFILE" "$FRAME_PROCESSOR_LIBRARY_PATH_OVERRIDE" VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH player-frame-processor-diagnostic)"
    export VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH="$frame_processor_plugin_path"
    echo "Using diagnostic frame processor plugin: $VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH"
  fi

  case "$MODE" in
    loader)
      run_loader_test
      run_macos_runtime_test
      run_headless_decode_test
      run_headless_lifecycle_test
      run_source_switch_cleanup_test
      run_source_normalizer_lease_cleanup_test
      ;;
    playback)
      run_playback_test
      run_playback_fallback_test
      run_frame_processor_test
      run_strict_frame_processor_runtime_test
      run_strict_frame_processor_host_test
      ;;
    basic-player)
      run_basic_player_smoke_test "$target_dir"
      ;;
    all)
      run_loader_test
      run_macos_runtime_test
      run_headless_decode_test
      run_headless_lifecycle_test
      run_source_switch_cleanup_test
      run_source_normalizer_lease_cleanup_test
      run_playback_test
      run_playback_fallback_test
      run_frame_processor_test
      run_strict_frame_processor_runtime_test
      run_strict_frame_processor_host_test
      run_basic_player_smoke_test "$target_dir"
      ;;
  esac
}

main "$@"
