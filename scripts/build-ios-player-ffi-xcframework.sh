#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PROJECT_DIR="$ROOT_DIR/lib/ios/VesperPlayerKit"
OUTPUT_DIR="$PROJECT_DIR/.build/rust-player-ffi"
XCFRAMEWORK_PATH="$OUTPUT_DIR/VesperPlayerFFI.xcframework"
HEADERS_DIR="$PROJECT_DIR/Sources/VesperPlayerFFIResolver/include"
PROFILE="${1:-release}"

if [[ "$PROFILE" != "debug" && "$PROFILE" != "release" ]]; then
  echo "Unsupported profile: $PROFILE" >&2
  echo "Usage: $0 [debug|release]" >&2
  exit 1
fi

PROFILE_DIR="$PROFILE"
BUILD_FLAGS=()
if [[ "$PROFILE" == "release" ]]; then
  BUILD_FLAGS+=(--release)
fi

DEVICE_TARGET="aarch64-apple-ios"
SIMULATOR_TARGETS=(
  "aarch64-apple-ios-sim"
)

installed_targets="$(rustup target list --installed)"
if [[ "$installed_targets" == *"x86_64-apple-ios"* ]]; then
  SIMULATOR_TARGETS+=("x86_64-apple-ios")
fi

require_target() {
  local target="$1"
  if [[ "$installed_targets" != *"$target"* ]]; then
    echo "Missing Rust Apple target: $target" >&2
    echo "Install it with: rustup target add $target" >&2
    exit 1
  fi
}

require_target "$DEVICE_TARGET"
for target in "${SIMULATOR_TARGETS[@]}"; do
  require_target "$target"
done

build_target() {
  local target="$1"
  local build_command=(cargo build --target "$target" -p player-ffi-resolver)
  if [[ "$PROFILE" == "release" ]]; then
    build_command+=(--release)
  fi
  "${build_command[@]}"
}

copy_built_library() {
  local source_path="$1"
  local destination_path="$2"
  mkdir -p "$(dirname "$destination_path")"
  cp "$source_path" "$destination_path"
}

strip_static_archive_if_needed() {
  local archive_path="$1"

  if [[ "$PROFILE" != "release" ]]; then
    return 0
  fi

  xcrun strip -S -x "$archive_path"
}

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

build_target "$DEVICE_TARGET"
copy_built_library \
  "$ROOT_DIR/target/$DEVICE_TARGET/$PROFILE_DIR/libplayer_ffi_resolver.a" \
  "$OUTPUT_DIR/iphoneos/libplayer_ffi_resolver.a"
strip_static_archive_if_needed "$OUTPUT_DIR/iphoneos/libplayer_ffi_resolver.a"

simulator_archives=()
for target in "${SIMULATOR_TARGETS[@]}"; do
  build_target "$target"

  simulator_output_dir="$OUTPUT_DIR/$target"
  simulator_output_path="$simulator_output_dir/libplayer_ffi_resolver.a"
  copy_built_library \
    "$ROOT_DIR/target/$target/$PROFILE_DIR/libplayer_ffi_resolver.a" \
    "$simulator_output_path"
  strip_static_archive_if_needed "$simulator_output_path"
  simulator_archives+=("$simulator_output_path")
done

mkdir -p "$OUTPUT_DIR/iphonesimulator"
if [[ ${#simulator_archives[@]} -eq 1 ]]; then
  cp "${simulator_archives[0]}" "$OUTPUT_DIR/iphonesimulator/libplayer_ffi_resolver.a"
else
  lipo -create "${simulator_archives[@]}" \
    -output "$OUTPUT_DIR/iphonesimulator/libplayer_ffi_resolver.a"
fi
strip_static_archive_if_needed "$OUTPUT_DIR/iphonesimulator/libplayer_ffi_resolver.a"

rm -rf "$XCFRAMEWORK_PATH"
xcodebuild -create-xcframework \
  -library "$OUTPUT_DIR/iphoneos/libplayer_ffi_resolver.a" \
  -headers "$HEADERS_DIR" \
  -library "$OUTPUT_DIR/iphonesimulator/libplayer_ffi_resolver.a" \
  -headers "$HEADERS_DIR" \
  -output "$XCFRAMEWORK_PATH"

echo
echo "Built player-ffi Apple artifacts into:"
echo "  $OUTPUT_DIR"
