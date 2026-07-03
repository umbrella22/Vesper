# Vesper Flutter Host Demo

A runnable Flutter sample app that integrates the Vesper Player SDK through
the federated [`vesper_player`](../../lib/flutter/vesper_player/) plugin.

Use this example as a reference for:

- Wiring `VesperPlayerController` and `VesperPlayerView` into a Flutter UI
- Routing playback through the Android and iOS host kits
- Source selection, quality / audio / subtitle / speed sheets
- Configuring `VesperPlaybackResiliencePolicy`
- Exercising Android external playback through Cast / DLNA and iOS AirPlay
- SourceNormalizer plugin diagnostics panel on Android and iOS. The example
  defaults to `preflightOnly` and lets you switch among `disabled`,
  `diagnosticsOnly`, `preflightOnly`, `preferNormalized`, and
  `requireNormalized` at runtime.
- FrameProcessor diagnostic plugin logging when the optional artifact is
  bundled. The example does not expose a mobile FrameProcessor toggle and does
  not route frames through the plugin.

## Requirements

- Flutter 3.44.0+
- Android Studio + arm64 device or emulator (for Android target)
- Xcode 16+ and an arm64 Simulator or device (for iOS target)
- Rust toolchain with the corresponding mobile targets installed

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
./scripts/vesper ios ffi release
cd examples/flutter-host
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
generated `jniLibs`. The iOS Runner project includes a build phase that embeds
the optional `VesperPlayerFfmpegRuntime.framework`,
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

The Flutter example depends on `vesper_player_source_normalizer_ffmpeg` and uses
`VesperSourceNormalizerConfiguration.preferBundled()` /
`VesperSourceNormalizerConfiguration.requireBundled()` so Android and iOS host
kits can discover bundled SourceNormalizer binaries themselves. FrameProcessor
remains explicit and still uses native host plugin paths. FFmpeg runtime
libraries are provided by the Android runtime AAR or by the iOS
`VesperPlayerFfmpegRuntime.framework`; neither runtime is passed as a plugin
path.

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
