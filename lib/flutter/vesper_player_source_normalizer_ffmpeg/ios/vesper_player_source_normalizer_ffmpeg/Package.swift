// swift-tools-version: 5.9
import PackageDescription
import Foundation

private func resolveLocalArtifactPath(_ candidates: [[String]]) -> String? {
    let fileManager = FileManager.default
    var searchDirectory = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()
        .standardizedFileURL

    while true {
        for pathComponents in candidates {
            let candidate = pathComponents.reduce(searchDirectory) { partial, component in
                partial.appendingPathComponent(component, isDirectory: false)
            }
            if fileManager.fileExists(atPath: candidate.path) {
                return candidate.path
            }
        }

        let parent = searchDirectory.deletingLastPathComponent()
        if parent.path == searchDirectory.path {
            break
        }
        searchDirectory = parent
    }

    return nil
}

private let runtimeArtifactPath = resolveLocalArtifactPath([
    ["lib", "ios", "VesperPlayerKit", ".build", "player-ffmpeg-runtime", "VesperPlayerFfmpegRuntime.xcframework"],
    ["third_party", "vesper-player-sdk", "lib", "ios", "VesperPlayerKit", ".build", "player-ffmpeg-runtime", "VesperPlayerFfmpegRuntime.xcframework"],
])

private let pluginArtifactPath = resolveLocalArtifactPath([
    ["lib", "ios", "VesperPlayerKit", ".build", "player-source-normalizer-ffmpeg-plugin", "VesperPlayerSourceNormalizerFfmpegPlugin.xcframework"],
    ["third_party", "vesper-player-sdk", "lib", "ios", "VesperPlayerKit", ".build", "player-source-normalizer-ffmpeg-plugin", "VesperPlayerSourceNormalizerFfmpegPlugin.xcframework"],
])

private var targetDependencies: [Target.Dependency] = [
    .product(name: "FlutterFramework", package: "FlutterFramework"),
]

private var targets: [Target] = []

if let runtimeArtifactPath, let pluginArtifactPath {
    targetDependencies.append(.target(name: "VesperPlayerFfmpegRuntime"))
    targetDependencies.append(.target(name: "VesperPlayerSourceNormalizerFfmpegPlugin"))
    targets.append(
        .binaryTarget(
            name: "VesperPlayerFfmpegRuntime",
            path: runtimeArtifactPath
        )
    )
    targets.append(
        .binaryTarget(
            name: "VesperPlayerSourceNormalizerFfmpegPlugin",
            path: pluginArtifactPath
        )
    )
}

targets.append(
    .target(
        name: "vesper_player_source_normalizer_ffmpeg",
        dependencies: targetDependencies
    )
)

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
    targets: targets
)
