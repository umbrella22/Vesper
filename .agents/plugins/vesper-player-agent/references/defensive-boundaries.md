# Vesper Defensive Boundary Contract

Keep defensive checks where the boundary owns the invariant. Do not remove a
guard merely because the normal path is simple.

## Required Boundary Semantics

- Define ownership, release order, stale-handle/lease behavior, failure output,
  and panic/exception mapping for FFI, JNI, plugin ABI, Swift, Kotlin, Dart,
  and platform callbacks.
- Use sentinel initial state, synchronized or atomic creation, idempotent close,
  stale-token rejection, and constructor-failure cleanup for session objects.
- Use `catch_unwind` at Rust C/plugin boundaries and map panic to a status.
- Preserve unknown cross-language warnings, diagnostics, capabilities, and enum
  values instead of silently mapping them to a known value.

## Bounded Work

Every media-driven queue, cache, registry, retry loop, packet-skip loop, event
batch, pending-frame list, and readiness wait needs a capacity, timeout,
eviction rule, bounded iteration, or a documented proof that it cannot grow.

One logical command should normally own one total deadline. Readiness, apply,
readback, and any evidence-backed internal retry share the remaining budget;
they must not each receive a fresh full timeout. For subtitle selection, wait on
the exact stable-ID TEXT target and confirm from exact selection-parameters
readback. A generic generation callback may wake a recheck but is not success
evidence.

## Lock And Async Rules

- Do not hold mutexes, monitors, synchronized methods, or global registry locks
  across blocking I/O, socket operations, executor shutdown, platform callbacks,
  or long JNI/FFmpeg/plugin calls.
- Async-to-sync bridges require timeout, cancellation handling, and an explicit
  fallback or error. Prefer propagating async APIs.
- Rust library code uses `Result` and `?`, not `unwrap()` or `expect()`; recover
  poisoned mutexes with the repository's explicit policy.

## Review Classification

When reviewing guards, classify findings as necessary guard, misplaced guard,
duplicated control flow, broad exception swallowing, missing boundary guard, or
panic/unwind risk. Add a focused regression test for stale handles, queue caps,
timeouts, invalid input, unknown values, cancellation races, or partial cleanup.
