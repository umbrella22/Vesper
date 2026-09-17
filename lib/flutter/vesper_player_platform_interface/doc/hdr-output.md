# Current display output

`VesperPlayerSnapshot.hdrOutput` describes evidence about the active display
path. Capability probes describe whether a requested media format or playback
path is supported. A probe result never supplies output confirmation.

## Implemented support

| Producer | Current output state | Observation limit |
| --- | --- | --- |
| Android host kit and Flutter plugin | `unknown` / `outputObservationUnavailable` | Display HDR types, Media3 formats and codec support do not observe the active Surface's output. |
| iOS host kit and Flutter plugin | `unknown` / `outputObservationUnavailable` | AVPlayer HDR eligibility, EDR configuration and asset metadata do not observe this player's actual display output. |
| Older platform without the field | `null`, exposed as `hdrOutputState == unknown` | No output evidence was supplied. |

Both mobile host kits track native output lifecycles and reject obsolete
observation tokens. Neither producer currently enables an observer that confirms
HDR or SDR. Native system playback can still present HDR; output observability
is a separate capability. Hosts display "Output unconfirmed" while the state
is unknown, without blocking startup or automatically changing tracks.

## Runtime flow

The native host kit allocates one output tracker per player controller. A source
activation increments `sourceRevision` and `outputGeneration` before replacing
the source. Direct selection and playback-sequence activation use the same
boundary, including repeated selection of the same URL.

Native playback, Surface, display and PiP events increment `outputGeneration`
and clear the state, format and evidence to unknown. The controller exposes the
latest snapshot; the Flutter plugin publishes it in the existing player snapshot
stream and adds `playerId`. Generation changes occur at native event boundaries,
so a sampled track A can return to A through B without reviving the first A's
observation token.

Both kits initialize source and output generations to one when an initial source
is configured, and zero before any source. Android's `hdrOutput` StateFlow exposes
only the latest value and may conflate transitions. Consumers that need every
invalidation use `setOnHdrOutputChangedListener`, which delivers the current value
and each subsequent transition synchronously on the main looper. The Flutter
Android plugin uses that listener so immediate reconfirmation cannot skip unknown.
iOS uses the captured `hdrOutputPublisher` value and skips the corresponding
general object-change snapshot to avoid duplicate output events.

| Invalidation boundary | Android | iOS |
| --- | --- | --- |
| Source activation or reload | Native source selection and preparation, including sequence activation | Native source selection and load, including sequence activation |
| Video selection | Media3 input-format callback and effective track/catalog updates | Effective track/catalog updates, AVPlayerItem presentation-size and access-log events |
| Player and observer lifecycle | Media3 recreation, callback invalidation, stop and disposal | AVPlayer replacement, observer removal, stop and disposal |
| Surface or window | Host rebinding, SurfaceHolder and SurfaceTexture lifecycle, window attachment | Host rebinding, AVPlayerLayer/presenter changes, window attachment |
| Display path | DisplayManager change/removal callbacks | Display traits, screen mode/brightness, scene and HDR-eligibility change notifications, external playback |
| PiP | Flutter system PiP transitions call `invalidateHdrOutput()` | Flutter system PiP transitions call `invalidateHdrOutput()` |

Display and format notifications only invalidate evidence. They cannot confirm
output. Native hosts that manage PiP themselves call
`VesperPlayerController.invalidateHdrOutput()` at their handoff boundaries.

## Fields

`state` is `unknown`, `sdr`, or `hdr`. `format` independently identifies
`hdr10`, `hlg`, or `dolbyVision`, and defaults to `unknown`. A platform may
confirm HDR without identifying its output format. Source encoding does not
determine output format after system conversion or tone mapping.

| Field | Meaning |
| --- | --- |
| `playerId` | Flutter player session identifier. A recreated player has a new identity. |
| `sourceRevision` | Session-local count of native source activations; zero precedes the first source. Independent of Sequence cache revisions. |
| `outputGeneration` | Monotonically increasing invalidation count within the controller. Repeated track, source and display identities do not reset it. |
| `effectiveVideoTrackId`, `catalogRevision` | Native track context when available; neither field proves display output or replaces a generation. |
| `displayId` | Android attached-host display ID when observable. iOS leaves this absent because no reliable active-output display identifier is supplied. |
| `evidence` | Observation point that confirmed current output. Absent for current unavailable producers. |
| `reason` | `outputObservationUnavailable` for current producers. Future reason strings are preserved. |

The native tracker's internal token contains its instance identity,
`sourceRevision`, and `outputGeneration`. A result must match all three and the
tracker must remain live. Replacing a Surface on the same screen, switching
A→B→A, repeating a source, and disposing/recreating a controller reject old
results. Disposal is idempotent. No playback URL, authorization header or content
label participates in this identity.

Unknown future state / format strings decode to the corresponding `unknown`
enum and retain their raw strings for diagnostics and re-encoding. Capability
model defaults are unchanged. Snapshot copying preserves output evidence;
`clearHdrOutput: true` explicitly returns it to the absent/unconfirmed state.
Known HDR/SDR states with missing or blank identity/evidence or missing/negative
generations decode as unknown with `incompleteOutputEvidence` when no reason was
supplied. Their original enum strings remain available as raw diagnostic values;
re-encoding writes unknown rather than restoring an incomplete confirmation.

## Enabling output confirmation

A platform observer must identify an observation point that proves output on the
active player/display path and document its supported OS and presentation paths.
Decoder format, source metadata, panel support, configuration requests and
screenshots alone are insufficient. SDR also requires explicit output evidence.

The observer captures a token before asynchronous work and applies its result
through the native tracker on the player thread. It must publish unknown before
starting a replacement observation, cancel work on invalidation, and reject a
late result using the captured token. Loss of observation clears confirmation.
Capability requests, candidate sources and recent-probe diagnostic caches remain
independent of this path.

Lifecycle and token regression tests use synthetic output evidence. They verify
invalidation and stale-result rejection; they do not establish HDR playback or
HDR/SDR confirmation on a device. Enabling confirmation requires real-device
validation against reliable native diagnostics across track, Surface, display
and PiP changes, including transitions that keep the same display ID.
