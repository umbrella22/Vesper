#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROJECT_DIR="$ROOT_DIR/lib/ios/VesperPlayerKit"
PROJECT_FILE="$PROJECT_DIR/VesperPlayerKit.xcodeproj"
BUILD_DIR="$PROJECT_DIR/.build/xcframework"
IOS_ARCHIVE="$BUILD_DIR/VesperPlayerKit-iOS.xcarchive"
SIM_ARCHIVE="$BUILD_DIR/VesperPlayerKit-iOS-Simulator.xcarchive"
OUTPUT_PATH="$BUILD_DIR/VesperPlayerKit.xcframework"
# Apple iOS binary packaging is arm64-only; do not reintroduce an x86_64
# simulator slice.
SIMULATOR_ARCHS_ENV="${VESPER_IOS_SIMULATOR_ARCHS:-arm64}"
SIMULATOR_ARCHS=()
SIMULATOR_BUILD_ARCHIVES=()

if ! command -v xcodegen >/dev/null 2>&1; then
  echo "xcodegen is required to generate the VesperPlayerKit framework project." >&2
  echo "Install it with: brew install xcodegen" >&2
  exit 1
fi

resolve_simulator_archs() {
  local token
  local -a normalized=()
  local seen=" "

  read -r -a normalized <<<"${SIMULATOR_ARCHS_ENV//,/ }"

  for token in "${normalized[@]}"; do
    if [[ -z "$token" ]]; then
      continue
    fi

    case "$token" in
      arm64)
        ;;
      *)
        echo "Unsupported iOS simulator architecture: $token" >&2
        echo "Supported values: arm64" >&2
        exit 1
        ;;
    esac

    if [[ "$seen" == *" $token "* ]]; then
      continue
    fi

    SIMULATOR_ARCHS+=("$token")
    seen+="$token "
  done

  if [[ ${#SIMULATOR_ARCHS[@]} -eq 0 ]]; then
    echo "No iOS simulator architectures were selected." >&2
    exit 1
  fi
}

build_archive() {
  local sdk="$1"
  local archive_path="$2"
  shift 2

  xcodebuild archive \
    -project "$PROJECT_FILE" \
    -scheme VesperPlayerKit \
    -sdk "$sdk" \
    -archivePath "$archive_path" \
    SKIP_INSTALL=NO \
    BUILD_LIBRARY_FOR_DISTRIBUTION=YES \
    CODE_SIGNING_ALLOWED=NO \
    CODE_SIGNING_REQUIRED=NO \
    SUPPORTS_MACCATALYST=NO \
    "$@"
}

merge_simulator_archives() {
  local merged_archive="$1"
  shift
  local source_archives=("$@")
  local base_framework
  local merged_framework
  local merged_modules_dir
  local archive_path
  local module_dir
  local -a binary_inputs=()

  rm -rf "$merged_archive"

  if [[ ${#source_archives[@]} -eq 1 ]]; then
    ditto "${source_archives[0]}" "$merged_archive"
    return 0
  fi

  base_framework="${source_archives[0]}/Products/Library/Frameworks/VesperPlayerKit.framework"
  merged_framework="$merged_archive/Products/Library/Frameworks/VesperPlayerKit.framework"
  merged_modules_dir="$merged_framework/Modules/VesperPlayerKit.swiftmodule"

  mkdir -p "$(dirname "$merged_framework")"
  ditto "$base_framework" "$merged_framework"

  for archive_path in "${source_archives[@]}"; do
    binary_inputs+=("$archive_path/Products/Library/Frameworks/VesperPlayerKit.framework/VesperPlayerKit")
  done

  lipo -create "${binary_inputs[@]}" -output "$merged_framework/VesperPlayerKit"

  mkdir -p "$merged_modules_dir"
  for archive_path in "${source_archives[@]}"; do
    module_dir="$archive_path/Products/Library/Frameworks/VesperPlayerKit.framework/Modules/VesperPlayerKit.swiftmodule"
    if [[ -d "$module_dir" ]]; then
      find "$module_dir" -maxdepth 1 -type f -exec cp {} "$merged_modules_dir/" \;
    fi
  done
}

mkdir -p "$BUILD_DIR"

"$ROOT_DIR/scripts/ios/build-player-ffi-xcframework.sh"

(cd "$PROJECT_DIR" && xcodegen generate)

resolve_simulator_archs

rm -rf "$IOS_ARCHIVE" "$SIM_ARCHIVE" "$OUTPUT_PATH"

for arch in "${SIMULATOR_ARCHS[@]}"; do
  SIMULATOR_BUILD_ARCHIVES+=("$BUILD_DIR/VesperPlayerKit-iOS-Simulator-$arch.xcarchive")
done

rm -rf "${SIMULATOR_BUILD_ARCHIVES[@]}"

build_archive \
  iphoneos \
  "$IOS_ARCHIVE"

for index in "${!SIMULATOR_ARCHS[@]}"; do
  arch="${SIMULATOR_ARCHS[$index]}"
  archive_path="${SIMULATOR_BUILD_ARCHIVES[$index]}"

  build_archive \
    iphonesimulator \
    "$archive_path" \
    ARCHS="$arch" \
    ONLY_ACTIVE_ARCH=YES
done

merge_simulator_archives "$SIM_ARCHIVE" "${SIMULATOR_BUILD_ARCHIVES[@]}"

xcodebuild -create-xcframework \
  -framework "$IOS_ARCHIVE/Products/Library/Frameworks/VesperPlayerKit.framework" \
  -framework "$SIM_ARCHIVE/Products/Library/Frameworks/VesperPlayerKit.framework" \
  -output "$OUTPUT_PATH"

echo
echo "Built VesperPlayerKit XCFramework at:"
echo "  $OUTPUT_PATH"
