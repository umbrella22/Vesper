# HarmonyOS preview testing

Preview bundles target API 26+ arm64 phones and tablets. Playback uses Media Kit
AVPlayer and a native Surface. The build job checks Rust/host contracts, three
Release HARs and four HAR-only consumers. Each bundle records its own source
identity; runtime results require a separate device or simulator observation.

## Download and install

Open [HarmonyOS preview artifacts](https://github.com/umbrella22/Vesper/actions/workflows/harmony-ci.yml),
select a successful run and download `harmony-preview-<run number>`. Extract the
Actions archive, then the `vesper-harmony-<version>-<source>.zip` inside it.
`SHA256SUMS` covers the preview and separate symbols archive; the preview also
contains a checksum inventory for every payload file.

The bundle contains:

| Path | Content |
| --- | --- |
| `hars/` | Core, ArkUI binding and optional controls HARs |
| `example/` | Unsigned Release example HAP |
| `example-source/` | Matching example project with local HARs in `vendor/` |
| `build-identity.json` | SDK version, source SHA-256, Git revision, workflow and verification scope |
| `evidence/` | Build checks and artifact receipts |

For a physical device, open `example-source/` in DevEco Studio with stable
Command Line Tools/SDK 26. Configure an application bundle identifier and a
development signing profile authorized for that device, then build and run the
entry module. Account/device registration and signing follow DevEco's current
signing interface. A downloaded unsigned HAP is not directly installable on an
ordinary physical device. The compiled application requires API 26 or later.

On a development simulator that accepts unsigned HAPs, select its exact target:

```sh
hdc list targets
hdc -t <target> install example/vesper-harmony-example-unsigned.hap
hdc -t <target> shell aa start -a EntryAbility -b dev.vesper.harmony.example
```

## Integrate the HARs

Copy the three HARs into the application's `vendor/` directory. Declare local
entry dependencies and matching root overrides, as in `example-source/`:

```json
{
  "dependencies": {
    "@vesper/player-kit": "file:../vendor/vesper-player-kit.har",
    "@vesper/player-kit-arkui": "file:../vendor/vesper-player-kit-arkui.har",
    "@vesper/player-kit-arkui-ui": "file:../vendor/vesper-player-kit-arkui-ui.har"
  }
}
```

Root overrides use the same package names with `file:./vendor/<name>.har`.
Resolve dependencies with `ohpm install --all`. Retain matching versions and
HARs from one bundle; the dependency order is controls → binding → core.
An application using the core alone can omit the two ArkUI packages.
Application manifests declare their own network/background permissions.
OHPM registry publication is deferred.

## Exercise playback and layout

Start with the bundled MP4 and static DASH samples to establish local playback.
Then test the public HLS/DASH examples or another public reproducible source.
Public origins can fail independently of the SDK. Record network conditions and
repeat with bundled/local media when identifying a network-dependent failure.

Check play/pause/replay, seeking, ten-second skips, rate, quality/audio/subtitle
selection where available, volume, gestures, fullscreen, lock and PiP. Check
system playback, source replacement, background/foreground transitions and
download pause/resume/offline/export separately. Unsupported actions retain an
explicit reason. A track catalog entry or completed selection request does not
establish audible output.

For layout issues, record screen resolution, window dimensions, orientation,
system font size and split/floating-window mode. Capture both the surrounding
page and the Stage. Wide example windows allocate the remaining left column to
the Stage; video preserves its aspect ratio inside that allocation. Compare
phone, tablet and resized windows without changing the active source.

## Report an issue

Use the [HarmonyOS issue form](https://github.com/umbrella22/Vesper/issues/new?template=harmonyos.yml).
Include the workflow run URL or `build-identity.json`, device model, OS/API build,
reproduction steps and expected/observed behavior. A screenshot or short video
helps with layout and lifecycle reports. Native crash investigations also need
the corresponding `harmony-symbols-<run number>` download.

In the example's Diagnostics tab, select the device/simulator environment and
record the audible-output observation. Run the relevant bounded scenario, then
use **Copy report** or **Save report** to attach the local diagnostic JSON.
Reports include `sdkSourceSha256`, native codes, timeline/tracks and bounded
transport observations; they exclude media URLs, credentials and private paths.
Review manually attached logs and media locators before submission. The
application sends no reports automatically.

## Current verification limits

CI artifact jobs do not start simulators or perform physical-device acceptance.
Their receipts mark runtime playback, audible output, HDR, power, accessibility
and production signing as unverified. Earlier runtime receipts apply only to
their recorded source/artifact identities.

Live DVR and dynamic DASH remain gated. Separate-audio HLS has profile-specific
limitations on the tested API 26 simulator. API 26 native static DASH alternate
audio is confirmed during fresh playback in the controlled fixture; its short
seek/rate phase remains unconfirmed. These observations do not identify whether
commercial-device behavior is affected or establish a vendor/Beta repair.
The SDK adds no independent decoder or media playback engine. Actual AVPlayer
decoder identity and audible output require their own evidence.
