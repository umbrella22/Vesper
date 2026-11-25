part of 'player_host_page.dart';

/// Download tab helpers: the create-download sheet and undoable removal.
extension _PlayerHostShellDownloads on _PlayerHostPageState {
  Future<void> _openDownloadCreateSheet(
    VesperDownloadManager manager,
    ExampleHostPalette palette,
  ) {
    final strings = ExampleHostStrings.of(context);
    void createAndClose(BuildContext sheetContext, Future<void> task) {
      Navigator.of(sheetContext).pop();
      unawaited(task);
    }

    return showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      useSafeArea: true,
      showDragHandle: true,
      builder: (sheetContext) => ExampleHostStringsScope(
        strings: strings,
        child: Padding(
          padding: EdgeInsets.only(
            bottom: MediaQuery.viewInsetsOf(sheetContext).bottom,
          ),
          child: SingleChildScrollView(
            key: const ValueKey<String>('downloads-create-sheet'),
            padding: const EdgeInsets.fromLTRB(18, 0, 18, 28),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              mainAxisSize: MainAxisSize.min,
              children: <Widget>[
                Text(
                  strings[ExampleHostText.downloadsNew],
                  style: Theme.of(sheetContext).textTheme.titleLarge,
                ),
                const SizedBox(height: 4),
                Text(
                  strings[ExampleHostText.downloadsNewSubtitle],
                  style: Theme.of(sheetContext).textTheme.bodySmall,
                ),
                const SizedBox(height: 12),
                ExampleDownloadCreateSection(
                  palette: palette,
                  remoteUrlController: _downloadUrlController,
                  message: null,
                  onUseHlsDemo: () => createAndClose(
                    sheetContext,
                    _createDownloadTask(
                      manager,
                      assetIdPrefix: flutterHlsPlaylistItemId,
                      source: flutterHlsDemoSource(),
                    ),
                  ),
                  onUseDashDemo: () => createAndClose(
                    sheetContext,
                    _createDownloadTask(
                      manager,
                      assetIdPrefix: flutterDashPlaylistItemId,
                      source: flutterDashDemoSource(),
                    ),
                  ),
                  onCreateRemote: () => createAndClose(
                    sheetContext,
                    _createRemoteDownloadTask(manager),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  /// Hides [task] immediately and removes it once the undo SnackBar closes
  /// without the user pressing undo.
  void _removeDownloadWithUndo(
    VesperDownloadManager manager,
    VesperDownloadTaskSnapshot task,
  ) {
    final strings = ExampleHostStrings.of(context);
    final messenger = ScaffoldMessenger.maybeOf(context);
    _updateState(() {
      _pendingRemovalTaskIds = <int>{..._pendingRemovalTaskIds, task.taskId};
    });
    if (messenger == null) {
      unawaited(_commitDownloadRemoval(manager, task.taskId));
      return;
    }
    var undone = false;
    messenger.hideCurrentSnackBar();
    final controller = messenger.showSnackBar(
      SnackBar(
        key: ValueKey<String>('download-removed-${task.taskId}'),
        content: Text(strings[ExampleHostText.downloadRemoved]),
        duration: const Duration(seconds: 4),
        persist: false,
        action: SnackBarAction(
          label: strings[ExampleHostText.undo],
          onPressed: () {
            undone = true;
            if (mounted) {
              _updateState(() {
                _pendingRemovalTaskIds = <int>{
                  ..._pendingRemovalTaskIds.where((id) => id != task.taskId),
                };
              });
            }
          },
        ),
      ),
    );
    unawaited(
      controller.closed.then((_) {
        if (!undone) {
          return _commitDownloadRemoval(manager, task.taskId);
        }
      }),
    );
  }

  Future<void> _commitDownloadRemoval(
    VesperDownloadManager manager,
    int taskId,
  ) async {
    try {
      await manager.removeTask(taskId);
    } finally {
      if (mounted) {
        _updateState(() {
          _pendingRemovalTaskIds = <int>{
            ..._pendingRemovalTaskIds.where((id) => id != taskId),
          };
        });
      }
    }
  }
}
