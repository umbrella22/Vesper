# vesper_player_android

[`vesper_player`] 的 Android 平台实现包，基于 **Media3 ExoPlayer** 与 **Vesper Android Host Kit**（`lib/android/vesper-player-kit`）。

此包由 `vesper_player` 在 Android 上自动注册，**普通应用开发者无需直接依赖此包**。

## 平台能力

| 格式 / 功能 | 状态 |
|---|---|
| 本地文件 | ✅ |
| Progressive HTTP | ✅ |
| HLS | ✅ |
| DASH | ✅ |
| 直播（Live） | ✅ |
| DVR 直播（LiveDvr） | ✅ |
| 轨道选择（视频 / 音频 / 字幕） | ✅ |
| 自适应比特率（ABR） | ✅ Auto / Constrained / FixedTrack |
| 缓冲 / 重试 / 缓存策略 | ✅ |
| 下载管理 | ✅ |
| 预加载 | ✅ |

## 技术说明

- **播放后端**：Media3 ExoPlayer，通过 `VesperPlayerController` Kotlin facade 封装
- **Flutter 集成**：`MethodChannel` + `EventChannel`，通道 ID 为 `io.github.ikaros.vesper_player`
- **视图承载**：`AndroidView`（PlatformView），视图类型 ID 为 `io.github.ikaros.vesper_player/platform_view`
- **渲染路径**：根据场景自动选择（全屏 / 固定舞台优先 SurfaceView；滚动 / 复杂舞台优先 TextureView）
- **Rust 运行时**：通过 JNI 桥接 `player-runtime`，共享 defaults / timeline / resilience / playlist 语义

## 可选 `player-ffmpeg` remux 插件

Android 侧如果要把 HLS / DASH 下载结果导出为 `.mp4`，需要由宿主 app 显式把 `player-ffmpeg` 插件打进 APK，并把 `libplayer_ffmpeg.so` 的真实路径传给 `VesperDownloadConfiguration.pluginLibraryPaths`。

典型接法：

1. 用脚本构建 Android 插件产物：
   ```sh
   ./scripts/build-android-player-ffmpeg-plugin.sh <output-dir> [debug|release] [abi...]
   ```
2. 把输出目录挂到 app 的 `sourceSets.main.jniLibs`
3. 运行时从 `applicationInfo.nativeLibraryDir` 查找 `libplayer_ffmpeg.so`
4. 把查到的绝对路径传给下载管理器配置

当前 Android 侧 FFmpeg 预编译是**按需触发**的：只有在宿主显式构建 `player-ffmpeg` 插件时才会拉起 FFmpeg 预编译流程。脚本目前支持例如 `VESPER_ANDROID_FFMPEG_ENABLE_DASH=0` 这种粗粒度裁剪，但还没有细到 demuxer / muxer / protocol / codec 白名单级别。

仓库里的两个 Android example 已默认这样做：

- `examples/android-compose-host/app/build.gradle.kts`
- `examples/flutter-host/android/app/build.gradle.kts`

这也意味着：**普通 app 不会因为依赖 `vesper_player_android` 就自动带上 FFmpeg**；只有宿主显式选择接入插件时，才会构建并打包对应 `.so`。

## 最低版本要求

- Android API Level 26+
- Flutter 3.24.0+

## 相关资源

- 主包：[`vesper_player`]
- 平台接口：[`vesper_player_platform_interface`]
- Android Host Kit 源码：`lib/android/vesper-player-kit`
