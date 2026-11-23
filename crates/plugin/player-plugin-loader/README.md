# Vesper Player Plugin Loader

`vesper-player-plugin-loader` validates native Vesper plugins and exposes checked
host-side capabilities through `player_plugin_loader::native`.

- Dynamic libraries export `vesper_native_plugin_entry` and return a
  `VesperPluginRoot`. Linked registrations validate the same ABI major and root
  size. Retired exports have no fallback.
- Hosts resolve typed capabilities by interface ID. The loader validates the
  table prefix and reports malformed or unsupported interfaces.
- `PluginCatalogIndex` and `PluginCatalogImporter` handle metadata and artifact
  digests without opening libraries or retaining live sessions.
- Android and Apple hosts use build-time embedded registry records to validate
  target, architecture, and artifact integrity before plugin admission.

Native code runs in the host process. `LoadedNativePlugin::load_development`
opens an explicitly selected local library for development or inspection.
Mobile hosts admit plugins from installed platform artifacts.

The [author API](../../../lib/doc/plugin-author-api.md) describes the Rust and
C/C++ contracts. The [migration guide](../../../lib/doc/native-plugin-migration.md)
covers rebuilding and repackaging earlier plugins.
