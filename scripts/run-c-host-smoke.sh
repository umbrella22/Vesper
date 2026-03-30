#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CC_BIN="${CC:-cc}"
BUILD_ONLY=0
SOURCE_PATH="$ROOT_DIR/test-video.mp4"

for arg in "$@"; do
  case "$arg" in
    --build-only)
      BUILD_ONLY=1
      ;;
    *)
      SOURCE_PATH="$arg"
      ;;
  esac
done

cd "$ROOT_DIR"

echo "[c-host] building player-ffi"
cargo build -p player-ffi

echo "[c-host] compiling examples/c-host/main.c"
"$CC_BIN" \
  examples/c-host/main.c \
  -Iinclude \
  -Ltarget/debug \
  -Wl,-rpath,@executable_path \
  -lplayer_ffi \
  -o target/debug/c-host-smoke

if [[ "$BUILD_ONLY" -eq 1 ]]; then
  echo "[c-host] built target/debug/c-host-smoke"
  exit 0
fi

echo "[c-host] running target/debug/c-host-smoke $SOURCE_PATH"
target/debug/c-host-smoke "$SOURCE_PATH"
