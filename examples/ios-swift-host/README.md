# Vesper iOS Host Demo

This example is the runnable iOS host app for the Vesper Player SDK.

It intentionally lives under `examples/` so it can serve as:

- a host-integration reference
- a runnable preview app
- a UI demo for platform consumers

## Stack

- `Swift`
- `SwiftUI` for host controls and app shell
- `UIView` + `AVPlayerLayer` wrapper for the video presentation surface
- host UI owns controls and progress UI

## Current Status

This host app is now intentionally thin:

- the app shell is SwiftUI
- the actual iOS integration layer now lives under `lib/ios/VesperPlayerKit`
- the example app imports that package as a local dependency
- the default host path now boots through an `AVPlayer`-backed native bridge
- the example app owns its own Apple HLS sample preset
- the host can switch between that preset source and a user-picked local video file
- the sample app now includes the first polished player shell:
  - `System / Light / Dark` theme modes
  - fullscreen stage
  - quality / audio / subtitle / speed bottom sheets
  - double-tap seek and SwiftUI previews
  - video-only Photos picker

The host-facing shape now lives in the package itself:

- `VesperPlayerSource`
- `VesperPlayerController`
- `PlayerSurfaceContainer`

The SwiftUI demo now consumes that wrapper layer instead of talking to the raw bridge directly.

## Bridge Modes

The package is shaped around one bridge contract with two implementations:

- `FakePlayerBridge`
  - local interactive preview bridge
- `VesperNativePlayerBridge`
  - current `AVPlayer`-backed native host bridge and the integration point for `player-platform-ios`

The current SwiftUI screen now boots with the native bridge by default, while the fake bridge is
still kept in-tree as a lightweight fallback/reference implementation.

The package exposes a host-facing controller layer:

- `VesperPlayerController`
  - source selection, playback commands, published UI state, backend label
- `VesperPlayerSource`
  - stable source DTO used by the host UI
- `PlayerSurfaceContainer`
  - reusable SwiftUI surface wrapper over `UIView + AVPlayerLayer`

The host UI now also has a local source-selection path through the Photos video picker so the source
selection behavior can converge with Android before streaming-specific UI is introduced.

Those preset/demo sources intentionally live in `examples/ios-swift-host`; the reusable package under
`lib/ios/VesperPlayerKit` exposes only generic `VesperPlayerSource` APIs and does not embed demo URLs.

## Project Split

- `lib/ios/VesperPlayerKit`
  - local Swift Package + future `XCFramework` project
- `examples/ios-swift-host`
  - runnable sample app that imports the package

## Xcode Handoff

XcodeGen now has two useful entrypoints:

1. `lib/ios/VesperPlayerKit/project.yml`
   - framework project for future `XCFramework` packaging
2. `examples/ios-swift-host/project.yml`
   - runnable demo app that imports `VesperPlayerKit`

For the demo app:

1. generate or open the demo project from `project.yml`
2. confirm the Swift host app boots with the AVPlayer native bridge
3. validate Photos video picker / `play / pause / seek / stop / rate`
4. validate fullscreen, theme switching, and track/ABR sheets

For the reusable iOS binary artifact itself, use:

- `scripts/build-ios-vesper-player-kit-xcframework.sh`

## Layout

- `project.yml`
  - XcodeGen descriptor for the demo app
- `Sources/VesperPlayerHostDemoApp.swift`
  - iOS app entrypoint
- `Sources/PlayerHostView.swift`
  - SwiftUI host UI
- `../../lib/ios/VesperPlayerKit/Package.swift`
  - local Swift Package entrypoint
- `../../lib/ios/VesperPlayerKit/project.yml`
  - framework/XCFramework-oriented XcodeGen descriptor
- `../../lib/ios/VesperPlayerKit/Sources/VesperPlayerKit/VesperPlayerController.swift`
  - host-facing controller wrapper
- `../../lib/ios/VesperPlayerKit/Sources/VesperPlayerKit/VesperPlayerSource.swift`
  - host-facing source DTO
- `../../lib/ios/VesperPlayerKit/Sources/VesperPlayerKit/PlayerSurfaceView.swift`
  - `UIViewRepresentable` + `UIView` video host shell
- `../../lib/ios/VesperPlayerKit/Sources/VesperPlayerKit/PlayerBridge.swift`
  - bridge-facing host contract
- `../../lib/ios/VesperPlayerKit/Sources/VesperPlayerKit/FakePlayerBridge.swift`
  - local interactive placeholder until native bridge lands
