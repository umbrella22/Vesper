# C++17 native gain plugin

The shared library exports `vesper_native_plugin_entry` and one fixed-block `AudioProcessor`. Split processing reads the input and writes the output. In-place processing uses the same writable buffer descriptor for both arguments. Both modes preserve the time and rate epoch of the input block.

```sh
cmake -S . -B build -DVESPER_PLUGIN_HEADERS=/path/to/sdk/include
cmake --build build --config Release
```

The build needs the native plugin ABI header and a C++17 compiler. It has no Rust crate dependencies.
