# Vesper

Vesper is a native-first, multi-platform player SDK.

The current direction is:

- native platform playback first
- shared runtime and FFI contracts in Rust
- software/backend fallback where native playback is not enough
- example apps kept separate from distributable libraries

## Repository Layout

- `crates/`
  - shared Rust core, runtime, FFI, backend, render, and platform crates
- `lib/`
  - distributable platform integration layers
  - Android core: `lib/android/vesper-player-kit`
  - Android Compose adapter: `lib/android/vesper-player-kit-compose`
  - iOS: `lib/ios/VesperPlayerKit`
  - Flutter federated plugin packages: `lib/flutter/vesper_player*`
- `examples/`
  - runnable host apps that demonstrate how to consume the libraries

## Current Platform Direction

- Android
  - `VesperPlayerKit` Android core library
  - optional `VesperPlayerKitCompose` adapter for Jetpack Compose
  - native `Media3 ExoPlayer` host path
  - local file, progressive URL, HLS, and DASH inputs
- iOS
  - `VesperPlayerKit` Swift Package / framework
  - native `AVPlayer` host path
  - local file, progressive URL, and HLS inputs
  - DASH source/API shape exists in shared models, but the current AVPlayer backend still reports it as unsupported
  - Apple line does not currently promise DASH playback as a stable shipped capability
- Flutter
  - federated plugin baseline under `lib/flutter/`
  - `vesper_player`, `vesper_player_platform_interface`, `vesper_player_android`, and `vesper_player_ios` already have first real implementations
  - `vesper_player_macos` remains experimental / placeholder
- Desktop
  - Rust host runtime and example player
  - desktop support continues separately from the mobile-first product catch-up work
  - local file, progressive URL, and HLS are the current hardening path; DASH model/demo paths may exist, but desktop playback does not yet promise DASH as a stable capability

## Examples

- Vesper Android host demo: `examples/android-compose-host`
- Vesper iOS host demo: `examples/ios-swift-host`
- Vesper Flutter host demo: `examples/flutter-host`
- Vesper desktop demo: `examples/basic-player`

## Desktop FFmpeg

Desktop Rust builds that link FFmpeg now resolve libraries in this order:

1. use the repository-local desktop FFmpeg install under `third_party/ffmpeg/desktop` when it already exists
2. otherwise use the latest system FFmpeg exposed through `pkg-config` or Homebrew `ffmpeg`
3. if neither exists, build and install the matching workspace FFmpeg major/minor release into `third_party/ffmpeg/desktop`

The local source archive cache follows the existing repository convention:

- if `ffmpeg-<major>.<minor>.tar.xz` already exists at the repository root, it is reused
- otherwise the build helper downloads the matching archive from `https://ffmpeg.org/releases/`

Useful overrides:

- `VESPER_DESKTOP_FFMPEG_DIR`: change the repository-local desktop FFmpeg install directory
- `VESPER_DESKTOP_FFMPEG_VERSION`: override the auto-resolved FFmpeg major/minor version
- `VESPER_DESKTOP_FFMPEG_SOURCE_ARCHIVE`: point to a pre-downloaded FFmpeg source archive
- `VESPER_DESKTOP_FFMPEG_SOURCE_URL`: override the source download URL
- `VESPER_REAL_PKG_CONFIG`: force the wrapper to use a specific `pkg-config` binary

## Libraries

- VesperPlayerKit for Android core: `lib/android/vesper-player-kit`
- VesperPlayerKit Compose adapter: `lib/android/vesper-player-kit-compose`
- VesperPlayerKit for iOS: `lib/ios/VesperPlayerKit`
- Vesper Flutter main package: `lib/flutter/vesper_player`
- Vesper Flutter platform interface: `lib/flutter/vesper_player_platform_interface`
- Vesper Flutter Android / iOS packages: `lib/flutter/vesper_player_android`, `lib/flutter/vesper_player_ios`

## C ABI Notes

- `player-ffi` now exposes generation-checked value handles in `include/player_ffi.h`
- zero-initialized handles are invalid sentinels and may be used for plain C stack storage
- stale, consumed, or double-destroyed handles now return `PLAYER_FFI_ERROR_CODE_INVALID_STATE` instead of relying on raw-pointer undefined behavior

## Release Downloads

- GitHub Releases publish mobile downloads under the VesperPlayerKit product name
- Android core packages are shipped as `VesperPlayerKit-android-<abi>.aar`
- Android Compose adapter packages are shipped as `VesperPlayerKitCompose-android-<abi>.aar`
- iOS packages are shipped as `VesperPlayerKit-ios-*.framework.zip` and `VesperPlayerKit.xcframework.zip`
- each tagged release also includes `SHA256SUMS.txt` for package verification

## Status

Vesper is still evolving and has not been opened as a stable external SDK yet. The Android/iOS host kits
already have releasable package paths, while the Flutter federated plugin is still in an implementation-first
stage and has not been published as a stable external package family.

## License

Vesper is licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).

Additional attribution and future bundled-binary notes live in:

- [NOTICE](NOTICE)
- [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)
