part of 'player_host_page.dart';

extension _PlayerHostLayout on _PlayerHostPageState {
  Widget _buildPlayerFutureContent(
    BuildContext context, {
    required bool immersivePlayer,
    required ExampleHostPalette palette,
    double bottomPadding = 18,
  }) {
    final strings = ExampleHostStrings.of(context);
    return FutureBuilder<VesperPlayerController>(
      future: _controllerFuture,
      builder: (context, asyncSnapshot) {
        if (asyncSnapshot.hasError && !asyncSnapshot.hasData) {
          return ExampleErrorState(error: asyncSnapshot.error);
        }

        final controller = asyncSnapshot.data ?? _controller;
        if (controller == null) {
          return const ExampleLoadingState();
        }
        final playlistItems = _buildPlaylistItems();

        final content = immersivePlayer
            ? _buildLandscapeLayout(controller, asyncSnapshot)
            : _buildPortraitLayout(
                context,
                controller,
                playlistItems,
                palette,
                asyncSnapshot,
                bottomPadding,
              );

        return Stack(
          children: <Widget>[
            Positioned.fill(child: content),
            if (_isApplyingResilienceProfile)
              Positioned(
                top: 18,
                right: 18,
                child: ExampleBusyPill(
                  label: strings[ExampleHostText.busyApplyingPolicy],
                ),
              ),
            if (_isRebuildingController)
              Positioned(
                top: 18,
                left: 18,
                child: ExampleBusyPill(
                  label: strings[ExampleHostText.busySwitchingPlugin],
                ),
              ),
          ],
        );
      },
    );
  }

  Widget _buildDiagnosticsFutureContent(
    BuildContext context, {
    required ExampleHostPalette palette,
  }) {
    return FutureBuilder<VesperPlayerController>(
      future: _controllerFuture,
      builder: (context, asyncSnapshot) {
        if (asyncSnapshot.hasError && !asyncSnapshot.hasData) {
          return ExampleErrorState(error: asyncSnapshot.error);
        }

        final controller = asyncSnapshot.data ?? _controller;
        if (controller == null) {
          return const ExampleLoadingState();
        }

        return ValueListenableBuilder<VesperPlayerSnapshot>(
          valueListenable: controller.snapshotListenable,
          builder: (context, snapshot, _) {
            return _buildDiagnosticsLayout(
              context,
              controller,
              snapshot,
              palette,
            );
          },
        );
      },
    );
  }

  Widget _buildPictureInPicturePresentationContent() {
    return FutureBuilder<VesperPlayerController>(
      future: _controllerFuture,
      builder: (context, asyncSnapshot) {
        final controller = asyncSnapshot.data ?? _controller;
        if (controller == null) {
          return const ColoredBox(color: Colors.black);
        }
        return ValueListenableBuilder<VesperPlayerSnapshot>(
          valueListenable: controller.snapshotListenable,
          builder: (context, snapshot, _) {
            return ColoredBox(
              color: Colors.black,
              child: SizedBox.expand(
                child: ExamplePlayerStage(
                  controller: controller,
                  snapshot: snapshot,
                  controlLayout: ui.VesperStageControlLayout.expanded,
                  isFullscreen: false,
                  sheetOpen: false,
                  pictureInPicturePresentation: true,
                  onOpenSheet: (_) {},
                  onToggleFullscreen: () {},
                ),
              ),
            );
          },
        );
      },
    );
  }

  Widget _buildPortraitLayout(
    BuildContext context,
    VesperPlayerController controller,
    List<ExamplePlaylistItemViewData> playlistItems,
    ExampleHostPalette palette,
    AsyncSnapshot<VesperPlayerController> asyncSnapshot, [
    double bottomPadding = 18,
  ]) {
    final transientError = asyncSnapshot.hasError ? asyncSnapshot.error : null;
    final strings = ExampleHostStrings.of(context);

    return NotificationListener<ScrollUpdateNotification>(
      onNotification: _handlePlayScroll,
      child: SingleChildScrollView(
        padding: EdgeInsets.fromLTRB(18, 8, 18, bottomPadding),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            if (transientError != null) ...<Widget>[
              ExampleInlineControllerError(error: transientError),
              const SizedBox(height: 18),
            ],
            SizedBox(
              width: double.infinity,
              height: 248,
              child: ValueListenableBuilder<VesperPlayerSnapshot>(
                valueListenable: controller.snapshotListenable,
                builder: (context, snapshot, _) {
                  return ExamplePlayerStage(
                    controller: controller,
                    snapshot: snapshot,
                    controlLayout: ui.VesperStageControlLayout.compact,
                    isFullscreen: false,
                    sheetOpen: _sheetOpen,
                    deviceControls: _deviceControls,
                    topBarPrimaryAction: _buildStageRouteAction(controller),
                    onOpenSheet: (sheet) =>
                        unawaited(_openToolSheet(controller, sheet)),
                    onToggleFullscreen: () =>
                        unawaited(_toggleFullscreen(Orientation.portrait)),
                    pictureInPicturePresentation: _pictureInPicturePresentation,
                  );
                },
              ),
            ),
            const SizedBox(height: 16),
            _SnapshotSelector<({String title, String subtitle})>(
              listenable: controller.snapshotListenable,
              selector: (snapshot) => (
                title: snapshot.sourceLabel.isEmpty
                    ? snapshot.title
                    : snapshot.sourceLabel,
                subtitle: snapshot.subtitle,
              ),
              builder: (context, header) => ExampleNowPlayingCard(
                title: header.title,
                subtitle: header.subtitle,
              ),
            ),
            const SizedBox(height: 12),
            ExampleQuickActionsRow(
              actions: _buildPlayQuickActions(
                context,
                controller,
                palette,
                strings,
              ),
            ),
            const SizedBox(height: 20),
            ExampleQueuePanel(
              palette: palette,
              playlistItems: playlistItems,
              onSelectItem: (itemId) =>
                  unawaited(_focusPlaylistItem(controller, itemId)),
              onManageQueue: () {
                showModalBottomSheet<void>(
                  context: context,
                  isScrollControlled: true,
                  builder: (context) => ExampleQueueSheet(
                    palette: palette,
                    playlistItems: playlistItems,
                    onSelectItem: (itemId) =>
                        unawaited(_focusPlaylistItem(controller, itemId)),
                  ),
                );
              },
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildDiagnosticsLayout(
    BuildContext context,
    VesperPlayerController controller,
    VesperPlayerSnapshot snapshot,
    ExampleHostPalette palette,
  ) {
    final activeSource = _activePlaybackSource();
    return SingleChildScrollView(
      padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 18),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          ExampleDiagnosticsSummarySection(
            palette: palette,
            sourceLabel: snapshot.sourceLabel.isEmpty
                ? snapshot.title
                : snapshot.sourceLabel,
            sourceProtocol:
                activeSource?.protocol.name ??
                ExampleHostStrings.of(context)[ExampleHostText.none],
            routeLabel: _diagnosticsRouteLabel(),
            playbackOrigin: _playbackOrigin,
            sourceNormalizerSetting: _sourceNormalizerSetting,
          ),
          const SizedBox(height: 18),
          ExampleEventLogSection(palette: palette, entries: _hostLogEntries),
          const SizedBox(height: 18),
          ExampleShellSectionHeader(
            title: ExampleHostStrings.of(
              context,
            )[ExampleHostText.labSystemPlayback],
          ),
          const SizedBox(height: 8),
          _buildSystemPlaybackSection(controller, palette),
          const SizedBox(height: 18),
          ExamplePluginDiagnosticsSection(
            palette: palette,
            sourceNormalizerSetting: _sourceNormalizerSetting,
            sourceNormalizerPluginReferences: _sourceNormalizerPluginReferences,
            frameProcessorPluginReferences: _frameProcessorPluginReferences,
            pluginDiagnostics: controller.pluginDiagnostics,
            isCapturingHdrEvidence: _isCapturingHdrEvidence,
            hdrEvidenceActiveSourceAvailable:
                _selectedHdrEvidencePresetUsesNetworkControl ||
                exampleDolbyAcceptancePresetById(
                      _selectedHdrEvidencePreset.sampleId,
                    ) !=
                    null ||
                activeSource != null,
            hdrEvidencePresets: <ExampleHdrEvidenceSamplePreset>[
              ...exampleHdrEvidenceP0Presets,
              ...exampleDolbyAcceptanceHdrEvidencePresets(),
            ],
            selectedHdrEvidencePreset: _selectedHdrEvidencePreset,
            onSourceNormalizerSettingChange: (setting) =>
                unawaited(_applySourceNormalizerSetting(setting)),
            onHdrEvidencePresetChange: (preset) {
              _updateState(() {
                _selectedHdrEvidencePreset = preset;
              });
            },
            onCaptureHdrEvidence: () =>
                unawaited(_captureHdrEvidenceBundle(controller)),
          ),
          const SizedBox(height: 18),
          ExampleResilienceSection(
            palette: palette,
            activePolicy: snapshot.resiliencePolicy,
            selectedProfile: _selectedResilienceProfile,
            onApplyProfile: _applyResilienceProfile,
          ),
          if (snapshot.lastError != null) ...<Widget>[
            const SizedBox(height: 18),
            ExampleRecentErrorSection(
              palette: palette,
              error: snapshot.lastError!,
            ),
          ],
        ],
      ),
    );
  }

  String _diagnosticsRouteLabel() {
    if (_sourceNormalizerSetting == ExampleSourceNormalizerSetting.disabled ||
        _sourceNormalizerSetting ==
            ExampleSourceNormalizerSetting.diagnosticsOnly) {
      return 'direct native';
    }
    return _sourceNormalizerSetting.title(ExampleHostStrings.of(context));
  }

  Widget _buildLandscapeLayout(
    VesperPlayerController controller,
    AsyncSnapshot<VesperPlayerController> asyncSnapshot,
  ) {
    return Stack(
      children: <Widget>[
        Positioned.fill(
          child: ValueListenableBuilder<VesperPlayerSnapshot>(
            valueListenable: controller.snapshotListenable,
            builder: (context, snapshot, _) {
              return ExamplePlayerStage(
                controller: controller,
                snapshot: snapshot,
                controlLayout: ui.VesperStageControlLayout.expanded,
                isFullscreen: true,
                sheetOpen: _sheetOpen,
                deviceControls: _deviceControls,
                topBarPrimaryAction: _buildStageRouteAction(controller),
                onOpenSheet: (sheet) =>
                    unawaited(_openToolSheet(controller, sheet)),
                onToggleFullscreen: () =>
                    unawaited(_toggleFullscreen(Orientation.landscape)),
                pictureInPicturePresentation: _pictureInPicturePresentation,
              );
            },
          ),
        ),
        if (asyncSnapshot.hasError)
          Positioned(
            top: 18,
            left: 18,
            right: 96,
            child: ExampleInlineControllerError(error: asyncSnapshot.error),
          ),
      ],
    );
  }

  Widget? _buildStageRouteAction(VesperPlayerController controller) {
    if (Platform.isAndroid) {
      return const VesperExternalRouteIconButton(size: 38);
    }
    if (Platform.isIOS) {
      return ui.VesperAirPlayRouteIconButton(controller: controller, size: 38);
    }
    return null;
  }

  Widget _buildDownloadFutureContent(ExampleHostPalette palette) {
    final strings = ExampleHostStrings.of(context);
    final downloadManagerFuture = _ensureDownloadManagerFuture();
    return FutureBuilder<VesperDownloadManager>(
      future: downloadManagerFuture,
      builder: (context, asyncSnapshot) {
        if (asyncSnapshot.hasError && !asyncSnapshot.hasData) {
          return ExampleErrorState(error: asyncSnapshot.error);
        }

        final manager = asyncSnapshot.data ?? _downloadManager;
        if (manager == null) {
          return const ExampleLoadingState();
        }

        return ValueListenableBuilder<VesperDownloadSnapshot>(
          valueListenable: manager.snapshotListenable,
          builder: (context, snapshot, _) {
            return SingleChildScrollView(
              padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 18),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: <Widget>[
                  ExampleDownloadHeader(
                    palette: palette,
                    isDownloadExportPluginInstalled:
                        _isDownloadExportPluginInstalled,
                  ),
                  if (asyncSnapshot.hasError) ...<Widget>[
                    const SizedBox(height: 18),
                    ExampleInlineControllerError(error: asyncSnapshot.error),
                  ],
                  if (_downloadMessage != null) ...<Widget>[
                    const SizedBox(height: 12),
                    Text(
                      _downloadMessage!,
                      style: TextStyle(color: palette.error),
                    ),
                  ],
                  const SizedBox(height: 18),
                  ExampleShellSectionHeader(
                    title: strings[ExampleHostText.downloadsTasks],
                    trailing: FilledButton.icon(
                      key: const ValueKey<String>('downloads-new'),
                      onPressed: () =>
                          unawaited(_openDownloadCreateSheet(manager, palette)),
                      icon: const Icon(Icons.add_rounded),
                      label: Text(strings[ExampleHostText.downloadsNew]),
                    ),
                  ),
                  const SizedBox(height: 8),
                  ExampleDownloadTasksSection(
                    palette: palette,
                    tasks: snapshot.tasks
                        .where(
                          (task) =>
                              !_pendingRemovalTaskIds.contains(task.taskId),
                        )
                        .toList(growable: false),
                    pendingTasks: _pendingDownloadTasks,
                    isDownloadExportPluginInstalled:
                        _isDownloadExportPluginInstalled,
                    savingTaskIds: _savingTaskIds,
                    exportProgressByTaskId: _exportProgressByTaskId,
                    onPrimaryAction: (task) =>
                        unawaited(_handleDownloadPrimaryAction(manager, task)),
                    onSaveToGallery: (task) =>
                        unawaited(_saveDownloadToGallery(manager, task)),
                    onRemoveTask: (task) =>
                        _removeDownloadWithUndo(manager, task),
                  ),
                ],
              ),
            );
          },
        );
      },
    );
  }
}

class _SnapshotSelector<T> extends StatefulWidget {
  const _SnapshotSelector({
    required this.listenable,
    required this.selector,
    required this.builder,
  });

  final ValueListenable<VesperPlayerSnapshot> listenable;
  final T Function(VesperPlayerSnapshot snapshot) selector;
  final Widget Function(BuildContext context, T value) builder;

  @override
  State<_SnapshotSelector<T>> createState() => _SnapshotSelectorState<T>();
}

class _SnapshotSelectorState<T> extends State<_SnapshotSelector<T>> {
  late T _value;

  @override
  void initState() {
    super.initState();
    _value = widget.selector(widget.listenable.value);
    widget.listenable.addListener(_handleSnapshotChanged);
  }

  @override
  void didUpdateWidget(covariant _SnapshotSelector<T> oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.listenable, widget.listenable)) {
      oldWidget.listenable.removeListener(_handleSnapshotChanged);
      widget.listenable.addListener(_handleSnapshotChanged);
    }
    _syncSelectedValue();
  }

  @override
  void dispose() {
    widget.listenable.removeListener(_handleSnapshotChanged);
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => widget.builder(context, _value);

  void _handleSnapshotChanged() {
    _syncSelectedValue();
  }

  void _syncSelectedValue() {
    final nextValue = widget.selector(widget.listenable.value);
    if (nextValue == _value) {
      return;
    }
    setState(() {
      _value = nextValue;
    });
  }
}
