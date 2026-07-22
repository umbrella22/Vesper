#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ios-framework.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ios-release.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROJECT_DIR="$ROOT_DIR/lib/ios/VesperPlayerKit"
OUTPUT_DIR="$ROOT_DIR/dist/release/ios"
PACKAGE_DIR="$ROOT_DIR/lib/ios/VesperPlayerOptionalPlugins"
PACKAGE_ARTIFACTS_DIR="${VESPER_IOS_OPTIONAL_PACKAGE_ARTIFACTS_DIR:-$PACKAGE_DIR/Artifacts}"
PROFILE="source-normalizer"
DRY_RUN=0
SELECTED_SLICES=()

RUNTIME_FRAMEWORKS=(
  VesperFFmpegAVCodec
  VesperFFmpegAVFormat
  VesperFFmpegAVUtil
)

PLUGIN_FRAMEWORKS=()
for plugin_id in "${VESPER_IOS_OPTIONAL_PLUGIN_IDS[@]}"; do
  vesper_ios_resolve_plugin "$plugin_id"
  PLUGIN_FRAMEWORKS+=("$VESPER_IOS_PLUGIN_FRAMEWORK")
done

usage() {
  cat <<EOF >&2
Usage: $0 [output-dir] [options] [ios-arm64] [ios-simulator-arm64]

Options:
  --profile <name>   Shared FFmpeg profile (default: source-normalizer)
  --dry-run          Print the composite staging plan without building
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
      echo "Unknown iOS optional plugin staging option: $1" >&2
      usage
      exit 1
      ;;
  esac
done

if ! resolved_slices="$(vesper_apple_resolve_selected_slices ${SELECTED_SLICES[@]+"${SELECTED_SLICES[@]}"})"; then
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
    echo "The distributable optional plugin package requires an ios-arm64 device slice." >&2
    exit 1
    ;;
esac

runtime_args=(
  "$OUTPUT_DIR"
  --profile "$PROFILE"
)
if [[ "$DRY_RUN" == "1" ]]; then
  runtime_args+=(--dry-run)
fi
runtime_args+=("${SELECTED_SLICES[@]}")

if [[ "$DRY_RUN" == "0" ]]; then
  mkdir -p "$OUTPUT_DIR"
  vesper_ios_remove_optional_release_assets "$OUTPUT_DIR"
fi

env \
  VESPER_APPLE_FFMPEG_VERSION="$VESPER_IOS_OPTIONAL_FFMPEG_VERSION" \
  VESPER_APPLE_FFMPEG_SOURCE_URL="$VESPER_IOS_OPTIONAL_FFMPEG_SOURCE_URL" \
  VESPER_APPLE_FFMPEG_EXPECTED_SOURCE_SHA256="$VESPER_IOS_OPTIONAL_FFMPEG_SOURCE_SHA256" \
  "$ROOT_DIR/scripts/ios/stage-player-ffmpeg-runtime-release.sh" \
    "${runtime_args[@]}"

for plugin_id in "${VESPER_IOS_OPTIONAL_PLUGIN_IDS[@]}"; do
  vesper_ios_resolve_plugin "$plugin_id"
  plugin_args=("$plugin_id" "$OUTPUT_DIR")
  if [[ "$VESPER_IOS_PLUGIN_USES_FFMPEG" == "1" ]]; then
    plugin_args+=(--profile "$PROFILE")
  fi
  if [[ "$DRY_RUN" == "1" ]]; then
    plugin_args+=(--dry-run)
  fi
  plugin_args+=("${SELECTED_SLICES[@]}")

  if [[ "$DRY_RUN" == "0" && "$VESPER_IOS_PLUGIN_USES_FFMPEG" == "1" ]]; then
    env VESPER_SKIP_IOS_FFMPEG_RUNTIME_STAGE=1 \
      "$ROOT_DIR/scripts/ios/stage-player-plugin-release.sh" \
        "${plugin_args[@]}"
  else
    "$ROOT_DIR/scripts/ios/stage-player-plugin-release.sh" \
      "${plugin_args[@]}"
  fi
done

if [[ "$DRY_RUN" == "1" ]]; then
  echo "Canonical local package artifacts:"
  for framework_name in "${RUNTIME_FRAMEWORKS[@]}" "${PLUGIN_FRAMEWORKS[@]}"; do
    echo "  $PACKAGE_ARTIFACTS_DIR/$framework_name.xcframework"
  done
  echo "Compliance release assets:"
  echo "  $OUTPUT_DIR/VesperPlayerOptionalPlugins-FFmpeg-Compliance.zip"
  echo "  $OUTPUT_DIR/VesperPlayerOptionalPlugins-FFmpeg-<version>-source.tar.xz"
  exit 0
fi

copy_xcframework() {
  local framework_name="$1"
  local source_path="$2"
  local destination_path="$PACKAGE_ARTIFACTS_DIR/$framework_name.xcframework"

  if [[ ! -d "$source_path" ]]; then
    echo "Missing staged XCFramework for the optional package:" >&2
    echo "  $source_path" >&2
    exit 1
  fi

  rm -rf "$destination_path"
  ditto "$source_path" "$destination_path"
}

vesper_require_command ditto
rm -rf "$PACKAGE_ARTIFACTS_DIR"
mkdir -p "$PACKAGE_ARTIFACTS_DIR"

for framework_name in "${RUNTIME_FRAMEWORKS[@]}"; do
  copy_xcframework \
    "$framework_name" \
    "$PROJECT_DIR/.build/player-ffmpeg-runtime/$framework_name.xcframework"
done

for plugin_id in "${VESPER_IOS_OPTIONAL_PLUGIN_IDS[@]}"; do
  vesper_ios_resolve_plugin "$plugin_id"
  copy_xcframework \
    "$VESPER_IOS_PLUGIN_FRAMEWORK" \
    "$PROJECT_DIR/.build/$VESPER_IOS_PLUGIN_BUILD_DIRECTORY/$VESPER_IOS_PLUGIN_FRAMEWORK.xcframework"
done

echo "Staged the canonical local iOS optional plugin package artifacts:"
for framework_name in "${RUNTIME_FRAMEWORKS[@]}" "${PLUGIN_FRAMEWORKS[@]}"; do
  echo "  $PACKAGE_ARTIFACTS_DIR/$framework_name.xcframework"
done

"$ROOT_DIR/scripts/ios/stage-player-optional-plugins-compliance-release.sh" \
  "$OUTPUT_DIR"
