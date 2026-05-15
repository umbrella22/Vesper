#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROJECT_DIR="$ROOT_DIR/lib/android"
MODULE_TASK="${1:-assembleRelease}"
PROJECT_GRADLEW="$PROJECT_DIR/gradlew"
LOCAL_GRADLE="$(find "$PROJECT_DIR/.gradle/wrapper/dists" -path '*/bin/gradle' -type f -perm -111 2>/dev/null | sort | tail -n 1 || true)"
FALLBACK_GRADLEW="$ROOT_DIR/examples/android-compose-host/gradlew"

export GRADLE_USER_HOME="${GRADLE_USER_HOME:-$ROOT_DIR/.gradle/gradle-user-home}"

if [[ -x "$PROJECT_GRADLEW" ]]; then
  GRADLE_CMD=("$PROJECT_GRADLEW")
elif [[ -n "$LOCAL_GRADLE" && -x "$LOCAL_GRADLE" ]]; then
  GRADLE_CMD=("$LOCAL_GRADLE")
elif [[ -x "$FALLBACK_GRADLEW" ]]; then
  GRADLE_CMD=("$FALLBACK_GRADLEW")
else
  cat <<EOF >&2
No Gradle wrapper or local Gradle distribution was found for building the Android AAR.

Checked project wrapper:
  $PROJECT_GRADLEW

Checked local distributions under:
  $PROJECT_DIR/.gradle/wrapper/dists

Checked fallback wrapper:
  $FALLBACK_GRADLEW
EOF

  exit 1
fi

exec "${GRADLE_CMD[@]}" -p "$PROJECT_DIR" \
    ":vesper-player-kit:$MODULE_TASK" \
    ":vesper-player-kit-ffmpeg-runtime:$MODULE_TASK" \
    ":vesper-player-kit-external-playback:$MODULE_TASK" \
    ":vesper-player-kit-compose:$MODULE_TASK" \
    ":vesper-player-kit-compose-ui:$MODULE_TASK"
