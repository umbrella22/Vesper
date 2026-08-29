# Vesper WASM Observer Diagnostic

This is a Rust WASM Component plugin for Vesper Player SDK. The component receives bounded structured events only; it has no filesystem, network, environment, process, clock, or random host imports.

Build and inspect it with:

```sh
vesper plugin build vesper-plugin.toml --profile dev
vesper plugin inspect vesper-plugin.toml --artifact dist/vesper_plugin_wasm_observer_diagnostic.wasm --transport wasm
vesper plugin check vesper-plugin.toml --artifact dist/vesper_plugin_wasm_observer_diagnostic.wasm --transport wasm
```
