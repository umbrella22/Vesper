# Changelog

## 0.4.0 - Unreleased

### Added

- Added MethodChannel mapping for external subtitle configurations and
  `setSubtitleStyle`, backed by the iOS host-kit subtitle overlay.
- Added complete RTMP, RTSP, and HTTP-FLV protocol wire mapping with explicit
  iOS unsupported playback errors.
- Added track support, catalog revision, playback-path, and structured
  fixed-track error mapping for the iOS wire contract.

### Breaking Changes

- MethodChannel, EventChannel, and platform-view identifiers now use the
  `io.github.umbrella22` reverse-DNS root. The Swift package and module remain
  named `VesperPlayerKit`; custom channel integrations must use the new names.
- The iOS Flutter implementation now requires Flutter 3.44.0 or newer.
- Subtitle MethodChannel calls now await the iOS host API and return canonical
  `vesper_subtitle_error` details for validation and convergence failures.

### Changed

- Picture in Picture failures now emit PiP-specific events and method errors
  without publishing generic player errors.
- `setAbrPolicy` now forwards the optional expected catalog revision and keeps
  native fixed-track capability details in the returned platform error.
- `exitPictureInPicture` now lets `AVPictureInPictureControllerDelegate`
  callbacks publish the final inactive state.
- Subtitle snapshots now carry canonical catalog/selection state and
  requested/confirmed/effective selection fields.
- Bundled SourceNormalizer discovery now resolves the signed sibling framework
  embedded by the Flutter App target through the canonical optional-plugin
  SwiftPM package.

## 0.3.0 - 2026-05-18

- Prepared package metadata for the 0.3.0 release.
