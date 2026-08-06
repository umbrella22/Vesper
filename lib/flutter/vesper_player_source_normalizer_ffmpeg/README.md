# vesper_player_source_normalizer_ffmpeg

Optional native dependency package for Vesper Player's FFmpeg SourceNormalizer
plugin.

Android apps that depend on this package receive the native Maven dependency
needed by `VesperSourceNormalizerConfiguration.preferBundled()` and
`VesperSourceNormalizerConfiguration.requireBundled()`. On iOS, the Flutter
package does not add optional XCFramework products automatically; the App target
embeds the required direct products. The package does not add Flutter
MethodChannels or Dart runtime behavior.

## iOS Packaging

Stage the canonical local package before SwiftPM resolution:

```sh
./scripts/vesper ios stage-optional-plugins-release \
  /tmp/vesper-ios-optional-plugins-release \
  --profile source-normalizer \
  ios-arm64 ios-simulator-arm64
```

The Flutter App target embeds and signs
`VesperPlayerSourceNormalizerFfmpegPlugin`, `VesperFFmpegAVCodec`,
`VesperFFmpegAVFormat`, and `VesperFFmpegAVUtil` as direct products from
`VesperPlayerOptionalPlugins`. Xcode places those frameworks as top-level
siblings under `Runner.app/Frameworks`. Hosts that enable Decoder or
FrameProcessor capabilities add their direct plugin products separately.
The bundled configuration presets select
`VesperBundledPluginReferences.sourceNormalizerFfmpeg`; the internal iOS
artifact resolver maps that identity to the embedded SourceNormalizer framework
executable.

This package is not part of the default `vesper_player` dependency graph or the
default pub publishing script. Publish or depend on it only for apps that
explicitly opt in to the FFmpeg SourceNormalizer plugin.

Tagged Vesper GitHub Releases publish this plugin with the other six optional
iOS sibling XCFrameworks, `VesperPlayerOptionalPlugins-FFmpeg-Compliance.zip`,
and the versioned exact corresponding FFmpeg source archive. The release
verifier treats that as one mandatory set. App and SDK distributors must still
preserve the included FFmpeg license, notices, configure metadata, source
availability, and LGPL relinking rights separately from Vesper's Apache-2.0
license; see [THIRD_PARTY_NOTICES.md](../../../THIRD_PARTY_NOTICES.md).

This Flutter package enables the SourceNormalizer dependency declaration; the
iOS App target remains responsible for embedding the direct SourceNormalizer
and FFmpeg products. Repository hosts that need the complete optional matrix
embed all seven direct products.
