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
| `VesperDownloadProgressSnapshot` | Byte, segment, and ratio-based progress                                      |
| `VesperDownloadError`            | Download-specific error model                                                |

### Download events

| Event type                    | Emitted when                     |
| ----------------------------- | -------------------------------- |
| `VesperDownloadSnapshotEvent` | Download state changes           |
| `VesperDownloadErrorEvent`    | A download error occurs          |
| `VesperDownloadDisposedEvent` | The download manager is disposed |

### Enums

```dart
VesperPlayerSourceKind
VesperPlayerSourceProtocol
VesperPlaybackState
VesperTimelineKind
VesperPlayerBackendFamily
VesperPlayerRenderSurfaceKind
VesperMediaTrackKind
VesperTrackSelectionMode
VesperAbrMode
VesperFixedTrackStatus
VesperBufferingPreset
VesperRetryBackoff
VesperCachePreset
VesperPlayerErrorCategory
VesperViewportHintKind
VesperDownloadContentFormat
VesperDownloadState
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

Methods that remain unimplemented should report
`VesperPlayerError.unsupported()`. That keeps capability checks explicit and
lets apps branch on `VesperPlayerCapabilities` instead of depending on
exceptions.

Snapshot payloads should also round-trip the backend's current
`VesperPlaybackResiliencePolicy`, `VesperTrackSelectionSnapshot`, and
best-effort `effectiveVideoTrackId`, plus raw `videoVariantObservation`
evidence when the backend can expose bitrate and rendered size directly.
Backends should also provide `fixedTrackStatus` when they can observe
fixed-track convergence directly, so Flutter UI can render the effective
runtime state instead of only optimistic local intent.

`createPlayer` also accepts `renderSurfaceKind` and `benchmarkConfiguration`.
Android platform packages should map `auto` to a Flutter-overlay-safe default
surface and allow explicit `surfaceView` opt-in. Native implementations should
forward benchmark settings to the host kit and keep `consoleLogging` disabled by
default.

Coarse capability fields such as `supportsTrackSelection` or
`supportsAbrPolicy` should not be treated as implicit support for every
fine-grained mode. Platform plugins should populate fields like
`supportsVideoTrackSelection`, `supportsAbrFixedTrack`, and
`supportsAbrMaxResolution` explicitly.

## Related Packages

- `vesper_player`
- `vesper_player_android`
- `vesper_player_ios`
