#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE="debug"
MODE="loader"
LIBRARY_PATH_OVERRIDE="${VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH:-}"
SOURCE_PATH_OVERRIDE="${VESPER_DECODER_VIDEOTOOLBOX_SOURCE:-}"

usage() {
  cat <<EOF >&2
Usage: $(basename "$0") [debug|release] [loader|playback|all]

Examples:
  $(basename "$0")
  $(basename "$0") debug loader
  $(basename "$0") debug playback
  $(basename "$0") release all
EOF
}

for token in "$@"; do
  case "$token" in
    debug|release)
      PROFILE="$token"
      ;;
    loader|playback|all)
      MODE="$token"
      ;;
    *)
      usage
      exit 1
      ;;
  esac
done

shared_library_name() {
  case "$(uname -s)" in
    Darwin)
      echo "libplayer_decoder_videotoolbox.dylib"
      ;;
    Linux)
      echo "libplayer_decoder_videotoolbox.so"
      ;;
    MINGW*|MSYS*|CYGWIN*)
      echo "player_decoder_videotoolbox.dll"
      ;;
    *)
      echo "Unsupported platform: $(uname -s)" >&2
      exit 1
      ;;
  esac
}

resolve_target_dir() {
  if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
    if [[ "$CARGO_TARGET_DIR" = /* ]]; then
      printf '%s\n' "$CARGO_TARGET_DIR"
    else
      printf '%s\n' "$ROOT_DIR/$CARGO_TARGET_DIR"
    fi
    return 0
  fi

  printf '%s\n' "$ROOT_DIR/target"
}

resolve_plugin_path() {
  local library_name="$1"
  local target_dir="$2"
  local candidate

  if [[ -n "$LIBRARY_PATH_OVERRIDE" ]]; then
    if [[ ! -f "$LIBRARY_PATH_OVERRIDE" ]]; then
      echo "VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH points to a missing file: $LIBRARY_PATH_OVERRIDE" >&2
      exit 1
    fi
    printf '%s\n' "$LIBRARY_PATH_OVERRIDE"
    return 0
  fi

  for candidate in \
    "$target_dir/$PROFILE/$library_name" \
    "$target_dir/$PROFILE/deps/$library_name" \
    "$target_dir/debug/$library_name" \
    "$target_dir/debug/deps/$library_name" \
    "$target_dir/release/$library_name" \
    "$target_dir/release/deps/$library_name"; do
    if [[ -f "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done

  echo "Could not find $library_name under $target_dir; build player-decoder-videotoolbox first or set VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH." >&2
  exit 1
}

build_plugin() {
  if [[ -n "$LIBRARY_PATH_OVERRIDE" ]]; then
    return 0
  fi

  if [[ "$PROFILE" == "release" ]]; then
    cargo build -p player-decoder-videotoolbox --release
  else
    cargo build -p player-decoder-videotoolbox
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
    tests::dynamic_loader_opens_real_videotoolbox_decoder_shared_library \
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

main() {
  local library_name
  local target_dir
  local plugin_path
  local smoke_source

  if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "VideoToolbox decoder verification only runs on macOS." >&2
    exit 1
  fi

  library_name="$(shared_library_name)"
  target_dir="$(resolve_target_dir)"

  build_plugin
  plugin_path="$(resolve_plugin_path "$library_name" "$target_dir")"
  export VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH="$plugin_path"

  echo "Using VideoToolbox decoder plugin: $VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH"

  smoke_source="$(resolve_smoke_source "$target_dir")"
  export VESPER_DECODER_VIDEOTOOLBOX_SOURCE="$smoke_source"
  echo "Using VideoToolbox smoke source: $VESPER_DECODER_VIDEOTOOLBOX_SOURCE"

  case "$MODE" in
    loader)
      run_loader_test
      run_macos_runtime_test
      run_headless_decode_test
      run_headless_lifecycle_test
      ;;
    playback)
      run_playback_test
      run_playback_fallback_test
      ;;
    all)
      run_loader_test
      run_macos_runtime_test
      run_headless_decode_test
      run_headless_lifecycle_test
      run_playback_test
      run_playback_fallback_test
      ;;
  esac
}

main "$@"
