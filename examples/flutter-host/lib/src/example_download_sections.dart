import 'package:flutter/material.dart';
import 'package:vesper_player/vesper_player.dart';

import 'example_player_helpers.dart';
import 'example_player_models.dart';
import 'example_player_sections.dart';

final class ExamplePendingDownloadTask {
  const ExamplePendingDownloadTask({
    required this.requestId,
    required this.assetId,
    required this.label,
    required this.sourceUri,
  });

  final String requestId;
  final String assetId;
  final String label;
  final String sourceUri;
}

class ExampleDownloadHeader extends StatelessWidget {
  const ExampleDownloadHeader({
    super.key,
    required this.palette,
    required this.isDownloadExportPluginInstalled,
  });

  final ExampleHostPalette palette;
  final bool isDownloadExportPluginInstalled;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        Text(
          'Vesper Download',
          style: theme.textTheme.headlineMedium?.copyWith(
            color: palette.title,
            fontWeight: FontWeight.w900,
            letterSpacing: -1.2,
          ),
        ),
        const SizedBox(height: 8),
        Text(
          '这个页面专门用于下载会话回归，重点验证共享下载状态机和宿主桥接，不把它包装成完整离线产品流程。',
          style: theme.textTheme.bodyMedium?.copyWith(
            color: palette.body,
            height: 1.45,
          ),
        ),
        const SizedBox(height: 4),
        Text(
          isDownloadExportPluginInstalled ? 'MP4 合成库：已安装' : 'MP4 合成库未安装。',
          style: theme.textTheme.bodySmall?.copyWith(
            color: palette.body,
            height: 1.45,
          ),
        ),
      ],
    );
  }
}

class ExampleDownloadCreateSection extends StatelessWidget {
  const ExampleDownloadCreateSection({
    super.key,
    required this.palette,
    required this.remoteUrlController,
    required this.onUseHlsDemo,
    required this.onUseDashDemo,
    required this.onCreateRemote,
    this.message,
  });

  final ExampleHostPalette palette;
  final TextEditingController remoteUrlController;
  final VoidCallback onUseHlsDemo;
  final VoidCallback onUseDashDemo;
  final VoidCallback onCreateRemote;
  final String? message;

  @override
  Widget build(BuildContext context) {
    return ExampleSectionShell(
      palette: palette,
      title: '创建下载任务',
      subtitle: '用 HLS 和 DASH 演示按钮快速回归，或者粘贴远程 URL 创建一个前台下载任务。',
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Wrap(
            spacing: 10,
            runSpacing: 10,
            children: <Widget>[
              OutlinedButton(
                onPressed: onUseHlsDemo,
                child: const Text('下载 HLS 演示'),
              ),
              OutlinedButton(
                onPressed: onUseDashDemo,
                child: const Text('下载 DASH 演示'),
              ),
            ],
          ),
          if (message case final value?) ...<Widget>[
            const SizedBox(height: 12),
            Text(
              value,
              style: Theme.of(context).textTheme.bodySmall?.copyWith(
                color: const Color(0xFFC13C36),
                height: 1.45,
              ),
            ),
          ],
          const SizedBox(height: 14),
          TextField(
            controller: remoteUrlController,
            keyboardType: TextInputType.url,
            maxLines: 1,
            decoration: const InputDecoration(labelText: '下载 URL'),
          ),
          const SizedBox(height: 14),
          FilledButton(
            onPressed: onCreateRemote,
            style: FilledButton.styleFrom(
              backgroundColor: palette.primaryAction,
              foregroundColor: Colors.white,
            ),
            child: const Text('创建远程任务'),
          ),
        ],
      ),
    );
  }
}

class ExampleDownloadTasksSection extends StatelessWidget {
  const ExampleDownloadTasksSection({
    super.key,
    required this.palette,
    required this.tasks,
    required this.pendingTasks,
    required this.isDownloadExportPluginInstalled,
    required this.savingTaskIds,
    required this.exportProgressByTaskId,
    required this.onPrimaryAction,
    required this.onSaveToGallery,
    required this.onRemoveTask,
  });

  final ExampleHostPalette palette;
  final List<VesperDownloadTaskSnapshot> tasks;
  final List<ExamplePendingDownloadTask> pendingTasks;
  final bool isDownloadExportPluginInstalled;
  final Set<int> savingTaskIds;
  final Map<int, double> exportProgressByTaskId;
  final ValueChanged<VesperDownloadTaskSnapshot> onPrimaryAction;
  final ValueChanged<VesperDownloadTaskSnapshot> onSaveToGallery;
  final ValueChanged<VesperDownloadTaskSnapshot> onRemoveTask;

  @override
  Widget build(BuildContext context) {
    final visibleTasks = tasks
        .where((task) => task.state != VesperDownloadState.removed)
        .toList(growable: false);
    final taskChildren = <Widget>[
      ...pendingTasks.reversed.map(
        (task) => Padding(
          padding: const EdgeInsets.only(bottom: 12),
          child: _ExamplePendingDownloadTaskRow(task: task, palette: palette),
        ),
      ),
      ...visibleTasks.reversed.map(
        (task) => Padding(
          padding: const EdgeInsets.only(bottom: 12),
          child: _ExampleDownloadTaskRow(
            task: task,
            palette: palette,
            isDownloadExportPluginInstalled: isDownloadExportPluginInstalled,
            isSaving: savingTaskIds.contains(task.taskId),
            exportProgress: exportProgressByTaskId[task.taskId],
            onPrimaryAction: () => onPrimaryAction(task),
            onSaveToGallery: () => onSaveToGallery(task),
            onRemoveTask: () => onRemoveTask(task),
          ),
        ),
      ),
    ];
    return ExampleSectionShell(
      palette: palette,
      title: '任务列表',
      subtitle: '这个示例保持在前台执行和任务生命周期回归层面，不宣称后台恢复、离线 DRM 或完整打包式 HLS/DASH 产品能力。',
      child: visibleTasks.isEmpty && pendingTasks.isEmpty
          ? Text(
              '还没有创建任何下载任务。',
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(color: palette.body),
            )
          : Column(children: taskChildren),
    );
  }
}

class _ExamplePendingDownloadTaskRow extends StatelessWidget {
  const _ExamplePendingDownloadTaskRow({
    required this.task,
    required this.palette,
  });

  final ExamplePendingDownloadTask task;
  final ExampleHostPalette palette;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(16),
      decoration: BoxDecoration(
        color: palette.fieldBackground,
        borderRadius: BorderRadius.circular(20),
        border: Border.all(color: palette.sectionStroke),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Text(
            task.label,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: theme.textTheme.bodyLarge?.copyWith(
              color: palette.title,
              fontWeight: FontWeight.w600,
            ),
          ),
          const SizedBox(height: 8),
          Text(
            'assetId: ${task.assetId} · 准备中',
            style: theme.textTheme.bodySmall?.copyWith(
              color: palette.body,
              height: 1.45,
            ),
          ),
          const SizedBox(height: 10),
          const LinearProgressIndicator(),
          const SizedBox(height: 10),
          Text(
            '正在读取远程清单并生成下载计划…',
            style: theme.textTheme.bodySmall?.copyWith(
              color: palette.body,
              height: 1.45,
            ),
          ),
          const SizedBox(height: 6),
          Text(
            '远程地址：${task.sourceUri}',
            maxLines: 2,
            overflow: TextOverflow.ellipsis,
            style: theme.textTheme.bodySmall?.copyWith(
              color: palette.body,
              height: 1.45,
            ),
          ),
        ],
      ),
    );
  }
}

class _ExampleDownloadTaskRow extends StatelessWidget {
  const _ExampleDownloadTaskRow({
    required this.task,
    required this.palette,
    required this.isDownloadExportPluginInstalled,
    required this.isSaving,
    required this.exportProgress,
    required this.onPrimaryAction,
    required this.onSaveToGallery,
    required this.onRemoveTask,
  });

  final VesperDownloadTaskSnapshot task;
  final ExampleHostPalette palette;
  final bool isDownloadExportPluginInstalled;
  final bool isSaving;
  final double? exportProgress;
  final VoidCallback onPrimaryAction;
  final VoidCallback onSaveToGallery;
  final VoidCallback onRemoveTask;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final primaryActionLabel = _downloadPrimaryActionLabel(task.state);
    final canSaveToGallery =
        task.assetIndex.completedPath?.trim().isNotEmpty ?? false;
    final requiresExport =
        task.source.contentFormat == VesperDownloadContentFormat.hlsSegments ||
        task.source.contentFormat == VesperDownloadContentFormat.dashSegments;
    final saveButtonVisuallyUnavailable =
        requiresExport && !isDownloadExportPluginInstalled && !isSaving;
    final normalizedExportProgress = exportProgress?.clamp(0, 1).toDouble();
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(16),
      decoration: BoxDecoration(
        color: palette.fieldBackground,
        borderRadius: BorderRadius.circular(20),
        border: Border.all(color: palette.sectionStroke),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Text(
            task.source.source.label,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: theme.textTheme.bodyLarge?.copyWith(
              color: palette.title,
              fontWeight: FontWeight.w600,
            ),
          ),
          const SizedBox(height: 8),
          Text(
            'assetId: ${task.assetId} · ${_downloadStateLabel(task.state)}',
            style: theme.textTheme.bodySmall?.copyWith(
              color: palette.body,
              height: 1.45,
            ),
          ),
          const SizedBox(height: 6),
          Text(
            _downloadProgressSummary(task),
            style: theme.textTheme.bodySmall?.copyWith(
              color: palette.body,
              height: 1.45,
            ),
          ),
          if (isSaving && normalizedExportProgress != null) ...<Widget>[
            const SizedBox(height: 10),
            LinearProgressIndicator(
              value: normalizedExportProgress,
              color: palette.primaryAction,
            ),
            const SizedBox(height: 6),
            Text(
              '正在合成 MP4… ${(normalizedExportProgress * 100).toInt()}%',
              style: theme.textTheme.bodySmall?.copyWith(
                color: palette.body,
                height: 1.45,
              ),
            ),
          ],
          if (task.assetIndex.completedPath case final path?) ...<Widget>[
            const SizedBox(height: 6),
            Text(
              '输出位置: $path',
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: theme.textTheme.bodySmall?.copyWith(
                color: palette.body,
                height: 1.45,
              ),
            ),
          ],
          if (task.error?.message case final message?) ...<Widget>[
            const SizedBox(height: 6),
            Text(
              '错误: $message',
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: theme.textTheme.bodySmall?.copyWith(
                color: const Color(0xFFC13C36),
                height: 1.45,
              ),
            ),
          ],
          const SizedBox(height: 12),
          Wrap(
            spacing: 10,
            runSpacing: 10,
            children: <Widget>[
              if (primaryActionLabel != null)
                FilledButton(
                  onPressed: isSaving ? null : onPrimaryAction,
                  style: FilledButton.styleFrom(
                    backgroundColor: palette.primaryAction,
                    foregroundColor: Colors.white,
                  ),
                  child: Text(primaryActionLabel),
                ),
              if (canSaveToGallery)
                OutlinedButton(
                  onPressed: isSaving ? null : onSaveToGallery,
                  style: OutlinedButton.styleFrom(
                    foregroundColor: saveButtonVisuallyUnavailable
                        ? palette.body.withValues(alpha: 0.55)
                        : palette.title,
                  ),
                  child: Text(
                    isSaving && normalizedExportProgress != null
                        ? '正在合成 MP4'
                        : '转存到相册',
                  ),
                ),
              TextButton(
                onPressed: isSaving ? null : onRemoveTask,
                child: const Text('移除'),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

String _downloadStateLabel(VesperDownloadState state) {
  return switch (state) {
    VesperDownloadState.queued => '排队中',
    VesperDownloadState.preparing => '准备中',
    VesperDownloadState.downloading => '下载中',
    VesperDownloadState.paused => '已暂停',
    VesperDownloadState.completed => '已完成',
    VesperDownloadState.failed => '失败',
    VesperDownloadState.removed => '已移除',
  };
}

String? _downloadPrimaryActionLabel(VesperDownloadState state) {
  return switch (state) {
    VesperDownloadState.queued || VesperDownloadState.failed => '开始',
    VesperDownloadState.preparing || VesperDownloadState.downloading => '暂停',
    VesperDownloadState.paused => '恢复',
    VesperDownloadState.completed || VesperDownloadState.removed => null,
  };
}

String _downloadProgressSummary(VesperDownloadTaskSnapshot task) {
  final ratio = task.progress.completionRatio;
  final ratioText = ratio == null ? '进度未知' : '${(ratio * 100).toInt()}%';
  final bytesText =
      '${formatDownloadBytes(task.progress.receivedBytes)} / ${formatDownloadBytes(task.progress.totalBytes)}';
  return '$ratioText · $bytesText';
}
