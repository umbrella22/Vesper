# Vesper iOS Host Demo

A runnable SwiftUI sample app that integrates the Vesper Player SDK through
the [`VesperPlayerKit`](../../lib/ios/VesperPlayerKit/) Swift Package.

Use this example as a reference for:

- Embedding `VesperPlayerController` and `PlayerSurfaceContainer` in SwiftUI
- Selecting local videos via the Photos picker
- Playing HLS or local files through `AVPlayer`
- Switching themes, sources, tracks, and ABR policies

## Features Demonstrated

- System / Light / Dark theme modes
- Fullscreen stage
- Quality / audio / subtitle / playback-speed bottom sheets
- AirPlay route picker in portrait and fullscreen playback
- Double-tap seek
- Video-only Photos picker
- Built-in Apple HLS sample preset
- SourceNormalizer plugin diagnostics panel. The example defaults to
  `preflightOnly` and lets you switch among `disabled`, `diagnosticsOnly`,
  `preflightOnly`, `preferNormalized`, and `requireNormalized` at runtime.
- FrameProcessor diagnostic plugin logging. The example embeds the diagnostic
  plugin when available, but does not open frame sessions or alter rendering.

Demo URLs are owned by the example. The reusable package under
[`lib/ios/VesperPlayerKit`](../../lib/ios/VesperPlayerKit/) only exposes
generic `VesperPlayerSource` APIs.

## AirPlay

The player stage includes the SDK `VesperAirPlayRouteButton`, backed by
`AVRoutePickerView`. Selecting an AirPlay device routes the underlying
`AVPlayer`, so the existing play / pause / seek controls continue to operate
the active route. The native player explicitly allows external playback when a
source is loaded.

## Requirements

- Xcode 16+
- iOS 17.0+ deployment target
- Rust toolchain with iOS targets installed
- [XcodeGen](https://github.com/yonaskolb/XcodeGen)
- Apple Silicon Mac (Simulator slices are arm64-only)

## Run

1. Build the Rust iOS resolver bundle (required before resolving the Swift
   package):

   ```sh
   ./scripts/vesper ios ffi
   ```

2. Generate the Xcode project:

   ```sh
   cd examples/ios-swift-host && xcodegen generate
   ```

3. Open `VesperPlayerHostDemo.xcodeproj` in Xcode and run on an arm64
   Simulator or device.

The generated Xcode project includes a post-build script that embeds the
optional `VesperPlayerFfmpegRuntime.framework`,
`VesperPlayerRemuxFfmpegPlugin.framework`,
`VesperPlayerSourceNormalizerFfmpegPlugin.framework`, and
`VesperPlayerFrameProcessorDiagnosticPlugin.framework`. Release hosts should
consume the matching
`VesperPlayerFfmpegRuntime.xcframework.zip` and
`VesperPlayerRemuxFfmpegPlugin.xcframework.zip`,
`VesperPlayerSourceNormalizerFfmpegPlugin.xcframework.zip`, and
`VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip` artifacts built
from the same FFmpeg profile where applicable.

## Optional Plugin Diagnostics

The iOS example passes only plugin framework binary paths to
`VesperPlayerController`. The shared `VesperPlayerFfmpegRuntime.framework` is
embedded and signed by the host, but it is not passed as a plugin path.

SourceNormalizer diagnostics and preflight modes do not change playback. In
`preferNormalized` and `requireNormalized`, the host may open a disk-backed
normalized resource session and hand the resulting fMP4 or short-window HLS
resource to AVPlayer through a `vesper-normalized://` resource loader.
`preferNormalized` falls back to the original source on failure;
`requireNormalized` reports a source error. FrameProcessor remains
debug diagnostics only in this example and is never marked as participating in
mobile playback.

## Build From CLI

Debug build for an installed Simulator:

```sh
cd examples/ios-swift-host
xcodegen generate
xcodebuild \
  -project VesperPlayerHostDemo.xcodeproj \
  -scheme VesperPlayerHostDemo \
  -destination 'generic/platform=iOS Simulator' \
  ARCHS=arm64 ONLY_ACTIVE_ARCH=YES \
  CODE_SIGNING_ALLOWED=NO build
```

Release build for device (no codesign):

```sh
cd examples/ios-swift-host
xcodegen generate
xcodebuild \
  -project VesperPlayerHostDemo.xcodeproj \
  -scheme VesperPlayerHostDemo \
  -configuration Release \
  -sdk iphoneos \
  -destination 'generic/platform=iOS' \
  CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO build
```

## Test

```sh
./scripts/vesper ios ffi release
cd examples/ios-swift-host
xcodegen generate
xcodebuild test \
  -project VesperPlayerHostDemo.xcodeproj \
  -scheme VesperPlayerHostDemo \
  -destination 'id=<SIMULATOR_ID>' \
  ARCHS=arm64 ONLY_ACTIVE_ARCH=YES \
  CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO
```

List available Simulator IDs with:

```sh
xcodebuild -project VesperPlayerHostDemo.xcodeproj \
  -scheme VesperPlayerHostDemo -showdestinations
```

## Layout

- `project.yml` — XcodeGen descriptor
- `Sources/VesperPlayerHostDemoApp.swift` — iOS app entrypoint
- `Sources/PlayerHostView.swift` — SwiftUI host UI

Reusable host kit (separate project):

- [`lib/ios/VesperPlayerKit`](../../lib/ios/VesperPlayerKit/) — Swift Package and XCFramework project for `VesperPlayerController`, `VesperPlayerSource`, `PlayerSurfaceContainer`
