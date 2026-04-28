#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PROJECT_DIR="$ROOT_DIR/lib/android"
MODULE_TASK="${1:-assembleRelease}"
CACHED_GRADLE_BIN=""

export GRADLE_USER_HOME="${GRADLE_USER_HOME:-$ROOT_DIR/.gradle/gradle-user-home}"

for gradle_cache_dir in \
  "$ROOT_DIR/examples/android-compose-host/.gradle/wrapper/dists" \
  "$PROJECT_DIR/.gradle/wrapper/dists" \
  "$ROOT_DIR/.gradle/wrapper/dists"; do
  [[ -d "$gradle_cache_dir" ]] || continue
  CACHED_GRADLE_BIN="$(
    find "$gradle_cache_dir" \
      -path '*/gradle-9.4.0/bin/gradle' \
      -type f \
      -print -quit
  )"
  [[ -n "$CACHED_GRADLE_BIN" ]] && break
done

if [[ -n "$CACHED_GRADLE_BIN" && -x "$CACHED_GRADLE_BIN" ]]; then
  exec "$CACHED_GRADLE_BIN" -p "$PROJECT_DIR" \
    ":vesper-player-kit:$MODULE_TASK" \
    ":vesper-player-kit-compose:$MODULE_TASK" \
    ":vesper-player-kit-compose-ui:$MODULE_TASK"
fi

if command -v gradle >/dev/null 2>&1; then
  exec gradle -p "$PROJECT_DIR" \
    ":vesper-player-kit:$MODULE_TASK" \
    ":vesper-player-kit-compose:$MODULE_TASK" \
    ":vesper-player-kit-compose-ui:$MODULE_TASK"
fi

cat <<EOF >&2
No Gradle CLI was found for building the Android AAR.

Use one of these options:
  1. Open $PROJECT_DIR in Android Studio and run:
       :vesper-player-kit:$MODULE_TASK
       :vesper-player-kit-compose:$MODULE_TASK
       :vesper-player-kit-compose-ui:$MODULE_TASK
  2. Keep an extracted Gradle 9.4 distribution in examples/android-compose-host/.gradle, lib/android/.gradle, or .gradle
  3. Install a global 'gradle' command and rerun this script
EOF

exit 1
