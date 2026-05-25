# Changelog

## 0.3.0 - 2026-05-18

### Added

- Added release staging for the optional
  `VesperPlayerFfmpegRuntime.xcframework.zip` and
  `VesperPlayerRemuxFfmpegPlugin.xcframework.zip` artifacts.

### Changed

- The core `VesperPlayerKit.xcframework` remains FFmpeg-free; FFmpeg-backed
  remux support is distributed as separate signable runtime and plugin
  XCFrameworks.

## 0.2.0 - 2026-05-13

### Breaking Changes

- `player-ffi-ios` now reports the same error code and category taxonomy as the
  desktop FFI. Regenerate any downstream native bindings before integrating this
  release.
- `AVAudioSession` activation is shared across Vesper controllers. Disposing one
  controller no longer deactivates the process audio session while another
  Vesper owner is active.
- System audio interruptions and route changes are now reflected in
  `PlayerHostUiState.isInterrupted`; hosts should treat that field as the source
  of truth for interruption UI.
