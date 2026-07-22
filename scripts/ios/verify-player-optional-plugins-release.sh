#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ios-framework.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ios-release.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg-validate.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
OUTPUT_DIR="${1:-$ROOT_DIR/dist/release/ios}"
COMPLIANCE_ARCHIVE="$OUTPUT_DIR/VesperPlayerOptionalPlugins-FFmpeg-Compliance.zip"

FRAMEWORKS=("${VESPER_IOS_OPTIONAL_RELEASE_FRAMEWORKS[@]}")
FFMPEG_BACKED_FRAMEWORKS=(
  VesperFFmpegAVCodec
  VesperFFmpegAVFormat
  VesperFFmpegAVUtil
  VesperPlayerRemuxFfmpegPlugin
  VesperPlayerSourceNormalizerFfmpegPlugin
)
metadata_value() {
  vesper_ffmpeg_metadata_value "$1" "$2"
}

verify_matching_file() {
  local expected_path="$1"
  local actual_path="$2"
  local description="$3"

  if ! cmp -s "$expected_path" "$actual_path"; then
    echo "$description does not match its release source:" >&2
    echo "  expected: $expected_path" >&2
    echo "  actual:   $actual_path" >&2
    return 1
  fi
}

require_text_snippets() {
  local file_path="$1"
  local description="$2"
  shift 2
  local snippet

  for snippet in "$@"; do
    if ! grep -Fq -- "$snippet" "$file_path"; then
      echo "$description is missing required release text: $snippet" >&2
      return 1
    fi
  done
}

vesper_require_command cmp
vesper_require_command ditto
vesper_require_command lipo
vesper_require_command otool
vesper_require_command shasum
vesper_require_command tar

if [[ -f "$OUTPUT_DIR/VesperPlayerFfmpegRuntime.xcframework.zip" ]]; then
  echo "The legacy umbrella FFmpeg runtime must not be released:" >&2
  echo "  $OUTPUT_DIR/VesperPlayerFfmpegRuntime.xcframework.zip" >&2
  exit 1
fi
if find "$OUTPUT_DIR" -maxdepth 1 -type f -name '*.dylib*' -print -quit | grep -q .; then
  echo "Optional iOS releases must not contain bare dylibs." >&2
  exit 1
fi

for framework_name in "${FRAMEWORKS[@]}"; do
  archive_path="$OUTPUT_DIR/$framework_name.xcframework.zip"
  if [[ ! -f "$archive_path" ]]; then
    echo "Missing optional iOS framework release artifact: $archive_path" >&2
    exit 1
  fi
done
if [[ ! -f "$COMPLIANCE_ARCHIVE" ]]; then
  echo "Missing FFmpeg compliance archive: $COMPLIANCE_ARCHIVE" >&2
  exit 1
fi

shopt -s nullglob
SOURCE_ASSETS=("$OUTPUT_DIR"/VesperPlayerOptionalPlugins-FFmpeg-*-source.tar.xz)
shopt -u nullglob
if [[ ${#SOURCE_ASSETS[@]} -ne 1 ]]; then
  echo "Expected exactly one corresponding FFmpeg source asset, found ${#SOURCE_ASSETS[@]}." >&2
  printf '  %s\n' ${SOURCE_ASSETS[@]+"${SOURCE_ASSETS[@]}"} >&2
  exit 1
fi
SOURCE_ASSET="${SOURCE_ASSETS[0]}"
vesper_ios_verify_release_asset_allowlist \
  "$OUTPUT_DIR" \
  "$(basename "$SOURCE_ASSET")"

TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/vesper-ios-optional-release-verify.XXXXXX")"
trap 'rm -rf "$TEMP_DIR"' EXIT
EXPECTED_PROFILE_HASH=""
EXPECTED_DECLARED_PROFILE=""
EXPECTED_SOURCE_SHA256=""
REFERENCE_DEVICE_METADATA=""
REFERENCE_SIMULATOR_METADATA=""

is_ffmpeg_backed() {
  local candidate="$1"
  local framework_name
  for framework_name in "${FFMPEG_BACKED_FRAMEWORKS[@]}"; do
    [[ "$candidate" == "$framework_name" ]] && return 0
  done
  return 1
}

for framework_name in "${FRAMEWORKS[@]}"; do
  archive_path="$OUTPUT_DIR/$framework_name.xcframework.zip"
  extract_dir="$TEMP_DIR/$framework_name"
  mkdir -p "$extract_dir"
  ditto -x -k "$archive_path" "$extract_dir"
  xcframework_path="$extract_dir/$framework_name.xcframework"
  archive_entries=()
  while IFS= read -r -d '' archive_entry; do
    archive_entries+=("$archive_entry")
  done < <(find "$extract_dir" -mindepth 1 -maxdepth 1 -print0)
  if [[ ${#archive_entries[@]} -ne 1 || \
    "${archive_entries[0]:-}" != "$xcframework_path" || \
    ! -d "$xcframework_path" || \
    -L "$xcframework_path" ]]
  then
    echo "Optional iOS framework archives must contain exactly one expected XCFramework:" >&2
    echo "  $archive_path" >&2
    exit 1
  fi
  vesper_ios_verify_xcframework_manifest "$xcframework_path" "$framework_name"

  slice_frameworks=()
  while IFS= read -r -d '' framework_path; do
    slice_frameworks+=("$framework_path")
  done < <(find "$xcframework_path" -type d -name "$framework_name.framework" -print0)
  if [[ ${#slice_frameworks[@]} -ne 2 ]]; then
    echo "Optional iOS XCFrameworks must contain exactly two framework slices:" >&2
    echo "  $xcframework_path (${#slice_frameworks[@]} found)" >&2
    exit 1
  fi

  device_framework="$(vesper_ios_xcframework_slice_framework "$xcframework_path" "$framework_name" iphoneos)"
  simulator_framework="$(vesper_ios_xcframework_slice_framework "$xcframework_path" "$framework_name" iphonesimulator)"
  vesper_ios_verify_flat_framework "$device_framework" "$framework_name"
  vesper_ios_verify_framework_platform "$device_framework" "$framework_name" iPhoneOS
  vesper_ios_verify_flat_framework "$simulator_framework" "$framework_name"
  vesper_ios_verify_framework_platform "$simulator_framework" "$framework_name" iPhoneSimulator

  for framework_dir in "$device_framework" "$simulator_framework"; do
    archs="$(lipo -archs "$framework_dir/$framework_name")"
    if [[ "$archs" != "arm64" ]]; then
      echo "Optional iOS XCFramework slices must contain only arm64:" >&2
      echo "  $framework_dir/$framework_name ($archs)" >&2
      exit 1
    fi
  done

  if is_ffmpeg_backed "$framework_name"; then
    device_profile="$(tr -d '[:space:]' <"$device_framework/profile-hash.txt")"
    simulator_profile="$(tr -d '[:space:]' <"$simulator_framework/profile-hash.txt")"
    if [[ -z "$device_profile" || "$device_profile" != "$simulator_profile" ]]; then
      echo "FFmpeg profile hash mismatch in $framework_name." >&2
      exit 1
    fi
    if [[ -z "$EXPECTED_PROFILE_HASH" ]]; then
      EXPECTED_PROFILE_HASH="$device_profile"
    elif [[ "$device_profile" != "$EXPECTED_PROFILE_HASH" ]]; then
      echo "Optional iOS artifacts do not share one FFmpeg profile hash." >&2
      exit 1
    fi

    device_metadata="$device_framework/ios-arm64-vesper-ffmpeg-build-metadata.txt"
    simulator_metadata="$simulator_framework/ios-simulator-arm64-vesper-ffmpeg-build-metadata.txt"
    if [[ ! -f "$device_metadata" || ! -f "$simulator_metadata" ]]; then
      echo "Missing FFmpeg build metadata in $framework_name." >&2
      exit 1
    fi
    for metadata_path in "$device_metadata" "$simulator_metadata"; do
      vesper_ffmpeg_validate_lgpl_shared_metadata_file "$metadata_path"
    done

    device_target="$(metadata_value "$device_metadata" target)"
    simulator_target="$(metadata_value "$simulator_metadata" target)"
    if [[ "$device_target" != "ios-arm64" || \
      "$simulator_target" != "ios-simulator-arm64" ]]
    then
      echo "Unexpected FFmpeg metadata target in $framework_name:" >&2
      echo "  device:    $device_target" >&2
      echo "  simulator: $simulator_target" >&2
      exit 1
    fi
    for metadata_path in "$device_metadata" "$simulator_metadata"; do
      if [[ "$(metadata_value "$metadata_path" platform)" != "apple" || \
        "$(metadata_value "$metadata_path" declared_platform)" != "ios" ]]
      then
        echo "Unexpected FFmpeg metadata platform in $framework_name:" >&2
        echo "  $metadata_path" >&2
        exit 1
      fi
    done

    slice_invariant_metadata_keys=(
      platform
      profile
      declared_profile
      declared_platform
      profile_hash
      tls_backend
      enable_dash
      libraries
      demuxers
      muxers
      protocols
      decoders
      parsers
      bsfs
      external_dependencies
      license_flags
      ffmpeg_version
      source_archive
      source_url
      source_sha256
    )
    for metadata_key in "${slice_invariant_metadata_keys[@]}"; do
      device_metadata_value="$(metadata_value "$device_metadata" "$metadata_key")"
      simulator_metadata_value="$(metadata_value "$simulator_metadata" "$metadata_key")"
      if [[ "$device_metadata_value" != "$simulator_metadata_value" ]]; then
        echo "FFmpeg metadata mismatch between device and simulator for $metadata_key in $framework_name." >&2
        exit 1
      fi
    done

    device_metadata_profile="$(metadata_value "$device_metadata" profile_hash)"
    simulator_metadata_profile="$(metadata_value "$simulator_metadata" profile_hash)"
    if [[ "$device_metadata_profile" != "$device_profile" || \
      "$simulator_metadata_profile" != "$simulator_profile" ]]
    then
      echo "FFmpeg profile-hash.txt does not match build metadata in $framework_name." >&2
      exit 1
    fi

    device_declared_profile="$(metadata_value "$device_metadata" declared_profile)"
    simulator_declared_profile="$(metadata_value "$simulator_metadata" declared_profile)"
    if [[ -z "$device_declared_profile" || "$device_declared_profile" != "$simulator_declared_profile" ]]; then
      echo "FFmpeg declared profile mismatch in $framework_name." >&2
      exit 1
    fi
    if [[ -z "$EXPECTED_DECLARED_PROFILE" ]]; then
      EXPECTED_DECLARED_PROFILE="$device_declared_profile"
    elif [[ "$device_declared_profile" != "$EXPECTED_DECLARED_PROFILE" ]]; then
      echo "Optional iOS artifacts do not share one declared FFmpeg profile." >&2
      exit 1
    fi

    if [[ -z "$REFERENCE_DEVICE_METADATA" ]]; then
      REFERENCE_DEVICE_METADATA="$device_metadata"
      REFERENCE_SIMULATOR_METADATA="$simulator_metadata"
    else
      cmp "$REFERENCE_DEVICE_METADATA" "$device_metadata"
      cmp "$REFERENCE_SIMULATOR_METADATA" "$simulator_metadata"
    fi

    device_source_sha256="$(metadata_value "$device_metadata" source_sha256)"
    simulator_source_sha256="$(metadata_value "$simulator_metadata" source_sha256)"
    if [[ ! "$device_source_sha256" =~ ^[0-9a-f]{64}$ ]] || \
      [[ "$device_source_sha256" != "$simulator_source_sha256" ]]
    then
      echo "FFmpeg build-time source SHA-256 mismatch in $framework_name." >&2
      exit 1
    fi
    if [[ -z "$EXPECTED_SOURCE_SHA256" ]]; then
      EXPECTED_SOURCE_SHA256="$device_source_sha256"
    elif [[ "$device_source_sha256" != "$EXPECTED_SOURCE_SHA256" ]]; then
      echo "Optional iOS artifacts do not share one build-time source SHA-256." >&2
      exit 1
    fi
  fi

  for framework_dir in "$device_framework" "$simulator_framework"; do
    vesper_ios_verify_optional_framework_dependencies \
      "$framework_dir/$framework_name" \
      "$framework_name"
  done
done

COMPLIANCE_EXTRACT_DIR="$TEMP_DIR/compliance"
mkdir -p "$COMPLIANCE_EXTRACT_DIR"
ditto -x -k "$COMPLIANCE_ARCHIVE" "$COMPLIANCE_EXTRACT_DIR"
COMPLIANCE_DIR="$COMPLIANCE_EXTRACT_DIR/VesperPlayerOptionalPlugins-FFmpeg-Compliance"
for required_file in \
  README.md \
  NOTICE.txt \
  SOURCE.txt \
  BUILDING.md \
  RELINKING.md \
  COPYING.LGPLv2.1 \
  COPYING.LGPLv3 \
  FFMPEG_LICENSE.md \
  VESPER_THIRD_PARTY_NOTICES.md \
  build-metadata/ios-arm64-vesper-ffmpeg-build-metadata.txt \
  build-metadata/ios-simulator-arm64-vesper-ffmpeg-build-metadata.txt
do
  if [[ ! -s "$COMPLIANCE_DIR/$required_file" ]]; then
    echo "Missing or empty compliance bundle entry: $required_file" >&2
    exit 1
  fi
done
if [[ ! -e "$COMPLIANCE_DIR/changes.diff" ]]; then
  echo "Missing compliance bundle entry: changes.diff" >&2
  exit 1
fi

SOURCE_RECORD="$COMPLIANCE_DIR/SOURCE.txt"
RECORDED_FFMPEG_VERSION="$(metadata_value "$SOURCE_RECORD" ffmpeg_version)"
RECORDED_DECLARED_PROFILE="$(metadata_value "$SOURCE_RECORD" declared_profile)"
RECORDED_SOURCE_URL="$(metadata_value "$SOURCE_RECORD" source_url)"
RECORDED_SOURCE_SHA256="$(metadata_value "$SOURCE_RECORD" source_sha256)"
if [[ "$(metadata_value "$SOURCE_RECORD" license_mode)" != "LGPL-2.1-or-later" ]]; then
  echo "Unexpected FFmpeg license mode in compliance bundle." >&2
  exit 1
fi
if [[ "$(metadata_value "$SOURCE_RECORD" linkage)" != "dynamic-frameworks" ]]; then
  echo "Unexpected FFmpeg linkage mode in compliance bundle." >&2
  exit 1
fi
if [[ "$(metadata_value "$SOURCE_RECORD" local_changes)" != "none" ]]; then
  echo "The current release expects an unmodified upstream FFmpeg source archive." >&2
  exit 1
fi
if [[ -s "$COMPLIANCE_DIR/changes.diff" ]]; then
  echo "changes.diff must be empty when local_changes=none." >&2
  exit 1
fi
if [[ "$(metadata_value "$SOURCE_RECORD" profile_hash)" != "$EXPECTED_PROFILE_HASH" ]]; then
  echo "Compliance bundle profile hash does not match the XCFrameworks." >&2
  exit 1
fi
if [[ "$RECORDED_DECLARED_PROFILE" != "$EXPECTED_DECLARED_PROFILE" ]]; then
  echo "Compliance bundle declared profile does not match the XCFrameworks." >&2
  exit 1
fi
if [[ "$RECORDED_FFMPEG_VERSION" != "$(metadata_value "$REFERENCE_DEVICE_METADATA" ffmpeg_version)" ]]; then
  echo "Compliance bundle FFmpeg version does not match the framework metadata." >&2
  exit 1
fi
if [[ "$RECORDED_SOURCE_URL" != "$(metadata_value "$REFERENCE_DEVICE_METADATA" source_url)" ]]; then
  echo "Compliance bundle source URL does not match the framework metadata." >&2
  exit 1
fi
if [[ "$(metadata_value "$SOURCE_RECORD" source_asset)" != "$(basename "$SOURCE_ASSET")" ]]; then
  echo "Compliance bundle source asset name does not match the released source archive." >&2
  exit 1
fi
EXPECTED_SOURCE_ASSET_NAME="VesperPlayerOptionalPlugins-FFmpeg-$RECORDED_FFMPEG_VERSION-source.tar.xz"
if [[ "$(basename "$SOURCE_ASSET")" != "$EXPECTED_SOURCE_ASSET_NAME" ]]; then
  echo "Released FFmpeg source asset name does not match the recorded version." >&2
  exit 1
fi
ACTUAL_SOURCE_SHA256="$(shasum -a 256 "$SOURCE_ASSET" | awk '{print $1}')"
if [[ "$ACTUAL_SOURCE_SHA256" != "$EXPECTED_SOURCE_SHA256" ]]; then
  echo "Released FFmpeg source checksum does not match the build-time framework metadata." >&2
  exit 1
fi
if [[ "$RECORDED_SOURCE_SHA256" != "$ACTUAL_SOURCE_SHA256" ]]; then
  echo "Released FFmpeg source checksum does not match the compliance bundle." >&2
  exit 1
fi

SOURCE_TREE="$TEMP_DIR/source"
vesper_extract_source_tree "$SOURCE_ASSET" "$SOURCE_TREE"
verify_matching_file \
  "$SOURCE_TREE/COPYING.LGPLv2.1" \
  "$COMPLIANCE_DIR/COPYING.LGPLv2.1" \
  "The LGPL-2.1 license copy"
verify_matching_file \
  "$SOURCE_TREE/COPYING.LGPLv3" \
  "$COMPLIANCE_DIR/COPYING.LGPLv3" \
  "The LGPL-3.0 license copy"
verify_matching_file \
  "$SOURCE_TREE/LICENSE.md" \
  "$COMPLIANCE_DIR/FFMPEG_LICENSE.md" \
  "The FFmpeg license summary"
verify_matching_file \
  "$ROOT_DIR/THIRD_PARTY_NOTICES.md" \
  "$COMPLIANCE_DIR/VESPER_THIRD_PARTY_NOTICES.md" \
  "The Vesper third-party notices copy"

require_text_snippets \
  "$COMPLIANCE_DIR/README.md" \
  "Compliance README" \
  "$EXPECTED_SOURCE_ASSET_NAME" \
  "$EXPECTED_PROFILE_HASH" \
  'VesperFFmpegAV*' \
  "Remux" \
  "SourceNormalizer"
require_text_snippets \
  "$COMPLIANCE_DIR/NOTICE.txt" \
  "FFmpeg notice" \
  "GNU Lesser General Public License version 2.1 or later" \
  "not licensed under Vesper's Apache-2.0 license" \
  "$EXPECTED_SOURCE_ASSET_NAME" \
  "Source SHA-256: $ACTUAL_SOURCE_SHA256" \
  "Build profile: $RECORDED_DECLARED_PROFILE ($EXPECTED_PROFILE_HASH)" \
  "VesperFFmpegAVCodec" \
  "VesperFFmpegAVFormat" \
  "VesperFFmpegAVUtil" \
  "RELINKING.md"
require_text_snippets \
  "$COMPLIANCE_DIR/RELINKING.md" \
  "FFmpeg relinking instructions" \
  "VesperFFmpegAVCodec.framework" \
  "VesperFFmpegAVFormat.framework" \
  "VesperFFmpegAVUtil.framework" \
  "BUILDING.md" \
  "Embed & Sign" \
  '@rpath/<Name>.framework/<Name>' \
  "source availability" \
  "relinking rights" \
  "LGPL-compatible reverse-engineering terms"

BUILDING_RECORD="$COMPLIANCE_DIR/BUILDING.md"
required_building_snippets=(
  "VESPER_APPLE_FFMPEG_VERSION=$RECORDED_FFMPEG_VERSION"
  "VESPER_APPLE_FFMPEG_SOURCE_ARCHIVE=/path/to/$EXPECTED_SOURCE_ASSET_NAME"
  "VESPER_APPLE_FFMPEG_SOURCE_URL=$RECORDED_SOURCE_URL"
  "VESPER_APPLE_FFMPEG_FORCE=1"
  "--profile $RECORDED_DECLARED_PROFILE"
)
require_text_snippets \
  "$BUILDING_RECORD" \
  "BUILDING.md" \
  "${required_building_snippets[@]}"

cmp \
  "$REFERENCE_DEVICE_METADATA" \
  "$COMPLIANCE_DIR/build-metadata/ios-arm64-vesper-ffmpeg-build-metadata.txt"
cmp \
  "$REFERENCE_SIMULATOR_METADATA" \
  "$COMPLIANCE_DIR/build-metadata/ios-simulator-arm64-vesper-ffmpeg-build-metadata.txt"

echo "Verified optional iOS plugin release artifacts:"
printf '  %s.xcframework.zip\n' "${FRAMEWORKS[@]}"
echo "  $(basename "$COMPLIANCE_ARCHIVE")"
echo "  $(basename "$SOURCE_ASSET")"
echo "  FFmpeg profile hash: $EXPECTED_PROFILE_HASH"
echo "  FFmpeg source SHA-256: $ACTUAL_SOURCE_SHA256"
