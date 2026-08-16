# Platform Packages

`lib/` is the distribution layer for Vesper's platform host kits and Flutter
packages. It contains Kotlin, Swift, and Dart wrappers around the shared Rust
runtime. Rust crates live under [`../crates`](../crates/); no Rust crate belongs
in this directory.

## Runtime Flow

```text
Shared Rust runtime and models
  +-> JNI bridge -> Android host kit -> Media3 -> Android AARs
  +-> C / Swift bridge -> iOS host kit -> AVPlayer -> SwiftPM / XCFramework

Flutter Dart API
  +-> MethodChannel / EventChannel -> Android host kit
  +-> MethodChannel / EventChannel -> iOS host kit
```

Media3 and AVPlayer remain the production mobile playback engines. The Rust
layer defines shared source, timeline, track, policy, download, event, error,
capability, and diagnostic semantics. Flutter adapts the same native host kits;
it does not call JNI or C FFI directly.

## Package Selection

### Android

Android packages are Gradle modules under [`android/`](android/README.md) and
produce AAR artifacts.

| Module | Role |
| --- | --- |
| `vesper-player-kit` | Core controller, source and track APIs, download manager, Media3 host, and `libvesper_player_android.so` |
| `vesper-player-kit-external-playback` | Optional Google Cast, DLNA / UPnP, local relay, and route UI integration |
| `vesper-player-kit-ffmpeg-runtime` | Optional shared FFmpeg runtime for FFmpeg-backed Android extensions |
| `vesper-player-kit-decoder-mediacodec` | Experimental MediaCodec decoder plugin for the explicit SDK-managed native-frame route |
| `vesper-player-kit-source-normalizer-ffmpeg` | Experimental FFmpeg SourceNormalizer plugin and verified embedded registry metadata |
| `vesper-player-kit-frame-processor-diagnostic` | Experimental FrameProcessor diagnostic plugin |
| `vesper-player-kit-compose` | Optional Compose lifecycle and surface adapter without opinionated controls |
| `vesper-player-kit-compose-ui` | Optional Compose `VesperPlayerStage` controls built on the Compose adapter |

View-based hosts can depend on `vesper-player-kit` alone. Compose hosts add the
adapter, and add the UI module only when the packaged stage fits the product.
External playback and FFmpeg-backed extensions remain explicit dependencies.

GitHub release staging can produce the complete AAR set. Stable Maven
publication contains the core kit, both Compose modules, external playback,
and its transitive FFmpeg runtime. SourceNormalizer, Decoder, FrameProcessor,
and offline-remux plugin distribution remain outside that default dependency
closure. The Android package README and release workflow define the current
published set.

### iOS

[`ios/VesperPlayerKit`](ios/VesperPlayerKit/README.md) is the core Swift package
and XCFramework project. The source-local package exposes three products:

| Product | Role |
| --- | --- |
| `VesperPlayerKit` | Public `@MainActor` controller, source, track, download, diagnostics, and AVPlayer host APIs |
| `VesperPlayerKitUI` | Optional SwiftUI `VesperPlayerStage` controls |
| `VesperPlayerFFI` | Low-level binary product consumed by the host kit and bridge shim |

The remote binary package at `umbrella22/VesperPlayerKit` intentionally exports
only `VesperPlayerKit` and `VesperPlayerKitUI`. Its binary `VesperPlayerKit`
target already contains the native bridge closure; remote consumers, including
`vesper_player_ios`, must not request a separate `VesperPlayerFFI` product.

`ios/VesperPlayerOptionalPlugins` exposes seven direct binary products. The
three `VesperFFmpeg*` products are shared FFmpeg component dependencies;
`VesperPlayerRemuxFfmpegPlugin` provides optional post-download remux. The
SourceNormalizer, VideoToolbox decoder, and FrameProcessor products are
experimental plugin surfaces. There is no aggregate umbrella product.

### Flutter

Flutter is a federated package family. The main package registers Android and
iOS only; no Flutter desktop implementation is published.

| Package | Role |
| --- | --- |
| [`vesper_player`](flutter/vesper_player/README.md) | Public Dart controller, source, snapshot, event, download, and native view API |
| [`vesper_player_platform_interface`](flutter/vesper_player_platform_interface/README.md) | Shared public DTOs and federated platform contract |
| [`vesper_player_android`](flutter/vesper_player_android/README.md) | Android MethodChannel / EventChannel adapter over the Android host kit |
| [`vesper_player_ios`](flutter/vesper_player_ios/README.md) | iOS MethodChannel / EventChannel adapter over `VesperPlayerKit` |
| [`vesper_player_external_playback`](flutter/vesper_player_external_playback/README.md) | Optional Android Cast / DLNA and relay integration |
| [`vesper_player_source_normalizer_ffmpeg`](flutter/vesper_player_source_normalizer_ffmpeg/README.md) | Experimental optional Android/iOS SourceNormalizer artifact package |
| [`vesper_player_ui`](flutter/vesper_player_ui/README.md) | Experimental optional controls, Stage UI, and AirPlay route UI |

The platform interface is the only home for public cross-platform Dart DTOs.
Platform packages serialize and adapt those DTOs without introducing parallel
public models.

## Supported Platform Floor

| Platform | Minimum | Distributed architecture |
| --- | --- | --- |
| Android | API 26+, Kotlin 2.x | `arm64-v8a` |
| iOS | iOS 17+, Xcode 16+ | arm64 device and Apple Silicon Simulator |
| Flutter | Dart 3.6+, Flutter 3.44+ | Android and iOS through the native host kits |

Older mobile OS versions, 32-bit Android, Intel Android ABIs, and Intel iOS
Simulator are outside the current product boundary.

## Build and Release Entry Points

Run platform packaging through the Rust `vesper` CLI from the repository root.
Local Android commands use the selected Android project's cached Gradle
distribution and service home; repository-root `.gradle/` state is not used.

```sh
# Android core and optional release staging
./scripts/vesper android aar
./scripts/vesper android stage-release
VESPER_ANDROID_INCLUDE_OPTIONAL_PLUGINS=1 \
  ./scripts/vesper android stage-release

# iOS core package and verified release staging
./scripts/vesper ios ffi
./scripts/vesper ios kit-xcframework
./scripts/vesper ios stage-release /tmp/vesper-ios-release
./scripts/vesper ios verify-release /tmp/vesper-ios-release --scope core

# Flutter local development and publish staging
./scripts/vesper flutter local-overrides
./scripts/vesper flutter stage-pub /tmp/vesper-flutter-pub
./scripts/vesper flutter pub-dry-run /tmp/vesper-flutter-pub
```

Optional FFmpeg-backed artifacts retain FFmpeg's license, notices,
corresponding source, configure metadata, and LGPL relinking obligations. They
are not relicensed as part of the Apache-2.0 Vesper source distribution.

## Player Stage UI

The optional Stage UI packages provide the controls overlay, timeline,
fullscreen and menu entry points, auto-hide behavior, playback surface
composition, and gestures inside the player area:

- Android: `vesper-player-kit-compose-ui`
- iOS: `VesperPlayerKitUI`
- Flutter: `vesper_player_ui`

`VesperPlayerStageSheet` reports five sheet destinations: menu, quality, audio,
subtitle, and speed. The host supplies the actual sheet content and page-level
business logic.

### Gesture Contract

- Tap toggles the control overlay.
- Double tap toggles play and pause.
- Horizontal drag previews and commits a timeline seek.
- Vertical drag on the left adjusts brightness through host callbacks.
- Vertical drag on the right adjusts volume through host callbacks.
- Long press temporarily selects 2x playback and restores the previous rate on
  release or cancellation.

Brightness and volume remain host capabilities. Missing callbacks disable the
corresponding gesture. `pictureInPicturePresentation` hides the custom overlay
and disables Stage gestures while system Picture in Picture controls are
active.

### Android Stage

The UI module depends on the Compose adapter, which depends on the core kit.
Register the source modules in `settings.gradle.kts`:

```kotlin
include(":vesper-player-kit")
include(":vesper-player-kit-compose")
include(":vesper-player-kit-compose-ui")
```

Add the UI dependency in the consuming module's `build.gradle.kts`:

```kotlin
dependencies {
    implementation(project.dependencies.project(":vesper-player-kit-compose-ui"))
}
```

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

Android `VesperPlayerStage` creates `VesperPlayerSurface` internally. A host
must not place another player surface behind it.

### iOS Stage

The local Swift package provides both the core and UI products. Add the package
dependency to the host package definition:

```swift
.package(path: "lib/ios/VesperPlayerKit")
```

Add both products to the consuming target dependencies:

```swift
.product(name: "VesperPlayerKit", package: "VesperPlayerKit")
.product(name: "VesperPlayerKitUI", package: "VesperPlayerKit")
```

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
    onSetPlaybackRate: { controller.setPlaybackRate($0) },
    onToggleFullscreen: { toggleFullscreen() },
    onOpenSheet: { sheet in activeSheet = sheet },
    currentBrightnessRatio: deviceControls.currentBrightnessRatio,
    onSetBrightnessRatio: deviceControls.setBrightnessRatio,
    currentVolumeRatio: deviceControls.currentVolumeRatio,
    onSetVolumeRatio: deviceControls.setVolumeRatio
)
```

iOS receives the playback surface as `AnyView`; the host constructs that
surface and retains controller lifecycle responsibility.

### Flutter Stage

Repository development first generates the package-family path overrides. This
command updates repository packages and `examples/flutter-host`; it does not
modify an external host:

```sh
./scripts/vesper flutter local-overrides
```

The repository host then depends on the main package and optional UI package:

```yaml
dependencies:
  vesper_player:
    path: ../../lib/flutter/vesper_player
  vesper_player_ui:
    path: ../../lib/flutter/vesper_player_ui
```

An external host must also provide root-level overrides for the federated
package family as documented in
[`flutter/vesper_player/README.md`](flutter/vesper_player/README.md#installation).

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

`VesperPlayerStage` contains `VesperPlayerView`. The host supplies an
implementation of `VesperPlayerDeviceControls` when brightness and volume
gestures are required.

## Host Responsibilities

- Create and dispose the player controller.
- Apply fullscreen state, orientation policy, and system bar behavior.
- Render menu, quality, audio, subtitle, and speed sheets.
- Bridge brightness and volume reads and writes, including platform permission
  handling.
- Implement page-level autoplay, playlist, download, and resilience behavior.

Runnable integration examples live under
[`../examples/android-compose-host`](../examples/android-compose-host),
[`../examples/ios-swift-host`](../examples/ios-swift-host), and
[`../examples/flutter-host`](../examples/flutter-host).
