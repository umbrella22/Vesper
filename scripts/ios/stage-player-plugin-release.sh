#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ios-framework.sh"

usage() {
  cat <<EOF >&2
Usage: $0 <plugin-id> [output-dir] [options] [ios-arm64] [ios-simulator-arm64]

Options:
  --profile <name>   FFmpeg profile name (FFmpeg plugins only)
  --dry-run          Print the resolved release inputs without building

Supported plugin IDs: ${VESPER_IOS_OPTIONAL_PLUGIN_IDS[*]}
EOF
}

PLUGIN_ID="${1:-}"
if ! vesper_ios_resolve_plugin "$PLUGIN_ID"; then
  usage
  exit 1
fi
shift

ROOT_DIR="$VESPER_REPO_ROOT"
PROJECT_DIR="$ROOT_DIR/lib/ios/VesperPlayerKit"
OUTPUT_DIR="$ROOT_DIR/dist/release/ios"
BUILD_DIR="$PROJECT_DIR/.build/$VESPER_IOS_PLUGIN_BUILD_DIRECTORY"
RAW_OUTPUT_DIR="$BUILD_DIR/raw"
FRAMEWORK_STAGING_DIR="$BUILD_DIR/frameworks"
XCFRAMEWORK_PATH="$BUILD_DIR/$VESPER_IOS_PLUGIN_FRAMEWORK.xcframework"
RUNTIME_BUILD_DIR="$PROJECT_DIR/.build/player-ffmpeg-runtime"
FRAMEWORK_NAME="$VESPER_IOS_PLUGIN_FRAMEWORK"
FRAMEWORK_BUNDLE="$FRAMEWORK_NAME.framework"
PROFILE="$VESPER_IOS_PLUGIN_DEFAULT_FFMPEG_PROFILE"
DRY_RUN=0
REQUESTED_SLICES=()

if [[ "$VESPER_IOS_PLUGIN_USES_FFMPEG" == "1" ]]; then
  source "$ROOT_DIR/scripts/lib/ffmpeg.sh"
  source "$ROOT_DIR/scripts/lib/ffmpeg-profile.sh"
  source "$ROOT_DIR/scripts/lib/ffmpeg-validate.sh"
fi

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
  echo "Unable to resolve iOS $VESPER_IOS_PLUGIN_DESCRIPTION release version from project metadata." >&2
  exit 1
fi

if [[ $# -gt 0 && "$1" != --* && "$1" != ios-* ]]; then
  OUTPUT_DIR="$1"
  shift
fi

while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile)
      if [[ "$VESPER_IOS_PLUGIN_USES_FFMPEG" != "1" ]]; then
        echo "--profile is only supported for FFmpeg-backed iOS plugins." >&2
        exit 1
      fi
      [[ -n "${2:-}" ]] || { echo "--profile requires a value." >&2; exit 1; }
      PROFILE="$2"
      shift 2
      ;;
    --profile=*)
      if [[ "$VESPER_IOS_PLUGIN_USES_FFMPEG" != "1" ]]; then
        echo "--profile is only supported for FFmpeg-backed iOS plugins." >&2
        exit 1
      fi
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
      REQUESTED_SLICES+=("$1")
      shift
      ;;
    *)
      echo "Unknown iOS $VESPER_IOS_PLUGIN_DESCRIPTION release option: $1" >&2
      usage
      exit 1
      ;;
  esac
done

if ! resolved_slices="$(vesper_apple_resolve_selected_slices ${REQUESTED_SLICES[@]+"${REQUESTED_SLICES[@]}"})"; then
  exit 1
fi
SELECTED_SLICES=()
while IFS= read -r slice; do
  [[ -n "$slice" ]] && SELECTED_SLICES+=("$slice")
done <<<"$resolved_slices"

case " ${SELECTED_SLICES[*]} " in
  *" ios-arm64 "*)
    ;;
  *)
    echo "iOS $VESPER_IOS_PLUGIN_DESCRIPTION release requires an ios-arm64 device slice." >&2
    exit 1
    ;;
esac

FFMPEG_ARGS=()
RUNTIME_LIBRARIES=()
PROFILE_HASH=""
FFMPEG_APPLE_DIR=""
if [[ "$VESPER_IOS_PLUGIN_USES_FFMPEG" == "1" ]]; then
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

  if ! ffmpeg_args_output="$(resolve_ffmpeg_args)"; then
    exit 1
  fi
  while IFS= read -r arg; do
    [[ -n "$arg" ]] && FFMPEG_ARGS+=("$arg")
  done <<<"$ffmpeg_args_output"
  vesper_ffmpeg_parse_common_args apple "${FFMPEG_ARGS[@]}"
  FFMPEG_APPLE_DIR="${VESPER_APPLE_FFMPEG_OUTPUT_DIR:-${VESPER_FFMPEG_OUTPUT_DIR:-$(vesper_ffmpeg_default_output_dir apple "$ROOT_DIR/third_party/ffmpeg/apple")}}"
  vesper_ffmpeg_profile_resolve "$PROFILE" ios
  PROFILE_HASH="$(vesper_ffmpeg_profile_key apple)"
  RUNTIME_LIBRARIES=("${VESPER_PROFILE_RESOLVED_LIBRARIES[@]}")
fi

if [[ "$DRY_RUN" == "1" ]]; then
  echo "Resolved iOS $VESPER_IOS_PLUGIN_DESCRIPTION release:"
  if [[ "$VESPER_IOS_PLUGIN_USES_FFMPEG" == "1" ]]; then
    vesper_ffmpeg_profile_print_resolved "$PROFILE" ios
    printf 'profile_hash=%s\n' "$PROFILE_HASH"
  fi
  echo "Selected slices:"
  printf '  %s\n' "${SELECTED_SLICES[@]}"
  if [[ "$VESPER_IOS_PLUGIN_USES_FFMPEG" == "1" ]]; then
    echo "Build arguments:"
    printf '  %q\n' "${FFMPEG_ARGS[@]}" "${SELECTED_SLICES[@]}"
    echo "Required runtime zips:"
    for library_name in "${RUNTIME_LIBRARIES[@]}"; do
      framework_name="$(vesper_ios_ffmpeg_framework_name "$library_name")"
      echo "  $OUTPUT_DIR/$framework_name.xcframework.zip"
    done
  fi
  echo "Output zip:"
  echo "  $OUTPUT_DIR/$FRAMEWORK_NAME.xcframework.zip"
  exit 0
fi

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
  mkdir -p "$framework_dir/Headers" "$framework_dir/Modules"
  cp "$source_dir/$VESPER_IOS_PLUGIN_DYLIB" "$binary_path"
  vesper_ios_prepare_framework_binary "$binary_path" "$FRAMEWORK_NAME"
  if [[ "$VESPER_IOS_PLUGIN_USES_FFMPEG" == "1" ]]; then
    printf '%s\n' "$(vesper_ffmpeg_sha256_file "$binary_path")" >"$framework_dir/binary-sha256.txt"
    metadata_path="$(vesper_apple_slice_output_root "$slice" "$FFMPEG_APPLE_DIR")/vesper-ffmpeg-build-metadata.txt"
    if [[ ! -f "$metadata_path" ]]; then
      echo "Missing FFmpeg build metadata for $slice: $metadata_path" >&2
      exit 1
    fi
    cp "$metadata_path" "$framework_dir/$slice-vesper-ffmpeg-build-metadata.txt"
    printf '%s\n' "$PROFILE_HASH" >"$framework_dir/profile-hash.txt"
  fi
  vesper_ios_write_binary_framework_module "$framework_dir" "$FRAMEWORK_NAME"
  vesper_ios_framework_info_plist \
    "$framework_dir/Info.plist" \
    "$FRAMEWORK_NAME" \
    "$VESPER_IOS_PLUGIN_BUNDLE_IDENTIFIER" \
    "$platform_name" \
    "$minimum_os_version" \
    "$VESPER_RELEASE_VERSION" \
    "$VESPER_RELEASE_BUILD"
  vesper_ios_verify_flat_framework "$framework_dir" "$FRAMEWORK_NAME"
}

verify_runtime_profile_for_slice() {
  local slice="$1"
  local library_name
  local framework_name
  local xcframework_path
  local metadata_path
  local framework_path
  local profile_path
  local runtime_hash

  for library_name in "${RUNTIME_LIBRARIES[@]}"; do
    framework_name="$(vesper_ios_ffmpeg_framework_name "$library_name")"
    xcframework_path="$RUNTIME_BUILD_DIR/$framework_name.xcframework"
    metadata_path="$(find "$xcframework_path" -path "*/$framework_name.framework/$slice-vesper-ffmpeg-build-metadata.txt" -type f | head -n 1 || true)"
    if [[ -z "$metadata_path" ]]; then
      echo "Unable to find $slice runtime metadata inside $xcframework_path" >&2
      exit 1
    fi
    framework_path="${metadata_path%/$slice-vesper-ffmpeg-build-metadata.txt}"
    profile_path="$framework_path/profile-hash.txt"
    if [[ ! -f "$profile_path" ]]; then
      echo "Missing iOS FFmpeg runtime profile hash: $profile_path" >&2
      exit 1
    fi
    runtime_hash="$(cat "$profile_path")"
    if [[ "$runtime_hash" != "$PROFILE_HASH" ]]; then
      echo "iOS FFmpeg runtime profile hash mismatch for $slice/$framework_name:" >&2
      echo "  runtime: $runtime_hash" >&2
      echo "  plugin:  $PROFILE_HASH" >&2
      exit 1
    fi
  done
}

vesper_require_command xcodebuild
vesper_require_command install_name_tool
vesper_require_command otool
vesper_require_command lipo
vesper_require_command plutil
vesper_require_command ditto

rm -rf "$RAW_OUTPUT_DIR" "$FRAMEWORK_STAGING_DIR" "$XCFRAMEWORK_PATH"
rm -f "$OUTPUT_DIR/$FRAMEWORK_NAME.xcframework.zip"
if [[ "$PLUGIN_ID" == "source-normalizer-ffmpeg" ]]; then
  rm -rf "$OUTPUT_DIR/VesperPlayerSourceNormalizerFfmpegPluginPackage"
fi
mkdir -p "$OUTPUT_DIR" "$FRAMEWORK_STAGING_DIR"

if [[ "$VESPER_IOS_PLUGIN_USES_FFMPEG" == "1" && "${VESPER_SKIP_IOS_FFMPEG_RUNTIME_STAGE:-0}" != "1" ]]; then
  "$ROOT_DIR/scripts/ios/stage-player-ffmpeg-runtime-release.sh" \
    "$OUTPUT_DIR" \
    --profile "$PROFILE" \
    "${SELECTED_SLICES[@]}"
fi

builder_args=("$PLUGIN_ID" "$RAW_OUTPUT_DIR" release)
if [[ "$VESPER_IOS_PLUGIN_USES_FFMPEG" == "1" ]]; then
  builder_args+=("${FFMPEG_ARGS[@]}")
fi
builder_args+=("${SELECTED_SLICES[@]}")
if [[ "$VESPER_IOS_PLUGIN_USES_FFMPEG" == "1" ]]; then
  export VESPER_DECLARED_FFMPEG_PROFILE="$PROFILE"
  export VESPER_DECLARED_FFMPEG_PLATFORM="ios"
  env VESPER_SKIP_APPLE_FFMPEG_PREBUILDS=1 \
    "$ROOT_DIR/scripts/ios/build-player-plugin.sh" "${builder_args[@]}"
else
  "$ROOT_DIR/scripts/ios/build-player-plugin.sh" "${builder_args[@]}"
fi

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
  esac
  if [[ ! -f "$source_dir/$VESPER_IOS_PLUGIN_DYLIB" ]]; then
    echo "Missing $VESPER_IOS_PLUGIN_DESCRIPTION binary for $slice: $source_dir/$VESPER_IOS_PLUGIN_DYLIB" >&2
    exit 1
  fi

  slice_framework_root="$FRAMEWORK_STAGING_DIR/$slice"
  create_framework \
    "$slice" \
    "$source_dir" \
    "$platform_name" \
    "$(vesper_apple_ios_deployment_target)" \
    "$slice_framework_root"
  if [[ "$VESPER_IOS_PLUGIN_USES_FFMPEG" == "1" ]]; then
    verify_runtime_profile_for_slice "$slice"
    vesper_ios_verify_sibling_framework_dependencies \
      "$slice_framework_root/$FRAMEWORK_BUNDLE/$FRAMEWORK_NAME" \
      "$RUNTIME_BUILD_DIR/frameworks/$slice"
  fi
  lipo "$slice_framework_root/$FRAMEWORK_BUNDLE/$FRAMEWORK_NAME" -verify_arch arm64
  FRAMEWORK_ARGS+=(-framework "$slice_framework_root/$FRAMEWORK_BUNDLE")
done

xcodebuild -create-xcframework \
  "${FRAMEWORK_ARGS[@]}" \
  -output "$XCFRAMEWORK_PATH"

ditto -c -k --sequesterRsrc --keepParent \
  "$XCFRAMEWORK_PATH" \
  "$OUTPUT_DIR/$FRAMEWORK_NAME.xcframework.zip"

echo "Staged optional iOS $VESPER_IOS_PLUGIN_DESCRIPTION release artifact:"
echo "  $OUTPUT_DIR/$FRAMEWORK_NAME.xcframework.zip"
if [[ "$VESPER_IOS_PLUGIN_USES_FFMPEG" == "1" ]]; then
  echo "Requires matching top-level FFmpeg component frameworks:"
  for library_name in "${RUNTIME_LIBRARIES[@]}"; do
    framework_name="$(vesper_ios_ffmpeg_framework_name "$library_name")"
    echo "  $OUTPUT_DIR/$framework_name.xcframework.zip"
  done
fi
