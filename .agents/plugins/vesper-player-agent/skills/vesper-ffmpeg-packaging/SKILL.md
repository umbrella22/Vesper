---
name: vesper-ffmpeg-packaging
description: Use when changing Vesper FFmpeg profiles, configure flags, prebuilt payloads, Android FFmpeg runtime AAR, relay remux, download remux, player-remux-ffmpeg, DLNA DASH fallback, packaging scripts, notices, LGPL/GPL/nonfree licensing, or FFmpeg-backed release artifacts.
metadata:
  short-description: FFmpeg profile and packaging rules
---

# Vesper FFmpeg Packaging

## Load First

- `../../references/knowledge-map.md`
- `../../references/repository-memory.md`
- `../../references/ffmpeg-contract.md`
- `../../references/plugin-runtime-contract.md` when FFmpeg-backed plugin
  dependency metadata participates in catalog resolution or plan creation
- `../../references/platform-hosts.md` for Android/iOS artifact ownership
- The checked-in profile document, package READMEs, and notices in the checkout

## Non-Negotiables

- Vesper source remains Apache-2.0.
- Optional FFmpeg-backed artifacts keep FFmpeg's own license, notices,
  corresponding source, configure metadata, and LGPL relinking obligations.
- Do not add `--enable-gpl` or `--enable-nonfree` silently.
- Do not bundle `libav*` in both a runtime AAR and a feature plugin.
- Do not use Gradle `pickFirst` to hide duplicate FFmpeg runtimes.
- Do not reintroduce per-consumer requirements files when the shared profile
  system can represent the need.

## Profile Model

`scripts/ffmpeg-profiles.toml` is the source of truth. Profiles describe
capabilities; platforms are build targets.

Expected profile fields include:

- `extends`
- `libraries`
- `demuxers`
- `muxers`
- `protocols`
- `parsers`
- `bsfs`
- `tls`
- validation such as `forbid_network` and `forbid_openssl`
- `platform_overrides.<platform>`

Merging should preserve order, deduplicate arrays, and apply platform overrides
last. CLI overlays may add explicit capability, but must still pass validation.

## Android Split

- `vesper-player-kit-ffmpeg-runtime` is the only AAR carrying FFmpeg runtime
  libraries and assets.
- External playback relay JNI and download remux plugins carry only their glue
  binary.
- Runtime and plugin profile hashes must match.
- Represent the FFmpeg closure as bounded runtime dependency metadata in the
  plugin catalog. Resolve it into the immutable plan before loading plugin glue;
  catalog and plan objects never retain live FFmpeg handles.
- Baseline host AAR must remain usable without FFmpeg payload.

## Gradle Input Ownership

For a task that can generate `third_party/ffmpeg/android` or another runtime
directory, do not register that initially absent directory as an unconditional
input. Declare the CLI, Cargo manifests, profile/source-policy files, feature
crate, ABI, build profile, and FFmpeg profile that produce it, and treat the
generated directory as task local state/output.

If `--skip-ffmpeg-runtime` means an existing runtime is required, register that
directory as an input only in skip mode and register the skip flag itself as an
input property. Missing generated state may proceed to the generator; missing
required prebuilt state must fail explicitly. Verify with the real Gradle build
task from an initially absent directory; configuration and dry-run commands do
not exercise input snapshot validation.

## Relay Remux

Relay remux is remux only:

- no transcoding
- no DRM
- no site-specific headers or business rules
- no DASH MPD rewrite to make TVs pull origin segments
- no default FFmpeg network dependency

For DASH fallback, prefer host/platform fetching plus local pipe/file input to
FFmpeg. Keep diagnostics stable enough for Flutter, Android logs, HTTP error
bodies, and real-device matrices.

Important diagnostic codes include:

- `missing_runtime`
- `profile_mismatch`
- `unsupported_device_caps`
- `unsupported_encrypted_dash`
- `unsupported_dash_layout`
- `unsupported_dynamic_dash`
- `host_fetch_failed`
- `host_fetch_timeout`
- `ffmpeg_open_failed`
- `ffmpeg_muxer_missing`
- `remux_timeout`
- `range_not_ready`
- `client_cancelled`

## Documentation and Notices

Any change to configure flags, external libraries, profile capabilities,
prebuilt payloads, bundled runtime layout, or remux packaging must update:

- `THIRD_PARTY_NOTICES.md`
- relevant Android, iOS, Flutter, or package README
- relevant package changelog when public packaging behavior changes
- release notes when release artifacts change

## Validation

```sh
./scripts/vesper ffmpeg --list-profiles
./scripts/vesper ffmpeg --platform android --profile default --dry-run
./scripts/vesper ffmpeg --platform ios --profile default --dry-run
./scripts/vesper ffmpeg --platform android --profile default --verify-only
./scripts/vesper mobile verify-no-remux android
```

For Android Gradle packaging, use the local cached Gradle distribution per root
`AGENTS.md`, not online wrapper downloads.
