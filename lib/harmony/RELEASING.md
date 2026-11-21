# HarmonyOS preview release

The preview distributes three API 26+ arm64 HARs in dependency order:
`@vesper/player-kit`, `@vesper/player-kit-arkui`, and
`@vesper/player-kit-arkui-ui`. The current package version is
`0.9.1-harmony.0`. The core includes Rust and Node-API libraries; the other two
packages declare exact versioned dependencies.

Preview distribution currently uses [CI artifacts](CI.md) and local HAR
dependencies. Device feedback follows the [testing guide](TESTING.md). The
artifact workflow requires no OHPM account and performs no registry upload.
The registry procedure below applies when publication is enabled separately.

## Registry account and namespace

Publication requires an account on the [OHPM registry](https://ohpm.openharmony.cn/),
the registry's publisher identity (`publish_id`), and its associated SSH private
key. Account verification, public-key registration and publisher permissions are
completed in the registry's current account/publishing interface. The installed
CLI accepts an existing identity/key pair and does not create an account.

The publisher must have permission to publish all three packages under
`@vesper`. Namespace availability and permission remain unverified until the
registry confirms them. If that namespace cannot be obtained, change all three
package names, dependencies, imports, build helpers and examples together, then
rebuild and revalidate before uploading.

The private key remains outside the repository. The publish ID and key path are
provided locally or through the release runner's credential store. Neither the
private key nor credential-bearing configuration belongs in artifacts, logs or
diagnostic reports. CLI `ohpm help publish` describes the supported arguments.

## Build and verify

The release checkout must contain the intended source changes, with unrelated
working changes excluded. Build all HARs and consumers from the same source
identity. The builder rejects source drift during compilation; the freezer
checks HAR/consumer receipts before recording hashes. Local overrides prove HAR
consumption and do not prove registry resolution.

```sh
VESPER_HARMONY_TOOLS=/path/to/command-line-tools
python3 scripts/harmony/host-contracts.py --tools "$VESPER_HARMONY_TOOLS"
python3 scripts/harmony/build-kit.py --tools "$VESPER_HARMONY_TOOLS" --offline
python3 scripts/harmony/build-consumer.py --tools "$VESPER_HARMONY_TOOLS" --har target/harmony-kit/dist/vesper-player-kit.har
python3 scripts/harmony/build-consumer.py --tools "$VESPER_HARMONY_TOOLS" --har target/harmony-kit/dist/vesper-player-kit.har --ui
python3 scripts/harmony/build-consumer.py --tools "$VESPER_HARMONY_TOOLS" --har target/harmony-kit/dist/vesper-player-kit.har --no-internet
python3 scripts/harmony/build-consumer.py --tools "$VESPER_HARMONY_TOOLS" --har target/harmony-kit/dist/vesper-player-kit.har --example
python3 scripts/harmony/verify-artifacts.py --directory target/harmony-kit/dist --hap target/harmony-kit/dist/vesper-harmony-ui-consumer-unsigned.hap
python3 scripts/harmony/verify-artifacts.py --directory target/harmony-kit/dist --hap target/harmony-kit/dist/vesper-harmony-kit-consumer-offline-unsigned.hap --no-internet
```

Prepare media through `fixture-server.py --prepare-only` before freezing a new
checkout. Record API 26 runtime receipts, retained failures and fixture hashes.
Stop emulator instances before native builds on the local host. Then run:

```sh
python3 scripts/harmony/freeze-candidate.py --tools "$VESPER_HARMONY_TOOLS"
"$VESPER_HARMONY_TOOLS/bin/ohpm" prepublish target/harmony-kit/dist/vesper-player-kit.har
"$VESPER_HARMONY_TOOLS/bin/ohpm" prepublish target/harmony-kit/dist/vesper-player-kit-arkui.har
"$VESPER_HARMONY_TOOLS/bin/ohpm" prepublish target/harmony-kit/dist/vesper-player-kit-arkui-ui.har
```

`prepublish` validates content without publishing. Source-code inclusion warnings
are expected for these Apache-2.0 source HARs. License files and C++ runtime
notices remain packaged. Warnings do not establish namespace permission or
successful upload. Preserve the candidate, build receipts, artifact SHA-256
values and separated debug symbols.

## Publish in dependency order

Before uploading, inspect the registry for existing versions:

```sh
"$VESPER_HARMONY_TOOLS/bin/ohpm" info @vesper/player-kit@0.9.1-harmony.0
"$VESPER_HARMONY_TOOLS/bin/ohpm" info @vesper/player-kit-arkui@0.9.1-harmony.0
"$VESPER_HARMONY_TOOLS/bin/ohpm" info @vesper/player-kit-arkui-ui@0.9.1-harmony.0
```

Network/authentication errors do not mean that a name is available. Treat
published versions as immutable. A conflicting version requires a coordinated
new version and fresh artifacts. After the account and namespace are ready,
provide local values for `VESPER_OHPM_PUBLISH_ID` and `VESPER_OHPM_KEY_PATH`:

```sh
"$VESPER_HARMONY_TOOLS/bin/ohpm" publish target/harmony-kit/dist/vesper-player-kit.har --tag preview --publish_registry https://ohpm.openharmony.cn/ohpm/ --publish_id "$VESPER_OHPM_PUBLISH_ID" --key_path "$VESPER_OHPM_KEY_PATH"
"$VESPER_HARMONY_TOOLS/bin/ohpm" publish target/harmony-kit/dist/vesper-player-kit-arkui.har --tag preview --publish_registry https://ohpm.openharmony.cn/ohpm/ --publish_id "$VESPER_OHPM_PUBLISH_ID" --key_path "$VESPER_OHPM_KEY_PATH"
"$VESPER_HARMONY_TOOLS/bin/ohpm" publish target/harmony-kit/dist/vesper-player-kit-arkui-ui.har --tag preview --publish_registry https://ohpm.openharmony.cn/ohpm/ --publish_id "$VESPER_OHPM_PUBLISH_ID" --key_path "$VESPER_OHPM_KEY_PATH"
```

Wait for each registry result before uploading its dependent. If the process
stops after publishing a dependency, record that partial state and inspect the
existing package before resuming. Do not unpublish an accepted dependency to
repair a later upload. Publication success, registry indexing and installation
success have separate receipts. Inspect `ohpm dist-tags list <package>` to
confirm that the intended preview tag points to the tested version.

## Verify installation and announce

Create a fresh API 26+ arm64 consumer outside the local build staging folders.
Declare the three exact registry versions in its `oh-package.json5`, remove all
`file:` dependencies and local overrides, and use a fresh project-local cache.
Run `ohpm install --all`, inspect the lockfile and dependency graph, then compile
and run the example's essential playback and diagnostics workflows. This checks
transitive resolution, native library packaging and public imports. Existing
`build-consumer.py` outputs select local HARs and cannot substitute for this check.

The release record contains source/artifact identities, package versions and
registry links, clean-consumer receipts, tested runtime builds, known profile
failures and unverified observations. AVPlayer selects its system decoder; a
missing hardware catalog match or unknown actual decoder is not a release gate.
Physical audio/HDR/interruption/power evidence remains unverified without
blocking the scoped preview. Failed or unverified DVR/dynamic DASH capabilities
remain gated. Simulator results do not establish behavior on every device.

The example HAPs are unsigned Release artifacts. Running the example on a
physical device requires host signing/provisioning separately from HAR registry
publication. Release feedback uses the [local diagnostic workflow](vesper-player-kit/diagnostic-feedback.md);
it does not configure automatic uploads.
