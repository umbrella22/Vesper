# Native mobile controls and example parity

The Harmony controls follow the shared mobile behavior exposed by the Android
Compose and iOS SwiftUI Stage implementations. Their layout uses HarmonyOS
symbols, native sheets, sliders, switches, Navigation and Tabs. This comparison
defines implementation scope; runtime acceptance belongs to the artifact-specific
verification records.

| Shared behavior | Harmony implementation |
| --- | --- |
| Playback and timeline | Play, pause, replay, ten-second skips, VOD scrubber and time; loaded/seekable ranges stay separate from buffering percentage |
| Control layouts | Compact inline and expanded fullscreen defaults; standard remains available; measured overflow moves shortcuts to More |
| Customization | Per-role system resources or WrappedBuilder icons, eight button styles, reusable public buttons, slots and controlled/local presentation |
| Header and shortcuts | Source title, timeline/loading state, lock, settings, current speed/quality, subtitles and PiP |
| Menus | Content-sized native sheets, bounded wide panel, fixed header, selected-row scrolling, nested back and native close |
| Selection feedback | Selected tracks/Off/bitrate mode, requested outcome, current dimensions and effective bitrate |
| Video interaction | Single tap visibility, double tap playback, hold for temporary 2× with exact prior-rate restoration |
| Drag interaction | Axis arbitration, VOD seek preview/commit, continuous volume/brightness and visual HUD |
| Cancellation | Epoch changes, geometry/layout/fullscreen, locking, settings, hidden presentation and disappearance |
| Presentation lifetime | A retained controller/binding/Surface; gestures suppressed during native PiP or another page |
| Playback workflow | File/HLS/DASH/URL actions, bounded queue preview and complete queue manager, previous/next and custom removal |
| Playback policy | Requested/effective/unsupported parameters; resource retry separate from opt-in bounded activation recovery |
| Language preferences | One-time exact/base language selection after catalog readiness; no native automatic reselection claim |
| Captions | Validated basic style, measured video/header/control geometry, bounded long-text overflow and Off/seek/epoch cleanup |
| Window policy | Shared native-window leases, visible-playing-video screen-on policy, temporary brightness and baseline restoration |
| Preload and cache | Per-source/next preload, cancellation, bytes/reuse outcome, separate memory/disk budgets and disk clearing |
| System integration | AVSession/background-audio toggle, PiP start/automatic preference and actual native state |
| Offline workflow | Credential-free hashed partial resume with source/version verification, offline playback/deletion, user titles and export eligibility; streamed originals/ZIP up to 256 MiB |
| Diagnostics | Player/resource/selection/capability state, system/PiP, preload/cache/policy, bounded history and redacted report |
| Appearance | Native System/Light/Dark, warm Vesper Stage or explicit system accent, reduced motion and independent inline/fullscreen layout |

The native execution contract limits a few actions. Harmony audio/subtitle
automatic reselection is unavailable; explicit track choices and subtitle Off
work. Numeric seek/rate apply to VOD. Live DVR and live-edge navigation require
a verified seekable live route and remain unavailable, including on API 26.
API 26 track filters and time-range queries retain their separate runtime gates.
AVPlayer controls buffering thresholds and decoder selection; the policy UI
reports unsupported tuning instead of claiming it took effect.

Android Cast/DLNA, iOS AirPlay/FairPlay, experimental native-frame/plugin workflows
and DRM offline assets do not become Harmony capabilities through this UI work.
The platform guide defines supported media profiles and evidence boundaries.

Source anchors for the comparison:

- Android: `lib/android/vesper-player-kit-compose-ui/` Stage, gestures, overlay and
  sheets; `examples/android-compose-host/` source, queue, resilience and downloads.
- iOS: `lib/ios/VesperPlayerKit/Sources/VesperPlayerKitUI/Stage/` controls, gestures,
  scrubber and models; `examples/ios-swift-host/Sources/` player and download sections.
- Harmony: `Stage.ets`, `StageSettings.ets`, `Interaction.ets`, and
  `examples/harmony-arkui-host/entry/src/main/ets/` panels and model.
