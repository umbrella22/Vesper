# Changelog

## Unreleased

### Breaking Changes

- The Android Flutter implementation now requires Flutter 3.44.0 or newer.
- `renderSurfaceKind: auto` now selects `SurfaceView`. Pass `textureView`
  explicitly for overlay-heavy, scrolling, clipping, rounded-corner, or
  animation-heavy screens that need the previous composition behavior.

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
