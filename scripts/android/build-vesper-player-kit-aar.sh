#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROJECT_DIR="$ROOT_DIR/lib/android"
MODULE_TASK="${1:-assembleRelease}"
GRADLEW="$ROOT_DIR/examples/android-compose-host/gradlew"
LOCAL_GRADLE="$(find "$PROJECT_DIR/.gradle/wrapper/dists" -path '*/bin/gradle' -type f -perm -111 2>/dev/null | sort | tail -n 1 || true)"

export GRADLE_USER_HOME="${GRADLE_USER_HOME:-$ROOT_DIR/.gradle/gradle-user-home}"

if [[ -x "$GRADLEW" ]]; then
  GRADLE_CMD=("$GRADLEW")
elif [[ -n "$LOCAL_GRADLE" && -x "$LOCAL_GRADLE" ]]; then
  GRADLE_CMD=("$LOCAL_GRADLE")
else
  cat <<EOF >&2
No Gradle wrapper or local Gradle distribution was found for building the Android AAR.

Expected executable wrapper:
  $GRADLEW

Checked local distributions under:
  $PROJECT_DIR/.gradle/wrapper/dists
EOF

  exit 1
fi

exec "${GRADLE_CMD[@]}" -p "$PROJECT_DIR" \
    ":vesper-player-kit:$MODULE_TASK" \
    ":vesper-player-kit-cast:$MODULE_TASK" \
    ":vesper-player-kit-relay:$MODULE_TASK" \
    ":vesper-player-kit-dlna:$MODULE_TASK" \
    ":vesper-player-kit-compose:$MODULE_TASK" \
    ":vesper-player-kit-compose-ui:$MODULE_TASK"
