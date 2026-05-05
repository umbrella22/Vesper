#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/apple.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
FFMPEG_APPLE_DIR="${VESPER_APPLE_FFMPEG_OUTPUT_DIR:-$ROOT_DIR/third_party/ffmpeg/apple}"
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
      echo "$OUTPUT_DIR/iphoneos/libplayer_remux_ffmpeg.dylib"
      ;;
    ios-simulator-arm64)
      echo "$OUTPUT_DIR/iphonesimulator/$(vesper_ios_slice_rust_target "$1")/libplayer_remux_ffmpeg.dylib"
      ;;
    *)
      return 1
      ;;
  esac
}

slice_needs_prebuilt() {
  local ffmpeg_dir
  local libdir

  ffmpeg_dir="$(vesper_apple_slice_output_root "$1" "$FFMPEG_APPLE_DIR")"
  libdir="$(vesper_apple_slice_output_libdir "$1")"
  [[ ! -f "$ffmpeg_dir/lib/$libdir/libavcodec.a" ]]
}

ensure_loader_rpath() {
  local binary_path="$1"

  if ! otool -l "$binary_path" | grep -Fq "@loader_path"; then
    install_name_tool -add_rpath "@loader_path" "$binary_path"
  fi
}

prepare_runtime_directory() {
  local directory_path="$1"
  local binary_path

  while IFS= read -r binary_path; do
    ensure_loader_rpath "$binary_path"
  done < <(find "$directory_path" -maxdepth 1 -type f -name 'lib*.dylib*' | sort)
}

prepare_plugin_binary() {
  local binary_path="$1"
  install_name_tool -id "@rpath/libplayer_remux_ffmpeg.dylib" "$binary_path"
  ensure_loader_rpath "$binary_path"
}

selected_slices=()
while IFS= read -r slice; do
  selected_slices+=("$slice")
done < <(vesper_apple_resolve_selected_slices "$@")

required_targets=()
for slice in "${selected_slices[@]}"; do
  required_targets+=("$(vesper_ios_slice_rust_target "$slice")")
done

vesper_apple_require_rust_targets "${required_targets[@]}"

missing_prebuilt_slices=()
for slice in "${selected_slices[@]}"; do
  if slice_needs_prebuilt "$slice"; then
    missing_prebuilt_slices+=("$slice")
  fi
done

if [[ ${#missing_prebuilt_slices[@]} -gt 0 ]]; then
  "$ROOT_DIR/scripts/apple/build-ffmpeg-prebuilts.sh" "${missing_prebuilt_slices[@]}"
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
  ffmpeg_libdir="$(vesper_apple_slice_output_libdir "$slice")"
  output_path="$(slice_output_path "$slice")"
  cargo_target_dir="$ROOT_DIR/target/player-remux-ffmpeg-ios/$(vesper_path_cache_key "$ffmpeg_dir")"
  cargo_command=(
    cargo
    build
    --target "$rust_target"
    -p player-remux-ffmpeg
  )

  if [[ ${#BUILD_FLAGS[@]} -gt 0 ]]; then
    cargo_command+=("${BUILD_FLAGS[@]}")
  fi

  mkdir -p "$(dirname "$output_path")"

  env \
    FFMPEG_DIR="$ffmpeg_dir" \
    CARGO_TARGET_DIR="$cargo_target_dir" \
    "${cargo_command[@]}"

  cp "$cargo_target_dir/$rust_target/$PROFILE_DIR/libplayer_remux_ffmpeg.dylib" "$output_path"
  if compgen -G "$ffmpeg_dir/lib/$ffmpeg_libdir/"'lib*.dylib*' >/dev/null; then
    cp -RP "$ffmpeg_dir"/lib/"$ffmpeg_libdir"/lib*.dylib* "$(dirname "$output_path")/"
  fi
  prepare_runtime_directory "$(dirname "$output_path")"
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
    "$OUTPUT_DIR/iphonesimulator/libplayer_remux_ffmpeg.dylib"
  simulator_ffmpeg_dir="$(vesper_apple_slice_output_root "${simulator_slices[0]}" "$FFMPEG_APPLE_DIR")"
  simulator_ffmpeg_libdir="$(vesper_apple_slice_output_libdir "${simulator_slices[0]}")"
  if compgen -G "$simulator_ffmpeg_dir/lib/$simulator_ffmpeg_libdir/"'lib*.dylib*' >/dev/null; then
    cp -RP \
      "$simulator_ffmpeg_dir"/lib/"$simulator_ffmpeg_libdir"/lib*.dylib* \
      "$OUTPUT_DIR/iphonesimulator/"
  fi
  prepare_runtime_directory "$OUTPUT_DIR/iphonesimulator"
  prepare_plugin_binary "$OUTPUT_DIR/iphonesimulator/libplayer_remux_ffmpeg.dylib"
fi

echo
echo "Built iOS player-remux-ffmpeg plugin libraries into:"
echo "  $OUTPUT_DIR"
echo "Selected slices:"
for slice in "${selected_slices[@]}"; do
  echo "  $slice"
done
