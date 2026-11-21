# HarmonyOS ArkUI host

The standalone Vesper Player example consumes the three staged Harmony HARs on
API 26+ arm64 phones and tablets. Its **Play**, **Diagnostics**, and **Downloads**
pages cover the same workflows as the Android Compose and iOS SwiftUI examples
using HarmonyOS components: `Navigation`, native bottom `Tabs`, `List`, sheets,
sliders, switches and system colors. Wide windows use a player/sidebar layout.

## Try the app

Download a [preview artifact](../../lib/harmony/TESTING.md) for the matching
unsigned HAP and a portable example project with three vendored HARs. Open the
project in DevEco Studio and configure device signing to run on a physical phone
or tablet. CI artifact checks cover builds and host contracts; device playback
results retain their own source identity.

The Diagnostics tab runs bounded checks on the selected video: progression,
alternate audio, seeking to the final 15 seconds and completion, and live DVR
availability. The user can record audible-output and device/simulator observations,
then copy or save a local JSON report through the document picker. Reports exclude
media URLs, headers and private paths, and are never uploaded automatically.
See the [diagnostic feedback contract](../../lib/harmony/vesper-player-kit/diagnostic-feedback.md)
for sample timing, evidence limits and report fields.

The queue starts with two offline demos: a 12-second MP4 studio pattern and a
static single-Period SegmentBase DASH sample. Both are generated test media
bundled with the app. The third and fourth entries are Apple's public HLS and
Akamai's Envivio DASH demonstrations, matching the other native examples and
requiring an internet connection. Public demonstration URLs can change or become
unavailable independently of the SDK.

The primary source actions open the document picker, public DASH sample, public
HLS sample or URL sheet. The bundled DASH fixture remains available in the queue.
Select a queue entry to play, or paste an HTTP(S) media URL. Automatic protocol
detection uses the URL path; select MP4, HLS, or DASH explicitly for endpoints
without a media extension. Query parameters are preserved. Embedded URL
credentials are rejected. **Choose file** opens the system document picker and
imports a private copy of an MP4/M4V/MOV video. The queue holds twelve entries;
imports share a 1 GiB budget and are cleaned up when the app closes or restarts.
The first bundled sample is prepared without autoplay so the Stage's Play
button works immediately after setup.

The queue preview shows up to five entries around the active video. **Manage
queue** opens the complete list, current playback/preload state, custom-item
removal and eligible remote MP4/DASH preload actions. Previous/next navigation
remains explicit; completion does not automatically advance the sequence.

The Stage supplies play/pause/replay, ten-second skips, a seek timeline,
fullscreen, track/subtitle settings, rate, volume, brightness gestures, loading
and retry states, and a control lock. Playing controls hide after three seconds
without interaction; tap the video to reveal them. Double tap plays or pauses,
and holding a playing VOD temporarily uses 2× before restoring its exact previous
rate. Volume and brightness adjust continuously with a visual HUD. Speed/quality
shortcuts appear when measured space permits; other actions stay in More. Inline
playback defaults to compact; fullscreen defaults to expanded with time, scrubber
and bottom actions. Audio, subtitles, quality, speed and caption styles use
separate content-sized native settings panes.
The app adds previous/next, Picture in Picture, and an **App settings**
sheet for independent inline/fullscreen layouts, optional system-blue accent,
reduced motion and persisted System/Light/Dark
appearance. System/Light/Dark choices appear above the Stage. Phones and narrow
windows use a 248 vp inline Stage; the header, theme choices, Stage and playback
sections scroll together, following the shared Android/iOS content arrangement.
Wide windows fill the remaining left column with the Stage and scroll the right
playback panel independently. The host owns this allocation; the SDK Stage fills
the dimensions supplied by the host. Fullscreen temporarily suppresses the
surrounding content and restores the preceding scroll position on exit. Inline
Stage geometry has a straight outer edge.
One controller, binding, and mounted Stage are
retained across page, appearance, control-layout, and fullscreen changes.
Native video keeps its aspect ratio, with black bars in wider or taller areas.
Entering fullscreen for landscape video requests a landscape window; portrait
video keeps the host's orientation. Exit, failed entry and page closure restore
the preceding layout and orientation after pending native changes have settled.
The application follows the system font size up to 2×. The AppScope
configuration profile is part of the standalone build; Stage shortcuts yield
space to essential actions as measured text grows.

Actions use Huawei system symbol resources through `SymbolGlyph` and
`SymbolGlyphModifier`, including play, pause, skip, fullscreen, PiP, settings,
download and share. No external icon font or copied platform icon assets are
required. Example icons have accessible labels and 48 vp touch targets; SDK Stage
buttons retain at least 44 vp targets.
The resources are compiled with the HarmonyOS API 26 SDK.

**System playback** controls the app's AVSession/background-audio integration.
Automatic PiP starts disabled. Enabling it creates and configures the native PiP
owner immediately, without requiring a manual PiP session first. Manual and
automatic PiP borrow the same Surface; actual native PiP state suppresses Stage
interaction until restoration. **Playback options** exposes
System/Responsive/Resilient buffer and resource-policy profiles, separate opt-in bounded
playback recovery, one-time audio/subtitle language preferences, startup preload/
cancellation and disk cache clearing. Diagnostics preserves requested/effective/
unsupported parameters. Responsive requests a ten-second buffer preference and
a 500 ms start threshold; Resilient requests twenty seconds and two seconds.
These native hints apply on the next activation, and local `fdSrc` playback
reports them as unsupported. Selecting System clears those explicit preferences.
Diagnostics separates the current epoch's submitted native hints from pending
policy requests, accepted track-filter readback and observed output.
Resource retry affects future SDK requests; playback
recovery explicitly reactivates and waits for rendering. Pause/source changes
cancel recovery, and expired credentials require host source refresh.
Preload uses two workers, a four-job pending capacity, an 8 MiB memory budget and
a 30-second lifetime. Local files and native HLS do not support standalone startup
preload. An eligible queued remote MP4 or static SegmentBase DASH source can be
preloaded from **Manage queue**. Completion and playback reuse are distinct facts.
Disk bytes use a separate 32 MiB budget; clearing them retains the active
playback's memory ownership. The app closes the disk owner after player disposal.

Diagnostics shows the current timeline, first frame, dimensions, playback epoch,
Surface identity, resource counters, and platform capabilities. Its history holds
the latest eighty state/rate/selection changes and app errors. Track selection
requests, confirmed tracks, effective selections, native PiP/SystemSession state,
policy and preload/cache outcomes are also inspectable. **Copy report** copies player
diagnostics, capabilities, task snapshots, and history without source descriptors
or request headers. Unknown decoder, HDR, or audio-output evidence stays unknown.

Downloads accepts finite progressive MP4, unencrypted finite HLS, and the
supported static SegmentBase DASH profile. Tasks can be paused, resumed, played
offline, or deleted, and their checkpoints survive process restarts. For a
restored incomplete task, enter the original media URL before resuming: the SDK
does not persist access credentials. Play another video before deleting the
currently playing offline asset. **Save** opens the system destination picker;
**Share** opens the system chooser. Eligible originals and segmented ZIP outputs
share a 256 MiB limit including ZIP overhead. Cards explain export eligibility
and disable unavailable actions. Streaming uses bounded buffers/concurrency and
a 512 MiB temporary directory budget. The share message confirms panel
presentation; a share retains its temporary file for ten minutes.
Restored prefixes are reused only after chunk hashes, ETag, length and locator
fingerprint agree. Changed locators restart transfer; failed tasks require source
refresh. User titles, rather than internal asset IDs, identify tasks.

## Build from staged HARs

Run from the repository root, with the actual Command Line Tools location:

```sh
./scripts/vesper harmony --command-line-tools /path/to/command-line-tools kit-har --offline
./scripts/vesper harmony --command-line-tools /path/to/command-line-tools kit-consumer \
  --har target/harmony-kit/dist/vesper-player-kit.har --example
```

The builder copies this example's sources, resources, and application metadata
into a retained Hvigor project under `target/harmony-kit/consumer-*`, then resolves
the copied HARs with versioned dependencies and local overrides. The consumer
step compiles neither Rust nor library source paths. It writes:

- `target/harmony-kit/dist/vesper-harmony-example-unsigned.hap`
- `target/harmony-kit/dist/vesper-harmony-example-unsigned.build.json`

The build record binds the app to its working-source fingerprint, HAR hashes
and portable project file inventory.
Rebuild HARs after changing fingerprinted inputs. Install the unsigned local HAP
on a provisioned development simulator; production distribution requires signing
and registry acceptance. Example changes require their own runtime verification;
earlier SDK-candidate results do not validate a rebuilt app.

```sh
/path/to/command-line-tools/sdk/default/openharmony/toolchains/hdc install \
  target/harmony-kit/dist/vesper-harmony-example-unsigned.hap
/path/to/command-line-tools/sdk/default/openharmony/toolchains/hdc shell aa start \
  -b dev.vesper.harmony.example -a EntryAbility
```

No fixture server is required for bundled playback. For controlled network and
download checks, start the [fixture server](../../scripts/harmony/fixture-server.py)
with `--root target/harmony-kit/media --allow-anonymous`, then reverse port 18765
with `hdc rport tcp:18765 tcp:18765` and enter a fixture URL in the app. The
explicit anonymous option allows this URL-only app to use the loopback test
origin; the server requires its probe header by default. The ability declares INTERNET and background audio, respects the
host orientation outside fullscreen, and supports fullscreen, split, and floating
windows. Fullscreen landscape playback temporarily applies the policy described above.

## Implementation and checks

- `pages/Index.ets` owns the stable Stage, native window, safe areas, and navigation.
- `components/` contains the three pages and shared presentation controls.
- `model/ExampleModel.ets` orchestrates the public kit APIs and owns app resources.
- `model/ExampleSupport.ets` contains URL selection, palette, and bounded history helpers.
- `model/ExampleFullscreen.ets` owns orientation changes and native restoration.
- `tests/` covers URL/progress contracts and app ownership, cancellation, retry,
  offline deletion, and export regressions.

The shared `kit-host-contracts` command runs these app tests as well as the SDK
host suites. App-only checks can run directly with the bundled Node/TypeScript:

```sh
export VESPER_HARMONY_TYPESCRIPT=/path/to/command-line-tools/hvigor/hvigor-ohos-plugin/node_modules/typescript/lib/typescript.js
/path/to/command-line-tools/tool/node/bin/node examples/harmony-arkui-host/tests/support.cjs
/path/to/command-line-tools/tool/node/bin/node examples/harmony-arkui-host/tests/model.cjs
/path/to/command-line-tools/tool/node/bin/node examples/harmony-arkui-host/tests/fullscreen.cjs
```

See the [platform guide](../../lib/harmony/README.md) for media profiles, resource
lifetimes, diagnostics, and the preview's device-verification boundaries. This
example does not establish physical decoder/HDR/audio evidence or expand the
supported protocol and DRM surface.
