# vesper_player_external_playback

Optional external playback plugin for Vesper Player Flutter hosts.

The Android implementation provides:

- Google Cast control for an already selected Cast route.
- DLNA / UPnP AV device discovery and playback control.
- Local HTTP relay for local media, content URIs, and sources that require
  request headers.

Cast route selection still uses the system Cast route button. DLNA devices are
reported through `VesperExternalPlaybackController.routes`.

Use `VesperExternalRouteIconButton` inside a player-stage action slot on Android
to surface the system Cast route picker as a full icon-sized native hit area.
The Android platform view follows the surrounding Flutter `Theme` brightness by
default and passes an opaque light or dark MediaRouter theme to Cast so route
dialog contrast calculation never depends on a transparent host background.
`VesperExternalRouteButton` remains available for existing integrations.
