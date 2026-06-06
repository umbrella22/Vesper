# vesper_player

A cross-platform Flutter video player built around native-first backends:

- Android uses ExoPlayer through the Vesper Android host kit
- iOS uses AVPlayer through the Vesper iOS host kit

The package exposes one Dart API surface so host apps can keep playback, track
selection, resilience, download, preload, and benchmark capture flows aligned
across platforms.

## Platform Support

| Feature                  | Android                                      | iOS                                                 |
| ------------------------ | -------------------------------------------- | --------------------------------------------------- |
| Local files              | ✅                                           | ✅                                                  |
| Progressive HTTP         | ✅                                           | ✅                                                  |
| HLS                      | ✅                                           | ✅                                                  |
| DASH                     | ✅                                           | ✅ DASH-to-HLS bridge for VOD / live fMP4           |
| Live streams             | ✅                                           | ✅                                                  |
| Live DVR                 | ✅                                           | ✅                                                  |
| Track selection          | ✅                                           | ✅                                                  |
| Adaptive bitrate (ABR)   | ✅                                           | ⚠️ Constrained + best-effort fixed-track on iOS 15+ |
| Buffering / retry policy | ✅                                           | ✅                                                  |
| Download management      | ✅                                           | ✅                                                  |
| Preload                  | ✅                                           | ✅                                                  |
| System playback controls | ✅ MediaSession notification + FGS           | ✅ Now Playing / RemoteCommand                      |
| External playback        | ✅ Optional `vesper_player_external_playback` package | ✅ AirPlay route picker via `vesper_player_ui`      |

The main package currently registers Android and iOS implementations only.
Desktop Flutter implementations, including macOS, are intentionally not shipped
while the Flutter desktop integration model settles.

## Installation

The Flutter packages are source-distributed from this repository and currently
set `publish_to: none`. In a host app, use path or git dependencies until the
package family is published:

```yaml
dependencies:
  vesper_player:
    path: path/to/rust-player-sdk/lib/flutter/vesper_player
  # Optional unified Android Cast / DLNA external playback.
  vesper_player_external_playback:
    path: path/to/rust-player-sdk/lib/flutter/vesper_player_external_playback
  # Optional stage controls and AirPlay route button.
  vesper_player_ui:
    path: path/to/rust-player-sdk/lib/flutter/vesper_player_ui
```

## Quick Start

### Minimal playback

```dart
import 'package:vesper_player/vesper_player.dart';

// 1. Create a controller.
final controller = await VesperPlayerController.create(
  initialSource: VesperPlayerSource.hls(
    uri: 'https://example.com/stream.m3u8',
    label: 'Sample video',
  ),
);

// 2. Embed the view in your widget tree.
VesperPlayerView(controller: controller)

// 3. Start playback.
await controller.play();

// 4. Dispose when the widget goes away.
await controller.dispose();
```

### Listen to playback state

```dart
// Snapshot stream: emits when player state changes.
controller.snapshots.listen((snapshot) {
  print('Playback state: ${snapshot.playbackState}');
  print('Position: ${snapshot.timeline.positionMs}ms');
  print('Buffering: ${snapshot.isBuffering}');
  print('Retry attempts: ${snapshot.resiliencePolicy.retry.maxAttempts}');
});

// Event stream: emits errors and lifecycle events.
controller.events.listen((event) {
  if (event is VesperPlayerErrorEvent) {
    print('Error: ${event.error.message}');
  }
});

// You can also read the latest snapshot directly.
final snapshot = controller.snapshot;
```

`VesperPlayerSnapshot` is the authoritative runtime view of the active backend.
It carries timeline state, capabilities, current track selection, the effective
runtime video variant through `effectiveVideoTrackId`, explicit fixed-track
settling state through `fixedTrackStatus`, raw runtime bitrate and size
evidence through `videoVariantObservation`, the effective
`resiliencePolicy`, and the latest surfaced playback error.

## Core API

### `VesperPlayerController`

The primary control surface for playback.

```dart
final controller = await VesperPlayerController.create(
  initialSource: VesperPlayerSource.hls(uri: 'https://example.com/stream.m3u8'),
  renderSurfaceKind: VesperPlayerRenderSurfaceKind.auto,
  resiliencePolicy: const VesperPlaybackResiliencePolicy.resilient(),
  trackPreferencePolicy: const VesperTrackPreferencePolicy(
    preferredAudioLanguage: 'en',
    preferredSubtitleLanguage: 'en',
  ),
);

await controller.selectSource(
  VesperPlayerSource.local(uri: '/path/to/video.mp4'),
);
await controller.play();
await controller.pause();
await controller.togglePause();
await controller.stop();

await controller.seekBy(10000);
await controller.seekToRatio(0.5);
await controller.seekToLiveEdge();

await controller.setPlaybackRate(1.5);
```

### `VesperPlayerView`

Embeds the native video surface into Flutter UI.

On Android, `VesperPlayerController.create(renderSurfaceKind: ...)` controls
the native surface used by `VesperPlayerView`. The default `auto` mode uses
`SurfaceView` so Flutter 3.44+ hosts can take the high-fidelity native video
path by default. Select `textureView` explicitly when a screen depends on
complex Flutter overlays, scrolling, clipping, rounded corners, or
animation-heavy composition. iOS accepts the option for API symmetry but always
uses the platform's AVPlayer-backed surface.

```dart
VesperPlayerView(
  controller: controller,
  visible: true,
  overlay: Stack(
    children: [
      // Your overlay UI goes here.
    ],
  ),
)
```

### System Playback

System playback integration is optional and controlled from the Flutter
controller. It enables the platform media session, lock-screen / notification
controls, and background audio continuation for the active player.

```dart
final status = await controller.getSystemPlaybackPermissionStatus();
if (status == VesperSystemPlaybackPermissionStatus.denied) {
  await controller.requestSystemPlaybackPermissions();
}
await controller.configureSystemPlayback(
  const VesperSystemPlaybackConfiguration(
    metadata: VesperSystemPlaybackMetadata(
      title: 'Sample video',
      artist: 'Vesper Player SDK',
      contentUri: 'https://example.com/stream.m3u8',
    ),
    controls: VesperSystemPlaybackControls.videoDefault(),
  ),
);

await controller.updateSystemPlaybackMetadata(
  const VesperSystemPlaybackMetadata(title: 'Next episode'),
);
await controller.clearSystemPlayback();
```

The default configuration is enabled, continues audio in the background, shows
system controls, and enables 10-second seek back / play-pause / seek forward
system media actions. Custom seek offsets are clamped to 1-60 seconds, and
`showSeekActions: false` removes seek actions even when `controls` includes
them. The SDK supports one active system media session: the most recently
configured controller owns system controls.

Host apps still own platform declarations. iOS apps must include
`UIBackgroundModes = audio` when background playback is intended. Android apps
must merge or declare foreground-service media playback permissions. Android
13+ exempts media-session playback notifications from the runtime notification
permission, so a denied `POST_NOTIFICATIONS` result must not stop playback; use
the permission API only for app-controlled notification UX.

### AirPlay and Cast

For iOS route selection, depend on `vesper_player_ui` and place
`VesperAirPlayRouteButton` near your player controls, or
`VesperAirPlayRouteIconButton` in a `VesperPlayerStage` top-bar action slot:

```dart
VesperAirPlayRouteButton(controller: controller)
```

The button is backed by `AVRoutePickerView` and prioritizes video-capable
routes by default. Users can also continue to route from Control Center.
AirDrop is file sharing, not media playback routing.

For unified Android Cast / DLNA control, depend on the optional
`vesper_player_external_playback` package. It keeps discovery, relay, and Cast
Framework dependencies outside the default player package:

```dart
final external = VesperExternalPlaybackController();
await external.startDiscovery();
await external.connect(route.routeId);
await external.load(
  VesperExternalPlaybackMediaItem(
    sources: <VesperPlayerSource>[source],
    metadata: const VesperSystemPlaybackMetadata(title: 'Sample video'),
  ),
);
```

Cast route selection still uses the system Cast route button. DLNA routes are
reported through `VesperExternalPlaybackController.routes`. Sources with
headers, local files, and `content://` inputs are served through a tokenized
local HTTP relay when the proxy policy allows it. Cast V2 direct playback still
supports remote `http` / `https` HLS, DASH, and progressive sources with the
default Google receiver. DRM, transcoding, DASH manifest rewrite, and custom
receiver flows are outside the MVP scope.

Android hosts that use DLNA discovery or relay-backed local playback must
declare their own cleartext HTTP policy in the app manifest or Android network
security configuration. The SDK packages do not enable app-wide cleartext
traffic.

Use `VesperExternalRouteIconButton()` from `vesper_player_external_playback` in
a `VesperPlayerStage` top-bar action slot on Android to surface the system Cast
route button. `VesperExternalRouteButton()` remains available for existing
control rows.

### `VesperPlayerSource`

```dart
VesperPlayerSource.hls(uri: 'https://example.com/stream.m3u8')
VesperPlayerSource.dash(
  uri: 'https://example.com/manifest.mpd',
  headers: <String, String>{
    'Referer': 'https://example.com/player',
    'User-Agent': 'VesperPlayer',
  },
)
VesperPlayerSource.local(uri: '/storage/emulated/0/Movies/video.mp4')
VesperPlayerSource.remote(uri: 'https://example.com/video.mp4')
```

### Snapshot Listenable

`VesperPlayerController` also exposes `snapshotListenable`, a `ValueNotifier<VesperPlayerSnapshot>`
you can pass directly to `ValueListenableBuilder` for granular widget rebuilds without subscribing
to the `snapshots` stream:

```dart
ValueListenableBuilder<VesperPlayerSnapshot>(
  valueListenable: controller.snapshotListenable,
  builder: (context, snapshot, _) => Text('${snapshot.timeline.positionMs} ms'),
)
```

### Preload Budget

`VesperPreloadBudgetPolicy` can be supplied at controller creation to cap preload concurrency,
memory, disk, and warm-up window:

```dart
final controller = await VesperPlayerController.create(
  preloadBudgetPolicy: const VesperPreloadBudgetPolicy(
    maxConcurrentTasks: 2,
    maxMemoryBytes: 64 * 1024 * 1024,
    warmupWindowMs: 8000,
  ),
);
```

### Benchmark Configuration

`VesperBenchmarkConfiguration` can be supplied at controller creation when you
need native host-kit benchmark events during profiling:

```dart
final controller = await VesperPlayerController.create(
  benchmarkConfiguration: const VesperBenchmarkConfiguration(
    enabled: true,
    includeRawEvents: true,
    maxBufferedEvents: 2048,
    consoleLogging: true,
  ),
);
```

`enabled` turns on benchmark capture. `consoleLogging` is separate and remains
off by default; keep it disabled in normal app builds unless you are actively
tracing startup or playback behavior.

## Track Selection And ABR

```dart
final catalog = controller.snapshot.trackCatalog;
final audioTracks = catalog.audioTracks;
final videoTracks = catalog.videoTracks;

await controller.setAudioTrackSelection(
  VesperTrackSelection.track(audioTracks.first.id),
);

await controller.setAudioTrackSelection(const VesperTrackSelection.auto());
await controller.setSubtitleTrackSelection(
  const VesperTrackSelection.disabled(),
);

await controller.setAbrPolicy(
  const VesperAbrPolicy.constrained(maxHeight: 720),
);

await controller.setAbrPolicy(
  VesperAbrPolicy.fixedTrack(videoTracks.last.id),
);
```

On iOS, `VesperAbrPolicy.fixedTrack(...)` is implemented as best-effort HLS
variant pinning on iOS 15+, not exact AVPlayer video-track switching. Single-
axis constraints such as `VesperAbrPolicy.constrained(maxHeight: 720)` are also
supported on iOS HLS, but they are restored only after the current variant
catalog is ready so the missing dimension can be inferred safely. Check
`supportsAbrFixedTrack` and `supportsVideoTrackSelection` before exposing that
control in product UI.

Android and iOS both surface the currently active adaptive variant through
`controller.snapshot.effectiveVideoTrackId`. Flutter UI can combine that with
`trackCatalog.videoTracks` to show the actual quality currently in use during
`auto` or constrained ABR.

Both mobile backends also surface `controller.snapshot.videoVariantObservation`
when they have direct runtime evidence for the currently rendered adaptive
variant. On Android that is derived from ExoPlayer's active `videoFormat`; on
iOS it is derived from AVPlayer access-log bitrate plus presentation size.
Flutter UI can use this signal to explain what the player is currently
rendering even when a stable `effectiveVideoTrackId` is not available yet.

On iOS, `controller.snapshot.fixedTrackStatus` provides an explicit runtime
signal for best-effort `fixedTrack` convergence:

- `pending`: the host is still waiting for enough runtime evidence to identify the active variant
- `locked`: the observed variant has remained on the requested fixed-track target long enough to
  be treated as stable
- `fallback`: sustained runtime evidence shows that the player is still rendering a different
  variant than the requested target

When `fixedTrackStatus` is not available on a backend, Flutter UI can still
fall back to comparing the requested `trackId` with `effectiveVideoTrackId`,
but new platform implementations should prefer surfacing the explicit status.

On iOS, a restored `fixedTrack` request that keeps rendering a different
variant after sustained runtime observation is now treated as a non-fatal
convergence failure. The host surfaces that through `controller.snapshot.lastError`
and, for restore flows, automatically falls back to constrained ABR using the
requested variant limits when possible, otherwise back to automatic ABR.

## Live And DVR

```dart
final timeline = controller.snapshot.timeline;

if (timeline.kind == VesperTimelineKind.liveDvr) {
  final seekableRange = timeline.seekableRange!;
  print('Seekable range: ${seekableRange.startMs}ms ~ ${seekableRange.endMs}ms');
  print('Live offset: ${timeline.liveOffsetMs}ms');

  await controller.seekToLiveEdge();

  if (timeline.isAtLiveEdge()) {
    print('Playback is currently at the live edge.');
  }
}
```

## Resilience Policy

Use `VesperPlaybackResiliencePolicy` to tune buffering, retry, and cache
behavior.

```dart
final controller = await VesperPlayerController.create(
  resiliencePolicy: const VesperPlaybackResiliencePolicy.resilient(),
);

final policy = VesperPlaybackResiliencePolicy(
  buffering: const VesperBufferingPolicy.streaming(),
  retry: const VesperRetryPolicy(
    maxAttempts: 5,
    backoff: VesperRetryBackoff.exponential,
    baseDelayMs: 500,
    maxDelayMs: 8000,
  ),
  cache: const VesperCachePolicy.resilient(),
);

await controller.setPlaybackResiliencePolicy(policy);

final effectivePolicy = controller.snapshot.resiliencePolicy;
print('Active buffering preset: ${effectivePolicy.buffering.preset}');
```

Built-in presets:

| Preset         | Buffering       | Retry                  | Recommended for           |
| -------------- | --------------- | ---------------------- | ------------------------- |
| `default`      | default         | default                | General use               |
| `balanced()`   | balanced        | linear backoff         | Stable networks           |
| `streaming()`  | streaming-first | aggressive retries     | Continuous streaming      |
| `resilient()`  | larger buffers  | exponential backoff x6 | Weak networks             |
| `lowLatency()` | low latency     | fail fast              | Low-latency live playback |

## Download Management

`VesperDownloadManager` manages local downloads, pause and resume, startup task
restore, resumable partial transfers, and progress tracking.

```dart
final manager = await VesperDownloadManager.create();

final taskId = await manager.createTask(
  assetId: 'my-video-01',
  source: VesperDownloadSource.fromSource(
    source: VesperPlayerSource.hls(uri: 'https://example.com/stream.m3u8'),
  ),
  profile: const VesperDownloadProfile(
    preferredAudioLanguage: 'en',
    allowMeteredNetwork: false,
  ),
);

manager.snapshots.listen((snapshot) {
  for (final task in snapshot.tasks) {
    final ratio = task.progress.completionRatio;
    print('Task ${task.taskId}: ${(ratio! * 100).toInt()}% state=${task.state}');
  }
});

await manager.pauseTask(taskId!);
await manager.resumeTask(taskId);
await manager.removeTask(taskId);
await manager.dispose();
```

### Mobile prepare-phase download flow

For remote VOD `HLS`, static `DASH`, and `FLV` downloads, the Android and iOS
host kits now run a native prepare phase before transfer starts. Flutter apps
can pass the entry-point source into `createTask(...)` with an empty
`VesperDownloadAssetIndex`; the host kit expands manifests, resolves byte
ranges, probes every remote byte total, writes local rewritten manifests or
concat lists, and then emits `VesperDownloadTaskUpdatedEvent` with a full task
before the first progress patch.

Recommended host flow:

1. Insert a temporary "preparing" row in the app UI as soon as the user taps download.
2. Call `createTask(...)` with `VesperDownloadProfile(targetDirectory: ...)`.
   Set `targetOutputFormat: VesperDownloadOutputFormat.mp4` for HLS, DASH, and
   FLV segmented sources when the desired completed artifact is MP4.
3. Replace the temporary row with the real task and listen to
   `manager.snapshots`; the snapshot is updated from `taskCreated`,
   `taskUpdated`, and `taskRemoved` events, so total bytes and segment counts
   appear before transfer progress.

Hosts may still pass a prebuilt `VesperDownloadAssetIndex` for custom catalogs.
In that case the native prepare phase completes missing resource sizes before
download. Pause, resume, and remove operations should be keyed by `taskId`, not
by URL.

Headers on `VesperPlayerSource.headers` are forwarded by the Android and iOS
host kits during download preparation and transfer. Use them for generic HTTP
context such as `User-Agent`, `Referer`, `Origin`, `Cookie`, or authorization
headers; the SDK applies them to manifest reads, size probes, and media
transfers, and ignores empty header names or blank values.

The default `VesperDownloadConfiguration` enables `restoreTasksOnStartup` and
`resumePartialDownloads`. Android and iOS persist task snapshots under the
download base directory, restore interrupted preparing/downloading tasks on the
next manager creation, and resume existing partial remote files with range
requests when the server supports them. Complete resources stream by default,
`Range: bytes=<existing>-` is used for resume, and fixed Range chunks are used
only when `rangeChunkBytes` is configured. If a server ignores a resume range,
only that partial resource is deleted and restarted from byte zero; expired or
unavailable URLs fail with a stale-resource error. This is SDK-managed
foreground download recovery; OS-managed process-death background transfer is
not enabled by default and remains a separate host opt-in design.

On iOS, offline media URLs must be HTTPS because the SDK does not relax App
Transport Security for `http://` resources. On Android, downloads are stored
under the app-private files directory by default. Use `shareTaskOutput(...)` for
the native share sheet or Android `FileProvider`, and `saveTaskOutput(...)` for
the iOS document export flow or Android 10+ MediaStore `Downloads` / `Movies`.
On Android 9 and older, use `shareTaskOutput(...)`/FileProvider or a host-owned
export flow because the SDK does not request legacy public storage permissions.

### Optional `.mp4` export through `player-remux-ffmpeg`

`player-remux-ffmpeg` is an optional dynamic plugin that remuxes downloaded HLS,
DASH, or FLV assets into `.mp4`. Android hosts must package the shared
`vesper-player-kit-ffmpeg-runtime` AAR separately, and iOS hosts must embed the
shared `VesperPlayerFfmpegRuntime.xcframework.zip` alongside
`VesperPlayerRemuxFfmpegPlugin.xcframework.zip`. Export becomes available only
after the host app packages the runtime, packages the plugin library, and passes
the plugin absolute path through `VesperDownloadConfiguration.pluginLibraryPaths`.

```dart
final pluginLibraryPaths = <String>[
  '/absolute/path/to/libvesper_remux_ffmpeg.so',
  '/absolute/path/to/VesperPlayerRemuxFfmpegPlugin.framework/VesperPlayerRemuxFfmpegPlugin',
];

final manager = await VesperDownloadManager.create(
  configuration: VesperDownloadConfiguration(
    runPostProcessorsOnCompletion: false,
    pluginLibraryPaths: pluginLibraryPaths,
  ),
);

manager.events.listen((event) {
  if (event is VesperDownloadExportProgressEvent) {
    print('task ${event.taskId}: ${(event.ratio * 100).toInt()}%');
  }
});

await manager.exportTaskOutput(taskId, '/path/to/output.mp4');
await manager.shareTaskOutput(taskId, fileName: 'movie.mp4', mimeType: 'video/mp4');
final savedUri = await manager.saveTaskOutput(
  taskId,
  fileName: 'movie.mp4',
  collection: VesperDownloadPublicCollection.movies,
);
```

Key points:

- `pluginLibraryPaths` must point to an already packaged and accessible
  Android `libvesper_remux_ffmpeg.so` or iOS remux plugin framework binary.
  Do not include the iOS shared FFmpeg runtime path in `pluginLibraryPaths`.
- `exportTaskOutput(...)` triggers the plugin and reports progress through
  `VesperDownloadExportProgressEvent`.
- The mobile examples in this repository already show the full host wiring.
  Android builds the plugin during Gradle `preBuild`; iOS can either use the
  Xcode embed script during local development or consume the optional
  `VesperPlayerFfmpegRuntime.xcframework.zip` and
  `VesperPlayerRemuxFfmpegPlugin.xcframework.zip` release artifacts.
- Depending on `vesper_player` alone does not pull FFmpeg into your app. That
  keeps app size stable when export is not needed.
- FFmpeg prebuilts are selected through `./scripts/vesper ffmpeg --platform
  android|ios --profile <name>`. The default mobile profiles stay local-only
  and validate that network and OpenSSL remain disabled.
- If the host bundles the remux plugin, treat it as an FFmpeg redistribution:
  include FFmpeg license text and notices, provide corresponding FFmpeg source
  and configure flags, preserve LGPL relinking rights, and track OpenSSL /
  libxml2 notices when those libraries are included. See
  [THIRD_PARTY_NOTICES.md](../../../THIRD_PARTY_NOTICES.md).

### Optional mobile plugin diagnostics

`VesperPlayerController.create(...)` accepts two optional mobile plugin
configurations:

- `sourceNormalizerConfiguration` with `disabled`, `diagnosticsOnly`,
  `preflightOnly`, `preferNormalized`, and `requireNormalized` modes
- `frameProcessorConfiguration` with `disabled` and `diagnosticsOnly` modes
- `nativeFramePipelineConfiguration` with `disabled`, `diagnosticsOnly`,
  `preferNativeFrame`, and `requireNativeFrame` modes

Both are disabled by default. `pluginLibraryPaths` must contain plugin binary
paths only: Android `.so` paths or iOS plugin framework binary paths. The
Android FFmpeg runtime AAR and iOS `VesperPlayerFfmpegRuntime.xcframework.zip`
are package dependencies and should not be placed in `pluginLibraryPaths`.

SourceNormalizer mobile can load the optional FFmpeg plugin, report capability
diagnostics in `controller.pluginDiagnostics`, and in `preflightOnly` mode
attempt an open/close packet-session check for the selected source.
`preferNormalized` and `requireNormalized` are opt-in host-kit paths that may
replace the platform source with a disk-backed fMP4 or short-window HLS
resource. `preferNormalized` falls back to the original source when
normalization fails; `requireNormalized` reports a source error. Standard HLS
and DASH stay native-first by default unless normalization is explicitly
required or forced by a test profile. The repository smoke expectations live in
`fixtures/media/source-normalizer-smoke-matrix.json`.

FrameProcessor mobile remains opt-in and is only consumed by the explicit
Native Frame Pipeline route. The host kits now report native-frame diagnostics
for SourceNormalizer packet input, platform decoder adapter, presenter profile,
fallback reason, and counters. iOS local/VOD native-frame playback can run
through VideoToolbox, MetalLayer presentation, and the Swift native audio
bridge. Android still reports the planned MediaCodec/SurfaceView route and
falls back until that packet lane is wired. `requireNativeFrame` reports a
capability error when the requested native-frame lane is unavailable instead of
silently using the system player. Default mobile playback is unchanged.

Download task states:

```text
queued -> preparing -> downloading -> completed
                  \-> paused ->/
                  \-> failed
                  \-> removed
```

## Capability Discovery

Platform and backend support is reported through `VesperPlayerCapabilities`, so
apps can guard unsupported features without relying on exception handling.

```dart
final caps = controller.snapshot.capabilities;

if (caps.supportsDash) {
  // DASH is available on the current backend.
}

if (caps.supportsTrackSelection) {
  // Track selection is supported.
}

if (caps.supportsAbrFixedTrack) {
  // Fixed-track ABR pinning is available on this backend.
  // On iOS this is best-effort variant pinning, not exact track switching.
}

if (caps.isExperimental) {
  // The current backend is still experimental.
}
```

## Related Packages

| Package                            | Description                               |
| ---------------------------------- | ----------------------------------------- |
| `vesper_player_platform_interface` | Shared platform contract and DTOs         |
| `vesper_player_android`            | Android implementation built on ExoPlayer |
| `vesper_player_ios`                | iOS implementation built on AVPlayer      |
