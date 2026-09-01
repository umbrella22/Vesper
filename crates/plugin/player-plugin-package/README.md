# Vesper Player Plugin Packages

`vesper-player-plugin-package` provides deterministic package, signature,
trust-store, installation, and descriptor primitives for Vesper plugins. It is
for package managers, host tooling, and integrations that need the same package
rules as the `vesper plugin` CLI.

## Package Flow

The crate parses author-owned `vesper-plugin.toml` data into a validated
`PluginProjectManifest`, derives a canonical `PluginDescriptor`, and builds a
`.vesper-plugin` archive. The archive contains generated `manifest.json`,
sorted `SHA256SUMS`, and `signature.json`, plus declared artifacts, licenses,
notices, and redistribution metadata.

Artifact digests are calculated from the declared source files. They are never
accepted as author-provided manifest values. The package verifier checks the
archive layout, path rules, size limits, checksums, Ed25519 signature, publisher
key identity, and trust-store status before yielding a verified package.

## Main APIs

- `PluginProjectManifest` parses and validates the project input.
- `PluginDescriptor` and `CanonicalPluginDescriptor` provide canonical,
  artifact-independent identity and capability metadata.
- `PluginSigningKey`, `PluginPublicKey`, and `PluginTrustStore` model publisher
  signing and key rotation or revocation.
- `build_signed_plugin_package` and `verify_signed_plugin_package` build and
  verify archive bytes.
- `install_verified_plugin_package`, `list_installed_plugins`, and
  `uninstall_plugin` manage a verified installed catalog.

## Security Boundary

A valid package proves that an authorized publisher signed the recorded
artifact bytes. It does not make a Native plugin safe to execute. Native code
remains trusted code and must be loaded only under the host's deployment
policy. The package crate does not open dynamic libraries, instantiate WASM,
or select playback capabilities.

For a ready-to-use author workflow, install `vesper-player-cli` and use
`vesper plugin key`, `package`, `verify`, and `install`. Host runtimes can pass
the verified catalog to `vesper-player-plugin-loader` for loading.

The canonical JSON schemas are maintained in the
[Vesper repository](https://github.com/umbrella22/Vesper/tree/main/schemas/vesper-plugin).
