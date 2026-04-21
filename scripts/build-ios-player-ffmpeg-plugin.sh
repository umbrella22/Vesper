#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
FFMPEG_APPLE_DIR="${VESPER_APPLE_FFMPEG_OUTPUT_DIR:-$ROOT_DIR/third_party/ffmpeg/apple}"
OUTPUT_DIR="${1:-}"

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

DEFAULT_SLICES=(
  "ios-arm64"
  "ios-simulator-arm64"
)
# Apple 侧 player-ffmpeg plugin 分发统一收敛为 arm64-only。

resolve_selected_slices() {
  local -a resolved=()
  local token

  if [[ $# -gt 0 ]]; then
    resolved=("$@")
  else
    resolved=("${DEFAULT_SLICES[@]}")
  fi

  if [[ ${#resolved[@]} -eq 0 ]]; then
    echo "No iOS player-ffmpeg slices were selected." >&2
    exit 1
  fi

  for token in "${resolved[@]}"; do
    case "$token" in
      ios-arm64|ios-simulator-arm64)
        ;;
      *)
        echo "Unsupported iOS player-ffmpeg slice: $token" >&2
        echo "Supported slices: ios-arm64, ios-simulator-arm64" >&2
        exit 1
        ;;
    esac
  done

  printf '%s\n' "${resolved[@]}"
}

slice_rust_target() {
  case "$1" in
    ios-arm64)
      echo "aarch64-apple-ios"
      ;;
    ios-simulator-arm64)
      echo "aarch64-apple-ios-sim"
      ;;
    *)
      return 1
      ;;
  esac
}

slice_prebuilt_root() {
  case "$1" in
    ios-arm64)
      echo "$FFMPEG_APPLE_DIR/ios"
      ;;
    ios-simulator-arm64)
      echo "$FFMPEG_APPLE_DIR/ios-simulator"
      ;;
    *)
      return 1
      ;;
  esac
}

slice_output_path() {
  case "$1" in
    ios-arm64)
      echo "$OUTPUT_DIR/iphoneos/libplayer_ffmpeg.dylib"
      ;;
    ios-simulator-arm64)
      echo "$OUTPUT_DIR/iphonesimulator/$(slice_rust_target "$1")/libplayer_ffmpeg.dylib"
      ;;
    *)
      return 1
      ;;
  esac
}

slice_prebuilt_libdir() {
  case "$1" in
    ios-arm64|ios-simulator-arm64)
      echo "arm64"
      ;;
    *)
      return 1
      ;;
  esac
}

path_cache_key() {
  local path="$1"
  local sanitized="${path#/}"

  sanitized="${sanitized//\//_}"
  sanitized="${sanitized//:/_}"
  sanitized="${sanitized// /_}"

  printf '%s\n' "$sanitized"
}

slice_needs_prebuilt() {
  case "$1" in
    ios-arm64)
      [[ ! -f "$FFMPEG_APPLE_DIR/ios/lib/arm64/libavcodec.a" ]]
      ;;
    ios-simulator-arm64)
      [[ ! -f "$FFMPEG_APPLE_DIR/ios-simulator/lib/arm64/libavcodec.a" ]]
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

prepare_runtime_directory() {
  local directory_path="$1"
  local binary_path

  while IFS= read -r binary_path; do
    ensure_loader_rpath "$binary_path"
  done < <(find "$directory_path" -maxdepth 1 -type f -name 'lib*.dylib*' | sort)
}

prepare_plugin_binary() {
  local binary_path="$1"
  install_name_tool -id "@rpath/libplayer_ffmpeg.dylib" "$binary_path"
  ensure_loader_rpath "$binary_path"
}

selected_slices=()
while IFS= read -r slice; do
  selected_slices+=("$slice")
done < <(resolve_selected_slices "$@")

required_targets=()
for slice in "${selected_slices[@]}"; do
  required_targets+=("$(slice_rust_target "$slice")")
done

installed_targets="$(rustup target list --installed)"
missing_targets=()
for target in "${required_targets[@]}"; do
  if [[ "$installed_targets" != *"$target"* ]]; then
    missing_targets+=("$target")
  fi
done

if [[ ${#missing_targets[@]} -gt 0 ]]; then
  echo "Required Rust Apple targets are missing:" >&2
  for target in "${missing_targets[@]}"; do
    echo "  $target" >&2
  done
  echo >&2
  echo "Install them with:" >&2
  echo "  rustup target add ${missing_targets[*]}" >&2
  exit 1
fi

missing_prebuilt_slices=()
for slice in "${selected_slices[@]}"; do
  if slice_needs_prebuilt "$slice"; then
    missing_prebuilt_slices+=("$slice")
  fi
done

if [[ ${#missing_prebuilt_slices[@]} -gt 0 ]]; then
  "$ROOT_DIR/scripts/build-apple-ffmpeg-prebuilts.sh" "${missing_prebuilt_slices[@]}"
fi

PROFILE_DIR="$PROFILE"
BUILD_FLAGS=()
if [[ "$PROFILE" == "release" ]]; then
  BUILD_FLAGS+=(--release)
fi

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

for slice in "${selected_slices[@]}"; do
  rust_target="$(slice_rust_target "$slice")"
  ffmpeg_dir="$(slice_prebuilt_root "$slice")"
  ffmpeg_libdir="$(slice_prebuilt_libdir "$slice")"
  output_path="$(slice_output_path "$slice")"
  cargo_target_dir="$ROOT_DIR/target/player-ffmpeg-ios/$(path_cache_key "$ffmpeg_dir")"
  cargo_command=(
    cargo
    build
    --target "$rust_target"
    -p player-ffmpeg
  )

  if [[ ${#BUILD_FLAGS[@]} -gt 0 ]]; then
    cargo_command+=("${BUILD_FLAGS[@]}")
  fi

  mkdir -p "$(dirname "$output_path")"

  env \
    FFMPEG_DIR="$ffmpeg_dir" \
    CARGO_TARGET_DIR="$cargo_target_dir" \
    "${cargo_command[@]}"

  cp "$cargo_target_dir/$rust_target/$PROFILE_DIR/libplayer_ffmpeg.dylib" "$output_path"
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
    "$OUTPUT_DIR/iphonesimulator/libplayer_ffmpeg.dylib"
  simulator_ffmpeg_dir="$(slice_prebuilt_root "${simulator_slices[0]}")"
  simulator_ffmpeg_libdir="$(slice_prebuilt_libdir "${simulator_slices[0]}")"
  if compgen -G "$simulator_ffmpeg_dir/lib/$simulator_ffmpeg_libdir/"'lib*.dylib*' >/dev/null; then
    cp -RP \
      "$simulator_ffmpeg_dir"/lib/"$simulator_ffmpeg_libdir"/lib*.dylib* \
      "$OUTPUT_DIR/iphonesimulator/"
  fi
  prepare_runtime_directory "$OUTPUT_DIR/iphonesimulator"
  prepare_plugin_binary "$OUTPUT_DIR/iphonesimulator/libplayer_ffmpeg.dylib"
fi

echo
echo "Built iOS player-ffmpeg plugin libraries into:"
echo "  $OUTPUT_DIR"
echo "Selected slices:"
for slice in "${selected_slices[@]}"; do
  echo "  $slice"
done
