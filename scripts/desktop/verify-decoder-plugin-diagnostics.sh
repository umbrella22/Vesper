#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/desktop.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROFILE="debug"
MODE="all"
LIBRARY_PATH_OVERRIDE="${VESPER_DECODER_FIXTURE_PLUGIN_PATH:-}"

usage() {
  cat <<EOF >&2
Usage: $(basename "$0") [debug|release] [loader|macos|all]

Examples:
  $(basename "$0")
  $(basename "$0") loader
  $(basename "$0") debug all
  $(basename "$0") release macos
EOF
}

for token in "$@"; do
  case "$token" in
    debug|release)
      PROFILE="$token"
      ;;
    loader|macos|all)
      MODE="$token"
      ;;
    *)
      usage
      exit 1
      ;;
  esac
done

build_plugin() {
  if [[ -n "$LIBRARY_PATH_OVERRIDE" ]]; then
    return 0
  fi

  if [[ "$PROFILE" == "release" ]]; then
    cargo build -p player-decoder-fixture --release
  else
    cargo build -p player-decoder-fixture
  fi
}

run_loader_test() {
  cargo test \
    -p player-plugin-loader \
    tests::dynamic_loader_opens_real_decoder_fixture_shared_library \
    -- \
    --ignored \
    --exact
}

run_macos_diagnostics_test() {
  if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "Skipping macOS decoder diagnostics test on $(uname -s)."
    return 0
  fi

  cargo test \
    -p player-platform-macos \
    tests::macos_runtime_diagnostics_loads_real_decoder_fixture_library \
    -- \
    --ignored \
    --exact
}

main() {
  local library_name
  local target_dir
  local plugin_path

  library_name="$(vesper_desktop_shared_library_name player_decoder_fixture)"
  target_dir="$(vesper_desktop_target_dir)"

  build_plugin
  plugin_path="$(vesper_desktop_resolve_plugin_path "$library_name" "$target_dir" "$PROFILE" "$LIBRARY_PATH_OVERRIDE" VESPER_DECODER_FIXTURE_PLUGIN_PATH player-decoder-fixture)"
  export VESPER_DECODER_PLUGIN_PATHS="$(vesper_desktop_normalize_runtime_path "$plugin_path")"
  export VESPER_DECODER_FIXTURE_CODECS="${VESPER_DECODER_FIXTURE_CODECS:-fixture-video,H264,HEVC}"

  echo "Using decoder fixture plugin: $VESPER_DECODER_PLUGIN_PATHS"
  echo "Fixture decoder codecs: $VESPER_DECODER_FIXTURE_CODECS"

  case "$MODE" in
    loader)
      run_loader_test
      ;;
    macos)
      run_macos_diagnostics_test
      ;;
    all)
      run_loader_test
      run_macos_diagnostics_test
      ;;
  esac
}

main "$@"
