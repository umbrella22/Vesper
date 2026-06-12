#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/android.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROJECT_DIR="$ROOT_DIR/lib/android"
MODULE_TASK="${1:-assembleRelease}"
FALLBACK_PROJECT_DIR="$ROOT_DIR/examples/android-compose-host"

export GRADLE_USER_HOME="${GRADLE_USER_HOME:-$ROOT_DIR/.gradle/gradle-user-home}"

GRADLE_CMD=("$(vesper_android_resolve_gradle "$PROJECT_DIR" "$FALLBACK_PROJECT_DIR")")

tasks=(
    ":vesper-player-kit:$MODULE_TASK"
    ":vesper-player-kit-compose:$MODULE_TASK"
    ":vesper-player-kit-compose-ui:$MODULE_TASK"
)

case "${VESPER_ANDROID_INCLUDE_OPTIONAL_PLUGINS:-0}" in
  1|true|TRUE|yes|YES)
    DECODER_MEDIACODEC_MODULE_DIR="$PROJECT_DIR/vesper-player-kit-decoder-mediacodec"
    SOURCE_NORMALIZER_MODULE_DIR="$PROJECT_DIR/vesper-player-kit-source-normalizer-ffmpeg"
    FRAME_PROCESSOR_MODULE_DIR="$PROJECT_DIR/vesper-player-kit-frame-processor-diagnostic"
    "$ROOT_DIR/scripts/android/build-player-decoder-mediacodec-plugin.sh" \
        "$DECODER_MEDIACODEC_MODULE_DIR/src/main/jniLibs" \
        release
    "$ROOT_DIR/scripts/android/build-player-source-normalizer-ffmpeg-plugin.sh" \
        "$SOURCE_NORMALIZER_MODULE_DIR/src/main/jniLibs" \
        release \
        --profile default \
        --metadata-dir "$SOURCE_NORMALIZER_MODULE_DIR/src/main/assets/vesper-source-normalizer-ffmpeg"
    "$ROOT_DIR/scripts/android/build-player-frame-processor-diagnostic-plugin.sh" \
        "$FRAME_PROCESSOR_MODULE_DIR/src/main/jniLibs" \
        release
    tasks+=(
        ":vesper-player-kit-ffmpeg-runtime:$MODULE_TASK"
        ":vesper-player-kit-decoder-mediacodec:$MODULE_TASK"
        ":vesper-player-kit-source-normalizer-ffmpeg:$MODULE_TASK"
        ":vesper-player-kit-frame-processor-diagnostic:$MODULE_TASK"
        ":vesper-player-kit-external-playback:$MODULE_TASK"
    )
    ;;
esac

exec "${GRADLE_CMD[@]}" -p "$PROJECT_DIR" "${tasks[@]}"
