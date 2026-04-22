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

- `scripts/build-ios-player-ffi-xcframework.sh`
- `scripts/build-ios-vesper-player-kit-xcframework.sh`
- `scripts/stage-ios-vesper-player-kit-release.sh`

When consuming `lib/ios/VesperPlayerKit` as a local Swift Package, build the Rust resolver bundle
first:

- `scripts/build-ios-player-ffi-xcframework.sh`

The native unit-test baseline is now expected to run through Xcode as well:

- `xcodebuild -project lib/ios/VesperPlayerKit/VesperPlayerKit.xcodeproj -scheme VesperPlayerKit -destination 'platform=macOS,variant=Mac Catalyst,name=My Mac' ARCHS=arm64 ONLY_ACTIVE_ARCH=YES test CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO`

That script:

- builds the Rust `player-ffi-resolver` Apple bundle consumed by the Swift package / shim
- regenerates the framework project with `xcodegen`
- archives iOS + iOS Simulator frameworks
- creates `VesperPlayerKit.xcframework`

### Apple Architecture Policy

Apple packaging in this repository is intentionally `arm64`-only:

- iOS device artifacts ship `arm64`
- iOS Simulator artifacts ship `arm64` only
- Mac Catalyst artifacts, when enabled, ship `arm64` only
- local `xcodebuild` verification should also stay on `ARCHS=arm64` for Simulator / Mac Catalyst
- do not reintroduce `x86_64` simulator or Catalyst slices in packaging scripts, CI inputs, or release assets

GitHub Releases now publish VesperPlayerKit for iOS downloads through:

- `.github/workflows/mobile-lib-release.yml`

Current iOS download package names:

- `VesperPlayerKit-ios-arm64.framework.zip`
- `VesperPlayerKit-ios-simulator-arm64.framework.zip`
- `VesperPlayerKit.xcframework.zip`

Download guidance:

- use `VesperPlayerKit-ios-arm64.framework.zip` for device-only packaging
- use `VesperPlayerKit-ios-simulator-arm64.framework.zip` when validating on Apple Silicon iOS Simulator
- use `VesperPlayerKit.xcframework.zip` when you want one distributable Apple package that covers device + Apple Silicon simulator targets
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
    @StateObject private var controller = VesperPlayerControllerFactory.makeDefault(
        resiliencePolicy: .resilient()
    )

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

The iOS host API now exposes first-round playback resilience controls:

- `VesperPlaybackResiliencePolicy`
- `VesperBufferingPolicy`
- `VesperRetryPolicy`
- `VesperCachePolicy`

These now shape `AVPlayer` buffering behavior and controlled retry/backoff for remote
sources. Cache configuration is currently mapped as a best-effort process-wide `URLCache.shared`
capacity hint for remote playback, and it does not pretend to offer the same transport depth that
`Media3` exposes on Android.

The iOS host API also exposes a lightweight hardware decode probe:

- `VesperCodecSupport.hardwareDecodeSupported(for:)`

It currently normalizes the common `H264 / AVC / AVC1` and `HEVC / H265 / HVC1 / HEV1` aliases and
checks VideoToolbox support for the requested codec. Unknown codec names return `false`.

## ABR Notes

`VesperPlayerKit` now exposes two iOS ABR routes on top of `AVPlayer`:

- `VesperAbrPolicy.constrained(...)`
- `VesperAbrPolicy.fixedTrack(...)`

Important iOS-specific semantics:

- `fixedTrack` is best-effort HLS variant pinning on iOS 15+, not exact AVPlayer video-track
  switching
- single-axis constrained limits such as `VesperAbrPolicy.constrained(maxHeight: 720)` are
  supported for HLS, but the host waits for the current variant catalog before inferring the
  missing width/height
- `effectiveVideoTrackId` is also best-effort: it is derived from the current HLS variant ladder,
  access-log bitrate, and presentation size once the runtime has enough evidence
- `videoVariantObservation` exposes the raw runtime evidence the host is using for that inference:
  access-log bitrate plus the latest rendered presentation size
- `fixedTrackStatus` gives the latest runtime convergence signal for a best-effort fixed-track
  request: `.pending`, `.locked`, or `.fallback`
- resilience reload / restore now defer both `fixedTrack` and single-axis constrained ABR until
  the current HLS variant catalog is loaded, so those policies do not fail early during reload
- if a restored fixed-track `trackId` no longer exists verbatim after the HLS ladder drifts, the
  host now tries to remap it onto the closest semantically equivalent variant before surfacing
  unsupported
- if a restored fixed-track request keeps rendering a different observed variant after sustained
  runtime evidence, the host now surfaces a non-fatal `lastError` and degrades that restored
  request into constrained ABR using the requested variant limits when possible, otherwise back to
  automatic ABR

## Download Flow Notes

`VesperDownloadManager` can manage single-file and segmented downloads, but remote segmented sources
work best when the host app performs a small planning step before calling `createTask(...)`.

Recommended host flow for remote HLS on iOS:

- show an optimistic "preparing" row in the UI immediately after the user taps create
- read the remote manifest in the background and build `VesperDownloadAssetIndex(resources +
  segments)` plus a dedicated `targetDirectory`
- call `createTask(...)` only after the prepared source / profile / asset index are ready

Additional notes:

- the foreground executor now downloads `assetIndex.resources + assetIndex.segments` together when
  both are provided
- pause / resume / remove should always be keyed by `taskId`; do not merge multiple tasks by URL in
  host UI state
- the current iOS example only wires this segmented download path for HLS; DASH stays explicitly
  unsupported on the AVPlayer backend

## How It Relates To The Example

`examples/ios-swift-host` now imports `VesperPlayerKit` as a local package dependency.

That means:

- `lib/ios/VesperPlayerKit`
  - reusable Swift-native integration surface
- `examples/ios-swift-host`
  - runnable sample app showing how to use it
