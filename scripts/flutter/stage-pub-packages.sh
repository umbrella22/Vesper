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

rewrite_source_normalizer_ios_package() {
  local package_dir="$1"
  local manifest="$package_dir/ios/vesper_player_source_normalizer_ffmpeg/Package.swift"
  local ios_release_dir="$ROOT_DIR/dist/release/ios"
  local runtime_zip="$ios_release_dir/VesperPlayerFfmpegRuntime.xcframework.zip"
  local plugin_zip="$ios_release_dir/VesperPlayerSourceNormalizerFfmpegPlugin.xcframework.zip"
  local binary_base_url="${VESPER_IOS_BINARY_BASE_URL:-https://github.com/umbrella22/Vesper/releases/download/v$VERSION}"
  local runtime_checksum
  local plugin_checksum

  if [[ ! -f "$manifest" ]]; then
    return
  fi
  if [[ ! -f "$runtime_zip" || ! -f "$plugin_zip" ]]; then
    echo "Staged $package_dir with local-development iOS package manifest."
    echo "  Set VESPER_IOS_BINARY_BASE_URL and stage iOS XCFramework zips to emit binaryTarget metadata."
    return
  fi

  runtime_checksum="$(swift package compute-checksum "$runtime_zip")"
  plugin_checksum="$(swift package compute-checksum "$plugin_zip")"

  cat >"$manifest" <<EOF
// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "vesper_player_source_normalizer_ffmpeg",
    defaultLocalization: "en",
    platforms: [
        .iOS("17.0"),
    ],
    products: [
        .library(
            name: "vesper-player-source-normalizer-ffmpeg",
            targets: ["vesper_player_source_normalizer_ffmpeg"]
        ),
    ],
    dependencies: [
        .package(name: "FlutterFramework", path: "../FlutterFramework"),
    ],
    targets: [
        .binaryTarget(
            name: "VesperPlayerFfmpegRuntime",
            url: "$binary_base_url/VesperPlayerFfmpegRuntime.xcframework.zip",
            checksum: "$runtime_checksum"
        ),
        .binaryTarget(
            name: "VesperPlayerSourceNormalizerFfmpegPlugin",
            url: "$binary_base_url/VesperPlayerSourceNormalizerFfmpegPlugin.xcframework.zip",
            checksum: "$plugin_checksum"
        ),
        .target(
            name: "vesper_player_source_normalizer_ffmpeg",
            dependencies: [
                .product(name: "FlutterFramework", package: "FlutterFramework"),
                "VesperPlayerFfmpegRuntime",
                "VesperPlayerSourceNormalizerFfmpegPlugin",
            ]
        ),
    ]
)
EOF
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
  rewrite_pubspec "$stage_dir/pubspec.yaml"
  if [[ "$package" == "vesper_player_source_normalizer_ffmpeg" ]]; then
    rewrite_source_normalizer_ios_package "$stage_dir"
  fi
  validate_staged_package "$stage_dir"
done

echo "Staged Flutter pub packages into:"
echo "  $OUTPUT_DIR"
printf '  %s\n' "${packages[@]}"
if [[ "${#packages[@]}" -eq "${#core_packages[@]}" ]]; then
  echo "Skipped optional Flutter plugin packages. Set VESPER_FLUTTER_INCLUDE_OPTIONAL_PLUGINS=1 to stage them."
fi
