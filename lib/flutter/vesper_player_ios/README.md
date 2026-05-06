# vesper_player_ios

The iOS implementation package for `vesper_player`.

It is built on AVPlayer and the Vesper iOS host kit in `lib/ios/VesperPlayerKit`.
The package is registered automatically by `vesper_player`, so most app code
does not need to depend on it directly.

## Platform Capabilities

| Format / feature                    | Status                                                                                             |
| ----------------------------------- | -------------------------------------------------------------------------------------------------- |
| Local files                         | ✅                                                                                                 |
| Progressive HTTP                    | ✅                                                                                                 |
| HLS                                 | ✅                                                                                                 |
| DASH                                | ✅ DASH-to-HLS bridge for single-period fMP4 VOD / live                                            |
| Live streams                        | ✅                                                                                                 |
| Live DVR                            | ✅                                                                                                 |
| Track selection (audio / subtitles) | ✅                                                                                                 |
| Track selection (video)             | ⚠️ Not exact AVPlayer track switching; use ABR variant pinning and the track catalog               |
| Adaptive bitrate (ABR)              | ✅ `constrained`; `fixedTrack` is best-effort variant pinning on iOS 15+                           |
| Buffering / retry / cache policy    | ✅                                                                                                 |
| Download management                 | ✅                                                                                                 |
| Preload                             | ✅                                                                                                 |
| System playback controls            | ✅ Now Playing + RemoteCommand                                                                     |
| AirPlay route picker                | ✅ Via `VesperAirPlayRouteButton` in `vesper_player_ui`                                            |

> The iOS DASH path supports single-period fMP4 manifests for static VOD and
> dynamic live / DVR when they use either `SegmentBase + sidx` or
> `SegmentTemplate` / `SegmentTimeline`. It also exposes DASH manifest audio,
> video, and WebVTT subtitle catalogs for host UI.
> Source headers are forwarded to MPD, SIDX,
> init segment, and media segment requests; media bytes are served through the
> SDK resource-loader proxy so protected origins do not depend on AVPlayer
> propagating headers to nested HLS segment URLs. Check
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
> `controller.snapshot.effectiveVideoTrackId`. The snapshot also carries raw
> runtime evidence through `controller.snapshot.videoVariantObservation`,
> populated from AVPlayer access-log bitrate and the current presentation size.
> For best-effort fixed-track convergence, the Flutter snapshot also exposes
> `controller.snapshot.fixedTrackStatus` with `pending / locked / fallback`; iOS keeps the status
> `pending` while evidence is still settling, only publishes `locked` after a stable match, and only
> publishes `fallback` after sustained mismatch evidence.
> If a restored fixed-track request remains on a different observed variant for
> long enough, the iOS host now reports that through `controller.snapshot.lastError`
> and automatically degrades the restored request into constrained ABR with the
> requested variant limits when possible, otherwise back to automatic ABR.

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
follow that flow. DASH playback is supported through the iOS DASH-to-HLS
bridge, but DASH download entry points remain disabled in the iOS example host.

## Technical Notes

- Playback backend: AVPlayer behind the `VesperPlayerController` Swift facade
- Flutter integration: `MethodChannel` and `EventChannel` using `io.github.ikaros.vesper_player`
- View embedding: `UiKitView` with view type `io.github.ikaros.vesper_player/platform_view`
- System playback: `configureSystemPlayback` writes `MPNowPlayingInfoCenter`, registers `MPRemoteCommandCenter`, and activates an `AVAudioSession` playback category with long-form video route sharing when background audio is enabled
- Rust runtime: bridged through the `player-ffi-ios` XCFramework so defaults, timeline, resilience, and playlist behavior stay aligned with the shared runtime

## System Playback Host Requirements

`getSystemPlaybackPermissionStatus()` and `requestSystemPlaybackPermissions()`
return `notRequired` on iOS because Now Playing, remote commands, and AirPlay
route picking do not require a runtime permission. Apps that intend to continue
audio while locked or in the background must still declare `UIBackgroundModes`
with the `audio` value in the app `Info.plist`.

The SDK registers play, pause, toggle, stop, skip, and playback-position remote
commands for the most recently configured controller. `clearSystemPlayback()` or
controller disposal removes Now Playing metadata and remote command handlers.

Use `VesperAirPlayRouteButton` from `vesper_player_ui` for an in-app AirPlay
picker backed by `AVRoutePickerView`. The SDK keeps the audio session and Now
Playing state aligned with the active controller, and the route picker
prioritizes video-capable devices by default. Users can still choose AirPlay
targets from Control Center. AirDrop is file sharing, not media playback
routing.

## Optional `player-remux-ffmpeg` Remux Plugin

If the host app wants to export downloaded HLS or DASH content to `.mp4`, it
must embed the `player-remux-ffmpeg` dynamic library into the app bundle and pass the
real `libplayer_remux_ffmpeg.dylib` path through
`VesperDownloadConfiguration.pluginLibraryPaths`.

Typical setup:

1. Add an Xcode Run Script phase to the app target:

   ```sh
   /bin/bash "$SRCROOT/../../../scripts/ios/embed-player-remux-ffmpeg-plugin.sh" "vesper_player_ios.framework"
   ```

   For the native iOS host kit, replace the argument with `VesperPlayerKit.framework`.

2. Resolve `libplayer_remux_ffmpeg.dylib` at runtime from `Bundle.main.privateFrameworksPath`
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

When the host bundles the plugin, treat the shipped `.dylib` files as FFmpeg
redistribution. Include FFmpeg license text and notices, provide the exact
corresponding FFmpeg source and configure flags, and preserve LGPL relinking
rights. The repository-level release checklist is in
[THIRD_PARTY_NOTICES.md](../../../THIRD_PARTY_NOTICES.md).

## Minimum Requirements

- iOS 14.0+
- Flutter 3.41.0+

## Related Resources

- Main package: `vesper_player`
- Platform contract: `vesper_player_platform_interface`
- iOS host kit source: `lib/ios/VesperPlayerKit`
