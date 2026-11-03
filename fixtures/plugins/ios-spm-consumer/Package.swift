// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "vesper-ios-spm-consumer",
    platforms: [
        .iOS(.v17),
    ],
    dependencies: [
        .package(
            url: "https://github.com/umbrella22/VesperPlayerKit.git",
            exact: "0.9.1"
        ),
    ],
    targets: [
        .executableTarget(
            name: "VesperIosSpmConsumer",
            dependencies: [
                .product(name: "VesperPlayerKit", package: "VesperPlayerKit"),
            ]
        ),
    ]
)
