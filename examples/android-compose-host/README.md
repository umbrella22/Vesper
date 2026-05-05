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
- Compose previews
- Built-in HLS demo source
- Built-in DASH demo source
- Generic remote URL field with `HLS / DASH / progressive` inference

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
examples/android-compose-host/gradlew -p examples/android-compose-host \
  -Pvesper.player.android.abis=arm64-v8a \
  assembleRelease
```

## Test

```sh
./scripts/vesper android jni release arm64-v8a
examples/android-compose-host/gradlew -p examples/android-compose-host \
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
