# Changelog

## 0.4.3-rc.1 - 2026-08-17

- Prepared package metadata for the 0.4.3-rc.1 release.

## 0.4.2 - 2026-08-16

- Prepared package metadata for the 0.4.2 release.

## 0.4.1 - 2026-08-14

### Added

- Added `VesperSubtitleStyle`, `VesperExternalSubtitleSource`, canonical
  subtitle catalog/selection state, requested/confirmed/effective selection,
  `VesperSubtitleException`, and the `setSubtitleStyle` platform contract.
- Added `VesperTrackSupport`, catalog revision and playback-path fields, raw
  unknown-value preservation, and `VesperFixedTrackSelectionException`.
- Added RTMP, RTSP, and HTTP-FLV source protocol values. HTTP `.flv` inference
  remains progressive; explicit live sources use `VesperPlayerSource.flvLive`.
- Added `VesperPlayerCommandException` for structured mobile source and seek
  failures, including preserved native details and obsolete-command detection.

### Breaking Changes

- The shared platform interface now requires Flutter 3.44.0 or newer.
- Android platform implementations should treat `renderSurfaceKind: auto` as
  `SurfaceView` and reserve `textureView` for explicit compatibility opt-in.
- `requestPictureInPicture` now accepts a nullable configuration so a request
  without overrides preserves the previously applied Picture in Picture
  configuration.
- `VesperPlayerSource.externalSubtitles` is the canonical external subtitle
  field. `VesperSubtitleSideLoad` and `subtitleConfigurations` remain deprecated
  migration aliases.

### Added

- Added shared Picture in Picture configuration, availability, error, and event
  DTOs for system-player PiP integrations.

### Changed

- `setAbrPolicy` carries an optional expected catalog revision; fixed-track
  capability failures decode to a stable typed exception without treating
  missing support evidence as confirmed unsupported.
- Mobile source and seek platform methods now define native completion
  semantics. Obsolete commands fail only their originating future and must not
  publish a terminal error for the active generation.

## 0.3.0 - 2026-05-18

- Prepared package metadata for the 0.3.0 release.

## 0.2.0 - 2026-05-13

### Breaking Changes

- This package is now the single source for optional external-playback DTOs:
  `VesperExternalPlaybackRoute`, `VesperExternalPlaybackMediaItem`,
  `VesperExternalPlaybackResult`, and `VesperExternalPlaybackSessionEvent`.
  Platform packages must not publish duplicate public DTO definitions for those
  contracts.
