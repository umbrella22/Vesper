# vesper_player_source_normalizer_ffmpeg

Optional native dependency package for Vesper Player's FFmpeg SourceNormalizer
plugin.

Apps that depend on this package receive the Android Maven AAR or iOS SPM binary
artifacts needed by `VesperSourceNormalizerConfiguration.preferBundled()` and
`VesperSourceNormalizerConfiguration.requireBundled()`. The package does not add
Flutter MethodChannels or Dart runtime behavior.

This package is not part of the default `vesper_player` dependency graph or the
default pub publishing script. Publish or depend on it only for apps that
explicitly opt in to the FFmpeg SourceNormalizer plugin.

FrameProcessor and mobile Decoder artifacts are not included.
