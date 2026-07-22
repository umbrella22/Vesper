// swift-tools-version: 5.10
import Foundation
import PackageDescription

private let artifactNames = [
    "VesperFFmpegAVCodec",
    "VesperFFmpegAVFormat",
    "VesperFFmpegAVUtil",
    "VesperPlayerRemuxFfmpegPlugin",
    "VesperPlayerSourceNormalizerFfmpegPlugin",
    "VesperPlayerDecoderVideoToolboxPlugin",
    "VesperPlayerFrameProcessorDiagnosticPlugin",
]

private let packageRoot = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
for artifactName in artifactNames {
    let relativePath = "Artifacts/\(artifactName).xcframework"
    let artifactPath = packageRoot.appendingPathComponent(relativePath).path
    if !FileManager.default.fileExists(atPath: artifactPath) {
        fatalError(
            """
            Missing optional iOS artifact at \(relativePath).
            Run scripts/vesper ios stage-optional-plugins-release before resolving this package.
            """
        )
    }
}

private let ffmpegTargets = [
    "VesperFFmpegAVCodec",
    "VesperFFmpegAVFormat",
    "VesperFFmpegAVUtil",
]

private let binaryTargets: [Target] = artifactNames.map { artifactName in
    .binaryTarget(
        name: artifactName,
        path: "Artifacts/\(artifactName).xcframework"
    )
}

private func productTarget(
    _ name: String,
    dependencies: [String]
) -> Target {
    .target(
        name: name,
        dependencies: dependencies.map { .target(name: $0) },
        path: "Sources/\(name)"
    )
}

let package = Package(
    name: "VesperPlayerOptionalPlugins",
    platforms: [
        .iOS(.v17),
    ],
    products: [
        .library(
            name: "VesperPlayerFfmpegRuntime",
            targets: ["VesperPlayerFfmpegRuntimeProduct"]
        ),
        .library(
            name: "VesperPlayerRemuxFfmpegPlugin",
            targets: ["VesperPlayerRemuxFfmpegPluginProduct"]
        ),
        .library(
            name: "VesperPlayerSourceNormalizerFfmpegPlugin",
            targets: ["VesperPlayerSourceNormalizerFfmpegPluginProduct"]
        ),
        .library(
            name: "VesperPlayerDecoderVideoToolboxPlugin",
            targets: ["VesperPlayerDecoderVideoToolboxPluginProduct"]
        ),
        .library(
            name: "VesperPlayerFrameProcessorDiagnosticPlugin",
            targets: ["VesperPlayerFrameProcessorDiagnosticPluginProduct"]
        ),
        .library(
            name: "VesperPlayerOptionalPlugins",
            targets: ["VesperPlayerOptionalPluginsProduct"]
        ),
    ],
    targets: binaryTargets + [
        productTarget(
            "VesperPlayerFfmpegRuntimeProduct",
            dependencies: ffmpegTargets
        ),
        productTarget(
            "VesperPlayerRemuxFfmpegPluginProduct",
            dependencies: ["VesperPlayerRemuxFfmpegPlugin"] + ffmpegTargets
        ),
        productTarget(
            "VesperPlayerSourceNormalizerFfmpegPluginProduct",
            dependencies: ["VesperPlayerSourceNormalizerFfmpegPlugin"] + ffmpegTargets
        ),
        productTarget(
            "VesperPlayerDecoderVideoToolboxPluginProduct",
            dependencies: ["VesperPlayerDecoderVideoToolboxPlugin"]
        ),
        productTarget(
            "VesperPlayerFrameProcessorDiagnosticPluginProduct",
            dependencies: ["VesperPlayerFrameProcessorDiagnosticPlugin"]
        ),
        productTarget(
            "VesperPlayerOptionalPluginsProduct",
            dependencies: artifactNames
        ),
    ]
)
