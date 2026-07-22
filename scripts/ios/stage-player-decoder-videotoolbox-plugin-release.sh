#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ios-framework.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROJECT_DIR="$ROOT_DIR/lib/ios/VesperPlayerKit"
OUTPUT_DIR="$ROOT_DIR/dist/release/ios"
BUILD_DIR="$PROJECT_DIR/.build/player-decoder-videotoolbox-plugin"
RAW_OUTPUT_DIR="$BUILD_DIR/raw"
FRAMEWORK_STAGING_DIR="$BUILD_DIR/frameworks"
XCFRAMEWORK_PATH="$BUILD_DIR/VesperPlayerDecoderVideoToolboxPlugin.xcframework"
FRAMEWORK_NAME="VesperPlayerDecoderVideoToolboxPlugin"
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
  echo "Unable to resolve iOS VideoToolbox decoder plugin release version from project metadata." >&2
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
      echo "Unknown iOS VideoToolbox decoder plugin release option: $1" >&2
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
    echo "iOS VideoToolbox decoder plugin release requires an ios-arm64 device slice." >&2
    exit 1
    ;;
esac

if [[ "$DRY_RUN" == "1" ]]; then
  echo "Resolved iOS VideoToolbox decoder plugin release:"
  echo "Selected slices:"
  printf '  %s\n' "${SELECTED_SLICES[@]}"
  echo "Output zip:"
  echo "  $OUTPUT_DIR/VesperPlayerDecoderVideoToolboxPlugin.xcframework.zip"
  exit 0
fi

create_framework() {
  local source_dir="$1"
  local platform_name="$2"
  local minimum_os_version="$3"
  local output_dir="$4"
  local framework_dir="$output_dir/$FRAMEWORK_BUNDLE"
  local binary_path="$framework_dir/$FRAMEWORK_NAME"

  rm -rf "$framework_dir"
  mkdir -p "$framework_dir/Headers" "$framework_dir/Modules"

  cp "$source_dir/libvesper_decoder_videotoolbox.dylib" "$binary_path"
  vesper_ios_prepare_framework_binary "$binary_path" "$FRAMEWORK_NAME"
  vesper_ios_write_binary_framework_module "$framework_dir" "$FRAMEWORK_NAME"
  vesper_ios_framework_info_plist \
    "$framework_dir/Info.plist" \
    "$FRAMEWORK_NAME" \
    "io.github.ikaros.vesper.player.decoder-videotoolbox-plugin" \
    "$platform_name" \
    "$minimum_os_version" \
    "$VESPER_RELEASE_VERSION" \
    "$VESPER_RELEASE_BUILD"
  vesper_ios_verify_flat_framework "$framework_dir" "$FRAMEWORK_NAME"
}

vesper_require_command xcodebuild
vesper_require_command install_name_tool
vesper_require_command otool
vesper_require_command lipo
vesper_require_command plutil
vesper_require_command ditto

rm -rf "$RAW_OUTPUT_DIR" "$FRAMEWORK_STAGING_DIR" "$XCFRAMEWORK_PATH"
mkdir -p "$OUTPUT_DIR" "$FRAMEWORK_STAGING_DIR"

"$ROOT_DIR/scripts/ios/build-player-decoder-videotoolbox-plugin.sh" \
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
      echo "Unsupported iOS VideoToolbox decoder plugin release slice: $slice" >&2
      exit 1
      ;;
  esac

  if [[ ! -f "$source_dir/libvesper_decoder_videotoolbox.dylib" ]]; then
    echo "Missing VideoToolbox decoder plugin binary for $slice: $source_dir/libvesper_decoder_videotoolbox.dylib" >&2
    exit 1
  fi

  slice_framework_root="$FRAMEWORK_STAGING_DIR/$slice"
  create_framework "$source_dir" "$platform_name" "$(vesper_apple_ios_deployment_target)" "$slice_framework_root"
  lipo "$slice_framework_root/$FRAMEWORK_BUNDLE/$FRAMEWORK_NAME" -verify_arch arm64
  FRAMEWORK_ARGS+=(-framework "$slice_framework_root/$FRAMEWORK_BUNDLE")
done

xcodebuild -create-xcframework \
  "${FRAMEWORK_ARGS[@]}" \
  -output "$XCFRAMEWORK_PATH"

ditto -c -k --sequesterRsrc --keepParent \
  "$XCFRAMEWORK_PATH" \
  "$OUTPUT_DIR/VesperPlayerDecoderVideoToolboxPlugin.xcframework.zip"

echo "Staged optional iOS VideoToolbox decoder plugin release artifact:"
echo "  $OUTPUT_DIR/VesperPlayerDecoderVideoToolboxPlugin.xcframework.zip"
