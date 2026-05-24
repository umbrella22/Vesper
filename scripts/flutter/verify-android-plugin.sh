#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/android.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROJECT_DIR="$ROOT_DIR/examples/flutter-host/android"

export GRADLE_USER_HOME="${GRADLE_USER_HOME:-$ROOT_DIR/.gradle/gradle-user-home}"

GRADLE_CMD=("$(vesper_android_resolve_gradle "$PROJECT_DIR")")

exec "${GRADLE_CMD[@]}" -p "$PROJECT_DIR" \
  ":vesper_player_android:compileDebugKotlin" \
  ":vesper_player_external_playback:compileDebugKotlin"
