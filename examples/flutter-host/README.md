# Vesper Flutter Host Demo

A runnable Flutter sample app that integrates the Vesper Player SDK through
the federated [`vesper_player`](../../lib/flutter/vesper_player/) plugin.

Use this example as a reference for:

- Wiring `VesperPlayerController` and `VesperPlayerView` into a Flutter UI
- Routing playback through the Android and iOS host kits
- Source selection, quality / audio / subtitle / speed sheets
- Configuring `VesperPlaybackResiliencePolicy`

## Requirements

- Flutter 3.41.0+
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
