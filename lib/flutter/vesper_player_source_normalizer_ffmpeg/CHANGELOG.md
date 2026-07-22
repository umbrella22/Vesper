# Changelog

## 0.4.0 - Unreleased

- Aligned package and Android plugin metadata with the Vesper 0.4 package
  family.
- Changed the iOS SPM package to depend on the canonical
  `VesperPlayerSourceNormalizerFfmpegPlugin` product. Flutter App targets embed
  and sign its framework and the FFmpeg component frameworks as top-level
  siblings.
- Tagged iOS releases now publish the SourceNormalizer framework as part of the
  seven-XCFramework optional set, gated on the FFmpeg compliance archive and
  exact corresponding source archive.

## 0.3.0

- Adds optional native dependency wiring for the FFmpeg SourceNormalizer plugin.
