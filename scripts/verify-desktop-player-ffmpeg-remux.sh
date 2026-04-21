#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE="debug"
MODE="all"
LIBRARY_PATH_OVERRIDE="${VESPER_PLAYER_FFMPEG_PLUGIN_PATH:-}"

usage() {
  cat <<EOF >&2
Usage: $(basename "$0") [debug|release] [loader|example|all]

Examples:
  $(basename "$0")
  $(basename "$0") loader
  $(basename "$0") debug all
  $(basename "$0") release example
EOF
}

for token in "$@"; do
  case "$token" in
    debug|release)
      PROFILE="$token"
      ;;
    loader|example|all)
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
      echo "libplayer_ffmpeg.dylib"
      ;;
    Linux)
      echo "libplayer_ffmpeg.so"
      ;;
    MINGW*|MSYS*|CYGWIN*)
      echo "player_ffmpeg.dll"
      ;;
    *)
      echo "Unsupported platform: $(uname -s)" >&2
      exit 1
      ;;
  esac
}

normalize_runtime_path() {
  local path="$1"
  case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*)
      if command -v cygpath >/dev/null 2>&1; then
        cygpath -w "$path"
      else
        printf '%s\n' "$path"
      fi
      ;;
    *)
      printf '%s\n' "$path"
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

ensure_tool_available() {
  local tool="$1"
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Required tool is unavailable: $tool" >&2
    exit 1
  fi
}

is_ci_environment() {
  [[ "${CI:-}" == "true" || -n "${GITHUB_ACTIONS:-}" ]]
}

resolve_plugin_path() {
  local library_name="$1"
  local target_dir="$2"
  local candidate

  if [[ -n "$LIBRARY_PATH_OVERRIDE" ]]; then
    if [[ ! -f "$LIBRARY_PATH_OVERRIDE" ]]; then
      echo "VESPER_PLAYER_FFMPEG_PLUGIN_PATH points to a missing file: $LIBRARY_PATH_OVERRIDE" >&2
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

  echo "Could not find $library_name under $target_dir; build player-ffmpeg first or set VESPER_PLAYER_FFMPEG_PLUGIN_PATH." >&2
  exit 1
}

build_plugin() {
  if [[ -n "$LIBRARY_PATH_OVERRIDE" ]]; then
    return 0
  fi

  if [[ "$PROFILE" == "release" ]]; then
    cargo build -p player-ffmpeg --release
  else
    cargo build -p player-ffmpeg
  fi
}

run_loader_test() {
  cargo test \
    -p player-plugin-loader \
    tests::dynamic_loader_opens_real_player_ffmpeg_shared_library \
    -- \
    --ignored \
    --exact
}

run_example_test() {
  ensure_tool_available ffmpeg
  ensure_tool_available ffprobe

  if [[ ! -f "$ROOT_DIR/test-video.mp4" ]]; then
    if is_ci_environment; then
      echo "Desktop remux fixture is missing in CI, skipping example remux verification: $ROOT_DIR/test-video.mp4" >&2
      return 0
    fi
    echo "Desktop remux fixture is missing: $ROOT_DIR/test-video.mp4" >&2
    exit 1
  fi

  cargo test \
    -p basic-player \
    desktop_download::tests::desktop_export_remuxes_downloaded_hls_fixture_to_mp4_via_dynamic_plugin \
    -- \
    --ignored \
    --exact
}

main() {
  local library_name
  local target_dir
  local plugin_path

  library_name="$(shared_library_name)"
  target_dir="$(resolve_target_dir)"

  build_plugin
  plugin_path="$(resolve_plugin_path "$library_name" "$target_dir")"
  export VESPER_PLAYER_FFMPEG_PLUGIN_PATH="$(normalize_runtime_path "$plugin_path")"

  echo "Using player-ffmpeg plugin: $VESPER_PLAYER_FFMPEG_PLUGIN_PATH"

  case "$MODE" in
    loader)
      run_loader_test
      ;;
    example)
      run_example_test
      ;;
    all)
      run_loader_test
      run_example_test
      ;;
  esac
}

main "$@"
