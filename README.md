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
- Optional plugin architecture for advanced media workflows: post-download
  remux, native-frame decoder experiments, internal frame processor diagnostics,
  and desktop-first source normalization.
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
| Download manager         | ✅ VOD prepare + restore + export | ✅ VOD prepare + restore + export       | ✅ public `player-host-desktop::download` service | ✅ Android / iOS                      |
| Hardware decode probe    | `VesperDecoderBackend`       | `VesperCodecSupport`                          | macOS VideoToolbox native-frame opt-in    | Reflected through mobile capabilities |
| Plugin startup diagnostics | Internal runtime diagnostics | Internal runtime diagnostics                  | ✅ decoder / frame processor / source normalizer diagnostics | Exposed as create-result diagnostics where supported |

The Flutter macOS package exists as an experimental stub and does not yet ship a
real playback backend. Product UI should rely on runtime capability flags rather
than assuming every row above is available on every backend.

## Repository Layout

```text
crates/      Rust workspace: shared core, runtime, FFI, backends, render, platform glue
lib/         Distributable platform integration layers
  android/   Android AAR modules: core kit, external playback, FFmpeg runtime, Compose adapter, optional Compose UI
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

Desktop plugin experiments are opt-in. `basic-player` can load native-frame
decoder plugins, frame processor diagnostic plugins, and packet-stream source
normalizer plugins through environment-configured library paths. These paths are
intended for SDK development and diagnostics, not for Android / iOS public
host-kit APIs.

Recommended SourceNormalizer smoke command:

```sh
VESPER_SOURCE_NORMALIZER_PLUGIN_PATHS=target/debug/libplayer_source_normalizer_ffmpeg.dylib \
VESPER_SOURCE_NORMALIZER_MODE=prefer-normalized \
cargo run -p basic-player
```

FrameProcessor remains a diagnostics / debug route unless you explicitly choose
a stricter desktop processing mode:

```sh
VESPER_FRAME_PROCESSOR_PLUGIN_PATHS=target/debug/libplayer_frame_processor_diagnostic.dylib \
VESPER_FRAME_PROCESSOR_MODE=diagnostics \
cargo run -p basic-player
```

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
- `vesper-player-kit-external-playback`: optional Google Cast, DLNA / UPnP AV,
  and local relay integration.
- `vesper-player-kit-ffmpeg-runtime`: optional FFmpeg runtime package used by
  remux and relay workflows.
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
- `vesper_player_macos`: experimental macOS package stub without a real
  playback backend yet.
- `vesper_player_external_playback`: optional Android Cast / DLNA controller
  with local HTTP relay support.
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

Android helper scripts use project-local cached Gradle distributions for local
development and a CI-provisioned `gradle` executable in GitHub Actions. This
keeps local agent work offline-safe while letting CI install Gradle through
`gradle/actions/setup-gradle`.

iOS Rust build helpers resolve the workspace through the SDK root Cargo
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
profile, profile hash, source archive, license-sensitive flags, and exact
configure line for release review.

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
  cbindgen and should be synced with the script below instead of edited by hand.
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
- Android external playback: `VesperPlayerKitExternalPlayback-android-<abi>.aar`
- Android FFmpeg runtime: `VesperPlayerKitFfmpegRuntime-android-<abi>.aar`
- Optional Android SourceNormalizer FFmpeg plugin: `VesperPlayerKitSourceNormalizerFfmpeg-android-<abi>.aar`
- Optional Android FrameProcessor diagnostic plugin: `VesperPlayerKitFrameProcessorDiagnostic-android-<abi>.aar`
- Android Compose sample APK: `VesperPlayerAndroidComposeHost-android-<abi>-debug-signed.apk`
- Flutter Android sample APK: `VesperPlayerFlutterHost-android-<abi>-debug-signed.apk`
- iOS framework slices: `VesperPlayerKit-ios-*.framework.zip`
- iOS XCFramework: `VesperPlayerKit.xcframework.zip`
- Optional iOS FFmpeg runtime: `VesperPlayerFfmpegRuntime.xcframework.zip`
- Optional iOS FFmpeg remux plugin: `VesperPlayerRemuxFfmpegPlugin.xcframework.zip`
- Optional iOS SourceNormalizer FFmpeg plugin: `VesperPlayerSourceNormalizerFfmpegPlugin.xcframework.zip`
- Optional iOS FrameProcessor diagnostic plugin: `VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip`
- `SHA256SUMS.txt` for release artifact verification

Android packaging is currently `arm64-v8a` only, including the downloadable
sample APKs. The sample APKs are debug-signed for side-load evaluation only and
are not production app-store artifacts. iOS packaging is arm64 only for device,
Apple Silicon Simulator, and optional Catalyst slices. The iOS core
`VesperPlayerKit.xcframework` does not embed FFmpeg; FFmpeg-backed remux support
and SourceNormalizer support are shipped as separate optional runtime
and plugin artifacts that the host app signs and embeds. Plugin library path
configuration points only at plugin binaries; the shared FFmpeg runtime is a
package dependency, not a plugin path. All FFmpeg-backed optional plugins and
their shared runtime must come from the same FFmpeg profile so
`profile-hash.txt` values match.

The mobile SourceNormalizer artifact can run diagnostics/preflight and, in
`preferNormalized` or `requireNormalized`, expose disk-backed fMP4 or
short-window HLS output to Android ExoPlayer and iOS AVPlayer through local
resource layers. Packet-stream output remains reserved for the future native
frame pipeline. The mobile FrameProcessor artifact is diagnostics-only: it can
be packaged and probed for capabilities, but it does not open frame sessions,
process frames, or participate in default mobile playback. Mobile Decoder
artifacts and configuration remain deferred.

Release AARs / XCFrameworks are fully packaged binary artifacts. Host apps that
consume these downloads do not run the repository's local JNI or FFmpeg
generation tasks during their own Gradle / Xcode build.

## Current Status

Vesper is still evolving and has not yet shipped as a stable 1.0 public SDK.
Android and iOS host kits have releasable package paths, while the Flutter
federated packages are still source-distributed from this repository. The macOS
Flutter package is currently a stub without a real playback backend, and the
macOS native VideoToolbox native-frame decoder path remains opt-in experimental;
FFmpeg software fallback is the default desktop route.

## License

Vesper is licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).
FFmpeg-backed optional artifacts are governed by FFmpeg's own LGPL/GPL terms,
depending on the exact build configuration, and are tracked separately.

Additional attribution and bundled-binary notes live in:

- [NOTICE](NOTICE)
- [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)
