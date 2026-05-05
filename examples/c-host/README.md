# Vesper C Host Smoke Example

A minimal C host that demonstrates how to consume the Vesper `player-ffi` C
ABI from a plain C program. Useful as a starting point for integrating the SDK
into C/C++ applications, native plugins, or non-Rust runtimes.

This example covers:

- Media probing
- Reading media info
- Initializing the player
- Dispatching `Play`
- Draining startup events

Frame rendering is intentionally out of scope. This is a smoke test for the
FFI surface, not a production host shell.

## Quick Start

From the project root:

```sh
scripts/vesper ffi c-host-smoke
```

With a custom source:

```sh
scripts/vesper ffi c-host-smoke /absolute/or/relative/path/to/video.mp4
```

Build without running:

```sh
scripts/vesper ffi c-host-smoke --build-only
```

## FFI Header

The public C header is checked in at
[`include/player_ffi.h`](../../include/player_ffi.h) and is generated from
`crates/core/player-ffi` via `cbindgen`.

- Regenerate: `scripts/vesper ffi generate`
- Verify it is up to date: `scripts/vesper ffi verify`

## Handle Semantics

- `PlayerFfiInitializerHandle` and `PlayerFfiHandle` are generation-checked
  value handles, not ownership-carrying raw pointers.
- Zero-initialize handles for empty storage.
- Stale, consumed, or double-destroyed handles return
  `PLAYER_FFI_ERROR_CODE_INVALID_STATE`.
