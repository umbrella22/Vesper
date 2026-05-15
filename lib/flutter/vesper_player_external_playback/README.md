# vesper_player_external_playback

Optional external playback plugin for Vesper Player Flutter hosts.

The Android implementation provides:

- Google Cast control for an already selected Cast route.
- DLNA / UPnP AV device discovery and playback control.
- Local HTTP relay for local media, content URIs, and sources that require
  request headers.
- Optional relay FFmpeg adaptation through the Android
  `vesper-player-kit-external-playback` facade and shared
  `vesper-player-kit-ffmpeg-runtime` AAR.

Cast route selection still uses the system Cast route button. DLNA devices are
reported through `VesperExternalPlaybackController.routes`, which delegates to
the Kotlin facade at
`io.github.ikaros.vesper.player.android.external.VesperExternalPlaybackController`.

Use `VesperExternalRouteIconButton` inside a player-stage action slot on Android
to surface the system Cast route picker as a full icon-sized native hit area.
The Android platform view follows the surrounding Flutter `Theme` brightness by
default and passes an opaque light or dark MediaRouter theme to Cast so route
button and dialog contrast calculation never depends on a transparent host
background.
`VesperExternalRouteButton` remains available for existing integrations.

## Android Host Requirements

The Android plugin manifest contributes internet, network-state, Wi-Fi, nearby
Wi-Fi devices, multicast, and Cast options-provider declarations. It does not
enable cleartext traffic globally.

DLNA discovery and relay playback can require app-owned cleartext HTTP access
for local-network device descriptions and tokenized relay URLs. Apps that use
those features must opt in from the host app manifest or an Android network
security configuration:

```xml
<application
    android:usesCleartextTraffic="true">
</application>
```

Hosts with stricter policy should use a network security configuration that
only allows the local hosts required by their discovery and relay flows.
