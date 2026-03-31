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
  - Android: `lib/android/vesper-player-kit`
  - iOS: `lib/ios/VesperPlayerKit`
- `examples/`
  - runnable host apps that demonstrate how to consume the libraries
- `docs/`
  - architecture notes, strategy docs, and support matrices

## Current Platform Direction

- Android
  - `VesperPlayerKit` Android library
  - native `Media3 ExoPlayer` host path
  - local file, progressive URL, HLS, and DASH inputs
- iOS
  - `VesperPlayerKit` Swift Package / framework
  - native `AVPlayer` host path
  - local file, progressive URL, and HLS inputs
- Desktop
  - Rust host runtime and example player
  - desktop support continues separately from the mobile-first product catch-up work

## Examples

- Vesper Android host demo: `examples/android-compose-host`
- Vesper iOS host demo: `examples/ios-swift-host`
- Vesper desktop demo: `examples/basic-player`

## Libraries

- VesperPlayerKit for Android: `lib/android/vesper-player-kit`
- VesperPlayerKit for iOS: `lib/ios/VesperPlayerKit`

## Release Downloads

- GitHub Releases publish mobile downloads under the VesperPlayerKit product name
- Android packages are shipped as `VesperPlayerKit-android-<abi>.aar`
- iOS packages are shipped as `VesperPlayerKit-ios-*.framework.zip` and `VesperPlayerKit.xcframework.zip`
- each tagged release also includes `SHA256SUMS.txt` for package verification
- see [docs/RELEASE-DOWNLOAD-GUIDE.md](docs/RELEASE-DOWNLOAD-GUIDE.md) for package selection guidance

## Status

Vesper is still evolving and has not been opened as a stable external SDK yet. That gives us room
to keep refining naming, API shape, runtime contracts, and release packaging before wider adoption.

## License

Vesper is licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).

Additional attribution and future bundled-binary notes live in:

- [NOTICE](NOTICE)
- [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)
