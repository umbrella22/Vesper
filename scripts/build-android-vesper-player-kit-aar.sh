#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PROJECT_DIR="$ROOT_DIR/lib/android"
MODULE_TASK="${1:-assembleRelease}"

if [[ -x "$PROJECT_DIR/gradlew" ]]; then
  exec "$PROJECT_DIR/gradlew" -p "$PROJECT_DIR" \
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
  3. Install a global 'gradle' command and rerun this script
EOF

exit 1
