#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT_DIR/scripts/lib/ios-framework.sh"
source "$ROOT_DIR/scripts/lib/ios-release.sh"

FRAMEWORK_NAME="VesperPlayerSourceNormalizerFfmpegPlugin"
XCFRAMEWORK_DIR="$ROOT_DIR/lib/ios/VesperPlayerOptionalPlugins/Artifacts/$FRAMEWORK_NAME.xcframework"
DEVICE_FRAMEWORK="$XCFRAMEWORK_DIR/ios-arm64/$FRAMEWORK_NAME.framework"
SIMULATOR_FRAMEWORK="$XCFRAMEWORK_DIR/ios-arm64-simulator/$FRAMEWORK_NAME.framework"

for framework_dir in "$DEVICE_FRAMEWORK" "$SIMULATOR_FRAMEWORK"; do
  if [[ ! -d "$framework_dir" ]]; then
    echo "Missing staged framework required by the platform validation regression: $framework_dir" >&2
    exit 1
  fi
done

vesper_ios_verify_framework_platform "$DEVICE_FRAMEWORK" "$FRAMEWORK_NAME" iPhoneOS
vesper_ios_verify_framework_platform "$SIMULATOR_FRAMEWORK" "$FRAMEWORK_NAME" iPhoneSimulator

TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/vesper-ios-framework-platform.XXXXXX")"
trap 'rm -rf "$TEMP_DIR"' EXIT
SPOOFED_FRAMEWORK="$TEMP_DIR/$FRAMEWORK_NAME.framework"
ditto "$SIMULATOR_FRAMEWORK" "$SPOOFED_FRAMEWORK"
/usr/libexec/PlistBuddy -c 'Set :CFBundleSupportedPlatforms:0 iPhoneOS' "$SPOOFED_FRAMEWORK/Info.plist"
/usr/libexec/PlistBuddy -c 'Set :DTPlatformName iphoneos' "$SPOOFED_FRAMEWORK/Info.plist"

failure_output=""
if failure_output="$(
  vesper_ios_verify_framework_platform \
    "$SPOOFED_FRAMEWORK" \
    "$FRAMEWORK_NAME" \
    iPhoneOS 2>&1
)"; then
  echo "A simulator Mach-O was incorrectly accepted as an iPhoneOS framework." >&2
  exit 1
fi
if [[ "$failure_output" != *"Unexpected framework Mach-O build platform"* ]]; then
  echo "The spoofed simulator framework failed for an unexpected reason:" >&2
  echo "$failure_output" >&2
  exit 1
fi

EXPECTED_PLUGIN_DEPENDENCIES="$(printf '%s\n' \
  '@rpath/VesperFFmpegAVCodec.framework/VesperFFmpegAVCodec' \
  '@rpath/VesperFFmpegAVFormat.framework/VesperFFmpegAVFormat' \
  '@rpath/VesperFFmpegAVUtil.framework/VesperFFmpegAVUtil')"
vesper_ios_verify_exact_dynamic_dependency_list \
  fixture-plugin \
  VesperPlayerSourceNormalizerFfmpegPlugin \
  "$EXPECTED_PLUGIN_DEPENDENCIES" \
  VesperFFmpegAVCodec \
  VesperFFmpegAVFormat \
  VesperFFmpegAVUtil

if failure_output="$(
  vesper_ios_verify_exact_dynamic_dependency_list \
    fixture-plugin \
    VesperPlayerSourceNormalizerFfmpegPlugin \
    '@rpath/VesperFFmpegAVCodec.framework/VesperFFmpegAVCodec' \
    VesperFFmpegAVCodec \
    VesperFFmpegAVFormat \
    VesperFFmpegAVUtil 2>&1
)"; then
  echo "A missing required FFmpeg sibling dependency was incorrectly accepted." >&2
  exit 1
fi
if [[ "$failure_output" != *"is missing dynamic dependency"* ]]; then
  echo "The missing FFmpeg sibling failed for an unexpected reason:" >&2
  echo "$failure_output" >&2
  exit 1
fi

unexpected_plugin_dependencies="$EXPECTED_PLUGIN_DEPENDENCIES
@rpath/VesperFFmpegSWScale.framework/VesperFFmpegSWScale"
if failure_output="$(
  vesper_ios_verify_exact_dynamic_dependency_list \
    fixture-plugin \
    VesperPlayerSourceNormalizerFfmpegPlugin \
    "$unexpected_plugin_dependencies" \
    VesperFFmpegAVCodec \
    VesperFFmpegAVFormat \
    VesperFFmpegAVUtil 2>&1
)"; then
  echo "An unreleased FFmpeg sibling dependency was incorrectly accepted." >&2
  exit 1
fi
if [[ "$failure_output" != *"unexpected non-system dynamic dependency"* ]]; then
  echo "The unexpected FFmpeg sibling failed for an unexpected reason:" >&2
  echo "$failure_output" >&2
  exit 1
fi

legacy_runtime_dependencies="$EXPECTED_PLUGIN_DEPENDENCIES
@rpath/VesperPlayerFfmpegRuntime.framework/VesperPlayerFfmpegRuntime"
if failure_output="$(
  vesper_ios_verify_exact_dynamic_dependency_list \
    fixture-plugin \
    VesperPlayerSourceNormalizerFfmpegPlugin \
    "$legacy_runtime_dependencies" \
    VesperFFmpegAVCodec \
    VesperFFmpegAVFormat \
    VesperFFmpegAVUtil 2>&1
)"; then
  echo "A legacy umbrella FFmpeg runtime dependency was incorrectly accepted." >&2
  exit 1
fi
if [[ "$failure_output" != *"unexpected non-system dynamic dependency"* ]]; then
  echo "The legacy umbrella FFmpeg dependency failed for an unexpected reason:" >&2
  echo "$failure_output" >&2
  exit 1
fi

if failure_output="$(
  vesper_ios_verify_exact_dynamic_dependency_list \
    fixture-diagnostic \
    VesperPlayerFrameProcessorDiagnosticPlugin \
    '@rpath/VesperFFmpegAVUtil.framework/VesperFFmpegAVUtil' 2>&1
)"; then
  echo "A diagnostic plugin FFmpeg dependency was incorrectly accepted." >&2
  exit 1
fi
if [[ "$failure_output" != *"unexpected non-system dynamic dependency"* ]]; then
  echo "The diagnostic FFmpeg dependency failed for an unexpected reason:" >&2
  echo "$failure_output" >&2
  exit 1
fi

otool() {
  printf '%s\n' \
    'fixture-with-spaces:' \
    '    @rpath/Support Files/UnexpectedSibling.framework/UnexpectedSibling (compatibility version 1.0.0, current version 1.0.0)'
}
spaced_dependency="$(vesper_ios_binary_dependencies fixture-with-spaces)"
unset -f otool
if [[ "$spaced_dependency" != '@rpath/Support Files/UnexpectedSibling.framework/UnexpectedSibling' ]]; then
  echo "The otool dependency parser did not preserve locator whitespace:" >&2
  echo "  $spaced_dependency" >&2
  exit 1
fi
if failure_output="$(
  vesper_ios_verify_exact_dynamic_dependency_list \
    fixture-with-spaces \
    VesperPlayerFrameProcessorDiagnosticPlugin \
    "$spaced_dependency" 2>&1
)"; then
  echo "An unexpected sibling dependency containing whitespace was incorrectly accepted." >&2
  exit 1
fi
if [[ "$failure_output" != *"unexpected non-system dynamic dependency"* ]]; then
  echo "The whitespace-bearing sibling dependency failed for an unexpected reason:" >&2
  echo "$failure_output" >&2
  exit 1
fi

MANIFEST_DIR="$TEMP_DIR/release-manifest"
mkdir -p "$MANIFEST_DIR"
for framework_name in "${VESPER_IOS_OPTIONAL_RELEASE_FRAMEWORKS[@]}"; do
  touch "$MANIFEST_DIR/$framework_name.xcframework.zip"
done
touch \
  "$MANIFEST_DIR/VesperPlayerKit.xcframework.zip" \
  "$MANIFEST_DIR/VesperPlayerOptionalPlugins-FFmpeg-Compliance.zip" \
  "$MANIFEST_DIR/VesperPlayerOptionalPlugins-FFmpeg-8.1.2-source.tar.xz"
vesper_ios_verify_release_asset_allowlist \
  "$MANIFEST_DIR" \
  VesperPlayerOptionalPlugins-FFmpeg-8.1.2-source.tar.xz

touch "$MANIFEST_DIR/VesperPlayerRetiredPlugin.xcframework.zip"
if failure_output="$(
  vesper_ios_verify_release_asset_allowlist \
    "$MANIFEST_DIR" \
    VesperPlayerOptionalPlugins-FFmpeg-8.1.2-source.tar.xz 2>&1
)"; then
  echo "A stale optional framework release asset was incorrectly accepted." >&2
  exit 1
fi
if [[ "$failure_output" != *"Unexpected top-level iOS release asset"* ]]; then
  echo "The stale optional framework asset failed for an unexpected reason:" >&2
  echo "$failure_output" >&2
  exit 1
fi
rm -f "$MANIFEST_DIR/VesperPlayerRetiredPlugin.xcframework.zip"

touch "$MANIFEST_DIR/FFmpeg-older-source.tar.xz"
if failure_output="$(
  vesper_ios_verify_release_asset_allowlist \
    "$MANIFEST_DIR" \
    VesperPlayerOptionalPlugins-FFmpeg-8.1.2-source.tar.xz 2>&1
)"; then
  echo "An unrelated source tarball was incorrectly accepted." >&2
  exit 1
fi
if [[ "$failure_output" != *"Unexpected top-level iOS release asset"* ]]; then
  echo "The unrelated source tarball failed for an unexpected reason:" >&2
  echo "$failure_output" >&2
  exit 1
fi

forced_rebuild_value="$(
  vesper_ios_run_forced_ffmpeg_release_build \
    /bin/sh -c 'printf %s "$VESPER_APPLE_FFMPEG_FORCE"'
)"
if [[ "$forced_rebuild_value" != "1" ]]; then
  echo "The iOS FFmpeg release build did not force a source rebuild." >&2
  exit 1
fi

echo "Verified iOS framework platform, dependency, and release-asset validation."
