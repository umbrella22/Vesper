#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
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

shared_library_name() {
  case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*)
      echo "player_decoder_d3d11.dll"
      ;;
    *)
      echo "D3D11 decoder verification only runs on Windows." >&2
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

resolve_plugin_path() {
  local library_name="$1"
  local target_dir="$2"
  local candidate

  if [[ -n "$LIBRARY_PATH_OVERRIDE" ]]; then
    if [[ ! -f "$LIBRARY_PATH_OVERRIDE" ]]; then
      echo "VESPER_DECODER_D3D11_PLUGIN_PATH points to a missing file: $LIBRARY_PATH_OVERRIDE" >&2
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

  echo "Could not find $library_name under $target_dir; build player-decoder-d3d11 first or set VESPER_DECODER_D3D11_PLUGIN_PATH." >&2
  exit 1
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

  library_name="$(shared_library_name)"
  target_dir="$(resolve_target_dir)"

  build_plugin
  plugin_path="$(resolve_plugin_path "$library_name" "$target_dir")"
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
