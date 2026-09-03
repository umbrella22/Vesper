# Vesper Mobile Plugin Playback Contract

Catalog, resolver, plan, scope, and playback-slot rules are defined in
`plugin-runtime-contract.md`. This card narrows those rules to mobile source,
frame, and audio routes.

## Plugin Family Roles

- SourceNormalizer repairs or remuxes sources before playback consumption.
- Decoder converts compressed packets to decoded native frames in the
  SDK-managed native-frame lane.
- FrameProcessor transforms decoded native frames and depends on a decoder or
  existing native-frame pipeline.
- AudioProcessor transforms PCM frames in a Native SDK-managed audio lane. It
  does not receive Media3 or AVPlayer output on the direct native route.
- Performance Diagnostics is a bounded observer through the existing Native
  BenchmarkSink. Platform hosts own `FrameMetrics`, `CADisplayLink`, AVPlayer,
  and Flutter `FrameTiming` probes; the sink only aggregates sanitized events.
- Do not merge these families or make a SourceNormalizer change implicitly add
  mobile Decoder artifacts or default FrameProcessor playback.

## Routes

- `fMP4 local stream`: disk-backed fragmented MP4 consumed by Media3 or AVPlayer.
- `HLS short-window`: bounded local playlist and segments consumed by Media3 or
  AVPlayer.
- `packet stream`: compressed packet output reserved for a future
  SDK-controlled Decoder/FrameProcessor lane.
- `bypassed` or `fallbackOriginal`: the original source remains the input.

Normal HLS/DASH stays on native player paths. A SourceNormalizer is selected
only for an explicit mode, a known unsupported/weird source, a test force
setting, or a validated adaptive decision.

## Mode And Participation

- `disabled`: do not load or preflight.
- `diagnosticsOnly`: load and report capability only.
- `preflightOnly`: probe/open/close; playback still uses the original source.
- `preferNormalized`: try normalized playback, then fall back with diagnostics.
- `requireNormalized`: fail source selection if normalization cannot be ready.

For Flutter production opt-in, prefer
`VesperSourceNormalizerConfiguration.preferBundled()` after adding the optional
native dependency. Use `requireBundled()` for tests or strict ingest flows that
must fail rather than use the original source. The preset selects a canonical
plugin identity; it does not make every Vesper app bundle optional artifacts.

Mark `Participated` only after Media3 or AVPlayer successfully consumes a
normalized fMP4/HLS resource. A packet stream is not participation in the
system-player route. FrameProcessor is diagnostics-only on system-player routes
until decoded frames are explicitly owned by the SDK.

AudioProcessor participation has the same explicit boundary: a Native PCM chain
must be created by an SDK-managed audio route and must report accepted,
processed frames. Installing or resolving an AudioProcessor does not alter the
Android Media3 or iOS AVPlayer DirectNative audio path. WASM cannot participate
in realtime PCM under the standard invocation policy.

## Resource Safety

- Use disk-backed session caches with bounded reads, range validation, growing
  file readiness, quota enforcement, and cleanup on cancellation/close.
- Tokenize session-local resource paths and enforce root containment.
- A cancelled request must not implicitly dispose the whole normalization
  session.
- Bound retries, waits, skips, and readiness loops; report a timeout or quota
  diagnostic rather than spinning forever.

## Android

Keep the normalized resource server inside the main host kit. Bind loopback to
`127.0.0.1:0` and use tokenized URLs such as:

- `/normalized/{token}/primary`
- `/normalized/{token}/playlist.m3u8`
- `/normalized/{token}/segments/...`

Give Media3 a loopback progressive URL for fMP4 or a loopback `.m3u8` for HLS.
The internal registry may resolve native artifacts, but public configuration
uses `VesperPluginReference` rather than filesystem paths.

## iOS

Keep `vesper-dash://` and `vesper-normalized://` as separate product routes.
Shared AVFoundation helpers may perform bounded local reads and cancellation,
but must not mix DASH manifest generation, ABR, normalization sessions, profile
hashes, or fallback policy.

## Diagnostics

Expose mode, route, participation, selected profile, content type, resource kind,
disk usage/quota, fallback reason, and concise capability/track information.
Example UI must not claim that mobile playback was replaced unless participation
is true.

## Distribution Boundary

- A dependency or Swift product decides which native artifact is embedded. A
  `VesperPluginReference` selects one already embedded artifact at runtime. Do
  not use filesystem paths or FFmpeg component libraries as plugin references.
- Android SourceNormalizer carries only its plugin binary and profile metadata
  and depends on the one shared FFmpeg runtime AAR. The presence of Gradle
  modules is not evidence that optional Maven publications are enabled or
  consumable from hosted coordinates.
- The iOS optional package currently exposes seven same-named direct products:
  three FFmpeg components and four plugins. It has no aggregate
  `VesperPlayerOptionalPlugins` library product. State explicitly whether a host
  should embed one capability closure or the whole seven-framework set.
- Local staging and archive layout checks prove artifact shape only. A release
  claim additionally needs a clean external consumer using published
  coordinates/remote SwiftPM, final bundle dependency checks, signing, and
  physical-device execution.
- Performance Diagnostics is independently optional. Android hosts may use
  debug/profile-only dependencies; iOS hosts that promise Release exclusion
  need a Run/Profile helper target that is absent from Archive linkage. Inspect
  the final APK/IPA for the native binary, plugin identity, and registry
  fragments instead of inferring exclusion from source configuration.
