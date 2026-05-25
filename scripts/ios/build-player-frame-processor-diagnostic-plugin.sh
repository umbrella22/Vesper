#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/apple.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
OUTPUT_DIR="${1:-}"

vesper_require_rust_tools_for_xcode

if [[ -z "$OUTPUT_DIR" ]]; then
  echo "Usage: $0 <output-dir> [debug|release] [slice...]" >&2
  exit 1
fi

shift || true

PROFILE="debug"
if [[ $# -gt 0 && ( "$1" == "debug" || "$1" == "release" ) ]]; then
  PROFILE="$1"
  shift
fi

slice_output_path() {
  case "$1" in
    ios-arm64)
      echo "$OUTPUT_DIR/iphoneos/libplayer_frame_processor_diagnostic.dylib"
      ;;
    ios-simulator-arm64)
      echo "$OUTPUT_DIR/iphonesimulator/$(vesper_ios_slice_rust_target "$1")/libplayer_frame_processor_diagnostic.dylib"
      ;;
    *)
      return 1
      ;;
  esac
}

prepare_plugin_binary() {
  local binary_path="$1"
  install_name_tool -id "@rpath/libplayer_frame_processor_diagnostic.dylib" "$binary_path"
}

selected_slices=()
while IFS= read -r slice; do
  selected_slices+=("$slice")
done < <(vesper_apple_resolve_selected_slices "$@")

required_targets=()
for slice in "${selected_slices[@]}"; do
  required_targets+=("$(vesper_ios_slice_rust_target "$slice")")
done

vesper_apple_require_rust_targets ${required_targets[@]+"${required_targets[@]}"}

PROFILE_DIR="$PROFILE"
BUILD_FLAGS=()
if [[ "$PROFILE" == "release" ]]; then
  BUILD_FLAGS+=(--release)
fi

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

for slice in "${selected_slices[@]}"; do
  rust_target="$(vesper_ios_slice_rust_target "$slice")"
  output_path="$(slice_output_path "$slice")"
  cargo_target_dir="$ROOT_DIR/target/player-frame-processor-diagnostic-ios"
  cargo_command=(
    cargo
    build
    --manifest-path "$ROOT_DIR/Cargo.toml"
    --target "$rust_target"
    -p player-frame-processor-diagnostic
  )

  if [[ ${#BUILD_FLAGS[@]} -gt 0 ]]; then
    cargo_command+=("${BUILD_FLAGS[@]}")
  fi

  mkdir -p "$(dirname "$output_path")"

  env \
    CARGO_TARGET_DIR="$cargo_target_dir" \
    "${cargo_command[@]}"

  cp "$cargo_target_dir/$rust_target/$PROFILE_DIR/libplayer_frame_processor_diagnostic.dylib" "$output_path"
  prepare_plugin_binary "$output_path"
done

simulator_slices=()
for slice in "${selected_slices[@]}"; do
  case "$slice" in
    ios-simulator-arm64)
      simulator_slices+=("$slice")
      ;;
  esac
done

if [[ ${#simulator_slices[@]} -gt 0 ]]; then
  mkdir -p "$OUTPUT_DIR/iphonesimulator"
  cp \
    "$(slice_output_path "${simulator_slices[0]}")" \
    "$OUTPUT_DIR/iphonesimulator/libplayer_frame_processor_diagnostic.dylib"
  prepare_plugin_binary "$OUTPUT_DIR/iphonesimulator/libplayer_frame_processor_diagnostic.dylib"
fi

unexpected_runtime="$(
  find "$OUTPUT_DIR" -type f \
    \( -name 'libav*.dylib*' -o -name 'libsw*.dylib*' -o -name 'libssl*.dylib*' -o -name 'libcrypto*.dylib*' -o -name 'libxml2*.dylib*' \) \
    -print -quit
)"
if [[ -n "$unexpected_runtime" ]]; then
  echo "iOS player-frame-processor-diagnostic must not bundle FFmpeg runtime dylibs:" >&2
  echo "  $unexpected_runtime" >&2
  exit 1
fi

echo
echo "Built iOS player-frame-processor-diagnostic plugin libraries into:"
echo "  $OUTPUT_DIR"
