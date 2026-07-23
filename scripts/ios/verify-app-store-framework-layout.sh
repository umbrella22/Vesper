#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ios-framework.sh"

APP_PATH="${1:-}"
VERIFY_SIGNATURES=0

EXPECTED_FRAMEWORKS=(
  VesperFFmpegAVCodec
  VesperFFmpegAVFormat
  VesperFFmpegAVUtil
  VesperPlayerRemuxFfmpegPlugin
  VesperPlayerSourceNormalizerFfmpegPlugin
  VesperPlayerDecoderVideoToolboxPlugin
  VesperPlayerFrameProcessorDiagnosticPlugin
)

FFMPEG_BACKED_FRAMEWORKS=(
  VesperFFmpegAVCodec
  VesperFFmpegAVFormat
  VesperFFmpegAVUtil
  VesperPlayerRemuxFfmpegPlugin
  VesperPlayerSourceNormalizerFfmpegPlugin
)

usage() {
  cat <<EOF >&2
Usage: $0 <app-path> [--verify-signatures]

Verifies the complete optional-plugin App Store bundle layout. The app must
contain all seven optional frameworks as top-level siblings under Frameworks/.
EOF
}

if [[ -z "$APP_PATH" ]]; then
  usage
  exit 1
fi
shift

while [[ $# -gt 0 ]]; do
  case "$1" in
    --verify-signatures)
      VERIFY_SIGNATURES=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown App Store layout verification option: $1" >&2
      usage
      exit 1
      ;;
  esac
done

if [[ ! -d "$APP_PATH" ]]; then
  echo "App bundle does not exist: $APP_PATH" >&2
  exit 1
fi

vesper_verify_directory_excludes_test_fixtures "$APP_PATH"
FLUTTER_AOT_BINARY="$APP_PATH/Frameworks/App.framework/App"
if [[ -f "$FLUTTER_AOT_BINARY" ]]; then
  vesper_verify_binary_excludes_test_fixture_markers "$FLUTTER_AOT_BINARY"
fi

FRAMEWORKS_DIR="$APP_PATH/Frameworks"
if [[ ! -d "$FRAMEWORKS_DIR" ]]; then
  echo "App bundle is missing its Frameworks directory: $FRAMEWORKS_DIR" >&2
  exit 1
fi

vesper_require_command otool
vesper_require_command lipo
vesper_require_command plutil

unexpected_dylib="$(
  find "$FRAMEWORKS_DIR" -type f -name '*.dylib*' -print -quit
)"
if [[ -n "$unexpected_dylib" ]]; then
  echo "App bundles validated by this release gate must not ship standalone dylibs:" >&2
  echo "  $unexpected_dylib" >&2
  exit 1
fi

nested_frameworks="$(
  find "$FRAMEWORKS_DIR" -mindepth 2 -type d \
    \( -name Frameworks -o -name '*.framework' \) \
    -print -quit
)"
if [[ -n "$nested_frameworks" ]]; then
  echo "App bundles must not contain nested framework directories:" >&2
  echo "  $nested_frameworks" >&2
  exit 1
fi

if [[ -d "$FRAMEWORKS_DIR/VesperPlayerFfmpegRuntime.framework" ]]; then
  echo "The legacy umbrella FFmpeg runtime framework is not distributable:" >&2
  echo "  $FRAMEWORKS_DIR/VesperPlayerFfmpegRuntime.framework" >&2
  exit 1
fi

for framework_name in "${EXPECTED_FRAMEWORKS[@]}"; do
  framework_dir="$FRAMEWORKS_DIR/$framework_name.framework"
  if [[ ! -d "$framework_dir" ]]; then
    echo "Missing required top-level optional framework:" >&2
    echo "  $framework_dir" >&2
    exit 1
  fi

  vesper_ios_verify_flat_framework "$framework_dir" "$framework_name"
  vesper_ios_verify_framework_platform "$framework_dir" "$framework_name" iPhoneOS
  vesper_ios_verify_sibling_framework_dependencies \
    "$framework_dir/$framework_name" \
    "$FRAMEWORKS_DIR"
  vesper_ios_verify_optional_framework_dependencies \
    "$framework_dir/$framework_name" \
    "$framework_name"
  framework_archs="$(lipo -archs "$framework_dir/$framework_name")"
  if [[ "$framework_archs" != "arm64" ]]; then
    echo "Optional iOS App Store frameworks must contain only arm64:" >&2
    echo "  $framework_dir/$framework_name ($framework_archs)" >&2
    exit 1
  fi
done

expected_profile_hash=""
for framework_name in "${FFMPEG_BACKED_FRAMEWORKS[@]}"; do
  profile_path="$FRAMEWORKS_DIR/$framework_name.framework/profile-hash.txt"
  if [[ ! -f "$profile_path" ]]; then
    echo "Missing FFmpeg profile hash for $framework_name:" >&2
    echo "  $profile_path" >&2
    exit 1
  fi

  profile_hash="$(tr -d '[:space:]' <"$profile_path")"
  if [[ -z "$profile_hash" ]]; then
    echo "Empty FFmpeg profile hash: $profile_path" >&2
    exit 1
  fi
  if [[ -z "$expected_profile_hash" ]]; then
    expected_profile_hash="$profile_hash"
  elif [[ "$profile_hash" != "$expected_profile_hash" ]]; then
    echo "FFmpeg profile hash mismatch in the app bundle:" >&2
    echo "  expected: $expected_profile_hash" >&2
    echo "  actual:   $profile_hash ($framework_name)" >&2
    exit 1
  fi
done

if [[ "$VERIFY_SIGNATURES" == "1" ]]; then
  vesper_require_command codesign
  for framework_name in "${EXPECTED_FRAMEWORKS[@]}"; do
    codesign --verify --strict "$FRAMEWORKS_DIR/$framework_name.framework"
  done
  codesign --verify --strict --deep "$APP_PATH"
fi

echo "Verified App Store-compatible optional iOS framework layout:"
echo "  $APP_PATH"
echo "  FFmpeg profile hash: $expected_profile_hash"
printf '  %s.framework\n' "${EXPECTED_FRAMEWORKS[@]}"
