if [[ -n "${VESPER_IOS_RELEASE_SH_INCLUDED:-}" ]]; then
  return 0 2>/dev/null || exit 0
fi
VESPER_IOS_RELEASE_SH_INCLUDED=1

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/ffmpeg.sh"

VESPER_IOS_OPTIONAL_FFMPEG_VERSION="8.1.2"
VESPER_IOS_OPTIONAL_FFMPEG_SOURCE_URL="https://ffmpeg.org/releases/ffmpeg-8.1.2.tar.xz"
VESPER_IOS_OPTIONAL_FFMPEG_SOURCE_SHA256="464beb5e7bf0c311e68b45ae2f04e9cc2af88851abb4082231742a74d97b524c"

VESPER_IOS_OPTIONAL_RELEASE_FRAMEWORKS=(
  VesperFFmpegAVCodec
  VesperFFmpegAVFormat
  VesperFFmpegAVUtil
  VesperPlayerRemuxFfmpegPlugin
  VesperPlayerSourceNormalizerFfmpegPlugin
  VesperPlayerDecoderVideoToolboxPlugin
  VesperPlayerFrameProcessorDiagnosticPlugin
)

vesper_ios_run_forced_ffmpeg_release_build() {
  env VESPER_APPLE_FFMPEG_FORCE=1 "$@"
}

vesper_ios_verify_canonical_ffmpeg_source() {
  local version="$1"
  local source_url="$2"
  local source_archive="$3"
  local actual_sha256

  if [[ "$version" != "$VESPER_IOS_OPTIONAL_FFMPEG_VERSION" ]]; then
    echo "Unexpected FFmpeg version for the canonical optional iOS release: $version" >&2
    return 1
  fi
  if [[ "$source_url" != "$VESPER_IOS_OPTIONAL_FFMPEG_SOURCE_URL" ]]; then
    echo "Unexpected FFmpeg source URL for the canonical optional iOS release: $source_url" >&2
    return 1
  fi
  if [[ ! -f "$source_archive" ]]; then
    echo "Missing canonical FFmpeg source archive: $source_archive" >&2
    return 1
  fi
  actual_sha256="$(vesper_ffmpeg_sha256_file "$source_archive")" || return 1
  if [[ "$actual_sha256" != "$VESPER_IOS_OPTIONAL_FFMPEG_SOURCE_SHA256" ]]; then
    echo "Canonical optional iOS FFmpeg source SHA-256 mismatch:" >&2
    echo "  expected: $VESPER_IOS_OPTIONAL_FFMPEG_SOURCE_SHA256" >&2
    echo "  actual:   $actual_sha256" >&2
    return 1
  fi
}

vesper_ios_remove_optional_release_assets() {
  local output_dir="$1"

  [[ -d "$output_dir" ]] || return 0
  find "$output_dir" -mindepth 1 -maxdepth 1 \
    \( -type f -o -type l \) \
    \( \
      -name 'VesperFFmpeg*.xcframework.zip' -o \
      -name 'VesperPlayerFfmpegRuntime*.xcframework.zip' -o \
      -name 'VesperPlayer*Plugin*.xcframework.zip' -o \
      -name 'VesperPlayerOptionalPlugins-FFmpeg-*' \
    \) \
    -delete
}

vesper_ios_release_asset_name_is_allowed() {
  local asset_name="$1"
  local source_asset_name="$2"
  local framework_name

  case "$asset_name" in
    VesperPlayerKit-ios-arm64.framework.zip|\
      VesperPlayerKit-ios-simulator-arm64.framework.zip|\
      VesperPlayerKit.xcframework.zip|\
      VesperPlayerOptionalPlugins-FFmpeg-Compliance.zip|\
      "$source_asset_name")
      return 0
      ;;
  esac

  for framework_name in "${VESPER_IOS_OPTIONAL_RELEASE_FRAMEWORKS[@]}"; do
    if [[ "$asset_name" == "$framework_name.xcframework.zip" ]]; then
      return 0
    fi
  done
  return 1
}

vesper_ios_verify_release_asset_allowlist() {
  local output_dir="$1"
  local source_asset_name="$2"
  local asset_path
  local asset_name

  while IFS= read -r -d '' asset_path; do
    asset_name="$(basename "$asset_path")"
    if [[ ! -f "$asset_path" || -L "$asset_path" ]] || \
      ! vesper_ios_release_asset_name_is_allowed "$asset_name" "$source_asset_name"
    then
      echo "Unexpected top-level iOS release asset: $asset_path" >&2
      return 1
    fi
  done < <(find "$output_dir" -mindepth 1 -maxdepth 1 -print0)
}
