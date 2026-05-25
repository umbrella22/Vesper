# scripts Directory

`scripts/` is organized by platform and purpose. Use `scripts/vesper` for common local tasks. The categorized scripts remain available for CI, Gradle, Xcode, and advanced flows that need direct script arguments.

## Layout

```text
scripts/
  vesper      Unified task entrypoint
  lib/        Shared Bash functions and platform constants
  android/    Android private FFmpeg implementation details, JNI, AAR, release staging, optional plugins
  apple/      Apple private FFmpeg prebuilt implementation details
  ios/        iOS FFI, XCFramework, FFmpeg runtime, optional plugins, embed phase, release staging
  desktop/    desktop FFmpeg, pkg-config wrapper, desktop plugin verification
  ffi/        C header generation / verification and C host smoke tests
  flutter/    Flutter pub staging, dry-run, and automated publish helpers
  mobile/     mobile host kit packaging verification
  release/    Version metadata checks and GitHub Release notes generation
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
./scripts/vesper android source-normalizer-plugin /tmp/vesper-android-source-normalizer release --profile default
./scripts/vesper android frame-processor-plugin /tmp/vesper-android-frame-processor release
./scripts/vesper android stage-release
./scripts/vesper android sample-apks /tmp/vesper-android-samples arm64-v8a

./scripts/vesper ios ffi release
./scripts/vesper ios verify-bridge-shim
./scripts/vesper ios ffmpeg-runtime-release /tmp/vesper-ios-release --profile default ios-arm64 ios-simulator-arm64
./scripts/vesper ios stage-remux-plugin-release /tmp/vesper-ios-release --profile default ios-arm64 ios-simulator-arm64
./scripts/vesper ios stage-source-normalizer-plugin-release /tmp/vesper-ios-release --profile default ios-arm64 ios-simulator-arm64
./scripts/vesper ios stage-frame-processor-plugin-release /tmp/vesper-ios-release ios-arm64 ios-simulator-arm64
./scripts/vesper ios kit-xcframework
./scripts/vesper ios stage-release

./scripts/vesper desktop ensure-ffmpeg
./scripts/vesper desktop verify-decoder-diagnostics
./scripts/vesper desktop verify-decoder-videotoolbox loader
./scripts/vesper desktop verify-remux

./scripts/vesper mobile verify-no-remux android
./scripts/vesper mobile verify-no-remux ios
./scripts/vesper flutter stage-pub /tmp/vesper-flutter-pub
./scripts/vesper flutter pub-dry-run /tmp/vesper-flutter-pub
./scripts/vesper release prepare-from-tag vMAJOR.MINOR.PATCH
./scripts/vesper release verify-current
./scripts/vesper release notes <tag> [output-path]
```

## Gradle Resolution

Android helper scripts use a CI-provisioned `gradle` executable when `CI=true`.
GitHub Actions jobs install that executable with `gradle/actions/setup-gradle`.
Local agent work remains offline-safe: scripts look for a project-local cached
Gradle distribution under `.gradle/wrapper/dists/**/bin/gradle` and do not
invoke `gradlew` when that cache is missing.

## Flutter Pub Publishing

Repository `pubspec.yaml` files keep path dependencies for local development.
The pub helpers stage temporary packages, remove `publish_to: none`, copy the
root license, and rewrite internal package dependencies to hosted constraints
for the selected version. If no version argument is passed, the staging helper
uses the current `vesper_player` package version.

Release workflows do not hardcode product versions. They derive the numeric
product version from the pushed tag, apply it to the CI workspace with
`./scripts/vesper release prepare-from-tag "$RELEASE_TAG"`, and verify the
updated metadata before packaging. Stable tags such as `vMAJOR.MINOR.PATCH`
publish staged Flutter packages to pub.dev; RC tags publish GitHub release
artifacts but do not publish to pub.dev.

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
only when a private flow needs raw prebuilts. `player-remux-ffmpeg`,
`player-source-normalizer-ffmpeg`, and the external-playback relay FFmpeg JNI
library must package only their own plugin/JNI libraries and depend on the
shared runtime AAR. The FrameProcessor diagnostic plugin is not FFmpeg-backed.

```sh
./scripts/vesper ffmpeg --platform android --profile default --abi arm64-v8a
./scripts/vesper android remux-plugin /tmp/vesper-android-remux release --profile download-remux
./scripts/vesper android source-normalizer-plugin /tmp/vesper-android-source-normalizer release --profile default
```

The external-playback relay FFmpeg JNI library is built by the Android
`vesper-player-kit-external-playback` Gradle module through its private
`buildRelayFfmpegAndroidJni` task. Release and example builds use the `default`
profile so the shared runtime and relay JNI profile hashes match.

iOS core kit packaging does not include FFmpeg. Optional remux support is staged
as two signable XCFrameworks: one shared FFmpeg runtime and one remux plugin:

```sh
./scripts/vesper ios ffmpeg-runtime-release /tmp/vesper-ios-release \
  --profile default \
  ios-arm64 ios-simulator-arm64

./scripts/vesper ios stage-remux-plugin-release /tmp/vesper-ios-release \
  --profile default \
  ios-arm64 ios-simulator-arm64
```

The remux plugin release depends on the shared iOS FFmpeg runtime release and
will stage it automatically when needed. The runtime and plugin artifacts both
write `profile-hash.txt`; staging fails if the hashes do not match. The plugin
XCFramework must not contain `libav*`, `libsw*`, `libxml2*`, `libssl*`, or
`libcrypto*` dylibs.

The SourceNormalizer FFmpeg plugin follows the same shared-runtime boundary:

```sh
./scripts/vesper ios stage-source-normalizer-plugin-release /tmp/vesper-ios-release \
  --profile default \
  ios-arm64 ios-simulator-arm64
```

It writes profile metadata, verifies the runtime/plugin profile hashes, and
must not contain FFmpeg dylibs. The mobile v1 behavior is diagnostics/preflight
only; it does not replace Android or iOS playback sources. The FrameProcessor
diagnostic release command stages a non-FFmpeg plugin shell:

```sh
./scripts/vesper ios stage-frame-processor-plugin-release /tmp/vesper-ios-release \
  ios-arm64 ios-simulator-arm64
```

That artifact exists for packaging and capability diagnostics only. It does not
process frames or participate in default mobile playback. Decoder mobile
artifacts remain deferred.

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
