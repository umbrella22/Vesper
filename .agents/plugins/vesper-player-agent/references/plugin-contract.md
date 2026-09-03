# Vesper Plugin Contract

This card is the bundled reference for the current Vesper plugin platform.
The implementation and tests in the checkout remain authoritative.

## Author Surface

- Official author languages are Rust Native and Rust WASM.
- `player-plugin` is the only safe Rust author dependency. Ordinary author
  code should contain no `unsafe` and no `extern "C"`.
- There is no C or C++ author SDK. Internal C-compatible ABI, iOS bridge FFI,
  and FFmpeg C dependencies remain host implementation details.
- Native signatures prove publisher identity and artifact integrity only. They
  do not sandbox trusted native code.

## Native Entry And ABI

- Native libraries export exactly `vesper_plugin_entry`.
- The entry returns one `VesperPluginRoot` containing a fixed-width size and
  ABI version, owner and identity data, interface enumeration/query callbacks,
  byte release, and owner destruction.
- Every typed interface starts with `VesperInterfaceHeader` containing
  `struct_size`, `interface_id`, major/minor version, and context.
- Minor revisions are append-only. Signature, ownership, or layout changes
  require a new major interface version.
- The loader validates the root and each table once, then exposes checked
  wrappers or typed traits. Hot paths must not repeat optional callback checks.
- `plugin_kind` is diagnostic classification only. It never selects an ABI
  table or transport.

## Interface Inventory

The current interface IDs are stable UUIDs:

| Interface | ID |
| --- | --- |
| PostDownloadProcessor | `e9479dbc-42d2-575e-b39e-a24bc512fbc7` |
| PipelineEventHook | `c7a69475-79b2-5b5e-a477-08844a5da5d1` |
| BenchmarkSink | `2d8e5be8-b1de-5e83-8fe0-6118aabc5118` |
| NativeDecoder | `d68be0ed-1958-5922-8b7a-bc6778a26b43` |
| FrameProcessor | `fc050597-b7b7-5c81-83b9-b42555f8b825` |
| AudioProcessor | `f3fc5d7c-581f-5e0a-85bf-df00d7adb13e` |
| SourceNormalizerPacket | `a2d653fa-d6ce-5f14-93b8-a818a7a77fdf` |
| SourceNormalizerResource | `b76d1f06-62d7-5d71-aa06-2780e4b4fd0d` |

Stable extension points are PostDownloadProcessor, PipelineEventHook, and
BenchmarkSink. NativeDecoder, FrameProcessor, AudioProcessor, and both
SourceNormalizer interfaces are experimental or optional. They require an
explicit host route and participation evidence; installation or capability
discovery never makes them the default mobile playback path.

The official performance diagnostics plugin reuses `BenchmarkSink` without an
ABI revision. It receives bounded, sanitized event batches and returns an
aggregate schema v1 report from `flush`. The host owns frame probes, per-player
run coordination, and optional artifact selection. The plugin does not receive
media data, URLs, request headers, cookies, account data, overlay text, or raw
error messages.

## Ownership And Lifecycle

- Native sessions are `Send` and used through `&mut self`; do not upgrade them
  to `Sync`. Root/factory owners may be `Send + Sync` when proven safe.
- Packet borrows, host-buffer leases, and native-resource leases are separate
  concepts. Do not collapse them into one generic handle.
- Session tokens are non-zero `u64` slot/generation values. Lease IDs are
  independent `u64` values and can only be released by the creating
  session/interface.
- `flush` invalidates and drains all outstanding leases. `close` is idempotent,
  may report failure, and still attempts cleanup.
- Panic, unknown status, truncated output, or an ABI violation poisons the
  instance. Only cleanup is allowed after poisoning.
- All wrappers share one owner reference. The final owner reference invokes
  `destroy_owner` exactly once. Dynamic libraries remain process-live; the
  loader does not call `dlclose`.

## References And Diagnostics

- `PluginReference` requires a validated reverse-DNS `plugin_id` and explicit
  `Native` or `Wasm` transport. There is no `Auto` transport.
- Omitting a capability instance is valid only when exactly one implementation
  matches; multiple matches are an ambiguity error.
- Diagnostics distinguish found, loaded, compatible, selected, participated,
  bypassed, fallback, and rejected states.
- Preserve unknown status, capability, warning, and enum values for host
  diagnostics rather than silently mapping them to a supported value.
- EventHook inputs use opaque resource identities and bounded structured
  diagnostics; raw paths, credentials, DRM material, and sensitive URLs do not
  cross the plugin boundary.

## Runtime Selection Boundary

The runtime phases are documented in `plugin-runtime-contract.md`:

1. `PluginCatalogImporter` validates metadata and artifact digests into a
   `PluginCatalogIndex` without opening executable code.
2. `PluginResolver` applies host transport, target, architecture, ABI, semver,
   and priority policy to produce a deterministic `PluginResolution`.
3. `PluginPlan` stores the canonical catalog projection, selected providers,
   dependency-first artifacts, and catalog/plan fingerprints.
4. `PluginRuntime` activates the immutable plan under hierarchical scopes and
   only then creates checked loader capabilities.

Catalog records and plans contain no live owners, handles, workers, queues,
callbacks, or media. A changed catalog or host policy requires a new plan and
runtime generation.

## Transport And Workload

`PluginTransport` has only `Native` and `Wasm`. `PluginInvocationPolicy` keeps
transport separate from workload: `RealtimeMedia` is Native-only under the
standard policy, while bounded `Observer` and `Offline` work may use either
transport. A policy rejection is typed and is not a missing-plugin result.
The loader never silently retries a rejected request through the other
transport. `plugin_kind` remains a diagnostic label and is not a selector.

## Native AudioProcessor

`AudioProcessor` is a Native realtime PCM interface. The safe author surface is
`AudioProcessorPluginFactory` and `AudioProcessorSession`; hosts compose sessions
with `AudioProcessorChain`. Capabilities advertise accepted/output PCM formats,
flush support, in-flight limits, finite-positive playback-rate bounds, and
supported `AudioPitchMode` values.

`AudioPlaybackPolicy` contains `playback_rate` and
`AudioPitchMode::{PreservePitch, FollowRate}`. `PreservePitch` and `FollowRate`
are distinct processing contracts, not aliases. The bounded chain applies
ordered processors, reports backpressure, and supports flush and idempotent
close. Each processor returns plugin-owned PCM while preserving the host-owned
`pts_us` and discontinuity marker. The host retains clock, scheduling, and A/V
timing; a processor cannot take over those authorities.

Android Media3 and iOS AVPlayer DirectNative routes do not consume plugin PCM.
Native audio processing is available only where an explicit SDK-managed audio
route exists, and remains experimental until host/device evidence closes that
route.

## WASM Component Boundary

- Use Wasmtime Component Model with Rust `wasm32-wasip2` components.
- WASM currently supports only PipelineEventHook and BenchmarkSink.
- Components receive bounded structured events, not media bytes, file paths,
  network handles, DRM data, environment, processes, clock, or random access.
- The host grants only the structured log import. Default limits are 64 MiB
  memory, 10M fuel per call, 50 ms EventHook, 250 ms batch, 2 s flush, 256 KiB
  output, and bounded event/batch queues.
- Overflow drops the newest item and increments a counter. Trap, timeout, or
  close failure quarantines the component; the host does not silently switch
  transports.

WASM does not implement NativeDecoder, FrameProcessor, AudioProcessor, or
SourceNormalizer media lanes. Requests for those realtime interfaces fail by
workload policy before artifact lookup, with a diagnostic that identifies the
requested interface and workload.

## Package And Trust Boundary

- `vesper-plugin.toml` is author input; the Rust CLI writes canonical
  `manifest.json` and deterministic `.vesper-plugin` ZIP payloads.
- Packages contain target artifacts, sorted `SHA256SUMS`, Ed25519 signatures,
  license/notices, and runtime dependency metadata.
- Canonical descriptors carry catalog schema and migration identity. Rejection
  of an old ABI or schema must include the plugin identity, expected/actual
  versions, and a stable migration-guide entry; a bare version number is not an
  actionable migration error.
- Release loading requires a host-configured publisher trust store. Key
  rotation supports overlapping publisher keys and explicit revocation.
- Unsigned or raw native libraries are development-only and must be enabled by
  an explicit policy with a warning.
- Installation verifies before staging, rejects traversal, symlinks, duplicate
  paths, checksum drift, archive bombs, and ambiguous targets, then promotes
  atomically.

The first-party `plugins/audio-processor-diagnostic` fixture implements the two
pitch modes as different DSP paths: WSOLA for `PreservePitch` and bounded linear
resampling for `FollowRate`. That fixture proves the ABI, checked loader, and
deterministic DSP oracle; other processors must advertise and prove their own
rate/pitch behavior.

## Mobile And Security Non-Goals

- Android and iOS production playback remains Media3 and AVPlayer.
- Mobile plugins are build-time embedded; runtime code download is not allowed.
- Plugins never receive protected media, Widevine/FairPlay material, headers,
  tokens, or raw sensitive URLs.
- `inspect` and `check` worker processes provide tool-level crash isolation,
  not a runtime sandbox.
