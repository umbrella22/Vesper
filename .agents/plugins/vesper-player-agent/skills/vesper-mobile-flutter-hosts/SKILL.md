---
name: vesper-mobile-flutter-hosts
description: Use when changing Vesper Android host kits, iOS VesperPlayerKit, Flutter federated packages, MethodChannel/EventChannel behavior, system playback, AirPlay, Cast, DLNA, relay, surfaces, PlatformView, SurfaceView, TextureView, SwiftUI/UIKit, or example mobile hosts.
metadata:
  short-description: Mobile and Flutter host boundaries
---

# Vesper Mobile and Flutter Hosts

## Load First

- `../../references/knowledge-map.md`
- `../../references/repository-memory.md`
- `../../references/plugin-runtime-contract.md` for plan, scope, slot, and
  participation boundaries
- `../../references/platform-hosts.md`
- `../../references/defensive-boundaries.md` for lifecycle/channel changes
- The current host-kit and Flutter package READMEs when a checkout is available
- For command selection and release evidence: `$vesper-validation-playbook`

## Host Kit Boundaries

Android public API centers on `VesperPlayerController`, `VesperPlayerSource`,
and track selection. Raw JNI, bridge payloads, native binding interfaces, and
backend internals should be internal or restricted.

iOS public API belongs in `VesperPlayerKit`, stays `@MainActor`, and should not
expose raw bridge details. Shared `AVAudioSession` state must respect active
owner and platform interruption or route-change rules.

Plugin references select an already embedded identity from an immutable runtime
plan. Catalog import and resolution do not grant a host capability, and plugin
availability or preflight must not be reported as playback participation.

Flutter mobile packages call host kits through MethodChannel and EventChannel.
Do not call JNI or C FFI directly from Dart on Android or iOS.

## Lifecycle Guardrails

- Session-like host-kit objects should use a sentinel initial state, synchronized
  or atomic creation, idempotent dispose/close, stale-handle rejection, and
  cleanup when constructor or registration work fails after native creation.
- Do not hold Kotlin monitors, synchronized blocks, Swift locks, global
  registries, or channel state locks across socket creation/close, file I/O,
  executor shutdown, platform callbacks, or long JNI/FFmpeg/plugin calls.
- Swift async APIs must not be bridged to synchronous callers with unbounded
  semaphores. Use timeout, cancellation, and fallback/error behavior when a
  synchronous boundary cannot be avoided.
- EventChannel and MethodChannel lifecycle changes should be synchronous when
  Flutter already calls on the main thread; avoid delayed Task assignment windows
  that can send events to a stale sink.
- Cross-language warnings, capability records, diagnostics, and enum decoders
  should preserve or report unknown raw values instead of silently falling back
  to an unrelated supported value.

## Flutter Contract

- `vesper_player_platform_interface` owns public DTOs.
- Platform packages serialize, adapt, and report capabilities.
- Main package exposes controller, view, state, and event semantics.
- UI package owns reusable controls and stage helpers.
- Example host demonstrates integration and regression behavior only.

Do not expose `SurfaceView`, `TextureView`, `AVPlayerLayer`, JNI, or C ABI as
Dart public API promises. Express platform differences through capabilities,
snapshots, unsupported errors, or internal strategy.

## Rendering

Separate two decisions:

- Flutter embedding path: hole punch, PlatformView, sibling layer, overlay host.
- Native video surface: Android `SurfaceView` or `TextureView`, iOS native view
  or layer, macOS experimental host.

Android `SurfaceView` is the high-fidelity target for fixed/fullscreen HDR,
high refresh, and low power. `TextureView` or equivalent native host view is a
valid stable fallback for scrolling, clipping, rounded corners, transforms, and
animation-heavy scenes.

Media3 DirectNative remains the Android audio clock and PCM sink. A Native
AudioProcessor can run only through an explicit SDK-managed PCM route; it does
not intercept Media3 audio because an optional artifact is present. iOS
AVPlayer DirectNative has the same boundary. Keep `PreservePitch`/
`FollowRate`, rate limits, PTS, and discontinuity diagnostics in the runtime
model, not in Flutter-specific surface code.

## Channel Lifecycle

- Dart implementation constructors should not touch default binary messenger
  handler registration unless the package has proven it is safe.
- Prefer lazy, idempotent MethodChannel handler registration before first real
  platform call when auto-registration can run before Flutter binding setup.
- Default EventChannel streams shared by many default controllers should use a
  static broadcast stream and shared latest cache.
- Custom test channels may remain instance-local.

## High-Frequency Timeline Updates

- Use `sampleTimeline` for periodic progress polling and patch only timeline
  state. A polling tick must not rebuild track, subtitle, viewport, plugin
  diagnostic, source identity, or error state.
- Fence awaited samples with the controller snapshot revision. Seek, source
  change, full refresh, stop, error, and dispose make an older result stale.
- Use bounded backoff on sample failure and keep the last authoritative state;
  do not publish a playback error for a progress diagnostic failure.
- Keep full refresh for explicit/command/source reconciliation. Require physical
  Profile/Release measurements before claiming high-refresh performance; a
  timeline-only implementation and unit tests prove state isolation, not device
  acceptance.

## Track And Subtitle Commands

- Preserve track support status, reason, source, playback path, catalog revision,
  and unknown raw wire values through Kotlin/Swift/Dart mapping.
- A fixed-track command may carry the catalog revision observed by UI, but the
  host must re-read current platform support before applying. Android rejects
  explicit unsupported/exceeds-capability tracks before changing the player;
  iOS fixed-track remains best-effort.
- Android subtitle Track selection waits within one total deadline for the exact
  stable-ID TEXT target in current Media3 tracks. Reuse the catalog/override
  resolver and never compare raw `Format.id` as public identity.
- Confirm selection from exact current parameters readback. Track generations
  only wake a recheck; renderer-active track and cue delivery are later states.
  Preserve command/source/item epochs, supersede, source switch, and dispose
  fences.

## Diagnostic Event Handling

- Forward Pipeline EventHook report events exactly once without mutating the
  controller snapshot or `lastError`; preserve final dispose reports and reject
  post-dispose events.
- Treat Media3 dropped frames as benchmark data for the current generation only.
  When benchmark is disabled, do not allocate event attributes, log an
  unconditional warning, or change playback policy.

When forwarding plugin diagnostics, preserve plan fingerprint, session/item/
source/playback generation, transport, workload, participation stage, fallback,
and quarantine reason. Do not collapse `policy rejected`, `not found`, and
`fallback` into one platform exception.

## External Playback

External playback is route/session infrastructure, not just a cast button.

- Android external playback covers Cast, DLNA, relay, and relay format
  adaptation through the current host-kit/package integration surface. Do not
  create new distributable AAR modules unless current code and root `AGENTS.md`
  support that split.
- iOS uses system AirPlay route picker; do not add programmatic AirPlay or DLNA
  to iOS without a separate design.
- Relay is infrastructure for headers, local files, `content://`, and non-public
  URLs. It must not leak cookies or authorization in remote URLs or logs.
- SDK must not embed site-specific business rules.
- Unsupported DRM, encrypted DASH, unsupported DASH layouts, or inaccessible
  sources should return explicit diagnostics.

## Validation

Use `$vesper-validation-playbook` to select the matching host-kit, Flutter
package, public API, example, artifact, and device checks. For local Android
commands, resolve the project-cached Gradle executable according to the current
checkout's root rules and keep `GRADLE_USER_HOME` inside that Android project.
