#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ios-framework.sh"

usage() {
  cat <<EOF >&2
Usage: $0 <plugin-id> <output-dir> [debug|release] [ffmpeg-options...] [slice...]

Supported plugin IDs: ${VESPER_IOS_OPTIONAL_PLUGIN_IDS[*]}
EOF
}

PLUGIN_ID="${1:-}"
if ! vesper_ios_resolve_plugin "$PLUGIN_ID"; then
  usage
  exit 1
fi
shift

ROOT_DIR="$VESPER_REPO_ROOT"
OUTPUT_DIR="${1:-}"

vesper_require_rust_tools_for_xcode

if [[ -z "$OUTPUT_DIR" ]]; then
  usage
  exit 1
fi
shift

PROFILE="debug"
if [[ $# -gt 0 && ( "$1" == "debug" || "$1" == "release" ) ]]; then
  PROFILE="$1"
  shift
fi

BUILD_INPUT_ARGS=("$@")
FFMPEG_APPLE_DIR=""
if [[ "$VESPER_IOS_PLUGIN_USES_FFMPEG" == "1" ]]; then
  source "$ROOT_DIR/scripts/lib/ffmpeg.sh"
  source "$ROOT_DIR/scripts/lib/ffmpeg-validate.sh"
  vesper_ffmpeg_parse_common_args apple "${BUILD_INPUT_ARGS[@]}"
  FFMPEG_APPLE_BASE_DIR="$ROOT_DIR/third_party/ffmpeg/apple"
  FFMPEG_APPLE_DIR="${VESPER_APPLE_FFMPEG_OUTPUT_DIR:-${VESPER_FFMPEG_OUTPUT_DIR:-$(vesper_ffmpeg_default_output_dir apple "$FFMPEG_APPLE_BASE_DIR")}}"
fi

slice_output_path() {
  case "$1" in
    ios-arm64)
      echo "$OUTPUT_DIR/iphoneos/$VESPER_IOS_PLUGIN_DYLIB"
      ;;
    ios-simulator-arm64)
      echo "$OUTPUT_DIR/iphonesimulator/$(vesper_ios_slice_rust_target "$1")/$VESPER_IOS_PLUGIN_DYLIB"
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

  install_name_tool -id "@rpath/$VESPER_IOS_PLUGIN_DYLIB" "$binary_path"
  if [[ "$VESPER_IOS_PLUGIN_USES_FFMPEG" == "1" ]]; then
    ensure_loader_rpath "$binary_path"
    vesper_ios_remove_rpath \
      "$binary_path" \
      "@loader_path/VesperPlayerFfmpegRuntime.framework/Frameworks"
    vesper_ios_remove_rpath \
      "$binary_path" \
      "@loader_path/../VesperPlayerFfmpegRuntime.framework/Frameworks"
    vesper_ios_ensure_rpath "$binary_path" "@loader_path/.."
  fi
}

resolved_slices=""
if [[ "$VESPER_IOS_PLUGIN_USES_FFMPEG" == "1" ]]; then
  if ! resolved_slices="$(vesper_apple_resolve_selected_slices ${VESPER_FFMPEG_POSITIONAL_ARGS[@]+"${VESPER_FFMPEG_POSITIONAL_ARGS[@]}"})"; then
    exit 1
  fi
else
  if ! resolved_slices="$(vesper_apple_resolve_selected_slices "${BUILD_INPUT_ARGS[@]}")"; then
    exit 1
  fi
fi

selected_slices=()
while IFS= read -r slice; do
  [[ -n "$slice" ]] && selected_slices+=("$slice")
done <<<"$resolved_slices"

required_targets=()
for slice in "${selected_slices[@]}"; do
  required_targets+=("$(vesper_ios_slice_rust_target "$slice")")
done
vesper_apple_require_rust_targets "${required_targets[@]}"

if [[ "$VESPER_IOS_PLUGIN_USES_FFMPEG" == "1" && "${VESPER_SKIP_APPLE_FFMPEG_PREBUILDS:-0}" != "1" ]]; then
  "$ROOT_DIR/scripts/apple/build-ffmpeg-prebuilts.sh" "${BUILD_INPUT_ARGS[@]}"
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
  output_path="$(slice_output_path "$slice")"
  cargo_target_dir="$ROOT_DIR/target/$VESPER_IOS_PLUGIN_CRATE-ios"
  if [[ "$VESPER_IOS_PLUGIN_USES_FFMPEG" == "1" ]]; then
    ffmpeg_dir="$(vesper_apple_slice_output_root "$slice" "$FFMPEG_APPLE_DIR")"
    ffmpeg_input_fingerprint="$(vesper_ffmpeg_build_input_fingerprint "$ffmpeg_dir")"
    cargo_target_dir="$cargo_target_dir/$(vesper_path_cache_key "$ffmpeg_dir")/$ffmpeg_input_fingerprint"
  fi
  cargo_command=(
    cargo
    build
    --manifest-path "$ROOT_DIR/Cargo.toml"
    --target "$rust_target"
    -p "$VESPER_IOS_PLUGIN_CRATE"
  )
  if [[ ${#BUILD_FLAGS[@]} -gt 0 ]]; then
    cargo_command+=("${BUILD_FLAGS[@]}")
  fi

  mkdir -p "$(dirname "$output_path")"
  if [[ "$VESPER_IOS_PLUGIN_USES_FFMPEG" == "1" ]]; then
    if [[ "$VESPER_IOS_PLUGIN_LINK_HEADERPAD" == "1" ]]; then
      env \
        FFMPEG_DIR="$ffmpeg_dir" \
        CARGO_TARGET_DIR="$cargo_target_dir" \
        RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-Wl,-headerpad_max_install_names" \
        "${cargo_command[@]}"
    else
      env \
        FFMPEG_DIR="$ffmpeg_dir" \
        CARGO_TARGET_DIR="$cargo_target_dir" \
        "${cargo_command[@]}"
    fi
  else
    env CARGO_TARGET_DIR="$cargo_target_dir" "${cargo_command[@]}"
  fi

  cp "$cargo_target_dir/$rust_target/$PROFILE_DIR/$VESPER_IOS_PLUGIN_DYLIB" "$output_path"
  prepare_plugin_binary "$output_path"
done

simulator_slice=""
for slice in "${selected_slices[@]}"; do
  if [[ "$slice" == "ios-simulator-arm64" ]]; then
    simulator_slice="$slice"
    break
  fi
done
if [[ -n "$simulator_slice" ]]; then
  mkdir -p "$OUTPUT_DIR/iphonesimulator"
  cp \
    "$(slice_output_path "$simulator_slice")" \
    "$OUTPUT_DIR/iphonesimulator/$VESPER_IOS_PLUGIN_DYLIB"
  prepare_plugin_binary "$OUTPUT_DIR/iphonesimulator/$VESPER_IOS_PLUGIN_DYLIB"
fi

unexpected_runtime="$(
  find "$OUTPUT_DIR" -type f \
    \( -name 'libav*.dylib*' -o -name 'libsw*.dylib*' -o -name 'libssl*.dylib*' -o -name 'libcrypto*.dylib*' -o -name 'libxml2*.dylib*' \) \
    -print -quit
)"
if [[ -n "$unexpected_runtime" ]]; then
  echo "iOS $VESPER_IOS_PLUGIN_CRATE must not bundle FFmpeg runtime dylibs:" >&2
  echo "  $unexpected_runtime" >&2
  if [[ "$VESPER_IOS_PLUGIN_USES_FFMPEG" == "1" ]]; then
    echo "Embed the matching VesperFFmpeg component frameworks alongside the plugin instead." >&2
  fi
  exit 1
fi

echo
echo "Built iOS $VESPER_IOS_PLUGIN_CRATE plugin libraries into:"
echo "  $OUTPUT_DIR"
if [[ "$VESPER_IOS_PLUGIN_USES_FFMPEG" == "1" ]]; then
  echo "Using Apple FFmpeg prebuilts:"
  echo "  $FFMPEG_APPLE_DIR"
  echo "FFmpeg profile:"
  echo "  $VESPER_FFMPEG_PROFILE"
  echo "Selected slices:"
  printf '  %s\n' "${selected_slices[@]}"
  echo "This dylib is an intermediate build input; package it as $VESPER_IOS_PLUGIN_FRAMEWORK.framework for app distribution."
fi
