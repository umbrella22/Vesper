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
        .target(
            name: "vesper_player_source_normalizer_ffmpeg",
            dependencies: [
                .product(name: "FlutterFramework", package: "FlutterFramework"),
            ]
        ),
    ]
)
