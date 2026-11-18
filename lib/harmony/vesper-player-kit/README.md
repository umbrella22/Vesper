# @vesper/player-kit

The core HarmonyOS preview runs Media Kit AVPlayer and exposes shared Rust source,
sequence, preload and download semantics. It contains no ArkUI components. The
runtime floor is API 20 on arm64; the local package version is
`0.9.1-harmony.0`. Registry publication is pending; physical-device acceptance
remains unverified and does not block the scoped preview.

```ts
import { VesperPlayerController, VesperSourceSession } from '@vesper/player-kit';

const sources = new VesperSourceSession();
const player = new VesperPlayerController(context);
player.attachSurface(surfaceId);
const source = sources.register({
  kind: 'remote', protocol: 'progressive', label: 'Example',
  uri: 'https://media.example/movie.mp4'
});
try {
  await player.activate(source, { startPositionMs: 1000, playWhenReady: true });
} finally {
  source.close();
}
// When this playback session ends:
await player.dispose();
sources.close();
```

Registering a source creates no player. Descriptors are immutable copies;
activation obtains a separate lease. Close prevents new acquisition and preserves
accepted leases. Invalidation and expiry fence later activation. Replacing a
source, cancellation, timeout and disposal settle pending operations and reject
stale callbacks by playback epoch.

Native presentation preserves the video's aspect ratio and centers it with black
bars when the Surface has a different shape. The controller sets the API 20
scaled-aspect mode after native preparation.

`subscribe()` delivers snapshots and structured events; its return value removes
the observer. `snapshot()` includes native state, source/activation/epoch,
first-frame timing, tracks, requested/confirmed/effective selection, loader/cache
bytes and retained resources. `VesperPlayerError` preserves the native error code.
No first-frame or `playing` state establishes audible output or hardware decoding.
Buffering is a separate snapshot flag with native percentage/cached-duration
observations. Unknown buffering event types retain their raw value.

`setPlaybackPolicy()` returns per-parameter requested/effective/unsupported
results. `VesperBufferingPresets` provides three native preference presets for the
next activation; strict independent buffer thresholds and decoder enforcement
remain unsupported. `refreshDiagnostics()` separates native buffer/statistic
observations from upstream Transport counters and fences results by epoch.
`diagnosticReport()` returns a detached feedback DTO; `exportDiagnosticReport()`
returns the same bounded report as JSON. Reports exclude URLs, headers, private
paths, labels, subtitle contents and free-form errors. The [feedback contract](diagnostic-feedback.md)
defines runtime identity, timeline sampling, user observations and local export.
`queryVideoDecoderCapability()` queries hardware catalog support without creating
a codec or claiming AVPlayer decoder identity. `retry.maxAttempts` configures one to three attempts for future
controller resource requests; independent download/preload and native recovery
retain their separate policies.

`VesperPlaybackRecovery` supplies opt-in bounded reactivation for known transient
errors, preserving VOD position/rate and cancelling on user pause or source
replacement. Success requires a rendered frame. Its activation callback invokes
the controller/sequence before its first suspension. `setTrackPreferences()`
performs one explicit language-based audio/subtitle selection after catalog
readiness; it does not enable native automatic reselection. Per-kind selection
snapshots preserve request, confirmation and observed output separately.

`VesperPlaybackSequence` accepts source handles or host-resolved items. Replace,
navigation and resolver completion never start playback. Attach a controller and
call `activate()` explicitly. Resolution forwards revision, request, generation
and expiry fences to the shared Rust coordinator. Close the sequence separately.

`handle.preload()` uses bounded startup memory; optional `VesperDiskCache` has an
independent partition/budget. DASH startup reports `playbackReusable` only after
native playback consumes cached bytes and produces a first frame. Progressive
prefix preloading reports `downloadOnly`.

`VesperDownloadManager.open(context)` provides finite progressive, unencrypted
HLS VOD and finite static single-Period DASH SegmentBase/SegmentTemplate assets,
including finite SegmentTimeline and Time-addressed resources. Planning has
independent 4096-file, 900 KiB plan and 1 GiB download limits, with bounded batch
length discovery. Checkpoints contain no
headers. Completed resources are verified before `offlineSource()` returns a
local descriptor. Interrupted downloads restore paused and require fresh access
context through `refreshSource()`. Manager close joins physical operations.
Original exports preserve progressive files or segmented ZIP assets; save/share
use explicit host actions and retain temporary copies separately.
Alternate-audio switching requires separate native acceptance: the controlled
fixed-duration Template fixture times out on the API 26 emulator, including a
direct AVPlayer HTTP comparison without SDK routing. Its downloaded audio
catalog does not prove successful switching.
Checkpointed 64 KiB chunks are hashed and reused only after locator, ETag and
length verification; refreshed signed URLs conservatively restart. Originals
and ZIP outputs stream within a shared 256 MiB output limit, two-preparation
concurrency and a 512 MiB temporary reservation budget. Task snapshots expose
the pending resume requirement, reusable bytes and export eligibility.

`VesperSystemSession.create()` integrates AVSession metadata/commands and optional
background audio. Hosts declare INTERNET for remote playback and
KEEP_BACKGROUND_RUNNING plus ability `audioPlayback` for background audio. API
26 constraints/time ranges are runtime-gated; live default-position seeking on
the resource-loader/loopback routes returns `unsupported`. Dynamic DASH activation
also returns `unsupported`: the internal bounded AVC/AAC Timeline-to-HLS prototype
did not pass independent-audio refresh acceptance on the tested API 20/26
runtimes. Its host contracts do not establish native playback support. Decoder enforcement, HDR and
physical interruption/route behavior require separate evidence.

The [platform guide](../README.md) defines media profiles, cancellation, cache,
download/export, system behavior, diagnostics, build commands and release gaps.
The [ArkUI binding](../vesper-player-kit-arkui/README.md) supplies a stable native
Surface when an application uses ArkUI.
