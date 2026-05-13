# Changelog

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
