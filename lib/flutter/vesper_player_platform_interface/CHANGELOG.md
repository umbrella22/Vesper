# Changelog

## 0.3.0 - 2026-05-18

- Prepared package metadata for the 0.3.0 release.

## 0.2.0 - 2026-05-13

### Breaking Changes

- This package is now the single source for optional external-playback DTOs:
  `VesperExternalPlaybackRoute`, `VesperExternalPlaybackMediaItem`,
  `VesperExternalPlaybackResult`, and `VesperExternalPlaybackSessionEvent`.
  Platform packages must not publish duplicate public DTO definitions for those
  contracts.
