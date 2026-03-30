# Vesper Android Host Demo

This example is the runnable Android host app for the Vesper Player SDK.

It intentionally lives under `examples/` so it can serve as:

- a host-integration reference
- a runnable preview app
- a UI demo for platform consumers

## Stack

- `Jetpack Compose`
- `Kotlin 2`
- Android host UI owns controls and progress UI
- video presentation stays in a native surface container hosted from Compose

## Current Status

This host app is now intentionally thin:

- the app shell is Compose-based
- the actual Kotlin/JNI integration layer now lives under `lib/android/vesper-player-kit`
- the example app consumes that library module as a normal dependency
- the current project now boots the Rust-native bridge by default

This keeps the sample app closer to real SDK consumption instead of acting like a hidden SDK layer.

The host-facing Android integration surface now lives in the library module:

- `VesperPlayerSource`
- `VesperPlayerController`
- `VesperPlayerSurface`

The example Compose screen consumes those wrappers instead of binding directly to the raw bridge
contract.

## Bridge Modes

The Android library is shaped around one bridge contract with two implementations:

- `FakePlayerBridge`
  - local interactive preview bridge
- `VesperNativePlayerBridge`
  - JNI-backed bridge that mirrors the Rust host session and drives `ExoPlayer`

The default example now boots with `VesperNativePlayerBridge`, but the Compose UI and surface host
no longer depend on that specific implementation.

The library exposes a host-facing controller layer:

- `VesperPlayerController`
  - source selection, playback commands, state flow, backend label
- `VesperPlayerSurface`
  - reusable Compose + `AndroidView` surface host shell
- `VesperPlayerSource`
  - stable source DTO used by the host UI

The native path now also has a concrete surface strategy:

- host UI stays in `Compose`
- video attaches through a `TextureView`
- the future Rust bridge plugs in through `VesperNativeJni.kt`
- Rust-side mirror DTOs now live in `crates/platform/mobile/player-platform-android`
  - `AndroidHostSnapshot`
  - `AndroidHostEvent`
- the JNI symbols are now implemented in `crates/platform/jni/player-jni-android`

The example app also supports selecting a local video through the Android document picker. The
selected `content://` URI is passed directly into the JNI/ExoPlayer bridge. For remote playback,
the example app itself now owns these preset/test inputs:

- built-in HLS demo
- built-in DASH demo
- a generic remote stream URL field that infers `HLS / DASH / progressive` from the URL

Those preset URLs intentionally live in `examples/android-compose-host`; the reusable library under
`lib/android/vesper-player-kit` only accepts generic `VesperPlayerSource` values and does not embed demo URLs.

## Project Split

- `lib/android/vesper-player-kit`
  - Android library module / future `AAR`
- `examples/android-compose-host`
  - runnable sample app that depends on `:vesper-player-kit`

## Android Studio Handoff

You can now choose between two entrypoints:

1. open `lib/android`
   - work on the reusable Android library / future `AAR`
2. open `examples/android-compose-host`
   - run the sample app that consumes the library

For a runnable app flow:

1. open `examples/android-compose-host`
2. sync the Gradle project
3. build the Android JNI libraries:
   - `scripts/build-android-vesper-player-kit-jni.sh`
   - or `scripts/build-android-vesper-player-kit-jni.sh release`
   - or run Gradle tasks on `:vesper-player-kit`
   - if this fails, first verify Rust targets are installed:
     - `rustup target add aarch64-linux-android x86_64-linux-android`
   - then verify Android Studio has fully installed `NDK (Side by side) 29.0.14206865`
   - if Studio installs a different NDK version, the script will automatically use the newest complete NDK under your Android SDK, or you can override it with `ANDROID_NDK_ROOT=...`
4. confirm `.so` files landed under `lib/android/vesper-player-kit/src/main/jniLibs`
5. run the app on an emulator/device
6. validate the `TextureView + ExoPlayer + Rust session` playback loop

For the reusable library artifact itself, use:

- `scripts/build-android-vesper-player-kit-aar.sh`

## Tooling Notes

The project is pinned to:

- Android Gradle Plugin `9.1.0`
- Gradle Wrapper `9.4.0`
- Kotlin `2.3.10`
- Compose BOM `2026.02.01`
- Android NDK `29.0.14206865`

With `AGP 9.x`, Kotlin Android support is built in, so this example does not apply the
`org.jetbrains.kotlin.android` plugin separately.

Project-local Gradle storage is also intentional:

- wrapper distributions are stored under `examples/android-compose-host/.gradle/wrapper/dists`
- Android Studio Gradle service home is pinned to
  `examples/android-compose-host/.gradle/local-gradle-user-home`

If you open `lib/android` directly, that project has its own local Gradle state as well.

This keeps the example from polluting a shared global Gradle cache setup.

These choices follow current official docs:

- AGP release notes: https://developer.android.com/build/releases/agp-9-1-0-release-notes
- Gradle release notes: https://docs.gradle.org/current/release-notes.html
- Kotlin releases: https://kotlinlang.org/docs/releases.html
- Compose compiler/setup: https://developer.android.com/develop/ui/compose/setup-compose-dependencies-and-compiler
- Compose BOM: https://developer.android.com/develop/ui/compose/bom
- Media3 / ExoPlayer: https://developer.android.com/media/media3/exoplayer/hello-world

## Layout

- `app/src/main/java/.../MainActivity.kt`
  - Android entrypoint
- `app/src/main/java/.../PlayerHostApp.kt`
  - Compose host UI
- `../../lib/android/vesper-player-kit/src/main/java/.../VesperPlayerController.kt`
  - host-facing controller wrapper
- `../../lib/android/vesper-player-kit/src/main/java/.../VesperPlayerSource.kt`
  - host-facing source DTO
- `../../lib/android/vesper-player-kit/src/main/java/.../VesperPlayerSurface.kt`
  - reusable Compose surface host
- `../../lib/android/vesper-player-kit/src/main/java/.../PlayerBridge.kt`
  - bridge-facing host contract
- `../../lib/android/vesper-player-kit/src/main/java/.../FakePlayerBridge.kt`
  - local interactive preview placeholder
- `../../lib/android/vesper-player-kit/src/main/java/.../VesperNativeJniBindings.kt`
  - ExoPlayer-backed JNI bridge implementation
- `scripts/build-android-vesper-player-kit-jni.sh`
  - helper for building `player-jni-android` into `lib/android/vesper-player-kit/src/main/jniLibs`
