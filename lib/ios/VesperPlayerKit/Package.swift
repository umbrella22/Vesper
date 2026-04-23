// swift-tools-version: 5.10
import PackageDescription
import Foundation

private let rustResolverRelativePath = "Artifacts/rust-player-ffi/VesperPlayerFFI.xcframework"
private let rustResolverPath = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent()
    .appendingPathComponent(rustResolverRelativePath)
    .path

if !FileManager.default.fileExists(atPath: rustResolverPath) {
    fatalError(
        """
        Missing Rust iOS resolver bundle at \(rustResolverRelativePath).
        Run scripts/build-ios-player-ffi-xcframework.sh before building VesperPlayerKit as a Swift package.
        """
    )
}

let package = Package(
    name: "VesperPlayerKit",
    defaultLocalization: "en",
    platforms: [
        .iOS(.v17),
    ],
    products: [
        .library(
            name: "VesperPlayerKit",
            targets: ["VesperPlayerKit"]
        ),
    ],
    targets: [
        .binaryTarget(
            name: "VesperPlayerFFI",
            path: rustResolverRelativePath
        ),
        .target(
            name: "VesperPlayerKitBridgeShim",
            path: "Sources/VesperPlayerKitBridgeShim",
            publicHeadersPath: "include"
        ),
        .target(
            name: "VesperPlayerKit",
            dependencies: ["VesperPlayerKitBridgeShim", "VesperPlayerFFI"],
            path: "Sources/VesperPlayerKit",
            resources: [
                .process("Resources"),
            ]
        ),
        .testTarget(
            name: "VesperPlayerKitTests",
            dependencies: ["VesperPlayerKit"],
            path: "Tests/VesperPlayerKitTests"
        ),
    ]
)
