// swift-tools-version: 5.9
import PackageDescription

private let vesperPlayerKitVersion: Version = "0.12.0"

// Repository development only: point VESPER_FLUTTER_IOS_LOCAL_KIT at a local
// VesperPlayerKit package to build against the checkout's host kit. Published
// builds leave it unset and resolve the remote binary package.
private let vesperPlayerKitLocalPath = Context.environment["VESPER_FLUTTER_IOS_LOCAL_KIT"]
    .flatMap { $0.isEmpty ? nil : $0 }

private let vesperPlayerKitDependency: Package.Dependency =
    vesperPlayerKitLocalPath.map { Package.Dependency.package(path: $0) } ?? .package(
        url: "https://github.com/umbrella22/VesperPlayerKit.git",
        exact: vesperPlayerKitVersion
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
            type: .dynamic,
            targets: ["vesper_player_source_normalizer_ffmpeg"]
        ),
    ],
    dependencies: [
        .package(name: "FlutterFramework", path: "../FlutterFramework"),
        vesperPlayerKitDependency,
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
