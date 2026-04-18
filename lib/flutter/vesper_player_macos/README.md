# vesper_player_macos

[`vesper_player`] 的 macOS 平台实现包。

> ⚠️ **实验性**：此包当前处于实验阶段，尚无真实后端实现，API 行为与能力矩阵与移动端不完全对齐，不建议在生产环境使用。

## 当前状态

```dart
abstract final class VesperPlayerMacosPackage {
  static const bool isImplemented = false;
}
```

- 包结构与注册机制已就位
- 无真实播放后端；所有播放操作将通过 `VesperPlayerCapabilities` 报告为不支持
- 无 CI 路径

## 计划方向

macOS 后端将采用 **native-first** 路线（AVFoundation），并在基础控制闭环（本地文件 / 基础流媒体 / 状态链路）验证后逐步补齐能力。

具体进展见项目 ROADMAP.md 的 Phase 4。

## 相关资源

- 主包：[`vesper_player`]
- 平台接口：[`vesper_player_platform_interface`]
