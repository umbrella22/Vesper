---
name: vesper-mobile-plugin-playback
description: Use when adding or changing Vesper mobile playback plugin consumption, SourceNormalizer mobile playback participation, normalized fMP4 or HLS local resources, packet stream boundaries, FrameProcessor and Decoder mobile planning, Android loopback resource servers, iOS AVAssetResourceLoader delegates, plugin diagnostics, or mobile example plugin toggles.
metadata:
  short-description: Mobile plugin playback boundaries
---

# Vesper Mobile Plugin Playback

## Load First

- `../../references/knowledge-map.md`
- `../../references/repository-memory.md`
- `../../references/plugin-contract.md`
- `../../references/plugin-runtime-contract.md`
- `../../references/platform-hosts.md`
- `../../references/mobile-plugin-contract.md`
- The checkout's root `AGENTS.md` and package READMEs when present
- For generic plugin ABI work: `$vesper-plugin-workflow`
- For Android, iOS, or Flutter host-kit surfaces: `$vesper-mobile-flutter-hosts`
- For FFmpeg runtime, profile, and license packaging: `$vesper-ffmpeg-packaging`
- For command selection: `$vesper-validation-playbook`

## Product Boundaries

Keep the plugin families separate unless the user explicitly changes the
architecture:

- SourceNormalizer repairs or remuxes sources before playback consumption.
- Decoder owns packet-to-frame decode and belongs to the native frame lane.
- FrameProcessor transforms decoded frames and therefore depends on a decoder or
  native frame pipeline before it can participate in mobile playback.

SourceNormalizer can participate in mobile playback without taking over decode
when it produces a system-player-readable resource. FrameProcessor cannot
truthfully participate in mobile playback through ExoPlayer or AVPlayer until
the SDK controls decoded frames.

Resolve and attach these capabilities through an immutable `PluginPlan` and a
`PluginScope`. Use the active playback correlation for actual playback and the
next-prewarm correlation for preparation only; prewarm cannot commit the master
clock, video surface, audio sink, or participation state.

Do not add mobile Decoder artifacts or default FrameProcessor playback behavior
as a side effect of SourceNormalizer work.

## SourceNormalizer Routes

Use these route meanings consistently:

- `fMP4 local stream`: disk-backed fragmented MP4 resource consumed by
  ExoPlayer or AVPlayer.
- `HLS short-window`: disk-backed local HLS playlist and segments consumed by
  ExoPlayer or AVPlayer.
- `packet stream`: plugin packet output reserved for a future SDK-controlled
  Decoder or FrameProcessor native frame pipeline.
- `bypassed` or `fallbackOriginal`: original source is still the playback input.

For fMP4 streaming, use fragmented MP4 output flags such as
`frag_keyframe`, `empty_moov`, and `default_base_moof`. Do not reuse file-style
`faststart` as the default streaming profile.

For HLS short-window, enforce a bounded playlist and segment window. Use SDK-side
quota cleanup in addition to muxer delete-old-segment behavior.

## Routing Policy

Default mobile behavior is native-first:

- Normal HLS and DASH should stay on ExoPlayer or AVPlayer native paths.
- On iOS, normal DASH continues through the existing DASH bridge.
- Installing a SourceNormalizer plugin must not automatically intercept normal
  DASH or HLS.
- SourceNormalizer participates only for weird sources, explicit
  `preferNormalized` or `requireNormalized`, test-panel force settings, or an
  adaptive decision that native playback is not suitable.

Mode semantics:

- `disabled`: do not load or preflight.
- `diagnosticsOnly`: load plugin and report capability only.
- `preflightOnly`: open or probe and close; playback still uses the original
  source.
- `preferNormalized`: try normalized playback, then fall back to original source
  with a visible diagnostic on failure.
- `requireNormalized`: fail source selection with a clear error if normalized
  playback cannot be prepared.

For a normal Flutter production opt-in, lead with
`VesperSourceNormalizerConfiguration.preferBundled()` after declaring the
optional native dependency. Use `requireBundled()` for tests or strict ingest
flows. Keep `diagnosticsOnly` and `preflightOnly` as advanced bring-up modes.
SourceNormalizer does not require FrameProcessor.

Only mark `Participated` after the host successfully hands a normalized fMP4 or
HLS resource to ExoPlayer or AVPlayer. Packet stream should not be marked as
participated in the mobile system-player route.

AudioProcessor participation has the same explicit boundary: a Native PCM chain
must be created by an SDK-managed audio route and must report accepted,
processed frames. Installing or resolving an AudioProcessor does not alter the
Android Media3 or iOS AVPlayer DirectNative audio path. WASM cannot participate
in realtime PCM under the standard invocation policy.

For Native AudioProcessor routes, validate `AudioPlaybackPolicy` before opening:
`playback_rate` must be finite and positive, and `PreservePitch` versus
`FollowRate` must be supported by every processor in the chain. Preserve the
input PTS and discontinuity marker. Queue saturation is backpressure, not an
implicit drop or transport switch.

## Resource I/O Rules

Never use unbounded in-memory pipes for media bytes. System players can issue
Range requests, reconnect, read concurrently, and slow-read; use disk-backed
session cache with bounded read buffers.

Required resource behavior:

- bounded file reads
- Range support, including invalid-range errors
- growing-file reads for fMP4 readiness
- client cancel handling that does not necessarily close the whole session
- explicit session close or dispose cleanup
- tokenized access or route-local containment checks
- disk quota and cache usage diagnostics
- bounded retry, wait, skip, and readiness loops with diagnostics when a source
  never becomes ready
- cleanup when session registration, loopback startup, or resource-loader setup
  fails after native or filesystem resources were created

Recommended defaults are small read buffers, session-level disk caps, and a
global normalized cache cap. On quota failure, `preferNormalized` should fall
back and `requireNormalized` should fail clearly.

## Android Shape

Keep the Android normalized resource server inside the main
`vesper-player-kit`. Do not make the main kit depend on external playback
modules for SourceNormalizer playback consumption.

Use a loopback server bound to `127.0.0.1:0` with tokenized URLs, for example:

- `/normalized/{token}/primary`
- `/normalized/{token}/playlist.m3u8`
- `/normalized/{token}/segments/...`

For fMP4, give ExoPlayer a loopback progressive URL. For HLS, give ExoPlayer a
loopback `.m3u8` URL.

The internal bridge may resolve native plugin binaries from its registry, but
public Android, iOS, and Flutter configuration uses `VesperPluginReference`.
Do not expose filesystem paths or put FFmpeg runtime libraries in plugin
references.

## iOS Shape

Keep product semantics separate:

- `vesper-dash://` remains the DASH bridge.
- `vesper-normalized://` belongs to SourceNormalizer playback resources.
- Do not merge DASH and SourceNormalizer delegates into one product-level
  delegate.

It is fine to share low-level AVFoundation resource I/O helpers for content
information, offsets, bounded `FileHandle` reads, growing-file waits, and cancel
cleanup. Shared helpers must not know about DASH manifest generation, ABR,
SourceNormalizer sessions, profile hashes, fallback decisions, or route parsing.

The normalized loader should map session-scoped paths to local resources, verify
root containment, and respond through disk-backed reads. Cancelling a loading
request should cancel that request, not automatically dispose the whole
SourceNormalizer session.

## FFmpeg Runtime Boundary

FFmpeg-backed SourceNormalizer mobile artifacts depend on the shared FFmpeg
runtime:

- Android: shared FFmpeg runtime AAR.
- iOS: `VesperFFmpegAVCodec.xcframework.zip`,
  `VesperFFmpegAVFormat.xcframework.zip`, and
  `VesperFFmpegAVUtil.xcframework.zip`.

SourceNormalizer plugin artifacts must not bundle duplicate runtime libraries
such as `libav*`, `libsw*`, `libxml2*`, `libssl*`, or `libcrypto*`. Runtime and
plugin profile hashes must match, and release scripts should fail on mismatch.

Update `THIRD_PARTY_NOTICES.md` and relevant README files when changing FFmpeg
profile capabilities, bundled libraries, configure flags, runtime layout, or
release artifacts.

## Distribution Versus Publication

- Native dependency/product inclusion and runtime plugin selection are separate.
  The dependency embeds code; `VesperPluginReference` selects an embedded
  identity. FFmpeg runtime components are dependencies, never plugin references.
- Android module existence and local release staging do not prove optional Maven
  coordinates are enabled or remotely consumable.
- The iOS optional package exposes seven direct products, not an aggregate
  optional-plugin library. A consumer contract must choose and document either a
  per-capability closure or the whole sibling set.
- Do not claim distribution closure until a clean external consumer builds from
  hosted coordinates/remote SwiftPM, the final bundle has one matching FFmpeg
  closure, and signed device execution succeeds.

## Diagnostics and Examples

Diagnostics should show at least:

- mode
- route
- participation
- selected profile
- content type
- primary resource or playback URL kind
- disk bytes used and cache quota
- fallback reason or failure message
- concise capability or track summary when available

Examples may default SourceNormalizer to `preflightOnly` or a consciously chosen
test mode, but UI text must not imply that mobile playback was replaced unless
`Participated` is actually true.

FrameProcessor in mobile examples should remain diagnostics or debug logging
only until decoder/native-frame integration is intentionally implemented.

The same restriction applies to AudioProcessor examples: show capability,
policy, queue, and participation diagnostics unless an SDK-managed PCM host
route has been implemented and exercised.

## Validation

Choose the narrow checks for the touched surface, then one integration check that
proves the host-facing behavior.

Rust and plugin bridge:

```sh
cargo test -p player-plugin-abi -p player-plugin -p player-plugin-loader -p player-source-normalizer -p player-source-normalizer-ffmpeg -p player-platform-mobile
./scripts/vesper ffi verify
```

Use `$vesper-validation-playbook` for the Android host kit, iOS host kit,
Flutter DTO/channel, artifact, and device checks required by the changed route.
Resolve cached Gradle and Simulator destinations from the current checkout and
host instead of relying on pinned local identifiers.

Always finish documentation or skill-only edits with:

```sh
git diff --check
```
