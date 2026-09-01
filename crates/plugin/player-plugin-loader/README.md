# Vesper Player Plugin Loader

`vesper-player-plugin-loader` is the host-side loader for Vesper Native and
WASM plugin artifacts. Use it when building a desktop, tooling, or platform
integration that must validate a declared plugin contract before exposing a
typed Vesper capability.

## What It Does

- Imports and indexes metadata-only plugin catalog records with bounded
  artifact digest checks.
- Loads Native libraries through the fixed ABI, verifies the root identity and
  interface tables, and exposes typed capability factories through
  `PluginRegistry`.
- Reports supported, unsupported, unavailable, and malformed interfaces as
  structured diagnostics instead of silently accepting an unknown contract.
- Parses build-time Android and Apple embedded-plugin registry fragments,
  validates target and architecture metadata, and verifies native artifact
  integrity before loading.

The metadata path remains separate from runtime startup: `PluginCatalogIndex`
and `PluginCatalogImporter` do not open a dynamic library, retain a runtime
owner, create a WASM instance, or store media bytes.

## Optional Features

```toml
[dependencies]
vesper-player-plugin-loader = { version = "0.5", features = ["installed-catalog", "wasm"] }
```

- `installed-catalog` enables conversion from verified installed package
  records supplied by `vesper-player-plugin-package`.
- `wasm` enables WASM Component artifact declarations and the bounded
  `vesper-player-plugin-wasm-host` runtime path.

Without those features, the crate still supports the Native loader boundary.

## Deployment Boundary

Native artifact paths are host-owned locators, not public mobile application
inputs. Production hosts should begin with a verified package or embedded
registry record. Development-only inspection APIs make their unsigned/raw-path
policy explicit and should not become a production discovery mechanism.

WASM artifacts can implement only structured `PipelineEventHook` and
`BenchmarkSink` workloads. They are limited to desktop/tooling hosting; mobile
hosts embed Native artifacts during the application build and reject WASM
transport.

This crate does not sign packages, establish publisher trust, or author a
plugin. Use `vesper-player-plugin-package` for those package operations,
`vesper-player-plugin` for Rust Native plugins, and
`vesper-player-plugin-wasm` for Rust WASM Component guests.

See the [Vesper repository](https://github.com/umbrella22/Vesper) for the
plugin schemas, templates, and host-kit distribution guides.
