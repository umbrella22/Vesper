# Vesper Flutter Host Demo

A runnable Flutter sample app that integrates the Vesper Player SDK through
the federated [`vesper_player`](../../lib/flutter/vesper_player/) plugin.

Use this example as a reference for:

- Wiring `VesperPlayerController` and `VesperPlayerView` into a Flutter UI
- Routing playback through the Android and iOS host kits
- Source selection, quality / audio / subtitle / speed sheets
- Configuring `VesperPlaybackResiliencePolicy`
- Exercising Android external playback through Cast / DLNA and iOS AirPlay
- Keeping playback, diagnostics, and download workflows separated
- SourceNormalizer plugin diagnostics panel on Android and iOS. The example
  defaults to `preflightOnly` and lets you switch among `disabled`,
  `diagnosticsOnly`, `preflightOnly`, `preferNormalized`, and
  `requireNormalized` at runtime.
- FrameProcessor diagnostic plugin logging when the optional artifact is
  bundled. The example does not expose a mobile FrameProcessor toggle and does
  not route frames through the plugin.
- Dolby Browser Test Kit catalog with explicit `Play now` and `Add to queue`
  actions.
- Bounded in-app event log for host UI actions such as source selection,
  Dolby actions, plugin mode changes, external-route events, and HDR evidence
  capture results.

## Host Workspaces

The host is organized into three bottom-navigation workspaces:

- `Play` keeps the player stage, theme control, quick source actions, system
  playback / Picture in Picture, and compact queue focused on the first
  workflow.
- `Diagnostics` contains the session summary, bounded event log, Dolby
  catalog, plugin diagnostics, HDR evidence capture, and resilience controls.
- `Downloads` stays isolated for download regression testing.

Dolby presets default to ad-hoc `Play now`, which starts that preset without
changing the real playback queue. A preset enters continuous playback only when
the user selects `Add to queue`. The event log is an example-host operation
log; it does not read Logcat, native logs, or system diagnostic streams.

## Requirements

- Flutter 3.44.0+
- Android Studio with AGP 9.1 support, JDK 21, Android SDK 36, NDK
  `29.0.14206865`, and an arm64 device or emulator (for Android target)
- Xcode 16+ and an arm64 Simulator or device (for iOS target)
- Rust toolchain with the corresponding mobile targets installed

The Android and iOS example targets share the base application identifier
`io.github.umbrella22.vesper.example.flutterhost`. Native Flutter channels use
`io.github.umbrella22.vesper_player`; application code normally receives those
through plugin auto-registration.

## Run

```sh
cd examples/flutter-host
flutter pub get
flutter run
```

## Build

Android release APK:

```sh
cd examples/flutter-host
flutter build apk --release
```

iOS release (no codesign):

```sh
./scripts/vesper ios stage-optional-plugins-release \
  /tmp/vesper-flutter-ios-optional-plugins-release \
  --profile source-normalizer \
  ios-arm64 ios-simulator-arm64
./scripts/vesper ios ffi release
cd examples/flutter-host
flutter pub get
flutter build ios --release --no-codesign
```

> The Flutter iOS plugin uses Swift Package Manager. Enable it once per
> machine before building iOS targets:
>
> ```sh
> flutter config --enable-swift-package-manager
> ```

The Android Runner project builds and packages the optional remux,
SourceNormalizer, and FrameProcessor diagnostic plugin `.so` files into
generated `jniLibs`. This repository's iOS Runner directly embeds the seven
locally staged XCFrameworks so the example can verify the complete release set.
Hosted Flutter consumers instead add
`vesper_player_source_normalizer_ffmpeg` and/or
`vesper_player_remux_ffmpeg`, which resolve capability-level SwiftPM products.
Dart sends canonical `VesperPluginReference` values through MethodChannel. Each
native host kit resolves those identities to its packaged plugin artifacts;
FFmpeg component frameworks remain sibling dynamic dependencies.

`flutter pub get` initially generates its aggregate Swift package with Flutter's
default deployment target. `flutter build ios` raises it from the Runner's
durable `IPHONEOS_DEPLOYMENT_TARGET=17.0`. Before invoking `xcodebuild` directly,
run `flutter build ios --config-only --no-codesign` once after the final
`flutter pub get`; do not edit the generated ephemeral `Package.swift`.

## Optional Plugin Diagnostics

The Flutter example depends on `vesper_player_source_normalizer_ffmpeg` and
`vesper_player_remux_ffmpeg`. It uses
`VesperSourceNormalizerConfiguration.preferBundled()` /
`VesperSourceNormalizerConfiguration.requireBundled()` to emit the canonical
SourceNormalizer reference. An empty reference list selects no plugin; Android
and iOS host resolvers map explicit identities to bundled artifacts.
FrameProcessor remains explicit and uses its canonical plugin reference. FFmpeg
runtime libraries are provided by the Android runtime AAR or by the signed sibling
`VesperFFmpegAVCodec`, `VesperFFmpegAVFormat`, and `VesperFFmpegAVUtil`
frameworks on iOS; neither runtime form is a plugin registry entry.

SourceNormalizer diagnostics and preflight modes do not change playback. In
`preferNormalized` and `requireNormalized`, the Android and iOS host kits may
open a disk-backed normalized resource session and hand the resulting fMP4 or
short-window HLS resource to the platform player through Android loopback HTTP
or the iOS `vesper-normalized://` resource loader. `preferNormalized` falls
back to the original source on failure; `requireNormalized` reports a source
error. Standard HLS and DASH stay native-first unless normalization is
explicitly required or forced. The plugin diagnostics panel shows route,
profile, cache usage, fallback reason, and participation. FrameProcessor
remains debug diagnostics only in this example and is never marked as
participating in mobile playback.

## Debug-Only HDR / Dolby Vision Capture

The example host includes debug helpers for HDR / Dolby Vision evidence capture
in `lib/src/hdr_evidence_capture.dart`. They record one real playback run plus a
device capability baseline into the app-local `hdr-dv-evidence` output root.

The diagnostics panel exposes three capture presets:
`HDR10-HEVC-MAIN10-2160P60-PQ`, `HEVC-SDR-CONTROL`, and
`NETWORK-FAILURE-CONTROL`. HDR10 and HEVC SDR captures use the current active
source and ask for source metadata confirmation before recording. The network
control uses an unreachable loopback URL and must not produce HDR capability
evidence.

Android and iOS expose a debug-only `hdrEvidenceDevice` MethodChannel method for
this example host. It records display and decoder capability details; it is not
part of the Vesper public SDK. HDR / Dolby Vision samples should continue to
route to platform system playback, and the SDK-managed native-frame route
remains SDR-only.

## Test

```sh
cd examples/flutter-host
flutter analyze
flutter test
```

## CI

This example is exercised by [`.github/workflows/flutter-ci.yml`](../../.github/workflows/flutter-ci.yml):

- `flutter analyze`
- `flutter test`
- Android release APK build
- iOS release build
