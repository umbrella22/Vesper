# Rust native gain plugin

The factory exposes one fixed-block `AudioProcessor` through both the static Rust author API and the `vesper_native_plugin_entry` symbol. Each call halves the input samples and leaves the negotiated sample rate, channel count, and frame count unchanged.

Copy `Cargo.toml.template` to `Cargo.toml` and `src/lib.rs.template` to `src/lib.rs`. Replace `{{VESPER_SDK_VERSION}}` with an SDK version that exports native plugin ABI. For a source-checkout build before publication, specify the local `crates/plugin/player-plugin` path for that dependency. Run `cargo build --release` to produce the native library. Linked hosts can call `gain_plugin()` directly.
