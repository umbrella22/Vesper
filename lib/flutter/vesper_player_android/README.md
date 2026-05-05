# vesper_player_android

The Android implementation package for `vesper_player`.

It is built on Media3 ExoPlayer and the Vesper Android host kit located in
`lib/android/vesper-player-kit`. The package is registered automatically by
`vesper_player`, so application code usually does not need to depend on it
directly.

## Platform Capabilities

| Format / feature                            | Status                             |
| ------------------------------------------- | ---------------------------------- |
| Local files                                 | ✅                                 |
| Progressive HTTP                            | ✅                                 |
| HLS                                         | ✅                                 |
| DASH                                        | ✅                                 |
| Live streams                                | ✅                                 |
| Live DVR                                    | ✅                                 |
| Track selection (video / audio / subtitles) | ✅                                 |
| Adaptive bitrate (ABR)                      | ✅ Auto / Constrained / FixedTrack |
| Buffering / retry / cache policy            | ✅                                 |
| Download management                         | ✅                                 |
| Preload                                     | ✅                                 |

## Technical Notes

- Playback backend: Media3 ExoPlayer behind the `VesperPlayerController` Kotlin facade
- Flutter integration: `MethodChannel` and `EventChannel` using `io.github.ikaros.vesper_player`
- View embedding: `AndroidView` with view type `io.github.ikaros.vesper_player/platform_view`
- Render path: `VesperPlayerController.create(renderSurfaceKind: ...)` selects the Android surface for Flutter playback. `auto` maps to `TextureView` for overlay and gesture compatibility. Use `surfaceView` only when the host explicitly wants the native Android HDR / high-frame-rate fullscreen path and can keep Flutter overlays safe.
- Runtime snapshot: exposes the currently active adaptive video variant through `controller.snapshot.effectiveVideoTrackId`
- Runtime observation: also exposes `controller.snapshot.videoVariantObservation`, derived from ExoPlayer's active `videoFormat` bitrate and rendered size
- Rust runtime: bridged through JNI so defaults, timeline, resilience, and playlist semantics stay aligned with the rest of the SDK

## Optional `player-remux-ffmpeg` Remux Plugin

To export downloaded HLS or DASH assets as `.mp4`, the host app must package
the optional `player-remux-ffmpeg` plugin and pass the absolute path to
`libplayer_remux_ffmpeg.so` through
`VesperDownloadConfiguration.pluginLibraryPaths`.

Typical setup:

1. Build the Android plugin artifact:

   ```sh
   ./scripts/vesper android remux-plugin <output-dir> [debug|release] [abi...]
   ```

2. Add the output directory to `sourceSets.main.jniLibs` in the host app
3. Resolve `libplayer_remux_ffmpeg.so` from `applicationInfo.nativeLibraryDir` at runtime
4. Pass the resolved absolute path into the download manager configuration

Android FFmpeg prebuilts are generated on demand. The repository only builds
them when the host explicitly requests the remux plugin. The current script also
supports coarse feature gates such as `VESPER_ANDROID_FFMPEG_ENABLE_DASH=0`,
but it does not yet support fine-grained trimming by demuxer, muxer, protocol,
or codec.

Both Android examples in this repository already demonstrate the full setup:

- `examples/android-compose-host/app/build.gradle.kts`
- `examples/flutter-host/android/app/build.gradle.kts`

This also means that depending on `vesper_player_android` alone does not pull
FFmpeg into your app. The plugin is bundled only when the host chooses to do so.

When the host bundles the plugin, treat the shipped `.so` files as FFmpeg
redistribution. Include FFmpeg license text and notices, provide the exact
corresponding FFmpeg source and configure flags, preserve LGPL relinking
rights, and track OpenSSL / libxml2 notices when those libraries are included.
The repository-level release checklist is in
[THIRD_PARTY_NOTICES.md](../../../THIRD_PARTY_NOTICES.md).

## Minimum Requirements

- Android API Level 26+
- Flutter 3.24.0+
- arm64 device or arm64 emulator when running Android host builds

## Related Resources

- Main package: `vesper_player`
- Platform contract: `vesper_player_platform_interface`
- Android host kit source: `lib/android/vesper-player-kit`
