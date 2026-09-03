# Vesper Performance Diagnostics Contract

This card summarizes schema v1 for the optional official Native
`BenchmarkSink` plugin. The current checkout and its public
`docs/performance-diagnostics.md` remain authoritative.

## Product Boundary

- Plugin ID: `io.github.umbrella22.vesper.performance-diagnostics`
- Capability instance:
  `io.github.umbrella22.vesper.performance-diagnostics.benchmark`
- Native library: `vesper_performance_diagnostics`
- The core host kits expose the API and lazy coordinator but do not embed the
  optional binary.
- The plugin aggregates bounded events and returns reports through the existing
  `BenchmarkSink` batch and `flush` ABI. It performs no network or upload work.
- It must not read or emit video URLs, request headers, cookies, account data,
  overlay text, or raw error messages.

One player has at most one active run. A run starts and stops without recreating
the player. Snapshot leaves the run active. Stop is idempotent and caches its
final report. Controller disposal stops an active run, and partial startup
failure removes every recorder, callback, probe, worker, and queue it created.

## Session Surface

The public surfaces are equivalent in Dart, Kotlin, and Swift:

```text
start(controller, configuration) -> session
session.updateOverlayState(state)
session.recordMarker(name, value?, sequenceIndex?, expectedOverlayActive?)
session.snapshot() -> report
session.stop() -> final report
```

`includeRawEvents` defaults to false and `maxRawEvents` defaults to 256. The
range is 0 through 2048; disabled raw events allocate no raw-event storage.
Marker names are 1 to 64 ASCII bytes, begin with a letter or underscore, and
then use letters, digits, underscore, period, or hyphen. A run accepts at most
64 markers.

Overlay state contains `active`, `sampleClass`, optional nonnegative basic and
advanced item counts, and `advancedEffectsActive`. Sample class is `steady`,
`transition`, or `excluded`. Every frame captures that state at the same runtime
boundary; implementations do not align clocks across runtimes.

## Schema v1

All wire durations and timestamps use integer nanoseconds. A report contains:

- identity: `schemaVersion`, `runId`, `sessionId`, `platform`, and raw `probe`
- timing: `durationNs` and `frameBudgetNs`
- `cohorts`: `overlayInactive`, `overlayActive`, `transition`, and `excluded`
- `playback`: active duration, dropped frames, buffering count/duration, stalls
- `diagnosis`: raw kind, raw confidence, and evidence codes
- accounting: accepted, dropped, and raw-event-dropped counts
- diagnostics and an optional bounded raw-event list

Each cohort contains sample, jank, and severe-jank counts and ratios plus
`minLoadNs`, `p50LoadNs`, `p95LoadNs`, and `maxLoadNs`. Counts, ratios, and
extrema cover the complete run; p50 and p95 are bounded-reservoir estimates
sampled across the complete run. Jank is over one frame budget; severe jank is
over two budgets.

Known probes are:

| Probe | Frame observation | Playback observation |
| --- | --- | --- |
| `flutterFrameTiming` | max of Flutter build and raster duration, batched at 120 samples or 500 ms | native player events |
| `androidFrameMetrics` | window `TOTAL_DURATION` | Media3 buffering, drops, and stalls |
| `iosDisplayLink` | missed display-link vsync intervals | AVPlayer buffering and access-log deltas |

These probes observe different boundaries. Compare like with like and preserve
unknown probe strings.

## Fixed v1 Rules

- Both steady cohorts need at least 120 frames.
- UI pressure is a steady jank ratio of at least 5 percent or steady p95 over
  one frame budget.
- Overlay correlation is an active-minus-inactive jank increase of at least 5
  percentage points and at least 1.5 times, or a p95 increase of at least half a
  frame budget.
- Playback pressure is a steady buffering/stall interval of at least 500 ms, or
  dropped frames reaching `max(3, ceil(active playback minutes * 5))`.
- Schema v1 publishes total buffering duration and stall count, not the
  steady-only stall duration retained by the sink. Do not infer pressure from a
  nonzero stall count alone; use `native_playback_pressure` evidence when the
  public summary cannot reproduce the steady-duration decision.
- Confidence is low at 120-299 frames in the smaller steady cohort, medium at
  300-599, and high at 600 or more only with at least two transitions.

Known diagnoses are `insufficientEvidence`, `noSignificantPressure`,
`overlayCorrelatedUiPressure`, `hostUiPressureUncorrelated`,
`playbackPressure`, and `mixedPressure`. They describe correlation, not cause.
Known evidence codes are `steady_cohorts_below_120`,
`overlay_steady_cohort_delta`, `native_playback_pressure`,
`ui_pressure_not_overlay_correlated`, and `thresholds_not_exceeded`.

## Guided A/B

Wait up to 10 seconds for normal playback and non-empty overlay data, ignore a
5-second warm-up, and then run `off -> on -> off -> on` for 12 seconds each.
The first second after each change is `transition`; the diagnostics UI itself is
`excluded`. Use an in-memory settings override and restore the complete previous
state on every exit path.

## Distribution And Evidence

Android uses the optional Maven artifact
`vesper-player-kit-performance-diagnostics`; iOS uses the optional
`VesperPlayerPerformanceDiagnostics` SwiftPM product or
`VesperPlayerPerformanceDiagnosticsPlugin` XCFramework; Flutter offers
`vesper_player_performance_diagnostics`. A host that promises Release exclusion
must enforce that at the native build-configuration layer and inspect the final
APK/IPA for binaries, plugin identity, and registry fragments.

Local tests and archive inspection are not device evidence. Stable acceptance
requires Android native `FrameMetrics` on a physical Debug/Profile host and the
repository's specified iOS/device capture. A report from one platform or probe
does not prove another.
