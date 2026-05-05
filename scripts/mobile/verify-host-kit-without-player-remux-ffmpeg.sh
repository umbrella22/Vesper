#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
MODE="${1:-all}"
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/vesper-player-remux-ffmpeg-verify.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

ANDROID_AARS=(
  "$ROOT_DIR/lib/android/vesper-player-kit/build/outputs/aar/vesper-player-kit-release.aar"
  "$ROOT_DIR/lib/android/vesper-player-kit-compose/build/outputs/aar/vesper-player-kit-compose-release.aar"
)
IOS_XCFRAMEWORK="$ROOT_DIR/lib/ios/VesperPlayerKit/.build/xcframework/VesperPlayerKit.xcframework"

has_ffmpeg_payload() {
  local search_root="$1"
  find "$search_root" -type f \
    \( \
      -name 'libplayer_remux_ffmpeg*.so' -o \
      -name 'libplayer_remux_ffmpeg*.dylib' -o \
      -name 'libavcodec*.so' -o \
      -name 'libavcodec*.dylib' -o \
      -name 'libavformat*.so' -o \
      -name 'libavformat*.dylib' -o \
      -name 'libavutil*.so' -o \
      -name 'libavutil*.dylib' -o \
      -name 'libavfilter*.so' -o \
      -name 'libavfilter*.dylib' -o \
      -name 'libavdevice*.so' -o \
      -name 'libavdevice*.dylib' -o \
      -name 'libswresample*.so' -o \
      -name 'libswresample*.dylib' -o \
      -name 'libswscale*.so' -o \
      -name 'libswscale*.dylib' -o \
      -name 'libssl*.so' -o \
      -name 'libssl*.dylib' -o \
      -name 'libcrypto*.so' -o \
      -name 'libcrypto*.dylib' -o \
      -name 'libxml2*.so' -o \
      -name 'libxml2*.dylib' \
    \) \
    -print -quit
}

verify_android() {
  "$ROOT_DIR/scripts/android/build-vesper-player-kit-aar.sh" assembleRelease

  for aar_path in "${ANDROID_AARS[@]}"; do
    if [[ ! -f "$aar_path" ]]; then
      echo "Expected Android AAR was not found: $aar_path" >&2
      exit 1
    fi

    local unpack_dir="$TMP_DIR/$(basename "$aar_path" .aar)"
    mkdir -p "$unpack_dir"
    unzip -q "$aar_path" -d "$unpack_dir"

    local unexpected_payload
    unexpected_payload="$(has_ffmpeg_payload "$unpack_dir" || true)"
    if [[ -n "$unexpected_payload" ]]; then
      echo "Unexpected FFmpeg payload was packaged into $aar_path:" >&2
      echo "  $unexpected_payload" >&2
      exit 1
    fi

    local size_bytes
    size_bytes="$(wc -c < "$aar_path" | tr -d '[:space:]')"
    echo "Verified Android host artifact without FFmpeg payload: $aar_path (${size_bytes} bytes)"
  done
}

verify_ios() {
  "$ROOT_DIR/scripts/ios/build-vesper-player-kit-xcframework.sh"

  if [[ ! -d "$IOS_XCFRAMEWORK" ]]; then
    echo "Expected iOS XCFramework was not found: $IOS_XCFRAMEWORK" >&2
    exit 1
  fi

  local unexpected_payload
  unexpected_payload="$(has_ffmpeg_payload "$IOS_XCFRAMEWORK" || true)"
  if [[ -n "$unexpected_payload" ]]; then
    echo "Unexpected FFmpeg payload was packaged into $IOS_XCFRAMEWORK:" >&2
    echo "  $unexpected_payload" >&2
    exit 1
  fi

  while IFS= read -r framework_binary; do
    if otool -L "$framework_binary" | grep -Eq 'player_remux_ffmpeg|libav(codec|format|util|filter|device|swresample|swscale)'; then
      echo "Unexpected FFmpeg linkage found in framework binary: $framework_binary" >&2
      otool -L "$framework_binary" >&2
      exit 1
    fi
  done < <(find "$IOS_XCFRAMEWORK" -path '*/VesperPlayerKit.framework/VesperPlayerKit' -type f | sort)

  local size_kb
  size_kb="$(du -sk "$IOS_XCFRAMEWORK" | awk '{print $1}')"
  echo "Verified iOS host artifact without FFmpeg payload: $IOS_XCFRAMEWORK (${size_kb} KB)"
}

case "$MODE" in
  android)
    verify_android
    ;;
  ios)
    verify_ios
    ;;
  all)
    verify_android
    verify_ios
    ;;
  *)
    cat <<EOF >&2
Usage: $(basename "$0") [android|ios|all]
EOF
    exit 1
    ;;
esac
