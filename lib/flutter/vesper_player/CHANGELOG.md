# Changelog

## 0.4.2 - 2026-08-16

- Prepared package metadata for the 0.4.2 release.

## 0.4.1 - 2026-08-14

### Added

- Added controller-level subtitle styling through `setSubtitleStyle`.
- Added external subtitle source configurations and explicit RTMP, RTSP, and
  HTTP-FLV source factories from the shared platform interface.
- Added track support metadata, catalog revisions, and playback-path state to
  the public Flutter snapshot models.

### Breaking Changes

- Native plugin packages, channels, platform-view identifiers, and first-party
  plugin references now use the `io.github.umbrella22` reverse-DNS root instead
  of `io.github.ikaros`. Flutter auto-registration needs no application code,
  but custom channel integrations and stored plugin references must be updated.
- The Flutter package family now requires Flutter 3.44.0 or newer.
- External subtitle declarations now use `VesperExternalSubtitleSource` and
  `VesperPlayerSource.externalSubtitles`; the old names are deprecated aliases.
- Android `renderSurfaceKind: auto` now maps to `SurfaceView`; hosts that need
  the previous overlay-oriented path should pass `textureView` explicitly.
- `VesperPlayerController.requestPictureInPicture` now treats its
  configuration as an optional per-request override instead of always sending
  default values.

### Changed

- Flutter UI package imports are prepared for the official `material_ui`
  package split where SDK UI widgets use Material components.
- Picture in Picture request and exit failures are surfaced through PiP events
  and thrown platform errors without mutating the player snapshot error state.
- `setAbrPolicy` accepts an optional expected catalog revision and surfaces
  fixed-track capability failures as typed platform exceptions.
- `setSubtitleTrackSelection` now awaits native confirmation and throws
  `VesperSubtitleException` for structured subtitle failures. Snapshots expose
  requested, confirmed, and effective subtitle selection separately.

### Fixed

- Mobile source and seek futures now complete after native command readiness or
  seek confirmation. Obsolete source and seek failures reject only their
  originating future and no longer publish a synthetic player error.

## 0.3.0 - 2026-05-18

### Changed

- Android external playback now uses the consolidated
  `vesper-player-kit-external-playback` facade under the existing Dart API.
- iOS FFmpeg remux support is documented as an optional plugin XCFramework
  instead of part of the core iOS host kit.

## 0.2.0 - 2026-05-13

### Breaking Changes

- Optional external-playback DTOs are sourced from
  `vesper_player_platform_interface`. Import them from `vesper_player` or the
  platform-interface package instead of `vesper_player_external_playback`.
- Android local-network DLNA / relay playback no longer inherits cleartext HTTP
  permission from the SDK manifest. Host apps that relay `http://` LAN URLs must
  configure their own manifest or network security policy.
- Flutter UI defaults are English. Host apps that need localized stage text
  should pass `VesperPlayerStageStrings` to `VesperPlayerStage` or provide their
  own controls around the shared controller contracts.
