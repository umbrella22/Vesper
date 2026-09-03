#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
runner_temp="${RUNNER_TEMP:?RUNNER_TEMP is required}"
local_root="$(mktemp -d "${runner_temp%/}/vesper-flutter-ios-local-spm.XXXXXX")"
local_package="$local_root/VesperPlayerKit"
source_package="$repository_root/lib/ios/VesperPlayerKit"
optional_artifacts="$repository_root/lib/ios/VesperPlayerOptionalPlugins/Artifacts"

test -d "$source_package/Sources/VesperPlayerKit"
test -f "$source_package/Artifacts/rust-player-ffi/VesperPlayerFFI.xcframework/Info.plist"
test -d "$optional_artifacts"

mkdir -p "$local_package"
rsync -a \
  --exclude='.build' \
  --exclude='.swiftpm' \
  --exclude='*.xcodeproj' \
  --exclude='com.apple.DeveloperTools' \
  --exclude='.DS_Store' \
  "$source_package/Sources" \
  "$local_package/"
mkdir -p "$local_package/Artifacts/rust-player-ffi"
rsync -a \
  "$source_package/Artifacts/rust-player-ffi/" \
  "$local_package/Artifacts/rust-player-ffi/"

optional_frameworks=(
  VesperFFmpegAVCodec
  VesperFFmpegAVFormat
  VesperFFmpegAVUtil
  VesperPlayerRemuxFfmpegPlugin
  VesperPlayerSourceNormalizerFfmpegPlugin
  VesperPlayerPerformanceDiagnosticsPlugin
)
for framework in "${optional_frameworks[@]}"; do
  artifact="$optional_artifacts/$framework.xcframework"
  if [[ ! -d "$artifact" || -L "$artifact" || ! -f "$artifact/Info.plist" ]]; then
    echo "Expected staged optional iOS artifact is missing or is a symlink: $artifact" >&2
    exit 1
  fi
  rsync -a "$artifact" "$local_package/Artifacts/"
done

cat > "$local_package/Package.swift" <<'SWIFT'
// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "VesperPlayerKit",
    defaultLocalization: "en",
    platforms: [.iOS(.v17)],
    products: [
        .library(name: "VesperPlayerKit", targets: ["VesperPlayerKit"]),
        .library(name: "VesperPlayerKitUI", targets: ["VesperPlayerKitUI"]),
        .library(name: "VesperPlayerFFI", targets: ["VesperPlayerFFI"]),
        .library(
            name: "VesperPlayerSourceNormalizerFfmpeg",
            targets: [
                "VesperPlayerSourceNormalizerFfmpegPlugin",
                "VesperFFmpegAVCodec",
                "VesperFFmpegAVFormat",
                "VesperFFmpegAVUtil",
            ]
        ),
        .library(
            name: "VesperPlayerRemuxFfmpeg",
            targets: [
                "VesperPlayerRemuxFfmpegPlugin",
                "VesperFFmpegAVCodec",
                "VesperFFmpegAVFormat",
                "VesperFFmpegAVUtil",
            ]
        ),
        .library(
            name: "VesperPlayerPerformanceDiagnostics",
            targets: ["VesperPlayerPerformanceDiagnosticsPlugin"]
        ),
    ],
    targets: [
        .binaryTarget(
            name: "VesperPlayerFFI",
            path: "Artifacts/rust-player-ffi/VesperPlayerFFI.xcframework"
        ),
        .target(
            name: "VesperPlayerKitBridgeShim",
            dependencies: ["VesperPlayerFFI"],
            path: "Sources/VesperPlayerKitBridgeShim",
            publicHeadersPath: "include"
        ),
        .target(
            name: "VesperPlayerKit",
            dependencies: ["VesperPlayerKitBridgeShim", "VesperPlayerFFI"],
            path: "Sources/VesperPlayerKit",
            resources: [.process("Resources")]
        ),
        .target(
            name: "VesperPlayerKitUI",
            dependencies: ["VesperPlayerKit"],
            path: "Sources/VesperPlayerKitUI"
        ),
        .binaryTarget(
            name: "VesperFFmpegAVCodec",
            path: "Artifacts/VesperFFmpegAVCodec.xcframework"
        ),
        .binaryTarget(
            name: "VesperFFmpegAVFormat",
            path: "Artifacts/VesperFFmpegAVFormat.xcframework"
        ),
        .binaryTarget(
            name: "VesperFFmpegAVUtil",
            path: "Artifacts/VesperFFmpegAVUtil.xcframework"
        ),
        .binaryTarget(
            name: "VesperPlayerRemuxFfmpegPlugin",
            path: "Artifacts/VesperPlayerRemuxFfmpegPlugin.xcframework"
        ),
        .binaryTarget(
            name: "VesperPlayerSourceNormalizerFfmpegPlugin",
            path: "Artifacts/VesperPlayerSourceNormalizerFfmpegPlugin.xcframework"
        ),
        .binaryTarget(
            name: "VesperPlayerPerformanceDiagnosticsPlugin",
            path: "Artifacts/VesperPlayerPerformanceDiagnosticsPlugin.xcframework"
        ),
    ]
)
SWIFT

swift package dump-package --package-path "$local_package" > "$local_root/manifest.json"
jq -e '
  ([.products[].name] | sort) == [
    "VesperPlayerFFI",
    "VesperPlayerKit",
    "VesperPlayerKitUI",
    "VesperPlayerPerformanceDiagnostics",
    "VesperPlayerRemuxFfmpeg",
    "VesperPlayerSourceNormalizerFfmpeg"
  ]
' "$local_root/manifest.json" >/dev/null

manifests=(
  "$repository_root/lib/flutter/vesper_player_ios/ios/vesper_player_ios/Package.swift"
  "$repository_root/lib/flutter/vesper_player_source_normalizer_ffmpeg/ios/vesper_player_source_normalizer_ffmpeg/Package.swift"
  "$repository_root/lib/flutter/vesper_player_remux_ffmpeg/ios/vesper_player_remux_ffmpeg/Package.swift"
  "$repository_root/lib/flutter/vesper_player_performance_diagnostics/ios/vesper_player_performance_diagnostics/Package.swift"
)
for manifest in "${manifests[@]}"; do
  ruby - "$manifest" "$local_package" <<'RUBY'
manifest, package = ARGV
source = File.read(manifest)
remote = /\.package\(\n\s*url: "https:\/\/github\.com\/umbrella22\/VesperPlayerKit\.git",\n\s*exact: vesperPlayerKitVersion\n\s*\)/
local = /\.package\(name: "VesperPlayerKit", path: "([^"]+)"\)/
escaped_package = package.gsub('\\', '\\\\').gsub('"', '\\"')
replacement = ".package(name: \"VesperPlayerKit\", path: \"#{escaped_package}\")"
remote_matches = source.scan(remote).length
if remote_matches == 1
  updated = source.sub(remote, replacement)
elsif remote_matches.zero?
  local_matches = source.scan(local)
  unless local_matches.length == 1 && File.directory?(local_matches[0][0])
    abort("expected one existing local VesperPlayerKit package in #{manifest}")
  end
  updated = source
else
  abort("expected exactly one remote VesperPlayerKit dependency in #{manifest}, found #{remote_matches}")
end
unless updated.scan(/\.package\(name: "VesperPlayerKit", path:/).length == 1
  abort("local VesperPlayerKit dependency was not installed in #{manifest}")
end
File.write(manifest, updated)
RUBY
done

if [[ -n "${GITHUB_ENV:-}" ]]; then
  printf 'VESPER_FLUTTER_IOS_LOCAL_SPM_PACKAGE=%s\n' "$local_package" >> "$GITHUB_ENV"
fi
echo "Prepared local Flutter iOS Swift package at $local_package."
