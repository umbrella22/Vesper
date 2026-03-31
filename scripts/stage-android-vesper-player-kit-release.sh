#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PROJECT_DIR="$ROOT_DIR/lib/android"
CORE_MODULE_DIR="$PROJECT_DIR/vesper-player-kit"
COMPOSE_MODULE_DIR="$PROJECT_DIR/vesper-player-kit-compose"
OUTPUT_DIR="${1:-$ROOT_DIR/dist/release/android}"
shift || true

DEFAULT_ABIS=(
  "arm64-v8a"
  "x86_64"
)

selected_abis=("$@")
if [[ ${#selected_abis[@]} -eq 0 ]]; then
  selected_abis=("${DEFAULT_ABIS[@]}")
fi

if [[ -n "${ANDROID_SDK_ROOT:-}" ]]; then
  cat >"$PROJECT_DIR/local.properties" <<EOF
sdk.dir=${ANDROID_SDK_ROOT}
EOF
fi

if [[ -x "$PROJECT_DIR/gradlew" ]]; then
  GRADLE_CMD=("$PROJECT_DIR/gradlew" -p "$PROJECT_DIR")
elif command -v gradle >/dev/null 2>&1; then
  GRADLE_CMD=(gradle -p "$PROJECT_DIR")
else
  cat <<EOF >&2
No Gradle CLI was found for building Android release artifacts.

Use one of these options:
  1. Open $PROJECT_DIR in Android Studio and run:
       :vesper-player-kit:assembleRelease
       :vesper-player-kit-compose:assembleRelease
  2. Add a Gradle wrapper to $PROJECT_DIR
  3. Install a global 'gradle' command and rerun this script
EOF
  exit 1
fi

mkdir -p "$OUTPUT_DIR"

for abi in "${selected_abis[@]}"; do
  case "$abi" in
    arm64-v8a|x86_64)
      ;;
    *)
      echo "Unsupported Android ABI: $abi" >&2
      exit 1
      ;;
  esac

  rm -rf "$CORE_MODULE_DIR/src/main/jniLibs"
  "${GRADLE_CMD[@]}" :vesper-player-kit:clean :vesper-player-kit-compose:clean
  RUST_ANDROID_ABIS="$abi" "${GRADLE_CMD[@]}" \
    :vesper-player-kit:assembleRelease \
    :vesper-player-kit-compose:assembleRelease

  CORE_INPUT_AAR="$CORE_MODULE_DIR/build/outputs/aar/vesper-player-kit-release.aar"
  CORE_OUTPUT_AAR="$OUTPUT_DIR/VesperPlayerKit-android-$abi.aar"
  cp "$CORE_INPUT_AAR" "$CORE_OUTPUT_AAR"

  COMPOSE_INPUT_AAR="$COMPOSE_MODULE_DIR/build/outputs/aar/vesper-player-kit-compose-release.aar"
  COMPOSE_OUTPUT_AAR="$OUTPUT_DIR/VesperPlayerKitCompose-android-$abi.aar"
  cp "$COMPOSE_INPUT_AAR" "$COMPOSE_OUTPUT_AAR"

  echo "Staged VesperPlayerKit Android AARs:"
  echo "  $CORE_OUTPUT_AAR"
  echo "  $COMPOSE_OUTPUT_AAR"
done
