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
- 针对点播 HLS、静态 DASH 和 FLV 输入的离线下载规划：对清单获取、大小探测、分片传输以及可选通过重封装插件以 MP4 流拷贝导出的过程，统一应用源 HTTP 头。
- 由 SDK 管理的离线任务恢复与断点区间传输，适用于 Android 和 iOS；同时提供适用于 macOS、Windows 和 Linux 的共享桌面端下载服务。
  该服务包括：当 HTTP 服务器忽略续传区间时按资源单独重启；针对已知大小的 HTTP 资源使用有边界的 Range 分块；
  以及资源过期错误处理——通过宿主提供的恢复钩子处理过期或被拒的媒体 URL。
- Android 可选外部播放能力：Google Cast 控制、DLNA / UPnP AV 发现与控制，
  以及面向带鉴权 headers、本地文件和 `content://` source 的本地 HTTP relay。
- 在 Android、iOS 和 Flutter 移动端播放处于活动状态时，支持可配置的屏幕常亮处理。
- 移动端使用平台原生 surface，而不是通过帧拷贝方式回传画面。
- 可选 plugin 架构，覆盖更高级的媒体工作流：下载后 remux、native-frame
  decoder 实验、内部 frame processor 诊断，以及桌面优先的 source normalization。
- 面向 FFI host 的 generation-checked C value handles。
- Android、iOS、Flutter、Desktop Rust 和 C 的可运行 host 示例。

## 能力矩阵

下面是粗粒度能力概览。每个平台 README 会说明更精确的行为、fallback 规则和 host app
在暴露高级控制前应检查的 capability flags。

| 能力                   | Android (Media3)       | iOS (AVPlayer)                                | 桌面 Rust                            | Flutter 移动端                      |
| ---------------------- | ---------------------- | --------------------------------------------- | ------------------------------------ | ----------------------------------- |
| 本地文件               | ✅                     | ✅                                            | ✅                                   | ✅ Android / iOS                    |
| 渐进式 HTTP/HTTPS      | ✅                     | ✅                                            | ✅                                   | ✅ Android / iOS                    |
| HLS (`.m3u8`)          | ✅                     | ✅                                            | ✅                                   | ✅ Android / iOS                    |
| DASH (`.mpd`)          | ✅ 原生                | ✅ 面向 VOD / live fMP4 的 DASH-to-HLS bridge | ⚠️ 取决于 backend 的 FFmpeg demuxer  | ✅ Android 原生 / iOS bridge        |
| 直播 / DVR             | ✅                     | ✅                                            | ✅                                   | ✅ Android / iOS                    |
| 轨道选择               | ✅ 视频 / 音频 / 字幕  | ✅ 音频 / 字幕                                | ✅                                   | ✅ 遵循各平台语义                   |
| ABR `constrained` 策略 | ✅                     | ✅ HLS + DASH bridge 变体目录                 | ✅                                   | ✅ 遵循各平台语义                   |
| ABR `fixedTrack` 策略  | ✅ 精确                | ✅ iOS 15+ 上尽力进行 HLS/DASH 固定           | ✅                                   | ✅ 遵循各平台语义                   |
| 韧性策略               | ✅                     | ✅                                            | ✅                                   | ✅ Android / iOS                    |
| 预加载预算             | ✅                     | ✅                                            | ✅                                   | ✅ Android / iOS                    |
| 下载管理器             | ✅ VOD prepare + restore + export | ✅ VOD prepare + restore + export | ✅ public `player-host-desktop::download` service | ✅ Android / iOS                    |
| 外部播放               | ✅ Cast / DLNA 可选    | ✅ AirPlay route picker                       | ❌                                   | ✅ Android Cast / DLNA、iOS AirPlay |
| 硬件解码探测           | `VesperDecoderBackend` | `VesperCodecSupport`                          | macOS VideoToolbox native-frame 可选启用 | 通过移动端 capability 上报体现      |
| Plugin 启动诊断        | 内部 runtime diagnostics | 内部 runtime diagnostics                    | ✅ decoder / frame processor / source normalizer diagnostics | 在支持的平台通过 create-result diagnostics 暴露 |

Flutter macOS package 目前只是实验性占位实现，尚未提供真实播放后端。产品 UI
应以运行时能力标记为准，而不是假设上表能力在每个后端上都可用。

## 仓库结构

```text
crates/      Rust workspace：共享 core、runtime、FFI、backend、render 与平台桥接代码
lib/         可分发的平台集成层
  android/   Android AAR modules：core kit、external playback、FFmpeg runtime、Compose adapter、可选 Compose UI
  ios/       VesperPlayerKit Swift Package / XCFramework 项目
  flutter/   Federated Flutter packages：主 API、平台包、可选 UI
examples/    Android、iOS、Flutter、桌面 Rust 与 C 的可运行 host app
include/     生成的 C header：player_ffi.h
scripts/     构建、打包、验证与发布辅助脚本
third_party/ 仓库内置依赖与生成的预构建媒体库
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

桌面 plugin 实验需要显式选择启用。`basic-player` 可以通过环境变量配置的动态库路径
加载 native-frame decoder plugin、frame processor diagnostic plugin，以及
packet-stream source normalizer plugin。这些路径用于 SDK 开发和诊断，不属于
Android / iOS 公开 host kit API。

推荐的 SourceNormalizer smoke command：

```sh
VESPER_SOURCE_NORMALIZER_PLUGIN_PATHS=target/debug/libplayer_source_normalizer_ffmpeg.dylib \
VESPER_SOURCE_NORMALIZER_MODE=prefer-normalized \
cargo run -p basic-player
```

FrameProcessor 默认仍建议作为 diagnostics / debug 路径使用，除非你在桌面端显式选择
更严格的处理模式：

```sh
VESPER_FRAME_PROCESSOR_PLUGIN_PATHS=target/debug/libplayer_frame_processor_diagnostic.dylib \
VESPER_FRAME_PROCESSOR_MODE=diagnostics \
cargo run -p basic-player
```

### C ABI

先从生成的头文件 [include/player_ffi.h](include/player_ffi.h) 开始，再运行
[examples/c-host/README.md](examples/c-host/README.md) 中的 smoke example。

```sh
scripts/vesper ffi c-host-smoke
```

## 平台包

### Android

Android 以 AAR modules 分发：

- `vesper-player-kit`：core controller、source model、JNI bridge、download manager
  和 native video surface selection。
- `vesper-player-kit-external-playback`：可选 Google Cast、DLNA / UPnP AV
  与本地 relay 集成。
- `vesper-player-kit-ffmpeg-runtime`：供 remux 与 relay 工作流使用的可选
  FFmpeg runtime package。
- `vesper-player-kit-compose`：Compose adapter，提供 `VesperPlayerSurface` 和
  controller / state helpers。
- `vesper-player-kit-compose-ui`：可选的 opinionated Compose player stage。

详细的模块列表（含 external playback 与 FFmpeg runtime）及各模块 API 文档见
[lib/android/README.md](lib/android/README.md)。

最低要求：Android API 26+、Kotlin 2.x；发布的移动端产物需要 arm64 device 或 emulator。

### iOS

iOS 以 `VesperPlayerKit` 分发，可作为 local Swift Package 进行源码集成，也可作为
XCFramework 进行 release packaging。公开 API 以 Swift 为主，面向 UIKit / SwiftUI host。

最低要求：iOS 17.0+、Xcode 16+；发布产物面向 arm64 device / Apple Silicon Simulator。

### Flutter

Flutter 是 federated plugin family：

- `vesper_player`：公开 Dart API 与 `VesperPlayerView`。
- `vesper_player_platform_interface`：共享 DTO 与平台契约。
- `vesper_player_android`：基于 Android host kit 的 Android 实现。
- `vesper_player_ios`：基于 `VesperPlayerKit` 的 iOS 实现。
- `vesper_player_macos`：实验性 macOS package stub，尚未接入真实 playback backend。
- `vesper_player_external_playback`：可选 Android Cast / DLNA 统一控制器，支持本地
  HTTP relay 和系统 Cast route button。
- `vesper_player_ui`：可选 Flutter 控件与 player stage widgets。

Flutter packages 目前从本仓库源码分发，尚未发布到 pub.dev。

## 从源码构建

常用验证命令如下。平台特定的环境配置和工具链说明请阅读
[从这里开始](#从这里开始) 中链接的各平台 README。

```sh
# Rust workspace check
cargo check --workspace

# Generate / verify the C header
./scripts/vesper ffi generate
./scripts/vesper ffi verify

# Android AAR build
./scripts/vesper android aar

# iOS XCFramework build
./scripts/vesper ios kit-xcframework

# Desktop end-to-end remux integration test
./scripts/vesper desktop verify-remux
```

Android helper scripts 在本地开发时使用项目内已经缓存的 Gradle distribution，
在 GitHub Actions 中使用 CI provisioned `gradle` executable。这样本地 agent
工作不会触发在线 wrapper 下载，同时 CI 仍可通过 `gradle/actions/setup-gradle`
安装 Gradle。

## 移动端 FFmpeg Profiles

Android 和 iOS FFmpeg 构建使用根 profile CLI。公共入口为
`./scripts/vesper ffmpeg --platform android|ios|all --profile <name>`。
`download-remux`、`relay-remux` 和 `default` 均为本地 remux profile：仅启用本地
file/pipe 协议，并验证网络与 OpenSSL 均已禁用。default profile 合并了 download
和 relay remux 的能力。

```sh
./scripts/vesper ffmpeg --platform android --profile default --abi arm64-v8a
./scripts/vesper ffmpeg --platform ios --profile default --slice ios-arm64 --slice ios-simulator-arm64
```

Source normalization 使用独立的 runtime profile 文件
`scripts/source-normalizer-profiles.toml`。这些 profiles 描述非常规 source 或
容器不兼容 source 在运行时如何被识别和标准化；它们不会替代上面的构建期
FFmpeg packaging profiles。

调用方可通过 `--extra-libraries`、`--extra-demuxers`、`--extra-muxers`、
`--extra-protocols`、`--extra-parsers`、`--extra-bsfs` 以及可重复使用的
`--extra-configure-arg` 添加受控 overlay。如果 overlay 违反所选 profile 策略，
验证将失败。生成的 ABI 和 slice 会记录 `vesper-ffmpeg-build-metadata.txt`，包含
声明的 profile、profile hash、源码压缩包、许可敏感标志及完整 configure 命令，
供 release 审查。

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

### FFmpeg 许可合规

Vesper 采用 Apache-2.0 许可，但 FFmpeg 仍受其自身 FFmpeg 许可条款约束。仓库
默认不提交生成后的 FFmpeg 二进制；仅在宿主应用明确选择启用时，可选的 Android、
iOS 与桌面工作流才会构建或打包带 FFmpeg 的产物。

默认的 Vesper FFmpeg 脚本会避免 `--enable-gpl` 与
`--enable-nonfree`；除非调用方显式确认，脚本将拒绝这些标志。移动端
`download-remux`、`relay-remux` 和 `default` profiles 验证构建不含网络与
OpenSSL。桌面 fallback 构建默认偏向 LGPL，但静态方式分发桌面产物仍需要提供
可重新链接材料，或等效的 LGPL 合规机制。

在发布任何包含 FFmpeg 的 app 或 SDK 产物前，需要附上 FFmpeg 声明与许可文本，
提供精确对应的 FFmpeg 源码与 configure flags，保留用户重新链接权利，并在
打包 OpenSSL / libxml2 时同步跟踪它们的声明。发布检查清单与条目模板见
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

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
  `player-ffi-ios` Apple bundle 提供，不在生成的 C header 中。

```sh
./scripts/vesper ffi generate
./scripts/vesper ffi verify
```

## Release 下载

GitHub Releases 会以 `VesperPlayerKit` 产品名发布移动端下载产物：

- Android 核心包：`VesperPlayerKit-android-<abi>.aar`
- Android Compose 适配层：`VesperPlayerKitCompose-android-<abi>.aar`
- Android Compose UI：`VesperPlayerKitComposeUi-android-<abi>.aar`
- Android 外部播放：`VesperPlayerKitExternalPlayback-android-<abi>.aar`
- Android FFmpeg 运行时：`VesperPlayerKitFfmpegRuntime-android-<abi>.aar`
- 可选 Android SourceNormalizer FFmpeg 插件：`VesperPlayerKitSourceNormalizerFfmpeg-android-<abi>.aar`
- 可选 Android FrameProcessor 诊断插件：`VesperPlayerKitFrameProcessorDiagnostic-android-<abi>.aar`
- iOS framework 切片：`VesperPlayerKit-ios-*.framework.zip`
- iOS XCFramework：`VesperPlayerKit.xcframework.zip`
- 可选 iOS FFmpeg 运行时：`VesperPlayerFfmpegRuntime.xcframework.zip`
- 可选 iOS FFmpeg remux 插件：`VesperPlayerRemuxFfmpegPlugin.xcframework.zip`
- 可选 iOS SourceNormalizer FFmpeg 插件：`VesperPlayerSourceNormalizerFfmpegPlugin.xcframework.zip`
- 可选 iOS FrameProcessor 诊断插件：`VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip`
- 用于校验 release artifact 的 `SHA256SUMS.txt`

Android 打包当前仅提供 `arm64-v8a`。iOS 打包仅提供 arm64 device、Apple
Silicon Simulator 和可选 Catalyst slices。iOS 核心 `VesperPlayerKit.xcframework`
不嵌入 FFmpeg；FFmpeg-backed remux 支持以独立可选 runtime 和 plugin
XCFramework 形式发布，由 host app 单独签名和嵌入。SourceNormalizer
也沿用同一个 shared runtime 边界。plugin library path 配置只传插件 binary；
shared FFmpeg runtime 是包依赖，不放进 plugin path。所有 FFmpeg-backed optional
plugins 与 shared runtime 必须来自同一个 FFmpeg profile，保证
`profile-hash.txt` 一致。

移动端 SourceNormalizer artifact 可以做 diagnostics / preflight，并在
`preferNormalized` 或 `requireNormalized` 下把 disk-backed fMP4 或
short-window HLS 输出通过本地资源层交给 Android ExoPlayer 与 iOS AVPlayer。
packet-stream 输出仍保留给后续 native frame pipeline。移动端 FrameProcessor
artifact 仅是 diagnostics shell：可以打包、加载、上报 capability，但不会打开
frame session、不会处理真实帧，也不会参与默认移动端播放。移动端 Decoder
artifact 与配置继续暂停。

Release AAR / XCFramework 是完全打包的二进制产物。消费这些下载物的 host app
在其自身 Gradle / Xcode 构建中不会运行本仓库的 JNI 或 FFmpeg 生成任务。

## 当前状态

Vesper 仍在演进中，尚未作为稳定的 SDK 发布。Android 和 iOS host kits
已经有可发布的打包路径；Flutter federated packages 目前仍从本仓库源码分发。
macOS Flutter package 当前只是未接入真实播放后端的占位实现；macOS native
VideoToolbox native-frame decoder path 仍是可选启用的实验路径；桌面端默认路径仍是
FFmpeg software fallback。

## 许可

Vesper 使用 Apache License, Version 2.0 授权。见 [LICENSE](LICENSE)。
带 FFmpeg 的可选产物受 FFmpeg 自身 LGPL/GPL 条款约束，具体取决于实际构建配置，
并单独进行跟踪。

额外署名与捆绑二进制说明见：

- [NOTICE](NOTICE)
- [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)
