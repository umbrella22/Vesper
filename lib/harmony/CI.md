# HarmonyOS artifact builds

`.github/workflows/harmony-ci.yml` runs Rust/Python contracts on Ubuntu and
builds preview artifacts on a GitHub-hosted `macos-15` arm64 runner. Relevant
main-branch pushes, pull requests and manual dispatches trigger the workflow.
Fork pull requests run contracts without access to the private SDK. The workflow
does not publish to OHPM or create a Vesper GitHub Release.

## Toolchain provisioning

`scripts/harmony/ci-toolchain.json` pins a private release asset, its byte size
and SHA-256. `ci_toolchain.py install --release-manifest` downloads that exact
asset with GitHub CLI, checks its size/hash before extraction, rejects archive
escapes and validates the stable API 26 tool layout. Executable permissions and
relative SDK links survive extraction. A failed installation leaves no final
tool directory. The archive excludes emulator images, emulator instances,
user-home caches and Finder metadata.

The `HARMONY_SDK_TOKEN` repository Actions secret grants Contents: read access
to the toolchain repository named in the manifest. GitHub also requires Metadata:
read. The workflow's ordinary `GITHUB_TOKEN` has access only to the Vesper
repository and cannot download another private repository's assets. A dedicated
fine-grained token limits that additional access. Rotate the token before its
configured expiry and replace the Actions secret without changing source.

The SDK retains its vendor license/notice and is stored separately from public
Vesper artifacts. SDK directories and archives are excluded from Actions caches
and uploaded build artifacts. Only the SDK download step receives the dedicated
credential. The default workflow token remains `contents: read`.

To reproduce provisioning with an authenticated GitHub CLI:

```sh
python3 scripts/harmony/ci_toolchain.py install \
  --release-manifest scripts/harmony/ci-toolchain.json \
  --directory /path/to/temporary/harmony-command-line-tools
```

A new local toolchain can be archived with `ci_toolchain.py pack --tools <path>
--output <archive.tar.gz>`. Upload the archive and its checksum/version sidecars
to the restricted toolchain storage, then update the pinned manifest together.
An archive replacement with different bytes fails the existing checksum pin.

## Build and package

CI installs the repository-pinned Rust toolchain and OHOS target, installs
`cbindgen 0.29.4`, and fetches locked host/OHOS dependencies before offline Cargo
builds. OHPM/Hvigor use the bundled tools and local HAR dependencies.

`ci-artifacts.py` runs Rust tests, native/transpiled host contracts, the three HAR
builds and four consumers: core, UI, no-INTERNET and standalone example. Each HAP
is checked for Release metadata, API 26 floor, arm64 native bytes and applicable
permission/dependency contracts. Example receipts bind the portable source file
inventory as well as the HAP and vendored HARs.

```sh
cargo fetch --locked --target aarch64-apple-darwin --target aarch64-unknown-linux-ohos
python3 scripts/harmony/ci-artifacts.py \
  --tools /path/to/harmony-command-line-tools \
  --sdk-archive-sha256 <pinned-archive-sha256>
```

`freeze-candidate.py --without-media` records a build-only candidate with an
empty media inventory and `runtimeAcceptanceReady: false`. Source/HAR/HAP/receipt
and symbol hashes remain mandatory. Runtime acceptance rejects this scope before
device access. Running a simulator matrix requires prepared media and the
ordinary media-bound freeze operation.

`package_preview.py` requires all build checks and the portable example to match
the candidate. It emits the preview ZIP, a separate native symbols ZIP, outer
checksums and a packaging receipt in `target/harmony-kit/release/`. Every ZIP has
an internal checksum inventory. No research media or toolchain archive is copied
into the preview.

| Actions artifact | Content | Retention |
| --- | --- | --- |
| `harmony-preview-<run number>` | Three HARs, unsigned example, local-HAR example source, licenses, checksums and build evidence | 30 days |
| `harmony-symbols-<run number>` | Two native debug-symbol files and identity/checksums | 30 days |
| `harmony-build-evidence-<run number>` | Host/build logs, including failed builds | 14 days |

The [testing guide](TESTING.md) describes device signing, local integration and
issue reports. Successful artifacts establish build/host checks for their source
identity. Runtime/device results and later OHPM publication have separate receipts.
