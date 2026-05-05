#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/desktop.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROFILE="debug"
MODE="loader"
LIBRARY_PATH_OVERRIDE="${VESPER_DECODER_D3D11_PLUGIN_PATH:-}"

usage() {
  cat <<EOF >&2
Usage: $(basename "$0") [debug|release] [loader|all]

Examples:
  $(basename "$0")
  $(basename "$0") debug loader
  $(basename "$0") release all
EOF
}

for token in "$@"; do
  case "$token" in
    debug|release)
      PROFILE="$token"
      ;;
    loader|all)
      MODE="$token"
      ;;
    *)
      usage
      exit 1
      ;;
  esac
done

resolve_d3d11_library_name() {
  case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*)
      vesper_desktop_shared_library_name player_decoder_d3d11
      ;;
    *)
      echo "D3D11 decoder verification only runs on Windows." >&2
      exit 1
      ;;
  esac
}

build_plugin() {
  if [[ -n "$LIBRARY_PATH_OVERRIDE" ]]; then
    return 0
  fi

  if [[ "$PROFILE" == "release" ]]; then
    cargo build -p player-decoder-d3d11 --release
  else
    cargo build -p player-decoder-d3d11
  fi
}

run_loader_test() {
  cargo test \
    -p player-plugin-loader \
    tests::dynamic_loader_opens_real_d3d11_decoder_shared_library \
    -- \
    --ignored \
    --exact
}

run_crate_tests() {
  cargo test -p player-decoder-d3d11
}

main() {
  local library_name
  local target_dir
  local plugin_path

  library_name="$(resolve_d3d11_library_name)"
  target_dir="$(vesper_desktop_target_dir)"

  build_plugin
  plugin_path="$(vesper_desktop_resolve_plugin_path "$library_name" "$target_dir" "$PROFILE" "$LIBRARY_PATH_OVERRIDE" VESPER_DECODER_D3D11_PLUGIN_PATH player-decoder-d3d11)"
  export VESPER_DECODER_D3D11_PLUGIN_PATH="$plugin_path"

  echo "Using D3D11 decoder plugin: $VESPER_DECODER_D3D11_PLUGIN_PATH"

  case "$MODE" in
    loader)
      run_loader_test
      ;;
    all)
      run_crate_tests
      run_loader_test
      ;;
  esac
}

main "$@"
