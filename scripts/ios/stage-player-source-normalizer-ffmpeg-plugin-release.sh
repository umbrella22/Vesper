#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/apple.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg-profile.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg-validate.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROJECT_DIR="$ROOT_DIR/lib/ios/VesperPlayerKit"
OUTPUT_DIR="$ROOT_DIR/dist/release/ios"
BUILD_DIR="$PROJECT_DIR/.build/player-source-normalizer-ffmpeg-plugin"
RAW_OUTPUT_DIR="$BUILD_DIR/raw"
FRAMEWORK_STAGING_DIR="$BUILD_DIR/frameworks"
XCFRAMEWORK_PATH="$BUILD_DIR/VesperPlayerSourceNormalizerFfmpegPlugin.xcframework"
RUNTIME_BUILD_DIR="$PROJECT_DIR/.build/player-ffmpeg-runtime"
RUNTIME_XCFRAMEWORK_PATH="$RUNTIME_BUILD_DIR/VesperPlayerFfmpegRuntime.xcframework"
FRAMEWORK_NAME="VesperPlayerSourceNormalizerFfmpegPlugin"
FRAMEWORK_BUNDLE="$FRAMEWORK_NAME.framework"
PROFILE="source-normalizer"
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
  echo "Unable to resolve iOS source normalizer plugin release version from project metadata." >&2
  exit 1
fi

usage() {
  cat <<EOF >&2
Usage: $0 [output-dir] [options] [ios-arm64] [ios-simulator-arm64]

Options:
  --profile <name>   FFmpeg profile name (default: source-normalizer)
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
      echo "Unknown iOS source normalizer plugin release option: $1" >&2
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
    echo "iOS source normalizer plugin release requires an ios-arm64 device slice." >&2
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
PROFILE_HASH="$(vesper_ffmpeg_profile_key apple)"

if [[ "$DRY_RUN" == "1" ]]; then
  echo "Resolved iOS source normalizer plugin release:"
  vesper_ffmpeg_profile_print_resolved "$PROFILE" ios
  printf 'profile_hash=%s\n' "$PROFILE_HASH"
  echo "Selected slices:"
  printf '  %s\n' "${SELECTED_SLICES[@]}"
  echo "Build arguments:"
  printf '  %q\n' "${FFMPEG_ARGS[@]}" "${SELECTED_SLICES[@]}"
  echo "Runtime zip:"
  echo "  $OUTPUT_DIR/VesperPlayerFfmpegRuntime.xcframework.zip"
  echo "Output zip:"
  echo "  $OUTPUT_DIR/VesperPlayerSourceNormalizerFfmpegPlugin.xcframework.zip"
  exit 0
fi

framework_info_plist() {
  local output_path="$1"
  local platform_name="$2"
  local minimum_os_version="$3"

  /usr/libexec/PlistBuddy -c "Clear dict" "$output_path" >/dev/null 2>&1 || true
  /usr/libexec/PlistBuddy -c "Add :CFBundleDevelopmentRegion string en" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleExecutable string $FRAMEWORK_NAME" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleIdentifier string io.github.ikaros.vesper.player.source-normalizer-ffmpeg-plugin" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleInfoDictionaryVersion string 6.0" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleName string $FRAMEWORK_NAME" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundlePackageType string FMWK" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleShortVersionString string $VESPER_RELEASE_VERSION" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleSupportedPlatforms array" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleSupportedPlatforms:0 string $platform_name" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleVersion string $VESPER_RELEASE_BUILD" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :MinimumOSVersion string $minimum_os_version" "$output_path"
}

ensure_rpath() {
  local binary_path="$1"
  local rpath="$2"

  if ! otool -l "$binary_path" | grep -Fq "$rpath"; then
    install_name_tool -add_rpath "$rpath" "$binary_path"
  fi
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

  cp "$source_dir/libplayer_source_normalizer_ffmpeg.dylib" "$binary_path"
  install_name_tool -id "@rpath/$FRAMEWORK_BUNDLE/$FRAMEWORK_NAME" "$binary_path"
  ensure_rpath "$binary_path" "@loader_path/../VesperPlayerFfmpegRuntime.framework/Frameworks"
  ensure_rpath "$binary_path" "@loader_path/Frameworks"

  metadata_path="$(vesper_apple_slice_output_root "$slice" "$FFMPEG_APPLE_DIR")/vesper-ffmpeg-build-metadata.txt"
  if [[ ! -f "$metadata_path" ]]; then
    echo "Missing FFmpeg build metadata for $slice: $metadata_path" >&2
    exit 1
  fi
  cp "$metadata_path" "$framework_dir/Resources/$slice-vesper-ffmpeg-build-metadata.txt"
  printf '%s\n' "$PROFILE_HASH" >"$framework_dir/Resources/profile-hash.txt"

  printf '%s\n' \
    'void VesperPlayerSourceNormalizerFfmpegPluginLinkAnchor(void);' \
    >"$framework_dir/Headers/VesperPlayerSourceNormalizerFfmpegPlugin.h"
  printf '%s\n' \
    'framework module VesperPlayerSourceNormalizerFfmpegPlugin {' \
    '  umbrella header "VesperPlayerSourceNormalizerFfmpegPlugin.h"' \
    '  export *' \
    '  module * { export * }' \
    '}' \
    >"$framework_dir/Modules/module.modulemap"
  framework_info_plist "$framework_dir/Info.plist" "$platform_name" "$minimum_os_version"
}

runtime_profile_hash_for_slice() {
  local slice="$1"
  local framework_path
  local metadata_path
  local profile_path

  metadata_path="$(find "$RUNTIME_XCFRAMEWORK_PATH" -path "*/VesperPlayerFfmpegRuntime.framework/Resources/$slice-vesper-ffmpeg-build-metadata.txt" -type f | head -n 1 || true)"
  if [[ -z "$metadata_path" ]]; then
    echo "Unable to find $slice FFmpeg runtime metadata inside $RUNTIME_XCFRAMEWORK_PATH" >&2
    exit 1
  fi
  framework_path="${metadata_path%/Resources/$slice-vesper-ffmpeg-build-metadata.txt}"
  profile_path="$framework_path/Resources/profile-hash.txt"
  if [[ ! -f "$profile_path" ]]; then
    echo "Missing iOS FFmpeg runtime profile hash: $profile_path" >&2
    exit 1
  fi
  cat "$profile_path"
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
    echo "iOS source normalizer plugin framework must not bundle FFmpeg runtime dylibs:" >&2
    echo "  $unexpected" >&2
    echo "Embed VesperPlayerFfmpegRuntime.xcframework.zip alongside the plugin instead." >&2
    exit 1
  fi
}

vesper_require_command xcodebuild
vesper_require_command install_name_tool
vesper_require_command otool
vesper_require_command lipo

rm -rf "$RAW_OUTPUT_DIR" "$FRAMEWORK_STAGING_DIR" "$XCFRAMEWORK_PATH"
mkdir -p "$OUTPUT_DIR" "$FRAMEWORK_STAGING_DIR"

"$ROOT_DIR/scripts/ios/stage-player-ffmpeg-runtime-release.sh" \
  "$OUTPUT_DIR" \
  --profile "$PROFILE" \
  "${SELECTED_SLICES[@]}"

export VESPER_DECLARED_FFMPEG_PROFILE="$PROFILE"
export VESPER_DECLARED_FFMPEG_PLATFORM="ios"
env \
  VESPER_SKIP_APPLE_FFMPEG_PREBUILDS=1 \
  "$ROOT_DIR/scripts/ios/build-player-source-normalizer-ffmpeg-plugin.sh" \
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
      echo "Unsupported iOS source normalizer plugin release slice: $slice" >&2
      exit 1
      ;;
  esac

  if [[ ! -f "$source_dir/libplayer_source_normalizer_ffmpeg.dylib" ]]; then
    echo "Missing source normalizer plugin binary for $slice: $source_dir/libplayer_source_normalizer_ffmpeg.dylib" >&2
    exit 1
  fi

  slice_framework_root="$FRAMEWORK_STAGING_DIR/$slice"
  create_framework "$slice" "$source_dir" "$platform_name" "$(vesper_apple_ios_deployment_target)" "$slice_framework_root"
  runtime_hash="$(runtime_profile_hash_for_slice "$slice")"
  if [[ "$runtime_hash" != "$PROFILE_HASH" ]]; then
    echo "iOS FFmpeg runtime profile hash mismatch for $slice:" >&2
    echo "  runtime: $runtime_hash" >&2
    echo "  plugin:  $PROFILE_HASH" >&2
    exit 1
  fi
  verify_no_runtime_dylibs "$slice_framework_root/$FRAMEWORK_BUNDLE"
  lipo "$slice_framework_root/$FRAMEWORK_BUNDLE/$FRAMEWORK_NAME" -verify_arch arm64
  FRAMEWORK_ARGS+=(-framework "$slice_framework_root/$FRAMEWORK_BUNDLE")
done

xcodebuild -create-xcframework \
  "${FRAMEWORK_ARGS[@]}" \
  -output "$XCFRAMEWORK_PATH"

ditto -c -k --sequesterRsrc --keepParent \
  "$XCFRAMEWORK_PATH" \
  "$OUTPUT_DIR/VesperPlayerSourceNormalizerFfmpegPlugin.xcframework.zip"

echo "Staged optional iOS FFmpeg source normalizer plugin release artifact:"
echo "  $OUTPUT_DIR/VesperPlayerSourceNormalizerFfmpegPlugin.xcframework.zip"
echo "Requires shared iOS FFmpeg runtime artifact:"
echo "  $OUTPUT_DIR/VesperPlayerFfmpegRuntime.xcframework.zip"
