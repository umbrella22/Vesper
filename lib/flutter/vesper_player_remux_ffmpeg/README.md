# vesper_player_remux_ffmpeg

Optional native dependency package for Vesper Player's FFmpeg-backed
post-download MP4 remux plugin. It adds no MethodChannel or Dart runtime
behavior.

On Android, the package resolves
`vesper-player-kit-remux-ffmpeg` and its same-version Vesper host-kit and shared
FFmpeg runtime dependencies from Maven Central. On iOS, it resolves the exact
matching `VesperPlayerRemuxFfmpeg` product from the remote
`VesperPlayerKit` Swift package. That product contains only the remux plugin and
the matching AVCodec, AVFormat, and AVUtil components; it does not include the
Decoder or FrameProcessor plugins.

This package is not part of the default `vesper_player` dependency graph. Add it
only when the application enables post-download remux through the Vesper plugin
configuration.

FFmpeg remains under its upstream license terms. Distributors must preserve the
release notices, corresponding source, configure metadata, and LGPL relinking
rights described in
[THIRD_PARTY_NOTICES.md](../../../THIRD_PARTY_NOTICES.md).
