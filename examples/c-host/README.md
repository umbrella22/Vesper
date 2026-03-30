# Vesper C Host Smoke Example

This directory contains a minimal C host for the current Vesper `player-ffi` prototype.

It is intentionally narrow:

- probe media
- read media info
- initialize the player
- dispatch `Play`
- drain startup events

It does **not** render frames yet. The goal is validating that the current header, ABI, and
library output can be consumed by a plain C host.

## Quick Start

From the project root:

```bash
scripts/run-c-host-smoke.sh
```

Use a different source:

```bash
scripts/run-c-host-smoke.sh /absolute/or/relative/path/to/video.mp4
```

Build only:

```bash
scripts/run-c-host-smoke.sh --build-only
```

## Current Notes

- The checked-in `include/player_ffi.h` header is generated from `crates/core/player-ffi` via
  `cbindgen`.
- Run `scripts/generate-player-ffi-header.sh` to regenerate it.
- Run `scripts/verify-player-ffi-header.sh` to confirm it is up to date.
- The Rust library is currently built from `crates/core/player-ffi`.
- This example is a smoke test, not a production host shell.
