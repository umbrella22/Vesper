#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/android.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
FFMPEG_ANDROID_BASE_DIR="$ROOT_DIR/third_party/ffmpeg/android"
OPENSSL_ANDROID_DIR="$ROOT_DIR/third_party/openssl/android"
LIBXML2_ANDROID_DIR="$ROOT_DIR/third_party/libxml2/android"
OUTPUT_DIR="${1:-}"

if [[ -z "$OUTPUT_DIR" ]]; then
  echo "Usage: $0 <output-dir> [debug|release] [ffmpeg-options...] [abi...]" >&2
  exit 1
fi

shift || true

PROFILE="debug"
if [[ $# -gt 0 && ( "$1" == "debug" || "$1" == "release" ) ]]; then
  PROFILE="$1"
  shift
fi

vesper_ffmpeg_parse_common_args android "$@"
FFMPEG_ANDROID_DIR="${VESPER_ANDROID_FFMPEG_OUTPUT_DIR:-${VESPER_FFMPEG_OUTPUT_DIR:-$(vesper_ffmpeg_default_output_dir android "$FFMPEG_ANDROID_BASE_DIR")}}"

ANDROID_SDK_ROOT="$(vesper_android_sdk_root)"
ANDROID_NDK_VERSION="$(vesper_android_ndk_version)"
ANDROID_NDK_ROOT="${ANDROID_NDK_ROOT:-}"

selected_abis=()
while IFS= read -r abi; do
  selected_abis+=("$abi")
done < <(vesper_android_resolve_selected_abis ${VESPER_FFMPEG_POSITIONAL_ARGS[@]+"${VESPER_FFMPEG_POSITIONAL_ARGS[@]}"})

required_targets=()
for abi in "${selected_abis[@]}"; do
  required_targets+=("$(vesper_android_abi_to_rust_target "$abi")")
done

vesper_android_require_cargo_ndk "Android player-remux-ffmpeg plugins"
vesper_android_require_rust_targets ${required_targets[@]+"${required_targets[@]}"}

if ! ANDROID_NDK_ROOT="$(vesper_android_resolve_ndk_root "$ANDROID_SDK_ROOT" "$ANDROID_NDK_ROOT" "$ANDROID_NDK_VERSION")"; then
  vesper_android_report_missing_ndk "$ANDROID_SDK_ROOT" "$ANDROID_NDK_VERSION"
  exit 1
fi

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

"$ROOT_DIR/scripts/android/build-ffmpeg-prebuilts.sh" "$@"

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

  if [[ "$VESPER_FFMPEG_USE_OPENSSL" == "1" ]]; then
    for runtime_dependency in \
      "$OPENSSL_ANDROID_DIR/$abi/lib/libssl.so" \
      "$OPENSSL_ANDROID_DIR/$abi/lib/libcrypto.so"; do
      if [[ -f "$runtime_dependency" ]]; then
        cp "$runtime_dependency" "$OUTPUT_DIR/$abi/"
      fi
    done
  fi

  if [[ "$VESPER_FFMPEG_USE_LIBXML2" == "1" ]]; then
    runtime_dependency="$LIBXML2_ANDROID_DIR/$abi/lib/libxml2.so"
    if [[ -f "$runtime_dependency" ]]; then
      cp "$runtime_dependency" "$OUTPUT_DIR/$abi/"
    fi
  fi
done

echo
echo "Built Android player-remux-ffmpeg plugin libraries into:"
echo "  $OUTPUT_DIR"
echo "Using Android FFmpeg prebuilts:"
echo "  $FFMPEG_ANDROID_DIR"
echo "FFmpeg profile:"
echo "  $VESPER_FFMPEG_PROFILE"
echo "Selected Android ABIs:"
for abi in "${selected_abis[@]}"; do
  echo "  $abi"
done
