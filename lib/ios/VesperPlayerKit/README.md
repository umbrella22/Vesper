# VesperPlayerKit for iOS

iOS-native host kit for the Vesper Player SDK. Distributed as a Swift Package
or a prebuilt `XCFramework`, and consumable from any UIKit / SwiftUI app.

## Delivery

- `Package.swift` — local Swift Package consumed by app projects
- `project.yml` — XcodeGen descriptor for the framework / `XCFramework` build

GitHub Releases publish the following artifacts via
`.github/workflows/mobile-lib-release.yml`:

- `VesperPlayerKit-ios-arm64.framework.zip` — device-only packaging
- `VesperPlayerKit-ios-simulator-arm64.framework.zip` — Apple Silicon Simulator
- `VesperPlayerKit.xcframework.zip` — combined device + Apple Silicon Simulator

Apple packaging is `arm64`-only across iOS device, iOS Simulator, and (when
enabled) Mac Catalyst. Use an Apple Silicon Mac for Simulator validation. See
[Release Downloads](../../../README.md#release-downloads) for the public
package names and artifact-selection notes.

## Minimum Requirements

- iOS 14.0+
- Xcode 16+
- Apple Silicon Mac for Simulator builds
- Rust toolchain with iOS targets installed (when consuming as a local Swift Package)

## Installation

### Swift Package (local)

For app projects in this repository, depend on `lib/ios/VesperPlayerKit` as a
local Swift Package. Build the Rust resolver bundle once before resolving the
package:

```sh
./scripts/vesper ios ffi
```

### XCFramework

For distribution, build a self-contained framework:

```sh
./scripts/vesper ios kit-xcframework
./scripts/vesper ios stage-release
```

The build script:

- Compiles the Rust `player-ffi-ios` Apple bundle
- Regenerates the framework project with `xcodegen`
- Archives iOS + iOS Simulator frameworks
- Produces `VesperPlayerKit.xcframework`

## Public API

- `VesperPlayerController` — playback control surface (`@MainActor`); exposes `@Published` `uiState`, `trackCatalog`, `trackSelection`, `effectiveVideoTrackId`, `videoVariantObservation`, `fixedTrackStatus`, `resiliencePolicy`, `lastError`
- `VesperPlayerControllerFactory` — controller construction with policy presets
- `VesperPlayerSource` — media source DTO with `localFile(url:)`, `remoteUrl(_:)`, `hls(url:)`, `dash(url:)` factories
- `PlayerSurfaceContainer` — `UIViewRepresentable` SwiftUI surface
- `PlayerHostUiState` — published UI state DTO
- `VesperTrackSelection` — `.auto` / `.disabled` / `.track(id:)`
- `VesperAbrPolicy` — adaptive bitrate policy (`auto`, `constrained`, `fixedTrack`)
- `VesperPlaybackResiliencePolicy` with presets: `.balanced()`, `.streaming()`, `.resilient()`, `.lowLatency()`
- `VesperBufferingPolicy`, `VesperRetryPolicy`, `VesperCachePolicy`
- `VesperPreloadBudgetPolicy` — caps for concurrent preload tasks, memory, disk, warm-up window
- `VesperTrackPreferencePolicy` — preferred audio / subtitle languages
- `VesperCodecSupport` — hardware decode capability probe
- `VesperDownloadManager` — download orchestration with `createTask / startTask / pauseTask / resumeTask / removeTask / exportTaskOutput / drainEvents`

The package does not embed demo URLs or preset sources. Construct
`VesperPlayerSource` from your own content. A runnable sample lives at
[`examples/ios-swift-host`](../../../examples/ios-swift-host/).

## Minimal SwiftUI Usage

```swift
import VesperPlayerKit
import SwiftUI

struct PlayerView: View {
    @StateObject private var controller = VesperPlayerControllerFactory.makeDefault(
        resiliencePolicy: .resilient()
    )

    var body: some View {
        VStack {
            PlayerSurfaceContainer(controller: controller)
                .frame(height: 240)

            Text(controller.uiState.playbackState.rawValue)

            Button("Play") { controller.play() }
        }
        .onAppear { controller.initialize() }
        .onDisappear { controller.dispose() }
    }
}
```

## Resilience Policy

`VesperPlaybackResiliencePolicy` shapes `AVPlayer` buffering and controlled
retry/backoff for remote sources. Cache configuration is mapped as a
best-effort process-wide `URLCache.shared` capacity hint for remote playback;
it does not match the transport depth that Media3 offers on Android.

## Hardware Decode Probe

`VesperCodecSupport.hardwareDecodeSupported(for:)` normalizes common codec
aliases (`H264 / AVC / AVC1`, `HEVC / H265 / HVC1 / HEV1`) and checks
VideoToolbox support. Unknown codec names return `false`.

## Adaptive Bitrate

`VesperPlayerKit` exposes two ABR routes on top of `AVPlayer`:

- `VesperAbrPolicy.constrained(...)`
- `VesperAbrPolicy.fixedTrack(...)`

iOS-specific semantics:

- `fixedTrack` is best-effort HLS / DASH variant pinning on iOS 15+, not exact
  AVPlayer video-track switching. `supportsVideoTrackSelection` reports
  unsupported on iOS while `supportsAbrFixedTrack` reports supported as
  best-effort pinning.
- Single-axis constraints such as `constrained(maxHeight: 720)` are supported
  for HLS and the DASH bridge but apply only after the variant catalog is
  available, so the missing axis can be inferred safely.
- `effectiveVideoTrackId` is best-effort: derived from the current HLS / DASH
  variant ladder, access-log bitrate, and presentation size.
- `videoVariantObservation` exposes the raw runtime evidence (access-log
  bitrate, latest rendered presentation size).
- `fixedTrackStatus` reports best-effort convergence: `.pending` while
  evidence is settling, `.locked` after stable match, `.fallback` after
  sustained mismatch.
- Resilience reload defers `fixedTrack` and single-axis constrained ABR until
  the variant catalog is loaded.
- If a restored fixed-track `trackId` no longer exists verbatim after the HLS
  ladder drifts, the host attempts to remap it onto the closest semantically
  equivalent variant.
- If a restored fixed-track request keeps rendering a different observed
  variant under sustained evidence, the host surfaces a non-fatal `lastError`
  and degrades the request into constrained ABR using the requested limits,
  otherwise back to automatic ABR.

## DASH Support

DASH playback uses a Rust core (`crates/core/player-dash-hls-bridge`)
plus a thin Swift transport layer. It supports single-period fMP4 manifests for
static VOD and dynamic live / DVR when they use either `SegmentBase + sidx` or
`SegmentTemplate` / `SegmentTimeline` addressing. The bridge rejects DRM
`ContentProtection`, `SegmentList`, and multi-period manifests.

Responsibility split:

| Layer | Responsibilities                                                                                                                                                                          |
| ----- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Rust  | MPD / `SegmentBase` / `SegmentTemplate` / `SegmentTimeline` parsing, SIDX parsing, representation selection, HLS playlist generation, template expansion                                  |
| Swift | `AVAssetResourceLoaderDelegate` + `vesper-dash://` URL routing, `URLSession` requests, header injection, `NWListener` loopback HTTP server, segment cache, prefetch, AVPlayer integration |

FFI entry point (single coarse-grained JSON op):

- Rust: `player_dash_hls_bridge::ops::execute_json`
- C export: `player_ffi_dash_bridge_execute_json` (provided by the
  `player-ffi-ios` Apple bundle, **not** by `include/player_ffi.h`)
- Swift call site: `VesperPlayerKitBridgeShim`

Segment caching:

- Per-session LRU file cache: max 160 entries, max 256 MiB total
- Segments larger than 32 MiB stream through a session temp file in 256 KiB
  chunks instead of being held in memory

ABR behavior:

- The synthesized HLS master playlist exposes the playable DASH audio, video,
  and WebVTT subtitle renditions. Unsupported video codecs are filtered through
  `VesperCodecSupport` before the bridge exposes the HLS ladder.
- The DASH manifest track catalog exposes playable audio, video, and subtitle
  tracks so host UI can render a complete source-specific catalog.
- The synthesized HLS master playlist exposes the playable DASH variant ladder
  so AVPlayer can perform ABR.
- Startup prefetch targets a single variant; oversized media segments are
  skipped
- `VesperAbrPolicy` applies to both HLS and the DASH bridge

## Download Manager

`VesperDownloadManager` supports single-file and segmented downloads.

Recommended flow for remote HLS:

1. Show an optimistic "preparing" entry in the UI when the user starts a download.
2. Read the manifest in the background and build
   `VesperDownloadAssetIndex(resources + segments)` plus a dedicated
   `targetDirectory`.
3. Call `createTask(...)` only after the source / profile / asset index are ready.

Notes:

- The foreground executor downloads `assetIndex.resources + assetIndex.segments`
  together when both are provided.
- Pause / resume / remove are keyed by `taskId`; do not merge tasks by URL in
  host UI state.
- The bundled iOS example wires this segmented flow for HLS only. DASH
  download is not supported on the AVPlayer backend.

## Optional FFmpeg Remux Plugin

`exportTaskOutput(...)` uses an optional `player-remux-ffmpeg` dynamic plugin
when the host wants to export downloaded HLS or DASH assets to `.mp4`. The
host must embed a signed `libplayer_remux_ffmpeg.dylib` in the app bundle and
pass its absolute path through `VesperDownloadConfiguration.pluginLibraryPaths`.

Bundling that plugin makes the host responsible for FFmpeg notices,
corresponding source, configure flags, and LGPL relinking rights. See
[THIRD_PARTY_NOTICES.md](../../../THIRD_PARTY_NOTICES.md) before publishing
such an artifact.

## Testing The Package

Use Xcode for native unit tests; `swift test` will compile for the host macOS
target where UIKit is unavailable.

iOS Simulator (replace `<SIMULATOR_ID>` with an installed Simulator):

```sh
cd lib/ios/VesperPlayerKit
xcodegen generate
xcodebuild \
  -project VesperPlayerKit.xcodeproj \
  -scheme VesperPlayerKit \
  -destination 'id=<SIMULATOR_ID>' \
  ARCHS=arm64 ONLY_ACTIVE_ARCH=YES \
  CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO test
```

Mac Catalyst:

```sh
cd lib/ios/VesperPlayerKit
xcodegen generate
xcodebuild \
  -project VesperPlayerKit.xcodeproj \
  -scheme VesperPlayerKit \
  -destination 'platform=macOS,variant=Mac Catalyst,name=My Mac' \
  ARCHS=arm64 ONLY_ACTIVE_ARCH=YES \
  CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO test
```

List Simulator IDs:

```sh
cd lib/ios/VesperPlayerKit
xcodegen generate
xcodebuild \
  -project VesperPlayerKit.xcodeproj \
  -scheme VesperPlayerKit -showdestinations
```

DASH bridge tests:

```sh
cargo test -p player-dash-hls-bridge -p player-ffi-ios --lib
./scripts/vesper ios ffi debug
cd lib/ios/VesperPlayerKit
xcodegen generate
xcodebuild test \
  -project VesperPlayerKit.xcodeproj \
  -scheme VesperPlayerKit \
  -destination 'platform=iOS Simulator,name=iPhone 17' \
  -only-testing:VesperPlayerKitTests/VesperDashBridgeTests \
  CODE_SIGNING_ALLOWED=NO
```

## Runnable Sample

A SwiftUI sample app that consumes this package lives at
[`examples/ios-swift-host`](../../../examples/ios-swift-host/).
