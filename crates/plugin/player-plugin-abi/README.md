# Vesper Player Plugin ABI

[`include/vesper_plugin.h`](../../../include/vesper_plugin.h) defines ABI major
2 for C99 and C++17. The matching Rust types and constants are under
`player_plugin_abi`. The `vesper_native_plugin_entry` symbol exposes a root with
interface discovery, typed vtables, and a destructor for its owner.

The host negotiates stream formats before processing frames. Video frames and
packets carry independent release callbacks. A session can close while a GPU
submission still holds a lease. Audio processors accept host buffers according
to their negotiated fixed-block or streaming contract.

Rust authors use the safe `vesper-player-plugin` SDK. The
[author API](../../../lib/doc/plugin-author-api.md) describes the Rust factory
and C vtable paths. `./scripts/vesper plugin header-verify` checks the generated
header and compiles it in both supported languages.
