#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"

ROOT_DIR="$VESPER_REPO_ROOT"

packages=(
  vesper_player_platform_interface
  vesper_player_android
  vesper_player_ios
  vesper_player_macos
  vesper_player
  vesper_player_external_playback
  vesper_player_ui
)

write_package_overrides() {
  local package="$1"
  local package_dir="$ROOT_DIR/lib/flutter/$package"
  local overrides_file="$package_dir/pubspec_overrides.yaml"
  local dependency

  if [[ ! -f "$package_dir/pubspec.yaml" ]]; then
    echo "Missing Flutter package pubspec: $package_dir/pubspec.yaml" >&2
    exit 1
  fi

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

write_example_overrides() {
  local example_dir="$ROOT_DIR/examples/flutter-host"
  local overrides_file="$example_dir/pubspec_overrides.yaml"
  local dependency

  if [[ ! -f "$example_dir/pubspec.yaml" ]]; then
    echo "Missing Flutter example pubspec: $example_dir/pubspec.yaml" >&2
    exit 1
  fi

  {
    echo "dependency_overrides:"
    for dependency in "${packages[@]}"; do
      cat <<EOF
  $dependency:
    path: ../../lib/flutter/$dependency
EOF
    done
  } >"$overrides_file"
}

for package in "${packages[@]}"; do
  write_package_overrides "$package"
done

write_example_overrides

echo "Wrote local Flutter pubspec_overrides.yaml files."
printf '  lib/flutter/%s/pubspec_overrides.yaml\n' "${packages[@]}"
echo "  examples/flutter-host/pubspec_overrides.yaml"