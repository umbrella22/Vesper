// swift-tools-version: 5.9
import PackageDescription
import Foundation

private func resolveVesperPlayerOptionalPluginsPath() -> String {
    let fileManager = FileManager.default
    var searchDirectory = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()
        .standardizedFileURL
    let candidatePathComponents: [[String]] = [
        ["VesperPlayerOptionalPlugins"],
        ["lib", "ios", "VesperPlayerOptionalPlugins"],
        ["third_party", "vesper-player-sdk", "lib", "ios", "VesperPlayerOptionalPlugins"],
    ]

    while true {
        for pathComponents in candidatePathComponents {
            let candidate = pathComponents.reduce(searchDirectory) { partial, component in
                partial.appendingPathComponent(component, isDirectory: true)
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

    fatalError("Unable to locate VesperPlayerOptionalPlugins from \(#filePath)")
}

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
        .package(
            name: "VesperPlayerOptionalPlugins",
            path: resolveVesperPlayerOptionalPluginsPath()
        ),
    ],
    targets: [
        .target(
            name: "vesper_player_source_normalizer_ffmpeg",
            dependencies: [
                .product(name: "FlutterFramework", package: "FlutterFramework"),
                .product(
                    name: "VesperPlayerSourceNormalizerFfmpegPlugin",
                    package: "VesperPlayerOptionalPlugins"
                ),
            ]
        ),
    ]
)
