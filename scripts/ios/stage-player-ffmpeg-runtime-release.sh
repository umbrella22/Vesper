#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ios-framework.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ios-release.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg-profile.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg-validate.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROJECT_DIR="$ROOT_DIR/lib/ios/VesperPlayerKit"
OUTPUT_DIR="$ROOT_DIR/dist/release/ios"
BUILD_DIR="$PROJECT_DIR/.build/player-ffmpeg-runtime"
FRAMEWORK_STAGING_DIR="$BUILD_DIR/frameworks"
PROFILE="default"
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
  echo "Unable to resolve iOS FFmpeg runtime release version from project metadata." >&2
  exit 1
fi

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
      echo "Unknown iOS FFmpeg runtime release option: $1" >&2
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
    echo "iOS FFmpeg component framework release requires an ios-arm64 device slice." >&2
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
RUNTIME_LIBRARIES=("${VESPER_PROFILE_RESOLVED_LIBRARIES[@]}")

if [[ ${#RUNTIME_LIBRARIES[@]} -eq 0 ]]; then
  echo "The resolved iOS FFmpeg profile does not contain shared runtime libraries." >&2
  exit 1
fi

if [[ "$DRY_RUN" == "1" ]]; then
  echo "Resolved iOS FFmpeg component framework release:"
  vesper_ffmpeg_profile_print_resolved "$PROFILE" ios
  printf 'profile_hash=%s\n' "$PROFILE_HASH"
  echo "Selected slices:"
  printf '  %s\n' "${SELECTED_SLICES[@]}"
  echo "Build arguments:"
  printf '  %q\n' "${FFMPEG_ARGS[@]}" "${SELECTED_SLICES[@]}"
  echo "Output zips:"
  for library_name in "${RUNTIME_LIBRARIES[@]}"; do
    framework_name="$(vesper_ios_ffmpeg_framework_name "$library_name")"
    echo "  $OUTPUT_DIR/$framework_name.xcframework.zip"
  done
  exit 0
fi

create_component_framework() {
  local slice="$1"
  local ffmpeg_lib_dir="$2"
  local library_name="$3"
  local platform_name="$4"
  local minimum_os_version="$5"
  local output_dir="$6"
  local framework_name
  local framework_bundle
  local framework_dir
  local binary_path
  local source_binary
  local source_checksums_path
  local expected_source_sha256
  local actual_source_sha256
  local metadata_path

  framework_name="$(vesper_ios_ffmpeg_framework_name "$library_name")"
  framework_bundle="$framework_name.framework"
  framework_dir="$output_dir/$framework_bundle"
  binary_path="$framework_dir/$framework_name"
  source_binary="$ffmpeg_lib_dir/lib$library_name.dylib"

  if [[ ! -f "$source_binary" ]]; then
    echo "Missing FFmpeg shared library for $slice: $source_binary" >&2
    exit 1
  fi
  source_checksums_path="$(dirname "$(dirname "$ffmpeg_lib_dir")")/vesper-ffmpeg-library-sha256.txt"
  expected_source_sha256="$(
    vesper_ffmpeg_metadata_value "$source_checksums_path" "${library_name}_sha256"
  )"
  actual_source_sha256="$(vesper_ffmpeg_sha256_file "$source_binary")"
  if [[ "$actual_source_sha256" != "$expected_source_sha256" ]]; then
    echo "Apple FFmpeg shared library does not match its forced-build checksum:" >&2
    echo "  $source_binary" >&2
    exit 1
  fi

  rm -rf "$framework_dir"
  mkdir -p "$framework_dir/Headers" "$framework_dir/Modules"
  cp -L "$source_binary" "$binary_path"
  vesper_ios_prepare_framework_binary "$binary_path" "$framework_name"
  printf '%s\n' \
    "$(vesper_ffmpeg_sha256_file "$binary_path")" \
    >"$framework_dir/binary-sha256.txt"

  metadata_path="$(vesper_apple_slice_output_root "$slice" "$FFMPEG_APPLE_DIR")/vesper-ffmpeg-build-metadata.txt"
  if [[ ! -f "$metadata_path" ]]; then
    echo "Missing FFmpeg build metadata for $slice: $metadata_path" >&2
    exit 1
  fi
  cp "$metadata_path" "$framework_dir/$slice-vesper-ffmpeg-build-metadata.txt"
  printf '%s\n' "$PROFILE_HASH" >"$framework_dir/profile-hash.txt"

  vesper_ios_write_binary_framework_module "$framework_dir" "$framework_name"
  vesper_ios_framework_info_plist \
    "$framework_dir/Info.plist" \
    "$framework_name" \
    "$(vesper_ios_ffmpeg_bundle_identifier "$library_name")" \
    "$platform_name" \
    "$minimum_os_version" \
    "$VESPER_RELEASE_VERSION" \
    "$VESPER_RELEASE_BUILD"
  vesper_ios_verify_flat_framework "$framework_dir" "$framework_name"
}

vesper_require_command xcodebuild
vesper_require_command install_name_tool
vesper_require_command otool
vesper_require_command lipo
vesper_require_command plutil
vesper_require_command ditto

rm -rf "$BUILD_DIR"
rm -f "$OUTPUT_DIR/VesperPlayerFfmpegRuntime.xcframework.zip"
rm -f "$OUTPUT_DIR"/VesperFFmpeg*.xcframework.zip
mkdir -p "$OUTPUT_DIR" "$FRAMEWORK_STAGING_DIR"

export VESPER_DECLARED_FFMPEG_PROFILE="$PROFILE"
export VESPER_DECLARED_FFMPEG_PLATFORM="ios"
vesper_ios_run_forced_ffmpeg_release_build \
  "$ROOT_DIR/scripts/apple/build-ffmpeg-prebuilts.sh" \
  "${FFMPEG_ARGS[@]}" \
  "${SELECTED_SLICES[@]}"

for slice in "${SELECTED_SLICES[@]}"; do
  case "$slice" in
    ios-arm64)
      platform_name="iPhoneOS"
      ;;
    ios-simulator-arm64)
      platform_name="iPhoneSimulator"
      ;;
    *)
      echo "Unsupported iOS FFmpeg runtime release slice: $slice" >&2
      exit 1
      ;;
  esac

  ffmpeg_dir="$(vesper_apple_slice_output_root "$slice" "$FFMPEG_APPLE_DIR")"
  ffmpeg_libdir="$(vesper_apple_slice_output_libdir "$slice")"
  slice_framework_root="$FRAMEWORK_STAGING_DIR/$slice"
  for library_name in "${RUNTIME_LIBRARIES[@]}"; do
    create_component_framework \
      "$slice" \
      "$ffmpeg_dir/lib/$ffmpeg_libdir" \
      "$library_name" \
      "$platform_name" \
      "$(vesper_apple_ios_deployment_target)" \
      "$slice_framework_root"
  done

  for library_name in "${RUNTIME_LIBRARIES[@]}"; do
    framework_name="$(vesper_ios_ffmpeg_framework_name "$library_name")"
    vesper_ios_verify_sibling_framework_dependencies \
      "$slice_framework_root/$framework_name.framework/$framework_name" \
      "$slice_framework_root"
  done
done

for library_name in "${RUNTIME_LIBRARIES[@]}"; do
  framework_name="$(vesper_ios_ffmpeg_framework_name "$library_name")"
  xcframework_path="$BUILD_DIR/$framework_name.xcframework"
  framework_args=()

  for slice in "${SELECTED_SLICES[@]}"; do
    framework_path="$FRAMEWORK_STAGING_DIR/$slice/$framework_name.framework"
    lipo "$framework_path/$framework_name" -verify_arch arm64
    framework_args+=(-framework "$framework_path")
  done

  xcodebuild -create-xcframework \
    "${framework_args[@]}" \
    -output "$xcframework_path"

  ditto -c -k --sequesterRsrc --keepParent \
    "$xcframework_path" \
    "$OUTPUT_DIR/$framework_name.xcframework.zip"
done

echo "Staged optional iOS FFmpeg component framework release artifacts:"
for library_name in "${RUNTIME_LIBRARIES[@]}"; do
  framework_name="$(vesper_ios_ffmpeg_framework_name "$library_name")"
  echo "  $OUTPUT_DIR/$framework_name.xcframework.zip"
done
