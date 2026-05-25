# vesper_player_platform_interface

The shared platform interface for `vesper_player`.

This package defines the cross-platform abstractions, DTOs, and event contracts
used by the federated Flutter plugin. It is intended for platform plugin
authors. Application code should usually depend on `vesper_player` directly.

## What This Package Contains

### Platform abstraction

- `VesperPlayerPlatform`: the abstract base class every platform package must extend
- `VesperPlatformCreateResult`: the result type returned by `createPlayer`
- `VesperBenchmarkConfiguration`: opt-in benchmark capture and console logging settings forwarded by `createPlayer`
- `VesperPlayerRenderSurfaceKind`: Flutter-facing Android render surface preference forwarded by `createPlayer`
- `VesperSourceNormalizerConfiguration`: optional mobile plugin diagnostics / source preflight configuration, disabled by default
- `VesperFrameProcessorConfiguration`: optional mobile FrameProcessor capability diagnostics configuration, disabled by default
- `VesperSystemPlaybackConfiguration`: optional system media session and background audio integration
- `VesperExternalPlaybackAvailability`: AirPlay / Cast route availability DTOs for optional UI and platform packages
- External playback DTOs used by optional Cast / DLNA packages, including route,
  media item, result, and session event models

### Player data models

| Type                             | Description                                                                                                                                                                                                    |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `VesperPlayerSource`             | Media source definition for local files, remote URLs, HLS, or DASH                                                                                                                                             |
| `VesperPlayerSnapshot`           | Full player state snapshot, including runtime capabilities, current track selection, the effective video variant, raw video-variant observation, fixed-track settling state, resilience policy, and last error |
| `VesperPlayerCapabilities`       | Capability set reported by the active backend, including fine-grained track-selection and ABR support                                                                                                          |
| `VesperTimeline`                 | Playback timeline for VOD, live, and live DVR                                                                                                                                                                  |
| `VesperSeekableRange`            | Seekable range, mainly for DVR windows                                                                                                                                                                         |
| `VesperTrackCatalog`             | Available video, audio, and subtitle tracks                                                                                                                                                                    |
| `VesperMediaTrack`               | Details for a single media track                                                                                                                                                                               |
| `VesperTrackSelection`           | Track selection command: auto, disabled, or explicit track                                                                                                                                                     |
| `VesperTrackSelectionSnapshot`   | Current track selection state                                                                                                                                                                                  |
| `VesperAbrPolicy`                | Adaptive bitrate policy: auto, constrained, or fixed track                                                                                                                                                     |
| `VesperTrackPreferencePolicy`    | Preferred languages and default track preferences                                                                                                                                                              |
| `VesperPlaybackResiliencePolicy` | Top-level buffering, retry, and cache policy                                                                                                                                                                   |
| `VesperBufferingPolicy`          | Buffering policy presets or explicit values                                                                                                                                                                    |
| `VesperRetryPolicy`              | Retry attempts, backoff mode, and delay limits                                                                                                                                                                 |
| `VesperCachePolicy`              | Memory and disk cache policy                                                                                                                                                                                   |
| `VesperPreloadBudgetPolicy`      | Preload budget for concurrency, memory, disk, and warm windows                                                                                                                                                 |
| `VesperBenchmarkConfiguration`   | Opt-in benchmark collection, raw-event buffering, and console logging settings                                                                                                                                |
| `VesperPlayerRenderSurfaceKind`  | Render surface preference: auto, texture view, or surface view                                                                                                                                                |
| `VesperSystemPlaybackConfiguration` | System media session configuration for background audio and platform controls                                                                                                                               |
| `VesperSystemPlaybackControls`   | Compact system media controls, defaulting to 10-second seek back, play-pause, and 10-second seek forward                                                                                                      |
| `VesperSystemPlaybackMetadata`   | Now Playing / notification metadata such as title, artist, artwork, content URI, duration, and live flag                                                                                                      |
| `VesperExternalPlaybackAvailability` | External route availability for AirPlay and Cast                                                                                                                                                          |
| `VesperExternalPlaybackRouteSnapshot` | Active external route identity and state                                                                                                                                                                  |
| `VesperExternalPlaybackRoute` | External route identity reported by optional Cast / DLNA plugins                                                                                                                                              |
| `VesperExternalPlaybackMediaItem` | Media item payload for optional external playback plugins                                                                                                                                                  |
| `VesperExternalPlaybackResult` | Result returned by external playback operations                                                                                                                                                               |
| `VesperExternalPlaybackSessionEvent` | Session event emitted by external playback plugins                                                                                                                                                      |
| `VesperRoutePickerConfiguration`  | Route picker preferences shared by optional UI packages                                                                                                                                                      |
| `VesperPlayerViewport`           | Normalized viewport rectangle used for viewport hints                                                                                                                                                          |
| `VesperViewportHint`             | Visibility hint: visible, near visible, prefetch only, or hidden                                                                                                                                               |
| `VesperPlayerError`              | Playback error with category and retryability metadata                                                                                                                                                         |

### Player events

| Event type                  | Emitted when            |
| --------------------------- | ----------------------- |
| `VesperPlayerSnapshotEvent` | Player state changes    |
| `VesperPlayerErrorEvent`    | A playback error occurs |
| `VesperPlayerDisposedEvent` | The player is disposed  |

### Download data models

| Type                             | Description                                                                  |
| -------------------------------- | ---------------------------------------------------------------------------- |
| `VesperDownloadConfiguration`    | Download manager configuration                                               |
| `VesperDownloadSource`           | Download source including content format                                     |
| `VesperDownloadProfile`          | Download preferences such as language, tracks, directory, and network limits |
| `VesperDownloadAssetIndex`       | Planned resources, segments, size, version, and checksum metadata            |
| `VesperDownloadTaskSnapshot`     | Snapshot for a single task                                                   |
| `VesperDownloadSnapshot`         | Aggregate snapshot for all tasks                                             |
| `VesperDownloadTaskStatePatch`   | Incremental state update for a single task                                   |
| `VesperDownloadTaskProgressPatch` | Incremental progress update for a single task                               |
| `VesperDownloadProgressSnapshot` | Byte, segment, and ratio-based progress                                      |
| `VesperDownloadError`            | Download-specific error model                                                |
| `VesperDownloadPublicCollection` | Public save target for supported platform export helpers                     |

### Download events

| Event type                            | Emitted when                                      |
| ------------------------------------- | ------------------------------------------------- |
| `VesperDownloadInitialSnapshotEvent`  | A manager starts or a platform forces full sync   |
| `VesperDownloadTaskCreatedEvent`      | A task is created                                 |
| `VesperDownloadTaskUpdatedEvent`      | A compact task, state, or progress update arrives |
| `VesperDownloadTaskRemovedEvent`      | A task is removed                                 |
| `VesperDownloadErrorEvent`            | A download error occurs                           |
| `VesperDownloadExportProgressEvent`   | A native export operation reports progress        |
| `VesperDownloadDisposedEvent`         | The download manager is disposed                  |

### Enums

```dart
VesperPlayerSourceKind
VesperPlayerSourceProtocol
VesperPlaybackState
VesperTimelineKind
VesperPlayerBackendFamily
VesperPlayerRenderSurfaceKind
VesperBackgroundPlaybackMode
VesperSystemPlaybackPermissionStatus
VesperExternalPlaybackRouteKind
VesperMediaTrackKind
VesperTrackSelectionMode
VesperAbrMode
VesperFixedTrackStatus
VesperBufferingPreset
VesperRetryBackoff
VesperCachePreset
VesperPlayerErrorCode
VesperPlayerErrorCategory
VesperViewportHintKind
VesperDownloadContentFormat
VesperDownloadOutputFormat
VesperDownloadState
VesperDownloadPublicCollection
```

## Implementing A New Platform Package

Extend `VesperPlayerPlatform` and register your implementation in
`registerWith()`:

```dart
class VesperPlayerMyPlatform extends VesperPlayerPlatform {
  static void registerWith() {
    VesperPlayerPlatform.instance = VesperPlayerMyPlatform();
  }

  @override
  Future<VesperPlatformCreateResult> createPlayer({...}) async {
    // Platform implementation.
  }

  // Implement the remaining abstract members here.
}
```

Methods that remain unimplemented should report `VesperPlayerError` with
`code: VesperPlayerErrorCode.unsupported` and
`category: VesperPlayerErrorCategory.capability`. That keeps capability checks
explicit and lets apps branch on `VesperPlayerCapabilities` instead of
depending on exceptions.

Snapshot payloads should also round-trip the backend's current
`VesperPlaybackResiliencePolicy`, `VesperTrackSelectionSnapshot`, and
best-effort `effectiveVideoTrackId`, plus raw `videoVariantObservation`
evidence when the backend can expose bitrate and rendered size directly.
Backends should also provide `fixedTrackStatus` when they can observe
fixed-track convergence directly, so Flutter UI can render the effective
runtime state instead of only optimistic local intent.

`createPlayer` also accepts `renderSurfaceKind` and `benchmarkConfiguration`.
Android platform packages should map `auto` to `SurfaceView` for Flutter 3.44+
native video playback and keep explicit `textureView` as the compatibility path
for overlay-heavy or animation-heavy host screens. Native implementations
should forward benchmark settings to the host kit and keep `consoleLogging`
disabled by default.

Mobile plugin configurations are intentionally conservative. SourceNormalizer
`diagnosticsOnly` and `preflightOnly` can report plugin diagnostics, but mobile
platforms must keep playback on the original platform source unless a later
contract explicitly adds playback substitution. FrameProcessor
`diagnosticsOnly` must report availability without opening frame sessions or
marking the plugin as participated.

Coarse capability fields such as `supportsTrackSelection` or
`supportsAbrPolicy` should not be treated as implicit support for every
fine-grained mode. Platform plugins should populate fields like
`supportsVideoTrackSelection`, `supportsAbrFixedTrack`, and
`supportsAbrMaxResolution` explicitly.

## Related Packages

- `vesper_player`
- `vesper_player_android`
- `vesper_player_ios`
