# Changelog

## 0.4.0 - Unreleased

### Added

- Added MethodChannel mapping for external subtitle configurations and
  `setSubtitleStyle`, backed by the Android host-kit cue overlay.
- Added complete RTMP, RTSP, and HTTP-FLV protocol wire mapping.
- Added track support, catalog revision, playback-path, and structured
  fixed-track error mapping for the Android wire contract.

### Breaking Changes

- The Android Flutter implementation now requires Flutter 3.44.0 or newer.
- Subtitle MethodChannel calls now wait for the suspending Android host API and
  return `vesper_subtitle_error` details on failure.
- `renderSurfaceKind: auto` now selects `SurfaceView`. Pass `textureView`
  explicitly for overlay-heavy, scrolling, clipping, rounded-corner, or
  animation-heavy screens that need the previous composition behavior.

### Changed

- Picture in Picture failures now emit PiP-specific events and method errors
  without publishing generic player errors.
- `setAbrPolicy` now forwards the optional expected catalog revision and keeps
  native fixed-track capability details in the returned platform error without
  misclassifying generic ABR command failures as fixed-track rejections.
- Android PiP state now follows Activity mode-change callbacks, with
  best-effort foreground restore for `exitPictureInPicture`.
- Subtitle snapshots now carry canonical catalog/selection state and
  requested/confirmed/effective selection fields.

## 0.3.0 - 2026-05-18

### Changed

- Optional Android external playback is now provided by
  `vesper-player-kit-external-playback` instead of separate Cast, DLNA, and
  relay host-kit modules.

## 0.2.0 - 2026-05-13

### Breaking Changes

- The Android Flutter implementation no longer imports Android host-kit
  `Native*`, bridge, or JNI implementation types. Runtime snapshots read
  `backendFamily` from the public `VesperPlayerController.backendFamily` API.
- `renderSurfaceKind` is decoded to the public `VesperVideoSurfaceKind` facade.
  Host integrations that referenced Android internal surface types must switch
  to `VesperPlayerRenderSurfaceKind` on Dart and `VesperVideoSurfaceKind` on
  Android.
