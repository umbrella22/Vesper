# Vesper Audio Processor Diagnostic

This is a loadable first-party Native `AudioProcessor` conformance fixture. It
exports the safe Rust author surface through `vesper_plugin_entry` and provides
deterministic interleaved F32 processing for the experimental desktop pipeline.

`PreservePitch` uses the pure-Rust MIT-licensed `wsola` implementation.
`FollowRate` uses bounded linear resampling so duration and pitch follow the
requested rate. The plugin also applies a deterministic 0.5 gain for diagnostic
inspection. It is not part of the Media3 or AVPlayer default path and never
receives protected media.

```sh
cargo test --offline --manifest-path plugins/audio-processor-diagnostic/Cargo.toml
cargo build --offline --manifest-path plugins/audio-processor-diagnostic/Cargo.toml
```
