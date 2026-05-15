# scripts Directory

`scripts/` is organized by platform and purpose. Use `scripts/vesper` for common local tasks. The categorized scripts remain available for CI, Gradle, Xcode, and advanced flows that need direct script arguments.

## Layout

```text
scripts/
  vesper      Unified task entrypoint
  lib/        Shared Bash functions and platform constants
  android/    Android private FFmpeg implementation details, JNI, AAR, release staging, remux plugin
  apple/      Apple private FFmpeg prebuilt implementation details
  ios/        iOS FFI, XCFramework, remux plugin, embed phase, release staging
  desktop/    desktop FFmpeg, pkg-config wrapper, desktop plugin verification
  ffi/        C header generation / verification and C host smoke tests
  mobile/     mobile host kit packaging verification
  release/    GitHub Release notes generation
```

## Common Commands

```sh
./scripts/vesper ffi generate
./scripts/vesper ffi sync
./scripts/vesper ffi verify
./scripts/vesper ffi c-host-smoke

./scripts/vesper ffmpeg --list-profiles
./scripts/vesper ffmpeg --platform android --profile default --abi arm64-v8a
./scripts/vesper ffmpeg --platform ios --profile default --slice ios-arm64 --slice ios-simulator-arm64
./scripts/vesper android jni release arm64-v8a
./scripts/vesper android aar
./scripts/vesper android stage-release

./scripts/vesper ios ffi release
./scripts/vesper ios verify-bridge-shim
./scripts/vesper ios stage-remux-plugin-release /tmp/vesper-ios-release --profile default ios-arm64 ios-simulator-arm64
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

## Mobile FFmpeg Profiles

The public mobile FFmpeg entrypoint is the root command:
`./scripts/vesper ffmpeg --platform android|ios|all --profile <name>`.
Profiles are declared in `scripts/ffmpeg-profiles.toml`. The resolver supports
profile inheritance, platform overrides, validation policy, and command-line
overlays. `download-remux`, `relay-remux`, and `default` keep local remux
semantics by validating `--disable-network` and `--disable-openssl`.

```sh
./scripts/vesper ffmpeg \
  --platform android \
  --profile default \
  --abi arm64-v8a

./scripts/vesper ffmpeg \
  --platform ios \
  --profile download-remux \
  --slice ios-arm64 \
  --slice ios-simulator-arm64
```

Android FFmpeg runtime packaging is split from consumers. The root command builds
`vesper-player-kit-ffmpeg-runtime` by default; pass `--android-artifact prebuilts`
only when a private flow needs raw prebuilts. `player-remux-ffmpeg` and the
external-playback relay FFmpeg JNI library must package only their own plugin/JNI
libraries and depend on the shared runtime AAR.

```sh
./scripts/vesper ffmpeg --platform android --profile default --abi arm64-v8a
./scripts/vesper android remux-plugin /tmp/vesper-android-remux release --profile download-remux
```

The external-playback relay FFmpeg JNI library is built by the Android
`vesper-player-kit-external-playback` Gradle module through its private
`buildRelayFfmpegAndroidJni` task. Release and example builds use the `default`
profile so the shared runtime and relay JNI profile hashes match.

iOS core kit packaging does not include FFmpeg. Optional remux support is staged
as a signable XCFramework:

```sh
./scripts/vesper ios stage-remux-plugin-release /tmp/vesper-ios-release \
  --profile default \
  ios-arm64 ios-simulator-arm64
```

Supported overlays are:

- `--extra-libraries`
- `--extra-demuxers`
- `--extra-muxers`
- `--extra-protocols`
- `--extra-parsers`
- `--extra-bsfs`
- `--extra-configure-arg`
- `--tls-backend none|openssl` for Android
- `--tls-backend none|securetransport` for Apple

Lists may be comma or space separated. CI and documentation should use the root
`ffmpeg` command for runtime prebuilts; private Gradle/Xcode build phases may
consume the resolved artifacts produced by that command.

Resolved profile outputs are written under
`third_party/ffmpeg/<platform>/profiles/` by default. Every prebuilt slice writes
`vesper-ffmpeg-build-metadata.txt` with the declared profile, profile hash,
external dependencies, license-sensitive flags, source archive, and full
configure line.

## Conventions

- The default Android ABI is `arm64-v8a`; override it with command arguments or `RUST_ANDROID_ABIS`.
- The default Android NDK version is `29.0.14206865`. Scripts prefer `ANDROID_NDK_ROOT`, then resolve from `ANDROID_SDK_ROOT` / `ANDROID_HOME`.
- The default Apple/iOS slices are `ios-arm64` and `ios-simulator-arm64`; do not reintroduce x86 / x86_64 distribution slices.
- iOS Rust build scripts pass `--manifest-path "$ROOT_DIR/Cargo.toml"` to
  Cargo so they can be run from Xcode build phases, Flutter plugin builds, CI
  workspaces, or temporary directories.
- FFmpeg, OpenSSL, and libxml2 version, source URL, source archive, and output
  directory overrides continue to use the existing `VESPER_*` environment
  variable semantics.
- `scripts/lib/` contains only shared functions and default constants. Sourcing these files must not start build work.
