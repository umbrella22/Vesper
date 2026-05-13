# Changelog

## 0.2.0 - 2026-05-13

### Breaking Changes

- The Android Flutter implementation no longer imports Android host-kit
  `Native*`, bridge, or JNI implementation types. Runtime snapshots read
  `backendFamily` from the public `VesperPlayerController.backendFamily` API.
- `renderSurfaceKind` is decoded to the public `VesperVideoSurfaceKind` facade.
  Host integrations that referenced Android internal surface types must switch
  to `VesperPlayerRenderSurfaceKind` on Dart and `VesperVideoSurfaceKind` on
  Android.
