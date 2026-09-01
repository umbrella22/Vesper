# Vesper Player Plugin ABI

`vesper-player-plugin-abi` defines the stable raw ABI between a Vesper host and
a Native plugin. It contains the C-compatible root table, capability interface
tables, status values, entry-symbol constants, size checks, ownership rules,
and export helpers shared by all supported plugin languages.

## Who Needs This Crate

Use this crate only when implementing an ABI bridge, a non-Rust plugin author
SDK, or a custom loader at the raw boundary. Rust Native plugin authors should
use `vesper-player-plugin`, which builds and exports these records safely.
Host applications should normally use `vesper-player-plugin-loader`, which
validates a dynamic library before exposing typed capabilities.

## Contract Surface

The Native entry point is the fixed `vesper_plugin_entry` symbol. It returns a
`VesperPluginRoot`, which describes the plugin identity and its interface
tables. The ABI includes interfaces for:

- `PostDownloadProcessor`, `PipelineEventHook`, and `BenchmarkSink`.
- `NativeDecoder`, `FrameProcessor`, `AudioProcessor`, and packet/resource
  `SourceNormalizer` capabilities.

The crate publishes ABI major/minor constants, fixed interface identifiers,
required structure sizes, status codes, bounded byte ownership types, session
and lease IDs, and the `ExportPlugin` / `ExportInterface` traits used by the
safe Rust SDK.

## Compatibility Rules

Hosts must check ABI version, structure size, interface ID, interface version,
plugin identity, and all returned lengths before calling an interface. Unknown
interface IDs must remain visible as diagnostics rather than being treated as a
known capability. Owned bytes are released through the supplied ABI callback;
callers must not retain borrowed slices after the corresponding ABI operation.

The raw ABI is a contract for trusted native code. It does not load libraries,
verify a package signature, sandbox a plugin, or resolve a capability against a
catalog. Those functions belong to `vesper-player-plugin-loader` and
`vesper-player-plugin-package`.

## Recommended Entry Points

- Rust Native plugin authors: `vesper-player-plugin` and
  `#[player_plugin::export]`.
- Host runtime authors: `vesper-player-plugin-loader`.
- Package and trust-store integrations: `vesper-player-plugin-package`.
- Rust WASM Component authors and hosts: `vesper-player-plugin-wasm` and
  `vesper-player-plugin-wasm-host`.

The canonical plugin schemas and WIT contract are maintained in the
[Vesper repository](https://github.com/umbrella22/Vesper).
