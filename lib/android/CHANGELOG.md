# Changelog

## 0.2.0 - 2026-05-13

### Breaking Changes

- JNI, bridge, and `Native*` payload types are internal implementation details.
  Host apps should use `VesperPlayerController`, `VesperPlayerSource`,
  `VesperTrackSelection`, `VesperVideoSurfaceKind`, and the download/preload
  facades.
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
