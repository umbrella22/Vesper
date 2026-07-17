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
| External text subtitles                       | ✅ SRT / WebVTT / SSA through Media3 |
| Subtitle visibility / font scale             | ✅ Native overlay; scale `0.5...3.0` |
| Adaptive bitrate (ABR)                      | ✅ Auto / Constrained / FixedTrack |
| Buffering / retry / cache policy            | ✅                                 |
| Download management                         | ✅                                 |
| Preload                                     | ✅                                 |
| System playback / notification controls     | ✅ MediaSession + foreground service |
| Android external playback                   | ✅ Optional `vesper_player_external_playback` package |

## Technical Notes

- Playback backend: Media3 ExoPlayer behind the `VesperPlayerController` Kotlin facade
- Flutter integration: `MethodChannel` and `EventChannel` using `io.github.ikaros.vesper_player`
- View embedding: `AndroidView` with view type `io.github.ikaros.vesper_player/platform_view`
- Render path: `VesperPlayerController.create(renderSurfaceKind: ...)` selects the Android surface for Flutter playback. `auto` maps to `SurfaceView` for the Flutter 3.44+ high-fidelity native video path. Use `textureView` when the host depends on complex Flutter overlays, scrolling, clipping, rounded corners, or animation-heavy composition.
- Runtime snapshot: exposes the currently active adaptive video variant through `controller.snapshot.effectiveVideoTrackId`
- Backend family: runtime snapshots use the public Android `VesperPlayerController.backendFamily` facade and do not depend on Android host-kit bridge or `Native*` implementation types
- Runtime observation: also exposes `controller.snapshot.videoVariantObservation`, derived from ExoPlayer's active `videoFormat` bitrate and rendered size
- System playback: `configureSystemPlayback` binds the active ExoPlayer to a Media3 `MediaSessionService`, starts a media playback foreground service while audio is playing, exposes default 10-second seek back / play-pause / seek forward media actions through MediaSession button preferences, filters seek commands when `showSeekActions` is disabled, and clears the session on pause / stop / dispose
- Screen awake: `createPlayer(keepScreenOnDuringPlayback: ...)` and `setKeepScreenOnDuringPlayback(...)` control whether the host playback view keeps the display awake while playback is active
- Downloads: `VesperDownloadConfiguration` defaults to task snapshot restore and resumable partial transfers for SDK-managed downloads
- Rust runtime: bridged through JNI so defaults, timeline, resilience, and playlist semantics stay aligned with the rest of the SDK

## System Playback Host Requirements

`getSystemPlaybackPermissionStatus()` returns `notRequired`, `granted`, or
`denied` without prompting. `requestSystemPlaybackPermissions()` requests
`POST_NOTIFICATIONS` on Android 13+. The SDK does not request this permission
automatically; call it only from an app-controlled moment if the app wants
runtime notification permission for its broader notification UX.

The Android library manifest contributes:

- `android.permission.FOREGROUND_SERVICE`
- `android.permission.FOREGROUND_SERVICE_MEDIA_PLAYBACK`
- `android.permission.POST_NOTIFICATIONS`
- the SDK `MediaSessionService`

Host apps may declare the same permissions explicitly for review clarity.
Android 13+ exempts media-session playback notifications from the runtime
notification permission, so `POST_NOTIFICATIONS` denial must not block
background playback or foreground service startup.

Download restore is intentionally separate from Android OS-managed background
transfer. The Flutter package restores SDK task state on manager startup, resumes
partial files with validated range requests, and restarts only the affected
resource when a server ignores a resume range. Complete resources stream by
default, `Range: bytes=<existing>-` is used for resume, and fixed Range chunks
are used only when `rangeChunkBytes` is configured. It does not install a
WorkManager or download ForegroundService for process-death transfers.

The Android host kit stores downloads under the app-private
`filesDir/vesper-downloads` by default and does not request public storage
permissions. `shareTaskOutput(...)` shares completed private files through the
SDK FileProvider authority `${applicationId}.vesper.player.fileprovider`, and
`saveTaskOutput(...)` copies completed files into MediaStore `Downloads` or
`Movies` with scoped storage on Android 10 and newer. Android 9 and older hosts
should use the share helper or their own export flow because the SDK does not
request legacy public storage permissions.

Download source headers are passed through the Android host kit for manifest
reads, size probes, Media3 `DataSpec` fallback reads, and media transfers. Hosts
should put generic HTTP context such as `User-Agent`, `Referer`, `Origin`,
`Cookie`, or authorization headers on `VesperPlayerSource.headers`; the SDK
forwards them consistently and ignores empty header names or blank values.

## Optional Android External Playback

Android Cast and DLNA live in the separate `vesper_player_external_playback`
Flutter package. Its Android implementation calls the
`vesper-player-kit-external-playback` Kotlin facade, so the default player
package does not pull in Google Play Services, Cast Framework, DLNA discovery,
the local HTTP relay, or the optional FFmpeg runtime.

For local workspace builds, include `:vesper-player-kit-external-playback`
beside `:vesper-player-kit` and `:vesper-player-kit-ffmpeg-runtime` in the host
Android Gradle settings. The external-playback module contributes a default
`VesperExternalCastOptionsProvider` that uses Google's Default Media Receiver.
Hosts that need a custom receiver can override the manifest meta-data key
`io.github.ikaros.vesper.player.android.external.RECEIVER_APPLICATION_ID`.

Cast V2 supports remote `http` / `https` HLS, DASH, and progressive sources.
Sources with headers, local files, and `content://` inputs are exposed to Cast
and DLNA receivers through the external playback package's tokenized local HTTP
relay when the selected proxy policy allows it. DRM, transcoding, DASH manifest
rewrite, offline assets, and custom receiver behavior are outside this scope.

Host apps that use DLNA discovery or relay-backed playback must configure their
own Android cleartext policy. The Flutter external-playback package contributes
network and Cast metadata only; it does not enable
`android:usesCleartextTraffic` for the app.

## Optional `player-remux-ffmpeg` Remux Plugin

To export downloaded HLS, DASH, or FLV assets as `.mp4`, the host app must package
the optional `player-remux-ffmpeg` plugin and pass the absolute path to
`libvesper_remux_ffmpeg.so` through
`VesperDownloadConfiguration.pluginLibraryPaths`.

Typical setup:

1. Build the shared Android FFmpeg runtime for the enabled consumers:

   ```sh
   ./scripts/vesper ffmpeg --platform android --profile download-remux --abi arm64-v8a
   ```

2. Build the Android plugin artifact. Android ABI selection is controlled by
   `RUST_ANDROID_ABIS`; the plugin must not copy `libav*` runtime libraries:

   ```sh
   ./scripts/vesper android remux-plugin <output-dir> [debug|release]
   ```

3. Add `vesper-player-kit-ffmpeg-runtime` and the plugin output directory to the
   host app packaging.
4. Resolve `libvesper_remux_ffmpeg.so` from `applicationInfo.nativeLibraryDir` at runtime.
5. Pass the resolved absolute path into the download manager configuration.

When the same app also enables DLNA relay remux, build the runtime with both
consumers:

```sh
./scripts/vesper ffmpeg --platform android --profile default --abi arm64-v8a
```

Android FFmpeg prebuilts are generated on demand through the root FFmpeg profile
CLI. The `download-remux`, `relay-remux`, and `default` profiles are local-only
and validate that network and OpenSSL remain disabled. Fine-grained capability
overlays are available through `--extra-*` options, and validation fails when an
overlay conflicts with the selected profile policy.

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

## Optional Mobile Plugin Routes

`createPlayer` forwards
`VesperSourceNormalizerConfiguration` and
`VesperFrameProcessorConfiguration` to the Android host kit. Both are disabled
by default.

For SourceNormalizer v1, `diagnosticsOnly` loads the optional plugin and reports
capabilities through `pluginDiagnostics`; `preflightOnly` may also open and
close a packet session for the selected source. The Android player still gives
the original source to ExoPlayer, and preflight failures are non-fatal. Apps can
depend on `vesper_player_source_normalizer_ffmpeg` and use the bundled
configuration presets instead of app-side library-path wiring. Hosts that
package `vesper-player-kit-source-normalizer-ffmpeg` must also package the
matching `vesper-player-kit-ffmpeg-runtime`; the SourceNormalizer AAR carries
plugin metadata/profile hash but must not contain FFmpeg runtime `.so` files.

For FrameProcessor v1, `diagnosticsOnly` reports availability without opening
frame sessions or marking playback participation. Android playback participation
requires the explicit SDK-managed native-frame route: pass
`VesperNativeFramePipelineConfiguration` with `preferNativeFrame` or
`requireNativeFrame`, package the SourceNormalizer packet input, the
`vesper-player-kit-decoder-mediacodec` decoder plugin, and any optional
FrameProcessor plugin paths. The normal ExoPlayer / system-player route remains
unchanged and does not expose decoded frames to FrameProcessor plugins.

## Minimum Requirements

- Android API Level 26+
- Flutter 3.44.0+
- arm64 device or arm64 emulator when running Android host builds

## Related Resources

- Main package: `vesper_player`
- Platform contract: `vesper_player_platform_interface`
- Android host kit source: `lib/android/vesper-player-kit`

## Subtitle Notes

`VesperPlayerSource.subtitleConfigurations` is mapped to the Android host kit,
and `setSubtitleStyle` is a real MethodChannel command rather than a no-op. Text
cues render in the native surface host; frames are never sent through Dart.
HTTP `.flv` URLs remain progressive unless `VesperPlayerSource.flvLive` is used.
