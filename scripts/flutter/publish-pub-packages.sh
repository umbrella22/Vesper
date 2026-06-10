#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
STAGE_DIR="${1:-$ROOT_DIR/dist/release/flutter-pub}"
VERSION="${2:-}"

"$ROOT_DIR/scripts/flutter/stage-pub-packages.sh" "$STAGE_DIR" ${VERSION:+"$VERSION"}

core_packages=(
  vesper_player_platform_interface
  vesper_player_android
  vesper_player_ios
  vesper_player
  vesper_player_external_playback
  vesper_player_ui
)
optional_plugin_packages=(
  vesper_player_source_normalizer_ffmpeg
)
packages=("${core_packages[@]}")

case "${VESPER_FLUTTER_INCLUDE_OPTIONAL_PLUGINS:-0}" in
  1|true|TRUE|yes|YES)
    packages+=("${optional_plugin_packages[@]}")
    ;;
esac

pub_get_with_retry() {
  local attempt

  for attempt in 1 2 3 4 5 6; do
    if flutter pub get; then
      return 0
    fi

    echo "flutter pub get failed on attempt $attempt; waiting for pub.dev package index propagation." >&2
    sleep 20
  done

  flutter pub get
}

for package in "${packages[@]}"; do
  echo "::group::flutter pub publish $package"
  (
    cd "$STAGE_DIR/$package"
    pub_get_with_retry
    flutter pub publish --force
  )
  echo "::endgroup::"
done
