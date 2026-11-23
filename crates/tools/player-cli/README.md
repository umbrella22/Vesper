# Vesper Player CLI

`vesper` scaffolds and checks native plugins and runs source-checkout build
and release workflows.

```sh
./scripts/vesper plugin --help
```

The [migration guide](../../../lib/doc/native-plugin-migration.md) covers
rebuilding and repackaging older plugins.

## Plugin Author Workflow

`vesper plugin new` generates a fixed-block `AudioProcessor` in Rust (`--language rust`)
or C++17 (`--language cpp`). Both variants export `vesper_native_plugin_entry`.
For a source-checkout build, set the Rust scaffold's `player-plugin` dependency
to the checkout's `crates/plugin/player-plugin` directory.

From the checkout, scaffold and build the C++17 example with the generated header:

```sh
./scripts/vesper plugin new /tmp/vesper-gain-cpp \
  --plugin-id dev.example.gain --language cpp
cmake -S /tmp/vesper-gain-cpp -B /tmp/vesper-gain-cpp/build \
  -DVESPER_PLUGIN_HEADERS="$PWD/include"
cmake --build /tmp/vesper-gain-cpp/build --config Release
```

For projects that declare artifacts in `vesper-plugin.toml`, `plugin build`
builds and stages a selected Cargo artifact. `plugin inspect` reports manifest
or binary metadata and `plugin check` runs bounded native conformance checks.
`plugin descriptor`, `catalog`, and `registry-fragment` provide canonical
metadata for tooling and mobile embedding. `plugin header-verify` verifies
that the generated C header matches the Rust ABI and compiles as C99 and C++17.

## Repository Workflows

When run from a Vesper source checkout, the CLI also provides checked build,
verification, and staging commands for Android, iOS, Flutter, desktop, FFI,
media fixtures, and release metadata. Those command groups are maintainer
tools with platform toolchain and source-tree requirements. They are not a
runtime API for applications that consume the Android, iOS, or Flutter SDKs.

The preview `harmony` group provides `doctor`, `audit-dependencies`, and
`check-shared` for toolchain and dependency checks; `kit-har`,
`kit-host-contracts`, `kit-verify`, and `kit-consumer --har <artifact>` build and
verify the three HarmonyOS host-kit packages and independent consumers.
`kit-consumer --ui`, `--example`, and `--no-internet` select the Stage consumer,
standalone example, and offline consumer respectively. `emulator-gate` runs one
artifact-bound scenario on an already running official emulator. Pass
`--command-line-tools <directory>` or set
`VESPER_HARMONY_COMMAND_LINE_TOOLS`; no persistent PATH change is required.

Reports distinguish host tests, native linking, HAR consumption, simulator execution,
and physical-device acceptance. Packages are local previews, not published
OHPM releases. See the
[HarmonyOS preview guide](https://github.com/umbrella22/Vesper/tree/main/lib/harmony)
for media profiles, capability limits, candidate freezing and the final matrix.

## Author SDKs

Rust authors use `vesper-player-plugin`. C and C++ authors use the generated
[`vesper_plugin.h`](https://github.com/umbrella22/Vesper/blob/main/include/vesper_plugin.h).
The [Rust](https://github.com/umbrella22/Vesper/tree/main/templates/vesper-plugin/rust-native)
and [C++17](https://github.com/umbrella22/Vesper/tree/main/templates/vesper-plugin/cpp-cmake)
scaffolds implement the same fixed-block gain capability.

Android and iOS apps embed verified native artifacts at build time. Host routes
select supported capabilities with explicit plugin references.

See the [Vesper repository](https://github.com/umbrella22/Vesper) for platform
host-kit integration guides and release artifacts.
