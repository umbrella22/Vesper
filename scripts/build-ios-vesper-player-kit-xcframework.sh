#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PROJECT_DIR="$ROOT_DIR/lib/ios/VesperPlayerKit"
PROJECT_FILE="$PROJECT_DIR/VesperPlayerKit.xcodeproj"
BUILD_DIR="$PROJECT_DIR/.build/xcframework"
IOS_ARCHIVE="$BUILD_DIR/VesperPlayerKit-iOS.xcarchive"
SIM_ARCHIVE="$BUILD_DIR/VesperPlayerKit-iOS-Simulator.xcarchive"
OUTPUT_PATH="$BUILD_DIR/VesperPlayerKit.xcframework"

if ! command -v xcodegen >/dev/null 2>&1; then
  echo "xcodegen is required to generate the VesperPlayerKit framework project." >&2
  echo "Install it with: brew install xcodegen" >&2
  exit 1
fi

mkdir -p "$BUILD_DIR"

"$ROOT_DIR/scripts/build-ios-player-ffi-xcframework.sh"

(cd "$PROJECT_DIR" && xcodegen generate)

rm -rf "$IOS_ARCHIVE" "$SIM_ARCHIVE" "$OUTPUT_PATH"

xcodebuild archive \
  -project "$PROJECT_FILE" \
  -scheme VesperPlayerKit \
  -destination "generic/platform=iOS" \
  -archivePath "$IOS_ARCHIVE" \
  SKIP_INSTALL=NO \
  BUILD_LIBRARY_FOR_DISTRIBUTION=YES

xcodebuild archive \
  -project "$PROJECT_FILE" \
  -scheme VesperPlayerKit \
  -destination "generic/platform=iOS Simulator" \
  -archivePath "$SIM_ARCHIVE" \
  SKIP_INSTALL=NO \
  BUILD_LIBRARY_FOR_DISTRIBUTION=YES

xcodebuild -create-xcframework \
  -framework "$IOS_ARCHIVE/Products/Library/Frameworks/VesperPlayerKit.framework" \
  -framework "$SIM_ARCHIVE/Products/Library/Frameworks/VesperPlayerKit.framework" \
  -output "$OUTPUT_PATH"

echo
echo "Built VesperPlayerKit XCFramework at:"
echo "  $OUTPUT_PATH"
