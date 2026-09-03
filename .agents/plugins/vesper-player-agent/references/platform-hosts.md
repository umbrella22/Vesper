# Vesper Platform Host Contract

## Product Floor And Execution

- Android production playback uses Media3/ExoPlayer on API 26+ arm64-v8a.
- iOS production playback uses AVPlayer on iOS 17+ arm64 devices and Apple
  Silicon Simulator.
- Android and iOS remain native-first. Rust owns shared source, timeline,
  track, policy, download, event, error, capability, and diagnostic semantics;
  platform layers own actual player execution and surfaces.
- Protected media and DRM stay inside native platform players. They do not enter
  Rust media processing, plugin, download, preload, remux, or relay routes.
- Plugin runtime selection is explicit and host-owned: a verified
  `PluginReference` names the plugin identity and `Native` or `Wasm` transport;
  catalog, plan, and scope state remain internal runtime records. Installing an
  optional artifact does not opt a native mobile player into a plugin route.

## Android Host Kit

- Public APIs center on `VesperPlayerController`, `VesperPlayerSource`,
  `VesperTrackSelection`, `VesperDownloadManager`, and `VesperPlayerSurface`.
- JNI, bridge payloads, native binding interfaces, and plugin library paths are
  internal. Public plugin selection uses `VesperPluginReference`.
- `SurfaceView` is the default high-fidelity video surface for HDR, high refresh,
  and low power. `TextureView` is a fallback for scrolling, clipping, rounded
  corners, transforms, and animation-heavy layouts.
- Do not hold Kotlin monitors or synchronized blocks across file/socket I/O,
  executor shutdown, or long JNI/FFmpeg/plugin calls.
- Android Media3 DirectNative owns decoded audio, clock, and A/V scheduling.
  Native `AudioProcessor` PCM chains and SDK-managed `FrameProcessor` lanes are
  separate experimental routes and must not be wired into the normal Media3
  path by plugin discovery alone.
- Local Gradle work uses the project-cached distribution and project-local
  `GRADLE_USER_HOME`; do not let `gradlew` download a distribution. One online
  dependency refresh is allowed, then the first network failure triggers one
  identical `--offline` rerun from that cache.

## iOS Host Kit

- Public APIs belong to `VesperPlayerKit`, are `@MainActor`, and must not expose
  bridge or raw native ABI details.
- Distribution is an arm64 device and Apple Silicon Simulator XCFramework or
  Swift Package. Runtime code download is not part of mobile integration.
- Swift async APIs must remain async. A synchronous boundary must have a
  timeout, cancellation handling, and an explicit fallback/error result; never
  wait indefinitely on a semaphore.
- AVAudioSession, route changes, interruptions, background playback, and surface
  ownership belong to the host kit.
- iOS AVPlayer DirectNative likewise owns PCM output, clock, and A/V timing.
  Native AudioProcessor and native-frame capabilities remain opt-in SDK-managed
  experiments; protected FairPlay material never crosses those boundaries.

## Flutter Packages

- `vesper_player_platform_interface` owns public Dart DTOs and channel
  contracts. Platform packages serialize and adapt; they must not invent
  parallel public DTO families.
- Android and iOS Flutter packages call host kits through MethodChannel and
  EventChannel. Dart must not call JNI or C FFI directly.
- Keep `SurfaceView`, `TextureView`, `AVPlayerLayer`, JNI, C ABI, and native
  handles out of the public Dart API. Represent differences as capabilities,
  snapshots, diagnostics, or explicit unsupported errors.
- Channel registration is lazy and idempotent where possible. Shared default
  event streams must remain safe for multiple controller instances.

## Timeline Sampling

- Periodic progress refresh should call `sampleTimeline` and patch only the
  timeline field. It must not change track, subtitle, viewport, plugin
  diagnostics, source identity, or `lastError`.
- Record a controller snapshot revision before awaiting the platform result.
  Seek, source replacement, full refresh, stop, terminal error, and dispose must
  invalidate an older in-flight sample.
- Sampling failure keeps the last authoritative snapshot and uses bounded
  backoff. It is not a playback failure.
- Keep full refresh for explicit refresh, source/recovery transitions, command
  completion reconciliation, and non-timeline state. Do not recreate timeline
  normalization in Dart with a local clock unless device evidence proves the
  platform sample still misses the frame budget.

## Track Capability And Selection

- Track DTOs preserve support status, reason, source, playback path, bounded
  diagnostics, and unknown raw wire values. `unknown` is not `unsupported`.
- Track catalogs carry a monotonic revision. Pass an expected revision as a
  one-command precondition and re-read the current platform tracks before
  applying fixed selection.
- Android must not create a Media3 override for an explicitly
  `exceedsCapabilities` or `unsupported` track. A correlated runtime decode
  rejection may advance the session-local catalog revision and fall back to
  automatic selection without becoming a permanent device blacklist.
- iOS support remains conservative and fixed-track is best-effort variant
  pinning. Do not copy Android decoder conclusions into iOS.

## Android Subtitle Transaction

- Source-declared external subtitle identity, public catalog presence, exact
  Media3 TEXT target readiness, selection-parameters readback, renderer-active
  state, and cue delivery are distinct stages.
- Track mode waits within one total deadline until the requested stable ID maps
  to one selectable current TEXT override. Use the same stable-ID resolver for
  catalog mapping, readiness, apply, and readback; never compare raw `Format.id`
  as the public identity.
- Confirm only from exact current selection-parameters readback. Track-change
  generations may trigger another read but are neither necessary nor sufficient
  success evidence.
- Preserve command ID, source/item epoch, callback generation, supersede, source
  switch, and dispose fencing. A target that never became selectable is a
  readiness/not-found failure; only an applied request lacking exact readback is
  a selection timeout.

## Diagnostic Events

- Media3 `onDroppedVideoFrames` is a benchmark sample, not a playback error or a
  Flutter/system compositor result. Record it only for the current generation
  when benchmark collection is enabled and the count is positive; do not emit an
  unconditional warning or change playback state.
- Flutter controller should forward a decoded Pipeline EventHook reports event
  exactly once without reducing it into snapshot or error state. Preserve final
  reports emitted during dispose and reject events delivered after disposal.

Performance Diagnostics uses one lazy coordinator per player and at most one
active run. Disabled players register no timing callback, native frame probe,
timer, worker, or queue. Flutter submits `FrameTiming` in batches; Android uses
`FrameMetrics`; iOS uses `CADisplayLink` and AVPlayer access-log deltas. These
probes observe different boundaries and must remain identified in reports.
Every frame carries its overlay state at capture time, so hosts do not align
clocks across runtimes. Partial startup, stop, and player disposal must remove
all probe and recorder resources.

Plugin diagnostics must distinguish catalog/import, resolution, plan, load,
selection, participation, fallback, rejection, failure, and quarantine. A
loaded or preflighted capability is not evidence that a mobile host used it.

Performance reports likewise separate UI frame cohorts from native playback
pressure. Preserve the probe and unknown raw values, validate sample
sufficiency, and describe overlay relationships as correlation rather than
causation.

## External Playback

- Android Cast, DLNA, relay, and relay format adaptation are host-kit/package
  responsibilities. Do not add a new distributable module without an explicit
  package boundary.
- iOS uses the system AirPlay route picker. Do not add programmatic AirPlay or
  DLNA as an implicit side effect.
- Relay infrastructure may handle headers, local files, `content://`, and
  non-public URLs, but must not leak cookies, authorization, or site-specific
  business rules into remote URLs or logs.
- Unsupported DRM, encrypted DASH, unsupported layouts, and inaccessible
  sources must produce explicit diagnostics.

## Public Boundary Rule

Keep host-facing contracts expressed as controller, source, snapshot, event,
timeline, track, surface, system playback, and external route. Do not expose
Media3, AVFoundation, JNI, C ABI, filesystem paths, or platform resource
handles merely because an internal implementation uses them.
