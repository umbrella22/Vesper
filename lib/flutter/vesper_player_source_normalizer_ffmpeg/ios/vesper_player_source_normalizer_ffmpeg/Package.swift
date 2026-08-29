// swift-tools-version: 5.9
import PackageDescription

private let vesperPlayerKitVersion: Version = "0.5.0"

let package = Package(
    name: "vesper_player_source_normalizer_ffmpeg",
    defaultLocalization: "en",
    platforms: [
        .iOS("17.0"),
    ],
    products: [
        .library(
            name: "vesper-player-source-normalizer-ffmpeg",
            type: .dynamic,
            targets: ["vesper_player_source_normalizer_ffmpeg"]
        ),
    ],
    dependencies: [
        .package(name: "FlutterFramework", path: "../FlutterFramework"),
        .package(
            url: "https://github.com/umbrella22/VesperPlayerKit.git",
            exact: vesperPlayerKitVersion
        ),
    ],
    targets: [
        .target(
            name: "vesper_player_source_normalizer_ffmpeg",
            dependencies: [
                .product(name: "FlutterFramework", package: "FlutterFramework"),
                .product(
                    name: "VesperPlayerSourceNormalizerFfmpeg",
                    package: "VesperPlayerKit"
                ),
            ]
        ),
    ]
)
