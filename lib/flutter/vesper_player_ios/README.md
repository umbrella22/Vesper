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

For remote VOD HLS, static DASH, and FLV downloads, the iOS host kit runs a
native prepare phase before transfer starts. The prepare phase expands the
manifest or clip list, rejects live or size-unknown inputs, writes local
rewritten manifests or concat lists, and reports the completed asset index
through `taskUpdated` before download progress begins. Download events are a
breaking incremental stream: `initialSnapshot`, `taskCreated`, `taskUpdated`,
`taskRemoved`, `downloadError`, and `exportProgress`.

Recommended flow:

1. Insert a temporary "preparing" task in the host UI as soon as the user taps download
2. Call `createTask(...)` with the entry-point source, a target directory, and
   an empty `VesperDownloadAssetIndex`
3. Set `targetOutputFormat` to `.mp4` for HLS, DASH, and FLV segmented sources
   when the completed artifact should be MP4

The native iOS example and the Flutter example in this repository already
follow that flow for HLS, DASH, and FLV.

`VesperDownloadConfiguration` enables task snapshot restore and resumable partial
downloads by default. The iOS host kit restores interrupted tasks when the
manager is recreated and resumes existing partial files with range requests when
the server supports them. It validates resume ranges before appending partial
files and restarts only the affected resource when a server ignores a resume
range. Complete resources stream by default, `Range: bytes=<existing>-` is used
for resume, and fixed Range chunks are used only when `rangeChunkBytes` is
configured. This is SDK-managed foreground recovery, not an iOS background
`URLSessionConfiguration.background` implementation.

Remote media URLs used by the iOS offline downloader and DASH bridge must be
HTTPS. The SDK does not relax App Transport Security for `http://` media
resources; host apps that must support insecure HTTP should fetch those
resources outside the SDK and pass local file URLs to the player or downloader.
SDK-created download directories, state files, generated resources, and final
offline files are excluded from iCloud backup.

Use `shareTaskOutput(...)` for the native share sheet and `saveTaskOutput(...)`
for the iOS document export picker. Both expose completed files without moving
or deleting the SDK-owned offline copy.

Download source headers are passed through the iOS host kit for manifest reads,
size probes, and media transfers. Hosts should put generic HTTP context such as
`User-Agent`, `Referer`, `Origin`, `Cookie`, or authorization headers on
`VesperPlayerSource.headers`; the SDK forwards them consistently and ignores
empty header names or blank values.

## Technical Notes

- Playback backend: AVPlayer behind the `VesperPlayerController` Swift facade
- Flutter integration: `MethodChannel` and `EventChannel` using `io.github.ikaros.vesper_player`
- View embedding: `UiKitView` with view type `io.github.ikaros.vesper_player/platform_view`
- System playback: `configureSystemPlayback` writes `MPNowPlayingInfoCenter`, registers `MPRemoteCommandCenter` with default 10-second skip back / play-pause / skip forward actions, and activates an `AVAudioSession` playback category with long-form video route sharing when background audio is enabled
- Screen awake: `createPlayer(keepScreenOnDuringPlayback: ...)` and `setKeepScreenOnDuringPlayback(...)` control the SDK idle-timer policy while playback is active
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

If the host app wants to export downloaded HLS, DASH, or FLV content to `.mp4`,
it must embed the optional shared FFmpeg runtime plus `player-remux-ffmpeg`
plugin and pass the real plugin framework binary path through
`VesperDownloadConfiguration.pluginLibraryPaths`. FFmpeg is not embedded in the
core iOS host kit.

Typical setup:

1. Add an Xcode Run Script phase to the app target:

   ```sh
   /bin/bash "$SRCROOT/../../../scripts/ios/embed-player-remux-ffmpeg-plugin.sh" "vesper_player_ios.framework"
   ```

   For the native iOS host kit, replace the argument with `VesperPlayerKit.framework`.

2. For release downloads, embed and sign both
   `VesperPlayerFfmpegRuntime.xcframework.zip` and
   `VesperPlayerRemuxFfmpegPlugin.xcframework.zip` instead of shipping bare
   `.dylib` files. Build both artifacts from the same FFmpeg profile so their
   `profile-hash.txt` values match.
3. Resolve the plugin framework binary at runtime from
   `Bundle.main.privateFrameworksPath` or the app `Frameworks` directory.
4. Pass the resolved absolute path into the download manager configuration.

Apple FFmpeg prebuilts are built on demand through the root profile CLI:

```sh
./scripts/vesper ffmpeg --platform ios --profile default --slice ios-arm64 --slice ios-simulator-arm64
./scripts/vesper ios ffmpeg-runtime-release /tmp/vesper-ios-release --profile default ios-arm64 ios-simulator-arm64
./scripts/vesper ios stage-remux-plugin-release /tmp/vesper-ios-release --profile default ios-arm64 ios-simulator-arm64
```

Both iOS examples in this repository already embed the plugin that way:

- `examples/ios-swift-host/VesperPlayerHostDemo.xcodeproj`
- `examples/flutter-host/ios/Runner.xcodeproj`

Note that iOS only allows signed dynamic libraries that are already inside the
app bundle. Loading unsigned or remotely downloaded plugins is not supported.

When the host bundles the plugin, treat the optional XCFramework contents as
FFmpeg redistribution. Include FFmpeg license text and notices, provide the
exact corresponding FFmpeg source and configure flags, and preserve LGPL
relinking rights. The repository-level release checklist is in
[THIRD_PARTY_NOTICES.md](../../../THIRD_PARTY_NOTICES.md).

## Optional Mobile Plugin Diagnostics

`createPlayer` forwards
`VesperSourceNormalizerConfiguration` and
`VesperFrameProcessorConfiguration` to `VesperPlayerKit`. Both are disabled by
default.

For SourceNormalizer v1, `diagnosticsOnly` loads the optional plugin and reports
capabilities through `pluginDiagnostics`; `preflightOnly` may also open and
close a packet session for the selected source. AVPlayer still receives the
original source, and preflight failures are non-fatal. Hosts that embed
`VesperPlayerSourceNormalizerFfmpegPlugin.xcframework.zip` must also embed and
sign the matching `VesperPlayerFfmpegRuntime.xcframework.zip`; both artifacts
must have matching `profile-hash.txt` values. The shared runtime framework is
not a plugin path.

For FrameProcessor v1,
`VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip` is a diagnostics
shell only. It can report capability diagnostics, but it never opens frame
sessions, processes frames, or participates in iOS playback. Mobile Decoder
artifacts remain deferred.

## Minimum Requirements

- iOS 17.0+
- Flutter 3.44.0+

## Related Resources

- Main package: `vesper_player`
- Platform contract: `vesper_player_platform_interface`
- iOS host kit source: `lib/ios/VesperPlayerKit`
