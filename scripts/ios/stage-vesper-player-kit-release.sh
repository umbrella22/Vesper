#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROJECT_DIR="$ROOT_DIR/lib/ios/VesperPlayerKit"
BUILD_DIR="$PROJECT_DIR/.build/xcframework"
IOS_ARCHIVE="$BUILD_DIR/VesperPlayerKit-iOS.xcarchive"
SIM_ARCHIVE="$BUILD_DIR/VesperPlayerKit-iOS-Simulator.xcarchive"
ARM64_XCFRAMEWORK_PATH="$BUILD_DIR/VesperPlayerKit-arm64.xcframework"
OUTPUT_DIR="${1:-$ROOT_DIR/dist/release/ios}"
FRAMEWORK_NAME="VesperPlayerKit.framework"
BINARY_NAME="VesperPlayerKit"

mkdir -p "$OUTPUT_DIR"

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

  ditto -c -k --sequesterRsrc --keepParent \
    "$temp_dir/$FRAMEWORK_NAME" \
    "$output_zip"

  rm -rf "$temp_dir"
}

temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir" "$ARM64_XCFRAMEWORK_PATH"' EXIT

ARM64_SIMULATOR_FRAMEWORK="$temp_dir/$FRAMEWORK_NAME"
stage_framework_zip \
  "$SIMULATOR_FRAMEWORK" \
  "$temp_dir/VesperPlayerKit-ios-simulator-arm64.framework.zip" \
  "arm64"
ditto -x -k "$temp_dir/VesperPlayerKit-ios-simulator-arm64.framework.zip" "$temp_dir"

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

"$ROOT_DIR/scripts/ios/stage-player-remux-ffmpeg-plugin-release.sh" \
  "$OUTPUT_DIR" \
  --profile default \
  ios-arm64 ios-simulator-arm64

"$ROOT_DIR/scripts/ios/stage-player-source-normalizer-ffmpeg-plugin-release.sh" \
  "$OUTPUT_DIR" \
  --profile default \
  ios-arm64 ios-simulator-arm64

"$ROOT_DIR/scripts/ios/stage-player-frame-processor-diagnostic-plugin-release.sh" \
  "$OUTPUT_DIR" \
  ios-arm64 ios-simulator-arm64

if [[ ! -f "$OUTPUT_DIR/VesperPlayerFfmpegRuntime.xcframework.zip" ]]; then
  echo "Missing staged iOS FFmpeg runtime artifact:" >&2
  echo "  $OUTPUT_DIR/VesperPlayerFfmpegRuntime.xcframework.zip" >&2
  exit 1
fi

if [[ ! -f "$OUTPUT_DIR/VesperPlayerSourceNormalizerFfmpegPlugin.xcframework.zip" ]]; then
  echo "Missing staged iOS SourceNormalizer artifact:" >&2
  echo "  $OUTPUT_DIR/VesperPlayerSourceNormalizerFfmpegPlugin.xcframework.zip" >&2
  exit 1
fi

if [[ ! -f "$OUTPUT_DIR/VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip" ]]; then
  echo "Missing staged iOS FrameProcessor diagnostic artifact:" >&2
  echo "  $OUTPUT_DIR/VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip" >&2
  exit 1
fi

echo "Staged VesperPlayerKit iOS release assets into:"
echo "  $OUTPUT_DIR"
