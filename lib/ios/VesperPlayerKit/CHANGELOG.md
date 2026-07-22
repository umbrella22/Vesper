# Changelog

## Unreleased

### Added

- Added bounded side-loaded SRT, WebVTT, and SSA/ASS parsing with UTF-8, 2 MiB,
  eight-track, and 10,000-cue limits, plus a native subtitle overlay integrated
  with track selection.
- Added `VesperSubtitleStyle` visibility and font scaling for side-loaded and
  embedded AVPlayer subtitles.
- Added tagged-release staging for the seven optional sibling XCFrameworks.
  FFmpeg-backed artifacts are now gated on a compliance archive and the exact
  corresponding versioned FFmpeg source archive in the same release, matched to
  the SHA-256 recorded when each FFmpeg slice was built.

### Changed

- RTMP, RTSP, and HTTP-FLV direct playback fail with explicit capability errors.
- HTTP `.flv` URLs remain progressive unless the protocol is set explicitly.
- Bundled SourceNormalizer discovery now resolves its signed sibling framework
  executable. The flat-dylib compatibility path was removed.

### Fixed

- Release framework archives now hide the static BridgeShim module from public
  and private textual interfaces, share the canonical XCFramework slices, omit
  AppleDouble metadata, and pass an isolated import and link smoke.
- Optional-plugin release verification now rejects undeclared Mach-O dynamic
  dependencies, extra archives and XCFramework slices, empty or altered FFmpeg
  compliance files, cross-slice metadata mismatches, and retired assets left by
  a same-tag release rerun. FFmpeg release staging always rebuilds from source.

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
