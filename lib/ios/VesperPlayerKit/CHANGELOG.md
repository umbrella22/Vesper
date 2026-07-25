# Changelog

## 0.4.0 - Unreleased

### Breaking Changes

- `VesperPlayerController.setSubtitleTrackSelection` is now `async throws` and
  completes only after AVPlayer or the external overlay confirms convergence.
- External subtitle declarations now use `VesperExternalSubtitleSource` and
  `VesperPlayerSource.externalSubtitles`; the previous names are deprecated
  aliases.

### Added

- Added bounded side-loaded SRT, WebVTT, and SSA/ASS parsing with UTF-8, 2 MiB,
  eight-track, and 10,000-cue limits, plus a native subtitle overlay integrated
  with track selection.
- Added `VesperSubtitleStyle` visibility and font scaling for side-loaded and
  embedded AVPlayer subtitles.
- Added canonical catalog/selection subtitle state, requested/confirmed/effective
  selection, structured command errors, and source/item epoch fencing.
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

- Playback diagnostics now remove credentials, query parameters, and fragments
  from current-source details, lifecycle and retry logs, HDR evidence, DASH
  network errors, and AVPlayer error-log URLs.
- Foreground-download errors now redact diagnostic URLs while retaining complete
  stale-resource URIs for recovery callbacks and retried requests.
- Release framework archives now hide the static BridgeShim module from public
  and private textual interfaces, share the canonical XCFramework slices, omit
  AppleDouble metadata, and pass an isolated import and link smoke.
- Optional-plugin release verification now rejects undeclared Mach-O dynamic
  dependencies, extra archives and XCFramework slices, empty or altered FFmpeg
  compliance files, cross-slice metadata mismatches, and retired assets left by
  a same-tag release rerun. FFmpeg release staging always rebuilds from source.
- Local audio-only playback no longer waits forever for an AVPlayerLayer video
  frame before starting.
- Explicit automatic subtitle selection can use a default track even when the
  startup policy leaves subtitles disabled by default.

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
