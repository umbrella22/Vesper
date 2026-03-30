// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "VesperPlayerKit",
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
        .target(
            name: "VesperPlayerKit",
            path: "Sources/VesperPlayerKit"
        ),
    ]
)
