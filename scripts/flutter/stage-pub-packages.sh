#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
OUTPUT_DIR="${1:-$ROOT_DIR/dist/release/flutter-pub}"
VERSION="${2:-}"

if [[ -z "$VERSION" ]]; then
  VERSION="$(sed -n 's/^version: //p' "$ROOT_DIR/lib/flutter/vesper_player/pubspec.yaml" | head -n 1)"
fi

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][A-Za-z0-9.-]+)?$ ]]; then
  echo "Unable to resolve a valid Flutter package version: $VERSION" >&2
  exit 1
fi

packages=(
  vesper_player_platform_interface
  vesper_player_android
  vesper_player_ios
  vesper_player_macos
  vesper_player
  vesper_player_external_playback
  vesper_player_ui
)

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

rewrite_pubspec() {
  local pubspec="$1"

  perl -0pi -e 's/^publish_to:\s*none\n//m; s/^publish_to:\s*'\''none'\''\n//m; s/^publish_to:\s*"none"\n//m' "$pubspec"
  perl -0pi -e "s{^version: .*}{version: $VERSION}m" "$pubspec"
  perl -0pi -e 's/^repository:.*\n//m; s/^issue_tracker:.*\n//m' "$pubspec"
  perl -0pi -e "s{^homepage:.*}{homepage: https://github.com/umbrella22/Vesper\nrepository: https://github.com/umbrella22/Vesper\nissue_tracker: https://github.com/umbrella22/Vesper/issues}m" "$pubspec"

  for package in "${packages[@]}"; do
    perl -0pi -e "s{^  $package:\\n    path: \\.\\./$package\\n}{  $package: ^$VERSION\n}mg" "$pubspec"
    perl -0pi -e "s{^  $package: \\^[0-9]+\\.[0-9]+\\.[0-9]+(?:[+-][A-Za-z0-9.-]+)?\\n}{  $package: ^$VERSION\n}mg" "$pubspec"
  done
}

for package in "${packages[@]}"; do
  source_dir="$ROOT_DIR/lib/flutter/$package"
  stage_dir="$OUTPUT_DIR/$package"

  if [[ ! -f "$source_dir/pubspec.yaml" ]]; then
    echo "Missing Flutter package pubspec: $source_dir/pubspec.yaml" >&2
    exit 1
  fi

  mkdir -p "$(dirname "$stage_dir")"
  rsync -a \
    --exclude '.dart_tool' \
    --exclude 'build' \
    --exclude 'pubspec.lock' \
    --exclude 'pubspec_overrides.yaml' \
    "$source_dir/" "$stage_dir/"

  cp "$ROOT_DIR/LICENSE" "$stage_dir/LICENSE"
  rewrite_pubspec "$stage_dir/pubspec.yaml"
done

echo "Staged Flutter pub packages into:"
echo "  $OUTPUT_DIR"
printf '  %s\n' "${packages[@]}"
