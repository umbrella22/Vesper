#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/android.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROJECT_DIR="$ROOT_DIR/lib/android"
CORE_MODULE_DIR="$PROJECT_DIR/vesper-player-kit"
COMPOSE_MODULE_DIR="$PROJECT_DIR/vesper-player-kit-compose"
COMPOSE_UI_MODULE_DIR="$PROJECT_DIR/vesper-player-kit-compose-ui"
EXTERNAL_PLAYBACK_MODULE_DIR="$PROJECT_DIR/vesper-player-kit-external-playback"
FFMPEG_RUNTIME_MODULE_DIR="$PROJECT_DIR/vesper-player-kit-ffmpeg-runtime"
PROJECT_GRADLEW="$PROJECT_DIR/gradlew"
LOCAL_GRADLE="$(find "$PROJECT_DIR/.gradle/wrapper/dists" -path '*/bin/gradle' -type f -perm -111 2>/dev/null | sort | tail -n 1 || true)"
FALLBACK_GRADLEW="$ROOT_DIR/examples/android-compose-host/gradlew"
OUTPUT_DIR="${1:-$ROOT_DIR/dist/release/android}"
shift || true

selected_abis=("$@")
if [[ ${#selected_abis[@]} -eq 0 ]]; then
  selected_abis=("${VESPER_ANDROID_DEFAULT_ABIS[@]}")
fi

if [[ -n "${ANDROID_SDK_ROOT:-}" ]]; then
  cat >"$PROJECT_DIR/local.properties" <<EOF
sdk.dir=${ANDROID_SDK_ROOT}
EOF
fi

if [[ -x "$PROJECT_GRADLEW" ]]; then
  GRADLE_CMD=("$PROJECT_GRADLEW" -p "$PROJECT_DIR")
elif [[ -n "$LOCAL_GRADLE" && -x "$LOCAL_GRADLE" ]]; then
  GRADLE_CMD=("$LOCAL_GRADLE" -p "$PROJECT_DIR")
elif [[ -x "$FALLBACK_GRADLEW" ]]; then
  GRADLE_CMD=("$FALLBACK_GRADLEW" -p "$PROJECT_DIR")
else
  cat <<EOF >&2
No Gradle wrapper was found for building Android release artifacts.

Checked project wrapper:
  $PROJECT_GRADLEW

Checked local distributions under:
  $PROJECT_DIR/.gradle/wrapper/dists

Checked fallback wrapper:
  $FALLBACK_GRADLEW
EOF
  exit 1
fi

mkdir -p "$OUTPUT_DIR"

for abi in "${selected_abis[@]}"; do
  case "$abi" in
    arm64-v8a)
      ;;
    *)
      echo "Unsupported Android ABI: $abi" >&2
      exit 1
      ;;
  esac

  rm -rf "$CORE_MODULE_DIR/src/main/jniLibs"
  "$ROOT_DIR/scripts/vesper" ffmpeg --platform android --profile default --abi "$abi"

  "${GRADLE_CMD[@]}" \
    :vesper-player-kit:clean \
    :vesper-player-kit-compose:clean \
    :vesper-player-kit-compose-ui:clean \
    :vesper-player-kit-external-playback:clean \
    :vesper-player-kit-ffmpeg-runtime:clean
  RUST_ANDROID_ABIS="$abi" \
  VESPER_ANDROID_SKIP_FFMPEG_RUNTIME_BUILD=1 \
    "${GRADLE_CMD[@]}" \
    -Pvesper.player.android.abis="$abi" \
    -Pvesper.player.android.external.nativeBuildProfile=release \
    -Pvesper.player.android.external.ffmpegProfile=default \
    :vesper-player-kit-external-playback:buildRelayFfmpegAndroidJni
  RUST_ANDROID_ABIS="$abi" \
  VESPER_ANDROID_SKIP_FFMPEG_RUNTIME_BUILD=1 \
    "${GRADLE_CMD[@]}" \
    -Pvesper.player.android.abis="$abi" \
    -Pvesper.player.android.external.nativeBuildProfile=release \
    -Pvesper.player.android.external.ffmpegProfile=default \
    :vesper-player-kit:assembleRelease \
    :vesper-player-kit-compose:assembleRelease \
    :vesper-player-kit-compose-ui:assembleRelease \
    :vesper-player-kit-external-playback:assembleRelease \
    :vesper-player-kit-ffmpeg-runtime:assembleRelease

  CORE_INPUT_AAR="$CORE_MODULE_DIR/build/outputs/aar/vesper-player-kit-release.aar"
  CORE_OUTPUT_AAR="$OUTPUT_DIR/VesperPlayerKit-android-$abi.aar"
  cp "$CORE_INPUT_AAR" "$CORE_OUTPUT_AAR"

  COMPOSE_INPUT_AAR="$COMPOSE_MODULE_DIR/build/outputs/aar/vesper-player-kit-compose-release.aar"
  COMPOSE_OUTPUT_AAR="$OUTPUT_DIR/VesperPlayerKitCompose-android-$abi.aar"
  cp "$COMPOSE_INPUT_AAR" "$COMPOSE_OUTPUT_AAR"

  COMPOSE_UI_INPUT_AAR="$COMPOSE_UI_MODULE_DIR/build/outputs/aar/vesper-player-kit-compose-ui-release.aar"
  COMPOSE_UI_OUTPUT_AAR="$OUTPUT_DIR/VesperPlayerKitComposeUi-android-$abi.aar"
  cp "$COMPOSE_UI_INPUT_AAR" "$COMPOSE_UI_OUTPUT_AAR"

  EXTERNAL_PLAYBACK_INPUT_AAR="$EXTERNAL_PLAYBACK_MODULE_DIR/build/outputs/aar/vesper-player-kit-external-playback-release.aar"
  EXTERNAL_PLAYBACK_OUTPUT_AAR="$OUTPUT_DIR/VesperPlayerKitExternalPlayback-android-$abi.aar"
  cp "$EXTERNAL_PLAYBACK_INPUT_AAR" "$EXTERNAL_PLAYBACK_OUTPUT_AAR"

  FFMPEG_RUNTIME_INPUT_AAR="$FFMPEG_RUNTIME_MODULE_DIR/build/outputs/aar/vesper-player-kit-ffmpeg-runtime-release.aar"
  FFMPEG_RUNTIME_OUTPUT_AAR="$OUTPUT_DIR/VesperPlayerKitFfmpegRuntime-android-$abi.aar"
  cp "$FFMPEG_RUNTIME_INPUT_AAR" "$FFMPEG_RUNTIME_OUTPUT_AAR"

  echo "Staged VesperPlayerKit Android AARs:"
  echo "  $CORE_OUTPUT_AAR"
  echo "  $COMPOSE_OUTPUT_AAR"
  echo "  $COMPOSE_UI_OUTPUT_AAR"
  echo "  $EXTERNAL_PLAYBACK_OUTPUT_AAR"
  echo "  $FFMPEG_RUNTIME_OUTPUT_AAR"
done
