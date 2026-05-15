#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/apple.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg-profile.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg-validate.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROJECT_DIR="$ROOT_DIR/lib/ios/VesperPlayerKit"
OUTPUT_DIR="$ROOT_DIR/dist/release/ios"
BUILD_DIR="$PROJECT_DIR/.build/player-remux-ffmpeg-plugin"
RAW_OUTPUT_DIR="$BUILD_DIR/raw"
FRAMEWORK_STAGING_DIR="$BUILD_DIR/frameworks"
XCFRAMEWORK_PATH="$BUILD_DIR/VesperPlayerRemuxFfmpegPlugin.xcframework"
FRAMEWORK_NAME="VesperPlayerRemuxFfmpegPlugin"
FRAMEWORK_BUNDLE="$FRAMEWORK_NAME.framework"
PROFILE="default"
DRY_RUN=0
SELECTED_SLICES=()

usage() {
  cat <<EOF >&2
Usage: $0 [output-dir] [options] [ios-arm64] [ios-simulator-arm64]

Options:
  --profile <name>   FFmpeg profile name (default: default)
  --dry-run          Print the resolved release inputs without building
EOF
}

if [[ $# -gt 0 && "$1" != --* && "$1" != ios-* ]]; then
  OUTPUT_DIR="$1"
  shift
fi

while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile)
      [[ -n "${2:-}" ]] || { echo "--profile requires a value." >&2; exit 1; }
      PROFILE="$2"
      shift 2
      ;;
    --profile=*)
      PROFILE="${1#*=}"
      shift
      ;;
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
      echo "Unknown iOS remux plugin release option: $1" >&2
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
    echo "iOS remux plugin release requires an ios-arm64 device slice." >&2
    exit 1
    ;;
esac

resolve_ffmpeg_args() {
  local platform="ios"
  local protocols_csv
  local validation_args=()
  local restore_nounset=0

  vesper_ffmpeg_profile_resolve "$PROFILE" "$platform"
  protocols_csv="$(vesper_ffmpeg_profile_join_csv ${VESPER_PROFILE_RESOLVED_PROTOCOLS[@]+"${VESPER_PROFILE_RESOLVED_PROTOCOLS[@]}"})"
  validation_args=(
    "$protocols_csv"
    "$VESPER_PROFILE_RESOLVED_TLS_BACKEND"
    "${VESPER_PROFILE_VALIDATION_FORBID_NETWORK:-false}"
    "${VESPER_PROFILE_VALIDATION_FORBID_OPENSSL:-false}"
  )
  if declare -p VESPER_PROFILE_RESOLVED_EXTRA_CONFIGURE_ARGS >/dev/null 2>&1; then
    if [[ "$-" == *u* ]]; then
      restore_nounset=1
      set +u
    fi
    validation_args+=("${VESPER_PROFILE_RESOLVED_EXTRA_CONFIGURE_ARGS[@]}")
    if [[ "$restore_nounset" == "1" ]]; then
      set -u
    fi
  fi
  vesper_ffmpeg_validate_resolved_profile "${validation_args[@]}"

  vesper_ffmpeg_profile_emit_legacy_args
}

FFMPEG_ARGS=()
while IFS= read -r arg; do
  FFMPEG_ARGS+=("$arg")
done < <(resolve_ffmpeg_args)
vesper_ffmpeg_parse_common_args apple "${FFMPEG_ARGS[@]}"
FFMPEG_APPLE_DIR="${VESPER_APPLE_FFMPEG_OUTPUT_DIR:-${VESPER_FFMPEG_OUTPUT_DIR:-$(vesper_ffmpeg_default_output_dir apple "$ROOT_DIR/third_party/ffmpeg/apple")}}"
vesper_ffmpeg_profile_resolve "$PROFILE" ios

if [[ "$DRY_RUN" == "1" ]]; then
  echo "Resolved iOS remux plugin release:"
  vesper_ffmpeg_profile_print_resolved "$PROFILE" ios
  printf 'profile_hash=%s\n' "$(vesper_ffmpeg_profile_key apple)"
  echo "Selected slices:"
  printf '  %s\n' "${SELECTED_SLICES[@]}"
  echo "Build arguments:"
  printf '  %q\n' "${FFMPEG_ARGS[@]}" "${SELECTED_SLICES[@]}"
  echo "Output zip:"
  echo "  $OUTPUT_DIR/VesperPlayerRemuxFfmpegPlugin.xcframework.zip"
  exit 0
fi

framework_info_plist() {
  local output_path="$1"
  local platform_name="$2"
  local minimum_os_version="$3"

  /usr/libexec/PlistBuddy -c "Clear dict" "$output_path" >/dev/null 2>&1 || true
  /usr/libexec/PlistBuddy -c "Add :CFBundleDevelopmentRegion string en" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleExecutable string $FRAMEWORK_NAME" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleIdentifier string io.github.ikaros.vesper.player.remux-ffmpeg-plugin" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleInfoDictionaryVersion string 6.0" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleName string $FRAMEWORK_NAME" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundlePackageType string FMWK" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleShortVersionString string 0.3.0" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleSupportedPlatforms array" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleSupportedPlatforms:0 string $platform_name" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleVersion string 0.3.0" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :MinimumOSVersion string $minimum_os_version" "$output_path"
}

ensure_rpath() {
  local binary_path="$1"
  local rpath="$2"

  if ! otool -l "$binary_path" | grep -Fq "$rpath"; then
    install_name_tool -add_rpath "$rpath" "$binary_path"
  fi
}

copy_runtime_dylibs() {
  local source_dir="$1"
  local framework_dir="$2"
  local runtime_dir="$framework_dir/Frameworks"

  mkdir -p "$runtime_dir"
  while IFS= read -r runtime_binary; do
    cp -RP "$runtime_binary" "$runtime_dir/"
  done < <(
    find "$source_dir" -maxdepth 1 \( -type f -o -type l \) -name 'lib*.dylib*' \
      ! -name 'libvesper_remux_ffmpeg.dylib' \
      ! -name 'libvesper_remux_ffmpeg.dylib.*' \
      | sort
  )
}

create_framework() {
  local slice="$1"
  local source_dir="$2"
  local platform_name="$3"
  local minimum_os_version="$4"
  local output_dir="$5"
  local framework_dir="$output_dir/$FRAMEWORK_BUNDLE"
  local binary_path="$framework_dir/$FRAMEWORK_NAME"
  local metadata_path

  rm -rf "$framework_dir"
  mkdir -p "$framework_dir/Headers" "$framework_dir/Modules" "$framework_dir/Resources"

  cp "$source_dir/libvesper_remux_ffmpeg.dylib" "$binary_path"
  install_name_tool -id "@rpath/$FRAMEWORK_BUNDLE/$FRAMEWORK_NAME" "$binary_path"
  ensure_rpath "$binary_path" "@loader_path/Frameworks"
  copy_runtime_dylibs "$source_dir" "$framework_dir"

  metadata_path="$(vesper_apple_slice_output_root "$slice" "$FFMPEG_APPLE_DIR")/vesper-ffmpeg-build-metadata.txt"
  if [[ -f "$metadata_path" ]]; then
    cp "$metadata_path" "$framework_dir/Resources/$slice-vesper-ffmpeg-build-metadata.txt"
  fi

  printf '%s\n' \
    'void VesperPlayerRemuxFfmpegPluginLinkAnchor(void);' \
    >"$framework_dir/Headers/VesperPlayerRemuxFfmpegPlugin.h"
  printf '%s\n' \
    'framework module VesperPlayerRemuxFfmpegPlugin {' \
    '  umbrella header "VesperPlayerRemuxFfmpegPlugin.h"' \
    '  export *' \
    '  module * { export * }' \
    '}' \
    >"$framework_dir/Modules/module.modulemap"
  framework_info_plist "$framework_dir/Info.plist" "$platform_name" "$minimum_os_version"
}

vesper_require_command xcodebuild
vesper_require_command install_name_tool
vesper_require_command otool
vesper_require_command lipo

rm -rf "$RAW_OUTPUT_DIR" "$FRAMEWORK_STAGING_DIR" "$XCFRAMEWORK_PATH"
mkdir -p "$OUTPUT_DIR" "$FRAMEWORK_STAGING_DIR"

export VESPER_DECLARED_FFMPEG_PROFILE="$PROFILE"
export VESPER_DECLARED_FFMPEG_PLATFORM="ios"
"$ROOT_DIR/scripts/ios/build-player-remux-ffmpeg-plugin.sh" \
  "$RAW_OUTPUT_DIR" \
  release \
  "${FFMPEG_ARGS[@]}" \
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
      echo "Unsupported iOS remux plugin release slice: $slice" >&2
      exit 1
      ;;
  esac

  if [[ ! -f "$source_dir/libvesper_remux_ffmpeg.dylib" ]]; then
    echo "Missing remux plugin binary for $slice: $source_dir/libvesper_remux_ffmpeg.dylib" >&2
    exit 1
  fi

  slice_framework_root="$FRAMEWORK_STAGING_DIR/$slice"
  create_framework "$slice" "$source_dir" "$platform_name" "$(vesper_apple_ios_deployment_target)" "$slice_framework_root"
  lipo -verify_arch arm64 "$slice_framework_root/$FRAMEWORK_BUNDLE/$FRAMEWORK_NAME"
  FRAMEWORK_ARGS+=(-framework "$slice_framework_root/$FRAMEWORK_BUNDLE")
done

xcodebuild -create-xcframework \
  "${FRAMEWORK_ARGS[@]}" \
  -output "$XCFRAMEWORK_PATH"

ditto -c -k --sequesterRsrc --keepParent \
  "$XCFRAMEWORK_PATH" \
  "$OUTPUT_DIR/VesperPlayerRemuxFfmpegPlugin.xcframework.zip"

echo "Staged optional iOS FFmpeg remux plugin release artifact:"
echo "  $OUTPUT_DIR/VesperPlayerRemuxFfmpegPlugin.xcframework.zip"
