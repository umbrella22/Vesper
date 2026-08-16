# vesper_player_ios

The iOS implementation package for `vesper_player`.

It is built on AVPlayer and the Vesper iOS host kit in `lib/ios/VesperPlayerKit`.
The package is registered automatically by `vesper_player`, so most app code
does not need to depend on it directly.

The published Flutter package resolves the binary Swift package from
`https://github.com/umbrella22/VesperPlayerKit.git` within the compatible
`0.4.x` line. It depends only on the remote `VesperPlayerKit` product. The
binary target already contains the native bridge closure, so consumers do not
need a monorepo checkout, a local `lib/ios/VesperPlayerKit` path, or a separate
`VesperPlayerFFI` product.

Native registration uses `io.github.umbrella22.vesper_player` for the player
MethodChannel and related EventChannel suffixes. This is a breaking pre-release
rename from `io.github.ikaros`; no old channel handlers are registered. The
Swift package, products, and `VesperPlayerKit` module name are unchanged.

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
| External text subtitles            | ✅ bounded UTF-8 SRT / WebVTT / SSA native overlay                                                  |
| Subtitle visibility / font scale    | ✅ AVPlayer text style rules plus native overlay                                                     |
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

## FairPlay DRM

The Flutter iOS implementation delegates DRM to `VesperPlayerKit` and AVPlayer.
Use `VesperPlayerDrmConfiguration` with `keySystem: 'fairPlay'`,
`licenseUri`, and either `fairPlayCertificateUri` or
`fairPlayCertificateBase64`. FairPlay is accepted only on the direct HLS
AVPlayer route; DASH, download, preload, SourceNormalizer normalized output,
external playback handoff, and SDK-managed native-frame routes report typed
unsupported capability errors.

Retryable FairPlay certificate or license failures surface through the existing
`VesperPlayerSnapshot.lastError` and `VesperPlayerErrorEvent` APIs after the
retry budget is exhausted. Error details include sanitized fields such as
`reason`, `keySystem`, `route`, `licenseUriHost`, `certificateUriHost`,
`httpStatusCode`, `attemptsExhausted`, and `maxAttempts`; full URLs, headers,
tokens, and certificate data are not emitted.

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
- Flutter integration: `MethodChannel` and `EventChannel` using `io.github.umbrella22.vesper_player`
- View embedding: `UiKitView` with view type `io.github.umbrella22.vesper_player/platform_view`
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
it must embed the three optional FFmpeg component frameworks plus the
`player-remux-ffmpeg` plugin framework. Select the plugin with a native
`VesperPluginReference` in
`VesperDownloadConfiguration.postDownloadPluginReferences`; executable paths
are internal build-time artifact locators. FFmpeg is not embedded in the core
iOS host kit.

Typical setup:

1. Stage the canonical optional package before SwiftPM resolution.
2. Add `VesperFFmpegAVCodec`, `VesperFFmpegAVFormat`, `VesperFFmpegAVUtil`, and
   `VesperPlayerRemuxFfmpegPlugin` from `VesperPlayerOptionalPlugins` to the App
   target with Embed & Sign. Add the other three direct plugin products only
   when the host enables those capabilities.
3. Let Xcode place the selected FFmpeg component and plugin
   frameworks as top-level siblings under `Runner.app/Frameworks`.
4. Configure plugin ID `io.github.umbrella22.vesper.remux-ffmpeg`, capability
   instance `io.github.umbrella22.vesper.remux-ffmpeg.post-download`, and native
   transport. The host kit resolves the embedded signed framework.

Apple FFmpeg prebuilts are built on demand through the root profile CLI:

```sh
./scripts/vesper ios stage-optional-plugins-release \
  /tmp/vesper-ios-optional-plugins-release \
  --profile source-normalizer \
  ios-arm64 ios-simulator-arm64
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

## Optional Mobile Plugin Routes

`createPlayer` forwards
`VesperSourceNormalizerConfiguration` and
`VesperFrameProcessorConfiguration` to `VesperPlayerKit`. Both are disabled by
default.

For SourceNormalizer, `diagnosticsOnly` loads the optional plugin and reports
capabilities through `pluginDiagnostics`; `preflightOnly` may also open and
close a packet session while AVPlayer still receives the original source.
`preferNormalized` and `requireNormalized` may instead hand a disk-backed fMP4
or short-window HLS resource to AVPlayer. Apps can depend on
`vesper_player_source_normalizer_ffmpeg` and use the bundled configuration
presets instead of app-side plugin-path wiring. On iOS, the App target directly
embeds `VesperPlayerSourceNormalizerFfmpegPlugin` plus
`VesperFFmpegAVCodec`, `VesperFFmpegAVFormat`, and `VesperFFmpegAVUtil` from the
optional package. The bundled resolver loads
`VesperPlayerSourceNormalizerFfmpegPlugin.framework/`
`VesperPlayerSourceNormalizerFfmpegPlugin`. FFmpeg component frameworks are not
plugin paths.

For FrameProcessor, `diagnosticsOnly` reports availability without opening
frame sessions or marking playback participation. iOS playback participation
requires the explicit SDK-managed native-frame route: pass
`VesperNativeFramePipelineConfiguration` with `preferNativeFrame` or
`requireNativeFrame` so the host kit can route SourceNormalizer packet input
through VideoToolbox, the optional FrameProcessor chain, and MetalLayer
presentation. Default AVPlayer playback remains unchanged; HDR and Dolby Vision
stay on AVPlayer / system playback, and the SDK-managed native-frame route is
SDR-only today.

## Minimum Requirements

- iOS 17.0+
- Flutter 3.44.0+

## Related Resources

- Main package: `vesper_player`
- Platform contract: `vesper_player_platform_interface`
- iOS host kit source: `lib/ios/VesperPlayerKit`

## Subtitle Notes

The Flutter package maps external subtitle configurations and subtitle style
commands to `VesperPlayerKit`. External input uses the host kit's eight-track,
2 MiB-per-track, and 10,000-cue limits. Unsupported formats and sources return
platform errors instead of succeeding as no-ops.
