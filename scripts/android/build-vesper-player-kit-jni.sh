#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/android.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
LIB_DIR="$ROOT_DIR/lib/android/vesper-player-kit"
JNI_LIBS_DIR="$LIB_DIR/src/main/jniLibs"
PROFILE="${1:-debug}"
shift || true
ANDROID_SDK_ROOT="$(vesper_android_sdk_root)"
ANDROID_NDK_VERSION="$(vesper_android_ndk_version)"
ANDROID_NDK_ROOT="${ANDROID_NDK_ROOT:-}"

selected_abis=()
while IFS= read -r abi; do
  selected_abis+=("$abi")
done < <(vesper_android_resolve_selected_abis "$@")

REQUIRED_TARGETS=()
for abi in "${selected_abis[@]}"; do
  REQUIRED_TARGETS+=("$(vesper_android_abi_to_rust_target "$abi")")
done

vesper_android_require_cargo_ndk "Android JNI libraries"
vesper_android_require_rust_targets "${REQUIRED_TARGETS[@]}"

if ! ANDROID_NDK_ROOT="$(vesper_android_resolve_ndk_root "$ANDROID_SDK_ROOT" "$ANDROID_NDK_ROOT" "$ANDROID_NDK_VERSION")"; then
  vesper_android_report_missing_ndk \
    "$ANDROID_SDK_ROOT" \
    "$ANDROID_NDK_VERSION" \
    "Install Android NDK $ANDROID_NDK_VERSION from Android Studio:
  Settings > Languages & Frameworks > Android SDK > SDK Tools > NDK (Side by side)"
  echo "If Android Studio installed a different NDK version, set ANDROID_NDK_ROOT before running this script." >&2
  exit 1
fi

rm -rf "$JNI_LIBS_DIR"
mkdir -p "$JNI_LIBS_DIR"

BUILD_FLAGS=()
if [[ "$PROFILE" == "release" ]]; then
  BUILD_FLAGS+=(--release)
fi

NDK_TARGET_ARGS=()
for abi in "${selected_abis[@]}"; do
  NDK_TARGET_ARGS+=(-t "$abi")
done

if [[ ${#BUILD_FLAGS[@]} -gt 0 ]]; then
  cargo ndk \
    -o "$JNI_LIBS_DIR" \
    "${NDK_TARGET_ARGS[@]}" \
    build \
    -p player-jni-android \
    "${BUILD_FLAGS[@]}"
else
  cargo ndk \
    -o "$JNI_LIBS_DIR" \
    "${NDK_TARGET_ARGS[@]}" \
    build \
    -p player-jni-android
fi

echo
echo "Built Android JNI libraries into:"
echo "  $JNI_LIBS_DIR"
echo "Selected Android ABIs:"
for abi in "${selected_abis[@]}"; do
  echo "  $abi"
done
