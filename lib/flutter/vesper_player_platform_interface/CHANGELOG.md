# Changelog

## Unreleased

### Breaking Changes

- The shared platform interface now requires Flutter 3.44.0 or newer.
- Android platform implementations should treat `renderSurfaceKind: auto` as
  `SurfaceView` and reserve `textureView` for explicit compatibility opt-in.

## 0.3.0 - 2026-05-18

- Prepared package metadata for the 0.3.0 release.

## 0.2.0 - 2026-05-13

### Breaking Changes

- This package is now the single source for optional external-playback DTOs:
  `VesperExternalPlaybackRoute`, `VesperExternalPlaybackMediaItem`,
  `VesperExternalPlaybackResult`, and `VesperExternalPlaybackSessionEvent`.
  Platform packages must not publish duplicate public DTO definitions for those
  contracts.
