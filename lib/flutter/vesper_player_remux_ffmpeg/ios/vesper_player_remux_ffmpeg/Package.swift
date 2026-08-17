// swift-tools-version: 5.9
import PackageDescription

private let vesperPlayerKitVersion: Version = "0.4.3-rc.1"

let package = Package(
    name: "vesper_player_remux_ffmpeg",
    defaultLocalization: "en",
    platforms: [
        .iOS("17.0"),
    ],
    products: [
        .library(
            name: "vesper-player-remux-ffmpeg",
            type: .dynamic,
            targets: ["vesper_player_remux_ffmpeg"]
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
            name: "vesper_player_remux_ffmpeg",
            dependencies: [
                .product(name: "FlutterFramework", package: "FlutterFramework"),
                .product(name: "VesperPlayerRemuxFfmpeg", package: "VesperPlayerKit"),
            ]
        ),
    ]
)
