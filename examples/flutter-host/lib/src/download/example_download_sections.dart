import 'package:material_ui/material_ui.dart';
import 'package:vesper_player/vesper_player.dart';

import '../player/example_player_helpers.dart';
import '../player/example_player_models.dart';
import '../player/example_player_sections.dart';
import '../shell/example_host_strings.dart';

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
    final strings = ExampleHostStrings.of(context);
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
          strings[ExampleHostText.downloadHeaderSubtitle],
          style: theme.textTheme.bodyMedium?.copyWith(
            color: palette.body,
            height: 1.45,
          ),
        ),
        const SizedBox(height: 4),
        Text(
          strings[isDownloadExportPluginInstalled
              ? ExampleHostText.downloadExportInstalled
              : ExampleHostText.downloadExportMissing],
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
    final strings = ExampleHostStrings.of(context);
    return ExampleSectionShell(
      palette: palette,
      title: strings[ExampleHostText.downloadCreateTitle],
      subtitle: strings[ExampleHostText.downloadCreateSubtitle],
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Wrap(
            spacing: 10,
            runSpacing: 10,
            children: <Widget>[
              OutlinedButton(
                onPressed: onUseHlsDemo,
                child: Text(strings[ExampleHostText.downloadHlsDemo]),
              ),
              OutlinedButton(
                onPressed: onUseDashDemo,
                child: Text(strings[ExampleHostText.downloadDashDemo]),
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
            decoration: InputDecoration(
              labelText: strings[ExampleHostText.downloadUrlLabel],
            ),
          ),
          const SizedBox(height: 14),
          FilledButton(
            onPressed: onCreateRemote,
            style: FilledButton.styleFrom(
              backgroundColor: palette.primaryAction,
              foregroundColor: palette.onPrimaryAction,
            ),
            child: Text(strings[ExampleHostText.downloadCreateRemote]),
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
    final strings = ExampleHostStrings.of(context);
    return ExampleSectionShell(
      palette: palette,
      title: strings[ExampleHostText.downloadTasksTitle],
      subtitle: strings[ExampleHostText.downloadTasksSubtitle],
      child: visibleTasks.isEmpty && pendingTasks.isEmpty
          ? Text(
              strings[ExampleHostText.downloadTasksEmpty],
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
    final strings = ExampleHostStrings.of(context);
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
            'assetId: ${task.assetId} · '
            '${strings[ExampleHostText.downloadStatePreparing]}',
            style: theme.textTheme.bodySmall?.copyWith(
              color: palette.body,
              height: 1.45,
            ),
          ),
          const SizedBox(height: 10),
          const LinearProgressIndicator(),
          const SizedBox(height: 10),
          Text(
            strings[ExampleHostText.downloadPlanning],
            style: theme.textTheme.bodySmall?.copyWith(
              color: palette.body,
              height: 1.45,
            ),
          ),
          const SizedBox(height: 6),
          Text(
            strings.format(
              ExampleHostText.downloadRemoteAddress,
              <String, Object?>{'uri': task.sourceUri},
            ),
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
    final strings = ExampleHostStrings.of(context);
    final primaryActionLabel = _downloadPrimaryActionLabel(task.state, strings);
    final canSaveToGallery =
        task.assetIndex.completedPath?.trim().isNotEmpty ?? false;
    final requiresExport =
        task.source.contentFormat == VesperDownloadContentFormat.hlsSegments ||
        task.source.contentFormat == VesperDownloadContentFormat.dashSegments ||
        task.source.contentFormat == VesperDownloadContentFormat.flvSegments;
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
            'assetId: ${task.assetId} · '
            '${_downloadStateLabel(task.state, strings)}',
            style: theme.textTheme.bodySmall?.copyWith(
              color: palette.body,
              height: 1.45,
            ),
          ),
          const SizedBox(height: 6),
          Text(
            _downloadProgressSummary(task, strings),
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
              strings.format(
                ExampleHostText.downloadExporting,
                <String, Object?>{
                  'percent': (normalizedExportProgress * 100).toInt(),
                },
              ),
              style: theme.textTheme.bodySmall?.copyWith(
                color: palette.body,
                height: 1.45,
              ),
            ),
          ],
          if (task.assetIndex.completedPath case final path?) ...<Widget>[
            const SizedBox(height: 6),
            Text(
              strings.format(
                ExampleHostText.downloadOutputPath,
                <String, Object?>{'path': path},
              ),
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
              strings.format(ExampleHostText.downloadError, <String, Object?>{
                'message': message,
              }),
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
                    foregroundColor: palette.onPrimaryAction,
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
                    strings[isSaving && normalizedExportProgress != null
                        ? ExampleHostText.downloadExportingShort
                        : ExampleHostText.downloadSaveToGallery],
                  ),
                ),
              TextButton(
                onPressed: isSaving ? null : onRemoveTask,
                child: Text(strings[ExampleHostText.downloadRemove]),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

String _downloadStateLabel(
  VesperDownloadState state,
  ExampleHostStrings strings,
) {
  return strings[switch (state) {
    VesperDownloadState.queued => ExampleHostText.downloadStateQueued,
    VesperDownloadState.preparing => ExampleHostText.downloadStatePreparing,
    VesperDownloadState.downloading => ExampleHostText.downloadStateDownloading,
    VesperDownloadState.paused => ExampleHostText.downloadStatePaused,
    VesperDownloadState.completed => ExampleHostText.downloadStateCompleted,
    VesperDownloadState.failed => ExampleHostText.downloadStateFailed,
    VesperDownloadState.removed => ExampleHostText.downloadStateRemoved,
    VesperDownloadState.unknown => ExampleHostText.downloadStateUnknown,
  }];
}

String? _downloadPrimaryActionLabel(
  VesperDownloadState state,
  ExampleHostStrings strings,
) {
  final key = switch (state) {
    VesperDownloadState.queued ||
    VesperDownloadState.failed => ExampleHostText.downloadActionStart,
    VesperDownloadState.preparing ||
    VesperDownloadState.downloading => ExampleHostText.downloadActionPause,
    VesperDownloadState.paused => ExampleHostText.downloadActionResume,
    VesperDownloadState.completed ||
    VesperDownloadState.removed ||
    VesperDownloadState.unknown => null,
  };
  return key == null ? null : strings[key];
}

String _downloadProgressSummary(
  VesperDownloadTaskSnapshot task,
  ExampleHostStrings strings,
) {
  final ratio = task.progress.completionRatio;
  final ratioText = ratio == null
      ? strings[ExampleHostText.downloadProgressUnknown]
      : '${(ratio * 100).toInt()}%';
  final bytesText =
      '${formatDownloadBytes(task.progress.receivedBytes)} / ${formatDownloadBytes(task.progress.totalBytes)}';
  return '$ratioText · $bytesText';
}
