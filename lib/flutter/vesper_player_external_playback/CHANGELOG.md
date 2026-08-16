# Changelog

## 0.4.2 - 2026-08-16

### Fixed

- The matching Android `vesper-player-kit-external-playback` coordinate and
  transitive FFmpeg runtime are now included in stable Maven Central
  publication, so a hosted Flutter application can resolve the package without
  a Vesper source checkout.

## 0.4.1 - 2026-08-14

### Breaking Changes

- The Android plugin package and all external-playback channels now use the
  `io.github.umbrella22` reverse-DNS root. No old channel aliases are
  registered.
- The external playback Flutter package now requires Flutter 3.44.0 or newer.

### Changed

- Material widgets are imported through the official `material_ui` package.
- The `material_ui` dependency now uses the stable 1.0 release.
- Android build tooling now uses Kotlin 2.4.10 and kotlinx.coroutines 1.11.0;
  the native external-playback host uses AndroidX AppCompat 1.8.0 and OkHttp
  5.4.0.

### Fixed

- Android resource merging no longer contributes an unused route-theme alias
  that could resolve through an inheritance cycle with the native host kit.

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
