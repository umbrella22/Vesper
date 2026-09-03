# VesperPlayerKit for Android

Android-native host kit for the Vesper Player SDK. Distributed as Android `AAR`
artifacts and consumable from any Android app or library.

## Modules

| Module                         | Purpose                                                                                                                                                                             |
| ------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `vesper-player-kit`            | Core Android library: `VesperPlayerController`, `VesperPlayerSource`, `VesperTrackSelection`, `VesperDownloadManager`, JNI-backed `ExoPlayer` bridge, `libvesper_player_android.so` |
| `vesper-player-kit-external-playback` | Optional Google Cast, DLNA / UPnP AV discovery, local HTTP relay, relay FFmpeg adaptation JNI, route button, and default Cast options provider                              |
| `vesper-player-kit-ffmpeg-runtime`    | Optional FFmpeg runtime AAR used by download remux and external playback relay remux                                                                                        |
| `vesper-player-kit-decoder-mediacodec` | Optional MediaCodec decoder plugin AAR used by the explicit SDK-managed native-frame route; it does not bundle FFmpeg runtime libraries                                    |
| `vesper-player-kit-source-normalizer-ffmpeg` | Optional SourceNormalizer FFmpeg plugin AAR for diagnostics, source preflight, and opt-in normalized-resource playback; depends on the shared FFmpeg runtime |
| `vesper-player-kit-remux-ffmpeg` | Optional FFmpeg-backed post-download MP4 remux plugin AAR; depends on the core kit and shared FFmpeg runtime and does not bundle `libav*` |
| `vesper-player-kit-frame-processor-diagnostic` | Optional FrameProcessor diagnostic plugin AAR for capability probing and opt-in SDK-managed native-frame processing; it does not bundle FFmpeg runtime libraries |
| `vesper-player-kit-performance-diagnostics` | Optional BenchmarkSink plugin AAR for bounded playback and overlay-correlated UI performance reports |
| `vesper-player-kit-compose`    | Optional Jetpack Compose adapter: `VesperPlayerSurface`, `rememberVesperPlayerController`, `rememberVesperPlayerUiState`, lifecycle-scoped progress refresh                         |
| `vesper-player-kit-compose-ui` | Optional opinionated Compose UI: `VesperPlayerStage` and stage helpers built on top of the Compose adapter                                                                          |

The external playback, FFmpeg runtime, MediaCodec decoder plugin,
SourceNormalizer plugin, remux plugin, FrameProcessor diagnostic plugin,
performance diagnostics plugin,
Compose adapter, and higher-level Compose UI modules are
optional. View-based or non-Compose hosts can depend on `vesper-player-kit`
alone without pulling in Google Play Services, Cast Framework, DLNA discovery,
FFmpeg, native-frame plugins, plugin diagnostics, Compose, or Material3.

Kotlin namespaces:

- `io.github.umbrella22.vesper.player.android`
- `io.github.umbrella22.vesper.player.android.external`
- `io.github.umbrella22.vesper.player.android.compose`
- `io.github.umbrella22.vesper.player.android.compose.ui`

Native library: `libvesper_player_android.so`.

### Identifier baseline

All current Android coordinates and packages are rooted at
`io.github.umbrella22`. This is a breaking pre-release rename from
`io.github.ikaros`; the SDK does not ship package aliases. Consumers moving an
older source checkout must update Kotlin imports, fully qualified manifest
class names, ProGuard/R8 rules, and first-party `VesperPluginReference` values.
Rebuild the Android JNI library at the same revision because exported JNI
symbols include `io.github.umbrella22.vesper.player.android`.

## Distribution

Stable and prerelease tags publish these coordinates to Maven Central:

```kotlin
dependencies {
    implementation("io.github.umbrella22.vesper:vesper-player-kit:<version>")

    // Optional Compose integration.
    implementation("io.github.umbrella22.vesper:vesper-player-kit-compose:<version>")
    implementation("io.github.umbrella22.vesper:vesper-player-kit-compose-ui:<version>")

    // Optional Cast, DLNA, and relay integration. The matching FFmpeg runtime
    // coordinate is resolved transitively for relay format adaptation.
    implementation("io.github.umbrella22.vesper:vesper-player-kit-external-playback:<version>")

    // Optional source normalization. Core and FFmpeg runtime resolve transitively.
    implementation("io.github.umbrella22.vesper:vesper-player-kit-source-normalizer-ffmpeg:<version>")

    // Optional post-download MP4 remux. Core and FFmpeg runtime resolve transitively.
    implementation("io.github.umbrella22.vesper:vesper-player-kit-remux-ffmpeg:<version>")

    // Optional diagnostics. Prefer debugImplementation/profileImplementation
    // when Release packages must not contain the plugin binary.
    debugImplementation("io.github.umbrella22.vesper:vesper-player-kit-performance-diagnostics:<version>")
    profileImplementation("io.github.umbrella22.vesper:vesper-player-kit-performance-diagnostics:<version>")
}
```

Use `mavenCentral()` in the consuming project's dependency repositories. The
core coordinate contains the Android host kit and arm64 JNI library. The
Compose coordinates preserve the module order: UI depends on the Compose
adapter, and the adapter depends on the core kit.

GitHub Releases also publish the following standalone artifacts via
`.github/workflows/mobile-lib-release.yml`:

- `VesperPlayerKit-android-arm64-v8a.aar`
- `VesperPlayerKitCompose-android-arm64-v8a.aar`
- `VesperPlayerKitComposeUi-android-arm64-v8a.aar`

Android packaging is `arm64-v8a` only. Use an arm64 device or arm64 Android
emulator. See [Release Downloads](../../README.md#release-downloads) for the
public package names and artifact-selection notes.

Maven releases publish eight coordinates: core, both Compose modules, external
playback, the shared FFmpeg runtime, SourceNormalizer, post-download remux, and
performance diagnostics.
The two FFmpeg-backed plugin coordinates remain direct opt-ins. Default
standalone GitHub Release staging remains core-only. Use the dedicated plugin
build commands, or set `VESPER_ANDROID_INCLUDE_OPTIONAL_PLUGINS=1` for release
staging, when you intentionally need Decoder, FrameProcessor, or the complete
source-built plugin set.

The optional `vesper-player-kit-compose-ui` module remains available both as a
source module and as a release AAR.

Prerelease tags such as `v0.5.0-rc.1` publish immutable prerelease coordinates
such as `0.5.0-rc.1`; consumers must request that exact prerelease version.
Stable tags publish the matching stable coordinates.

### Maven Central Publishing

The `publish-maven-central` release job builds, signs, validates, and uploads one
Central Portal bundle containing all eight coordinates, including the two
opt-in FFmpeg-backed plugin AARs and the diagnostics AAR. The Maven
`groupId` remains `io.github.umbrella22.vesper`; the Portal namespace controls
publishing permission and may be that value or one of its parent prefixes. The
job requires:

- an approved Central namespace, defaulting to `io.github.umbrella22`;
- a Central Portal user token stored as `MAVEN_CENTRAL_USERNAME` and
  `MAVEN_CENTRAL_PASSWORD` GitHub secrets;
- an ASCII-armored signing key in `MAVEN_GPG_PRIVATE_KEY` and its passphrase in
  `MAVEN_GPG_PASSPHRASE`;
- the matching public signing key published to a public OpenPGP keyserver.

Set the `MAVEN_NAMESPACE` GitHub variable when the approved Portal namespace
differs from the default. Changing it does not change the published coordinates.
The job uses automatic Central publication only after the GitHub Release has
completed. Local verification stages the signed repository and bundle without
contacting Central:

```sh
MAVEN_GPG_PRIVATE_KEY="$(path-to-secret-provider)" \
MAVEN_GPG_PASSPHRASE="$(path-to-passphrase-provider)" \
./scripts/vesper android publish-maven-central v0.5.0-rc.1 --dry-run
```

## Minimum Requirements

- Android API Level 26+
- Kotlin 2.x
- arm64 device or arm64 emulator

These requirements define the supported product boundary. The Android package
does not plan to add 32-bit or Intel ABIs, or compatibility behavior for older
Android versions, without a separate product-direction change.

## Source Build Toolchain

- Gradle Wrapper `9.7.1`
- Android Gradle Plugin `9.1.0`
- Gradle runtime JDK `21`
- Java / Kotlin bytecode target `17`
- Kotlin `2.2.10`
- Android SDK `36` with Build Tools `36.0.0`
- Android NDK `29.0.14206865`

Gradle and CI pin the listed NDK version. Shell build helpers allow an explicit
NDK override and can fall back to another complete installed NDK when the
default installation is unavailable.

## Building From Source

From the repository root:

```sh
./scripts/vesper android aar
./scripts/vesper android stage-release
VESPER_ANDROID_INCLUDE_OPTIONAL_PLUGINS=1 ./scripts/vesper android stage-release
```

Without a Gradle CLI, open `lib/android` in Android Studio and run:

- `:vesper-player-kit:assembleRelease`
- `:vesper-player-kit-compose:assembleRelease`
- `:vesper-player-kit-compose-ui:assembleRelease`

Optional plugin and extension modules can still be assembled explicitly:

- `:vesper-player-kit-external-playback:assembleRelease`
- `:vesper-player-kit-ffmpeg-runtime:assembleRelease`
- `:vesper-player-kit-decoder-mediacodec:assembleRelease`
- `:vesper-player-kit-source-normalizer-ffmpeg:assembleRelease`
- `:vesper-player-kit-frame-processor-diagnostic:assembleRelease`
- `:vesper-player-kit-performance-diagnostics:assembleRelease`

## Subtitle Baseline

The Android host kit forwards external SRT (`application/x-subrip`), WebVTT
(`text/vtt`), and SSA/ASS (`text/x-ssa`) tracks through Media3
`MediaItem.SubtitleConfiguration`. Text cues are rendered by a native overlay in
the same surface host as the video, so `SurfaceView` remains the default video
path and subtitle text does not cross Dart. `VesperSubtitleStyle` supports
visibility and a validated `fontScale` range of `0.5...3.0`. Per-cue typography,
animations, and subtitle synchronization offsets are not part of the stable
contract.

HTTP URLs ending in `.flv` are inferred as progressive VOD. Use
`VesperPlayerSource.flvLive(...)` only when the caller explicitly knows that the
source is an HTTP-FLV live stream. RTMP remains explicitly unsupported by the
stable Android host kit; RTSP and HTTP-FLV require real-device validation before
release support claims.

## Public API

Core (`vesper-player-kit`):

- `VesperPlayerController` — playback control surface (`play / pause / seek / selectSource / setPlaybackRate / setAbrPolicy / setResiliencePolicy / set*TrackSelection`)
- `VesperPlayerControllerFactory` — `createDefault(...)` for production bridge, `createPreview(...)` for a Fake bridge
- `VesperPlayerBackendFamily` — public backend family snapshot exposed through `VesperPlayerController.backendFamily`
- `VesperPlayerSource` — media source DTO with `local / remote / hls / dash / rtmp / rtsp / flvLive` factories
- `VesperExternalSubtitleSource` and `VesperSubtitleStyle` — external SRT / WebVTT / SSA attachment plus visibility and bounded font scaling
- `VesperPlayerDrmConfiguration` — Widevine license metadata for direct Media3 playback
- `VesperTrackSelection` — audio / subtitle / video track selection (`auto`, `disabled`, `track(id)`)
- Reactive state on the controller: `uiState`, `trackCatalog`, `trackSelection`, `subtitleState`, `requestedSubtitleSelection`, `confirmedSubtitleSelection`, `effectiveSubtitleTrackId`, `effectiveVideoTrackId`, `videoVariantObservation`, `resiliencePolicy` (all `StateFlow<...>`)
- `VesperAbrPolicy` — `auto`, `constrained`, `fixedTrack`
- `VesperPlaybackResiliencePolicy` with presets: `balanced()`, `streaming()`, `resilient()`, `lowLatency()`
- `VesperBufferingPolicy`, `VesperRetryPolicy`, `VesperCachePolicy`
- `VesperPreloadBudgetPolicy` — caps for concurrent preload tasks, memory, disk, warm-up window
- `VesperTrackPreferencePolicy` — preferred audio / subtitle languages
- `VesperDecoderBackend` — `SystemOnly` / `SystemPreferred` / `ExtensionPreferred`
- `VesperVideoSurfaceKind` — `SurfaceView` (default, HDR / high frame rate) or `TextureView` (scrolling / animated stages)
- `VesperDownloadManager` — download orchestration with `createTask / startTask / pauseTask / resumeTask / removeTask / exportTaskOutput / shareTaskOutput / saveTaskOutput`

`setSubtitleTrackSelection` is a suspending API in 0.4. It returns only after
Media3 confirms the requested state or throws a structured subtitle error.
`selectSourceAsync`, `seekByAsync`, `seekToRatioAsync`, and
`seekToLiveEdgeAsync` also wait for Media3 command readiness or completion.
Superseded commands throw `VesperPlayerCommandException` and do not overwrite
the current generation's `lastError`.
`VesperSubtitleSideLoad` remains a deprecated typealias, and
`VesperPlayerSource.subtitleConfigurations` remains a deprecated read-only alias;
new source declarations use `externalSubtitles`.

External playback (`vesper-player-kit-external-playback`):

- `VesperExternalPlaybackController` — unified Cast/DLNA route discovery and playback control
- `routes: StateFlow<List<VesperExternalPlaybackRoute>>` — route snapshots
- `events: SharedFlow<VesperExternalPlaybackEvent>` — route, playback, and diagnostic events
- `VesperExternalPlaybackMediaItem`, route/media/result/event DTOs, proxy policy, and format adaptation config
- `VesperExternalRouteButton` — Cast route button view backed by the Cast framework
- `VesperExternalCastOptionsProvider` — default Cast options provider using Google's Default Media Receiver unless the host overrides `io.github.umbrella22.vesper.player.android.external.RECEIVER_APPLICATION_ID`

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

## DRM Boundary

Android DRM support is native-only. Set
`VesperPlayerSource.drmConfiguration` with `keySystem = "widevine"` to let the
direct Media3 route build a `MediaItem.DrmConfiguration`. The license URI,
license headers, and `multiSession` flag are passed to Media3; source media
headers remain separate from license request headers. The Widevine license URI
must be non-empty; missing license configuration fails before Media3 startup so
hosts receive a clear configuration error.

DRM sources do not enter Rust, JNI media pipelines, optional plugins, download,
preload, remux, SourceNormalizer, native-frame playback, or external playback
relay routes. Those paths fail with an unsupported capability error instead of
silently stripping DRM. Non-Widevine key systems on Android also fail as
unsupported.

Provisioning, license acquisition, DRM system, and license-expired failures are
reported as retriable runtime/network failures. Scheme unsupported, device
revoked, and disallowed operation failures remain unsupported capability errors.

Private encryption that is not Widevine should be handled by a separate
host-owned or future SDK pre-decryption adapter before a normal source is handed
to Media3. That is outside the current DRM contract.

### DRM Diagnostics

The license must be issued for the same asset key ID as the content. Different
assets mean different keys, so the license may load while playback remains in
`BUFFERING` because the encrypted samples cannot be decrypted. For DASH
sources, compare the manifest `cenc:default_KID` with the provider asset that
backs `licenseUri`.

Capture DRM logs and let a Widevine source run for at least 30 seconds before
judging cross-region license behavior:

```bash
adb logcat -c
adb logcat -v time | grep -Ei \
  "VesperPlayerAndroidHost|DrmSession|onDrmKeys|MediaDrm|DefaultDrmSession|CryptoException"
```

Read `onDrmKeysLoaded` / `onDrmSessionManagerError` (not the raw `MediaDrm event`
code, whose meaning is version-dependent) as the source of truth. A Dolby Vision
Profile 5 source can also fail as a video capability mismatch on devices without
a compatible Dolby Vision decoder; that is distinct from Widevine license
failure.

For a physical-device check using the public Shaka Widevine demo pair, run the
opt-in instrumentation test below. It is skipped unless the network argument is
present so routine device suites remain independent of public services:

```bash
gradle -p lib/android \
  -Pvesper.player.android.abis=arm64-v8a \
  -Pandroid.testInstrumentationRunnerArguments.class=io.github.umbrella22.vesper.player.android.VesperWidevinePlaybackInstrumentationTest \
  -Pandroid.testInstrumentationRunnerArguments.vesperWidevineNetwork=true \
  :vesper-player-kit:connectedDebugAndroidTest
```

The test requires keys loaded, a video decoder, a rendered first frame, and at
least three seconds of timeline progress. A device that has not completed
Widevine provisioning must also reach Google's provisioning service; access to
the manifest or license host alone is insufficient.

For Dolby Vision Online Delivery Kit smoke coverage, the example hosts use the
public signal layout below. The old Browser Test Kit paths are retired/restricted
and must not be restored:

```text
https://ott.dolby.com/OnDelKits/Dolby_Vision_Online_Delivery_Kit/v1/test_signals/{clear|cenc|cbcs}/{P5|P8_1|P8_4}_{25|30|60}/{dash.mpd|master.m3u8}
```

The opt-in Android gate exercises a P8.1 30fps clear DASH signal and a CENC
DASH signal with the configured EZDRM Widevine license. It requires native
Media3 decoder initialization, a rendered first frame, at least three seconds
of timeline progress, and `onDrmKeysLoaded` for CENC:

```bash
gradle -p lib/android \
  -Pvesper.player.android.abis=arm64-v8a \
  -Pandroid.testInstrumentationRunnerArguments.class=io.github.umbrella22.vesper.player.android.VesperDolbyVisionPlaybackInstrumentationTest \
  -Pandroid.testInstrumentationRunnerArguments.vesperDolbyVisionNetwork=true \
  :vesper-player-kit:connectedDebugAndroidTest
```

This gate is opt-in because it depends on external Dolby and license services;
routine Android test suites remain independent of those services. Dolby
protected media stays on the direct native Media3 route and does not enter Rust
media processing, optional plugins, download, preload, remux, or relay paths.

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
import io.github.umbrella22.vesper.player.android.VesperPlaybackResiliencePolicy
import io.github.umbrella22.vesper.player.android.VesperDecoderBackend
import io.github.umbrella22.vesper.player.android.compose.VesperPlayerSurface
import io.github.umbrella22.vesper.player.android.compose.rememberVesperPlayerController
import io.github.umbrella22.vesper.player.android.compose.rememberVesperPlayerUiState

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
- `vesper-player-kit-decoder-mediacodec` contains only
  `libvesper_decoder_mediacodec.so`. It provides the Android hardware decoder
  plugin for the explicit SDK-managed native-frame route and must not carry
  `libav*`, `libsw*`, `libxml2*`, `libssl*`, or `libcrypto*` copies.
- `vesper-player-kit-remux-ffmpeg` contains only
  `libvesper_remux_ffmpeg.so` plus profile metadata; it depends on the core kit
  and shared runtime AAR.
- `vesper-player-kit-source-normalizer-ffmpeg` contains only
  `libvesper_source_normalizer_ffmpeg.so` plus profile metadata; it depends on
  the shared runtime AAR and must not carry `libav*`, `libsw*`, `libxml2*`,
  `libssl*`, or `libcrypto*` copies.
- `vesper-player-kit-frame-processor-diagnostic` contains only
  `libvesper_frame_processor_diagnostic.so` and does not depend on FFmpeg.

The mobile SourceNormalizer configuration is opt-in through
`VesperSourceNormalizerConfiguration`. `DiagnosticsOnly` loads the plugin and
reports capabilities. `PreflightOnly` may open and close a packet session for
the selected source and reports the result through `pluginDiagnostics`, but
ExoPlayer still plays the original `VesperPlayerSource`. Preflight failure is
non-fatal. Callers select plugins with `VesperPluginReference`; the embedded
registry maps each selected identity to its packaged `.so` inside the host kit.
The shared FFmpeg runtime AAR is a dynamic dependency and is not a plugin
registry entry. `PreferNormalized` and `RequireNormalized` may replace the
platform source with a disk-backed fMP4 or short-window HLS resource served by
the internal loopback server. `PreferNormalized` falls back to the original
source when normalization fails; `RequireNormalized` reports a source error.
Standard HLS and DASH stay native-first unless normalization is explicitly
required or forced by a test profile. The repository smoke expectations live in
`fixtures/media/source-normalizer-smoke-matrix.json`.

The mobile FrameProcessor path is opt-in through
`VesperNativeFramePipelineConfiguration`. The Android host kit now reports the
native-frame route decision for SourceNormalizer packet input, MediaCodec,
SurfaceView presentation, fallback reason, and frame counters. When
`preferNativeFrame` or `requireNativeFrame` is selected and the
SourceNormalizer, MediaCodec decoder, and optional FrameProcessor references
resolve from the embedded registry, the explicit SDK-managed SDR native-frame route reads packets,
decodes through MediaCodec, and presents through a `SurfaceView`. HDR and
Dolby Vision are routed to ExoPlayer / system playback; the
SDK-managed native-frame route is SDR-only today and is not an HDR-ready path.
The capability probe reports `recommendedPlaybackPath = systemPlayer` with an
`hdrNativeFrameUnsupported` capability warning rather than claiming programmable
native-frame HDR support.

Android video decode remains hardware-only in the main host kit. The
`VesperHardwareMediaCodecSelector` filters out software-only video decoders and
reports hardware / secure hardware decoder diagnostics; software fallback, if
reintroduced, must be an optional separate route instead of being folded into the
main `vesper-player-kit` behavior.
`preferNativeFrame` falls back to ExoPlayer when that route is unavailable;
`requireNativeFrame` reports a capability error. `TextureView` remains a system
player surface and falls back or fails according to the selected mode. Default
ExoPlayer playback is unchanged.

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
reviewed before release. Android OpenSSL overlays default to the OpenSSL 3.5 LTS
series, resolve the highest matching patch from `third_party/_cache` first, and
rebuild stale local prebuilts when their recorded version differs from the
selected version. See
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
