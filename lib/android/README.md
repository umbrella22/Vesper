# VesperPlayerKit for Android

`lib/android` now holds the Android-native VesperPlayerKit integration project for the Vesper Player SDK.

## What Lives Here

- `vesper-player-kit`
  - Android library module that packages:
    - Kotlin host facade
    - JNI-backed `ExoPlayer` bridge
    - Rust-generated `libvesper_player_android.so` files in `src/main/jniLibs`
- `vesper-player-kit-compose`
  - optional Compose adapter that packages:
    - Compose controller helpers
    - Compose surface wrapper
    - lifecycle-scoped progress refresh

This project is the future landing point for Android `AAR` generation.

The current Kotlin package namespace is:

- `io.github.ikaros.vesper.player.android`
- `io.github.ikaros.vesper.player.android.compose`

The current Android native library basename is:

- `vesper_player_android`

Which means Android loads:

- `libvesper_player_android.so`

GitHub Releases now publish VesperPlayerKit for Android downloads through:

- `.github/workflows/mobile-lib-release.yml`

Current Android download package names:

- `VesperPlayerKit-android-arm64-v8a.aar`
- `VesperPlayerKit-android-x86_64.aar`
- `VesperPlayerKitCompose-android-arm64-v8a.aar`
- `VesperPlayerKitCompose-android-x86_64.aar`

Download guidance:

- use `arm64-v8a` for physical Android devices
- use `x86_64` for desktop emulators and simulator-style validation
- see [newDoc/RELEASE-DOWNLOAD-GUIDE.md](../../newDoc/RELEASE-DOWNLOAD-GUIDE.md) for the full package-selection guide

## Packaging Helper

You can build the Android library artifact through:

- `scripts/build-android-vesper-player-kit-aar.sh`
- `scripts/stage-android-vesper-player-kit-release.sh`

If no Gradle CLI is available on the machine, the script will tell you to open `lib/android` in
Android Studio and run both `:vesper-player-kit:assembleRelease` and
`:vesper-player-kit-compose:assembleRelease` there.

## How It Relates To The Example

`examples/android-compose-host` is now just a runnable host app that depends on this module.

That means:

- `lib/android/vesper-player-kit`
  - integration library / future distributable SDK layer
- `lib/android/vesper-player-kit-compose`
  - optional Compose integration layer
- `examples/android-compose-host`
  - sample app that demonstrates how to call the library

The current Android host surface is also expected to handle:

- local files
- remote progressive URLs
- HLS (`.m3u8`)
- DASH (`.mpd`)

The library intentionally does not ship built-in demo URLs or preset source choices. Those belong
in a consuming host app such as `examples/android-compose-host`.

The Android host API now exposes first-round playback resilience controls:

- `VesperPlaybackResiliencePolicy`
- `VesperBufferingPolicy`
- `VesperRetryPolicy`
- `VesperCachePolicy`

These are now used to shape `ExoPlayer` startup buffering, retry behavior, and first-round disk
caching for remote streams, especially `HLS / DASH`.

## Key Entry Points

- `vesper-player-kit/build.gradle.kts`
  - Android core library module config
- `vesper-player-kit-compose/build.gradle.kts`
  - Android Compose adapter module config
- `vesper-player-kit/src/main/java/.../VesperPlayerController.kt`
  - host-facing controller API
- `vesper-player-kit/src/main/java/.../VesperPlayerSource.kt`
  - host-facing source DTO
- `vesper-player-kit-compose/src/main/java/.../VesperPlayerCompose.kt`
  - Compose helpers and reusable video surface

## Minimal Compose Usage

```kotlin
import androidx.compose.runtime.Composable
import io.github.ikaros.vesper.player.android.VesperPlaybackResiliencePolicy
import io.github.ikaros.vesper.player.android.compose.VesperPlayerSurface
import io.github.ikaros.vesper.player.android.compose.rememberVesperPlayerController
import io.github.ikaros.vesper.player.android.compose.rememberVesperPlayerUiState

@Composable
fun DemoPlayerScreen() {
    val controller =
        rememberVesperPlayerController(
            resiliencePolicy = VesperPlaybackResiliencePolicy.resilient()
        )
    val uiState = rememberVesperPlayerUiState(controller)

    VesperPlayerSurface(controller = controller)

    // Then bind your own controls to:
    // controller.play()
    // controller.pause()
    // controller.seekBy(...)
    // controller.selectSource(...)
    // uiState.playbackState / uiState.timeline / uiState.playbackRate
}
```

## Why The Compose Adapter Is Optional

`vesper-player-kit` intentionally stays UI-framework-neutral:

- Compose is not forced onto every Android consumer
- non-Compose hosts can depend on the core library without pulling in Compose or Material3
- future View-based, Flutter, React Native, or custom host wrappers can reuse the same core API

The Compose module is where UI-scoped refresh behavior now lives, so playback position updates are
driven only while a Compose host screen is active instead of being baked into the bridge itself.

## JNI Build

Rust JNI artifacts are built through:

- `scripts/build-android-vesper-player-kit-jni.sh`

The script now writes `.so` outputs into:

- `lib/android/vesper-player-kit/src/main/jniLibs`

The generated file name is:

- `libvesper_player_android.so`

These JNI outputs are treated as generated artifacts:

- the repository keeps only `src/main/jniLibs/.gitkeep`
- ABI folders and `.so` files are rebuilt locally through the helper script or Gradle tasks
- generated JNI binaries are intentionally ignored by git

So both the standalone library project and the example app consume the same native artifacts.
