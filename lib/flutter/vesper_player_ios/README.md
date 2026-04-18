# vesper_player_ios

[`vesper_player`] 的 iOS 平台实现包，基于 **AVPlayer** 与 **Vesper iOS Host Kit**（`lib/ios/VesperPlayerKit`）。

此包由 `vesper_player` 在 iOS 上自动注册，**普通应用开发者无需直接依赖此包**。

## 平台能力

| 格式 / 功能 | 状态 |
|---|---|
| 本地文件 | ✅ |
| Progressive HTTP | ✅ |
| HLS | ✅ |
| DASH | ❌ AVPlayer 后端不支持 |
| 直播（Live） | ✅ |
| DVR 直播（LiveDvr） | ✅ |
| 轨道选择（视频 / 音频 / 字幕） | ✅ |
| 自适应比特率（ABR） | ⚠️ 当前仅支持 `constrained`（maxBitRate 约束）；FixedTrack 模式能力有限 |
| 缓冲 / 重试 / 缓存策略 | ✅ |
| 下载管理 | ✅ |
| 预加载 | ✅ |

> **DASH 说明**：DASH 源的数据模型与 DTO 已有落点，但当前 AVPlayer 后端会明确返回 `unsupported`，不应在 iOS 上使用 `VesperPlayerSource.dash()`。可通过 `controller.snapshot.capabilities.supportsDash` 在运行时检查。

## 下载链路说明

iOS 侧如果只是把一个远程 `.m3u8` URL 直接传给 `createTask(...)`，宿主通常只能得到“manifest 入口”，不适合作为真正离线保存或后续 remux 的基础。

推荐链路是：

1. 用户点击创建任务时，宿主 UI 先插入一个“准备中”的占位任务。
2. 后台读取远程 HLS manifest，预先生成 `VesperDownloadAssetIndex.resources + segments`。
3. 再调用 `createTask(...)` 创建真实任务，让下载器去拉取 manifest 资源和媒体分片。

当前仓库里的 iOS native example 和 Flutter example 都已经按这条链路实现；同时 iOS example 仍然**明确跳过 DASH 下载入口**，不把它伪装成已支持能力。

## 技术说明

- **播放后端**：AVPlayer，通过 `VesperPlayerController` Swift facade 封装
- **Flutter 集成**：`MethodChannel` + `EventChannel`，通道 ID 为 `io.github.ikaros.vesper_player`
- **视图承载**：`UiKitView`（PlatformView），视图类型 ID 为 `io.github.ikaros.vesper_player/platform_view`
- **Rust 运行时**：通过 C FFI（`player-ffi-resolver` XCFramework）桥接 `player-runtime`，共享 defaults / timeline / resilience / playlist 语义

## 可选 `player-ffmpeg` remux 插件

iOS 侧如果要把 HLS / DASH 下载结果导出为 `.mp4`，宿主 app 需要把 `player-ffmpeg` 动态库嵌进 app bundle，并把 `libplayer_ffmpeg.dylib` 的真实路径传给 `VesperDownloadConfiguration.pluginLibraryPaths`。

典型接法：

1. 在 app target 增加 Xcode Run Script phase：
   ```sh
   /bin/bash "$SRCROOT/../../../scripts/embed-ios-player-ffmpeg-plugin.sh" "vesper_player_ios.framework"
   ```
   如果是原生 iOS host kit，则把参数换成 `VesperPlayerKit.framework`
2. 运行时从 `Bundle.main.privateFrameworksPath` / app `Frameworks` 目录查找 `libplayer_ffmpeg.dylib`
3. 把查到的绝对路径传给下载管理器配置

当前 Apple 侧 FFmpeg 预编译也是**按需触发**的：只有在宿主显式执行嵌入脚本时才会构建 / 拷贝对应 dylib。脚本目前支持例如 `VESPER_APPLE_FFMPEG_ENABLE_DASH=0` 这种粗粒度裁剪，但还没有细到 demuxer / muxer / protocol / codec 白名单级别。

仓库里的两个 iOS example 已默认这样做：

- `examples/ios-swift-host/VesperPlayerHostDemo.xcodeproj`
- `examples/flutter-host/ios/Runner.xcodeproj`

注意：iOS 仅支持 **app bundle 内已签名** 的动态库，不支持从网络下载或沙盒外部注入未签名插件。

## 最低版本要求

- iOS 14.0+
- Flutter 3.41.0+

## 相关资源

- 主包：[`vesper_player`]
- 平台接口：[`vesper_player_platform_interface`]
- iOS Host Kit 源码：`lib/ios/VesperPlayerKit`
