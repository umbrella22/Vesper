# VesperPlayerKit for iOS

iOS-native host kit for the Vesper Player SDK. Distributed as a Swift Package
or a prebuilt `XCFramework`, and consumable from any UIKit / SwiftUI app.

The distribution keeps `VesperPlayerKit` as its Swift package, product, and
module name. First-party bundle and plugin identities use the
`io.github.umbrella22.vesper` root; identifiers from the unreleased
`io.github.ikaros` source line are not accepted as compatibility aliases.

## Delivery

- `Package.swift` — local Swift Package consumed by app projects
- `project.yml` — XcodeGen descriptor for the framework / `XCFramework` build

Tagged GitHub Releases publish the following core artifacts via
`.github/workflows/mobile-lib-release.yml`:

- `VesperPlayerKit-ios-arm64.framework.zip` — device-only packaging
- `VesperPlayerKit-ios-simulator-arm64.framework.zip` — Apple Silicon Simulator
- `VesperPlayerKit.xcframework.zip` — combined device + Apple Silicon Simulator

The same release also publishes three FFmpeg component XCFrameworks and four
plugin XCFrameworks as an optional sibling set:

- `VesperFFmpegAVCodec.xcframework.zip`
- `VesperFFmpegAVFormat.xcframework.zip`
- `VesperFFmpegAVUtil.xcframework.zip`
- `VesperPlayerRemuxFfmpegPlugin.xcframework.zip`
- `VesperPlayerSourceNormalizerFfmpegPlugin.xcframework.zip`
- `VesperPlayerDecoderVideoToolboxPlugin.xcframework.zip`
- `VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip`

The optional set is released only with both mandatory redistribution assets:

- `VesperPlayerOptionalPlugins-FFmpeg-Compliance.zip` — FFmpeg licenses,
  notices, exact build metadata, and LGPL relinking instructions
- `VesperPlayerOptionalPlugins-FFmpeg-<version>-source.tar.xz` — exact
  corresponding FFmpeg source

Release verification fails if any optional framework or either redistribution
asset is absent, if the corresponding-source archive count is not exactly one,
if an extra top-level asset or XCFramework slice is present, if FFmpeg profile
metadata differs between sibling frameworks, if license and notice material is
empty or differs from its source, or if a legacy umbrella framework or bare
dylib is present.

Tagged binary packaging is `arm64`-only across iOS device and Apple Silicon
Simulator. The current release archives do not contain Catalyst slices. See
[Release Downloads](../../../README.md#release-downloads) for the public
package names and artifact-selection notes.

## Minimum Requirements

- iOS 17.0+
- Xcode 16+
- Apple Silicon Mac for Simulator builds
- Rust toolchain with iOS targets installed (when consuming as a local Swift Package)

These requirements define the supported product boundary. The package does not
plan to add older iOS deployment targets or Intel Simulator slices without a
separate product-direction change.

## Installation

### Swift Package (remote)

Stable releases are available from the dedicated package repository:

```text
https://github.com/umbrella22/VesperPlayerKit.git
```

Add that URL in Xcode's package dependency editor and select a released
`MAJOR.MINOR.PATCH` version. Link the `VesperPlayerKit` product for the binary
host kit. Link `VesperPlayerKitUI` as well when the app uses the version-matched
SwiftUI stage and controls.

The package repository contains the manifest, license, and
`VesperPlayerKitUI` sources. Its `VesperPlayerKit` binary target references the
matching `VesperPlayerKit.xcframework.zip` GitHub Release asset and pins the
archive checksum. Prerelease tags remain GitHub prereleases and are not pushed
to the package repository.

### Swift Package (local)

For app projects in this repository, depend on `lib/ios/VesperPlayerKit` as a
local Swift Package. Build the Rust resolver bundle once before resolving the
package:

```sh
./scripts/vesper ios ffi
```

### XCFramework

For distribution, build the core FFmpeg-free framework:

```sh
./scripts/vesper ios kit-xcframework
./scripts/vesper ios stage-release /tmp/vesper-ios-release
./scripts/vesper ios verify-release /tmp/vesper-ios-release --scope core
```

To reproduce the complete tagged-release set locally, enable optional staging:

```sh
./scripts/vesper ios stage-release /tmp/vesper-ios-release \
  --include-optional-plugins
./scripts/vesper ios verify-release /tmp/vesper-ios-release --scope complete
```

For physical-device acceptance of those verified optional artifacts, run the
separate `ios verify-optional-plugins-device` gate documented in
[`scripts/README.md`](../../../scripts/README.md). It retains the Release input
hashes, sanitized tested hashes, and XCResult rather than treating archive
verification as device evidence.

The Rust CLI stages the complete release file set and the optional Swift package
`Artifacts/` directory before committing either destination. Use
`--package-artifacts-directory <PATH>` only when a release workflow needs an
explicit package-artifact destination outside the default package directory.

The build script:

- Compiles the Rust `player-ffi-ios` Apple bundle
- Regenerates the framework project with `xcodegen`
- Archives iOS + iOS Simulator frameworks
- Produces `VesperPlayerKit.xcframework`

The iOS Rust build scripts resolve the Cargo workspace through the SDK root
manifest. They are safe to call from Xcode build phases, CI jobs, Flutter plugin
builds, or any other working directory.

### Swift Package Publishing

The `publish-swift-package` release job runs after the GitHub Release. It
downloads the released XCFramework, computes the SwiftPM checksum, validates the
generated manifest, and atomically pushes the package branch and matching tag.
Publishing requires:

- a public `umbrella22/VesperPlayerKit` repository, or another repository named
  by the `SPM_REPOSITORY` GitHub variable;
- an `SPM_PUBLISH_TOKEN` GitHub secret with Contents write access to that
  repository.

An existing matching tag is treated as a successful retry. A tag whose managed
package files differ is rejected. Local generation and manifest validation do
not require a token or write to GitHub:

```sh
./scripts/vesper ios publish-spm-index \
  v0.4.0 \
  dist/release/ios/VesperPlayerKit.xcframework.zip \
  --source-repository umbrella22/Vesper \
  --dry-run
```

## Public API

- `VesperPlayerController` — playback control surface (`@MainActor`); exposes `@Published` `uiState`, `trackCatalog`, `trackSelection`, `subtitleState`, requested/confirmed/effective subtitle selection, `effectiveVideoTrackId`, `videoVariantObservation`, `fixedTrackStatus`, `resiliencePolicy`, `lastError`
- `VesperPlayerControllerFactory` — controller construction with policy presets
- `VesperPlayerSource` — media source DTO with `localFile(url:)`, `remoteUrl(_:)`, `hls(url:)`, `dash(url:)` factories
- `VesperPlayerDrmConfiguration` — FairPlay license metadata for direct AVPlayer playback
- `PlayerSurfaceContainer` — `UIViewRepresentable` SwiftUI surface
- `PlayerHostUiState` — published UI state DTO
- `VesperTrackSelection` — `.auto` / `.disabled` / `.track(id:)`
- `VesperAbrPolicy` — adaptive bitrate policy (`auto`, `constrained`, `fixedTrack`)
- `VesperPlaybackResiliencePolicy` with presets: `.balanced()`, `.streaming()`, `.resilient()`, `.lowLatency()`
- `VesperBufferingPolicy`, `VesperRetryPolicy`, `VesperCachePolicy`
- `VesperPreloadBudgetPolicy` — caps for concurrent preload tasks, memory, disk, warm-up window
- `VesperTrackPreferencePolicy` — preferred audio / subtitle languages
- `VesperExternalSubtitleSource` and `VesperSubtitleStyle` — bounded external SRT / WebVTT / SSA parsing, visibility, and font scaling
- `VesperCodecSupport` — hardware decode capability probe
- `VesperDownloadManager` — download orchestration with `createTask / startTask / pauseTask / resumeTask / removeTask / exportTaskOutput / shareTaskOutput / saveTaskOutput / drainEvents`

The Flutter adapter uses `@_spi(VesperFlutter)` async source and seek entry
points. Source selection waits within one total deadline for a stable VOD, live
DVR, or non-seekable live timeline across retries. Seek completion requires
AVPlayer to report `finished == true` for the current command and source epoch.
Superseded SPI commands throw a structured obsolete `VesperPlayerError` without
replacing the current controller `lastError`. The synchronous Swift APIs remain
available for ordinary native hosts.

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
| Swift | `AVAssetResourceLoaderDelegate` + `vesper-dash://` URL routing, guarded `URLSession` requests, header injection, segment cache, prefetch, AVPlayer integration                            |

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

## DRM Boundary

iOS DRM support is native-only. Set `VesperPlayerSource.drmConfiguration` with
`keySystem = "fairPlay"` to let the direct AVPlayer route manage FairPlay
through `AVContentKeySession`. `fairPlayCertificateBase64` takes precedence over
`fairPlayCertificateUri`; one of them must be present for FairPlay playback.
License requests use `licenseUri` and `licenseHeaders`, and media headers are
not mixed into license requests.

FairPlay certificate, SPC, and CKC request failures are surfaced as retriable
runtime/network playback errors. Unsupported key systems, missing certificates,
and simulator FairPlay restrictions remain unsupported capability errors. The
default FairPlay content identifier preserves the full `skd://` identifier
string, including path and query, instead of truncating it to the host.

Retryable FairPlay runtime failures follow the active
`VesperPlaybackResiliencePolicy`. While retries remain, the controller only
updates the retry subtitle. After the retry budget is exhausted, the controller
pauses playback, clears buffering, and publishes `lastError` with sanitized
diagnostics such as `reason`, `keySystem`, `route`, `licenseUriHost`,
`certificateUriHost`, `httpStatusCode`, `attemptsExhausted`, and
`maxAttempts`. Full license URLs, headers, tokens, and certificate payloads are
not included in diagnostics.

DRM sources do not enter Rust, FFI, optional plugins, download, preload, remux,
SourceNormalizer, SDK-managed native-frame playback, the DASH bridge, or
external playback routes. Those paths fail with an unsupported capability error
instead of silently stripping DRM. Non-FairPlay key systems on iOS also fail as
unsupported.

Private encryption that is not FairPlay should be handled by a separate
host-owned or future SDK pre-decryption adapter before a normal source is handed
to AVPlayer. That is outside the current DRM contract.

## Subtitle Baseline

AVPlayer remains responsible for embedded legible tracks. The host kit applies
`VesperSubtitleStyle` through AVPlayer text style rules and supports external
UTF-8 SRT, WebVTT, and SSA/ASS files through a native overlay driven by the
player timeline. External subtitle input is bounded to eight tracks, 2 MiB per
track, 10,000 cues, and 16,384 characters per cue; unsupported URI schemes,
encodings, MIME types, and oversized inputs fail explicitly.

External tracks appear in the normal subtitle track catalog under the
source-local id supplied by `VesperExternalSubtitleSource`. All catalog ids are
opaque to callers. `setSubtitleTrackSelection` is `async throws` in 0.4 and
returns only after AVPlayer or the overlay backend confirms convergence.
`VesperSubtitleSideLoad` and `subtitleConfigurations` remain deprecated aliases.
Per-cue typography, animation fidelity, and
subtitle synchronization offsets are outside the stable contract. RTMP, RTSP,
and HTTP-FLV direct playback remain explicit capability errors on iOS. HTTP
`.flv` URLs infer progressive VOD unless callers set the live protocol
explicitly.

## Download Manager

`VesperDownloadManager` supports single-file and segmented downloads. For remote
VOD HLS, static DASH, and FLV inputs, the iOS host kit runs a native prepare
phase before transfer starts. The prepare phase expands manifests or clip lists,
resolves byte ranges, requires known remote byte totals, writes local rewritten
manifests or concat lists, and publishes a compact `taskUpdated` event before
download progress begins. Progress and state changes are incremental patches;
only task creation, asset-index replacement, and recovered plans carry full task
snapshots.

The default configuration also persists task snapshots, restores interrupted
tasks on startup, and resumes partially written remote files with range requests
when the server supports them. Pause, resume, and remove operations are keyed by
`taskId`; host UI state should not merge tasks by URL. If a server ignores a
resume range, the manager deletes only that partial resource and restarts the
same resource from byte zero. Expired or unavailable URLs fail with a
stale-resource error so the host can refresh the media link.

Remote media URLs used by the iOS offline downloader and DASH bridge must be
HTTPS. The SDK does not relax App Transport Security for `http://` media
resources; host apps that must support insecure HTTP should fetch those
resources outside the SDK and provide local file URLs.

The foreground executor streams complete resources by default and sends `Range:
bytes=<existing>-` only when resuming a partial file. Fixed closed Range chunks
are used only when `rangeChunkBytes` is explicitly configured. Each `206 Partial
Content` response is validated against `Content-Range` before bytes are
appended, and the SDK-created download directories, state file, generated
resources, and final offline files are marked as excluded from iCloud backup.

When `VesperPlayerSource.headers` is set, the download executor forwards those
headers to all SDK-owned network operations for the task: HLS, DASH, and FLV
manifest reads; HEAD and `Range: bytes=0-0` size probes; single-file transfers;
HLS map and segment transfers; DASH init and media segment transfers; FLV clip
transfers; and size completion for prebuilt asset indexes. Empty header names
and blank values are ignored, and the SDK does not add site-specific headers on
its own.

Hosts that can refresh signed or short-lived media URLs may pass
`staleResourceRecoveryHandler` to `VesperDownloadManager`. The handler receives
the failed task and a `VesperDownloadStaleResource`, returns a refreshed
`VesperDownloadSource`, and the executor re-runs preparation before starting the
same task. If no handler is provided, stale resources fail normally.

This is an SDK-managed foreground executor, not an iOS background
`URLSessionConfiguration.background` implementation. Hosts that need OS-managed
process-death background transfer should own that background session layer and
feed completed local assets back into the SDK.

Completed files can be exposed through `shareTaskOutput(...)`, which presents a
`UIActivityViewController`, or `saveTaskOutput(...)`, which presents an iOS
document export picker. `exportTaskOutput(...)` still writes to an explicit
host-provided path and keeps the original offline file in place.

## Optional iOS Plugin Package

FFmpeg is not embedded in the core `VesperPlayerKit.xcframework`. Repository
hosts stage the canonical optional package before SwiftPM resolution:

```sh
./scripts/vesper ios stage-optional-plugins-release \
  /tmp/vesper-ios-optional-plugins-release \
  --profile source-normalizer \
  ios-arm64 ios-simulator-arm64
```

The App target depends on the local `VesperPlayerOptionalPlugins` Swift package
and embeds its seven same-named binary products with Embed & Sign. SwiftPM then
places the three FFmpeg component frameworks and four plugin frameworks as
top-level siblings under `App.app/Frameworks`. Do not create flat dylibs,
nested frameworks, or the legacy `VesperPlayerFfmpegRuntime.framework` umbrella.

At runtime, select the remux plugin with a native `VesperPluginReference` in
`VesperDownloadConfiguration.postDownloadPluginReferences`. The host kit maps
that reference to the embedded signed plugin framework; executable paths stay
internal artifact locators. The three FFmpeg component frameworks are dynamic
dependencies, not plugin entries. All FFmpeg-backed siblings must carry the
same `profile-hash.txt` value.

Bundling these components makes the host responsible for FFmpeg notices,
corresponding source, configure flags, and LGPL relinking rights. See
[THIRD_PARTY_NOTICES.md](../../../THIRD_PARTY_NOTICES.md) before publishing such
an artifact. The canonical staging command generates and verifies
`VesperPlayerOptionalPlugins-FFmpeg-Compliance.zip` together with the versioned
corresponding-source archive; downstream app distributors must preserve that
redistribution boundary.

## Optional Mobile Plugin Routes

`VesperSourceNormalizerConfiguration` and `VesperFrameProcessorConfiguration`
are disabled by default. When enabled, their `pluginReferences` select explicit
native plugin identities. `VesperBundledPluginReferences` provides canonical
references for the distributed plugins, and the host kit maps those references
to signed framework executables internally. The App target embeds the FFmpeg
component frameworks as sibling dynamic dependencies; they are not plugin
registry entries. An empty reference list selects no plugin.

SourceNormalizer mobile supports `diagnosticsOnly`, `preflightOnly`,
`preferNormalized`, and `requireNormalized`. Diagnostics mode loads the optional
plugin and reports its capabilities through
`VesperPlayerController.pluginDiagnostics`. Preflight mode opens and closes a
packet session for the selected source without changing playback.
`preferNormalized` and `requireNormalized` may open a disk-backed normalized
resource session and hand the resulting fMP4 or short-window HLS resource to
AVPlayer. `preferNormalized` falls back to the original source when
normalization fails; `requireNormalized` reports a source error. Standard DASH
still uses the existing DASH bridge unless normalization is explicitly required.
Standard HLS and DASH stay native-first by default, and the repository smoke
expectations live in `fixtures/media/source-normalizer-smoke-matrix.json`.

The optional `VesperPlayerSourceNormalizerFfmpegPlugin.xcframework.zip` depends
on the sibling `VesperFFmpegAVCodec`, `VesperFFmpegAVFormat`, and
`VesperFFmpegAVUtil` XCFrameworks. All four must be built from the same FFmpeg
profile so their `profile-hash.txt` values match. The SourceNormalizer plugin
XCFramework must not contain duplicate FFmpeg libraries.

FrameProcessor mobile remains explicit through
`VesperNativeFramePipelineConfiguration`. The iOS host kit now exposes the
native-frame route decision for SourceNormalizer packet input, VideoToolbox,
MetalLayer presentation, Swift native audio bridge status, fallback reason, and
frame counters. Local/VOD SDR native-frame playback is explicitly opt-in:
`preferNativeFrame` falls back to AVPlayer when the packet, decoder, surface, or
presenter path cannot start, while `requireNativeFrame` reports a capability
error. HDR and Dolby Vision remain on AVPlayer / system playback; the
SDK-managed native-frame path is SDR-only today and is not an HDR-ready path.
The capability probe reports `recommendedPlaybackPath = systemPlayer` with
`hdrNativeFrameUnsupported` diagnostics instead of advertising native-frame HDR
support. Default AVPlayer playback is unchanged.

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
