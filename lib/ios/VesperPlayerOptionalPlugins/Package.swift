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
    "VesperPlayerPerformanceDiagnosticsPlugin",
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

private let binaryTargets: [Target] = artifactNames.map { artifactName in
    .binaryTarget(
        name: artifactName,
        path: "Artifacts/\(artifactName).xcframework"
    )
}

let package = Package(
    name: "VesperPlayerOptionalPlugins",
    platforms: [
        .iOS(.v17),
    ],
    products: artifactNames.map { artifactName in
        .library(name: artifactName, targets: [artifactName])
    },
    targets: binaryTargets
)
