#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/apple.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg-profile.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg-validate.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
FRAMEWORK_BUNDLE_NAME="${1:-}"
RUNTIME_FRAMEWORK_NAME="VesperPlayerFfmpegRuntime.framework"
PLUGIN_FRAMEWORK_NAME="VesperPlayerRemuxFfmpegPlugin.framework"
SOURCE_NORMALIZER_FRAMEWORK_NAME="VesperPlayerSourceNormalizerFfmpegPlugin.framework"
FRAME_PROCESSOR_FRAMEWORK_NAME="VesperPlayerFrameProcessorDiagnosticPlugin.framework"
RUNTIME_BINARY_NAME="VesperPlayerFfmpegRuntime"
PLUGIN_BINARY_NAME="VesperPlayerRemuxFfmpegPlugin"
SOURCE_NORMALIZER_BINARY_NAME="VesperPlayerSourceNormalizerFfmpegPlugin"
FRAME_PROCESSOR_BINARY_NAME="VesperPlayerFrameProcessorDiagnosticPlugin"
FFMPEG_PROFILE="${VESPER_IOS_FFMPEG_PROFILE:-${VESPER_APPLE_FFMPEG_PROFILE:-${VESPER_FFMPEG_PROFILE:-default}}}"

if [[ -z "$FRAMEWORK_BUNDLE_NAME" ]]; then
  echo "Usage: $0 <framework-bundle-name>" >&2
  exit 1
fi

if [[ -z "${TARGET_BUILD_DIR:-}" || -z "${FRAMEWORKS_FOLDER_PATH:-}" ]]; then
  echo "TARGET_BUILD_DIR and FRAMEWORKS_FOLDER_PATH are required." >&2
  exit 1
fi

build_profile="debug"
case "${CONFIGURATION:-Debug}" in
  *Release*)
    build_profile="release"
    ;;
esac

selected_slices=()
source_subdir=""
case "${PLATFORM_NAME:-}" in
  iphoneos)
    selected_slices=("ios-arm64")
    source_subdir="iphoneos"
    ;;
  iphonesimulator)
    source_subdir="iphonesimulator"
    arch_tokens="${ARCHS:-${CURRENT_ARCH:-${NATIVE_ARCH_ACTUAL:-arm64}}}"
    for arch in $arch_tokens; do
      case "$arch" in
        arm64)
          if [[ ! " ${selected_slices[*]-} " =~ " ios-simulator-arm64 " ]]; then
            selected_slices+=("ios-simulator-arm64")
          fi
          ;;
      esac
    done
    if [[ ${#selected_slices[@]} -eq 0 ]]; then
      selected_slices=("ios-simulator-arm64")
    fi
    ;;
  *)
    echo "Skipping player-remux-ffmpeg embed for unsupported platform: ${PLATFORM_NAME:-unknown}" >&2
    exit 0
    ;;
esac

resolve_frameworks_directory() {
  local frameworks_root="$TARGET_BUILD_DIR/$FRAMEWORKS_FOLDER_PATH"

  mkdir -p "$frameworks_root"
  echo "$frameworks_root"
}

resolve_plugin_library_directory() {
  local built_products_dir="${BUILT_PRODUCTS_DIR:-$TARGET_BUILD_DIR}"
  local frameworks_root="$TARGET_BUILD_DIR/$FRAMEWORKS_FOLDER_PATH"
  local candidate

  for candidate in \
    "$TARGET_BUILD_DIR/$FRAMEWORKS_FOLDER_PATH/$FRAMEWORK_BUNDLE_NAME" \
    "$TARGET_BUILD_DIR/PackageFrameworks/$FRAMEWORK_BUNDLE_NAME" \
    "$built_products_dir/PackageFrameworks/$FRAMEWORK_BUNDLE_NAME" \
    "$built_products_dir/$FRAMEWORK_BUNDLE_NAME" \
    "$TARGET_BUILD_DIR/$FRAMEWORK_BUNDLE_NAME"; do
    if [[ -d "$candidate" ]]; then
      echo "$candidate"
      return 0
    fi
  done

  mkdir -p "$frameworks_root"
  echo "$frameworks_root"
}

framework_info_plist() {
  local output_path="$1"
  local executable_name="$2"
  local bundle_identifier="$3"
  local platform_name="$4"
  local minimum_os_version="$5"

  /usr/libexec/PlistBuddy -c "Clear dict" "$output_path" >/dev/null 2>&1 || true
  /usr/libexec/PlistBuddy -c "Add :CFBundleDevelopmentRegion string en" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleExecutable string $executable_name" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleIdentifier string $bundle_identifier" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleInfoDictionaryVersion string 6.0" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleName string $executable_name" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundlePackageType string FMWK" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleShortVersionString string ${MARKETING_VERSION:-0.0.0}" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleSupportedPlatforms array" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleSupportedPlatforms:0 string $platform_name" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleVersion string ${CURRENT_PROJECT_VERSION:-1}" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :MinimumOSVersion string $minimum_os_version" "$output_path"
}

ensure_rpath() {
  local binary_path="$1"
  local rpath="$2"

  if ! otool -l "$binary_path" | grep -Fq "$rpath"; then
    install_name_tool -add_rpath "$rpath" "$binary_path"
  fi
}

normalize_runtime_dylib() {
  local binary_path="$1"
  local current_id
  local dylib_id

  current_id="$(otool -D "$binary_path" | tail -n 1)"
  dylib_id="${current_id##*/}"
  if [[ -z "$dylib_id" || "$dylib_id" != lib*.dylib* ]]; then
    dylib_id="$(basename "$binary_path")"
  fi
  install_name_tool -id "@rpath/$dylib_id" "$binary_path"
  ensure_rpath "$binary_path" "@loader_path"
}

compile_runtime_anchor() {
  local slice="$1"
  local output_path="$2"
  local source_path="$3"
  local sdk_name
  local clang_target

  sdk_name="$(vesper_apple_slice_sdk "$slice")"
  clang_target="$(vesper_apple_slice_clang_target "$slice" "$(vesper_apple_ios_deployment_target)")"
  printf '%s\n' 'void VesperPlayerFfmpegRuntimeLinkAnchor(void) {}' >"$source_path"
  xcrun --sdk "$sdk_name" clang \
    -target "$clang_target" \
    -dynamiclib \
    -install_name "@rpath/$RUNTIME_FRAMEWORK_NAME/$RUNTIME_BINARY_NAME" \
    "$source_path" \
    -o "$output_path"
  ensure_rpath "$output_path" "@loader_path/Frameworks"
}

copy_runtime_dylibs() {
  local source_dir="$1"
  local framework_dir="$2"
  local runtime_dir="$framework_dir/Frameworks"
  local copied_count=0
  local runtime_binary

  mkdir -p "$runtime_dir"
  while IFS= read -r runtime_binary; do
    cp -RP "$runtime_binary" "$runtime_dir/"
    copied_count=$((copied_count + 1))
  done < <(find "$source_dir" -maxdepth 1 \( -type f -o -type l \) -name 'lib*.dylib*' | sort)

  if [[ "$copied_count" -eq 0 ]]; then
    echo "Missing FFmpeg runtime dylibs in: $source_dir" >&2
    exit 1
  fi

  while IFS= read -r runtime_binary; do
    normalize_runtime_dylib "$runtime_binary"
  done < <(find "$runtime_dir" -maxdepth 1 -type f -name 'lib*.dylib*' | sort)
}

resolve_ffmpeg_args() {
  local platform="ios"
  local protocols_csv
  local validation_args=()
  local restore_nounset=0

  vesper_ffmpeg_profile_resolve "$FFMPEG_PROFILE" "$platform"
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

resolve_codesign_entitlements() {
  local candidates=()
  local candidate

  if [[ -n "${EXPANDED_CODE_SIGN_ENTITLEMENTS:-}" ]]; then
    candidates+=("$EXPANDED_CODE_SIGN_ENTITLEMENTS")
  fi
  if [[ -n "${TARGET_TEMP_DIR:-}" ]]; then
    if [[ -n "${FULL_PRODUCT_NAME:-}" ]]; then
      candidates+=("$TARGET_TEMP_DIR/$FULL_PRODUCT_NAME.xcent")
    fi
    if [[ -n "${PRODUCT_NAME:-}" ]]; then
      candidates+=("$TARGET_TEMP_DIR/$PRODUCT_NAME.app.xcent")
      candidates+=("$TARGET_TEMP_DIR/$PRODUCT_NAME.xcent")
    fi
  fi

  for candidate in "${candidates[@]}"; do
    if [[ "$candidate" != /* ]]; then
      candidate="${PROJECT_DIR:-$PWD}/$candidate"
    fi
    if [[ -f "$candidate" ]]; then
      echo "$candidate"
      return 0
    fi
  done

  return 1
}

sign_path() {
  local path="$1"

  if [[ "${CODE_SIGNING_ALLOWED:-NO}" == "NO" ]]; then
    return 0
  fi

  codesign --force --sign "${EXPANDED_CODE_SIGN_IDENTITY:--}" --timestamp=none "$path"
}

frameworks_directory="$(resolve_frameworks_directory)"
plugin_library_directory="$(resolve_plugin_library_directory)"
runtime_framework_dir="$frameworks_directory/$RUNTIME_FRAMEWORK_NAME"
plugin_framework_dir="$frameworks_directory/$PLUGIN_FRAMEWORK_NAME"
source_normalizer_framework_dir="$frameworks_directory/$SOURCE_NORMALIZER_FRAMEWORK_NAME"
frame_processor_framework_dir="$frameworks_directory/$FRAME_PROCESSOR_FRAMEWORK_NAME"
output_dir="${DERIVED_FILE_DIR:-${TARGET_TEMP_DIR:-/tmp}}/vesper-ios-player-remux-ffmpeg"
source_normalizer_output_dir="${DERIVED_FILE_DIR:-${TARGET_TEMP_DIR:-/tmp}}/vesper-ios-player-source-normalizer-ffmpeg"
frame_processor_output_dir="${DERIVED_FILE_DIR:-${TARGET_TEMP_DIR:-/tmp}}/vesper-ios-player-frame-processor-diagnostic"
runtime_anchor_source="$output_dir/runtime-anchor.c"

FFMPEG_ARGS=()
while IFS= read -r arg; do
  FFMPEG_ARGS+=("$arg")
done < <(resolve_ffmpeg_args)
vesper_ffmpeg_parse_common_args apple "${FFMPEG_ARGS[@]}"
ffmpeg_apple_dir="${VESPER_APPLE_FFMPEG_OUTPUT_DIR:-${VESPER_FFMPEG_OUTPUT_DIR:-$(vesper_ffmpeg_default_output_dir apple "$ROOT_DIR/third_party/ffmpeg/apple")}}"
profile_hash="$(vesper_ffmpeg_profile_key apple)"

export VESPER_DECLARED_FFMPEG_PROFILE="$FFMPEG_PROFILE"
export VESPER_DECLARED_FFMPEG_PLATFORM="ios"
"$ROOT_DIR/scripts/apple/build-ffmpeg-prebuilts.sh" \
  "${FFMPEG_ARGS[@]}" \
  "${selected_slices[@]}"
env \
  VESPER_SKIP_APPLE_FFMPEG_PREBUILDS=1 \
  "$ROOT_DIR/scripts/ios/build-player-remux-ffmpeg-plugin.sh" \
    "$output_dir" \
    "$build_profile" \
    "${FFMPEG_ARGS[@]}" \
    "${selected_slices[@]}"
env \
  VESPER_SKIP_APPLE_FFMPEG_PREBUILDS=1 \
  "$ROOT_DIR/scripts/ios/build-player-source-normalizer-ffmpeg-plugin.sh" \
    "$source_normalizer_output_dir" \
    "$build_profile" \
    "${FFMPEG_ARGS[@]}" \
    "${selected_slices[@]}"
"$ROOT_DIR/scripts/ios/build-player-frame-processor-diagnostic-plugin.sh" \
  "$frame_processor_output_dir" \
  "$build_profile" \
  "${selected_slices[@]}"

source_dir="$output_dir/$source_subdir"
source_normalizer_source_dir="$source_normalizer_output_dir/$source_subdir"
frame_processor_source_dir="$frame_processor_output_dir/$source_subdir"
if [[ ! -f "$source_dir/libvesper_remux_ffmpeg.dylib" ]]; then
  echo "Expected player-remux-ffmpeg output was not found: $source_dir/libvesper_remux_ffmpeg.dylib" >&2
  exit 1
fi
if [[ ! -f "$source_normalizer_source_dir/libplayer_source_normalizer_ffmpeg.dylib" ]]; then
  echo "Expected player-source-normalizer-ffmpeg output was not found: $source_normalizer_source_dir/libplayer_source_normalizer_ffmpeg.dylib" >&2
  exit 1
fi
if [[ ! -f "$frame_processor_source_dir/libplayer_frame_processor_diagnostic.dylib" ]]; then
  echo "Expected player-frame-processor-diagnostic output was not found: $frame_processor_source_dir/libplayer_frame_processor_diagnostic.dylib" >&2
  exit 1
fi

case "${PLATFORM_NAME:-}" in
  iphoneos)
    slice="ios-arm64"
    platform_name="iPhoneOS"
    ;;
  iphonesimulator)
    slice="ios-simulator-arm64"
    platform_name="iPhoneSimulator"
    ;;
esac

ffmpeg_dir="$(vesper_apple_slice_output_root "$slice" "$ffmpeg_apple_dir")"
ffmpeg_libdir="$(vesper_apple_slice_output_libdir "$slice")"
metadata_path="$ffmpeg_dir/vesper-ffmpeg-build-metadata.txt"
if [[ ! -f "$metadata_path" ]]; then
  echo "Missing FFmpeg build metadata for $slice: $metadata_path" >&2
  exit 1
fi

rm -rf \
  "$runtime_framework_dir" \
  "$plugin_framework_dir" \
  "$source_normalizer_framework_dir" \
  "$frame_processor_framework_dir"
mkdir -p \
  "$runtime_framework_dir/Headers" \
  "$runtime_framework_dir/Modules" \
  "$runtime_framework_dir/Resources" \
  "$plugin_framework_dir/Headers" \
  "$plugin_framework_dir/Modules" \
  "$plugin_framework_dir/Resources" \
  "$source_normalizer_framework_dir/Headers" \
  "$source_normalizer_framework_dir/Modules" \
  "$source_normalizer_framework_dir/Resources" \
  "$frame_processor_framework_dir/Headers" \
  "$frame_processor_framework_dir/Modules" \
  "$frame_processor_framework_dir/Resources"

compile_runtime_anchor "$slice" "$runtime_framework_dir/$RUNTIME_BINARY_NAME" "$runtime_anchor_source"
copy_runtime_dylibs "$ffmpeg_dir/lib/$ffmpeg_libdir" "$runtime_framework_dir"
cp "$metadata_path" "$runtime_framework_dir/Resources/$slice-vesper-ffmpeg-build-metadata.txt"
printf '%s\n' "$profile_hash" >"$runtime_framework_dir/Resources/profile-hash.txt"
printf '%s\n' 'void VesperPlayerFfmpegRuntimeLinkAnchor(void);' >"$runtime_framework_dir/Headers/VesperPlayerFfmpegRuntime.h"
printf '%s\n' \
  'framework module VesperPlayerFfmpegRuntime {' \
  '  umbrella header "VesperPlayerFfmpegRuntime.h"' \
  '  export *' \
  '  module * { export * }' \
  '}' \
  >"$runtime_framework_dir/Modules/module.modulemap"
framework_info_plist \
  "$runtime_framework_dir/Info.plist" \
  "$RUNTIME_BINARY_NAME" \
  "io.github.ikaros.vesper.player.ffmpeg-runtime" \
  "$platform_name" \
  "$(vesper_apple_ios_deployment_target)"

cp "$source_dir/libvesper_remux_ffmpeg.dylib" "$plugin_framework_dir/$PLUGIN_BINARY_NAME"
install_name_tool -id "@rpath/$PLUGIN_FRAMEWORK_NAME/$PLUGIN_BINARY_NAME" "$plugin_framework_dir/$PLUGIN_BINARY_NAME"
ensure_rpath "$plugin_framework_dir/$PLUGIN_BINARY_NAME" "@loader_path/../VesperPlayerFfmpegRuntime.framework/Frameworks"
cp "$metadata_path" "$plugin_framework_dir/Resources/$slice-vesper-ffmpeg-build-metadata.txt"
printf '%s\n' "$profile_hash" >"$plugin_framework_dir/Resources/profile-hash.txt"
printf '%s\n' 'void VesperPlayerRemuxFfmpegPluginLinkAnchor(void);' >"$plugin_framework_dir/Headers/VesperPlayerRemuxFfmpegPlugin.h"
printf '%s\n' \
  'framework module VesperPlayerRemuxFfmpegPlugin {' \
  '  umbrella header "VesperPlayerRemuxFfmpegPlugin.h"' \
  '  export *' \
  '  module * { export * }' \
  '}' \
  >"$plugin_framework_dir/Modules/module.modulemap"
framework_info_plist \
  "$plugin_framework_dir/Info.plist" \
  "$PLUGIN_BINARY_NAME" \
  "io.github.ikaros.vesper.player.remux-ffmpeg-plugin" \
  "$platform_name" \
  "$(vesper_apple_ios_deployment_target)"

cp "$source_normalizer_source_dir/libplayer_source_normalizer_ffmpeg.dylib" "$source_normalizer_framework_dir/$SOURCE_NORMALIZER_BINARY_NAME"
install_name_tool -id "@rpath/$SOURCE_NORMALIZER_FRAMEWORK_NAME/$SOURCE_NORMALIZER_BINARY_NAME" "$source_normalizer_framework_dir/$SOURCE_NORMALIZER_BINARY_NAME"
ensure_rpath "$source_normalizer_framework_dir/$SOURCE_NORMALIZER_BINARY_NAME" "@loader_path/../VesperPlayerFfmpegRuntime.framework/Frameworks"
cp "$metadata_path" "$source_normalizer_framework_dir/Resources/$slice-vesper-ffmpeg-build-metadata.txt"
printf '%s\n' "$profile_hash" >"$source_normalizer_framework_dir/Resources/profile-hash.txt"
printf '%s\n' 'void VesperPlayerSourceNormalizerFfmpegPluginLinkAnchor(void);' >"$source_normalizer_framework_dir/Headers/VesperPlayerSourceNormalizerFfmpegPlugin.h"
printf '%s\n' \
  'framework module VesperPlayerSourceNormalizerFfmpegPlugin {' \
  '  umbrella header "VesperPlayerSourceNormalizerFfmpegPlugin.h"' \
  '  export *' \
  '  module * { export * }' \
  '}' \
  >"$source_normalizer_framework_dir/Modules/module.modulemap"
framework_info_plist \
  "$source_normalizer_framework_dir/Info.plist" \
  "$SOURCE_NORMALIZER_BINARY_NAME" \
  "io.github.ikaros.vesper.player.source-normalizer-ffmpeg-plugin" \
  "$platform_name" \
  "$(vesper_apple_ios_deployment_target)"

cp "$frame_processor_source_dir/libplayer_frame_processor_diagnostic.dylib" "$frame_processor_framework_dir/$FRAME_PROCESSOR_BINARY_NAME"
install_name_tool -id "@rpath/$FRAME_PROCESSOR_FRAMEWORK_NAME/$FRAME_PROCESSOR_BINARY_NAME" "$frame_processor_framework_dir/$FRAME_PROCESSOR_BINARY_NAME"
printf '%s\n' 'void VesperPlayerFrameProcessorDiagnosticPluginLinkAnchor(void);' >"$frame_processor_framework_dir/Headers/VesperPlayerFrameProcessorDiagnosticPlugin.h"
printf '%s\n' \
  'framework module VesperPlayerFrameProcessorDiagnosticPlugin {' \
  '  umbrella header "VesperPlayerFrameProcessorDiagnosticPlugin.h"' \
  '  export *' \
  '  module * { export * }' \
  '}' \
  >"$frame_processor_framework_dir/Modules/module.modulemap"
framework_info_plist \
  "$frame_processor_framework_dir/Info.plist" \
  "$FRAME_PROCESSOR_BINARY_NAME" \
  "io.github.ikaros.vesper.player.frame-processor-diagnostic-plugin" \
  "$platform_name" \
  "$(vesper_apple_ios_deployment_target)"

if [[ "$plugin_library_directory" != "$frameworks_directory" && "$plugin_library_directory" != "$plugin_framework_dir" ]]; then
  ln -sfn "$plugin_framework_dir/$PLUGIN_BINARY_NAME" "$plugin_library_directory/libvesper_remux_ffmpeg.dylib"
fi

if [[ "${CODE_SIGNING_ALLOWED:-NO}" != "NO" ]]; then
  while IFS= read -r runtime_binary; do
    sign_path "$runtime_binary"
  done < <(find "$runtime_framework_dir/Frameworks" -maxdepth 1 -type f -name 'lib*.dylib*' | sort)
  sign_path "$runtime_framework_dir"
  sign_path "$plugin_framework_dir"
  sign_path "$source_normalizer_framework_dir"
  sign_path "$frame_processor_framework_dir"
  if [[ "$plugin_library_directory" == *.framework && "$plugin_library_directory" != "$plugin_framework_dir" ]]; then
    sign_path "$plugin_library_directory"
  fi

  if [[ -n "${CODESIGNING_FOLDER_PATH:-}" && -d "${CODESIGNING_FOLDER_PATH:-}" ]]; then
    app_codesign_args=(--force --sign "${EXPANDED_CODE_SIGN_IDENTITY:--}" --timestamp=none)
    app_entitlements="$(resolve_codesign_entitlements || true)"
    if [[ -n "$app_entitlements" ]]; then
      app_codesign_args+=(--entitlements "$app_entitlements" --generate-entitlement-der)
    fi
    codesign "${app_codesign_args[@]}" "$CODESIGNING_FOLDER_PATH"
  fi
fi

echo "Embedded VesperPlayerFfmpegRuntime, remux, SourceNormalizer, and FrameProcessor diagnostic plugins into $frameworks_directory"
