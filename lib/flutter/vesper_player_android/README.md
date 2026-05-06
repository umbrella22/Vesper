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
| System playback / notification controls     | ✅ MediaSession + foreground service |
| Android Cast                                | ✅ Optional `vesper_player_cast` package |

## Technical Notes

- Playback backend: Media3 ExoPlayer behind the `VesperPlayerController` Kotlin facade
- Flutter integration: `MethodChannel` and `EventChannel` using `io.github.ikaros.vesper_player`
- View embedding: `AndroidView` with view type `io.github.ikaros.vesper_player/platform_view`
- Render path: `VesperPlayerController.create(renderSurfaceKind: ...)` selects the Android surface for Flutter playback. `auto` maps to `TextureView` for overlay and gesture compatibility. Use `surfaceView` only when the host explicitly wants the native Android HDR / high-frame-rate fullscreen path and can keep Flutter overlays safe.
- Runtime snapshot: exposes the currently active adaptive video variant through `controller.snapshot.effectiveVideoTrackId`
- Runtime observation: also exposes `controller.snapshot.videoVariantObservation`, derived from ExoPlayer's active `videoFormat` bitrate and rendered size
- System playback: `configureSystemPlayback` binds the active ExoPlayer to a Media3 `MediaSessionService`, starts a media playback foreground service while audio is playing, exposes default 10-second seek back / play-pause / seek forward media actions through MediaSession button preferences, filters seek commands when `showSeekActions` is disabled, and clears the session on pause / stop / dispose
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

## Optional Android Cast

Android Cast lives in the separate `vesper_player_cast` Flutter package and the
optional `vesper-player-kit-cast` Android module. This keeps Google Play
Services and Cast Framework dependencies out of the default player package.

For local workspace builds, include `:vesper-player-kit-cast` beside
`:vesper-player-kit` in the host Android Gradle settings. The Cast module
contributes a default `VesperCastOptionsProvider` that uses Google's Default
Media Receiver. Hosts that need a custom receiver can override the manifest
meta-data key
`io.github.ikaros.vesper.player.android.cast.RECEIVER_APPLICATION_ID`.

Cast V2 supports remote `http` / `https` HLS, DASH, and progressive sources.
Local files, `content://` sources, DRM, request headers with the default
receiver, offline assets, and custom receiver behavior are outside this scope.

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
