# HarmonyOS diagnostic feedback

Playback executes in Media Kit AVPlayer with a native Surface. The system selects
the decoder. Hardware catalog queries do not identify the selected decoder or
gate playback. Vesper does not add a software decoder or a separate playback
pipeline. Android retains its independent hardware-decoder policy.

The controller retains local observations throughout its lifetime. Applications
choose when to sample native metrics and serialize a report. The SDK opens no
telemetry connection and writes no diagnostic files automatically.

```ts
await controller.refreshDiagnostics();
const report = controller.exportDiagnosticReport({
  applicationVersion: '1.2.0', applicationBuildIdentity: 'public-build-42',
  scenario: 'alternate-audio', fixtureId: 'public-test-vector-3',
  runtimeKind: 'device', audibleOutputObservation: 'not-assessed'
});
// Present a host-owned save/share action for this JSON string.
```

`diagnosticReport(options)` returns a detached `VesperDiagnosticReport` DTO.
`exportDiagnosticReport(options)` returns compact JSON. Both also work after
stop/disposal. Sampling with `refreshDiagnostics()` requires an active native
player and uses the serialized, epoch-fenced command queue.

## Report fields and evidence

| Group | Meaning |
| --- | --- |
| SDK identity | Package version and SHA-256 of working source inputs stamped into the HAR; direct source builds report `unknown` |
| Application identity | Host-provided public version/build identifier, independent of SDK identity |
| Runtime | API, OS name/display version, product model and device type; no serial number or account |
| Environment kind | `unknown`, `device` or `simulator`; explicitly `host-reported` |
| Material | Public fixture/scenario identifier and protocol; user media can use `user-selected` |
| Tracks | Numeric IDs, kind, MIME, language, selected state, dimensions and bitrate |
| Timeline | Current epoch: native state, event/operation, requested seek/track, positions, selected tracks, buffering and completion |
| Native sample | Buffer/statistic observations; API 26 adds native position, presentation timestamp and time ranges |
| Transport | Controller-lifetime request IDs, status, requested range, received bytes, duration and outcome |
| Human observation | `not-assessed`, `audible`, `silent` or `intermittent`, explicitly `human-reported` |

Position, loaded ranges, seekable ranges and PTS retain native coordinates and
units. PTS uses microseconds; positions/ranges use milliseconds. A retained PTS
carries its sample time and is not a fresh PTS read at every event. `seekDone`
and selected-track confirmation do not establish audible output or DVR rewind.
Native position, PTS and ranges have independent availability statuses. PTS is
unavailable before playback begins; sampling a prepared player skips that query
and preserves subsequent playback commands. Buffer information is unavailable
after completion because its native query does not permit that state.

`actualDecoder` and `decoderExecutionEvidence` remain `unknown`. Native download
metrics identify their route. Transport cannot see AVPlayer-internal requests.
Reports do not claim complete network tracing, audio-frame execution, HDR,
hardware execution, or power measurements.

## Retention and privacy

The controller retains 128 moments, with at most one position-only sample per
second, and the last 32 completed physical transport responses, including
redirects. Dropped counters expose eviction. Report metadata includes at most 64
tracks, 16 selected IDs per moment and eight ranges per kind. Serialized output
is limited to 128 Ki UTF-16 code units, providing a 512 KiB UTF-8 upper bound.
The SDK allocates no diagnostic timer.

Explicit field projection excludes source URLs, queries, headers, authentication
values, private paths, media labels, subtitle text, arbitrary native values and
free-form error messages. Accepted error identifiers and numeric native codes
remain available; other code spellings become `redacted`. Unknown native metric
names are bounded identifiers. Hosts supply public build/fixture identifiers;
the API rejects URLs/paths in these fields. Identifiers must not contain secrets.

## Example workflow and capability limits

The Diagnostics tab offers playback progression, alternate-audio, last-15-second
completion and live DVR availability scenarios for the selected video. Scenarios
are bounded and stop when the page closes or playback changes. The tail preset
seeks within the original full material and does not prove uninterrupted playback.
Missing audio tracks or rejected live operations produce explicit unsupported
results. Live seeking and dynamic DASH gates remain authoritative.

The user can mark the environment and audible-output observation, copy the report
or save `vesper-diagnostics.json` through the system document picker. Reports are
submitted by the user through the application's support channel. No automatic
receiver is configured.

Simulator results do not determine every commercial device's behavior. Known
profile failures remain in the [current checklist](../../../CURRENT-CHECKLIST.md).
Physical devices and Beta runtimes are not prerequisites for distributing the
verified preview scope. Expanding DVR, dynamic DASH or affected track profiles
requires new behavioral evidence tied to the source and runtime.
