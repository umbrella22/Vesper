#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/android.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
OUTPUT_DIR="${1:-}"

if [[ -z "$OUTPUT_DIR" ]]; then
  echo "Usage: $0 <output-dir> [debug|release]" >&2
  echo "Android ABI selection is controlled by RUST_ANDROID_ABIS." >&2
  exit 1
fi

shift || true

BUILD_PROFILE="debug"
if [[ $# -gt 0 && ( "$1" == "debug" || "$1" == "release" ) ]]; then
  BUILD_PROFILE="$1"
  shift
fi

if [[ $# -gt 0 ]]; then
  echo "Unexpected arguments: $*" >&2
  exit 1
fi

ANDROID_SDK_ROOT="$(vesper_android_sdk_root)"
ANDROID_NDK_VERSION="$(vesper_android_ndk_version)"
ANDROID_NDK_ROOT="${ANDROID_NDK_ROOT:-}"

selected_abis=()
while IFS= read -r abi; do
  selected_abis+=("$abi")
done < <(vesper_android_resolve_selected_abis)

required_targets=()
for abi in "${selected_abis[@]}"; do
  required_targets+=("$(vesper_android_abi_to_rust_target "$abi")")
done

vesper_android_require_cargo_ndk "Android player-frame-processor-diagnostic plugins"
vesper_android_require_rust_targets ${required_targets[@]+"${required_targets[@]}"}

if ! ANDROID_NDK_ROOT="$(vesper_android_resolve_ndk_root "$ANDROID_SDK_ROOT" "$ANDROID_NDK_ROOT" "$ANDROID_NDK_VERSION")"; then
  vesper_android_report_missing_ndk "$ANDROID_SDK_ROOT" "$ANDROID_NDK_VERSION"
  exit 1
fi

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

for abi in "${selected_abis[@]}"; do
  cargo_args=(ndk -o "$OUTPUT_DIR" -t "$abi" build -p player-frame-processor-diagnostic)
  if [[ "$BUILD_PROFILE" == "release" ]]; then
    cargo_args+=(--release)
  fi
  cargo "${cargo_args[@]}"
done

unexpected_runtime="$(
  find "$OUTPUT_DIR" -type f \
    \( -name 'libav*.so' -o -name 'libsw*.so' -o -name 'libssl*.so' -o -name 'libcrypto*.so' -o -name 'libxml2*.so' \) \
    -print -quit
)"
if [[ -n "$unexpected_runtime" ]]; then
  echo "player-frame-processor-diagnostic must not bundle FFmpeg runtime libraries:" >&2
  echo "  $unexpected_runtime" >&2
  exit 1
fi

echo
echo "Built Android player-frame-processor-diagnostic plugin libraries into:"
echo "  $OUTPUT_DIR"
