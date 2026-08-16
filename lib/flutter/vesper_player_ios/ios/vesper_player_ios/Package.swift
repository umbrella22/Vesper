// swift-tools-version: 5.9
import PackageDescription

private let minimumVesperPlayerKitVersion: Version = "0.4.1"

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
        .package(
            url: "https://github.com/umbrella22/VesperPlayerKit.git",
            .upToNextMinor(from: minimumVesperPlayerKitVersion)
        ),
    ],
    targets: [
        .target(
            name: "vesper_player_ios",
            dependencies: [
                .product(name: "FlutterFramework", package: "FlutterFramework"),
                .product(name: "VesperPlayerKit", package: "VesperPlayerKit"),
            ]
        ),
    ]
)
