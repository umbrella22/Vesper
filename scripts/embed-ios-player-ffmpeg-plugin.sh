#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
FRAMEWORK_BUNDLE_NAME="${1:-}"

if [[ -z "$FRAMEWORK_BUNDLE_NAME" ]]; then
  echo "Usage: $0 <framework-bundle-name>" >&2
  exit 1
fi

if [[ -z "${TARGET_BUILD_DIR:-}" || -z "${FRAMEWORKS_FOLDER_PATH:-}" ]]; then
  echo "TARGET_BUILD_DIR and FRAMEWORKS_FOLDER_PATH are required." >&2
  exit 1
fi

resolve_destination_directory() {
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

DESTINATION_DIRECTORY="$(resolve_destination_directory)"

PROFILE="debug"
case "${CONFIGURATION:-Debug}" in
  *Release*)
    PROFILE="release"
    ;;
esac

selected_slices=()
case "${PLATFORM_NAME:-}" in
  iphoneos)
    selected_slices=("ios-arm64")
    source_subdir="iphoneos"
    ;;
  iphonesimulator)
    source_subdir="iphonesimulator"
    # Apple 侧 iOS Simulator 分发固定为 arm64-only，不再跟随 x86_64。
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
    echo "Skipping player-ffmpeg embed for unsupported platform: ${PLATFORM_NAME:-unknown}" >&2
    exit 0
    ;;
esac

OUTPUT_DIR="${DERIVED_FILE_DIR:-${TARGET_TEMP_DIR:-/tmp}}/vesper-ios-player-ffmpeg"
"$ROOT_DIR/scripts/build-ios-player-ffmpeg-plugin.sh" "$OUTPUT_DIR" "$PROFILE" "${selected_slices[@]}"

SOURCE_DIR="$OUTPUT_DIR/$source_subdir"
if [[ ! -d "$SOURCE_DIR" ]]; then
  echo "Expected player-ffmpeg output directory was not found: $SOURCE_DIR" >&2
  exit 1
fi

while IFS= read -r existing_binary; do
  rm -f "$existing_binary"
done < <(
  find "$DESTINATION_DIRECTORY" -maxdepth 1 -type f \
    \( \
      -name 'libplayer_ffmpeg*.dylib*' -o \
      -name 'libavcodec*.dylib*' -o \
      -name 'libavformat*.dylib*' -o \
      -name 'libavutil*.dylib*' -o \
      -name 'libavfilter*.dylib*' -o \
      -name 'libavdevice*.dylib*' -o \
      -name 'libswresample*.dylib*' -o \
      -name 'libswscale*.dylib*' \
    \) \
    -print
)

while IFS= read -r source_binary; do
  cp -RP "$source_binary" "$DESTINATION_DIRECTORY/"
done < <(find "$SOURCE_DIR" -maxdepth 1 \( -type f -o -type l \) -name 'lib*.dylib*' | sort)

if [[ "${CODE_SIGNING_ALLOWED:-NO}" != "NO" ]]; then
  signing_identity="${EXPANDED_CODE_SIGN_IDENTITY:--}"
  while IFS= read -r copied_binary; do
    codesign --force --sign "$signing_identity" --timestamp=none "$copied_binary"
  done < <(find "$DESTINATION_DIRECTORY" -maxdepth 1 -type f -name 'lib*.dylib*' | sort)
  if [[ "$DESTINATION_DIRECTORY" == *.framework ]]; then
    codesign --force --sign "$signing_identity" --timestamp=none "$DESTINATION_DIRECTORY"
  elif [[ -n "${CODESIGNING_FOLDER_PATH:-}" && -d "${CODESIGNING_FOLDER_PATH:-}" ]]; then
    codesign --force --sign "$signing_identity" --timestamp=none "$CODESIGNING_FOLDER_PATH"
  fi
fi

echo "Embedded player-ffmpeg plugin dylibs into $DESTINATION_DIRECTORY"
