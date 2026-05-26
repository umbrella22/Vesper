# Vesper Android Host Demo

A runnable Jetpack Compose sample app that integrates the Vesper Player SDK
through the [`vesper-player-kit`](../../lib/android/) Android host kit.

Use this example as a reference for:

- Wiring `VesperPlayerController` and `VesperPlayerSurface` into a Compose UI
- Selecting local files via the Android document picker
- Playing HLS, DASH, or progressive HTTP streams
- Switching themes, sources, tracks, and ABR policies through bottom sheets

## Features Demonstrated

- System / Light / Dark theme modes
- Fullscreen stage
- Quality / audio / subtitle / playback-speed bottom sheets
- Double-tap seek, draggable scrubber
- Cast route selection and DLNA external playback
- Compose previews
- Built-in HLS demo source
- Built-in DASH demo source
- Generic remote URL field with `HLS / DASH / progressive` inference
- SourceNormalizer plugin diagnostics panel. The example defaults to
  `preflightOnly` and lets you switch among `disabled`, `diagnosticsOnly`,
  `preflightOnly`, `preferNormalized`, and `requireNormalized` at runtime.
- FrameProcessor diagnostic plugin logging. The example packages the diagnostic
  plugin when available, but does not open frame sessions or alter rendering.

## Optional Plugin Diagnostics

The Android example packages the SourceNormalizer FFmpeg plugin and the
FrameProcessor diagnostic plugin into generated `jniLibs` during Gradle builds.
The app passes only plugin binary paths to `VesperPlayerController`; FFmpeg
runtime libraries come from the shared `vesper-player-kit-ffmpeg-runtime` AAR
and are not included in `pluginLibraryPaths`.

SourceNormalizer diagnostics and preflight modes do not change playback. In
`preferNormalized` and `requireNormalized`, the host may open a disk-backed
normalized resource session and hand the resulting fMP4 or short-window HLS
resource to ExoPlayer through a loopback `127.0.0.1` resource server with Range
support. `preferNormalized` falls back to the original source on failure;
`requireNormalized` reports a source error. FrameProcessor remains
debug diagnostics only in this example and is never marked as participating in
mobile playback.

## Cast and DLNA

The player page includes an **External Playback** section:

- Use the Cast route button to select a Google Cast receiver. The example uses
  Google's Default Media Receiver unless the app manifest provides
  `io.github.ikaros.vesper.player.android.external.RECEIVER_APPLICATION_ID`.
- Use **Scan DLNA** to discover UPnP AV / DLNA renderers on the current LAN.
  Android 13+ prompts for `NEARBY_WIFI_DEVICES`; Cast does not require that
  permission.
- Connecting a route loads the active playlist item on the remote device,
  pauses local playback, and routes play / pause / seek controls to the remote
  session. Playback-rate controls stay local-only and are hidden while remote
  mode is active.

The external playback module may use a local HTTP relay for local files,
request-header sources, and DLNA DASH adaptation. The example enables cleartext
traffic at the app layer so LAN device descriptions and relay URLs can be read.
Remote progress is currently estimated by the example because the SDK does not
yet expose Cast/DLNA status polling.

The demo URLs are owned by the example app. The reusable library under
[`lib/android/vesper-player-kit`](../../lib/android/) does not embed demo URLs
and only accepts generic `VesperPlayerSource` values.

## Requirements

- Android Studio (Ladybug or newer)
- Android SDK 36 / minSdk 26
- NDK `29.0.14206865`
- Rust toolchain with `aarch64-linux-android` target
- arm64 device or arm64 emulator

## Run

1. Build the Android JNI libraries:

   ```sh
   ./scripts/vesper android jni
   # or for release: ./scripts/vesper android jni release
   ```

   Output is written to
   `lib/android/vesper-player-kit/src/main/jniLibs/<abi>/libvesper_player_android.so`.

   If the script fails, install missing tooling:

   ```sh
   rustup target add aarch64-linux-android
   ```

   Override the NDK with `ANDROID_NDK_ROOT=...` when needed.

2. Open `examples/android-compose-host` in Android Studio and sync Gradle.

3. Run the app on an arm64 emulator or physical device.

## Build From CLI

```sh
GRADLE_USER_HOME=$PWD/.gradle/gradle-user-home \
examples/android-compose-host/.gradle/wrapper/dists/gradle-9.4.0-bin/lcvyxq3t37f6mx9miaydrrgs/gradle-9.4.0/bin/gradle \
  -p examples/android-compose-host \
  -Pvesper.player.android.abis=arm64-v8a \
  assembleRelease
```

## Test

```sh
./scripts/vesper android jni release arm64-v8a
GRADLE_USER_HOME=$PWD/.gradle/gradle-user-home \
examples/android-compose-host/.gradle/wrapper/dists/gradle-9.4.0-bin/lcvyxq3t37f6mx9miaydrrgs/gradle-9.4.0/bin/gradle \
  -p examples/android-compose-host \
  -Pvesper.player.android.abis=arm64-v8a \
  :app:testDebugUnitTest
```

## Toolchain Pinning

The project is pinned to:

- Android Gradle Plugin `9.1.0`
- Gradle Wrapper `9.4.0`
- Kotlin `2.3.10`
- Compose BOM `2026.02.01`
- Android NDK `29.0.14206865`

With AGP 9.x, the `org.jetbrains.kotlin.android` plugin is built in and is not
applied separately.

Gradle storage is project-local and does not affect any shared global Gradle
cache:

- wrapper distributions: `examples/android-compose-host/.gradle/wrapper/dists`
- Gradle service home: `examples/android-compose-host/.gradle/local-gradle-user-home`

References:

- [AGP release notes](https://developer.android.com/build/releases/agp-9-1-0-release-notes)
- [Gradle release notes](https://docs.gradle.org/current/release-notes.html)
- [Kotlin releases](https://kotlinlang.org/docs/releases.html)
- [Compose setup](https://developer.android.com/develop/ui/compose/setup-compose-dependencies-and-compiler)
- [Compose BOM](https://developer.android.com/develop/ui/compose/bom)
- [Media3 / ExoPlayer](https://developer.android.com/media/media3/exoplayer/hello-world)

## Layout

- `app/src/main/java/.../MainActivity.kt` — Android entrypoint
- `app/src/main/java/.../PlayerHostApp.kt` — Compose host UI

Reusable host kit (separate project):

- [`lib/android/vesper-player-kit`](../../lib/android/) — `VesperPlayerController`, `VesperPlayerSource`, JNI bridge
- [`lib/android/vesper-player-kit-compose`](../../lib/android/) — Compose helpers, reusable surface host
- [`lib/android/vesper-player-kit-external-playback`](../../lib/android/) — Cast, DLNA, and local relay helpers
