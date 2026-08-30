# Migrating from Vesper 0.3 to 0.4

Vesper 0.4 is a coordinated breaking release. Upgrade the host kit, Flutter
packages, generated C bindings, and every native plugin as one unit. A 0.3
header, binding, or plugin binary cannot be combined with a 0.4 runtime.

## Upgrade Order

1. Move the Rust workspace to Rust 1.97 and Flutter consumers to Flutter 3.44
   or newer.
2. Regenerate [include/player_ffi.h](../include/player_ffi.h), then rebuild
   every C, C++, Swift bridge, or generated binding that includes it.
3. Rebuild and repackage every native plugin with the 0.4 player-plugin author
   SDK.
4. Replace all public raw plugin library path configuration with
   VesperPluginReference values for build-time embedded plugins.
5. Move Android and iOS subtitle selection call sites onto the async completion
   contract. Update Flutter error handling and subtitle state rendering.
6. Enable optional plugin routes explicitly after the core playback migration
   succeeds.

The supported mobile floor remains Android API 26+ on arm64-v8a, plus iOS 17+
on arm64 devices and Apple Silicon Simulator. Android production playback uses
Media3 and iOS production playback uses AVPlayer.

## C And iOS FFI ABI

include/player_ffi.h is generated from player-ffi and defines one C ABI. Do not
replace only a static or dynamic library while retaining a 0.3 generated header
or binding.

The 0.4 header expands PlayerFfiError with details_json and expands the layouts
of PlayerFfiTrack and PlayerFfiTrackCatalog. Track support, catalog revision,
playback-path, subtitle state, and structured error data also cross the current
FFI boundary. Treat every PlayerFfi declaration as part of the generated ABI
revision, including its allocation and free functions.

From the repository root, regenerate and check the checked-in header:

    ./scripts/vesper ffi generate
    ./scripts/vesper ffi verify

Then regenerate language bindings from the resulting header and recompile every
translation unit that imports it. Rebuild the host application and link it with
the matching 0.4 player-ffi artifact. The iOS host kit rebuilds its bridge
against this ABI as part of the normal VesperPlayerKit build.

Use the FFI smoke test after rebuilding a C consumer:

    ./scripts/vesper ffi c-host-smoke

## Native Plugin ABI And Plugin References

### Rebuild Every Native Plugin

The 0.4 loader accepts one vesper_plugin_entry root and discovers typed
capability tables through the root query contract. It does not load the 0.3
plugin entry or table contract. Rebuild each plugin against the 0.4
player-plugin SDK, update its manifest and package metadata, and produce a new
target artifact.

The public author surface is Rust Native and Rust WASM. Native plugins remain
trusted code. Rust WASM components support only PipelineEventHook and
BenchmarkSink for desktop and tooling; mobile hosts do not run WASM plugins.

Validate a rebuilt native artifact with the package and loader checks that match
its target:

    cargo check -p vesper-player-plugin-abi -p vesper-player-plugin -p vesper-player-plugin-loader
    cargo test -p vesper-player-plugin-abi -p vesper-player-plugin -p vesper-player-plugin-loader
    ./scripts/vesper plugin check path/to/vesper-plugin.toml \
      --artifact path/to/plugin-artifact --transport native

Run the matching plugin package and plugin verify commands when distributing a
signed .vesper-plugin archive. A successful package check proves the plugin
contract and package integrity; it does not prove a mobile playback route.

### Replace Raw Library Paths

The public pluginLibraryPaths configuration fields have been removed from the
Android, iOS, and Flutter download and benchmark APIs. They exposed bundle or
filesystem layout as a runtime plugin selection mechanism.

Use a VesperPluginReference with a reverse-DNS pluginId and explicit transport.
Mobile references use native transport. VesperBundledPluginReferences provides
canonical references for artifacts shipped with Vesper host kits.

| 0.3 configuration | 0.4 configuration |
| --- | --- |
| VesperDownloadConfiguration(pluginLibraryPaths: paths) | VesperDownloadConfiguration(postDownloadPluginReferences: references, eventHookPluginReferences: references) |
| VesperBenchmarkConfiguration(pluginLibraryPaths: paths) | VesperBenchmarkConfiguration(pluginReferences: references) |
| Host-side path discovery followed by raw library loading | Build-time embedded framework or AAR dependency plus VesperPluginReference selection |

For example, a Flutter download export configuration now selects the bundled
remux plugin by identity:

    final configuration = VesperDownloadConfiguration(
      postDownloadPluginReferences: <VesperPluginReference>[
        VesperBundledPluginReferences.remuxFfmpeg,
      ],
    );

The equivalent Kotlin and Swift VesperDownloadConfiguration APIs use
postDownloadPluginReferences and eventHookPluginReferences with the same
identity-based contract. Add the optional AAR or SwiftPM product at build time
before passing its reference. Android and iOS do not download plugin code at
runtime.

PluginReference identifies the requested capability. It does not choose a
filesystem path, bypass package validation, or make an experimental plugin part
of the direct Media3 or AVPlayer playback route.

## Subtitle Selection Completion

Subtitle selection now completes only after the native host confirms the
requested state. UI state must not mark a selection as applied when the command
is merely dispatched.

| Surface | 0.3 call shape | 0.4 call shape |
| --- | --- | --- |
| Android | Synchronous setSubtitleTrackSelection | Suspending setSubtitleTrackSelection |
| iOS | Synchronous setSubtitleTrackSelection | async throws setSubtitleTrackSelection |
| Flutter | Future<void> command dispatch | Future<void> resolves after native confirmation and throws VesperSubtitleException for structured failures |

Android callers must launch the suspending call from a coroutine that represents
the UI operation:

    viewModelScope.launch {
        controller.setSubtitleTrackSelection(
            VesperTrackSelection.track(track.id),
        )
        renderConfirmedSelection(controller.confirmedSubtitleSelection)
    }

iOS callers must await and handle the throwing operation:

    do {
        try await controller.setSubtitleTrackSelection(.track(track.id))
        renderConfirmedSelection(controller.confirmedSubtitleSelection)
    } catch {
        renderSubtitleFailure(error)
    }

Flutter callers must await the command and render structured failure data:

    try {
      await controller.setSubtitleTrackSelection(
        VesperTrackSelection.track(track.id),
      );
      renderConfirmedSelection(controller.confirmedSubtitleSelection);
    } on VesperSubtitleException catch (error) {
      renderSubtitleFailure(error.code, error.phase, error.retriable);
    }

requestedSubtitleSelection, confirmedSubtitleSelection, and
effectiveSubtitleTrackId have distinct meanings. A ready catalog can remain
usable when a selection fails. Render subtitleState.catalogState and
subtitleState.selectionState independently, and preserve opaque stable track
ids without parsing their format.

External subtitle source migration remains source-compatible only through
deprecated aliases. Replace VesperSubtitleSideLoad with
VesperExternalSubtitleSource and replace subtitleConfigurations with
externalSubtitles. Each source-local external subtitle id must be non-empty and
unique.

## Default And Optional Capabilities

The 0.4 core mobile route remains direct Media3 or AVPlayer playback. The
following state is disabled or empty by default and requires an explicit host
configuration.

| Capability | Default state | Enablement boundary |
| --- | --- | --- |
| Subtitle selection | VesperTrackSelection.disabled(); selectSubtitlesByDefault is false | Set subtitle preferences or submit an explicit automatic or fixed subtitle selection. |
| Download post-processors and EventHooks | postDownloadPluginReferences and eventHookPluginReferences are empty | Add embedded plugin references. runPostProcessorsOnCompletion alone cannot select a plugin. |
| Benchmark plugin execution | VesperBenchmarkConfiguration.enabled is false and its reference list is empty | Set enabled and provide embedded plugin references. |
| SourceNormalizer | VesperSourceNormalizerMode.disabled and an empty reference list | Select a mode and references. This remains an experimental route. |
| FrameProcessor | VesperFrameProcessorMode.disabled and an empty reference list | Select a mode and references. This remains an experimental route. |
| Native-frame decoder pipeline | VesperNativeFramePipelineMode.disabled and empty decoder or processor reference lists | Select a native-frame mode and compatible embedded plugin references. It does not replace direct Media3 or AVPlayer playback. |
| Android optional plugin artifacts | Default Android release publication excludes optional plugin AARs | Add the required optional artifact to staging and to the application dependency graph. |
| iOS optional plugin artifacts | The core Swift package does not imply the optional plugin products | Add each required VesperPlayerOptionalPlugins product to the application target and embed/sign its framework. |

Mobile WASM has no mobile route. PipelineEventHook and BenchmarkSink WASM
components are restricted to desktop and tooling. Protected media, media bytes,
filesystem, network, DRM material, and credentials do not cross the mobile
plugin boundary.

## Validation Matrix

Run the checks for every upgraded boundary:

    # Generated C ABI and a basic C consumer
    ./scripts/vesper ffi generate
    ./scripts/vesper ffi verify
    ./scripts/vesper ffi c-host-smoke

    # Native plugin ABI and loader
    cargo test -p vesper-player-plugin-abi -p vesper-player-plugin -p vesper-player-plugin-loader

    # Host artifacts
    ./scripts/vesper android aar
    ./scripts/vesper ios kit-xcframework

    # Flutter public API and behavior
    cd lib/flutter/vesper_player
    dart analyze --format=machine
    flutter test

Run package verification and a host or device test for every optional plugin
route enabled by the application. Plugin package success and archive validation
do not replace playback, subtitle, or surface evidence on the target host.
