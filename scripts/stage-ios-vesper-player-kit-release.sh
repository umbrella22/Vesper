#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PROJECT_DIR="$ROOT_DIR/lib/ios/VesperPlayerKit"
BUILD_DIR="$PROJECT_DIR/.build/xcframework"
IOS_ARCHIVE="$BUILD_DIR/VesperPlayerKit-iOS.xcarchive"
SIM_ARCHIVE="$BUILD_DIR/VesperPlayerKit-iOS-Simulator.xcarchive"
ARM64_XCFRAMEWORK_PATH="$BUILD_DIR/VesperPlayerKit-arm64.xcframework"
OUTPUT_DIR="${1:-$ROOT_DIR/dist/release/ios}"
FRAMEWORK_NAME="VesperPlayerKit.framework"
BINARY_NAME="VesperPlayerKit"

mkdir -p "$OUTPUT_DIR"

"$ROOT_DIR/scripts/build-ios-vesper-player-kit-xcframework.sh"

DEVICE_FRAMEWORK="$IOS_ARCHIVE/Products/Library/Frameworks/$FRAMEWORK_NAME"
SIMULATOR_FRAMEWORK="$SIM_ARCHIVE/Products/Library/Frameworks/$FRAMEWORK_NAME"

stage_framework_zip() {
  local source_framework="$1"
  local output_zip="$2"
  local extract_arch="${3:-}"
  local temp_dir

  temp_dir="$(mktemp -d)"
  cp -R "$source_framework" "$temp_dir/$FRAMEWORK_NAME"

  if [[ -n "$extract_arch" ]]; then
    lipo "$source_framework/$BINARY_NAME" \
      -extract "$extract_arch" \
      -output "$temp_dir/$FRAMEWORK_NAME/$BINARY_NAME"
  fi

  ditto -c -k --sequesterRsrc --keepParent \
    "$temp_dir/$FRAMEWORK_NAME" \
    "$output_zip"

  rm -rf "$temp_dir"
}

temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir" "$ARM64_XCFRAMEWORK_PATH"' EXIT

ARM64_SIMULATOR_FRAMEWORK="$temp_dir/$FRAMEWORK_NAME"
cp -R "$SIMULATOR_FRAMEWORK" "$ARM64_SIMULATOR_FRAMEWORK"
lipo "$SIMULATOR_FRAMEWORK/$BINARY_NAME" \
  -extract arm64 \
  -output "$ARM64_SIMULATOR_FRAMEWORK/$BINARY_NAME"

rm -rf "$ARM64_XCFRAMEWORK_PATH"
xcodebuild -create-xcframework \
  -framework "$DEVICE_FRAMEWORK" \
  -framework "$ARM64_SIMULATOR_FRAMEWORK" \
  -output "$ARM64_XCFRAMEWORK_PATH"

stage_framework_zip \
  "$DEVICE_FRAMEWORK" \
  "$OUTPUT_DIR/VesperPlayerKit-ios-arm64.framework.zip"

stage_framework_zip \
  "$SIMULATOR_FRAMEWORK" \
  "$OUTPUT_DIR/VesperPlayerKit-ios-simulator-arm64.framework.zip" \
  "arm64"

ditto -c -k --sequesterRsrc --keepParent \
  "$ARM64_XCFRAMEWORK_PATH" \
  "$OUTPUT_DIR/VesperPlayerKit.xcframework.zip"

echo "Staged VesperPlayerKit iOS release assets into:"
echo "  $OUTPUT_DIR"
