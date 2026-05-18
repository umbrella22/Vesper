#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
STAGE_DIR="${1:-$ROOT_DIR/dist/release/flutter-pub}"
VERSION="${2:-}"

"$ROOT_DIR/scripts/flutter/stage-pub-packages.sh" "$STAGE_DIR" ${VERSION:+"$VERSION"}

packages=(
  vesper_player_platform_interface
  vesper_player_android
  vesper_player_ios
  vesper_player_macos
  vesper_player
  vesper_player_external_playback
  vesper_player_ui
)

write_overrides() {
  local package="$1"
  local overrides_file="$STAGE_DIR/$package/pubspec_overrides.yaml"
  local dependency

  {
    echo "dependency_overrides:"
    for dependency in "${packages[@]}"; do
      if [[ "$dependency" == "$package" ]]; then
        continue
      fi

      cat <<EOF
  $dependency:
    path: ../$dependency
EOF
    done
  } >"$overrides_file"
}

for package in "${packages[@]}"; do
  write_overrides "$package"

  echo "::group::flutter pub publish --dry-run $package"
  (
    cd "$STAGE_DIR/$package"
    flutter pub get
    flutter pub publish --dry-run
  )
  echo "::endgroup::"
done
