# scripts Directory

`scripts/` is organized by platform and purpose. Use `scripts/vesper` for common local tasks. The categorized scripts remain available for CI, Gradle, Xcode, and advanced flows that need direct script arguments.

## Layout

```text
scripts/
  vesper      Unified task entrypoint
  lib/        Shared Bash functions and platform constants
  android/    Android FFmpeg、JNI、AAR、release staging、remux plugin
  apple/      Apple FFmpeg prebuilts
  ios/        iOS FFI、XCFramework、remux plugin、embed phase、release staging
  desktop/    desktop FFmpeg、pkg-config wrapper、desktop plugin verification
  ffi/        C header generation / verification and C host smoke tests
  mobile/     mobile host kit packaging verification
  release/    GitHub Release notes generation
```

## Common Commands

```sh
./scripts/vesper ffi generate
./scripts/vesper ffi verify
./scripts/vesper ffi c-host-smoke

./scripts/vesper android ffmpeg arm64-v8a
./scripts/vesper android jni release arm64-v8a
./scripts/vesper android aar
./scripts/vesper android stage-release

./scripts/vesper apple ffmpeg ios-arm64 ios-simulator-arm64
./scripts/vesper ios ffi release
./scripts/vesper ios remux-plugin /tmp/vesper-ios-player-remux-ffmpeg release ios-arm64 ios-simulator-arm64
./scripts/vesper ios kit-xcframework
./scripts/vesper ios stage-release

./scripts/vesper desktop ensure-ffmpeg
./scripts/vesper desktop verify-decoder-diagnostics
./scripts/vesper desktop verify-decoder-videotoolbox loader
./scripts/vesper desktop verify-remux

./scripts/vesper mobile verify-no-remux android
./scripts/vesper mobile verify-no-remux ios
./scripts/vesper release notes <tag> [output-path]
```

## Conventions

- The default Android ABI is `arm64-v8a`; override it with command arguments or `RUST_ANDROID_ABIS`.
- The default Android NDK version is `29.0.14206865`. Scripts prefer `ANDROID_NDK_ROOT`, then resolve from `ANDROID_SDK_ROOT` / `ANDROID_HOME`.
- The default Apple/iOS slices are `ios-arm64` and `ios-simulator-arm64`; do not reintroduce x86 / x86_64 distribution slices.
- FFmpeg, OpenSSL, and libxml2 version, source URL, source archive, and output directory overrides continue to use the existing `VESPER_*` environment variable semantics.
- `scripts/lib/` contains only shared functions and default constants. Sourcing these files must not start build work.
