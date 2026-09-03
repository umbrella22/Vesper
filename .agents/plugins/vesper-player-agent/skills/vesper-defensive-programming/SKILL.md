---
name: vesper-defensive-programming
description: Use when reviewing or refactoring Vesper guards, lifecycle checks, runCatching, catch blocks, null checks, generation tokens, release ordering, unwrap, expect, panic boundaries, or possible over-defensive programming in Rust, Kotlin, Swift, or Dart.
metadata:
  short-description: Precise guard and cleanup judgment
---

# Vesper Defensive Programming

## Load First

- `../../references/knowledge-map.md`
- `../../references/repository-memory.md`
- `../../references/defensive-boundaries.md`
- `../../references/plugin-runtime-contract.md` for catalog, plan, scope, and
  playback-generation invariants
- The current boundary implementation and its focused regression tests

## Core Rule

Do not classify every guard as over-defensive. Vesper has real risk at
multithreaded lifecycle, external protocol input, FFmpeg, FFI/JNI, platform API,
socket, filesystem, and Flutter channel boundaries.

The goal is not fewer checks. The goal is checks at the layer that actually owns
the invariant.

## Review-Derived Guardrails

Recent repo reviews showed that normal-path correctness is not enough for a
media SDK. Before removing or adding guards, ask whether the code handles
failure shape, long session shape, and host lifecycle shape:

- Boundary code must define ownership, release ordering, stale handle or stale
  lease behavior, failure outputs, and panic/exception mapping.
- Caches, registries, queues, event batches, packet-skip loops, retry loops, and
  pending-frame lists need a cap, timeout, eviction policy, or a proof that they
  cannot grow or spin unbounded.
- Locks, monitors, synchronized methods, and global registries must not cover
  blocking I/O, socket operations, executor shutdown, platform callbacks, or
  long JNI/FFmpeg/plugin calls.
- Async-to-sync bridges need timeout, cancellation handling, and explicit
  fallback or error behavior.
- Unknown warnings, diagnostics, capabilities, and enum values crossing language
  boundaries should be preserved or logged rather than silently collapsed.
- Constructor or registration failure must clean up partially created native
  sessions and leave output handles in a deterministic sentinel state.

For plugin runtime code, keep metadata, resolution, activation, and playback
authority in separate phases. Reject stale catalog/plan fingerprints and stale
scope attachments before invoking executable code. A next-prewarm scope may
prepare state but cannot commit the active clock, surface, audio sink, or
participation projection.

## Usually Necessary

Keep or improve guards that protect:

- release ordering, generation tokens, executor shutdown, active owner, and
  native resource close
- FFI, JNI, plugin ABI, C callbacks, FFmpeg, Media3, AVFoundation, CPAL, and
  Flutter channel boundaries
- XML, SOAP, SSDP, DASH MPD, SIDX, HLS playlist, DLNA protocol info, file URI,
  content URI, socket, and FFmpeg metadata input
- background loops where one failed request should not stop discovery, relay, or
  download workers
- NaN, infinity, out-of-range, overflow, or external enum value inputs crossing
  public API boundaries
- finite-positive AudioProcessor playback rates, supported pitch modes,
  bounded PCM queues, and preservation of host-owned PTS/discontinuity metadata

## Usually Suspicious

Consolidate or remove checks that are:

- repeated several times inside one local pure calculation
- broad `catch (Exception)` or `runCatching` around transformations that cannot
  throw
- sync and async implementations that duplicate business logic instead of
  sharing one core path
- dead null branches after a checked wrapper or state machine has already proven
  a value exists
- repeated Range, header, diagnostic, or processor-loop code in sibling methods

## Rust Rules

- Library code must not use `unwrap()` or `expect()`.
- Map impossible-looking failures to `Result` or a deterministic fallback.
- Reject or normalize non-finite public numeric input before `Duration` or media
  time conversions.
- Use `unwrap_or_else(|e| e.into_inner())` for poisoned mutex recovery when the
  repo convention expects best-effort recovery.
- At ABI boundaries, use `catch_unwind` and map panic to an error status.
- Pure logic crates stay `#![deny(unsafe_code)]`.

## Kotlin, Swift, Dart Rules

- Narrow exception handling to the failure class when possible, such as
  `IOException` for client disconnect.
- Do not make host apps fix SDK initialization ordering that the plugin can
  handle internally.
- Do not hide Java, Swift, or platform exceptions in a way that prevents a host
  from seeing unsupported, security, network, decode, or platform failures.
- Keep lifecycle stop/dispose paths idempotent, but do not report normal stop
  races as user-visible errors.

For a multi-stage command such as Android subtitle selection, use one total
deadline across readiness, apply, readback, and any proven retry. Wait on the
exact stable-ID target owned by the platform boundary and confirm from exact
readback. A generic generation/event callback may trigger another observation
but must not be treated as success. Preserve command/source generations so stale
callbacks, superseded commands, source switches, and dispose cannot complete a
newer transaction.

## Review Output

When reviewing defensive code, separate findings into:

- necessary guard
- misplaced guard
- duplicated control flow
- broad exception swallowing
- missing boundary guard
- panic or unwind risk

Then propose the smallest change that moves the check to the right owner.
