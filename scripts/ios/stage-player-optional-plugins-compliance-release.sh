#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg-validate.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
OUTPUT_DIR="${1:-$ROOT_DIR/dist/release/ios}"
COMPONENT_ARCHIVE="$OUTPUT_DIR/VesperFFmpegAVCodec.xcframework.zip"
COMPLIANCE_ARCHIVE="$OUTPUT_DIR/VesperPlayerOptionalPlugins-FFmpeg-Compliance.zip"
COMPLIANCE_NAME="VesperPlayerOptionalPlugins-FFmpeg-Compliance"

metadata_value() {
  vesper_ffmpeg_metadata_value "$1" "$2"
}

require_equal_metadata_value() {
  local key="$1"
  local device_metadata="$2"
  local simulator_metadata="$3"
  local device_value
  local simulator_value

  device_value="$(metadata_value "$device_metadata" "$key")"
  simulator_value="$(metadata_value "$simulator_metadata" "$key")"
  if [[ "$device_value" != "$simulator_value" ]]; then
    echo "FFmpeg metadata mismatch for $key:" >&2
    echo "  device:    $device_value" >&2
    echo "  simulator: $simulator_value" >&2
    exit 1
  fi
  printf '%s\n' "$device_value"
}

if [[ ! -f "$COMPONENT_ARCHIVE" ]]; then
  echo "Missing staged FFmpeg component archive: $COMPONENT_ARCHIVE" >&2
  exit 1
fi

vesper_require_command ditto
vesper_require_command shasum
vesper_require_command tar

TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/vesper-ios-ffmpeg-compliance.XXXXXX")"
trap 'rm -rf "$TEMP_DIR"' EXIT
EXTRACT_DIR="$TEMP_DIR/component"
mkdir -p "$EXTRACT_DIR"
ditto -x -k "$COMPONENT_ARCHIVE" "$EXTRACT_DIR"

DEVICE_METADATA="$(find "$EXTRACT_DIR" -type f -name 'ios-arm64-vesper-ffmpeg-build-metadata.txt' -print -quit)"
SIMULATOR_METADATA="$(find "$EXTRACT_DIR" -type f -name 'ios-simulator-arm64-vesper-ffmpeg-build-metadata.txt' -print -quit)"
if [[ -z "$DEVICE_METADATA" || -z "$SIMULATOR_METADATA" ]]; then
  echo "The FFmpeg component XCFramework must contain device and simulator build metadata." >&2
  exit 1
fi
for metadata_path in "$DEVICE_METADATA" "$SIMULATOR_METADATA"; do
  vesper_ffmpeg_validate_lgpl_shared_metadata_file "$metadata_path"
done

PROFILE_HASH="$(require_equal_metadata_value profile_hash "$DEVICE_METADATA" "$SIMULATOR_METADATA")"
DECLARED_PROFILE="$(require_equal_metadata_value declared_profile "$DEVICE_METADATA" "$SIMULATOR_METADATA")"
FFMPEG_VERSION="$(require_equal_metadata_value ffmpeg_version "$DEVICE_METADATA" "$SIMULATOR_METADATA")"
SOURCE_URL="$(require_equal_metadata_value source_url "$DEVICE_METADATA" "$SIMULATOR_METADATA")"
SOURCE_ARCHIVE="$(require_equal_metadata_value source_archive "$DEVICE_METADATA" "$SIMULATOR_METADATA")"
RECORDED_SOURCE_SHA256="$(require_equal_metadata_value source_sha256 "$DEVICE_METADATA" "$SIMULATOR_METADATA")"
EXTERNAL_DEPENDENCIES="$(require_equal_metadata_value external_dependencies "$DEVICE_METADATA" "$SIMULATOR_METADATA")"

if [[ ! "$RECORDED_SOURCE_SHA256" =~ ^[0-9a-f]{64}$ ]]; then
  echo "Invalid build-time FFmpeg source SHA-256: $RECORDED_SOURCE_SHA256" >&2
  exit 1
fi
if [[ ! -f "$SOURCE_ARCHIVE" ]]; then
  echo "The exact FFmpeg source archive recorded by the build is missing: $SOURCE_ARCHIVE" >&2
  exit 1
fi
if [[ "$SOURCE_ARCHIVE" != *.tar.xz ]]; then
  echo "Unsupported FFmpeg source archive format for release staging: $SOURCE_ARCHIVE" >&2
  exit 1
fi

SOURCE_ASSET_NAME="VesperPlayerOptionalPlugins-FFmpeg-$FFMPEG_VERSION-source.tar.xz"
SOURCE_ASSET="$OUTPUT_DIR/$SOURCE_ASSET_NAME"
SOURCE_SHA256="$(shasum -a 256 "$SOURCE_ARCHIVE" | awk '{print $1}')"
if [[ "$SOURCE_SHA256" != "$RECORDED_SOURCE_SHA256" ]]; then
  echo "The FFmpeg source archive no longer matches the SHA-256 recorded at build time:" >&2
  echo "  archive:  $SOURCE_ARCHIVE" >&2
  echo "  recorded: $RECORDED_SOURCE_SHA256" >&2
  echo "  actual:   $SOURCE_SHA256" >&2
  exit 1
fi

rm -f "$COMPLIANCE_ARCHIVE" "$OUTPUT_DIR"/VesperPlayerOptionalPlugins-FFmpeg-*-source.tar.xz
mkdir -p "$OUTPUT_DIR"
cp "$SOURCE_ARCHIVE" "$SOURCE_ASSET"

SOURCE_TREE="$TEMP_DIR/source"
vesper_extract_source_tree "$SOURCE_ARCHIVE" "$SOURCE_TREE"
for required_source_file in COPYING.LGPLv2.1 COPYING.LGPLv3 LICENSE.md; do
  if [[ ! -f "$SOURCE_TREE/$required_source_file" ]]; then
    echo "The FFmpeg source archive is missing $required_source_file" >&2
    exit 1
  fi
done

COMPLIANCE_DIR="$TEMP_DIR/$COMPLIANCE_NAME"
mkdir -p "$COMPLIANCE_DIR/build-metadata"
cp "$SOURCE_TREE/COPYING.LGPLv2.1" "$COMPLIANCE_DIR/"
cp "$SOURCE_TREE/COPYING.LGPLv3" "$COMPLIANCE_DIR/"
cp "$SOURCE_TREE/LICENSE.md" "$COMPLIANCE_DIR/FFMPEG_LICENSE.md"
cp "$ROOT_DIR/THIRD_PARTY_NOTICES.md" "$COMPLIANCE_DIR/VESPER_THIRD_PARTY_NOTICES.md"
cp "$DEVICE_METADATA" "$COMPLIANCE_DIR/build-metadata/ios-arm64-vesper-ffmpeg-build-metadata.txt"
cp "$SIMULATOR_METADATA" "$COMPLIANCE_DIR/build-metadata/ios-simulator-arm64-vesper-ffmpeg-build-metadata.txt"
: >"$COMPLIANCE_DIR/changes.diff"

cat >"$COMPLIANCE_DIR/SOURCE.txt" <<EOF
component=FFmpeg
ffmpeg_version=$FFMPEG_VERSION
license_mode=LGPL-2.1-or-later
linkage=dynamic-frameworks
declared_profile=$DECLARED_PROFILE
profile_hash=$PROFILE_HASH
source_url=$SOURCE_URL
source_asset=$SOURCE_ASSET_NAME
source_sha256=$SOURCE_SHA256
local_changes=none
external_dependencies=$EXTERNAL_DEPENDENCIES
EOF

cat >"$COMPLIANCE_DIR/NOTICE.txt" <<EOF
This optional iOS binary distribution uses libraries from the FFmpeg project
under the GNU Lesser General Public License version 2.1 or later.

FFmpeg is not licensed under Vesper's Apache-2.0 license. The exact
corresponding FFmpeg source is published in the same release as:

  $SOURCE_ASSET_NAME

Source SHA-256: $SOURCE_SHA256
Build profile: $DECLARED_PROFILE ($PROFILE_HASH)

The two Vesper FFmpeg plugin frameworks link to the separately distributed
VesperFFmpegAVCodec, VesperFFmpegAVFormat, and VesperFFmpegAVUtil dynamic
frameworks. See RELINKING.md for replacement and rebuild instructions.
EOF

cat >"$COMPLIANCE_DIR/BUILDING.md" <<EOF
# Rebuilding the iOS FFmpeg frameworks

This compliance bundle corresponds to FFmpeg $FFMPEG_VERSION and profile
\`$DECLARED_PROFILE\` (\`$PROFILE_HASH\`). The original source archive is
\`$SOURCE_ASSET_NAME\`, whose SHA-256 is recorded in \`SOURCE.txt\`.

From the matching Vesper release tag, stage the same device and Apple Silicon
Simulator artifacts with:

\`\`\`sh
VESPER_APPLE_FFMPEG_VERSION=$FFMPEG_VERSION \\
VESPER_APPLE_FFMPEG_SOURCE_ARCHIVE=/path/to/$SOURCE_ASSET_NAME \\
VESPER_APPLE_FFMPEG_SOURCE_URL=$SOURCE_URL \\
VESPER_APPLE_FFMPEG_FORCE=1 \\
  ./scripts/vesper ios stage-optional-plugins-release /tmp/vesper-ios-release \\
  --profile $DECLARED_PROFILE \\
  ios-arm64 ios-simulator-arm64
\`\`\`

The exact per-slice FFmpeg configure lines are preserved under
\`build-metadata/\`. Vesper extracts the upstream archive without applying
source patches; \`changes.diff\` is therefore intentionally empty.
EOF

cat >"$COMPLIANCE_DIR/RELINKING.md" <<EOF
# Replacing the FFmpeg dynamic frameworks

The optional iOS plugins use top-level dynamic framework dependencies:

- \`VesperFFmpegAVCodec.framework\`
- \`VesperFFmpegAVFormat.framework\`
- \`VesperFFmpegAVUtil.framework\`

To use a modified, interface-compatible FFmpeg build:

1. Rebuild the three component XCFrameworks with the command in \`BUILDING.md\`.
   For a modified FFmpeg tree, create a \`.tar.xz\` archive with one top-level
   source directory and point \`VESPER_APPLE_FFMPEG_SOURCE_ARCHIVE\` at that
   archive before rebuilding.
2. Replace the corresponding XCFramework inputs in the host application before
   the App target performs Embed & Sign.
3. Preserve each framework name, bundle executable name, and
   \`@rpath/<Name>.framework/<Name>\` install name so the plugin dependencies
   continue to resolve.
4. Build and sign the host application normally with the replacement
   frameworks.

The released Remux and SourceNormalizer plugin frameworks do not contain a
second static copy of FFmpeg. Final application distributors remain responsible
for preserving this notice, source availability, relinking rights, and
LGPL-compatible reverse-engineering terms in their own distribution.
EOF

cat >"$COMPLIANCE_DIR/README.md" <<EOF
# Vesper optional iOS FFmpeg compliance bundle

This directory accompanies the FFmpeg-backed iOS optional-plugin XCFrameworks.
It records the exact source, licenses, notices, build configuration, and dynamic
framework replacement path for profile \`$PROFILE_HASH\`.

The source archive is a separate asset in the same release:
\`$SOURCE_ASSET_NAME\`.

The FFmpeg redistribution boundary covers the three \`VesperFFmpegAV*\`
component frameworks plus the Remux and SourceNormalizer FFmpeg plugins. The
VideoToolbox Decoder and diagnostic FrameProcessor plugins do not bundle or
link FFmpeg.
EOF

ditto -c -k --sequesterRsrc --keepParent "$COMPLIANCE_DIR" "$COMPLIANCE_ARCHIVE"

echo "Staged optional iOS FFmpeg compliance release assets:"
echo "  $COMPLIANCE_ARCHIVE"
echo "  $SOURCE_ASSET"
echo "  source_sha256=$SOURCE_SHA256"
