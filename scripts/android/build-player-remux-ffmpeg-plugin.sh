#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/android.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
FFMPEG_ANDROID_DIR="$ROOT_DIR/third_party/ffmpeg/android"
OPENSSL_ANDROID_DIR="$ROOT_DIR/third_party/openssl/android"
LIBXML2_ANDROID_DIR="$ROOT_DIR/third_party/libxml2/android"
OUTPUT_DIR="${1:-}"

if [[ -z "$OUTPUT_DIR" ]]; then
  echo "Usage: $0 <output-dir> [debug|release] [abi...]" >&2
  exit 1
fi

shift || true

PROFILE="debug"
if [[ $# -gt 0 && ( "$1" == "debug" || "$1" == "release" ) ]]; then
  PROFILE="$1"
  shift
fi

ANDROID_SDK_ROOT="$(vesper_android_sdk_root)"
ANDROID_NDK_VERSION="$(vesper_android_ndk_version)"
ANDROID_NDK_ROOT="${ANDROID_NDK_ROOT:-}"

selected_abis=()
while IFS= read -r abi; do
  selected_abis+=("$abi")
done < <(vesper_android_resolve_selected_abis "$@")

required_targets=()
for abi in "${selected_abis[@]}"; do
  required_targets+=("$(vesper_android_abi_to_rust_target "$abi")")
done

vesper_android_require_cargo_ndk "Android player-remux-ffmpeg plugins"
vesper_android_require_rust_targets "${required_targets[@]}"

if ! ANDROID_NDK_ROOT="$(vesper_android_resolve_ndk_root "$ANDROID_SDK_ROOT" "$ANDROID_NDK_ROOT" "$ANDROID_NDK_VERSION")"; then
  vesper_android_report_missing_ndk "$ANDROID_SDK_ROOT" "$ANDROID_NDK_VERSION"
  exit 1
fi

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

missing_ffmpeg_abis=()
for abi in "${selected_abis[@]}"; do
  if [[ ! -f "$FFMPEG_ANDROID_DIR/$abi/lib/pkgconfig/libavformat.pc" ]]; then
    missing_ffmpeg_abis+=("$abi")
  fi
done

if [[ ${#missing_ffmpeg_abis[@]} -gt 0 ]]; then
  "$ROOT_DIR/scripts/android/build-ffmpeg-prebuilts.sh" "${missing_ffmpeg_abis[@]}"
fi

for abi in "${selected_abis[@]}"; do
  ffmpeg_abi_dir="$FFMPEG_ANDROID_DIR/$abi"
  pkgconfig_dir="$ffmpeg_abi_dir/lib/pkgconfig"

  if [[ ! -d "$pkgconfig_dir" ]]; then
    echo "Missing FFmpeg pkg-config directory for ABI $abi:" >&2
    echo "  $pkgconfig_dir" >&2
    exit 1
  fi

  if [[ "$PROFILE" == "release" ]]; then
    env \
      PKG_CONFIG_ALLOW_CROSS=1 \
      PKG_CONFIG_PATH="$pkgconfig_dir" \
      cargo ndk \
        -o "$OUTPUT_DIR" \
        -t "$abi" \
        build \
        -p player-remux-ffmpeg \
        --release
  else
    env \
      PKG_CONFIG_ALLOW_CROSS=1 \
      PKG_CONFIG_PATH="$pkgconfig_dir" \
      cargo ndk \
        -o "$OUTPUT_DIR" \
        -t "$abi" \
        build \
        -p player-remux-ffmpeg
  fi

  mkdir -p "$OUTPUT_DIR/$abi"
  find "$ffmpeg_abi_dir/lib" -maxdepth 1 -type f -name 'lib*.so' -exec cp {} "$OUTPUT_DIR/$abi/" \;

  for runtime_dependency in \
    "$OPENSSL_ANDROID_DIR/$abi/lib/libssl.so" \
    "$OPENSSL_ANDROID_DIR/$abi/lib/libcrypto.so" \
    "$LIBXML2_ANDROID_DIR/$abi/lib/libxml2.so"; do
    if [[ -f "$runtime_dependency" ]]; then
      cp "$runtime_dependency" "$OUTPUT_DIR/$abi/"
    fi
  done
done

echo
echo "Built Android player-remux-ffmpeg plugin libraries into:"
echo "  $OUTPUT_DIR"
echo "Selected Android ABIs:"
for abi in "${selected_abis[@]}"; do
  echo "  $abi"
done
