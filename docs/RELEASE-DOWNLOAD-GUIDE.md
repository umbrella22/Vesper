# VesperPlayerKit Release Download Guide

This guide explains which VesperPlayerKit package to download from GitHub Releases and when to use each one.

## Scope

Current tagged releases publish mobile integration artifacts for:

- Android
- iOS

Desktop release packaging is still tracked separately and is not part of the current VesperPlayerKit download set.

## Package Matrix

Current release downloads are:

- `VesperPlayerKit-android-arm64-v8a.aar`
- `VesperPlayerKit-android-x86_64.aar`
- `VesperPlayerKit-ios-arm64.framework.zip`
- `VesperPlayerKit-ios-simulator-arm64.framework.zip`
- `VesperPlayerKit-ios-simulator-x86_64.framework.zip`
- `VesperPlayerKit.xcframework.zip`
- `SHA256SUMS.txt`

## Android Packages

### `VesperPlayerKit-android-arm64-v8a.aar`

Use this package when:

- you are integrating on a physical Android device
- your target ABI is `arm64-v8a`
- you want the Android library artifact with the Rust `.so` payload already bundled inside the `AAR`

### `VesperPlayerKit-android-x86_64.aar`

Use this package when:

- you are validating integration on an `x86_64` Android emulator
- your local Android environment is specifically targeting `x86_64`

If you are packaging for a real shipped Android app, the device-focused `arm64-v8a` package is the normal default.

## iOS Packages

### `VesperPlayerKit-ios-arm64.framework.zip`

Use this package when:

- you only need the device framework slice
- you are manually embedding a single-device framework into an Apple build flow

### `VesperPlayerKit-ios-simulator-arm64.framework.zip`

Use this package when:

- you are running iOS Simulator on Apple Silicon
- you want a single simulator-only framework slice

### `VesperPlayerKit-ios-simulator-x86_64.framework.zip`

Use this package when:

- you are running iOS Simulator on an Intel Mac
- you want a single simulator-only framework slice

### `VesperPlayerKit.xcframework.zip`

Use this package when:

- you want one Apple package that covers both device and simulator targets
- you are distributing or consuming VesperPlayerKit as a reusable Apple binary package
- you do not specifically need to manage per-slice frameworks yourself

For most external iOS integrations, this is the safest default download.

## Checksum Verification

Each tagged release also publishes:

- `SHA256SUMS.txt`

Download that file alongside your selected package if you want integrity verification before integrating the binary.

Typical local verification flow:

1. Download the package you want.
2. Download `SHA256SUMS.txt` from the same release.
3. Run `shasum -a 256 -c SHA256SUMS.txt` from the folder containing those files.

## What Is Inside These Packages

### Android

The Android `AAR` packages are intended to be the distributable VesperPlayerKit integration layer and should already contain:

- the Android library module output
- the Kotlin-facing integration surface
- the Rust JNI libraries bundled under the packaged Android artifact

### iOS

The iOS framework downloads are intended to be binary integration artifacts and should contain:

- the VesperPlayerKit framework binary
- the selected architecture slice or slices, depending on which archive you downloaded

## What Is Not Included

These GitHub Release packages are not the same thing as:

- the example host apps under `examples/`
- the raw Rust workspace source tree
- desktop distribution artifacts

They are the reusable mobile-facing VesperPlayerKit download set.

## Related Docs

- [README.md](../README.md)
- [NOTICE](../NOTICE)
- [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md)
- [lib/android/README.md](../lib/android/README.md)
- [lib/ios/VesperPlayerKit/README.md](../lib/ios/VesperPlayerKit/README.md)
