# Changelog

## 0.4.0 - Unreleased

### Breaking Changes

- `VesperPlayerController.setSubtitleTrackSelection` is now suspending and
  completes only after Media3 confirms the requested selection.
- External subtitle declarations now use `VesperExternalSubtitleSource` and
  `VesperPlayerSource.externalSubtitles`; the previous names are deprecated
  aliases.

### Added

- Added side-loaded SRT, WebVTT, and SSA/ASS subtitle configurations, Media3 cue
  rendering in the native surface host, and `VesperSubtitleStyle` visibility /
  font scaling.
- Added explicit RTMP, RTSP, and HTTP-FLV source protocol DTOs; RTMP remains an
  explicit unsupported operation in the stable host kit.
- Added canonical catalog/selection subtitle state, requested/confirmed/effective
  selection, structured errors, source/command generation fencing, and isolated
  per-subtitle request headers.

### Changed

- HTTP `.flv` URLs infer progressive playback; use `VesperPlayerSource.flvLive`
  when the source is explicitly an HTTP-FLV live stream.

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
