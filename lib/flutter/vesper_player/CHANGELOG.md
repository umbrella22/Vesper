# Changelog

## Unreleased

### Breaking Changes

- The Flutter package family now requires Flutter 3.44.0 or newer.
- Android `renderSurfaceKind: auto` now maps to `SurfaceView`; hosts that need
  the previous overlay-oriented path should pass `textureView` explicitly.

### Changed

- Flutter UI package imports are prepared for the official `material_ui`
  package split where SDK UI widgets use Material components.

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
