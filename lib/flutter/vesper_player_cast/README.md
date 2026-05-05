# vesper_player_cast

Optional Google Cast sender integration for Flutter hosts that use
`vesper_player`.

This package is intentionally separate from `vesper_player_android` so apps that
do not need Cast do not pull in Google Play Services or Cast Framework
dependencies.

## What It Provides

- `VesperCastButton` — Android `MediaRouteButton` wired through
  `CastButtonFactory`
- `VesperCastController` — headless load, play, pause, stop, seek, and session
  event APIs
- Android host-kit dependency on `vesper-player-kit-cast`

## Android Host Setup

For local workspace builds, include both Android modules in the host app
settings:

```kotlin
include(":vesper-player-kit")
include(":vesper-player-kit-cast")

project(":vesper-player-kit").projectDir =
    file("path/to/rust-player-sdk/lib/android/vesper-player-kit")
project(":vesper-player-kit-cast").projectDir =
    file("path/to/rust-player-sdk/lib/android/vesper-player-kit-cast")
```

The Cast module contributes a default
`com.google.android.gms.cast.framework.OPTIONS_PROVIDER_CLASS_NAME` manifest
entry. It uses Google's Default Media Receiver unless the host sets:

```xml
<meta-data
    android:name="io.github.ikaros.vesper.player.android.cast.RECEIVER_APPLICATION_ID"
    android:value="YOUR_RECEIVER_APP_ID" />
```

## Usage

```dart
final cast = VesperCastController();

cast.events.listen((event) {
  // Load the current player source when a Cast session starts or resumes.
});

VesperCastButton()

await cast.loadFromPlayer(
  player: controller,
  source: VesperPlayerSource.hls(
    uri: 'https://example.com/stream.m3u8',
    label: 'Sample video',
  ),
  metadata: const VesperSystemPlaybackMetadata(title: 'Sample video'),
);
```

## V2 Scope

Cast V2 supports remote `http` / `https` HLS, DASH, and progressive sources
with the default Google receiver. Local files, `content://` sources, offline
assets, DRM, request headers with the default receiver, and full custom receiver
behavior are not implemented in this package.
