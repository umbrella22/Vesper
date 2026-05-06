#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/desktop.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROFILE="debug"
MODE="all"
LIBRARY_PATH_OVERRIDE="${VESPER_PLAYER_REMUX_FFMPEG_PLUGIN_PATH:-}"

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

ensure_tool_available() {
  local tool="$1"
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Required tool is unavailable: $tool" >&2
    exit 1
  fi
}

build_plugin() {
  if [[ -n "$LIBRARY_PATH_OVERRIDE" ]]; then
    return 0
  fi

  if [[ "$PROFILE" == "release" ]]; then
    cargo build -p player-remux-ffmpeg --release
  else
    cargo build -p player-remux-ffmpeg
  fi
}

run_loader_test() {
  cargo test \
    -p player-plugin-loader \
    tests::dynamic_loader_opens_real_player_remux_ffmpeg_shared_library \
    -- \
    --ignored \
    --exact
}

run_example_test() {
  ensure_tool_available ffmpeg
  ensure_tool_available ffprobe

  if [[ ! -f "$ROOT_DIR/fixtures/media/tiny-h264-aac.m4v" ]]; then
    if vesper_desktop_is_ci_environment; then
      echo "Desktop remux fixture is missing in CI, skipping example remux verification: $ROOT_DIR/fixtures/media/tiny-h264-aac.m4v" >&2
      return 0
    fi
    echo "Desktop remux fixture is missing: $ROOT_DIR/fixtures/media/tiny-h264-aac.m4v" >&2
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

  library_name="$(vesper_desktop_shared_library_name player_remux_ffmpeg)"
  target_dir="$(vesper_desktop_target_dir)"

  build_plugin
  plugin_path="$(vesper_desktop_resolve_plugin_path "$library_name" "$target_dir" "$PROFILE" "$LIBRARY_PATH_OVERRIDE" VESPER_PLAYER_REMUX_FFMPEG_PLUGIN_PATH player-remux-ffmpeg)"
  export VESPER_PLAYER_REMUX_FFMPEG_PLUGIN_PATH="$(vesper_desktop_normalize_runtime_path "$plugin_path")"

  echo "Using player-remux-ffmpeg plugin: $VESPER_PLAYER_REMUX_FFMPEG_PLUGIN_PATH"

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
