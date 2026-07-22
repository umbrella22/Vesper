#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ios-release.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROJECT_DIR="$ROOT_DIR/lib/ios/VesperPlayerKit"
BUILD_DIR="$PROJECT_DIR/.build/xcframework"
IOS_ARCHIVE="$BUILD_DIR/VesperPlayerKit-iOS.xcarchive"
SIM_ARCHIVE="$BUILD_DIR/VesperPlayerKit-iOS-Simulator.xcarchive"
XCFRAMEWORK_PATH="$BUILD_DIR/VesperPlayerKit.xcframework"
OUTPUT_DIR="${1:-$ROOT_DIR/dist/release/ios}"
FRAMEWORK_NAME="VesperPlayerKit.framework"
BINARY_NAME="VesperPlayerKit"

include_optional_plugins=0
case "${VESPER_IOS_INCLUDE_OPTIONAL_PLUGINS:-0}" in
  1|true|TRUE|yes|YES)
    include_optional_plugins=1
    ;;
esac

mkdir -p "$OUTPUT_DIR"

if [[ "$include_optional_plugins" -eq 0 ]]; then
  vesper_ios_remove_optional_release_assets "$OUTPUT_DIR"
fi

"$ROOT_DIR/scripts/ios/build-vesper-player-kit-xcframework.sh"

DEVICE_FRAMEWORK="$IOS_ARCHIVE/Products/Library/Frameworks/$FRAMEWORK_NAME"
SIMULATOR_FRAMEWORK="$SIM_ARCHIVE/Products/Library/Frameworks/$FRAMEWORK_NAME"

stage_framework_zip() {
  local source_framework="$1"
  local output_zip="$2"
  local extract_arch="${3:-}"
  local temp_dir
  local binary_info

  temp_dir="$(mktemp -d)"
  cp -R "$source_framework" "$temp_dir/$FRAMEWORK_NAME"
  find "$temp_dir/$FRAMEWORK_NAME/Modules" -type f -name '*.swiftmodule' -delete

  if [[ -n "$extract_arch" ]]; then
    binary_info="$(lipo -info "$source_framework/$BINARY_NAME")"
    if [[ "$binary_info" == *"are:"* ]]; then
      lipo "$source_framework/$BINARY_NAME" \
        -extract "$extract_arch" \
        -output "$temp_dir/$FRAMEWORK_NAME/$BINARY_NAME"
    elif [[ "$binary_info" != *"architecture: $extract_arch"* ]]; then
      echo "Expected $extract_arch framework binary, got: $binary_info" >&2
      exit 1
    fi
  fi

  rm -f "$output_zip"
  COPYFILE_DISABLE=1 ditto --norsrc -c -k --keepParent \
    "$temp_dir/$FRAMEWORK_NAME" \
    "$output_zip"

  rm -rf "$temp_dir"
}

stage_framework_zip \
  "$DEVICE_FRAMEWORK" \
  "$OUTPUT_DIR/VesperPlayerKit-ios-arm64.framework.zip"

stage_framework_zip \
  "$SIMULATOR_FRAMEWORK" \
  "$OUTPUT_DIR/VesperPlayerKit-ios-simulator-arm64.framework.zip" \
  "arm64"

rm -f "$OUTPUT_DIR/VesperPlayerKit.xcframework.zip"
COPYFILE_DISABLE=1 ditto --norsrc -c -k --keepParent \
  "$XCFRAMEWORK_PATH" \
  "$OUTPUT_DIR/VesperPlayerKit.xcframework.zip"

if [[ "$include_optional_plugins" -eq 1 ]]; then
  "$ROOT_DIR/scripts/ios/stage-player-optional-plugins-release.sh" \
    "$OUTPUT_DIR" \
    ios-arm64 ios-simulator-arm64
else
  echo "Skipped optional iOS plugin XCFrameworks. Set VESPER_IOS_INCLUDE_OPTIONAL_PLUGINS=1 to stage them."
fi

echo "Staged VesperPlayerKit iOS release assets into:"
echo "  $OUTPUT_DIR"
