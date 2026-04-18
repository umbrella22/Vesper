#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PROJECT_DIR="$ROOT_DIR/lib/android"
MODULE_TASK="${1:-assembleRelease}"
FALLBACK_WRAPPER="$ROOT_DIR/examples/android-compose-host/gradlew"
CACHED_GRADLE_BIN="$(
  find \
    "$ROOT_DIR/examples/android-compose-host/.gradle/wrapper/dists" \
    "$ROOT_DIR/.gradle/wrapper/dists" \
    -path '*/gradle-9.4.0/bin/gradle' \
    -type f \
    2>/dev/null | head -n 1
)"

if [[ -x "$PROJECT_DIR/gradlew" ]]; then
  exec "$PROJECT_DIR/gradlew" -p "$PROJECT_DIR" \
    ":vesper-player-kit:$MODULE_TASK" \
    ":vesper-player-kit-compose:$MODULE_TASK"
fi

if [[ -n "$CACHED_GRADLE_BIN" && -x "$CACHED_GRADLE_BIN" ]]; then
  exec "$CACHED_GRADLE_BIN" -p "$PROJECT_DIR" \
    ":vesper-player-kit:$MODULE_TASK" \
    ":vesper-player-kit-compose:$MODULE_TASK"
fi

if [[ -x "$FALLBACK_WRAPPER" ]]; then
  exec "$FALLBACK_WRAPPER" -p "$PROJECT_DIR" \
    ":vesper-player-kit:$MODULE_TASK" \
    ":vesper-player-kit-compose:$MODULE_TASK"
fi

if command -v gradle >/dev/null 2>&1; then
  exec gradle -p "$PROJECT_DIR" \
    ":vesper-player-kit:$MODULE_TASK" \
    ":vesper-player-kit-compose:$MODULE_TASK"
fi

cat <<EOF >&2
No Gradle CLI was found for building the Android AAR.

Use one of these options:
  1. Open $PROJECT_DIR in Android Studio and run:
       :vesper-player-kit:$MODULE_TASK
       :vesper-player-kit-compose:$MODULE_TASK
  2. Add a Gradle wrapper to $PROJECT_DIR
  3. Keep $FALLBACK_WRAPPER available as a wrapper fallback
  4. Keep an extracted Gradle distribution in examples/android-compose-host/.gradle or .gradle
  5. Install a global 'gradle' command and rerun this script
EOF

exit 1
