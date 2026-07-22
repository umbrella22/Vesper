#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RELEASE_DIR="${1:-$ROOT_DIR/dist/release/ios}"
VERIFY_SCRIPT="$ROOT_DIR/scripts/ios/verify-player-optional-plugins-release.sh"
COMPLIANCE_NAME="VesperPlayerOptionalPlugins-FFmpeg-Compliance"
FFMPEG_BACKED_FRAMEWORKS=(
  VesperFFmpegAVCodec
  VesperFFmpegAVFormat
  VesperFFmpegAVUtil
  VesperPlayerRemuxFfmpegPlugin
  VesperPlayerSourceNormalizerFfmpegPlugin
)

if [[ ! -d "$RELEASE_DIR" ]]; then
  echo "Missing optional iOS release fixture directory: $RELEASE_DIR" >&2
  exit 1
fi

TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/vesper-ios-optional-release-test.XXXXXX")"
trap 'rm -rf "$TEMP_DIR"' EXIT

copy_release_fixture() {
  local destination_dir="$1"

  rm -rf "$destination_dir"
  mkdir -p "$destination_dir"
  cp -R "$RELEASE_DIR/." "$destination_dir/"
}

expect_verification_failure() {
  local fixture_dir="$1"
  local expected_message="$2"
  local failure_output

  if failure_output="$(/bin/bash "$VERIFY_SCRIPT" "$fixture_dir" 2>&1)"; then
    echo "The malformed optional iOS release was incorrectly accepted: $fixture_dir" >&2
    exit 1
  fi
  if [[ "$failure_output" != *"$expected_message"* ]]; then
    echo "The malformed optional iOS release failed for an unexpected reason:" >&2
    echo "$failure_output" >&2
    exit 1
  fi
}

repack_compliance_fixture() {
  local fixture_dir="$1"
  local mutate_file="$2"
  local mutation="$3"
  local extract_dir="$fixture_dir/compliance-extract"
  local archive_path="$fixture_dir/VesperPlayerOptionalPlugins-FFmpeg-Compliance.zip"
  local compliance_dir="$extract_dir/$COMPLIANCE_NAME"

  mkdir -p "$extract_dir"
  ditto -x -k "$archive_path" "$extract_dir"
  case "$mutation" in
    empty)
      : >"$compliance_dir/$mutate_file"
      ;;
    replace)
      printf '%s\n' 'corrupted release fixture' >"$compliance_dir/$mutate_file"
      ;;
    append-duplicate-version)
      printf '%s\n' 'ffmpeg_version=9.9.9' >>"$compliance_dir/$mutate_file"
      ;;
    replace-device-target)
      sed 's/^target=ios-arm64$/target=invalid-device-target/' \
        "$compliance_dir/$mutate_file" \
        >"$compliance_dir/$mutate_file.tmp"
      mv "$compliance_dir/$mutate_file.tmp" "$compliance_dir/$mutate_file"
      ;;
    *)
      echo "Unsupported compliance fixture mutation: $mutation" >&2
      exit 1
      ;;
  esac
  rm -f "$archive_path"
  ditto -c -k --sequesterRsrc --keepParent "$compliance_dir" "$archive_path"
  rm -rf "$extract_dir"
}

STALE_ASSET_FIXTURE="$TEMP_DIR/stale-asset"
copy_release_fixture "$STALE_ASSET_FIXTURE"
touch "$STALE_ASSET_FIXTURE/VesperPlayerRetiredPlugin.xcframework.zip"
expect_verification_failure \
  "$STALE_ASSET_FIXTURE" \
  "Unexpected top-level iOS release asset"

EMPTY_NOTICE_FIXTURE="$TEMP_DIR/empty-notice"
copy_release_fixture "$EMPTY_NOTICE_FIXTURE"
repack_compliance_fixture "$EMPTY_NOTICE_FIXTURE" NOTICE.txt empty
expect_verification_failure \
  "$EMPTY_NOTICE_FIXTURE" \
  "Missing or empty compliance bundle entry: NOTICE.txt"

ALTERED_LICENSE_FIXTURE="$TEMP_DIR/altered-license"
copy_release_fixture "$ALTERED_LICENSE_FIXTURE"
repack_compliance_fixture "$ALTERED_LICENSE_FIXTURE" COPYING.LGPLv2.1 replace
expect_verification_failure \
  "$ALTERED_LICENSE_FIXTURE" \
  "The LGPL-2.1 license copy does not match its release source"

EXTRA_SLICE_FIXTURE="$TEMP_DIR/extra-slice"
copy_release_fixture "$EXTRA_SLICE_FIXTURE"
EXTRA_SLICE_ARCHIVE="$EXTRA_SLICE_FIXTURE/VesperFFmpegAVCodec.xcframework.zip"
EXTRA_SLICE_EXTRACT="$EXTRA_SLICE_FIXTURE/extra-slice-extract"
mkdir -p "$EXTRA_SLICE_EXTRACT"
ditto -x -k "$EXTRA_SLICE_ARCHIVE" "$EXTRA_SLICE_EXTRACT"
EXTRA_SLICE_XCFRAMEWORK="$EXTRA_SLICE_EXTRACT/VesperFFmpegAVCodec.xcframework"
cp -R \
  "$EXTRA_SLICE_XCFRAMEWORK/ios-arm64" \
  "$EXTRA_SLICE_XCFRAMEWORK/ios-arm64-maccatalyst"
rm -f "$EXTRA_SLICE_ARCHIVE"
ditto -c -k --sequesterRsrc --keepParent \
  "$EXTRA_SLICE_XCFRAMEWORK" \
  "$EXTRA_SLICE_ARCHIVE"
rm -rf "$EXTRA_SLICE_EXTRACT"
expect_verification_failure \
  "$EXTRA_SLICE_FIXTURE" \
  "Unexpected XCFramework top-level payload"

ALTERED_MANIFEST_FIXTURE="$TEMP_DIR/altered-manifest"
copy_release_fixture "$ALTERED_MANIFEST_FIXTURE"
ALTERED_MANIFEST_ARCHIVE="$ALTERED_MANIFEST_FIXTURE/VesperFFmpegAVCodec.xcframework.zip"
ALTERED_MANIFEST_EXTRACT="$ALTERED_MANIFEST_FIXTURE/altered-manifest-extract"
mkdir -p "$ALTERED_MANIFEST_EXTRACT"
ditto -x -k "$ALTERED_MANIFEST_ARCHIVE" "$ALTERED_MANIFEST_EXTRACT"
/usr/libexec/PlistBuddy \
  -c 'Set :AvailableLibraries:1:SupportedPlatformVariant maccatalyst' \
  "$ALTERED_MANIFEST_EXTRACT/VesperFFmpegAVCodec.xcframework/Info.plist"
rm -f "$ALTERED_MANIFEST_ARCHIVE"
ditto -c -k --sequesterRsrc --keepParent \
  "$ALTERED_MANIFEST_EXTRACT/VesperFFmpegAVCodec.xcframework" \
  "$ALTERED_MANIFEST_ARCHIVE"
rm -rf "$ALTERED_MANIFEST_EXTRACT"
expect_verification_failure \
  "$ALTERED_MANIFEST_FIXTURE" \
  "Unexpected XCFramework SupportedPlatformVariant"

MISSING_MANIFEST_PATH_FIXTURE="$TEMP_DIR/missing-manifest-path"
copy_release_fixture "$MISSING_MANIFEST_PATH_FIXTURE"
MISSING_MANIFEST_PATH_ARCHIVE="$MISSING_MANIFEST_PATH_FIXTURE/VesperPlayerDecoderVideoToolboxPlugin.xcframework.zip"
MISSING_MANIFEST_PATH_EXTRACT="$MISSING_MANIFEST_PATH_FIXTURE/missing-manifest-path-extract"
mkdir -p "$MISSING_MANIFEST_PATH_EXTRACT"
ditto -x -k "$MISSING_MANIFEST_PATH_ARCHIVE" "$MISSING_MANIFEST_PATH_EXTRACT"
MISSING_MANIFEST_PATH_XCFRAMEWORK="$MISSING_MANIFEST_PATH_EXTRACT/VesperPlayerDecoderVideoToolboxPlugin.xcframework"
mv \
  "$MISSING_MANIFEST_PATH_XCFRAMEWORK/ios-arm64/VesperPlayerDecoderVideoToolboxPlugin.framework" \
  "$MISSING_MANIFEST_PATH_XCFRAMEWORK/ios-arm64/Rogue.framework"
rm -f "$MISSING_MANIFEST_PATH_ARCHIVE"
ditto -c -k --sequesterRsrc --keepParent \
  "$MISSING_MANIFEST_PATH_XCFRAMEWORK" \
  "$MISSING_MANIFEST_PATH_ARCHIVE"
rm -rf "$MISSING_MANIFEST_PATH_EXTRACT"
expect_verification_failure \
  "$MISSING_MANIFEST_PATH_FIXTURE" \
  "XCFramework manifest LibraryPath does not exist for ios-arm64"

UNDECLARED_PAYLOAD_FIXTURE="$TEMP_DIR/undeclared-payload"
copy_release_fixture "$UNDECLARED_PAYLOAD_FIXTURE"
UNDECLARED_PAYLOAD_ARCHIVE="$UNDECLARED_PAYLOAD_FIXTURE/VesperPlayerDecoderVideoToolboxPlugin.xcframework.zip"
UNDECLARED_PAYLOAD_EXTRACT="$UNDECLARED_PAYLOAD_FIXTURE/undeclared-payload-extract"
mkdir -p "$UNDECLARED_PAYLOAD_EXTRACT"
ditto -x -k "$UNDECLARED_PAYLOAD_ARCHIVE" "$UNDECLARED_PAYLOAD_EXTRACT"
UNDECLARED_PAYLOAD_XCFRAMEWORK="$UNDECLARED_PAYLOAD_EXTRACT/VesperPlayerDecoderVideoToolboxPlugin.xcframework"
mkdir -p "$UNDECLARED_PAYLOAD_XCFRAMEWORK/ios-arm64/Unexpected.framework"
touch "$UNDECLARED_PAYLOAD_XCFRAMEWORK/ios-arm64/Unexpected.framework/Unexpected"
rm -f "$UNDECLARED_PAYLOAD_ARCHIVE"
ditto -c -k --sequesterRsrc --keepParent \
  "$UNDECLARED_PAYLOAD_XCFRAMEWORK" \
  "$UNDECLARED_PAYLOAD_ARCHIVE"
rm -rf "$UNDECLARED_PAYLOAD_EXTRACT"
expect_verification_failure \
  "$UNDECLARED_PAYLOAD_FIXTURE" \
  "Unexpected XCFramework slice payload for ios-arm64"

INVALID_TARGET_FIXTURE="$TEMP_DIR/invalid-target"
copy_release_fixture "$INVALID_TARGET_FIXTURE"
for framework_name in "${FFMPEG_BACKED_FRAMEWORKS[@]}"; do
  INVALID_TARGET_ARCHIVE="$INVALID_TARGET_FIXTURE/$framework_name.xcframework.zip"
  INVALID_TARGET_EXTRACT="$INVALID_TARGET_FIXTURE/$framework_name-target-extract"
  mkdir -p "$INVALID_TARGET_EXTRACT"
  ditto -x -k "$INVALID_TARGET_ARCHIVE" "$INVALID_TARGET_EXTRACT"
  INVALID_TARGET_XCFRAMEWORK="$INVALID_TARGET_EXTRACT/$framework_name.xcframework"
  INVALID_TARGET_METADATA="$INVALID_TARGET_XCFRAMEWORK/ios-arm64/$framework_name.framework/ios-arm64-vesper-ffmpeg-build-metadata.txt"
  sed 's/^target=ios-arm64$/target=invalid-device-target/' \
    "$INVALID_TARGET_METADATA" \
    >"$INVALID_TARGET_METADATA.tmp"
  mv "$INVALID_TARGET_METADATA.tmp" "$INVALID_TARGET_METADATA"
  rm -f "$INVALID_TARGET_ARCHIVE"
  ditto -c -k --sequesterRsrc --keepParent \
    "$INVALID_TARGET_XCFRAMEWORK" \
    "$INVALID_TARGET_ARCHIVE"
  rm -rf "$INVALID_TARGET_EXTRACT"
done
repack_compliance_fixture \
  "$INVALID_TARGET_FIXTURE" \
  build-metadata/ios-arm64-vesper-ffmpeg-build-metadata.txt \
  replace-device-target
expect_verification_failure \
  "$INVALID_TARGET_FIXTURE" \
  "Unexpected FFmpeg metadata target in VesperFFmpegAVCodec"

MISMATCHED_SLICE_FIXTURE="$TEMP_DIR/mismatched-slice-metadata"
copy_release_fixture "$MISMATCHED_SLICE_FIXTURE"
MISMATCHED_SLICE_ARCHIVE="$MISMATCHED_SLICE_FIXTURE/VesperFFmpegAVCodec.xcframework.zip"
MISMATCHED_SLICE_EXTRACT="$MISMATCHED_SLICE_FIXTURE/mismatched-slice-extract"
mkdir -p "$MISMATCHED_SLICE_EXTRACT"
ditto -x -k "$MISMATCHED_SLICE_ARCHIVE" "$MISMATCHED_SLICE_EXTRACT"
MISMATCHED_SIMULATOR_METADATA="$MISMATCHED_SLICE_EXTRACT/VesperFFmpegAVCodec.xcframework/ios-arm64-simulator/VesperFFmpegAVCodec.framework/ios-simulator-arm64-vesper-ffmpeg-build-metadata.txt"
sed 's/^ffmpeg_version=8\.1\.2$/ffmpeg_version=9.9.9/' \
  "$MISMATCHED_SIMULATOR_METADATA" \
  >"$MISMATCHED_SIMULATOR_METADATA.tmp"
mv "$MISMATCHED_SIMULATOR_METADATA.tmp" "$MISMATCHED_SIMULATOR_METADATA"
rm -f "$MISMATCHED_SLICE_ARCHIVE"
ditto -c -k --sequesterRsrc --keepParent \
  "$MISMATCHED_SLICE_EXTRACT/VesperFFmpegAVCodec.xcframework" \
  "$MISMATCHED_SLICE_ARCHIVE"
rm -rf "$MISMATCHED_SLICE_EXTRACT"
expect_verification_failure \
  "$MISMATCHED_SLICE_FIXTURE" \
  "FFmpeg metadata mismatch between device and simulator for ffmpeg_version"

DUPLICATE_METADATA_FIXTURE="$TEMP_DIR/duplicate-metadata"
copy_release_fixture "$DUPLICATE_METADATA_FIXTURE"
repack_compliance_fixture \
  "$DUPLICATE_METADATA_FIXTURE" \
  SOURCE.txt \
  append-duplicate-version
expect_verification_failure \
  "$DUPLICATE_METADATA_FIXTURE" \
  "Duplicate FFmpeg metadata key 'ffmpeg_version'"

echo "Verified optional iOS release manifest and FFmpeg compliance failure paths."
