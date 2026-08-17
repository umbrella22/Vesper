# Vesper Player SDK

Language: [Simplified Chinese](README.zh-CN.md)

Vesper is a native-first, multi-platform player SDK for applications that need
real platform playback behavior without rebuilding every product feature from
scratch on each target. Android playback runs through Media3 ExoPlayer, iOS
playback runs through AVPlayer, desktop playback uses native Rust pipelines,
and Flutter mobile apps consume the same capabilities through a federated
plugin.

The shared Rust layer keeps cross-platform semantics aligned: runtime contracts,
timeline and live-DVR state, playback resilience, ABR policy, playlist
coordination, preload and download planning, DASH bridging, and the public C ABI.
Platform host kits stay responsible for the rendering surface, lifecycle, native
media stack integration, and platform-specific capability reporting.

## Product Boundary

Vesper targets modern arm64 mobile platforms: Android API 26+ on `arm64-v8a`,
and iOS 17+ on arm64 devices and Apple Silicon Simulator. This platform floor is
a product boundary, not a compatibility backlog; older mobile OS versions,
32-bit Android, Intel Android ABIs, and Intel iOS Simulator are not planned.

The mobile production path remains Media3 on Android and AVPlayer on iOS. Vesper
does not aim to become a universal FFmpeg-first mobile engine or an ijkplayer
compatibility layer. Desktop FFmpeg playback, native-frame routes, decoder
plugins, FrameProcessor, and SourceNormalizer are experimental or optional
surfaces and do not define mobile release readiness.

## Start Here

Choose the integration path that matches your app. Read the first document for
the public API and packaging model, then use the example app as a runnable
reference. The complete host-kit, optional-package, artifact, and Stage UI map
lives in the [platform package guide](lib/README.md).

| Target                   | Read first                                                                                                       | Run / inspect next                                                                 | Useful when                                                                           |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| Android Kotlin / Compose | [lib/android/README.md](lib/android/README.md)                                                                   | [examples/android-compose-host/README.md](examples/android-compose-host/README.md) | You are integrating the AAR modules directly in an Android app.                       |
| iOS Swift / SwiftUI      | [lib/ios/VesperPlayerKit/README.md](lib/ios/VesperPlayerKit/README.md)                                           | [examples/ios-swift-host/README.md](examples/ios-swift-host/README.md)             | You are consuming the Swift Package or XCFramework from a UIKit / SwiftUI app.        |
| Flutter                  | [lib/flutter/vesper_player/README.md](lib/flutter/vesper_player/README.md)                                       | [examples/flutter-host/README.md](examples/flutter-host/README.md)                 | You want one Dart API over Android and iOS today; desktop Flutter targets are paused. |
| Flutter platform authors | [lib/flutter/vesper_player_platform_interface/README.md](lib/flutter/vesper_player_platform_interface/README.md) | [lib/flutter/vesper_player_ui/README.md](lib/flutter/vesper_player_ui/README.md)   | You are extending the federated plugin or adopting the optional Flutter UI package.   |
| C / C++ via FFI          | [include/player_ffi.h](include/player_ffi.h)                                                                     | [examples/c-host/README.md](examples/c-host/README.md)                             | You need the generated C ABI from a native host or plugin runtime.                    |
| Desktop Rust             | [examples/basic-player](examples/basic-player)                                                                   | [Desktop FFmpeg](#desktop-ffmpeg)                                                  | You are trying the desktop demo or working with the Rust playback pipeline.           |

## What You Get

- Native playback per platform: Media3 on Android, AVPlayer on iOS, and Rust
  desktop backends.
- Shared playback semantics for timeline, live edge, live DVR, track catalog,
  ABR, resilience policy, preload policy, and download orchestration.
- Offline download planning for VOD HLS, static DASH, and FLV inputs, with
  source HTTP headers applied consistently to manifest fetches, size probes,
  segment transfers, and optional MP4 stream-copy export through the remux
  plugin.
- SDK-managed offline task restore and resumable range transfers on Android and
  iOS, plus a shared desktop host download service for macOS, Windows, and Linux,
  including per-resource restart when an HTTP server ignores resume ranges and
  bounded Range chunks for known-size HTTP resources, and stale-resource errors
  with host-provided recovery hooks for expired or rejected media URLs.
- Configurable screen-awake handling while playback is active on Android, iOS,
  and Flutter mobile hosts.
- Optional Android external playback through Google Cast, DLNA / UPnP AV, and a
  local HTTP relay for protected headers, local files, and `content://` sources.
- Platform-native surfaces instead of frame-copy rendering paths for mobile
  playback.
- Native-only DRM playback integration for direct mobile playback paths:
  Widevine is configured on Android Media3, and FairPlay is configured on iOS
  AVPlayer. DRM sources are not processed by Rust, FFI, plugins, download,
  preload, remux, SourceNormalizer, or external playback relays.
- Typed plugin architecture with stable `PostDownloadProcessor`,
  `PipelineEventHook`, and `BenchmarkSink` interfaces. `NativeDecoder`,
  `FrameProcessor`, and packet/resource `SourceNormalizer` interfaces remain
  experimental.
- Plugin authors use the safe Rust SDK and explicit `PluginReference` values.
  Native plugins export one `vesper_plugin_entry`; Rust WASM Component
  plugins are limited to desktop/tooling `PipelineEventHook` and
  `BenchmarkSink` workloads with bounded structured input. C/C++ author SDKs,
  mobile WASM, media-byte transforms, and DRM plugins are outside this release.
- The Rust `vesper plugin` CLI creates, checks, signs, packages, verifies, and
  installs deterministic `.vesper-plugin` archives. Native signatures prove
  publisher and artifact integrity; they do not sandbox trusted native code.
- Generated, generation-checked C value handles for hosts that integrate through
  the FFI boundary.
- Runnable host applications for Android, iOS, Flutter, desktop Rust, and C.

## Capability Matrix

This is a coarse overview of the feature surface. Each platform README explains
the exact behavior, fallback rules, and capability flags that host apps should
check before exposing advanced controls.

| Capability                 | Android (Media3)                  | iOS (AVPlayer)                             | Desktop Rust                                                                                                                                        | Flutter mobile                                       |
| -------------------------- | --------------------------------- | ------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------- |
| Local file                 | ✅                                | ✅                                         | ✅                                                                                                                                                  | ✅ Android / iOS                                     |
| Progressive HTTP/HTTPS     | ✅                                | ✅                                         | ✅                                                                                                                                                  | ✅ Android / iOS                                     |
| HLS (`.m3u8`)              | ✅                                | ✅                                         | ✅ FFmpeg demuxer, no desktop ABR switching yet                                                                                                     | ✅ Android / iOS                                     |
| DASH (`.mpd`)              | ✅ native                         | ✅ DASH-to-HLS bridge for VOD / live fMP4  | ⚠️ backend-dependent FFmpeg demuxer                                                                                                                 | ✅ Android native / iOS bridge                       |
| Live / DVR                 | ✅                                | ✅                                         | ✅                                                                                                                                                  | ✅ Android / iOS                                     |
| Track selection            | ✅ video / audio / subtitles      | ✅ audio / subtitles                       | ✅                                                                                                                                                  | ✅ per-platform semantics                            |
| External text subtitles    | ⚠️ SRT / WebVTT / SSA via Media3 | ⚠️ bounded SRT / WebVTT / SSA host overlay | ❌ not part of the experimental desktop contract                                                                                                    | ✅ Android / iOS channels                            |
| ABR `constrained` policy   | ✅                                | ✅ HLS + DASH bridge variant catalogs      | ⚠️ policy DTO only for desktop FFmpeg HLS                                                                                                           | ✅ per-platform semantics                            |
| ABR `fixedTrack` policy    | ✅ exact                          | ✅ best-effort HLS/DASH pinning on iOS 15+ | ⚠️ policy DTO only for desktop FFmpeg HLS                                                                                                           | ✅ per-platform semantics                            |
| Resilience policy          | ✅                                | ✅                                         | ✅                                                                                                                                                  | ✅ Android / iOS                                     |
| Preload budget             | ✅                                | ✅                                         | ⚠️ shared policy/planner only; `player-host-desktop` uses a noop executor today                                                                      | ✅ Android / iOS                                     |
| Download manager           | ✅ VOD prepare + restore + export | ✅ VOD prepare + restore + export          | ✅ public `player-host-desktop::download` service                                                                                                   | ✅ Android / iOS                                     |
| DRM direct playback        | ✅ Widevine through Media3 direct paths | ✅ FairPlay through AVPlayer direct paths | ⛔ not supported                                                                                                                                    | ✅ Android / iOS direct native paths only           |
| Hardware decode probe      | `VesperDecoderBackend`            | `VesperCodecSupport`                       | macOS VideoToolbox native-frame opt-in; Windows D3D11 roadmap; Linux software-only today                                                            | Reflected through mobile capabilities                |
| Plugin startup diagnostics | Internal runtime diagnostics      | Internal runtime diagnostics               | macOS / Windows decoder diagnostics; macOS frame processor chain; Linux reports unsupported diagnostics for configured plugin paths                | Exposed as create-result diagnostics where supported |

Flutter support is mobile-only for now. Desktop Flutter targets are intentionally
not shipped while the Flutter desktop integration model settles. Product UI
should rely on runtime capability flags rather than assuming every row above is
available on every backend.

## DRM Boundary

DRM in Vesper is a native-only playback integration, not a media-processing
feature. `VesperPlayerSource.drmConfiguration` is a public source DTO so hosts
can pass license metadata to the platform player, but protected media stays
inside the platform DRM stack: Android uses Media3 / Widevine, and iOS uses
AVPlayer / FairPlay.

Any route that rewrites the stream, changes the origin, or asks the SDK to touch
decoded frames rejects DRM sources with an unsupported capability error. This
includes SourceNormalizer, SDK-managed native-frame playback, the iOS DASH
bridge, download, preload, post-download remux, and external playback relay
flows. Plugins do not receive Widevine or FairPlay protected media.

If a future product needs a cloud-vendor-style private encryption format, that
must be designed as a separate pre-decryption adapter that turns private
encrypted input into a normal source for the platform player. It is not part of
the current Widevine / FairPlay DRM contract.

## Repository Layout

```text
.agents/     Repository Codex marketplace, maintainer agent, and Rust skills
crates/      Rust workspace: shared core, runtime, FFI, backends, render, platform glue
lib/         Distributable platform integration layers; start with lib/README.md
  android/   Android AAR modules: core kit, external playback, FFmpeg runtime, Compose adapter, optional Compose UI
  ios/       VesperPlayerKit Swift Package / XCFramework project
  flutter/   Federated Flutter packages: main API, platform packages, optional UI
examples/    Runnable host apps for Android, iOS, Flutter, desktop Rust, and C
include/     Generated C header: player_ffi.h
plugins/     Vesper runtime plugin projects and package manifests
schemas/     Public language-neutral Vesper plugin package schemas
templates/   Native Rust and Rust WASM plugin author scaffolds
wit/         Vesper WASM Component contracts
scripts/     Thin vesper launcher and checked build / release policy data
third_party/ Vendored dependencies and generated prebuilt media libraries
```

The public integration surface is concentrated under [lib/](lib/README.md),
[examples/](examples/), and [include/](include/). The Rust crates under
[crates/](crates/) power the shared runtime and platform bridges. Repository
Codex integrations live under [.agents/](.agents/README.md); they are separate
from the Vesper runtime plugins under [plugins/](plugins/).

## Quick Start

### Android Package

```kotlin
val controller = VesperPlayerControllerFactory.createDefault(
    context = context,
    initialSource = VesperPlayerSource.hls(
        uri = "https://example.com/master.m3u8",
        label = "Sample",
    ),
    resiliencePolicy = VesperPlaybackResiliencePolicy.resilient(),
)

VesperPlayerSurface(controller = controller)
```

Read the Android host kit guide at [lib/android/README.md](lib/android/README.md)
and use [examples/android-compose-host/README.md](examples/android-compose-host/README.md)
for a complete Compose app.

### iOS Package

```swift
@StateObject private var controller = VesperPlayerControllerFactory.makeDefault(
    resiliencePolicy: .resilient()
)

PlayerSurfaceContainer(controller: controller)
    .onAppear { controller.initialize() }
    .onDisappear { controller.dispose() }
```

Read the iOS host kit guide at
[lib/ios/VesperPlayerKit/README.md](lib/ios/VesperPlayerKit/README.md) and use
[examples/ios-swift-host/README.md](examples/ios-swift-host/README.md) for the
SwiftUI sample app.

### Flutter Packages

```dart
final controller = await VesperPlayerController.create(
  initialSource: VesperPlayerSource.hls(
    uri: 'https://example.com/master.m3u8',
  ),
);

VesperPlayerView(controller: controller)
```

Read the main Flutter package guide at
[lib/flutter/vesper_player/README.md](lib/flutter/vesper_player/README.md) and
use [examples/flutter-host/README.md](examples/flutter-host/README.md) for a
cross-platform app wired to the native host kits.

### Desktop Rust

```sh
cargo run -p basic-player
```

The desktop demo starts with an empty stage. Drag in a file, click "Open Local
File", or paste a remote URL into the playlist tab. See [Desktop FFmpeg](#desktop-ffmpeg)
for how FFmpeg is resolved when desktop builds need demuxing / decoding support.

Desktop plugin experiments are opt-in. `basic-player` can load native-frame
decoder plugins, frame processor diagnostic plugins, and packet-stream source
normalizer plugins through internal environment-configured paths. These routes
are for SDK development and diagnostics; Android and iOS public host-kit APIs
stay on native platform playback by default and select build-time embedded
artifacts with `VesperPluginReference`.

Desktop rendering caveat: the current `wgpu` software-render path uses the
repository shader path for SDR video and is calibrated around Rec.709 limited
range. HDR, Dolby Vision, wide-gamut, and desktop shader color-management work
remain outside the stable desktop surface.

### C ABI

Start with the generated header at [include/player_ffi.h](include/player_ffi.h),
then run the smoke example described in [examples/c-host/README.md](examples/c-host/README.md).

```sh
scripts/vesper ffi c-host-smoke
```

### Plugin authoring

Create a Rust Native or Rust WASM plugin outside this workspace with the
scaffold templates, then use the Rust CLI for the complete local workflow. The
following cross-platform example creates a WASM EventHook plugin:

```sh
vesper plugin new \
  --plugin-id dev.example.analytics \
  --publisher dev.example \
  --license Apache-2.0 \
  --transport wasm \
  --capability event-hook \
  ./analytics-plugin
cd ./analytics-plugin
vesper plugin build vesper-plugin.toml --profile dev
vesper plugin inspect vesper-plugin.toml --manifest-only
vesper plugin inspect vesper-plugin.toml \
  --artifact dist/vesper_plugin_analytics.wasm \
  --transport wasm
vesper plugin check vesper-plugin.toml \
  --artifact dist/vesper_plugin_analytics.wasm \
  --transport wasm
vesper plugin key generate \
  --publisher dev.example \
  --signing-key-output publisher-key.json \
  --trust-store-output trust-store.json
vesper plugin package vesper-plugin.toml \
  --signing-key publisher-key.json \
  --output analytics.vesper-plugin
vesper plugin verify analytics.vesper-plugin --trust-store trust-store.json
```

Native scaffolds use the same workflow with `--transport native`; the generated
plugin README and `artifacts[0].source` field provide the platform-specific
dynamic-library path for `inspect` and `check`.

This is the intended published author workflow. The `player-plugin` and
`player-plugin-wasm` crates are not on crates.io yet, so a newly generated
external project cannot currently resolve the SDK without a repository-local
Cargo patch. Native and WASM scaffolds have passed that local patched
acceptance path; public authoring availability remains a release gate tracked in
[`CURRENT-CHECKLIST.md`](CURRENT-CHECKLIST.md).

`vesper-plugin.toml` is the author-owned source record. Packaging generates the
canonical manifest, sorted `SHA256SUMS`, Ed25519 signature envelope, notices,
and target metadata. Mobile hosts embed verified Native artifacts during the
Android/iOS build; they do not download or execute plugin code at runtime.

## Platform Packages

The [platform package guide](lib/README.md) is the canonical package map. It
lists every Android, iOS, and Flutter package, separates core, optional, and
experimental surfaces, and includes the shared Stage UI gesture and integration
contract.

### Android

Android is distributed as AAR modules:

- `vesper-player-kit`: core controller, source model, JNI bridge, download
  manager, and native video surface selection.
- `vesper-player-kit-external-playback`: optional Google Cast, DLNA / UPnP AV,
  and local relay integration.
- `vesper-player-kit-ffmpeg-runtime`: optional FFmpeg runtime package used by
  remux and relay workflows.
- `vesper-player-kit-source-normalizer-ffmpeg`: optional SourceNormalizer
  plugin that depends on the core kit and shared FFmpeg runtime.
- `vesper-player-kit-remux-ffmpeg`: optional post-download MP4 remux plugin
  that depends on the core kit and shared FFmpeg runtime.
- Decoder and FrameProcessor plugin AARs: explicit experimental native-frame
  and diagnostic extensions built from the source checkout.
- `vesper-player-kit-compose`: Compose adapter with `VesperPlayerSurface` and
  controller/state helpers.
- `vesper-player-kit-compose-ui`: optional opinionated Compose player stage.

Minimum target: Android API 26+, Kotlin 2.x, and an arm64 device or emulator for
the published mobile artifacts.

### iOS

iOS is distributed as `VesperPlayerKit`, available as a local Swift Package for
source integration and as an XCFramework for release packaging. Public APIs are
Swift-first and designed for UIKit / SwiftUI hosts.

Minimum target: iOS 17.0+, Xcode 16+, and arm64 device / Apple Silicon Simulator
builds for the published artifacts.

### Flutter

Flutter is a federated plugin family:

- `vesper_player`: public Dart API and `VesperPlayerView`.
- `vesper_player_platform_interface`: shared DTOs and platform contracts.
- `vesper_player_android`: Android implementation over the Android host kit.
- `vesper_player_ios`: iOS implementation over `VesperPlayerKit`.
- `vesper_player_external_playback`: optional Android Cast / DLNA controller
  with local HTTP relay support.
- `vesper_player_source_normalizer_ffmpeg`: optional experimental native
  SourceNormalizer artifacts.
- `vesper_player_remux_ffmpeg`: optional native dependency package for
  post-download MP4 remux.
- `vesper_player_ui`: optional Flutter controls and player stage widgets.

The Flutter package family is published to pub.dev. SourceNormalizer and remux
remain direct, opt-in dependencies and are not pulled in by `vesper_player`.

## Building From Source

Common verification commands are listed below. Platform-specific setup and
toolchain notes live in the platform READMEs linked from [Start Here](#start-here).

```sh
# Rust workspace check
cargo check --workspace

# Generate / verify the C header
./scripts/vesper ffi generate
./scripts/vesper ffi verify

# Android AAR build
./scripts/vesper android aar

# iOS XCFramework build
./scripts/vesper ios kit-xcframework

# Desktop end-to-end remux integration test
./scripts/vesper desktop verify-remux
```

The Android CLI uses project-local cached Gradle distributions for local
development and a CI-provisioned `gradle` executable in GitHub Actions. Each
Android project also keeps its service home under
`<project>/.gradle/gradle-user-home`; the repository root has no shared Gradle
state. This keeps local agent work offline-safe while letting CI install Gradle
through `gradle/actions/setup-gradle`.

iOS CLI build commands resolve the workspace through the SDK root Cargo
manifest, so they can be called from Xcode build phases, Flutter plugin builds,
CI working directories, or the repository root without depending on the current
shell directory.

## Mobile FFmpeg Profiles

Android and iOS FFmpeg builds use the root profile CLI. The public entrypoint is
`./scripts/vesper ffmpeg --platform android|ios|all --profile <name>`.
`download-remux`, `relay-remux`, and `default` are local remux profiles: they
enable only local file/pipe protocols and validate that network and OpenSSL are
disabled. The default profile unions download and relay remux capabilities.

```sh
./scripts/vesper ffmpeg --platform android --profile default --abi arm64-v8a
./scripts/vesper ffmpeg --platform ios --profile default --slice ios-arm64 --slice ios-simulator-arm64
```

Source normalization uses a separate runtime-profile file at
`scripts/source-normalizer-profiles.toml`. Those profiles describe how unusual
or container-incompatible sources are detected and normalized at runtime; they
do not replace the build-time FFmpeg packaging profiles above.

Callers can add controlled overlays with `--extra-libraries`,
`--extra-demuxers`, `--extra-muxers`, `--extra-protocols`,
`--extra-parsers`, `--extra-bsfs`, and repeated `--extra-configure-arg` flags.
Validation fails if an overlay violates the selected profile policy. Generated
ABIs and slices record `vesper-ffmpeg-build-metadata.txt` with the declared
profile, profile hash, source archive SHA-256, license-sensitive flags, exact
configure line, and platform linker overrides for release review. Apple builds
record the Darwin shared-library flags that replace FFmpeg's obsolete
`-single_module` default without modifying the upstream source archive.

Android builds that explicitly opt into `--tls-backend openssl` provision
OpenSSL from the 3.5 LTS series by default. FFmpeg source builds default to the
8.1.x series. Both defaults resolve the highest matching patch already present
in `third_party/_cache` before consulting upstream release indexes; release
metadata still records the exact resolved version. Use `VESPER_FFMPEG_VERSION`
or `VESPER_ANDROID_OPENSSL_VERSION` for exact-version reproduction, and use
`VESPER_FFMPEG_SERIES` or `VESPER_ANDROID_OPENSSL_SERIES` only for intentional
series moves. Stale local OpenSSL prebuilts are rebuilt when their `openssl.pc`
version does not match the selected version.

## Desktop FFmpeg

Desktop Rust builds that link FFmpeg resolve libraries in this order:

1. Use the repository-local desktop FFmpeg install under
   `third_party/ffmpeg/desktop` when it already exists.
2. Otherwise use the latest system FFmpeg exposed through `pkg-config` or
   Homebrew `ffmpeg`.
3. If neither exists, fail with the normal `pkg-config` diagnostic.

Provision the repository-local macOS fallback explicitly before building:

```sh
./scripts/vesper desktop ensure-ffmpeg
```

The Rust CLI resolves the highest available patch in the configured FFmpeg
series, downloads or reuses the audited source archive, and atomically installs
the resulting static libraries under `third_party/ffmpeg/desktop`. Cargo does
not run provisioning code from a build-script wrapper.

The local source archive cache is `third_party/_cache` by default. FFmpeg,
OpenSSL, and libxml2 source archives are reused from that directory before any
download is attempted; missing archives are downloaded there from their upstream
release URLs.

Useful overrides:

| Variable                               | Purpose                                                         |
| -------------------------------------- | --------------------------------------------------------------- |
| `VESPER_DESKTOP_FFMPEG_DIR`            | Override the repository-local desktop FFmpeg install directory. |
| `VESPER_FFMPEG_SERIES`                 | Override the shared default FFmpeg source series.               |
| `VESPER_FFMPEG_VERSION`                | Override the shared FFmpeg source version exactly.              |
| `VESPER_DESKTOP_FFMPEG_SERIES`         | Override the desktop fallback FFmpeg source series.             |
| `VESPER_DESKTOP_FFMPEG_VERSION`        | Override the desktop fallback FFmpeg source version exactly.    |
| `VESPER_DESKTOP_FFMPEG_SOURCE_ARCHIVE` | Point to a pre-downloaded FFmpeg source archive.                |
| `VESPER_DESKTOP_FFMPEG_SOURCE_URL`     | Override the source download URL.                               |
| `VESPER_THIRD_PARTY_SOURCE_CACHE_DIR`  | Override the shared source archive cache directory.             |

When `VESPER_DESKTOP_FFMPEG_DIR` points outside the default repository path,
also expose its metadata to Cargo, for example:

```sh
export PKG_CONFIG_PATH="$VESPER_DESKTOP_FFMPEG_DIR/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
```

### FFmpeg License Compliance

Vesper is Apache-2.0 licensed, but FFmpeg remains under its own FFmpeg
license terms. The repository does not commit generated FFmpeg binaries by
default; optional Android, iOS, and desktop workflows can build or bundle
FFmpeg-backed artifacts when a host application explicitly opts in. Tagged
releases publish the Android FFmpeg runtime and FFmpeg-backed plugin Maven
coordinates, and publish the optional iOS FFmpeg-backed XCFrameworks, only with
the generated compliance bundle and exactly one corresponding FFmpeg source
archive for those binaries.

The default Vesper FFmpeg scripts avoid `--enable-gpl` and
`--enable-nonfree`; the scripts refuse those flags unless the caller passes an
explicit acknowledgement. The mobile `download-remux`, `relay-remux`, and
`default` profiles validate no-network/no-OpenSSL builds. Desktop fallback
builds are LGPL-oriented by default, but static desktop redistribution still
requires relinking materials or an equivalent LGPL-compliant mechanism.

Before publishing an app or SDK artifact that includes FFmpeg, include FFmpeg
notices and license text, provide the exact corresponding FFmpeg source and
configure flags, preserve user relinking rights, and track OpenSSL / libxml2
notices when those libraries are bundled. The release checklist and entry
template live in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## C ABI Notes

- `player-ffi` exposes generation-checked value handles in
  [include/player_ffi.h](include/player_ffi.h). The header is generated by
  cbindgen and should be synced with the Rust `vesper ffi` CLI instead of edited by hand.
  The C host smoke build also syncs it before compiling the example.
- Zero-initialized handles are invalid sentinels and may be used for plain C
  stack storage.
- Stale, consumed, or double-destroyed handles return
  `PLAYER_FFI_ERROR_CODE_INVALID_STATE` instead of relying on raw-pointer
  undefined behavior.
- Status-returning `player_ffi_*` calls are wrapped with `catch_unwind`, so
  panics surface as structured backend / platform errors instead of unwinding
  across the C boundary.
- The DASH/HLS bridge entry point `player_ffi_dash_bridge_execute_json` is
  provided by the `player-ffi-ios` Apple bundle, not by the generated C
  header.

```sh
./scripts/vesper ffi sync
./scripts/vesper ffi verify
```

## Release Downloads

GitHub Releases publish mobile downloads under the `VesperPlayerKit` product
name:

- Android core: `VesperPlayerKit-android-<abi>.aar`
- Android Compose adapter: `VesperPlayerKitCompose-android-<abi>.aar`
- Android Compose UI: `VesperPlayerKitComposeUi-android-<abi>.aar`
- Android Compose sample APK: `VesperPlayerAndroidComposeHost-android-<abi>-debug-signed.apk`
- Flutter Android sample APK: `VesperPlayerFlutterHost-android-<abi>-debug-signed.apk`
- iOS framework slices: `VesperPlayerKit-ios-*.framework.zip`
- iOS XCFramework: `VesperPlayerKit.xcframework.zip`
- iOS optional FFmpeg components: `VesperFFmpegAVCodec.xcframework.zip`,
  `VesperFFmpegAVFormat.xcframework.zip`, and
  `VesperFFmpegAVUtil.xcframework.zip`
- iOS optional plugins: `VesperPlayerRemuxFfmpegPlugin.xcframework.zip`,
  `VesperPlayerSourceNormalizerFfmpegPlugin.xcframework.zip`,
  `VesperPlayerDecoderVideoToolboxPlugin.xcframework.zip`, and
  `VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip`
- iOS FFmpeg redistribution materials:
  `VesperPlayerOptionalPlugins-FFmpeg-Compliance.zip` and
  `VesperPlayerOptionalPlugins-FFmpeg-<version>-source.tar.xz`
- `SHA256SUMS.txt` for release artifact verification

The canonical iOS release gate validates the archive layout and metadata, then
imports and links the public module from textual interfaces in isolated module
caches:

```sh
./scripts/vesper ios verify-release /path/to/ios-release --scope core
./scripts/vesper ios verify-release /path/to/ios-release --scope complete
```

Archive verification does not execute plugin code on a device. Release owners
run the separate physical-device gate and retain its provenance and XCResult:

```sh
./scripts/vesper ios verify-optional-plugins-device /path/to/ios-release \
  --device <UDID> \
  --development-team <TEAM_ID> \
  --output-directory /path/to/new-evidence-directory \
  --allow-provisioning-updates
```

See [`scripts/README.md`](scripts/README.md) for the retained evidence contract
and [`CURRENT-CHECKLIST.md`](CURRENT-CHECKLIST.md) for the current acceptance
result.

Android packaging is currently `arm64-v8a` only, including the downloadable
sample APKs. The sample APKs are debug-signed for side-load evaluation only and
are not production app-store artifacts. iOS binary packaging is arm64 only for
iPhoneOS devices and Apple Silicon Simulator. Tagged releases include the seven
optional iOS framework archives only as one verified set with the FFmpeg
compliance asset and exactly one corresponding-source asset. Verification
rejects extra top-level assets or XCFramework slices and compares the bundled
license and notice material with its source. Same-tag workflow reruns reconcile
the GitHub Release asset list so retired artifacts are removed. The iOS core
`VesperPlayerKit.xcframework` does not embed FFmpeg; FFmpeg-backed remux support
and SourceNormalizer support are staged through the canonical optional-plugin
command. The iOS App target embeds three FFmpeg component frameworks plus four
plugin frameworks as signed top-level siblings. Hosts select plugins with
`VesperPluginReference`; the iOS artifact resolver maps those identities to the
embedded executables. The FFmpeg component frameworks are sibling dynamic
dependencies, not plugin registry entries. All FFmpeg-backed siblings must come
from the same FFmpeg profile so `profile-hash.txt` values match.

Optional SourceNormalizer, decoder, and FrameProcessor artifacts are for
diagnostics and explicit opt-in workflows. Default mobile playback remains
platform system-player first. HDR and Dolby Vision sources stay on platform
system playback; the SDK-managed native-frame route is SDR-only today and is
not advertised as an HDR-ready path.

Desktop plugin support is asymmetric while native-frame work is experimental:
macOS owns the active VideoToolbox native-frame and FrameProcessor validation
path, Windows owns a D3D11 native-frame presenter / decoder roadmap without a
FrameProcessor chain yet, and Linux currently stays on the FFmpeg software path
without loader-backed plugin execution, native-frame presentation, or
FrameProcessor support. When unsupported Windows FrameProcessor or Linux plugin
paths are configured, startup diagnostics now report the unsupported platform
capability explicitly instead of implying that the feature is wired.

Release AARs / XCFrameworks are fully packaged binary artifacts. Host apps that
consume these downloads do not run the repository's local JNI or FFmpeg
generation tasks during their own Gradle / Xcode build.

## Current Status

Vesper is still evolving and has not yet shipped as a stable 1.0 public SDK.
Android and iOS host kits have releasable package paths for the deliberate
modern arm64 platform boundary, while the Flutter federated packages are still
source-distributed from this repository. Desktop Flutter packages are not
shipped in the current package set. Desktop and SDK-managed native-frame paths
remain experimental and do not block mobile release readiness. The plugin
contracts, package schemas, Rust CLI, and WASM host are implemented in the
current checkout, and the complete iOS archive verifier has passed. Public plugin
release readiness still requires crates.io publication, a successful signed
iOS physical-device plugin gate, real-device DRM/live/lifecycle coverage, and a
clean independent author, WASM, and iOS consumption validation pass.

## License

Vesper is licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).
FFmpeg-backed optional artifacts are governed by FFmpeg's own LGPL/GPL terms,
depending on the exact build configuration, and are tracked separately.

Additional attribution and bundled-binary notes live in:

- [NOTICE](NOTICE)
- [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)
