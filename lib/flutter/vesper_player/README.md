# vesper_player

跨平台视频播放器 Flutter 插件，基于原生优先后端（Android ExoPlayer / iOS AVPlayer），通过统一 Dart API 提供一致的播放体验。

## 平台支持

| 功能 | Android | iOS | macOS |
|---|---|---|---|
| 本地文件 | ✅ | ✅ | ⚠️ 实验 |
| Progressive HTTP | ✅ | ✅ | ⚠️ 实验 |
| HLS | ✅ | ✅ | ⚠️ 实验 |
| DASH | ✅ | ❌ | ⚠️ 实验 |
| 直播（Live） | ✅ | ✅ | ⚠️ 实验 |
| DVR 直播 | ✅ | ✅ | ⚠️ 实验 |
| 轨道选择 | ✅ | ✅ | ⚠️ 实验 |
| 自适应比特率（ABR） | ✅ | ⚠️ 仅约束模式 | ⚠️ 实验 |
| 缓冲 / 重试策略 | ✅ | ✅ | ⚠️ 实验 |
| 下载管理 | ✅ | ✅ | ❌ |
| 预加载 | ✅ | ✅ | ❌ |

> macOS 后端当前仍处于实验阶段，能力矩阵与 API 行为可能与移动端不完全对齐。

## 安装

```yaml
dependencies:
  vesper_player: ^0.1.0
```

## 快速上手

### 最简播放

```dart
import 'package:vesper_player/vesper_player.dart';

// 1. 创建控制器
final controller = await VesperPlayerController.create(
  initialSource: VesperPlayerSource.hls(
    uri: 'https://example.com/stream.m3u8',
    label: '示例视频',
  ),
);

// 2. 在界面中嵌入视频视图
VesperPlayerView(controller: controller)

// 3. 播放
await controller.play();

// 4. 销毁（通常在 dispose 中调用）
await controller.dispose();
```

### 监听播放状态

```dart
// 订阅快照流（状态变化时触发）
controller.snapshots.listen((snapshot) {
  print('播放状态: ${snapshot.playbackState}');
  print('当前位置: ${snapshot.timeline.positionMs}ms');
  print('是否缓冲: ${snapshot.isBuffering}');
});

// 订阅事件流
controller.events.listen((event) {
  if (event is VesperPlayerErrorEvent) {
    print('错误: ${event.error.message}');
  }
});

// 也可以直接读取当前快照
final snapshot = controller.snapshot;
```

## 核心 API

### VesperPlayerController

播放器的主要控制入口。

```dart
// 创建（支持预配置策略）
final controller = await VesperPlayerController.create(
  initialSource: VesperPlayerSource.hls(uri: '...'),
  resiliencePolicy: const VesperPlaybackResiliencePolicy.resilient(),
  trackPreferencePolicy: const VesperTrackPreferencePolicy(
    preferredAudioLanguage: 'zh',
    preferredSubtitleLanguage: 'zh',
  ),
);

// 播放控制
await controller.selectSource(VesperPlayerSource.local(uri: '/path/to/video.mp4'));
await controller.play();
await controller.pause();
await controller.togglePause();
await controller.stop();

// 寻位
await controller.seekBy(10000);            // 向前 10 秒
await controller.seekToRatio(0.5);         // 跳到中间
await controller.seekToLiveEdge();         // 直播：跳回实时边缘

// 播放速率
await controller.setPlaybackRate(1.5);     // 1.5 倍速
```

### VesperPlayerView

将视频嵌入 Flutter 界面。

```dart
VesperPlayerView(
  controller: controller,
  visible: true,           // 控制视频可见性（不影响播放）
  overlay: Stack(          // 可选的浮层（控制栏、字幕等）
    children: [
      // 你的 UI 浮层
    ],
  ),
)
```

### 媒体源 VesperPlayerSource

```dart
// HLS 流
VesperPlayerSource.hls(uri: 'https://example.com/stream.m3u8')

// DASH 流（仅 Android）
VesperPlayerSource.dash(uri: 'https://example.com/manifest.mpd')

// 本地文件
VesperPlayerSource.local(uri: '/storage/emulated/0/Movies/video.mp4')

// 通用远程
VesperPlayerSource.remote(uri: 'https://example.com/video.mp4')
```

## 轨道选择与 ABR

```dart
// 查询可用轨道
final catalog = controller.snapshot.trackCatalog;
final audioTracks = catalog.audioTracks;   // List<VesperMediaTrack>
final videoTracks = catalog.videoTracks;

// 手动切换音轨
await controller.setAudioTrackSelection(
  VesperTrackSelection.track(audioTracks.first.id),
);

// 恢复自动选择
await controller.setAudioTrackSelection(const VesperTrackSelection.auto());

// 关闭字幕
await controller.setSubtitleTrackSelection(const VesperTrackSelection.disabled());

// ABR 策略：限制最高分辨率
await controller.setAbrPolicy(
  const VesperAbrPolicy.constrained(maxHeight: 720),
);

// ABR 策略：锁定指定视频轨道
await controller.setAbrPolicy(
  VesperAbrPolicy.fixedTrack(videoTracks.last.id),
);
```

## 直播与 DVR

```dart
// 检查时间线类型
final timeline = controller.snapshot.timeline;

if (timeline.kind == VesperTimelineKind.liveDvr) {
  // DVR 直播：可拖动的时间窗口
  final seekableRange = timeline.seekableRange!;
  print('可寻位范围: ${seekableRange.startMs}ms ~ ${seekableRange.endMs}ms');
  print('直播延迟: ${timeline.liveOffsetMs}ms');
  
  // 回到实时边缘
  await controller.seekToLiveEdge();
  
  // 检查是否在实时边缘
  if (timeline.isAtLiveEdge()) {
    print('当前处于实时位置');
  }
}
```

## 抗弹性策略

通过 `VesperPlaybackResiliencePolicy` 控制缓冲、重试和缓存行为。

```dart
// 使用内置预设
final controller = await VesperPlayerController.create(
  resiliencePolicy: const VesperPlaybackResiliencePolicy.resilient(), // 高抗性预设
);

// 或自定义配置
final policy = VesperPlaybackResiliencePolicy(
  buffering: const VesperBufferingPolicy.streaming(),
  retry: const VesperRetryPolicy(
    maxAttempts: 5,
    backoff: VesperRetryBackoff.exponential,
    baseDelayMs: 500,
    maxDelayMs: 8000,
  ),
  cache: const VesperCachePolicy.resilient(),
);

// 运行时动态更新
await controller.setPlaybackResiliencePolicy(policy);
```

**内置抗弹性预设**

| 预设 | 缓冲策略 | 重试策略 | 适用场景 |
|---|---|---|---|
| `default` | 默认 | 默认 | 通用 |
| `balanced()` | 平衡 | 线性退避 | 网络较好 |
| `streaming()` | 流式优先 | 激进重试 | 实时流媒体 |
| `resilient()` | 高缓冲 | 指数退避 x6 | 弱网环境 |
| `lowLatency()` | 低延迟 | 快速失败 | 直播低延迟 |

## 下载管理

`VesperDownloadManager` 管理媒体资源的本地下载，支持暂停、恢复和进度追踪。

```dart
// 创建下载管理器
final manager = await VesperDownloadManager.create();

// 创建下载任务（返回 taskId）
final taskId = await manager.createTask(
  assetId: 'my-video-01',                 // 唯一资产标识
  source: VesperDownloadSource.fromSource(
    source: VesperPlayerSource.hls(uri: 'https://example.com/stream.m3u8'),
  ),
  profile: const VesperDownloadProfile(
    preferredAudioLanguage: 'zh',
    allowMeteredNetwork: false,
  ),
);

// 监听下载进度
manager.snapshots.listen((snapshot) {
  for (final task in snapshot.tasks) {
    final ratio = task.progress.completionRatio;
    print('任务 ${task.taskId}: ${(ratio! * 100).toInt()}%  状态: ${task.state}');
  }
});

// 控制任务
await manager.pauseTask(taskId!);
await manager.resumeTask(taskId);
await manager.removeTask(taskId);

// 销毁
await manager.dispose();
```

### 远程 HLS / DASH 下载的推荐链路

如果下载源是远程 `HLS` / `DASH`，推荐不要直接把一个 manifest URL 原样传给 `createTask(...)` 就结束，而是按下面的流程接入：

1. 宿主 UI 在用户点击后先插入一个“准备中”的占位任务，名称可先用 URL 推导出的临时名。
2. 后台读取远端 manifest，并预先组装 `VesperDownloadSource`、`VesperDownloadProfile(targetDirectory: ...)` 和 `VesperDownloadAssetIndex(resources: ..., segments: ...)`。
3. 准备完成后再调用 `createTask(...)`，让真实任务接管 UI；如果 manifest 里能解析出更合适的标题，再覆盖占位名称。

这样做有两个好处：

- 用户点击后能立刻看到任务，避免“按钮点了但列表没反应”的空窗。
- 下载管理器会真正落地 `resources + segments`，后续离线保存、导出 `.mp4` 或做宿主级回归时，不会只剩一个 manifest URL。

补充说明：

- iOS 当前示例只对远程 `HLS` 走这条预规划链路；`DASH` 在 AVPlayer backend 上仍明确是 `unsupported`。
- 暂停 / 恢复 / 移除都应严格以 `taskId` 为准，宿主 UI 不应按 URL 归并多个任务的进度或状态。

### 可选：使用 `player-ffmpeg` 导出 `.mp4`

`player-ffmpeg` 是一个**可选动态插件**，用于把 HLS / DASH 分片下载结果 remux 成 `.mp4`。默认 SDK 与 Flutter 包**不会自动把它混入你的业务产物**；只有在宿主 app 明确把插件库打进包，并把路径传给 `VesperDownloadConfiguration.pluginLibraryPaths` 后，下载导出能力才会启用。

```dart
// 由宿主 app 自己解析出已打包插件的真实绝对路径。
final pluginLibraryPaths = <String>[
  '/absolute/path/to/libplayer_ffmpeg.so',
];

final manager = await VesperDownloadManager.create(
  configuration: VesperDownloadConfiguration(
    // 如果你希望“下载完成后先保留原始产物，等用户点保存时再导出”，
    // 就像 example 那样把它关掉；需要自动后处理时可改回 true。
    runPostProcessorsOnCompletion: false,
    pluginLibraryPaths: pluginLibraryPaths,
  ),
);

manager.events.listen((event) {
  if (event is VesperDownloadExportProgressEvent) {
    print('task ${event.taskId}: ${(event.ratio * 100).toInt()}%');
  }
});

await manager.exportTaskOutput(taskId, '/path/to/output.mp4');
```

要点：

- `pluginLibraryPaths` 需要传入宿主 app 内**已经打包并可访问**的 `libplayer_ffmpeg.so` / `libplayer_ffmpeg.dylib` 真实路径。
- `exportTaskOutput(...)` 会触发插件执行，并通过 `VesperDownloadExportProgressEvent` 回报导出进度。
- 当前移动端 example 已默认打包 remux 插件，可直接作为宿主接线参考：
  - Android example 通过 Gradle `preBuild` 构建并打入 `jniLibs`
  - iOS example 通过 Xcode `Embed player-ffmpeg plugin` build phase 嵌入已签名 dylib
- 业务 app 默认并不会因为依赖 `vesper_player` 就自动获得 FFmpeg；这是为了保证**不需要导出能力时，不额外增大产物体积**。
- 当前 FFmpeg 预编译仍是**粗粒度按需构建**：默认只在显式接入 `player-ffmpeg` 时才构建 / 打包，并支持通过环境变量关闭 DASH 相关依赖；但还没有做到 demuxer / muxer / protocol / codec 级别的最小白名单裁剪。

**下载任务状态**

```
queued → preparing → downloading → completed
                  ↘ paused ↗
                  ↘ failed
                  ↘ removed
```

## 能力查询

不同平台、不同后端的能力通过 `VesperPlayerCapabilities` 报告，避免在运行时调用不支持的 API。

```dart
final caps = controller.snapshot.capabilities;

if (caps.supportsDash) {
  // 可以使用 DASH 流
}

if (caps.supportsTrackSelection) {
  // 可以进行轨道选择
}

if (caps.isExperimental) {
  // 当前后端处于实验状态，能力可能不完整
}
```

## 相关包

| 包 | 描述 |
|---|---|
| [vesper_player_platform_interface] | 平台接口与共享数据模型（插件开发者使用） |
| [vesper_player_android] | Android 平台实现（基于 ExoPlayer） |
| [vesper_player_ios] | iOS 平台实现（基于 AVPlayer） |
| [vesper_player_macos] | macOS 实验实现 |
