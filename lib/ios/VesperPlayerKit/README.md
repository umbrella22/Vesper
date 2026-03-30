# VesperPlayerKit for iOS

`lib/ios/VesperPlayerKit` is the iOS-native VesperPlayerKit integration project for the Vesper Player SDK.

## Delivery Shapes

This folder now contains both native packaging entrypoints:

- `Package.swift`
  - local Swift Package used by the demo app
- `project.yml`
  - XcodeGen descriptor for building a framework / future `XCFramework`

## Packaging Helper

You can build the iOS binary artifact through:

- `scripts/build-ios-vesper-player-kit-xcframework.sh`
- `scripts/stage-ios-vesper-player-kit-release.sh`

That script:

- regenerates the framework project with `xcodegen`
- archives iOS + iOS Simulator frameworks
- creates `VesperPlayerKit.xcframework`

GitHub Releases now publish VesperPlayerKit for iOS downloads through:

- `.github/workflows/mobile-lib-release.yml`

Current iOS download package names:

- `VesperPlayerKit-ios-arm64.framework.zip`
- `VesperPlayerKit-ios-simulator-arm64.framework.zip`
- `VesperPlayerKit-ios-simulator-x86_64.framework.zip`
- `VesperPlayerKit.xcframework.zip`

Download guidance:

- use `VesperPlayerKit-ios-arm64.framework.zip` for device-only packaging
- use the simulator-specific framework zip that matches your host CPU when validating in Simulator
- use `VesperPlayerKit.xcframework.zip` when you want one distributable Apple package that covers both device and simulator targets
- see [newDoc/RELEASE-DOWNLOAD-GUIDE.md](../../../newDoc/RELEASE-DOWNLOAD-GUIDE.md) for the full package-selection guide

## What The Package Exposes

- `VesperPlayerController`
- `VesperPlayerControllerFactory`
- `VesperPlayerSource`
- `PlayerSurfaceContainer`
- host UI state DTOs such as `PlayerHostUiState`

The lower bridge internals stay inside the package:

- `PlayerBridge`
- `FakePlayerBridge`
- `VesperNativePlayerBridge`

The package intentionally does not embed demo URLs or preset source choices. Those belong in a
consuming host app such as `examples/ios-swift-host`.

## Minimal SwiftUI Usage

```swift
import VesperPlayerKit
import SwiftUI

struct DemoPlayerView: View {
    @StateObject private var controller = VesperPlayerControllerFactory.makeDefault()

    var body: some View {
        let uiState = controller.uiState

        VStack {
            PlayerSurfaceContainer(controller: controller)
                .frame(height: 240)

            Text(uiState.playbackState.rawValue)

            Button("Play") {
                controller.play()
            }
        }
        .onAppear { controller.initialize() }
        .onDisappear { controller.dispose() }
    }
}
```

## How It Relates To The Example

`examples/ios-swift-host` now imports `VesperPlayerKit` as a local package dependency.

That means:

- `lib/ios/VesperPlayerKit`
  - reusable Swift-native integration surface
- `examples/ios-swift-host`
  - runnable sample app showing how to use it
