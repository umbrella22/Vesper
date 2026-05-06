# vesper_player

A cross-platform Flutter video player built around native-first backends:

- Android uses ExoPlayer through the Vesper Android host kit
- iOS uses AVPlayer through the Vesper iOS host kit
- macOS is currently a package stub without a real playback backend

The package exposes one Dart API surface so host apps can keep playback, track
selection, resilience, download, preload, and benchmark capture flows aligned
across platforms.

## Platform Support

| Feature                  | Android                                      | iOS                                                 | macOS package        |
| ------------------------ | -------------------------------------------- | --------------------------------------------------- | -------------------- |
| Local files              | ✅                                           | ✅                                                  | ❌ Backend not wired |
| Progressive HTTP         | ✅                                           | ✅                                                  | ❌ Backend not wired |
| HLS                      | ✅                                           | ✅                                                  | ❌ Backend not wired |
| DASH                     | ✅                                           | ✅ DASH-to-HLS bridge for VOD / live fMP4           | ❌ Backend not wired |
| Live streams             | ✅                                           | ✅                                                  | ❌ Backend not wired |
| Live DVR                 | ✅                                           | ✅                                                  | ❌ Backend not wired |
| Track selection          | ✅                                           | ✅                                                  | ❌ Backend not wired |
| Adaptive bitrate (ABR)   | ✅                                           | ⚠️ Constrained + best-effort fixed-track on iOS 15+ | ❌ Backend not wired |
| Buffering / retry policy | ✅                                           | ✅                                                  | ❌ Backend not wired |
| Download management      | ✅                                           | ✅                                                  | ❌                   |
| Preload                  | ✅                                           | ✅                                                  | ❌                   |
| System playback controls | ✅ MediaSession notification + FGS           | ✅ Now Playing / RemoteCommand                      | ❌                   |
| External playback        | ✅ Optional `vesper_player_cast` package     | ✅ AirPlay route picker via `vesper_player_ui`      | ❌                   |

> `vesper_player_macos` exists as an experimental federated package stub. The
> main package currently registers Android and iOS implementations only.

## Installation

The Flutter packages are source-distributed from this repository and currently
set `publish_to: none`. In a host app, use path or git dependencies until the
package family is published:

```yaml
dependencies:
  vesper_player:
    path: path/to/rust-player-sdk/lib/flutter/vesper_player
  # Optional Android Cast sender integration.
  vesper_player_cast:
    path: path/to/rust-player-sdk/lib/flutter/vesper_player_cast
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
`TextureView` for Flutter overlay and gesture compatibility. Select
`surfaceView` explicitly for fullscreen HDR or high-frame-rate scenarios where
host UI composition allows it. iOS accepts the option for API symmetry but
always uses the platform's AVPlayer-backed surface.

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
`VesperAirPlayRouteButton` near your player controls:

```dart
VesperAirPlayRouteButton(controller: controller)
```

The button is backed by `AVRoutePickerView` and prioritizes video-capable
routes by default. Users can also continue to route from Control Center.
AirDrop is file sharing, not media playback routing.

For Android Cast, depend on the optional `vesper_player_cast` package. It keeps
Google Play Services and Cast Framework dependencies out of the default player
package:

```dart
final cast = VesperCastController();
final result = await cast.loadFromPlayer(
  player: controller,
  source: VesperPlayerSource.hls(
    uri: 'https://example.com/stream.m3u8',
    label: 'Sample video',
  ),
  metadata: const VesperSystemPlaybackMetadata(title: 'Sample video'),
);
```

Cast V2 supports remote `http` / `https` HLS, DASH, and progressive sources
with the default Google receiver. Local files, `content://` sources, DRM,
custom request headers, offline assets, and custom receiver flows are outside
the V2 scope and return an unsupported result without changing local playback.

### `VesperPlayerSource`

```dart
VesperPlayerSource.hls(uri: 'https://example.com/stream.m3u8')
VesperPlayerSource.dash(
  uri: 'https://example.com/manifest.mpd',
  headers: <String, String>{
    'Referer': 'https://www.bilibili.com/',
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

`VesperDownloadManager` manages local downloads, pause and resume, and progress
tracking.

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

### Recommended remote HLS / DASH download flow

If the download source is a remote `HLS` or `DASH` manifest, do not stop at
passing the manifest URL into `createTask(...)`. A better flow is:

1. Insert a temporary "preparing" task in the host UI as soon as the user
   taps download.
2. Resolve the remote manifest in the background and prebuild
   `VesperDownloadSource`, `VesperDownloadProfile(targetDirectory: ...)`, and
   `VesperDownloadAssetIndex(resources: ..., segments: ...)`.
3. Call `createTask(...)` only after the real asset plan is ready, then let
   the real task replace the placeholder entry.

Benefits:

- The user sees immediate feedback instead of waiting for manifest parsing.
- The download manager persists the real `resources + segments` plan, which is
  what later offline playback, `.mp4` export, and host-level regression checks
  actually need.

Notes:

- The current iOS example uses this planning flow for remote `HLS` only.
  Remote `DASH` playback is supported on iOS via an in-process DASH→HLS bridge
  in `lib/ios/VesperPlayerKit`, but the download / export planning flow has not
  been wired through that bridge yet.
- Pause, resume, and remove operations should be keyed by `taskId`, not by URL.

### Optional `.mp4` export through `player-remux-ffmpeg`

`player-remux-ffmpeg` is an optional dynamic plugin that remuxes downloaded HLS or
DASH assets into `.mp4`. The Flutter packages do not bundle it automatically.
Export becomes available only after the host app packages the plugin library
and passes its absolute path through
`VesperDownloadConfiguration.pluginLibraryPaths`.

```dart
final pluginLibraryPaths = <String>[
  '/absolute/path/to/libplayer_remux_ffmpeg.so',
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
```

Key points:

- `pluginLibraryPaths` must point to an already packaged and accessible
  `libplayer_remux_ffmpeg.so` or `libplayer_remux_ffmpeg.dylib`.
- `exportTaskOutput(...)` triggers the plugin and reports progress through
  `VesperDownloadExportProgressEvent`.
- The mobile examples in this repository already show the full host wiring:
  Android builds the plugin during Gradle `preBuild`, and iOS embeds a signed
  dylib through an Xcode build phase.
- Depending on `vesper_player` alone does not pull FFmpeg into your app. That
  keeps app size stable when export is not needed.
- FFmpeg prebuilt support is still coarse-grained. The current scripts support
  on-demand builds and environment-level feature gates such as disabling DASH,
  but not fine-grained whitelisting by demuxer, muxer, protocol, or codec.
- If the host bundles the remux plugin, treat it as an FFmpeg redistribution:
  include FFmpeg license text and notices, provide corresponding FFmpeg source
  and configure flags, preserve LGPL relinking rights, and track OpenSSL /
  libxml2 notices when those libraries are included. See
  [THIRD_PARTY_NOTICES.md](../../../THIRD_PARTY_NOTICES.md).

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
| `vesper_player_macos`              | Experimental macOS package stub           |
