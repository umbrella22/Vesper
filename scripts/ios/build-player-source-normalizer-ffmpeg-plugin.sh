#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/apple.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
FFMPEG_APPLE_BASE_DIR="$ROOT_DIR/third_party/ffmpeg/apple"
OUTPUT_DIR="${1:-}"

vesper_require_rust_tools_for_xcode

if [[ -z "$OUTPUT_DIR" ]]; then
  echo "Usage: $0 <output-dir> [debug|release] [ffmpeg-options...] [slice...]" >&2
  exit 1
fi

shift || true

PROFILE="debug"
if [[ $# -gt 0 && ( "$1" == "debug" || "$1" == "release" ) ]]; then
  PROFILE="$1"
  shift
fi

vesper_ffmpeg_parse_common_args apple "$@"
FFMPEG_APPLE_DIR="${VESPER_APPLE_FFMPEG_OUTPUT_DIR:-${VESPER_FFMPEG_OUTPUT_DIR:-$(vesper_ffmpeg_default_output_dir apple "$FFMPEG_APPLE_BASE_DIR")}}"

slice_output_path() {
  case "$1" in
    ios-arm64)
      echo "$OUTPUT_DIR/iphoneos/libplayer_source_normalizer_ffmpeg.dylib"
      ;;
    ios-simulator-arm64)
      echo "$OUTPUT_DIR/iphonesimulator/$(vesper_ios_slice_rust_target "$1")/libplayer_source_normalizer_ffmpeg.dylib"
      ;;
    *)
      return 1
      ;;
  esac
}

ensure_loader_rpath() {
  local binary_path="$1"

  if ! otool -l "$binary_path" | grep -Fq "@loader_path"; then
    install_name_tool -add_rpath "@loader_path" "$binary_path"
  fi
}

prepare_plugin_binary() {
  local binary_path="$1"
  install_name_tool -id "@rpath/libplayer_source_normalizer_ffmpeg.dylib" "$binary_path"
  ensure_loader_rpath "$binary_path"
  if ! otool -l "$binary_path" | grep -Fq "@loader_path/VesperPlayerFfmpegRuntime.framework/Frameworks"; then
    install_name_tool -add_rpath "@loader_path/VesperPlayerFfmpegRuntime.framework/Frameworks" "$binary_path"
  fi
  if ! otool -l "$binary_path" | grep -Fq "@loader_path/../VesperPlayerFfmpegRuntime.framework/Frameworks"; then
    install_name_tool -add_rpath "@loader_path/../VesperPlayerFfmpegRuntime.framework/Frameworks" "$binary_path"
  fi
}

selected_slices=()
while IFS= read -r slice; do
  selected_slices+=("$slice")
done < <(vesper_apple_resolve_selected_slices ${VESPER_FFMPEG_POSITIONAL_ARGS[@]+"${VESPER_FFMPEG_POSITIONAL_ARGS[@]}"})

required_targets=()
for slice in "${selected_slices[@]}"; do
  required_targets+=("$(vesper_ios_slice_rust_target "$slice")")
done

vesper_apple_require_rust_targets ${required_targets[@]+"${required_targets[@]}"}

if [[ "${VESPER_SKIP_APPLE_FFMPEG_PREBUILDS:-0}" != "1" ]]; then
  "$ROOT_DIR/scripts/apple/build-ffmpeg-prebuilts.sh" "$@"
fi

PROFILE_DIR="$PROFILE"
BUILD_FLAGS=()
if [[ "$PROFILE" == "release" ]]; then
  BUILD_FLAGS+=(--release)
fi

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

for slice in "${selected_slices[@]}"; do
  rust_target="$(vesper_ios_slice_rust_target "$slice")"
  ffmpeg_dir="$(vesper_apple_slice_output_root "$slice" "$FFMPEG_APPLE_DIR")"
  output_path="$(slice_output_path "$slice")"
  cargo_target_dir="$ROOT_DIR/target/player-source-normalizer-ffmpeg-ios/$(vesper_path_cache_key "$ffmpeg_dir")"
  cargo_command=(
    cargo
    build
    --manifest-path "$ROOT_DIR/Cargo.toml"
    --target "$rust_target"
    -p player-source-normalizer-ffmpeg
  )

  if [[ ${#BUILD_FLAGS[@]} -gt 0 ]]; then
    cargo_command+=("${BUILD_FLAGS[@]}")
  fi

  mkdir -p "$(dirname "$output_path")"

  env \
    FFMPEG_DIR="$ffmpeg_dir" \
    CARGO_TARGET_DIR="$cargo_target_dir" \
    "${cargo_command[@]}"

  cp "$cargo_target_dir/$rust_target/$PROFILE_DIR/libplayer_source_normalizer_ffmpeg.dylib" "$output_path"
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
    "$OUTPUT_DIR/iphonesimulator/libplayer_source_normalizer_ffmpeg.dylib"
  prepare_plugin_binary "$OUTPUT_DIR/iphonesimulator/libplayer_source_normalizer_ffmpeg.dylib"
fi

unexpected_runtime="$(
  find "$OUTPUT_DIR" -type f \
    \( -name 'libav*.dylib*' -o -name 'libsw*.dylib*' -o -name 'libssl*.dylib*' -o -name 'libcrypto*.dylib*' -o -name 'libxml2*.dylib*' \) \
    -print -quit
)"
if [[ -n "$unexpected_runtime" ]]; then
  echo "iOS player-source-normalizer-ffmpeg must not bundle FFmpeg runtime dylibs:" >&2
  echo "  $unexpected_runtime" >&2
  echo "Embed VesperPlayerFfmpegRuntime.framework alongside the plugin instead." >&2
  exit 1
fi

echo
echo "Built iOS player-source-normalizer-ffmpeg plugin libraries into:"
echo "  $OUTPUT_DIR"
echo "Using Apple FFmpeg prebuilts:"
echo "  $FFMPEG_APPLE_DIR"
echo "FFmpeg profile:"
echo "  $VESPER_FFMPEG_PROFILE"
echo "Selected slices:"
for slice in "${selected_slices[@]}"; do
  echo "  $slice"
done
echo "The plugin no longer copies FFmpeg runtime dylibs; embed VesperPlayerFfmpegRuntime.framework instead."
