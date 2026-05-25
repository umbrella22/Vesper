#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/apple.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROJECT_DIR="$ROOT_DIR/lib/ios/VesperPlayerKit"
OUTPUT_DIR="$ROOT_DIR/dist/release/ios"
BUILD_DIR="$PROJECT_DIR/.build/player-frame-processor-diagnostic-plugin"
RAW_OUTPUT_DIR="$BUILD_DIR/raw"
FRAMEWORK_STAGING_DIR="$BUILD_DIR/frameworks"
XCFRAMEWORK_PATH="$BUILD_DIR/VesperPlayerFrameProcessorDiagnosticPlugin.xcframework"
FRAMEWORK_NAME="VesperPlayerFrameProcessorDiagnosticPlugin"
FRAMEWORK_BUNDLE="$FRAMEWORK_NAME.framework"
DRY_RUN=0
SELECTED_SLICES=()

read_project_version() {
  sed -n 's/^[[:space:]]*CFBundleShortVersionString: "\([^"]*\)".*/\1/p' "$PROJECT_DIR/project.yml" \
    | head -n 1
}

read_project_build() {
  sed -n 's/^[[:space:]]*CFBundleVersion: "\([0-9][0-9]*\)".*/\1/p' "$PROJECT_DIR/project.yml" \
    | head -n 1
}

VESPER_RELEASE_VERSION="${VESPER_RELEASE_VERSION:-$(read_project_version)}"
VESPER_RELEASE_BUILD="${VESPER_RELEASE_BUILD:-${VESPER_RELEASE_IOS_BUILD:-$(read_project_build)}}"

if [[ -z "$VESPER_RELEASE_VERSION" || -z "$VESPER_RELEASE_BUILD" ]]; then
  echo "Unable to resolve iOS frame processor plugin release version from project metadata." >&2
  exit 1
fi

usage() {
  cat <<EOF >&2
Usage: $0 [output-dir] [options] [ios-arm64] [ios-simulator-arm64]

Options:
  --dry-run          Print the resolved release inputs without building
EOF
}

if [[ $# -gt 0 && "$1" != --* && "$1" != ios-* ]]; then
  OUTPUT_DIR="$1"
  shift
fi

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    ios-*)
      SELECTED_SLICES+=("$1")
      shift
      ;;
    *)
      echo "Unknown iOS frame processor plugin release option: $1" >&2
      usage
      exit 1
      ;;
  esac
done

if [[ ${#SELECTED_SLICES[@]} -eq 0 ]]; then
  SELECTED_SLICES=(ios-arm64 ios-simulator-arm64)
fi

case " ${SELECTED_SLICES[*]} " in
  *" ios-arm64 "*)
    ;;
  *)
    echo "iOS frame processor plugin release requires an ios-arm64 device slice." >&2
    exit 1
    ;;
esac

if [[ "$DRY_RUN" == "1" ]]; then
  echo "Resolved iOS frame processor diagnostic plugin release:"
  echo "Selected slices:"
  printf '  %s\n' "${SELECTED_SLICES[@]}"
  echo "Output zip:"
  echo "  $OUTPUT_DIR/VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip"
  exit 0
fi

framework_info_plist() {
  local output_path="$1"
  local platform_name="$2"
  local minimum_os_version="$3"

  /usr/libexec/PlistBuddy -c "Clear dict" "$output_path" >/dev/null 2>&1 || true
  /usr/libexec/PlistBuddy -c "Add :CFBundleDevelopmentRegion string en" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleExecutable string $FRAMEWORK_NAME" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleIdentifier string io.github.ikaros.vesper.player.frame-processor-diagnostic-plugin" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleInfoDictionaryVersion string 6.0" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleName string $FRAMEWORK_NAME" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundlePackageType string FMWK" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleShortVersionString string $VESPER_RELEASE_VERSION" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleSupportedPlatforms array" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleSupportedPlatforms:0 string $platform_name" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleVersion string $VESPER_RELEASE_BUILD" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :MinimumOSVersion string $minimum_os_version" "$output_path"
}

create_framework() {
  local source_dir="$1"
  local platform_name="$2"
  local minimum_os_version="$3"
  local output_dir="$4"
  local framework_dir="$output_dir/$FRAMEWORK_BUNDLE"
  local binary_path="$framework_dir/$FRAMEWORK_NAME"

  rm -rf "$framework_dir"
  mkdir -p "$framework_dir/Headers" "$framework_dir/Modules" "$framework_dir/Resources"

  cp "$source_dir/libplayer_frame_processor_diagnostic.dylib" "$binary_path"
  install_name_tool -id "@rpath/$FRAMEWORK_BUNDLE/$FRAMEWORK_NAME" "$binary_path"

  printf '%s\n' \
    'void VesperPlayerFrameProcessorDiagnosticPluginLinkAnchor(void);' \
    >"$framework_dir/Headers/VesperPlayerFrameProcessorDiagnosticPlugin.h"
  printf '%s\n' \
    'framework module VesperPlayerFrameProcessorDiagnosticPlugin {' \
    '  umbrella header "VesperPlayerFrameProcessorDiagnosticPlugin.h"' \
    '  export *' \
    '  module * { export * }' \
    '}' \
    >"$framework_dir/Modules/module.modulemap"
  framework_info_plist "$framework_dir/Info.plist" "$platform_name" "$minimum_os_version"
}

verify_no_runtime_dylibs() {
  local framework_dir="$1"
  local unexpected

  unexpected="$(
    find "$framework_dir" -type f \
      \( -name 'libav*.dylib*' -o -name 'libsw*.dylib*' -o -name 'libxml2*.dylib*' -o -name 'libssl*.dylib*' -o -name 'libcrypto*.dylib*' \) \
      -print -quit
  )"
  if [[ -n "$unexpected" ]]; then
    echo "iOS frame processor diagnostic plugin must not bundle FFmpeg runtime dylibs:" >&2
    echo "  $unexpected" >&2
    exit 1
  fi
}

vesper_require_command xcodebuild
vesper_require_command install_name_tool
vesper_require_command otool
vesper_require_command lipo

rm -rf "$RAW_OUTPUT_DIR" "$FRAMEWORK_STAGING_DIR" "$XCFRAMEWORK_PATH"
mkdir -p "$OUTPUT_DIR" "$FRAMEWORK_STAGING_DIR"

"$ROOT_DIR/scripts/ios/build-player-frame-processor-diagnostic-plugin.sh" \
  "$RAW_OUTPUT_DIR" \
  release \
  "${SELECTED_SLICES[@]}"

FRAMEWORK_ARGS=()
for slice in "${SELECTED_SLICES[@]}"; do
  case "$slice" in
    ios-arm64)
      source_dir="$RAW_OUTPUT_DIR/iphoneos"
      platform_name="iPhoneOS"
      ;;
    ios-simulator-arm64)
      source_dir="$RAW_OUTPUT_DIR/iphonesimulator"
      platform_name="iPhoneSimulator"
      ;;
    *)
      echo "Unsupported iOS frame processor plugin release slice: $slice" >&2
      exit 1
      ;;
  esac

  if [[ ! -f "$source_dir/libplayer_frame_processor_diagnostic.dylib" ]]; then
    echo "Missing frame processor plugin binary for $slice: $source_dir/libplayer_frame_processor_diagnostic.dylib" >&2
    exit 1
  fi

  slice_framework_root="$FRAMEWORK_STAGING_DIR/$slice"
  create_framework "$source_dir" "$platform_name" "$(vesper_apple_ios_deployment_target)" "$slice_framework_root"
  verify_no_runtime_dylibs "$slice_framework_root/$FRAMEWORK_BUNDLE"
  lipo "$slice_framework_root/$FRAMEWORK_BUNDLE/$FRAMEWORK_NAME" -verify_arch arm64
  FRAMEWORK_ARGS+=(-framework "$slice_framework_root/$FRAMEWORK_BUNDLE")
done

xcodebuild -create-xcframework \
  "${FRAMEWORK_ARGS[@]}" \
  -output "$XCFRAMEWORK_PATH"

ditto -c -k --sequesterRsrc --keepParent \
  "$XCFRAMEWORK_PATH" \
  "$OUTPUT_DIR/VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip"

echo "Staged optional iOS frame processor diagnostic plugin release artifact:"
echo "  $OUTPUT_DIR/VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip"
