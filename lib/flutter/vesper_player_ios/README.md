# vesper_player_ios

The iOS implementation package for `vesper_player`.

It is built on AVPlayer and the Vesper iOS host kit in `lib/ios/VesperPlayerKit`.
The package is registered automatically by `vesper_player`, so most app code
does not need to depend on it directly.

## Platform Capabilities

| Format / feature | Status |
|---|---|
| Local files | ✅ |
| Progressive HTTP | ✅ |
| HLS | ✅ |
| DASH | ❌ Not supported by the AVPlayer backend |
| Live streams | ✅ |
| Live DVR | ✅ |
| Track selection (audio / subtitles) | ✅ |
| Track selection (video) | ⚠️ Not exposed on the current AVPlayer route |
| Adaptive bitrate (ABR) | ⚠️ `constrained` is supported; `fixedTrack` is available as best-effort variant pinning on iOS 15+ |
| Buffering / retry / cache policy | ✅ |
| Download management | ✅ |
| Preload | ✅ |

> The DASH DTOs already exist, but the AVPlayer backend reports DASH as
> unsupported. Do not use `VesperPlayerSource.dash()` on iOS. Check
> `controller.snapshot.capabilities.supportsDash` if you need a runtime guard.
> For advanced playback controls, also prefer the fine-grained capability flags
> such as `supportsVideoTrackSelection` and `supportsAbrFixedTrack`.
> On iOS, `supportsAbrFixedTrack` means best-effort HLS variant pinning rather
> than exact AVPlayer video-track switching. The host keeps variant track IDs
> stable across reloads, restores both fixed-track pinning and single-axis
> constrained ABR only after the current HLS variant catalog is ready, will
> best-effort remap a restored fixed-track request onto a semantically
> equivalent variant when the HLS ladder drifts slightly, and best-effort
> surfaces the currently active HLS variant through
> `controller.snapshot.effectiveVideoTrackId`. For best-effort fixed-track
> convergence, the Flutter snapshot also exposes
> `controller.snapshot.fixedTrackStatus` with `pending / locked / fallback`.

## Recommended Download Planning Flow

If you pass a remote `.m3u8` URL directly into `createTask(...)`, the host
usually ends up with only the manifest entry point, which is not enough for a
real offline save or a later remux step.

Recommended flow:

1. Insert a temporary "preparing" task in the host UI as soon as the user taps download
2. Read the remote HLS manifest in the background and build
   `VesperDownloadAssetIndex.resources + segments`
3. Create the real task only after the asset plan is ready, so the downloader
   fetches both manifest resources and media segments

The native iOS example and the Flutter example in this repository already
follow that flow. The iOS example also continues to skip DASH download entry
points instead of pretending that DASH is supported.

## Technical Notes

- Playback backend: AVPlayer behind the `VesperPlayerController` Swift facade
- Flutter integration: `MethodChannel` and `EventChannel` using `io.github.ikaros.vesper_player`
- View embedding: `UiKitView` with view type `io.github.ikaros.vesper_player/platform_view`
- Rust runtime: bridged through the `player-ffi-resolver` XCFramework so defaults, timeline, resilience, and playlist behavior stay aligned with the shared runtime

## Optional `player-ffmpeg` Remux Plugin

If the host app wants to export downloaded HLS or DASH content to `.mp4`, it
must embed the `player-ffmpeg` dynamic library into the app bundle and pass the
real `libplayer_ffmpeg.dylib` path through
`VesperDownloadConfiguration.pluginLibraryPaths`.

Typical setup:

1. Add an Xcode Run Script phase to the app target:

   ```sh
   /bin/bash "$SRCROOT/../../../scripts/embed-ios-player-ffmpeg-plugin.sh" "vesper_player_ios.framework"
   ```

   For the native iOS host kit, replace the argument with `VesperPlayerKit.framework`.

2. Resolve `libplayer_ffmpeg.dylib` at runtime from `Bundle.main.privateFrameworksPath`
   or the app `Frameworks` directory
3. Pass the resolved absolute path into the download manager configuration

Apple FFmpeg prebuilts are also built on demand. The current scripts support
coarse feature gates such as `VESPER_APPLE_FFMPEG_ENABLE_DASH=0`, but not
fine-grained trimming by demuxer, muxer, protocol, or codec.

Both iOS examples in this repository already embed the plugin that way:

- `examples/ios-swift-host/VesperPlayerHostDemo.xcodeproj`
- `examples/flutter-host/ios/Runner.xcodeproj`

Note that iOS only allows signed dynamic libraries that are already inside the
app bundle. Loading unsigned or remotely downloaded plugins is not supported.

## Minimum Requirements

- iOS 14.0+
- Flutter 3.41.0+

## Related Resources

- Main package: `vesper_player`
- Platform contract: `vesper_player_platform_interface`
- iOS host kit source: `lib/ios/VesperPlayerKit`
