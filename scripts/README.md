# scripts Directory

`scripts/` is organized by platform and purpose. Use `scripts/vesper` for common local tasks. The categorized scripts remain available for CI, Gradle, Xcode, and advanced flows that need direct script arguments.

## Layout

```text
scripts/
  vesper      Unified task entrypoint
  lib/        Shared Bash functions and platform constants
  android/    Android FFmpeg, JNI, AAR, release staging, remux plugin
  apple/      Apple FFmpeg prebuilts
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

./scripts/vesper android ffmpeg arm64-v8a
./scripts/vesper android ffmpeg-runtime download-remux relay-remux
./scripts/vesper android jni release arm64-v8a
VESPER_ANDROID_FFMPEG_CONSUMERS="download-remux relay-remux" ./scripts/vesper android relay-ffmpeg-jni release
./scripts/vesper android aar
./scripts/vesper android stage-release

./scripts/vesper apple ffmpeg ios-arm64 ios-simulator-arm64
./scripts/vesper ios ffi release
./scripts/vesper ios verify-bridge-shim
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

## Mobile FFmpeg Profiles

Android and Apple FFmpeg prebuilt scripts support the same profile and overlay
model. `legacy` is the default and preserves the historical behavior.
`remux-local` is an opt-in trimmed preset for local stream-copy remuxing.
`custom` starts from `--disable-everything` and enables only the capabilities
provided by the caller.

```sh
./scripts/vesper android ffmpeg \
  --ffmpeg-profile remux-local \
  arm64-v8a

./scripts/vesper apple ffmpeg \
  --ffmpeg-profile custom \
  --enable-libraries avcodec,avformat,avutil \
  --enable-demuxers mov,dash,hls,concat,flv,mpegts \
  --enable-muxers mp4,mov \
  --enable-protocols file,pipe \
  --tls-backend none \
  ios-arm64 ios-simulator-arm64
```

Android FFmpeg runtime packaging is split from FFmpeg consumers. Build
`vesper-player-kit-ffmpeg-runtime` with the enabled consumer list first; the
resolver unions their requirements and writes the runtime profile metadata into
the AAR. `player-remux-ffmpeg` and `vesper-player-kit-relay-ffmpeg` must package
only their own plugin/JNI libraries and depend on that shared runtime.

```sh
./scripts/vesper android ffmpeg-runtime download-remux relay-remux
VESPER_ANDROID_FFMPEG_CONSUMERS="download-remux relay-remux" \
  ./scripts/vesper android remux-plugin /tmp/vesper-android-remux release
VESPER_ANDROID_FFMPEG_CONSUMERS="download-remux relay-remux" \
  ./scripts/vesper android relay-ffmpeg-jni release
```

Apple remux plugin build scripts still accept FFmpeg profile options directly:

```sh
./scripts/vesper ios remux-plugin /tmp/vesper-ios-remux release \
  --ffmpeg-profile remux-local \
  ios-arm64 ios-simulator-arm64
```

Supported overlays are:

- `--enable-libraries`
- `--enable-demuxers`
- `--enable-muxers`
- `--enable-protocols`
- `--enable-parsers`
- `--enable-bsfs`
- `--extra-configure-arg`
- `--tls-backend none|openssl` for Android
- `--tls-backend none|securetransport` for Apple

Lists may be comma or space separated. The scripts also accept matching
environment variables such as `VESPER_ANDROID_FFMPEG_PROFILE`,
`VESPER_APPLE_FFMPEG_ENABLE_DEMUXERS`, and
`VESPER_ANDROID_FFMPEG_EXTRA_CONFIGURE_ARGS`.

Non-legacy profile outputs are written under `third_party/ffmpeg/<platform>/profiles/`
by default, so custom builds do not overwrite legacy prebuilts. Every prebuilt
slice writes `vesper-ffmpeg-build-metadata.txt` with the profile, overlays,
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
