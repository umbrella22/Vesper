# Changelog

## 0.4.2 - 2026-08-16

### Fixed

- Stable Maven Central publication now includes
  `vesper-player-kit-external-playback` and its same-version
  `vesper-player-kit-ffmpeg-runtime` dependency instead of leaving the public
  external-playback coordinate unavailable.
- Maven staging now validates the complete internal dependency graph, uses
  LGPL metadata for the FFmpeg runtime POM, and builds a hosted Android
  application against the published external-playback coordinate.

SourceNormalizer, offline remux, Decoder, and FrameProcessor artifacts remain
opt-in and are not part of this default Maven dependency closure.

## 0.4.1 - 2026-08-14

### Breaking Changes

- Maven coordinates now use `io.github.umbrella22.vesper`, and all Kotlin,
  manifest, and JNI package identities moved from `io.github.ikaros` to
  `io.github.umbrella22`. There are no package aliases; update imports and
  manifest class names and rebuild `libvesper_player_android.so` with the host
  kit because JNI entry points encode the Kotlin package name.
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
- Added per-track support status and bounded diagnostics, catalog revisions, and
  playback-path identifiers to the native track catalog.
- Added structured fixed-track rejection errors with optional expected catalog
  revision evidence.

### Changed

- Android host modules now build with Kotlin 2.2.10, matching the compiler
  embedded in Android Gradle Plugin 9.1, plus Media3 1.11.0,
  kotlinx.coroutines 1.11.0, AndroidX AppCompat 1.8.0, Lifecycle 2.10.0,
  Compose BOM 2026.06.01, and OkHttp 5.4.0.
- HTTP `.flv` URLs infer progressive playback; use `VesperPlayerSource.flvLive`
  when the source is explicitly an HTTP-FLV live stream.
- Fixed-track requests revalidate the current Media3 track support before
  creating an override and leave playback state unchanged when rejected.
- Only fixed-track rejections use `VesperFixedTrackSelectionException`;
  generic ABR command failures retain their runtime error taxonomy.
- Subtitle selection waits for the exact stable-id TEXT track and confirms the
  applied selection within one bounded deadline.

### Fixed

- Instrumentation JNI libraries now stage under the host-kit project directory
  even when a Flutter consumer redirects Gradle build outputs, keeping the Rust
  CLI's module-owned publication boundary intact.
- External-playback theme attributes that require API 29 are now isolated in
  `values-v29`, so resource validation remains compatible with the API 26 floor.
- Pausing during an in-flight source load now cancels pending autoplay.
- Async source and seek commands now complete only after Media3 publishes
  command readiness or seek completion for the current source generation;
  superseded commands return structured obsolete failures without changing the
  active `lastError`.
- Delayed Media3 TEXT-track visibility no longer produces an early
  `subtitle_track_not_found` result when the target becomes selectable within
  the command deadline.
- Fixed-track capability details are retained when native errors cross the
  JNI and host-kit boundaries.

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
