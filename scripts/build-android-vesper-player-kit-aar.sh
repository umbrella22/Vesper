#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PROJECT_DIR="$ROOT_DIR/lib/android"
MODULE_TASK="${1:-assembleRelease}"
GRADLEW="$ROOT_DIR/examples/android-compose-host/gradlew"

export GRADLE_USER_HOME="${GRADLE_USER_HOME:-$ROOT_DIR/.gradle/gradle-user-home}"

if [[ -x "$GRADLEW" ]]; then
  exec "$GRADLEW" -p "$PROJECT_DIR" \
    ":vesper-player-kit:$MODULE_TASK" \
    ":vesper-player-kit-compose:$MODULE_TASK" \
    ":vesper-player-kit-compose-ui:$MODULE_TASK"
fi

cat <<EOF >&2
No Gradle wrapper was found for building the Android AAR.

Expected executable wrapper:
  $GRADLEW
EOF

exit 1
