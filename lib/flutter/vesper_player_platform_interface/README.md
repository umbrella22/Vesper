# vesper_player_platform_interface

`vesper_player` 的平台接口包，定义跨平台共享的抽象类、数据模型和事件协议。

此包遵循 [Flutter federated plugin] 规范，面向**平台插件开发者**，普通应用开发者应直接使用 [`vesper_player`] 主包。

## 包含内容

### 平台抽象

- `VesperPlayerPlatform` — 所有平台实现必须继承的抽象基类
- `VesperPlatformCreateResult` — `createPlayer` 的结果类型

### 播放器数据模型

| 类型 | 描述 |
|---|---|
| `VesperPlayerSource` | 媒体源（本地文件 / 远程 URL / HLS / DASH） |
| `VesperPlayerSnapshot` | 播放器完整状态快照 |
| `VesperPlayerCapabilities` | 当前后端的能力集合 |
| `VesperTimeline` | 播放时间线（VOD / Live / LiveDvr） |
| `VesperSeekableRange` | 可寻位范围（DVR 直播时间窗口） |
| `VesperTrackCatalog` | 媒体轨道目录（视频 / 音频 / 字幕） |
| `VesperMediaTrack` | 单条媒体轨道的详情 |
| `VesperTrackSelection` | 轨道选择指令（auto / disabled / 指定 track） |
| `VesperTrackSelectionSnapshot` | 当前轨道选择状态 |
| `VesperAbrPolicy` | 自适应比特率策略（auto / constrained / fixedTrack） |
| `VesperTrackPreferencePolicy` | 语言与默认轨道偏好策略 |
| `VesperPlaybackResiliencePolicy` | 抗弹性总策略（包含缓冲 / 重试 / 缓存） |
| `VesperBufferingPolicy` | 缓冲策略（预设或自定义参数） |
| `VesperRetryPolicy` | 重试策略（最大次数 / 退避模式 / 延迟范围） |
| `VesperCachePolicy` | 缓存策略（内存 / 磁盘配额） |
| `VesperPreloadBudgetPolicy` | 预加载预算（并发任务数 / 内存 / 磁盘 / 热身窗口） |
| `VesperPlayerViewport` | 视口矩形（用于 viewport hint 归一化） |
| `VesperViewportHint` | 视口可见性 hint（Visible / NearVisible / PrefetchOnly / Hidden） |
| `VesperPlayerError` | 播放错误（含分类与可重试标志） |

### 播放器事件

| 事件类型 | 触发时机 |
|---|---|
| `VesperPlayerSnapshotEvent` | 播放器状态变化 |
| `VesperPlayerErrorEvent` | 发生错误 |
| `VesperPlayerDisposedEvent` | 播放器已销毁 |

### 下载数据模型

| 类型 | 描述 |
|---|---|
| `VesperDownloadConfiguration` | 下载管理器配置 |
| `VesperDownloadSource` | 下载源（含内容格式） |
| `VesperDownloadProfile` | 下载配置（语言 / 轨道 / 目录 / 网络限制） |
| `VesperDownloadAssetIndex` | 资产索引（分片 / 版本 / 校验 / 大小） |
| `VesperDownloadTaskSnapshot` | 单个下载任务快照 |
| `VesperDownloadSnapshot` | 所有下载任务的汇总快照 |
| `VesperDownloadProgressSnapshot` | 下载进度（字节数 / 分片数 / 完成比例） |
| `VesperDownloadError` | 下载错误 |

### 下载事件

| 事件类型 | 触发时机 |
|---|---|
| `VesperDownloadSnapshotEvent` | 下载状态变化 |
| `VesperDownloadErrorEvent` | 下载发生错误 |
| `VesperDownloadDisposedEvent` | 下载管理器已销毁 |

### 枚举

```dart
VesperPlayerSourceKind        // local / remote
VesperPlayerSourceProtocol    // file / content / progressive / hls / dash / unknown
VesperPlaybackState           // ready / playing / paused / finished
VesperTimelineKind            // vod / live / liveDvr
VesperPlayerBackendFamily     // androidHostKit / iosHostKit / macosFfi / softwareFallback / fakeDemo
VesperMediaTrackKind          // video / audio / subtitle
VesperTrackSelectionMode      // auto / disabled / track
VesperAbrMode                 // auto / constrained / fixedTrack
VesperBufferingPreset         // defaultPreset / balanced / streaming / resilient / lowLatency
VesperRetryBackoff            // fixed / linear / exponential
VesperCachePreset             // defaultPreset / disabled / streaming / resilient
VesperPlayerErrorCategory     // input / source / network / decode / audioOutput / playback / capability / platform / unsupported
VesperViewportHintKind        // visible / nearVisible / prefetchOnly / hidden
VesperDownloadContentFormat   // hlsSegments / dashSegments / singleFile / unknown
VesperDownloadState           // queued / preparing / downloading / paused / completed / failed / removed
```

## 实现新平台插件

继承 `VesperPlayerPlatform` 并在 `registerWith()` 中注册：

```dart
class VesperPlayerMyPlatform extends VesperPlayerPlatform {
  static void registerWith() {
    VesperPlayerPlatform.instance = VesperPlayerMyPlatform();
  }

  @override
  Future<VesperPlatformCreateResult> createPlayer({...}) async {
    // 平台实现
  }

  // 实现其余抽象方法...
}
```

未实现的方法默认抛出 `VesperPlayerError.unsupported()`，确保上层可以通过 `VesperPlayerCapabilities` 进行能力检查而不是捕获异常。

## 相关资源

- 主包：[`vesper_player`]
- Android 实现：[`vesper_player_android`]
- iOS 实现：[`vesper_player_ios`]
