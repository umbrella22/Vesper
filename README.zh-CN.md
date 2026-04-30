# Vesper Player SDK

语言：[English](README.md)

Vesper 是一个 native-first 的多平台播放器 SDK，面向需要真实平台播放体验、
同时又不想在每个端重复实现产品能力的应用。Android 通过 Media3 ExoPlayer
播放，iOS 通过 AVPlayer 播放，桌面端使用原生 Rust 播放管线，Flutter 应用则
通过 federated plugin 复用同一套能力。

共享 Rust 层负责对齐跨平台语义：runtime contract、timeline 与 live-DVR 状态、
播放韧性策略、ABR policy、playlist 协调、preload 与 download 规划、DASH bridge，
以及公开的 C ABI。各平台 host kit 负责渲染 surface、生命周期、原生媒体栈集成
和平台能力上报。

## 从这里开始

根据你的接入目标选择阅读路径。先读第一份文档了解公开 API 与打包模型，再用
示例应用作为可运行参考。

| 目标                     | 先读                                                                                                             | 再运行 / 查看                                                                      | 适用场景                                                          |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| Android Kotlin / Compose | [lib/android/README.md](lib/android/README.md)                                                                   | [examples/android-compose-host/README.md](examples/android-compose-host/README.md) | 直接在 Android app 中接入 AAR modules。                           |
| iOS Swift / SwiftUI      | [lib/ios/VesperPlayerKit/README.md](lib/ios/VesperPlayerKit/README.md)                                           | [examples/ios-swift-host/README.md](examples/ios-swift-host/README.md)             | 在 UIKit / SwiftUI app 中消费 Swift Package 或 XCFramework。      |
| Flutter                  | [lib/flutter/vesper_player/README.md](lib/flutter/vesper_player/README.md)                                       | [examples/flutter-host/README.md](examples/flutter-host/README.md)                 | 当前用一套 Dart API 覆盖 Android / iOS；macOS 仍是 package stub。 |
| Flutter 平台包作者       | [lib/flutter/vesper_player_platform_interface/README.md](lib/flutter/vesper_player_platform_interface/README.md) | [lib/flutter/vesper_player_ui/README.md](lib/flutter/vesper_player_ui/README.md)   | 需要扩展 federated plugin，或接入可选 Flutter UI package。        |
| C / C++ via FFI          | [include/player_ffi.h](include/player_ffi.h)                                                                     | [examples/c-host/README.md](examples/c-host/README.md)                             | 需要从原生 host 或 plugin runtime 调用生成的 C ABI。              |
| Desktop Rust             | [examples/basic-player](examples/basic-player)                                                                   | [Desktop FFmpeg](#desktop-ffmpeg)                                                  | 试用桌面 demo，或接入 Rust 播放管线。                             |

## 你会获得什么

- 每个平台走原生播放路径：Android 使用 Media3，iOS 使用 AVPlayer，桌面端使用
  Rust backend。
- timeline、live edge、live DVR、track catalog、ABR、resilience policy、preload
  policy 和 download orchestration 的共享语义。
- 移动端使用平台原生 surface，而不是通过帧拷贝方式回传画面。
- 可选的 remux / codec plugin 架构，覆盖更高级的媒体工作流。
- 面向 FFI host 的 generation-checked C value handles。
- Android、iOS、Flutter、Desktop Rust 和 C 的可运行 host 示例。

## 能力矩阵

下面是粗粒度能力概览。每个平台 README 会说明更精确的行为、fallback 规则和 host app
在暴露高级控制前应检查的 capability flags。

| 能力                     | Android (Media3)             | iOS (AVPlayer)                                | Desktop Rust                              | Flutter mobile                        |
| ------------------------ | ---------------------------- | --------------------------------------------- | ----------------------------------------- | ------------------------------------- |
| Local file               | ✅                           | ✅                                            | ✅                                        | ✅ Android / iOS                      |
| Progressive HTTP/HTTPS   | ✅                           | ✅                                            | ✅                                        | ✅ Android / iOS                      |
| HLS (`.m3u8`)            | ✅                           | ✅                                            | ✅                                        | ✅ Android / iOS                      |
| DASH (`.mpd`)            | ✅ native                    | ⚠️ static fMP4 VOD via DASH-to-HLS bridge     | ⚠️ backend-dependent FFmpeg demuxer       | ✅ Android；⚠️ iOS bridge             |
| Live / DVR               | ✅                           | ✅                                            | ✅                                        | ✅ Android / iOS                      |
| Track selection          | ✅ video / audio / subtitles | ✅ audio / subtitles                          | ✅                                        | ✅ per-platform semantics             |
| ABR `constrained` policy | ✅                           | ✅ after variant catalog                      | ✅                                        | ✅ per-platform semantics             |
| ABR `fixedTrack` policy  | ✅ exact                     | ✅ best-effort HLS variant pinning on iOS 15+ | ✅                                        | ✅ per-platform semantics             |
| Resilience policy        | ✅                           | ✅                                            | ✅                                        | ✅ Android / iOS                      |
| Preload budget           | ✅                           | ✅                                            | ✅                                        | ✅ Android / iOS                      |
| Download manager         | ✅                           | ✅                                            | ✅ planner / executor in the desktop demo | ✅ Android / iOS                      |
| Hardware decode probe    | `VesperDecoderBackend`       | `VesperCodecSupport`                          | macOS VideoToolbox v2 opt-in              | Reflected through mobile capabilities |

Flutter macOS package 目前只是实验性 stub，尚未提供真实 playback backend。产品 UI
应以运行时 capability flags 为准，而不是假设上表能力在每个 backend 上都可用。

## 仓库结构

```text
crates/      Rust workspace: shared core, runtime, FFI, backends, render, platform glue
lib/         Distributable platform integration layers
  android/   Android AAR modules: core kit, Compose adapter, optional Compose UI
  ios/       VesperPlayerKit Swift Package / XCFramework project
  flutter/   Federated Flutter packages: main API, platform packages, optional UI
examples/    Runnable host apps for Android, iOS, Flutter, desktop Rust, and C
include/     Generated C header: player_ffi.h
scripts/     Build, packaging, verification, and release helper scripts
third_party/ Vendored dependencies and generated prebuilt media libraries
```

公开接入面主要集中在 [lib/](lib/)、[examples/](examples/) 和 [include/](include/)。
[crates/](crates/) 下的 Rust crates 支撑共享 runtime 与平台 bridge。

## 快速开始

### Android 包

```kotlin
val controller = VesperPlayerControllerFactory.createDefault(
    context = context,
    initialSource = VesperPlayerSource.hls(
        uri = "https://example.com/master.m3u8",
        label = "Sample",
    ),
    resiliencePolicy = VesperPlaybackResiliencePolicy.resilient(),
)

VesperPlayerSurface(controller = controller)
```

Android host kit 指南见 [lib/android/README.md](lib/android/README.md)，完整 Compose app
示例见 [examples/android-compose-host/README.md](examples/android-compose-host/README.md)。

### iOS 包

```swift
@StateObject private var controller = VesperPlayerControllerFactory.makeDefault(
    resiliencePolicy: .resilient()
)

PlayerSurfaceContainer(controller: controller)
    .onAppear { controller.initialize() }
    .onDisappear { controller.dispose() }
```

iOS host kit 指南见 [lib/ios/VesperPlayerKit/README.md](lib/ios/VesperPlayerKit/README.md)，
SwiftUI 示例见 [examples/ios-swift-host/README.md](examples/ios-swift-host/README.md)。

### Flutter 包

```dart
final controller = await VesperPlayerController.create(
  initialSource: VesperPlayerSource.hls(
    uri: 'https://example.com/master.m3u8',
  ),
);

VesperPlayerView(controller: controller)
```

Flutter 主包指南见 [lib/flutter/vesper_player/README.md](lib/flutter/vesper_player/README.md)，
跨平台 host 示例见 [examples/flutter-host/README.md](examples/flutter-host/README.md)。

### Desktop Rust

```sh
cargo run -p basic-player
```

桌面 demo 默认显示空舞台。拖入文件、点击 "Open Local File"，或在 playlist tab
中粘贴远程 URL 后才会开始播放。桌面构建需要 demux / decode 支持时如何解析 FFmpeg，
见 [Desktop FFmpeg](#desktop-ffmpeg)。

### C ABI

先从生成的头文件 [include/player_ffi.h](include/player_ffi.h) 开始，再运行
[examples/c-host/README.md](examples/c-host/README.md) 中的 smoke example。

```sh
scripts/run-c-host-smoke.sh
```

## 平台包

### Android

Android 以 AAR modules 分发：

- `vesper-player-kit`：core controller、source model、JNI bridge、download manager
  和 native video surface selection。
- `vesper-player-kit-compose`：Compose adapter，提供 `VesperPlayerSurface` 和
  controller / state helpers。
- `vesper-player-kit-compose-ui`：可选的 opinionated Compose player stage。

最低要求：Android API 26+、Kotlin 2.x；发布的移动端产物需要 arm64 device 或 emulator。

### iOS

iOS 以 `VesperPlayerKit` 分发，可作为 local Swift Package 进行源码集成，也可作为
XCFramework 进行 release packaging。公开 API 以 Swift 为主，面向 UIKit / SwiftUI host。

最低要求：iOS 14.0+、Xcode 16+；发布产物面向 arm64 device / Apple Silicon Simulator。

### Flutter

Flutter 是 federated plugin family：

- `vesper_player`：公开 Dart API 与 `VesperPlayerView`。
- `vesper_player_platform_interface`：共享 DTO 与平台契约。
- `vesper_player_android`：基于 Android host kit 的 Android 实现。
- `vesper_player_ios`：基于 `VesperPlayerKit` 的 iOS 实现。
- `vesper_player_macos`：实验性 macOS package stub，尚未接入真实 playback backend。
- `vesper_player_ui`：可选 Flutter 控件与 player stage widgets。

Flutter packages 目前从本仓库源码分发，尚未发布到 pub.dev。

## 从源码构建

常用验证命令如下。平台特定的环境配置和工具链说明请阅读
[从这里开始](#从这里开始) 中链接的各平台 README。

```sh
# Rust workspace check
cargo check --workspace

# Generate / verify the C header
./scripts/generate-player-ffi-header.sh
./scripts/verify-player-ffi-header.sh

# Android AAR build
./scripts/build-android-vesper-player-kit-aar.sh

# iOS XCFramework build
./scripts/build-ios-vesper-player-kit-xcframework.sh

# Desktop end-to-end remux integration test
./scripts/verify-desktop-player-remux-ffmpeg.sh
```

Android 和 Flutter Android 构建会使用对应项目中提交的 Gradle wrapper，因此本地构建
会与示例和脚本使用同一套 Gradle / Android Gradle Plugin 版本。

## Desktop FFmpeg

链接 FFmpeg 的 Desktop Rust 构建会按以下顺序解析库：

1. 如果 `third_party/ffmpeg/desktop` 下已经存在仓库本地 desktop FFmpeg install，
   优先使用它。
2. 否则使用通过 `pkg-config` 或 Homebrew `ffmpeg` 暴露的最新系统 FFmpeg。
3. 如果两者都不存在，则构建匹配 workspace major/minor 版本的 FFmpeg，并安装到
   `third_party/ffmpeg/desktop`。

本地源码压缩包缓存沿用仓库既有约定：

- 如果仓库根目录已经存在 `ffmpeg-<major>.<minor>.tar.xz`，则直接复用。
- 否则构建 helper 会从 `https://ffmpeg.org/releases/` 下载匹配的压缩包。

可用覆盖变量：

| 变量                                   | 用途                                          |
| -------------------------------------- | --------------------------------------------- |
| `VESPER_DESKTOP_FFMPEG_DIR`            | 覆盖仓库本地 desktop FFmpeg install 目录。    |
| `VESPER_DESKTOP_FFMPEG_VERSION`        | 覆盖自动解析的 FFmpeg major/minor 版本。      |
| `VESPER_DESKTOP_FFMPEG_SOURCE_ARCHIVE` | 指向已经预下载的 FFmpeg source archive。      |
| `VESPER_DESKTOP_FFMPEG_SOURCE_URL`     | 覆盖源码下载 URL。                            |
| `VESPER_REAL_PKG_CONFIG`               | 强制 wrapper 使用指定的 `pkg-config` binary。 |

## C ABI 说明

- `player-ffi` 在 [include/player_ffi.h](include/player_ffi.h) 中暴露
  generation-checked value handles。该头文件由 cbindgen 生成，应通过下面的脚本
  重新生成，而不是手动编辑。
- 零初始化 handle 是 invalid sentinel，可用于普通 C stack storage。
- stale、consumed 或 double-destroyed handle 会返回
  `PLAYER_FFI_ERROR_CODE_INVALID_STATE`，避免依赖 raw-pointer undefined behavior。
- 返回 status 的 `player_ffi_*` 调用由 `catch_unwind` 包裹，因此 panic 会转换成
  结构化 backend / platform error，而不会跨 C 边界 unwind。
- DASH/HLS bridge 入口 `player_ffi_dash_bridge_execute_json` 由
  `player-ffi-resolver` Apple bundle 提供，不在生成的 C header 中。

```sh
./scripts/generate-player-ffi-header.sh
./scripts/verify-player-ffi-header.sh
```

## Release Downloads

GitHub Releases 会以 `VesperPlayerKit` 产品名发布移动端下载产物：

- Android core: `VesperPlayerKit-android-<abi>.aar`
- Android Compose adapter: `VesperPlayerKitCompose-android-<abi>.aar`
- iOS framework slices: `VesperPlayerKit-ios-*.framework.zip`
- iOS XCFramework: `VesperPlayerKit.xcframework.zip`
- 用于校验 release artifact 的 `SHA256SUMS.txt`

Android packaging 当前仅提供 `arm64-v8a`。iOS packaging 仅提供 arm64 device、
Apple Silicon Simulator 和可选 Catalyst slices。

## 当前状态

Vesper 仍在演进中，尚未作为稳定的 SDK 发布。Android 和 iOS host kits
已经有可发布的 package paths；Flutter federated packages 目前仍从本仓库源码分发。
macOS Flutter package 当前只是未接入真实 playback backend 的 stub；macOS native
VideoToolbox v2 decoder path 仍是 opt-in 实验路径；桌面端默认路径仍是 FFmpeg
software fallback。

## License

Vesper 使用 Apache License, Version 2.0 授权。见 [LICENSE](LICENSE)。

额外署名与 bundled-binary 说明见：

- [NOTICE](NOTICE)
- [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)
