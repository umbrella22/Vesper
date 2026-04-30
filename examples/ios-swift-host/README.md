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
- Double-tap seek
- Video-only Photos picker
- Built-in Apple HLS sample preset

Demo URLs are owned by the example. The reusable package under
[`lib/ios/VesperPlayerKit`](../../lib/ios/VesperPlayerKit/) only exposes
generic `VesperPlayerSource` APIs.

## Requirements

- Xcode 16+
- iOS 14.0+ deployment target
- Rust toolchain with iOS targets installed
- [XcodeGen](https://github.com/yonaskolb/XcodeGen)
- Apple Silicon Mac (Simulator slices are arm64-only)

## Run

1. Build the Rust iOS resolver bundle (required before resolving the Swift
   package):

   ```sh
   ./scripts/build-ios-player-ffi-xcframework.sh
   ```

2. Generate the Xcode project:

   ```sh
   cd examples/ios-swift-host && xcodegen generate
   ```

3. Open `VesperPlayerHostDemo.xcodeproj` in Xcode and run on an arm64
   Simulator or device.

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
./scripts/build-ios-player-ffi-xcframework.sh release
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
