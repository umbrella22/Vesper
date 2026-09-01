# Vesper Player CLI

`vesper` is the command-line tool for Vesper plugin authors and for Vesper
source-checkout build and release workflows. Install this crate when creating,
validating, signing, or distributing a Vesper plugin.

```sh
cargo install vesper-player-cli --locked
vesper --help
```

## Plugin Author Workflow

The `plugin` command group provides the complete Rust Native and Rust WASM
Component authoring path:

- `vesper plugin new` creates a Native or WASM project with a version-matched
  Cargo dependency, WIT contract, manifest, license, and README.
- `vesper plugin build`, `inspect`, and `check` build a declared artifact,
  inspect its manifest or binary metadata, and run bounded conformance checks.
- `vesper plugin descriptor`, `catalog`, and `registry-fragment` produce
  canonical metadata for catalogs and Android or Apple build-time embedding.
- `vesper plugin key`, `package`, `verify`, `install`, `list`, and `uninstall`
  manage Ed25519 publisher keys and deterministic `.vesper-plugin` archives.

For example, this creates a WASM event hook, builds it, and checks the emitted
component:

```sh
vesper plugin new ./analytics \
  --plugin-id dev.example.analytics \
  --publisher dev.example \
  --license Apache-2.0 \
  --transport wasm \
  --capability event-hook

cd analytics
vesper plugin build vesper-plugin.toml --profile release
vesper plugin inspect vesper-plugin.toml \
  --artifact dist/vesper_plugin_analytics.wasm \
  --transport wasm
vesper plugin check vesper-plugin.toml \
  --artifact dist/vesper_plugin_analytics.wasm \
  --transport wasm
```

Package signing and verification operate on declared artifacts and a
host-configured trust store. A valid native signature proves the package
publisher and artifact integrity; it does not sandbox native code.

## Repository Workflows

When run from a Vesper source checkout, the CLI also provides checked build,
verification, and staging commands for Android, iOS, Flutter, desktop, FFI,
media fixtures, and release metadata. Those command groups are maintainer
tools with platform toolchain and source-tree requirements. They are not a
runtime API for applications that consume the Android, iOS, or Flutter SDKs.

## Choose the Right Package

Use `vesper-player-plugin` to write a Rust Native plugin, and
`vesper-player-plugin-wasm` to write a Rust WASM Component plugin. Use
`vesper-player-plugin-package` or `vesper-player-plugin-loader` only when
building a custom package manager or host runtime.

The repository contains the canonical
[plugin manifest schemas](https://github.com/umbrella22/Vesper/tree/main/schemas/vesper-plugin),
[Native template](https://github.com/umbrella22/Vesper/tree/main/templates/vesper-plugin/native),
and [WASM template](https://github.com/umbrella22/Vesper/tree/main/templates/vesper-plugin/wasm).

## Scope

The CLI does not provide a general media player or turn arbitrary plugins into
mobile runtime extensions. Android and iOS embed verified Native artifacts at
application build time. WASM plugins run only in the bounded desktop/tooling
host path and receive structured events, never media bytes or DRM material.

See the [Vesper repository](https://github.com/umbrella22/Vesper) for platform
host-kit integration guides and release artifacts.
