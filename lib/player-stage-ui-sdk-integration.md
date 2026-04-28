# Player Stage UI SDK Integration Guide

The player stage UI has been extracted from the examples into optional UI SDKs. The core player SDK remains responsible only for playback, render hosting, and state models. The stage UI SDK owns the overlay, timeline, fullscreen button, menu entry, auto-hide behavior, and gestures inside the playback area.

## Package Boundaries

- Android: `lib/android/vesper-player-kit-compose-ui`
- iOS: `VesperPlayerKitUI` product in the `VesperPlayerKit` Swift Package
- Flutter: `lib/flutter/vesper_player_ui`

The host app still owns sheet contents such as quality, audio track, subtitle, and playback speed lists. The stage UI only reports which panel should open through `VesperPlayerStageSheet`.

## Gestures

- Tap the playback area: show or hide controls.
- Double tap the playback area: play / pause.
- Drag horizontally in the playback area: preview and commit timeline seek by the current drag position.
- Drag vertically on the left half: adjust screen brightness.
- Drag vertically on the right half: adjust system volume.
- Long press the playback area: temporarily switch to 2x, then restore the previous speed when released or canceled.

Brightness and volume are host permissions and platform capabilities, so the UI SDK does not call system APIs directly. The host injects read and write logic through callbacks; when those callbacks are not provided, the corresponding gestures are ignored automatically.
Brightness, volume, and temporary speed changes all show compact horizontal feedback. Brightness and volume feedback includes an icon, progress bar, and percentage. Temporary speed feedback shows an icon and the current speed. Feedback dismisses automatically after about 520 ms.

## Android Integration

Add the Compose UI package to the host project:

```kotlin
include(":vesper-player-kit")
include(":vesper-player-kit-compose")
include(":vesper-player-kit-compose-ui")

project(":vesper-player-kit").projectDir = file("../../lib/android/vesper-player-kit")
project(":vesper-player-kit-compose").projectDir = file("../../lib/android/vesper-player-kit-compose")
project(":vesper-player-kit-compose-ui").projectDir = file("../../lib/android/vesper-player-kit-compose-ui")
```

```kotlin
dependencies {
    implementation(project(":vesper-player-kit-compose-ui"))
}
```

Minimal usage:

```kotlin
VesperPlayerStage(
    controller = controller,
    uiState = uiState,
    controlsVisible = controlsVisible,
    pendingSeekRatio = pendingSeekRatio,
    isPortrait = isPortrait,
    trackCatalog = controller.trackCatalog,
    trackSelection = controller.trackSelection,
    onControlsVisibilityChange = { controlsVisible = it },
    onPendingSeekRatioChange = { pendingSeekRatio = it },
    onOpenSheet = { sheet -> activeSheet = sheet },
    onToggleFullscreen = { toggleFullscreen() },
    currentBrightnessRatio = { deviceControls.currentBrightnessRatio() },
    onSetBrightnessRatio = { deviceControls.setBrightnessRatio(it) },
    currentVolumeRatio = { deviceControls.currentVolumeRatio() },
    onSetVolumeRatio = { deviceControls.setVolumeRatio(it) },
)
```

`VesperPlayerStage` uses `VesperPlayerSurface` internally. The host should not stack another player surface under it.

## iOS Integration

The Swift Package exposes both the core and UI products:

```swift
.package(path: "lib/ios/VesperPlayerKit")
```

```swift
.product(name: "VesperPlayerKit", package: "VesperPlayerKit")
.product(name: "VesperPlayerKitUI", package: "VesperPlayerKit")
```

Minimal usage:

```swift
import SwiftUI
import VesperPlayerKit
import VesperPlayerKitUI

VesperPlayerStage(
    surface: AnyView(PlayerSurfaceContainer(controller: controller)),
    uiState: controller.uiState,
    trackCatalog: controller.trackCatalog,
    trackSelection: controller.trackSelection,
    effectiveVideoTrackId: controller.effectiveVideoTrackId,
    fixedTrackStatus: controller.fixedTrackStatus,
    controlsVisible: $controlsVisible,
    pendingSeekRatio: $pendingSeekRatio,
    isCompactLayout: isCompactLayout,
    isFullscreen: isFullscreen,
    onSeekBy: { controller.seek(by: $0) },
    onTogglePause: { controller.togglePause() },
    onSeekToRatio: { controller.seek(toRatio: $0) },
    onSeekToLiveEdge: { controller.seekToLiveEdge() },
    onToggleFullscreen: { toggleFullscreen() },
    onOpenSheet: { sheet in activeSheet = sheet },
    currentBrightnessRatio: deviceControls.currentBrightnessRatio,
    onSetBrightnessRatio: deviceControls.setBrightnessRatio,
    currentVolumeRatio: deviceControls.currentVolumeRatio,
    onSetVolumeRatio: deviceControls.setVolumeRatio
)
```

Apple artifacts are delivered as arm64-only binaries. If the host project still builds an x86_64 simulator slice, exclude `x86_64` in the project settings to avoid linker lookups for x86 binaries that are no longer produced.

## Flutter Integration

Add the UI package:

```yaml
dependencies:
  vesper_player:
    path: ../../lib/flutter/vesper_player
  vesper_player_ui:
    path: ../../lib/flutter/vesper_player_ui
```

Minimal usage:

```dart
import 'package:vesper_player/vesper_player.dart';
import 'package:vesper_player_ui/vesper_player_ui.dart';

VesperPlayerStage(
  controller: controller,
  snapshot: snapshot,
  isPortrait: isPortrait,
  sheetOpen: activeSheet != null,
  onOpenSheet: (sheet) => activeSheet = sheet,
  onToggleFullscreen: toggleFullscreen,
  deviceControls: deviceControls,
)
```

Implement `VesperPlayerDeviceControls` for brightness and volume integration:

```dart
class HostDeviceControls implements VesperPlayerDeviceControls {
  @override
  Future<double?> currentBrightnessRatio() async => null;

  @override
  Future<double?> setBrightnessRatio(double ratio) async => null;

  @override
  Future<double?> currentVolumeRatio() async => null;

  @override
  Future<double?> setVolumeRatio(double ratio) async => null;
}
```

Flutter iOS / macOS hosts that inherit this repository's example configuration exclude x86_64 in build settings. New host apps should also keep the arm64-only Apple binary assumption.

## Host Responsibilities

- Create and dispose `VesperPlayerController`.
- Handle fullscreen state, orientation policy, and system bars.
- Provide the quality, audio track, subtitle, and playback speed sheets represented by `VesperPlayerStageSheet`.
- Provide brightness and volume read/write capability, including platform permissions or system control bridges.
- Own page-level business logic such as autoplay, playlists, downloads, and resilience policy.
