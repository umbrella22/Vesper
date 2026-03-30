# VesperPlayerKit for Android

`lib/android` now holds the Android-native VesperPlayerKit integration project for the Vesper Player SDK.

## What Lives Here

- `vesper-player-kit`
  - Android library module that packages:
    - Kotlin host facade
    - Compose surface wrapper
    - JNI-backed `ExoPlayer` bridge
    - Rust-generated `.so` files in `src/main/jniLibs`

This project is the future landing point for Android `AAR` generation.

The current Kotlin package namespace is:

- `io.github.ikaros.vesper.player.android`

GitHub Releases now publish VesperPlayerKit for Android downloads through:

- `.github/workflows/mobile-lib-release.yml`

Current Android download package names:

- `VesperPlayerKit-android-arm64-v8a.aar`
- `VesperPlayerKit-android-x86_64.aar`

Download guidance:

- use `arm64-v8a` for physical Android devices
- use `x86_64` for desktop emulators and simulator-style validation
- see [newDoc/RELEASE-DOWNLOAD-GUIDE.md](../../newDoc/RELEASE-DOWNLOAD-GUIDE.md) for the full package-selection guide

## Packaging Helper

You can build the Android library artifact through:

- `scripts/build-android-vesper-player-kit-aar.sh`
- `scripts/stage-android-vesper-player-kit-release.sh`

If no Gradle CLI is available on the machine, the script will tell you to open `lib/android` in
Android Studio and run `:vesper-player-kit:assembleRelease` there.

## How It Relates To The Example

`examples/android-compose-host` is now just a runnable host app that depends on this module.

That means:

- `lib/android/vesper-player-kit`
  - integration library / future distributable SDK layer
- `examples/android-compose-host`
  - sample app that demonstrates how to call the library

The current Android host surface is also expected to handle:

- local files
- remote progressive URLs
- HLS (`.m3u8`)
- DASH (`.mpd`)

The library intentionally does not ship built-in demo URLs or preset source choices. Those belong
in a consuming host app such as `examples/android-compose-host`.

## Key Entry Points

- `vesper-player-kit/build.gradle.kts`
  - Android library module config
- `vesper-player-kit/src/main/java/.../VesperPlayerController.kt`
  - host-facing controller API
- `vesper-player-kit/src/main/java/.../VesperPlayerSurface.kt`
  - reusable Compose video surface
- `vesper-player-kit/src/main/java/.../VesperPlayerSource.kt`
  - host-facing source DTO

## Minimal Compose Usage

```kotlin
import androidx.compose.runtime.Composable
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.ikaros.vesper.player.android.VesperPlayerSurface
import io.github.ikaros.vesper.player.android.rememberVesperPlayerController

@Composable
fun DemoPlayerScreen() {
    val controller = rememberVesperPlayerController()
    val uiState = controller.uiState.collectAsStateWithLifecycle().value

    VesperPlayerSurface(controller = controller)

    // Then bind your own controls to:
    // controller.play()
    // controller.pause()
    // controller.seekBy(...)
    // controller.selectSource(...)
    // uiState.playbackState / uiState.timeline / uiState.playbackRate
}
```

## JNI Build

Rust JNI artifacts are built through:

- `scripts/build-android-vesper-player-kit-jni.sh`

The script now writes `.so` outputs into:

- `lib/android/vesper-player-kit/src/main/jniLibs`

So both the standalone library project and the example app consume the same native artifacts.
