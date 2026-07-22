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

staging_excludes=(
  '.dart_tool'
  '.gradle'
  '.idea'
  '.kotlin'
  '.swiftpm'
  '.build'
  'build'
  'Pods'
  '.symlinks'
  'Flutter/ephemeral'
  'pubspec.lock'
  'pubspec_overrides.yaml'
  'local.properties'
  '*.iml'
  '*.xcworkspace'
  '*.xcuserdata'
  '*.xcuserstate'
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

validate_staged_package() {
  local package_dir="$1"
  local leaked_paths

  leaked_paths="$(
    find "$package_dir" \
      \( \
        -type d \( \
          -name '.dart_tool' -o \
          -name '.gradle' -o \
          -name '.idea' -o \
          -name '.kotlin' -o \
          -name '.swiftpm' -o \
          -name '.build' -o \
          -name 'build' -o \
          -name 'Pods' -o \
          -name '.symlinks' -o \
          -name 'xcode-derived' -o \
          -name 'ModuleCache.noindex' -o \
          -name 'Intermediates.noindex' \
        \) \
      \) -o \
      \( \
        -type f \( \
          -name 'pubspec.lock' -o \
          -name 'pubspec_overrides.yaml' -o \
          -name 'local.properties' -o \
          -name '*.iml' -o \
          -name '*.xcuserstate' \
        \) \
      \) \
      -print | sed -n '1,50p'
  )"

  if [[ -n "$leaked_paths" ]]; then
    echo "Refusing to stage Flutter pub package with generated local artifacts:" >&2
    printf '%s\n' "$leaked_paths" >&2
    exit 1
  fi
}

stage_ios_optional_plugins_package() {
  local flutter_package_stage_dir="$1"
  local source_dir="$ROOT_DIR/lib/ios/VesperPlayerOptionalPlugins"
  local destination_dir="$flutter_package_stage_dir/ios/VesperPlayerOptionalPlugins"
  local artifact_name
  local artifacts=(
    VesperFFmpegAVCodec
    VesperFFmpegAVFormat
    VesperFFmpegAVUtil
    VesperPlayerRemuxFfmpegPlugin
    VesperPlayerSourceNormalizerFfmpegPlugin
    VesperPlayerDecoderVideoToolboxPlugin
    VesperPlayerFrameProcessorDiagnosticPlugin
  )

  if [[ ! -f "$source_dir/Package.swift" ]]; then
    echo "Missing canonical iOS optional plugin package: $source_dir/Package.swift" >&2
    exit 1
  fi

  for artifact_name in "${artifacts[@]}"; do
    if [[ ! -d "$source_dir/Artifacts/$artifact_name.xcframework" ]]; then
      echo "Missing optional iOS artifact for Flutter pub staging:" >&2
      echo "  $source_dir/Artifacts/$artifact_name.xcframework" >&2
      echo "Run scripts/vesper ios stage-optional-plugins-release first." >&2
      exit 1
    fi
  done

  mkdir -p "$destination_dir"
  rsync -a \
    --exclude '.build' \
    --exclude '.swiftpm' \
    "$source_dir/" \
    "$destination_dir/"
}

for package in "${packages[@]}"; do
  source_dir="$ROOT_DIR/lib/flutter/$package"
  stage_dir="$OUTPUT_DIR/$package"
  rsync_excludes=()

  if [[ ! -f "$source_dir/pubspec.yaml" ]]; then
    echo "Missing Flutter package pubspec: $source_dir/pubspec.yaml" >&2
    exit 1
  fi

  for pattern in "${staging_excludes[@]}"; do
    rsync_excludes+=(--exclude "$pattern")
  done

  mkdir -p "$(dirname "$stage_dir")"
  rsync -a "${rsync_excludes[@]}" "$source_dir/" "$stage_dir/"

  cp "$ROOT_DIR/LICENSE" "$stage_dir/LICENSE"
  if [[ "$package" == "vesper_player_source_normalizer_ffmpeg" ]]; then
    stage_ios_optional_plugins_package "$stage_dir"
  fi
  rewrite_pubspec "$stage_dir/pubspec.yaml"
  validate_staged_package "$stage_dir"
done

echo "Staged Flutter pub packages into:"
echo "  $OUTPUT_DIR"
printf '  %s\n' "${packages[@]}"
if [[ "${#packages[@]}" -eq "${#core_packages[@]}" ]]; then
  echo "Skipped optional Flutter plugin packages. Set VESPER_FLUTTER_INCLUDE_OPTIONAL_PLUGINS=1 to stage them."
fi
