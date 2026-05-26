# VesperPlayerKit for Android

Android-native host kit for the Vesper Player SDK. Distributed as Android `AAR`
artifacts and consumable from any Android app or library.

## Modules

| Module                         | Purpose                                                                                                                                                                             |
| ------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `vesper-player-kit`            | Core Android library: `VesperPlayerController`, `VesperPlayerSource`, `VesperTrackSelection`, `VesperDownloadManager`, JNI-backed `ExoPlayer` bridge, `libvesper_player_android.so` |
| `vesper-player-kit-external-playback` | Optional Google Cast, DLNA / UPnP AV discovery, local HTTP relay, relay FFmpeg adaptation JNI, route button, and default Cast options provider                              |
| `vesper-player-kit-ffmpeg-runtime`    | Optional FFmpeg runtime AAR used by download remux and external playback relay remux                                                                                        |
| `vesper-player-kit-source-normalizer-ffmpeg` | Optional SourceNormalizer FFmpeg plugin AAR for diagnostics, source preflight, and opt-in normalized-resource playback; depends on the shared FFmpeg runtime |
| `vesper-player-kit-frame-processor-diagnostic` | Optional FrameProcessor diagnostic plugin AAR for capability probing only; it does not open frame sessions or participate in playback                                  |
| `vesper-player-kit-compose`    | Optional Jetpack Compose adapter: `VesperPlayerSurface`, `rememberVesperPlayerController`, `rememberVesperPlayerUiState`, lifecycle-scoped progress refresh                         |
| `vesper-player-kit-compose-ui` | Optional opinionated Compose UI: `VesperPlayerStage` and stage helpers built on top of the Compose adapter                                                                          |

The external playback, FFmpeg runtime, SourceNormalizer plugin, FrameProcessor
diagnostic plugin, Compose adapter, and higher-level Compose UI modules are
optional. View-based or non-Compose hosts can depend on `vesper-player-kit`
alone without pulling in Google Play Services, Cast Framework, DLNA discovery,
FFmpeg, plugin diagnostics, Compose, or Material3.

Kotlin namespaces:

- `io.github.ikaros.vesper.player.android`
- `io.github.ikaros.vesper.player.android.external`
- `io.github.ikaros.vesper.player.android.compose`
- `io.github.ikaros.vesper.player.android.compose.ui`

Native library: `libvesper_player_android.so`.

## Distribution

GitHub Releases publish the following artifacts via
`.github/workflows/mobile-lib-release.yml`:

- `VesperPlayerKit-android-arm64-v8a.aar`
- `VesperPlayerKitCompose-android-arm64-v8a.aar`
- `VesperPlayerKitComposeUi-android-arm64-v8a.aar`
- `VesperPlayerKitExternalPlayback-android-arm64-v8a.aar`
- `VesperPlayerKitFfmpegRuntime-android-arm64-v8a.aar`
- `VesperPlayerKitSourceNormalizerFfmpeg-android-arm64-v8a.aar`
- `VesperPlayerKitFrameProcessorDiagnostic-android-arm64-v8a.aar`

Android packaging is `arm64-v8a` only. Use an arm64 device or arm64 Android
emulator. See [Release Downloads](../../README.md#release-downloads) for the
public package names and artifact-selection notes.

The optional `vesper-player-kit-compose-ui` module remains available both as a
source module and as a release AAR.

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
- `:vesper-player-kit-external-playback:assembleRelease`
- `:vesper-player-kit-ffmpeg-runtime:assembleRelease`
- `:vesper-player-kit-compose:assembleRelease`
- `:vesper-player-kit-compose-ui:assembleRelease`

## Public API

Core (`vesper-player-kit`):

- `VesperPlayerController` — playback control surface (`play / pause / seek / selectSource / setPlaybackRate / setAbrPolicy / setResiliencePolicy / set*TrackSelection`)
- `VesperPlayerControllerFactory` — `createDefault(...)` for production bridge, `createPreview(...)` for a Fake bridge
- `VesperPlayerBackendFamily` — public backend family snapshot exposed through `VesperPlayerController.backendFamily`
- `VesperPlayerSource` — media source DTO with `local / remote / hls / dash` factories
- `VesperTrackSelection` — audio / subtitle / video track selection (`auto`, `disabled`, `track(id)`)
- Reactive state on the controller: `uiState`, `trackCatalog`, `trackSelection`, `effectiveVideoTrackId`, `videoVariantObservation`, `resiliencePolicy` (all `StateFlow<...>`)
- `VesperAbrPolicy` — `auto`, `constrained`, `fixedTrack`
- `VesperPlaybackResiliencePolicy` with presets: `balanced()`, `streaming()`, `resilient()`, `lowLatency()`
- `VesperBufferingPolicy`, `VesperRetryPolicy`, `VesperCachePolicy`
- `VesperPreloadBudgetPolicy` — caps for concurrent preload tasks, memory, disk, warm-up window
- `VesperTrackPreferencePolicy` — preferred audio / subtitle languages
- `VesperDecoderBackend` — `SystemOnly` / `SystemPreferred` / `ExtensionPreferred`
- `VesperVideoSurfaceKind` — `SurfaceView` (default, HDR / high frame rate) or `TextureView` (scrolling / animated stages)
- `VesperDownloadManager` — download orchestration with `createTask / startTask / pauseTask / resumeTask / removeTask / exportTaskOutput / shareTaskOutput / saveTaskOutput`

External playback (`vesper-player-kit-external-playback`):

- `VesperExternalPlaybackController` — unified Cast/DLNA route discovery and playback control
- `routes: StateFlow<List<VesperExternalPlaybackRoute>>` — route snapshots
- `events: SharedFlow<VesperExternalPlaybackEvent>` — route, playback, and diagnostic events
- `VesperExternalPlaybackMediaItem`, route/media/result/event DTOs, proxy policy, and format adaptation config
- `VesperExternalRouteButton` — Cast route button view backed by the Cast framework
- `VesperExternalCastOptionsProvider` — default Cast options provider using Google's Default Media Receiver unless the host overrides `io.github.ikaros.vesper.player.android.external.RECEIVER_APPLICATION_ID`

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

## Local-Network Cleartext HTTP

Hosts that use DLNA discovery or the local relay must own Android cleartext
policy. The SDK library manifests do not enable cleartext traffic globally.
Apps that need local-network `http://` device descriptions or relay URLs can
opt in at the app layer:

```xml
<application
    android:usesCleartextTraffic="true">
</application>
```

Hosts that do not want global cleartext should provide their own Android
network security configuration and allow only the local hosts they use for
discovery and relay traffic.

## Playback Screen Awake Policy

`VesperPlayerController` keeps the attached playback view screen-awake while
playback is actively running by default. Hosts can disable the policy when they
create the controller or later call `setKeepScreenOnDuringPlayback(false)`.

## Download Manager

`VesperDownloadManager` supports SDK-managed task restore and resumable partial
transfers. With the default `VesperDownloadConfiguration`, task snapshots are
persisted under the download base directory, interrupted preparing/downloading
tasks are restored on startup, and existing partial remote files are resumed with
range requests when the server supports them. If a server ignores a resume range,
the manager deletes only that partial resource and restarts the same resource from
byte zero; expired or unavailable URLs fail with a stale-resource error so the
host can refresh the media link.

When an HTTP resource has a known `sizeBytes` and no explicit byte range, the
foreground executor streams the resource by default and sends `Range:
bytes=<existing>-` only when resuming a partial file. Fixed closed Range chunks
are used only when `rangeChunkBytes` is explicitly configured. Each `206 Partial
Content` response is validated against `Content-Range` before bytes are
appended.

The default download base directory is under the app-private `filesDir`
(`filesDir/vesper-downloads`). The SDK does not request
`MANAGE_EXTERNAL_STORAGE`. `shareTaskOutput(...)` exposes a completed private
file through the SDK `FileProvider` authority
`${applicationId}.vesper.player.fileprovider`; `saveTaskOutput(...)` copies a
completed file into Android 10+ MediaStore `Downloads` or `Movies` with
`IS_PENDING`, without requesting broad storage access. Android 9 and older hosts
should use the share helper or a host-owned export flow.

When `VesperPlayerSource.headers` is set, the download executor forwards those
headers to all SDK-owned network operations for the task: HLS, DASH, and FLV
manifest reads; HEAD and `Range: bytes=0-0` size probes; Media3 `DataSpec`
fallback reads; single-file transfers; HLS map and segment transfers; DASH init
and media segment transfers; FLV clip transfers; and size completion for
prebuilt asset indexes. Empty header names and blank values are ignored, and the
SDK does not add site-specific headers on its own.

Hosts that can refresh signed or short-lived media URLs may pass a
`VesperDownloadStaleResourceRecoverer` to `VesperDownloadManager`. The recoverer
receives the failed task and a `VesperDownloadStaleResource`, returns a refreshed
`VesperDownloadSource`, and the executor re-runs preparation before starting the
same task. If no recoverer is provided, stale resources fail normally.

This is not an Android `WorkManager` or download `ForegroundService` wrapper for
process-death background transfer. Hosts that need OS-managed background
downloads should own that service layer, use the correct Android
`foregroundServiceType` such as `dataSync` when required, and feed completed
local assets back into the SDK.

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

Optional Vesper FFmpeg features use a split runtime:

- `vesper-player-kit-ffmpeg-runtime` is the only Android AAR that packages
  `libav*` plus enabled external runtime dependencies such as libxml2.
- `vesper-player-kit-external-playback` contains the Cast, DLNA, relay, and
  relay FFmpeg adaptation APIs/JNI, but it must not carry its own `libav*`
  copies.
- `player-remux-ffmpeg` contains only the download remux plugin `.so`.
- `vesper-player-kit-source-normalizer-ffmpeg` contains only
  `libplayer_source_normalizer_ffmpeg.so` plus profile metadata; it depends on
  the shared runtime AAR and must not carry `libav*`, `libsw*`, `libxml2*`,
  `libssl*`, or `libcrypto*` copies.
- `vesper-player-kit-frame-processor-diagnostic` contains only
  `libplayer_frame_processor_diagnostic.so` and does not depend on FFmpeg.

The mobile SourceNormalizer configuration is opt-in through
`VesperSourceNormalizerConfiguration`. `DiagnosticsOnly` loads the plugin and
reports capabilities. `PreflightOnly` may open and close a packet session for
the selected source and reports the result through `pluginDiagnostics`, but
ExoPlayer still plays the original `VesperPlayerSource`. Preflight failure is
non-fatal. `pluginLibraryPaths` must contain plugin `.so` paths only; the
shared FFmpeg runtime AAR is resolved by Android packaging, not passed as a
plugin path.

The mobile FrameProcessor configuration is opt-in through
`VesperFrameProcessorConfiguration`. Its v1 Android artifact is a diagnostics
shell only: supported plugins are reported as available or failed to load, but
they are never marked as participated, never open frame sessions, and never
alter rendering. Mobile decoder extension artifacts remain deferred.

Build the runtime through the root FFmpeg profile CLI:

```sh
./scripts/vesper ffmpeg --platform android --profile default --abi arm64-v8a
```

Hosts that consume prebuilt AARs do not need to wire these generation tasks into
their app build; the runtime assets and JNI libraries are already packaged in
the AAR. The explicit Gradle `merge*Assets`, `merge*JniLibFolders`, and
`generate*Lint*Model` dependencies shown in the repository examples are only
needed when a host consumes these modules as local Gradle project dependencies
and runs the repository generation scripts during the same build.

Do not use Gradle `pickFirst` to hide duplicate `libav*` payloads. If both DLNA
relay remux and download remux are enabled, package one shared
`vesper-player-kit-ffmpeg-runtime` profile and keep the relay/plugin artifacts
free of FFmpeg runtime libraries.

Adding a Media3 FFmpeg extension or bundling Vesper's optional FFmpeg runtime
makes the host responsible for FFmpeg notices, corresponding source, configure
flags, and LGPL relinking rights. The default Vesper `download-remux`,
`relay-remux`, and `default` profiles validate no-network/no-OpenSSL builds; any
overlay that enables GPL, nonfree, OpenSSL, or network capability must be
reviewed before release. See
[THIRD_PARTY_NOTICES.md](../../THIRD_PARTY_NOTICES.md) before publishing such an
artifact.

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
