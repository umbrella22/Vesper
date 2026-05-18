# Changelog

## 0.3.0 - 2026-05-18

### Breaking Changes

- Cast, DLNA, relay, and relay FFmpeg modules were consolidated into
  `vesper-player-kit-external-playback`. Public APIs now live under
  `io.github.ikaros.vesper.player.android.external`.

### Added

- Added release AAR staging for `vesper-player-kit-compose-ui`,
  `vesper-player-kit-external-playback`, and
  `vesper-player-kit-ffmpeg-runtime`.
- Added `VesperExternalPlaybackController` with `StateFlow` routes and
  `SharedFlow` events for unified external playback integration.

## 0.2.0 - 2026-05-13

### Breaking Changes

- JNI, bridge, and `Native*` payload types are internal implementation details.
  Host apps should use `VesperPlayerController`, `VesperPlayerSource`,
  `VesperTrackSelection`, `VesperVideoSurfaceKind`, and the download/preload
  facades.
- `VesperPlayerController.backend` has been removed. Use
  `VesperPlayerController.backendFamily` and `VesperPlayerBackendFamily` when
  code needs to distinguish the Android host-kit backend from the fake preview
  backend.
- The Gradle `check` lifecycle now includes `checkPublicApiSurface`, which fails
  if bridge, JNI, or `Native*` implementation declarations are made public again.
- `NativeVideoSurfaceKind` was replaced by `VesperVideoSurfaceKind` in public
  controller and Compose factory APIs.
- The DLNA and relay AARs no longer enable global cleartext traffic. Hosts that
  relay local-network HTTP URLs must opt in explicitly in their app manifest or
  network security configuration.
- `vesper-player-kit-compose` no longer applies rounded corners, black
  backgrounds, or outlines to the player surface. Use
  `vesper-player-kit-compose-ui` or host-side Compose styling for visuals.

### Fixed

- DLNA discovery publishes route updates only while the current discovery
  generation is active. A device description fetch that completes after `stop()`
  or after a restart is ignored.
- The SSDP NOTIFY listener prefers port 1900 with address reuse enabled. If
  another process already owns that port, discovery records a diagnostic and
  continues with an ephemeral listener while active M-SEARCH polling remains
  available.
