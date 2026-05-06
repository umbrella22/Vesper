# Vesper Player SDK

Language: [Simplified Chinese](README.zh-CN.md)

Vesper is a native-first, multi-platform player SDK for applications that need
real platform playback behavior without rebuilding every product feature from
scratch on each target. Android playback runs through Media3 ExoPlayer, iOS
playback runs through AVPlayer, desktop playback uses native Rust pipelines,
and Flutter apps consume the same capabilities through a federated plugin.

The shared Rust layer keeps cross-platform semantics aligned: runtime contracts,
timeline and live-DVR state, playback resilience, ABR policy, playlist
coordination, preload and download planning, DASH bridging, and the public C ABI.
Platform host kits stay responsible for the rendering surface, lifecycle, native
media stack integration, and platform-specific capability reporting.

## Start Here

Choose the integration path that matches your app. Read the first document for
the public API and packaging model, then use the example app as a runnable
reference.

| Target                   | Read first                                                                                                       | Run / inspect next                                                                 | Useful when                                                                         |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| Android Kotlin / Compose | [lib/android/README.md](lib/android/README.md)                                                                   | [examples/android-compose-host/README.md](examples/android-compose-host/README.md) | You are integrating the AAR modules directly in an Android app.                     |
| iOS Swift / SwiftUI      | [lib/ios/VesperPlayerKit/README.md](lib/ios/VesperPlayerKit/README.md)                                           | [examples/ios-swift-host/README.md](examples/ios-swift-host/README.md)             | You are consuming the Swift Package or XCFramework from a UIKit / SwiftUI app.      |
| Flutter                  | [lib/flutter/vesper_player/README.md](lib/flutter/vesper_player/README.md)                                       | [examples/flutter-host/README.md](examples/flutter-host/README.md)                 | You want one Dart API over Android and iOS today; macOS is a package stub.          |
| Flutter platform authors | [lib/flutter/vesper_player_platform_interface/README.md](lib/flutter/vesper_player_platform_interface/README.md) | [lib/flutter/vesper_player_ui/README.md](lib/flutter/vesper_player_ui/README.md)   | You are extending the federated plugin or adopting the optional Flutter UI package. |
| C / C++ via FFI          | [include/player_ffi.h](include/player_ffi.h)                                                                     | [examples/c-host/README.md](examples/c-host/README.md)                             | You need the generated C ABI from a native host or plugin runtime.                  |
| Desktop Rust             | [examples/basic-player](examples/basic-player)                                                                   | [Desktop FFmpeg](#desktop-ffmpeg)                                                  | You are trying the desktop demo or working with the Rust playback pipeline.         |

## What You Get

- Native playback per platform: Media3 on Android, AVPlayer on iOS, and Rust
  desktop backends.
- Shared playback semantics for timeline, live edge, live DVR, track catalog,
  ABR, resilience policy, preload policy, and download orchestration.
- Platform-native surfaces instead of frame-copy rendering paths for mobile
  playback.
- Optional remux / codec plugin architecture for advanced media workflows.
- Generated, generation-checked C value handles for hosts that integrate through
  the FFI boundary.
- Runnable host applications for Android, iOS, Flutter, desktop Rust, and C.

## Capability Matrix

This is a coarse overview of the feature surface. Each platform README explains
the exact behavior, fallback rules, and capability flags that host apps should
check before exposing advanced controls.

| Capability               | Android (Media3)             | iOS (AVPlayer)                                | Desktop Rust                              | Flutter mobile                        |
| ------------------------ | ---------------------------- | --------------------------------------------- | ----------------------------------------- | ------------------------------------- |
| Local file               | ✅                           | ✅                                            | ✅                                        | ✅ Android / iOS                      |
| Progressive HTTP/HTTPS   | ✅                           | ✅                                            | ✅                                        | ✅ Android / iOS                      |
| HLS (`.m3u8`)            | ✅                           | ✅                                            | ✅                                        | ✅ Android / iOS                      |
| DASH (`.mpd`)            | ✅ native                    | ✅ DASH-to-HLS bridge for VOD / live fMP4     | ⚠️ backend-dependent FFmpeg demuxer       | ✅ Android native / iOS bridge        |
| Live / DVR               | ✅                           | ✅                                            | ✅                                        | ✅ Android / iOS                      |
| Track selection          | ✅ video / audio / subtitles | ✅ audio / subtitles                          | ✅                                        | ✅ per-platform semantics             |
| ABR `constrained` policy | ✅                           | ✅ HLS + DASH bridge variant catalogs         | ✅                                        | ✅ per-platform semantics             |
| ABR `fixedTrack` policy  | ✅ exact                     | ✅ best-effort HLS/DASH pinning on iOS 15+    | ✅                                        | ✅ per-platform semantics             |
| Resilience policy        | ✅                           | ✅                                            | ✅                                        | ✅ Android / iOS                      |
| Preload budget           | ✅                           | ✅                                            | ✅                                        | ✅ Android / iOS                      |
| Download manager         | ✅                           | ✅                                            | ✅ planner / executor in the desktop demo | ✅ Android / iOS                      |
| Hardware decode probe    | `VesperDecoderBackend`       | `VesperCodecSupport`                          | macOS VideoToolbox v2 opt-in              | Reflected through mobile capabilities |

The Flutter macOS package exists as an experimental stub and does not yet ship a
real playback backend. Product UI should rely on runtime capability flags rather
than assuming every row above is available on every backend.

## Repository Layout

```text
crates/      Rust workspace: shared core, runtime, FFI, backends, render, platform glue
lib/         Distributable platform integration layers
  android/   Android AAR modules: core kit, Compose adapter, optional Compose UI
  ios/       VesperPlayerKit Swift Package / XCFramework project
  flutter/   Federated Flutter packages: main API, platform packages, optional UI
examples/    Runnable host apps for Android, iOS, Flutter, desktop Rust, and C
include/     Generated C header: player_ffi.h
scripts/     Build, packaging, verification, and release helper scripts
third_party/ Vendored dependencies and generated prebuilt media libraries
```

The public integration surface is concentrated under [lib/](lib/),
[examples/](examples/), and [include/](include/). The Rust crates under
[crates/](crates/) power the shared runtime and platform bridges.

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

### C ABI

Start with the generated header at [include/player_ffi.h](include/player_ffi.h),
then run the smoke example described in [examples/c-host/README.md](examples/c-host/README.md).

```sh
scripts/vesper ffi c-host-smoke
```

## Platform Packages

### Android

Android is distributed as AAR modules:

- `vesper-player-kit`: core controller, source model, JNI bridge, download
  manager, and native video surface selection.
- `vesper-player-kit-compose`: Compose adapter with `VesperPlayerSurface` and
  controller/state helpers.
- `vesper-player-kit-compose-ui`: optional opinionated Compose player stage.

Minimum target: Android API 26+, Kotlin 2.x, and an arm64 device or emulator for
the published mobile artifacts.

### iOS

iOS is distributed as `VesperPlayerKit`, available as a local Swift Package for
source integration and as an XCFramework for release packaging. Public APIs are
Swift-first and designed for UIKit / SwiftUI hosts.

Minimum target: iOS 14.0+, Xcode 16+, and arm64 device / Apple Silicon Simulator
builds for the published artifacts.

### Flutter

Flutter is a federated plugin family:

- `vesper_player`: public Dart API and `VesperPlayerView`.
- `vesper_player_platform_interface`: shared DTOs and platform contracts.
- `vesper_player_android`: Android implementation over the Android host kit.
- `vesper_player_ios`: iOS implementation over `VesperPlayerKit`.
- `vesper_player_macos`: experimental macOS package stub without a real
  playback backend yet.
- `vesper_player_ui`: optional Flutter controls and player stage widgets.

The Flutter packages currently ship from source in this repository and are not
published to pub.dev yet.

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

Android and Flutter Android builds use the Gradle wrappers checked into the
corresponding projects, so local builds use the same Gradle / Android Gradle
Plugin versions as the examples and scripts.

## Desktop FFmpeg

Desktop Rust builds that link FFmpeg resolve libraries in this order:

1. Use the repository-local desktop FFmpeg install under
   `third_party/ffmpeg/desktop` when it already exists.
2. Otherwise use the latest system FFmpeg exposed through `pkg-config` or
   Homebrew `ffmpeg`.
3. If neither exists, build and install the matching workspace FFmpeg
   major/minor release into `third_party/ffmpeg/desktop`.

The local source archive cache follows the existing repository convention:

- If `ffmpeg-<major>.<minor>.tar.xz` already exists at the repository root, it
  is reused.
- Otherwise the build helper downloads the matching archive from
  `https://ffmpeg.org/releases/`.

Useful overrides:

| Variable                               | Purpose                                                         |
| -------------------------------------- | --------------------------------------------------------------- |
| `VESPER_DESKTOP_FFMPEG_DIR`            | Override the repository-local desktop FFmpeg install directory. |
| `VESPER_DESKTOP_FFMPEG_VERSION`        | Override the auto-resolved FFmpeg major/minor version.          |
| `VESPER_DESKTOP_FFMPEG_SOURCE_ARCHIVE` | Point to a pre-downloaded FFmpeg source archive.                |
| `VESPER_DESKTOP_FFMPEG_SOURCE_URL`     | Override the source download URL.                               |
| `VESPER_REAL_PKG_CONFIG`               | Force the wrapper to use a specific `pkg-config` binary.        |

### FFmpeg License Compliance

Vesper is Apache-2.0 licensed, but FFmpeg remains under its own FFmpeg
license terms. The repository does not commit generated FFmpeg binaries by
default; optional Android, iOS, and desktop workflows can build or bundle
FFmpeg-backed artifacts when a host application explicitly opts in.

The default Vesper FFmpeg scripts avoid `--enable-gpl` and
`--enable-nonfree`. Android FFmpeg prebuilts currently use OpenSSL and pass
`--enable-version3`, so Android remux-plugin releases should be handled as
LGPLv3-or-later FFmpeg redistributions unless the release build is changed and
re-reviewed. Apple prebuilts and the desktop fallback are LGPL-oriented by
default, but static desktop redistribution still requires relinking materials
or an equivalent LGPL-compliant mechanism.

Before publishing an app or SDK artifact that includes FFmpeg, include FFmpeg
notices and license text, provide the exact corresponding FFmpeg source and
configure flags, preserve user relinking rights, and track OpenSSL / libxml2
notices when those libraries are bundled. The release checklist and entry
template live in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## C ABI Notes

- `player-ffi` exposes generation-checked value handles in
  [include/player_ffi.h](include/player_ffi.h). The header is generated by
  cbindgen and should be regenerated with the script below instead of edited by
  hand.
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
./scripts/vesper ffi generate
./scripts/vesper ffi verify
```

## Release Downloads

GitHub Releases publish mobile downloads under the `VesperPlayerKit` product
name:

- Android core: `VesperPlayerKit-android-<abi>.aar`
- Android Compose adapter: `VesperPlayerKitCompose-android-<abi>.aar`
- iOS framework slices: `VesperPlayerKit-ios-*.framework.zip`
- iOS XCFramework: `VesperPlayerKit.xcframework.zip`
- `SHA256SUMS.txt` for release artifact verification

Android packaging is currently `arm64-v8a` only. iOS packaging is arm64 only for
device, Apple Silicon Simulator, and optional Catalyst slices.

## Current Status

Vesper is still evolving and has not yet shipped as a stable 1.0 public SDK.
Android and iOS host kits have releasable package paths, while the Flutter
federated packages are still source-distributed from this repository. The macOS
Flutter package is currently a stub without a real playback backend, and the
macOS native VideoToolbox v2 decoder path remains opt-in experimental; FFmpeg
software fallback is the default desktop route.

## License

Vesper is licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).
FFmpeg-backed optional artifacts are governed by FFmpeg's own LGPL/GPL terms,
depending on the exact build configuration, and are tracked separately.

Additional attribution and bundled-binary notes live in:

- [NOTICE](NOTICE)
- [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)
