# vesper_player_source_normalizer_ffmpeg

Optional native dependency package for Vesper Player's FFmpeg SourceNormalizer
plugin.

Android apps that depend on this package receive the native Maven dependency
needed by `VesperSourceNormalizerConfiguration.preferBundled()` and
`VesperSourceNormalizerConfiguration.requireBundled()`. On iOS, it resolves the
exact matching `VesperPlayerSourceNormalizerFfmpeg` product from the remote
`VesperPlayerKit` Swift package. The package does not add Flutter MethodChannels
or Dart runtime behavior.

## iOS Packaging

The remote capability product embeds and signs
`VesperPlayerSourceNormalizerFfmpegPlugin`, `VesperFFmpegAVCodec`,
`VesperFFmpegAVFormat`, and `VesperFFmpegAVUtil`. Hosts that enable Decoder or
FrameProcessor capabilities add those plugins separately.
The bundled configuration presets select
`VesperBundledPluginReferences.sourceNormalizerFfmpeg`; the internal iOS
artifact resolver maps that identity to the embedded SourceNormalizer framework
executable.

This package is not part of the default `vesper_player` dependency graph. Add it
only for apps that explicitly opt in to the FFmpeg SourceNormalizer plugin.

Tagged Vesper GitHub Releases publish this plugin with the other six optional
iOS sibling XCFrameworks, `VesperPlayerOptionalPlugins-FFmpeg-Compliance.zip`,
and the versioned exact corresponding FFmpeg source archive. The release
verifier treats that as one mandatory set. App and SDK distributors must still
preserve the included FFmpeg license, notices, configure metadata, source
availability, and LGPL relinking rights separately from Vesper's Apache-2.0
license; see [THIRD_PARTY_NOTICES.md](../../../THIRD_PARTY_NOTICES.md).

This Flutter package enables the SourceNormalizer dependency declaration. It
does not pull in the remux, Decoder, or FrameProcessor products.
