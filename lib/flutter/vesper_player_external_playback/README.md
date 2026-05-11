# vesper_player_external_playback

Optional external playback plugin for Vesper Player Flutter hosts.

The Android implementation provides:

- Google Cast control for an already selected Cast route.
- DLNA / UPnP AV device discovery and playback control.
- Local HTTP relay for local media, content URIs, and sources that require
  request headers.

Cast route selection still uses the system Cast route button. DLNA devices are
reported through `VesperExternalPlaybackController.routes`.
