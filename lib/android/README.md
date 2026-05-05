# VesperPlayerKit for Android

Android-native host kit for the Vesper Player SDK. Distributed as Android `AAR`
artifacts and consumable from any Android app or library.

## Modules

| Module                         | Purpose                                                                                                                                                                             |
| ------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `vesper-player-kit`            | Core Android library: `VesperPlayerController`, `VesperPlayerSource`, `VesperTrackSelection`, `VesperDownloadManager`, JNI-backed `ExoPlayer` bridge, `libvesper_player_android.so` |
| `vesper-player-kit-cast`       | Optional Google Cast sender integration, media route button support, and default Cast options provider                                                                               |
| `vesper-player-kit-compose`    | Optional Jetpack Compose adapter: `VesperPlayerSurface`, `rememberVesperPlayerController`, `rememberVesperPlayerUiState`, lifecycle-scoped progress refresh                         |
| `vesper-player-kit-compose-ui` | Optional opinionated Compose UI: `VesperPlayerStage` and stage helpers built on top of the Compose adapter                                                                          |

The Cast module, Compose adapter, and higher-level Compose UI are optional.
View-based or non-Compose hosts can depend on `vesper-player-kit` alone without
pulling in Google Play Services, Cast Framework, Compose, or Material3.

Kotlin namespaces:

- `io.github.ikaros.vesper.player.android`
- `io.github.ikaros.vesper.player.android.cast`
- `io.github.ikaros.vesper.player.android.compose`
- `io.github.ikaros.vesper.player.android.compose.ui`

Native library: `libvesper_player_android.so`.

## Distribution

GitHub Releases publish the following artifacts via
`.github/workflows/mobile-lib-release.yml`:

- `VesperPlayerKit-android-arm64-v8a.aar`
- `VesperPlayerKitCompose-android-arm64-v8a.aar`

Android packaging is `arm64-v8a` only. Use an arm64 device or arm64 Android
emulator. See [Release Downloads](../../README.md#release-downloads) for the
public package names and artifact-selection notes.

The optional `vesper-player-kit-compose-ui` module is built from source in this
project. It does not currently have a separate release download artifact.

## Minimum Requirements

- Android API Level 26+
- Kotlin 2.x
- arm64 device or arm64 emulator

## Building From Source

From the repository root:

```sh
./scripts/vesper android aar
./scripts/vesper android stage-release
```

Without a Gradle CLI, open `lib/android` in Android Studio and run:

- `:vesper-player-kit:assembleRelease`
- `:vesper-player-kit-compose:assembleRelease`
- `:vesper-player-kit-compose-ui:assembleRelease`

## Public API

Core (`vesper-player-kit`):

- `VesperPlayerController` — playback control surface (`play / pause / seek / selectSource / setPlaybackRate / setAbrPolicy / setResiliencePolicy / set*TrackSelection`)
- `VesperPlayerControllerFactory` — `createDefault(...)` for production bridge, `createPreview(...)` for a Fake bridge
- `VesperPlayerSource` — media source DTO with `local / remote / hls / dash` factories
- `VesperTrackSelection` — audio / subtitle / video track selection (`auto`, `disabled`, `track(id)`)
- Reactive state on the controller: `uiState`, `trackCatalog`, `trackSelection`, `effectiveVideoTrackId`, `videoVariantObservation`, `resiliencePolicy` (all `StateFlow<...>`)
- `VesperAbrPolicy` — `auto`, `constrained`, `fixedTrack`
- `VesperPlaybackResiliencePolicy` with presets: `balanced()`, `streaming()`, `resilient()`, `lowLatency()`
- `VesperBufferingPolicy`, `VesperRetryPolicy`, `VesperCachePolicy`
- `VesperPreloadBudgetPolicy` — caps for concurrent preload tasks, memory, disk, warm-up window
- `VesperTrackPreferencePolicy` — preferred audio / subtitle languages
- `VesperDecoderBackend` — `SystemOnly` / `SystemPreferred` / `ExtensionPreferred`
- `NativeVideoSurfaceKind` — `SurfaceView` (default, HDR / high frame rate) or `TextureView` (scrolling / animated stages)
- `VesperDownloadManager` — download orchestration with `createTask / startTask / pauseTask / resumeTask / removeTask / exportTaskOutput`

Cast (`vesper-player-kit-cast`):

- `VesperCastController` — load, play, pause, stop, and seek the active Cast session
- `VesperCastOptionsProvider` — default Cast options provider using Google's Default Media Receiver unless the host overrides the receiver application ID in manifest meta-data

Compose adapter (`vesper-player-kit-compose`):

- `VesperPlayerSurface`
- `rememberVesperPlayerController`
- `rememberVesperPlayerUiState`

Compose UI (`vesper-player-kit-compose-ui`):

- `VesperPlayerStage` — opinionated player stage with controls overlay, gestures, fullscreen, sheets

The library does not ship preset URLs or demo sources. Construct
`VesperPlayerSource` from your own content.

## Supported Sources

- Local files
- Progressive HTTP/HTTPS
- HLS (`.m3u8`)
- DASH (`.mpd`)

## Minimal Compose Usage

```kotlin
import androidx.compose.runtime.Composable
import io.github.ikaros.vesper.player.android.VesperPlaybackResiliencePolicy
import io.github.ikaros.vesper.player.android.VesperDecoderBackend
import io.github.ikaros.vesper.player.android.compose.VesperPlayerSurface
import io.github.ikaros.vesper.player.android.compose.rememberVesperPlayerController
import io.github.ikaros.vesper.player.android.compose.rememberVesperPlayerUiState

@Composable
fun PlayerScreen() {
    val controller = rememberVesperPlayerController(
        resiliencePolicy = VesperPlaybackResiliencePolicy.resilient(),
        decoderBackend = VesperDecoderBackend.SystemOnly,
    )
    val uiState = rememberVesperPlayerUiState(controller)

    VesperPlayerSurface(controller = controller)

    // Bind your controls to:
    //   controller.play() / controller.pause()
    //   controller.seekBy(...) / controller.selectSource(...)
    //   uiState.playbackState / uiState.timeline / uiState.playbackRate
}
```

## Decoder Backends

`VesperDecoderBackend` controls how `vesper-player-kit` resolves decoders:

| Mode                 | Behavior                                                     |
| -------------------- | ------------------------------------------------------------ |
| `SystemOnly`         | Use platform decoders only (default)                         |
| `SystemPreferred`    | Allow optional extension decoders, prefer system decoders    |
| `ExtensionPreferred` | Prefer extension decoders when both paths can play the track |

`vesper-player-kit` does not depend on `androidx.media3:media3-exoplayer-ffmpeg`,
so the baseline AAR size stays unchanged when the FFmpeg extension is not
needed. Apps that want `SystemPreferred` or `ExtensionPreferred` with the FFmpeg
extension must add the Media3 FFmpeg dependency themselves.

Adding a Media3 FFmpeg extension or bundling Vesper's optional
`player-remux-ffmpeg` plugin makes the host responsible for FFmpeg notices,
corresponding source, configure flags, and LGPL relinking rights. See
[THIRD_PARTY_NOTICES.md](../../THIRD_PARTY_NOTICES.md) before publishing such
an artifact.

## JNI Artifacts

When building from source, the native library is produced by:

```sh
./scripts/vesper android jni
```

Output is written to
`lib/android/vesper-player-kit/src/main/jniLibs/<abi>/libvesper_player_android.so`.
Generated `.so` files are not committed to the repository.

## Runnable Sample

A Compose sample app that consumes these modules lives at
[examples/android-compose-host](../../examples/android-compose-host/).
