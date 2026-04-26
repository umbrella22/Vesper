#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
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

shared_library_name() {
  case "$(uname -s)" in
    Darwin)
      echo "libplayer_decoder_fixture.dylib"
      ;;
    Linux)
      echo "libplayer_decoder_fixture.so"
      ;;
    MINGW*|MSYS*|CYGWIN*)
      echo "player_decoder_fixture.dll"
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

resolve_plugin_path() {
  local library_name="$1"
  local target_dir="$2"
  local candidate

  if [[ -n "$LIBRARY_PATH_OVERRIDE" ]]; then
    if [[ ! -f "$LIBRARY_PATH_OVERRIDE" ]]; then
      echo "VESPER_DECODER_FIXTURE_PLUGIN_PATH points to a missing file: $LIBRARY_PATH_OVERRIDE" >&2
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

  echo "Could not find $library_name under $target_dir; build player-decoder-fixture first or set VESPER_DECODER_FIXTURE_PLUGIN_PATH." >&2
  exit 1
}

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

  library_name="$(shared_library_name)"
  target_dir="$(resolve_target_dir)"

  build_plugin
  plugin_path="$(resolve_plugin_path "$library_name" "$target_dir")"
  export VESPER_DECODER_PLUGIN_PATHS="$(normalize_runtime_path "$plugin_path")"
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
