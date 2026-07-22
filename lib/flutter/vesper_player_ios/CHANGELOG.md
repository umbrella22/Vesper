# Changelog

## Unreleased

### Added

- Added MethodChannel mapping for external subtitle configurations and
  `setSubtitleStyle`, backed by the iOS host-kit subtitle overlay.
- Added complete RTMP, RTSP, and HTTP-FLV protocol wire mapping with explicit
  iOS unsupported playback errors.

### Breaking Changes

- The iOS Flutter implementation now requires Flutter 3.44.0 or newer.

### Changed

- Picture in Picture failures now emit PiP-specific events and method errors
  without publishing generic player errors.
- `exitPictureInPicture` now lets `AVPictureInPictureControllerDelegate`
  callbacks publish the final inactive state.
- Bundled SourceNormalizer discovery now resolves the signed sibling framework
  embedded by the Flutter App target through the canonical optional-plugin
  SwiftPM package.

## 0.3.0 - 2026-05-18

- Prepared package metadata for the 0.3.0 release.
