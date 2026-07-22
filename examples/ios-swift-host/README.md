# Vesper iOS Host Demo

A runnable SwiftUI sample app that integrates the Vesper Player SDK through
the [`VesperPlayerKit`](../../lib/ios/VesperPlayerKit/) Swift Package.

Use this example as a reference for:

- Embedding `VesperPlayerController` and `PlayerSurfaceContainer` in SwiftUI
- Selecting local videos via the Photos picker
- Playing HLS or local files through `AVPlayer`
- Switching themes, sources, tracks, and ABR policies
- Keeping playback, diagnostics, and download workflows separated

## Features Demonstrated

- Three workspaces: `Play`, `Diagnostics`, and `Downloads`
- System / Light / Dark theme modes
- Fullscreen stage
- Compact playback queue with a full manage-queue sheet
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
- Dolby Browser Test Kit HLS presets, including locally configured FairPlay
  CBCS validation rows for real devices.
- Bounded in-app event log for host UI actions such as source selection,
  Dolby actions, plugin mode changes, external-route events, and HDR evidence
  capture results.

Demo URLs are owned by the example. The reusable package under
[`lib/ios/VesperPlayerKit`](../../lib/ios/VesperPlayerKit/) only exposes
generic `VesperPlayerSource` APIs.

## Host Workspaces

`Play` is the primary playback surface. It keeps the player stage, theme
control, quick source actions, system playback controls, Picture in Picture,
and the compact queue in the first workflow.

`Diagnostics` contains the session summary, bounded event log, Dolby catalog,
plugin diagnostics, HDR evidence capture, and resilience controls. Dolby
presets default to `Play now`, which starts that preset without changing the
real playback queue. A preset enters continuous playback only when the user
selects `Add to queue`.

`Downloads` stays isolated for download regression testing. The event log is an
example-host operation log; it does not read Logcat, native logs, or system
diagnostic streams.

## Local FairPlay Configuration

FairPlay credentials are never committed to this repository. The Dolby
FairPlay rows stay disabled until the app sees a local license URI plus either
a certificate URI or a base64 certificate. Set these values in the Xcode scheme
environment for real-device validation:

- `VESPER_IOS_FAIRPLAY_LICENSE_URI`
- `VESPER_IOS_FAIRPLAY_CERTIFICATE_URI`
- `VESPER_IOS_FAIRPLAY_CERTIFICATE_BASE64`
- `VESPER_IOS_FAIRPLAY_LICENSE_HEADERS_JSON`
- `VESPER_IOS_FAIRPLAY_AUTHORIZATION`

`VESPER_IOS_FAIRPLAY_CERTIFICATE_BASE64` takes precedence over the certificate
URI. Header JSON must be a flat object with string values; the authorization
variable is merged as the `Authorization` request header. The UI and logs only
show license host, certificate host, and header count, not full URLs, headers,
tokens, or certificate data. Simulator builds can validate unsupported/error
mapping, but real FairPlay decryption requires an iOS device.

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

1. Stage the canonical optional plugin package before SwiftPM resolution:

   ```sh
   ./scripts/vesper ios stage-optional-plugins-release \
     /tmp/vesper-ios-optional-plugins-release \
     --profile source-normalizer \
     ios-arm64 ios-simulator-arm64
   ```

2. Build the Rust iOS resolver bundle:

   ```sh
   ./scripts/vesper ios ffi
   ```

3. Generate the Xcode project:

   ```sh
   cd examples/ios-swift-host && xcodegen generate
   ```

4. Open `VesperPlayerHostDemo.xcodeproj` in Xcode and run on an arm64
   Simulator or device.

The generated App target directly depends on the aggregate
`VesperPlayerOptionalPlugins` SwiftPM product. Xcode embeds and signs seven
top-level sibling frameworks: `VesperFFmpegAVCodec`, `VesperFFmpegAVFormat`,
`VesperFFmpegAVUtil`, and the Remux, SourceNormalizer, VideoToolbox Decoder,
and diagnostic FrameProcessor plugin frameworks. The project has no custom
flat-dylib embedding phase and does not use the legacy umbrella runtime.

## Optional Plugin Diagnostics

The iOS example passes only plugin framework executable paths to
`VesperPlayerController`. The three FFmpeg component frameworks are embedded
and signed by the App target, but are not passed as plugin paths.

SourceNormalizer diagnostics and preflight modes do not change playback. In
`preferNormalized` and `requireNormalized`, the host may open a disk-backed
normalized resource session and hand the resulting fMP4 or short-window HLS
resource to AVPlayer through a `vesper-normalized://` resource loader.
`preferNormalized` falls back to the original source on failure;
`requireNormalized` reports a source error. Standard HLS and DASH stay
native-first unless normalization is explicitly required or forced. The plugin
diagnostics panel shows route, profile, cache usage, fallback reason, and
participation. FrameProcessor remains debug diagnostics only in this example
and is never marked as participating in mobile playback.

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
