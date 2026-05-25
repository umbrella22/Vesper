# Changelog

## Unreleased

### Breaking Changes

- The external playback Flutter package now requires Flutter 3.44.0 or newer.

### Changed

- Material widgets are imported through the official `material_ui` package.

## 0.3.0 - 2026-05-18

### Changed

- Android now calls the consolidated
  `vesper-player-kit-external-playback` Kotlin facade while keeping the Dart API
  unchanged.
- The Android route button platform view now uses
  `VesperExternalRouteButton` from the external-playback AAR.

## 0.2.0 - 2026-05-13

### Breaking Changes

- External playback DTOs are now defined by
  `vesper_player_platform_interface`. Import
  `package:vesper_player/vesper_player.dart` or
  `package:vesper_player_platform_interface/vesper_player_platform_interface.dart`
  for `VesperExternalPlaybackRoute`, `VesperExternalPlaybackMediaItem`,
  `VesperExternalPlaybackResult`, and `VesperExternalPlaybackSessionEvent`.
- The external-playback package no longer owns duplicate public DTO
  definitions.
- The Android plugin manifest no longer enables app-wide cleartext traffic.
  Hosts that use DLNA discovery or local relay URLs must declare their own
  manifest or network-security cleartext policy.
