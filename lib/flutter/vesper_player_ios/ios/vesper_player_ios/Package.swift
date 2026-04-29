// swift-tools-version: 5.9
import PackageDescription
import Foundation

private func resolveVesperPlayerKitPath() -> String {
    let fileManager = FileManager.default
    var searchDirectory = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()
        .standardizedFileURL
    let candidatePathComponents: [[String]] = [
        ["lib", "ios", "VesperPlayerKit"],
        ["third_party", "vesper-player-sdk", "lib", "ios", "VesperPlayerKit"],
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

    fatalError("Unable to locate VesperPlayerKit from \(#filePath)")
}

let package = Package(
    name: "vesper_player_ios",
    defaultLocalization: "en",
    platforms: [
        .iOS("17.0"),
    ],
    products: [
        .library(name: "vesper-player-ios", targets: ["vesper_player_ios"]),
    ],
    dependencies: [
        .package(name: "FlutterFramework", path: "../FlutterFramework"),
        .package(name: "VesperPlayerKit", path: resolveVesperPlayerKitPath()),
    ],
    targets: [
        .target(
            name: "vesper_player_ios",
            dependencies: [
                .product(name: "FlutterFramework", package: "FlutterFramework"),
                .product(name: "VesperPlayerKit", package: "VesperPlayerKit"),
                .product(name: "VesperPlayerFFI", package: "VesperPlayerKit"),
            ]
        ),
    ]
)
