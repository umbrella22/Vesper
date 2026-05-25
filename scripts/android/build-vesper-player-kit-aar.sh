#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/android.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROJECT_DIR="$ROOT_DIR/lib/android"
MODULE_TASK="${1:-assembleRelease}"
FALLBACK_PROJECT_DIR="$ROOT_DIR/examples/android-compose-host"

export GRADLE_USER_HOME="${GRADLE_USER_HOME:-$ROOT_DIR/.gradle/gradle-user-home}"

GRADLE_CMD=("$(vesper_android_resolve_gradle "$PROJECT_DIR" "$FALLBACK_PROJECT_DIR")")

exec "${GRADLE_CMD[@]}" -p "$PROJECT_DIR" \
    ":vesper-player-kit:$MODULE_TASK" \
    ":vesper-player-kit-ffmpeg-runtime:$MODULE_TASK" \
    ":vesper-player-kit-source-normalizer-ffmpeg:$MODULE_TASK" \
    ":vesper-player-kit-frame-processor-diagnostic:$MODULE_TASK" \
    ":vesper-player-kit-external-playback:$MODULE_TASK" \
    ":vesper-player-kit-compose:$MODULE_TASK" \
    ":vesper-player-kit-compose-ui:$MODULE_TASK"
