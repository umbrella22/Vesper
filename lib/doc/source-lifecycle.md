# Source registration, preload and activation

Vesper 0.8 separates an accepted media source from the player that eventually
uses it. A source session owns registrations and optional bounded preloads.
A controller performs explicit, awaited activation. A playback sequence owns
list navigation and source-resolution requests, and uses the same activation
path as ordinary playback.

The mobile implementations use Media3 and AVPlayer. Source sessions do not
create players, decoders, surfaces or audio output sessions. The new lifecycle
is supported by Android, iOS and their Flutter mobile adapters. Experimental
backends without the required native activation contract reject it explicitly.

## Responsibilities

| Host application | SDK |
| --- | --- |
| Choose content, rendition and account/access context | Accept immutable source registrations and assign opaque identities |
| Resolve signed URLs, credentials, business restrictions and source expiry | Fence stale resolver responses, retain leases and enforce expiry |
| Decide whether a replacement has the same timeline | Apply the explicitly supplied start position, rate and play intent |
| Decide when to activate, retry, skip or fetch another page | Await native readiness and initial seek, report completion or failure |
| Interpret first-frame and audio evidence for product behavior | Retain native diagnostics correlated by playback epoch |

Content IDs, URL patterns, audio priorities and quality tiers remain host
policy. Registering the same URL twice produces distinct source identities.
Reordering or reusing the same handle preserves its native cache scope.
Hosts never assemble cache identity strings or assign accepted source revisions.

## Ordinary playback

Flutter creates the source session independently of a controller:

```dart
final sources = await VesperSourceSession.create();
final handle = await sources.register(
  VesperPlayerSource.localDash(
    uri: localManifestUri,
    headers: requestHeaders,
  ),
  expiresAtEpochMs: signedSourceExpiry,
);

// Optional. No player is allocated by registration or preload.
final preload = await handle.preload();
final preloadResult = await preload.completion;

final controller = await VesperPlayerController.create();
final activation = await controller.activate(
  handle,
  options: const VesperSourceActivationOptions(
    playWhenReady: false,
    startPosition: Duration(seconds: 12),
    playbackRate: 1.25,
  ),
);

// Match later native diagnostics against activation.playbackEpoch.
// Attach VesperPlayerView to the controller as usual.
await controller.play();

// At the end of the host's ownership:
await controller.dispose();
await sources.dispose();
```

Preload is optional. A failed or unsupported preload does not by itself forbid
activation; native playback still determines support for the source. Hosts can
skip preloading entirely or observe its task while preparing the UI.

The corresponding native entrypoints are:

| Operation | Android | iOS |
| --- | --- | --- |
| Create | `VesperSourceSession(context, configuration)` | `try VesperSourceSession(configuration:)` |
| Register | `session.register(source, expiresAtEpochMs)` | `try session.register(source, expiresAtEpochMs:)` |
| Preload | `handle.preload(options).await()` | `await (try handle.preload(options:)).result` |
| Activate | `controller.activate(handle, options)` | `try await controller.activate(handle, options:)` |
| Release registration | `handle.close()` | `handle.close()` |
| Close session | `session.close()` | `session.close()` |
| Revoke access | `session.invalidate()` | `session.invalidate()` |

Android activation is suspending; iOS APIs run on the main actor. Swift
configuration and preload options are validated by throwing operations instead
of silently changing requested limits. Flutter validates locally and native
channels validate again at the resource boundary.

## Activation result and cancellation

Activation defaults to play enabled, position zero, rate 1 and a 30-second
timeout. The timeout must be between 1 ms and 60 seconds. It waits for native
source readiness, the requested initial seek and application of the initial
playback intent. A successful result contains an activation ID, session/source
identity and a positive native playback epoch.

Activation completion does not establish first-frame presentation or audible
output. Those are separate native observations. A preload task ID, sequence
activation epoch and native playback epoch represent different operations and
must not be substituted for each other.

A later activation supersedes an earlier pending activation on the same
controller. Timeout, caller cancellation, disposal and sequence detachment
settle pending work. Invalidation and expiry are checked again before committing
an activation. They prevent new activation; they do not automatically stop
playback that has already completed activation. Hosts that revoke current
playback should also stop or dispose their controller.

Cross-source position continuity is explicit: read the old timeline and pass a
start position only when the application knows the replacement shares it.
Vesper does not infer continuity from matching content IDs, labels or URLs.

## Ownership

| Operation | Registration/preload | Acquired playback or sequence lease |
| --- | --- | --- |
| Close one handle | Reject new acquisitions; cancel its preload | Remains usable until release, expiry or invalidation |
| Close a session | Close its handles and cancel preloads | Remains usable until release, expiry or invalidation |
| Invalidate a handle or session | Revoke the corresponding access and close registrations | Reject subsequent activation |
| Detach a sequence | Stop sequence observations and pending navigation | Session-owned shared preload continues |
| Dispose a controller | Settle pending activation and release playback ownership | Sequence detaches and releases its controller association |

Closing and invalidating serve different purposes. Flutter sessions remove their
native registry entry when either operation completes. Call `invalidate()`
**instead of** `dispose()` when revoking access; a disposed Flutter session
cannot later be used as an invalidation token. Native handles also offer
individual invalidation; Flutter currently exposes session-level invalidation.

Do not close a handle before an intended new acquisition. A sequence already
holding its lease can continue explicit navigation after ordinary handle close.
Native accepted descriptors copy headers and related source values; changing a
host-side dictionary does not alter a registered source. Register a replacement
to change the descriptor or credentials.

## Independent preload

Tasks expose `queued`, `running`, `completed`, `failed`, `unsupported` and
`cancelled`. Flutter preserves unknown wire values in its retained snapshot.
Failure reason codes identify known causes without exposing signed URLs,
request headers or raw native error descriptions. Hosts should retain unknown
codes for diagnostics and use the task status to decide control flow.
Concurrent preload calls on one handle share a live task; the first options
apply. Cancelling that task cancels the shared operation for every observer.
Cancelling a sequence observation does not cancel a session-owned task.

Flutter retains a task's completion and snapshot without background polling.
`refresh()` requests an updated observation. The native Flutter adapter retains
the latest task per registered source, including its terminal result, until
the next task for that source or registration/session release. A previously
started waiter keeps the task it acquired. There is no unbounded task history.

| Preload route | Reuse capability |
| --- | --- |
| Static, single-period, unencrypted DASH SegmentBase | `playbackReusable` through the formal mobile playback path |
| Supported progressive prefix read | `downloadOnly`; no promise that the formal player consumes these bytes |
| Unsupported route or disabled cache | `none`, with explicit unsupported state/reason |

`completed` means the bounded resource set reached the cache. `cacheHit`
describes the preload's own reads, and `actualBytes` includes bytes read from
cache. None of these values proves that formal playback consumed a cached
range. Representation selection, expiry or eviction can still cause a miss.

| Limit | Default | Accepted range |
| --- | --- | --- |
| Registered sources per session | 128 | 1–512 |
| Physical preload workers | 2 | 1–4 |
| Pending preloads | 4 | 0–32 |
| Session memory budget | 8 MiB | 0–16 MiB |
| Per-task byte cap | 8 MiB | 1 byte–16 MiB |
| Preload deadline | 5 seconds | 1 ms–60 seconds |

Deadlines include queue time. A cancelled or timed-out worker retains its
physical slot and staging reservation until it actually exits. Staged aggregate
bytes and resident bytes each obey the session budget. A zero memory budget
disables preloading. Progressive prefixes are at most 64 KiB. The shared DASH
cache is additionally limited to 32 MiB and 64 entries with a maximum 30-second
monotonic lifetime and source-expiry checks.

The source session owns this budget independently of controller resilience and
disk-cache policies. An accepted DASH handle can consume its warm bytes even
when the controller's general cache is disabled, including when the descriptor
is a local MPD. Disable preloading through the source session's memory budget.

## Sequences and migration from 0.7

Sequence items now reference accepted source handles. Flutter's provider
`resolveSource(request)` returns a `VesperSourceReference` directly; a
`VesperSourceHandle` is such a reference. The old public cache-identity and
resolved-source DTOs are removed. Native resolvers submit the typed request
and handle; the SDK carries request fences and assigns the next revision.

```dart
final sequence = await controller.attachPlaybackSequence(
  configuration: const VesperPlaybackSequenceConfiguration(sequenceId: 'feed'),
);
await sequence.replace([
  VesperPlaybackSequenceItem(
    itemId: 'clip-a',
    contentIdentity: const VesperPlaybackSequenceContentIdentity(
      providerNamespace: 'example', value: 'clip-a',
    ),
    source: handle,
  ),
]);
await sequence.activate('clip-a');
```

`replace`, append/prepend, removal, reorder and accepted resolver responses
update list state only. `replace` no longer accepts `activeItemId`. These
operations never select a neighboring item or restart an existing player.
The sequence cursor is list metadata and can be empty while earlier playback
continues. Only `activate`, `next` and `previous` request playback.

`activate(itemId)` awaits any necessary source resolution and native activation.
`next` and `previous` return an optional activation. At an empty/end/paging
boundary they return null/nil. A provider can fill a pending page afterward;
the application then calls navigation again. Appending that page does not
automatically play its first item. Replacing the list or removing a pending
activation's target supersedes that activation.

While a sequence owns a controller, direct source activation/selection on that
controller is rejected. Detach the sequence before ordinary activation. Existing
descriptor-based `selectSource` convenience APIs remain available, but do not
carry an independently registered preload handle.

Rust `SequenceCoordinator::replace` retains its optional cursor hint argument, but
the hint no longer counts as activation or advances the activation epoch. Rust
hosts must call `set_active` or navigation explicitly. Source acceptance alone
also does not advance the activation epoch.

Upgrade the native host kits, Flutter facade and platform packages together to
0.8.0. This is a deliberate sequence/source contract break; old channel payloads
that provide source descriptors and host-built cache identities are not the
0.8 source-registration contract.

The old iOS `VesperPlaybackSequenceWarmupSnapshot` and both platforms' internal
sequence preload executors are removed. Source sessions own physical work;
sequence warmup events describe scheduling and observation. Use each source's
`VesperPreloadTask` to inspect its current state and completion.

## Local manifests and playback evidence

Source location and protocol are independent. A local MPD uses local location
plus DASH protocol; its absolute HTTPS BaseURLs still produce HTTP media
requests with the accepted headers. Android and Flutter expose `localDash`;
Swift can construct `VesperPlayerSource(uri:label:kind:protocol:headers:)` with
`.local` and `.dash`. Preload's local reader is confined to the manifest role
and a 1 MiB file cap. It does not permit arbitrary local range resources or
relax HTTPS/redirect policy for network ranges.

See [DASH startup cache](dash-startup-cache.md) for transport, representation and
formal-consumption details, and [playback diagnostics](../flutter/vesper_player_platform_interface/doc/playback-diagnostics.md)
for first-frame/audio evidence.

## Reproducing the public iOS playback smoke

Build and install `VesperPlayerKitDeviceTestHost`, then launch it with
`--source-lifecycle-smoke`. It uses only public registration, preload and
activation APIs, a local MPD, and the committed six-second AVC fixture. The
default media URL points to that fixture in the public repository. The test
deletes its temporary MPD after preload, activates paused at 250 ms and 1.25x,
checks that the position stays paused, then plays past three seconds. Success
also requires first-frame diagnostics to match the activation's playback epoch.
The host prints a bounded `SOURCE_LIFECYCLE_SMOKE` JSON result and exits; its
watchdog terminates the scenario after 60 seconds. The production hardware
decode policy remains active, including on Simulator.

For formal cache-consumption evidence, supply two additional launch arguments:

```text
--source-lifecycle-smoke-media-url https://fixture.example/redirect/video.mp4
--source-lifecycle-smoke-stats-url https://fixture.example/stats
```

The HTTPS origin must serve `fixtures/media/dash-startup-video.mp4` with exact
206 ranges and require `X-Vesper-Fixture: source-lifecycle-080`. A same-origin
redirect can exercise header forwarding. Its statistics endpoint returns a
monotonic request history, with caching disabled:

```json
{"mediaRequests":[{"range":"bytes=771-846","headersMatched":true}],"manifestRequests":0}
```

The scenario compares baseline, post-preload and post-playback snapshots. It
requires exactly one request for each warm range (`0-770`, `771-846`,
`847-11668`), correct headers, no remote manifest request, and later playback
requests beginning at or after byte 11669. Repeating a warm range or fetching
the whole file fails the reuse assertion. A successful run without a statistics
endpoint proves playback, but does not prove origin-range reuse.

On 2026-09-29, the final 0.8 implementation passed this public path on an iPhone
16 Pro running iOS 27, including a same-origin HTTPS redirect with source
headers. The MPD was deleted after preload; paused position stayed at 250 ms,
playback ran at 1.25x beyond three seconds, and first-frame and activation epochs
matched. The origin observed three warm ranges and two later uncached ranges,
with no repeated warm range. The generated iOS playlist uses the final media URL
validated during preload for later native range reads. Android formal-factory
reuse is covered under Robolectric; no Android physical-device result is
claimed for this release.
