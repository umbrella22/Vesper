#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/android.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
OUTPUT_DIR="${1:-$ROOT_DIR/dist/release/android-samples}"
shift || true

selected_abis=("$@")
if [[ ${#selected_abis[@]} -eq 0 ]]; then
  selected_abis=("${VESPER_ANDROID_DEFAULT_ABIS[@]}")
fi

ANDROID_COMPOSE_PROJECT="$ROOT_DIR/examples/android-compose-host"
FLUTTER_PROJECT="$ROOT_DIR/examples/flutter-host"
FLUTTER_ANDROID_PROJECT="$FLUTTER_PROJECT/android"

shopt -s nullglob
mkdir -p "$OUTPUT_DIR"

ANDROID_COMPOSE_GRADLE="$(vesper_android_resolve_gradle "$ANDROID_COMPOSE_PROJECT")"

for abi in "${selected_abis[@]}"; do
  vesper_android_resolve_selected_abis "$abi" >/dev/null

  "$ROOT_DIR/scripts/vesper" ffmpeg --platform android --profile default --abi "$abi"

  "$ANDROID_COMPOSE_GRADLE" \
    -p "$ANDROID_COMPOSE_PROJECT" \
    -Pvesper.player.android.abis="$abi" \
    :app:assembleRelease

  compose_apks=("$ANDROID_COMPOSE_PROJECT"/app/build/outputs/apk/release/*.apk)
  if [[ ${#compose_apks[@]} -ne 1 ]]; then
    echo "Expected exactly one Android Compose sample APK, found ${#compose_apks[@]}." >&2
    printf '%s\n' "${compose_apks[@]}" >&2
    exit 1
  fi
  cp "${compose_apks[0]}" "$OUTPUT_DIR/VesperPlayerAndroidComposeHost-android-$abi-debug-signed.apk"

  if command -v flutter >/dev/null 2>&1; then
    (
      cd "$FLUTTER_PROJECT"
      flutter pub get
      flutter build apk --release --target-platform android-arm64 --split-per-abi
    )
  else
    FLUTTER_GRADLE="$(vesper_android_resolve_gradle "$FLUTTER_ANDROID_PROJECT")"
    "$FLUTTER_GRADLE" \
      -p "$FLUTTER_ANDROID_PROJECT" \
      -Pvesper.player.android.abis="$abi" \
      -Pvesper.player.android.app.abis="$abi" \
      :app:assembleRelease
  fi

  flutter_apks=("$FLUTTER_PROJECT"/build/app/outputs/flutter-apk/app-arm64-v8a-release.apk)
  if [[ ${#flutter_apks[@]} -ne 1 || ! -f "${flutter_apks[0]}" ]]; then
    echo "Expected exactly one Flutter Android sample APK." >&2
    printf '%s\n' "${flutter_apks[@]}" >&2
    exit 1
  fi
  cp "${flutter_apks[0]}" "$OUTPUT_DIR/VesperPlayerFlutterHost-android-$abi-debug-signed.apk"
done

echo "Staged Android sample APKs into:"
echo "  $OUTPUT_DIR"
