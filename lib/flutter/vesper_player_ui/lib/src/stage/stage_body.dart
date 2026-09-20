part of 'vesper_player_stage.dart';

class _VesperPlayerStageState extends State<VesperPlayerStage> {
  VesperPlayerStageSkin get _skin =>
      widget.skin ?? const VesperPlayerStageSkin();

  Size? _stageSize;
  int _interactionRevision = 0;
  int _deviceGestureGeneration = 0;
  VesperPlayerController? _speedGestureController;

  bool get _compact => widget.controlLayout == VesperStageControlLayout.compact;
  Timer? _controlsTimer;
  Timer? _gestureFeedbackTimer;
  bool _controlsVisible = true;
  double? _pendingSeekRatio;
  _StageAreaGestureKind? _stageGestureKind;
  _StageGestureFeedback? _gestureFeedback;
  double? _deviceGestureBaseRatio;
  double? _stageSeekRatio;
  double? _speedGestureRestoreRate;
  double _stageGestureStartX = 0;
  double _stageGestureDragDx = 0;
  double _deviceGestureDragDy = 0;
  bool _deviceGestureSetInFlight = false;
  bool _deviceGestureSetQueued = false;

  @override
  void initState() {
    super.initState();
    _syncAutoHide();
  }

  @override
  void didUpdateWidget(covariant VesperPlayerStage oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.controller != widget.controller ||
        oldWidget.controlLayout != widget.controlLayout ||
        oldWidget.isFullscreen != widget.isFullscreen) {
      _cancelInteraction();
    }
    final playbackChanged =
        oldWidget.snapshot.playbackState != widget.snapshot.playbackState;
    final bufferingChanged =
        oldWidget.snapshot.isBuffering != widget.snapshot.isBuffering;
    final sheetChanged = oldWidget.sheetOpen != widget.sheetOpen;
    final keepControlsVisibleChanged =
        oldWidget.keepControlsVisible != widget.keepControlsVisible;
    final pictureInPicturePresentationChanged =
        oldWidget.pictureInPicturePresentation !=
            widget.pictureInPicturePresentation;

    if (pictureInPicturePresentationChanged &&
        widget.pictureInPicturePresentation) {
      _enterPictureInPicturePresentation();
    }

    if (!widget.pictureInPicturePresentation &&
        ((sheetChanged && widget.sheetOpen) ||
            (keepControlsVisibleChanged && widget.keepControlsVisible))) {
      _controlsVisible = true;
    }

    if (playbackChanged ||
        bufferingChanged ||
        sheetChanged ||
        keepControlsVisibleChanged ||
        pictureInPicturePresentationChanged) {
      _syncAutoHide();
    }
  }

  @override
  void dispose() {
    _endTemporarySpeedGesture();
    _controlsTimer?.cancel();
    _gestureFeedbackTimer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return VesperPlayerStageTheme(
        skin: _skin,
        child: LayoutBuilder(builder: (context, constraints) {
          final size = constraints.biggest;
          if (_stageSize != null && _stageSize != size) _cancelInteraction();
          _stageSize = size;
          return _buildStage(context);
        }));
  }

  Widget _buildStage(BuildContext context) {
    final snapshot = widget.snapshot;
    final timeline = snapshot.timeline;
    final pictureInPicturePresentation = widget.pictureInPicturePresentation;
    final displayedRatio =
        (_pendingSeekRatio ?? timeline.displayedRatio ?? 0.0).clamp(0.0, 1.0);
    final showControls = !pictureInPicturePresentation &&
        (_controlsVisible ||
            snapshot.playbackState != VesperPlaybackState.playing ||
            widget.sheetOpen ||
            widget.keepControlsVisible);
    final title =
        snapshot.sourceLabel.isEmpty ? snapshot.title : snapshot.sourceLabel;

    return ClipRect(
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: _skin.colors.background,
          border: _compact
              ? Border.all(
                  color: _skin.colors.foreground
                      .withValues(alpha: _skin.colors.foreground.a * 0.08))
              : null,
        ),
        child: Stack(
          fit: StackFit.expand,
          children: <Widget>[
            Positioned.fill(
              child: VesperPlayerView(
                controller: widget.controller,
                onGeometryChanged: widget.onGeometryChanged,
              ),
            ),
            if (!pictureInPicturePresentation && widget.contentOverlay != null)
              Positioned.fill(
                child: IgnorePointer(
                  child: ExcludeSemantics(
                    child: RepaintBoundary(
                      child: widget.contentOverlay!,
                    ),
                  ),
                ),
              ),
            if (!pictureInPicturePresentation)
              _buildStageGestureLayer(showControls: showControls),
            if (!pictureInPicturePresentation)
              IgnorePointer(
                ignoring: true,
                child: AnimatedOpacity(
                  duration: const Duration(milliseconds: 180),
                  opacity: showControls ? 1 : 0,
                  child: DecoratedBox(
                    decoration: BoxDecoration(
                      gradient: LinearGradient(
                        begin: Alignment.topCenter,
                        end: Alignment.bottomCenter,
                        colors: <Color>[
                          _skin.colors.scrim
                              .withValues(alpha: _skin.colors.scrim.a * 0.68),
                          Colors.transparent,
                          Colors.transparent,
                          _skin.colors.scrim
                              .withValues(alpha: _skin.colors.scrim.a * 0.82),
                        ],
                      ),
                    ),
                  ),
                ),
              ),
            if (!pictureInPicturePresentation)
              IgnorePointer(
                ignoring: !showControls,
                child: AnimatedOpacity(
                  duration: const Duration(milliseconds: 180),
                  opacity: showControls ? 1 : 0,
                  child: Stack(
                    fit: StackFit.expand,
                    children: <Widget>[
                      Positioned(
                        top: 16,
                        left: 18,
                        right: 18,
                        child: _buildTopBar(context, snapshot, title),
                      ),
                      Positioned(
                        left: _compact ? 18 : 12,
                        right: _compact ? 18 : 12,
                        bottom: _compact ? 18 : 14,
                        child: _compact
                            ? _buildCompactTimeline(
                                context,
                                snapshot,
                                displayedRatio,
                              )
                            : _buildExpandedTimeline(
                                context,
                                snapshot,
                                displayedRatio,
                              ),
                      ),
                    ],
                  ),
                ),
              ),
            if (!pictureInPicturePresentation && _gestureFeedback != null)
              Positioned.fill(
                child: IgnorePointer(
                  child: Center(
                    child: AnimatedSwitcher(
                      duration: const Duration(milliseconds: 160),
                      child: _StageGestureFeedbackView(
                        key: ValueKey<_StageGestureKind>(
                          _gestureFeedback!.kind,
                        ),
                        feedback: _gestureFeedback!,
                      ),
                    ),
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }

  Widget _buildStageGestureLayer({required bool showControls}) {
    return Positioned.fill(
      child: LayoutBuilder(
        builder: (context, constraints) {
          // Keep the control bar out of the stage gesture arena. The values
          // include the bottom inset and a small hit-test buffer around the
          // rendered timeline/buttons.
          final metrics = _skin.metrics;
          final buttonHeight = math.max(
              48.0,
              _compact
                  ? math.max(
                      metrics.compact.size, metrics.compactFullscreen.size)
                  : math.max(
                      metrics.expanded.size, metrics.expandedFullscreen.size));
          final timelineHeight = math.max(
              22.0,
              math.max(metrics.timelineThumbSize, metrics.timelineTrackHeight) +
                  8);
          final reservedHeight = !showControls
              ? 0.0
              : _compact
                  ? 26 + math.max(buttonHeight, timelineHeight)
                  : 112 + (buttonHeight - 48) + (timelineHeight - 22);
          final gestureHeight = (constraints.maxHeight - reservedHeight)
              .clamp(0.0, constraints.maxHeight)
              .toDouble();

          return Align(
            alignment: Alignment.topCenter,
            child: SizedBox(
              width: constraints.maxWidth,
              height: gestureHeight,
              child: GestureDetector(
                key: ValueKey(_interactionRevision),
                behavior: HitTestBehavior.opaque,
                onTapUp: _handleContentTapUp,
                onDoubleTap: _togglePause,
                onLongPressStart: (_) => _startTemporarySpeedGesture(),
                onLongPressEnd: (_) => _endTemporarySpeedGesture(),
                onLongPressCancel: _endTemporarySpeedGesture,
                onPanStart: _handleStagePanStart,
                onPanUpdate: _handleStagePanUpdate,
                onPanEnd: _handleStagePanEnd,
                onPanCancel: _handleStagePanCancel,
                child: const SizedBox.expand(),
              ),
            ),
          );
        },
      ),
    );
  }

  Widget _buildTopBar(
    BuildContext context,
    VesperPlayerSnapshot snapshot,
    String title,
  ) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        if (widget.onNavigateBack != null) ...<Widget>[
          VesperStageIconButton(
            icon: const VesperStageIcon(VesperStageIconRole.navigateBack),
            label:
                widget.navigateBackSemanticLabel ?? widget.strings.navigateBack,
            variant: VesperStageButtonVariant.navigation,
            onPressed: widget.onNavigateBack!,
          ),
          SizedBox(width: _skin.metrics.buttonSpacing),
        ],
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              Row(
                children: <Widget>[
                  Expanded(
                    child: Text(
                      title,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: Theme.of(context).textTheme.titleMedium?.copyWith(
                            color: _skin.colors.foreground,
                            fontWeight: FontWeight.bold,
                          ),
                    ),
                  ),
                  if (snapshot.isBuffering) ...<Widget>[
                    SizedBox(width: _skin.metrics.buttonSpacing),
                    VesperStageChip(
                      label: widget.strings.buffering,
                      accent: _skin.colors.accent,
                      compact: true,
                    ),
                  ],
                ],
              ),
              const SizedBox(height: 4),
              Text(
                stageBadgeText(snapshot.timeline, strings: widget.strings),
                style: Theme.of(
                  context,
                )
                    .textTheme
                    .bodySmall
                    ?.copyWith(color: _skin.colors.secondaryForeground),
              ),
            ],
          ),
        ),
        const SizedBox(width: 10),
        if (widget.topBarPrimaryAction != null) ...<Widget>[
          widget.topBarPrimaryAction!,
          const SizedBox(width: 4),
        ],
        widget.topBarSecondaryAction ?? _defaultMenuAction(),
      ],
    );
  }

  Widget _defaultMenuAction() {
    return VesperStageIconButton(
      icon: const VesperStageIcon(VesperStageIconRole.more),
      label: widget.strings.more,
      variant: VesperStageButtonVariant.toolbar,
      onPressed: () => widget.onOpenSheet(VesperPlayerStageSheet.menu),
    );
  }

  Widget _buildCompactTimeline(
    BuildContext context,
    VesperPlayerSnapshot snapshot,
    double displayedRatio,
  ) {
    final isPlaying = snapshot.playbackState == VesperPlaybackState.playing;
    return Row(
      crossAxisAlignment: CrossAxisAlignment.center,
      children: <Widget>[
        VesperStageIconButton(
          icon: VesperStageIcon(
              isPlaying ? VesperStageIconRole.pause : VesperStageIconRole.play),
          label: isPlaying ? widget.strings.pause : widget.strings.play,
          variant: VesperStageButtonVariant.compact,
          onPressed: _togglePause,
        ),
        SizedBox(width: _skin.metrics.buttonSpacing),
        Expanded(
          child: VesperTimelineScrubber(
            key: ValueKey(_interactionRevision),
            displayedRatio: displayedRatio,
            compact: true,
            enabled: snapshot.timeline.isSeekable,
            onSeekPreview: _handleSeekPreview,
            onSeekCommit: _handleSeekCommit,
            onSeekCancel: _handleSeekCancel,
          ),
        ),
        SizedBox(width: _skin.metrics.buttonSpacing),
        Text(
          compactTimelineSummary(
            snapshot.timeline,
            _pendingSeekRatio,
            strings: widget.strings,
          ),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: Theme.of(context).textTheme.labelSmall?.copyWith(
            color: _skin.colors.foreground,
            fontFeatures: const <FontFeature>[FontFeature.tabularFigures()],
          ),
        ),
        if (snapshot.timeline.kind == VesperTimelineKind.liveDvr) ...<Widget>[
          SizedBox(width: _skin.metrics.buttonSpacing),
          VesperStagePillButton(
            label: liveButtonLabel(snapshot.timeline, strings: widget.strings),
            compact: true,
            onPressed: _seekToLiveEdge,
          ),
        ],
        SizedBox(width: _skin.metrics.buttonSpacing),
        VesperStageIconButton(
          icon: VesperStageIcon(widget.isFullscreen
              ? VesperStageIconRole.exitFullscreen
              : VesperStageIconRole.fullscreen),
          label: widget.isFullscreen
              ? widget.strings.exitFullscreen
              : widget.strings.fullscreen,
          variant: VesperStageButtonVariant.compactFullscreen,
          onPressed: widget.onToggleFullscreen,
        ),
      ],
    );
  }

  Widget _buildExpandedTimeline(
    BuildContext context,
    VesperPlayerSnapshot snapshot,
    double displayedRatio,
  ) {
    final isPlaying = snapshot.playbackState == VesperPlaybackState.playing;
    final qualityLabelText = qualityButtonLabel(
      snapshot.trackCatalog,
      snapshot.trackSelection,
      effectiveVideoTrackId: snapshot.effectiveVideoTrackId,
      fixedTrackStatus: snapshot.fixedTrackStatus,
      strings: widget.strings,
    );

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        Text(
          timelineSummary(
            snapshot.timeline,
            _pendingSeekRatio,
            strings: widget.strings,
          ),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: Theme.of(context).textTheme.labelLarge?.copyWith(
            color: _skin.colors.foreground,
            fontFeatures: const <FontFeature>[FontFeature.tabularFigures()],
          ),
        ),
        const SizedBox(height: 4),
        VesperTimelineScrubber(
          key: ValueKey(_interactionRevision),
          displayedRatio: displayedRatio,
          compact: true,
          enabled: snapshot.timeline.isSeekable,
          onSeekPreview: _handleSeekPreview,
          onSeekCommit: _handleSeekCommit,
          onSeekCancel: _handleSeekCancel,
        ),
        const SizedBox(height: 4),
        Row(
          children: <Widget>[
            VesperStageIconButton(
              icon: VesperStageIcon(isPlaying
                  ? VesperStageIconRole.pause
                  : VesperStageIconRole.play),
              label: isPlaying ? widget.strings.pause : widget.strings.play,
              variant: VesperStageButtonVariant.expanded,
              onPressed: _togglePause,
            ),
            if (widget.expandedControlBarLeading != null)
              widget.expandedControlBarLeading!,
            const Spacer(),
            if (snapshot.timeline.kind ==
                VesperTimelineKind.liveDvr) ...<Widget>[
              VesperStagePillButton(
                label:
                    liveButtonLabel(snapshot.timeline, strings: widget.strings),
                compact: true,
                onPressed: _seekToLiveEdge,
              ),
              SizedBox(width: _skin.metrics.buttonSpacing),
            ],
            VesperStagePillButton(
              label: speedBadge(snapshot.playbackRate),
              compact: true,
              onPressed: () => widget.onOpenSheet(VesperPlayerStageSheet.speed),
            ),
            SizedBox(width: _skin.metrics.buttonSpacing),
            VesperStagePillButton(
              label: qualityLabelText,
              compact: true,
              onPressed: () =>
                  widget.onOpenSheet(VesperPlayerStageSheet.quality),
            ),
            SizedBox(width: _skin.metrics.buttonSpacing),
            VesperStageIconButton(
              icon: VesperStageIcon(widget.isFullscreen
                  ? VesperStageIconRole.exitFullscreen
                  : VesperStageIconRole.fullscreen),
              label: widget.isFullscreen
                  ? widget.strings.exitFullscreen
                  : widget.strings.fullscreen,
              variant: VesperStageButtonVariant.expandedFullscreen,
              onPressed: widget.onToggleFullscreen,
            ),
          ],
        ),
      ],
    );
  }

  void _handleSeekPreview(double ratio) {
    if (widget.pictureInPicturePresentation) {
      return;
    }
    setState(() {
      _pendingSeekRatio = ratio;
    });
    _showControls();
  }

  void _handleSeekCommit(double ratio) {
    if (!mounted || widget.pictureInPicturePresentation) {
      return;
    }
    setState(() {
      _pendingSeekRatio = null;
    });
    _reportControllerCall(
        widget.controller.seekToRatio(ratio), 'seek to ratio');
    _showControls();
  }

  void _handleSeekCancel() {
    if (!mounted || widget.pictureInPicturePresentation) {
      return;
    }
    setState(() {
      _pendingSeekRatio = null;
    });
    _syncAutoHide();
  }

  void _handleContentTapUp(TapUpDetails details) {
    if (!mounted || widget.pictureInPicturePresentation) {
      return;
    }
    // Flutter synthesizes accessibility taps at the gesture area's center.
    // That is not a content hit position; keep the existing controls action.
    if (details.kind == PointerDeviceKind.unknown) {
      _handleTap();
      return;
    }
    // The gesture area is aligned with the complete Stage's top-left corner.
    if (widget.onContentTap?.call(details.localPosition) != true) {
      _handleTap();
    }
  }

  void _handleTap() {
    if (!mounted || widget.pictureInPicturePresentation) {
      return;
    }
    if (widget.keepControlsVisible) {
      _showControls();
      return;
    }
    setState(() {
      _controlsVisible = !_controlsVisible;
    });
    _syncAutoHide();
  }

  void _togglePause() {
    if (widget.pictureInPicturePresentation) {
      return;
    }
    _reportControllerCall(widget.controller.togglePause(), 'toggle pause');
    _showControls();
  }

  void _seekToLiveEdge() {
    if (widget.pictureInPicturePresentation) {
      return;
    }
    _reportControllerCall(
        widget.controller.seekToLiveEdge(), 'seek to live edge');
    _showControls();
  }

  void _handleStagePanStart(DragStartDetails details) {
    if (widget.pictureInPicturePresentation) {
      return;
    }
    _stageGestureKind = null;
    _deviceGestureBaseRatio = null;
    _stageGestureStartX = details.localPosition.dx;
    _stageGestureDragDx = 0;
    _deviceGestureDragDy = 0;
    _stageSeekRatio = null;
  }

  void _handleStagePanUpdate(DragUpdateDetails details) {
    if (widget.pictureInPicturePresentation) {
      return;
    }
    _stageGestureDragDx += details.delta.dx;
    _deviceGestureDragDy += details.delta.dy;

    if (_stageGestureKind == null) {
      final horizontalDistance = _stageGestureDragDx.abs();
      final verticalDistance = _deviceGestureDragDy.abs();
      if (horizontalDistance < 8 && verticalDistance < 8) {
        return;
      }

      if (horizontalDistance >= verticalDistance * 1.15) {
        if (!widget.snapshot.timeline.isSeekable) {
          _stageGestureKind = _StageAreaGestureKind.ignored;
          return;
        }
        _stageGestureKind = _StageAreaGestureKind.seek;
      } else if (verticalDistance >= horizontalDistance * 1.15) {
        final width =
            (context.size?.width ?? 1.0).clamp(1.0, double.infinity).toDouble();
        final kind = _stageGestureStartX < width / 2
            ? _StageAreaGestureKind.brightness
            : _StageAreaGestureKind.volume;
        if (widget.deviceControls == null) {
          _debugLogDeviceGestureUnavailable(kind, 'deviceControls is null');
          _stageGestureKind = _StageAreaGestureKind.ignored;
          return;
        }
        _stageGestureKind = kind;
        _reportControllerCall(
          _loadDeviceGestureBaseRatio(kind),
          'load device gesture base ratio',
        );
      } else {
        return;
      }
    }

    final kind = _stageGestureKind;
    if (kind == _StageAreaGestureKind.ignored || kind == null) {
      return;
    }
    if (kind == _StageAreaGestureKind.seek) {
      _updateStageSeekRatio(details.localPosition.dx);
      return;
    }

    _showControls();
    _scheduleDeviceGestureSet();
  }

  void _handleStagePanEnd(DragEndDetails _) {
    if (widget.pictureInPicturePresentation) {
      return;
    }
    final targetRatio = _stageSeekRatio;
    if (_stageGestureKind == _StageAreaGestureKind.seek &&
        targetRatio != null) {
      _stageSeekRatio = null;
      _handleSeekCommit(targetRatio);
    } else if (_stageGestureKind == _StageAreaGestureKind.seek) {
      _handleSeekCancel();
    }
    _resetStageGesture();
  }

  void _handleStagePanCancel() {
    if (widget.pictureInPicturePresentation) {
      return;
    }
    if (_stageGestureKind == _StageAreaGestureKind.seek) {
      _handleSeekCancel();
    }
    _resetStageGesture();
  }

  Future<void> _loadDeviceGestureBaseRatio(_StageAreaGestureKind kind) async {
    if (widget.pictureInPicturePresentation) {
      return;
    }
    final controls = widget.deviceControls;
    if (controls == null) {
      return;
    }
    final generation = _deviceGestureGeneration;
    final ratio = switch (kind) {
      _StageAreaGestureKind.brightness =>
        await controls.currentBrightnessRatio(),
      _StageAreaGestureKind.volume => await controls.currentVolumeRatio(),
      _StageAreaGestureKind.seek || _StageAreaGestureKind.ignored => null,
    };
    if (!mounted ||
        generation != _deviceGestureGeneration ||
        _stageGestureKind != kind) {
      return;
    }
    if (ratio == null) {
      _debugLogDeviceGestureUnavailable(kind, 'current ratio returned null');
      return;
    }
    _deviceGestureBaseRatio = ratio.clamp(0.0, 1.0).toDouble();
    _scheduleDeviceGestureSet();
  }

  void _scheduleDeviceGestureSet() {
    if (widget.pictureInPicturePresentation) {
      return;
    }
    if (_deviceGestureBaseRatio == null ||
        _stageGestureKind == null ||
        _stageGestureKind == _StageAreaGestureKind.seek ||
        _stageGestureKind == _StageAreaGestureKind.ignored) {
      return;
    }
    if (_deviceGestureSetInFlight) {
      _deviceGestureSetQueued = true;
      return;
    }
    _reportControllerCall(_applyDeviceGestureRatio(), 'apply device gesture');
  }

  Future<void> _applyDeviceGestureRatio() async {
    if (!mounted ||
        _deviceGestureSetInFlight ||
        widget.pictureInPicturePresentation) {
      return;
    }
    _deviceGestureSetInFlight = true;
    try {
      do {
        _deviceGestureSetQueued = false;
        if (!mounted) {
          return;
        }
        final controls = widget.deviceControls;
        final kind = _stageGestureKind;
        final baseRatio = _deviceGestureBaseRatio;
        if (controls == null ||
            kind == null ||
            baseRatio == null ||
            kind == _StageAreaGestureKind.seek ||
            kind == _StageAreaGestureKind.ignored) {
          return;
        }

        final height = (context.size?.height ?? 1.0)
            .clamp(1.0, double.infinity)
            .toDouble();
        final requestedRatio =
            (baseRatio - _deviceGestureDragDy / height * 1.15)
                .clamp(0.0, 1.0)
                .toDouble();
        final generation = _deviceGestureGeneration;
        final actualRatio = switch (kind) {
          _StageAreaGestureKind.brightness => await controls.setBrightnessRatio(
              requestedRatio,
            ),
          _StageAreaGestureKind.volume => await controls.setVolumeRatio(
              requestedRatio,
            ),
          _StageAreaGestureKind.seek || _StageAreaGestureKind.ignored => null,
        };
        if (!mounted ||
            generation != _deviceGestureGeneration ||
            _stageGestureKind != kind) {
          continue;
        }
        if (actualRatio == null) {
          _debugLogDeviceGestureUnavailable(kind, 'set ratio returned null');
          continue;
        }
        final value = actualRatio.clamp(0.0, 1.0).toDouble();
        _showGestureFeedback(
          _StageGestureFeedback(
            kind: switch (kind) {
              _StageAreaGestureKind.brightness => _StageGestureKind.brightness,
              _StageAreaGestureKind.volume => _StageGestureKind.volume,
              _StageAreaGestureKind.seek ||
              _StageAreaGestureKind.ignored =>
                _StageGestureKind.speed,
            },
            progress: value,
            label: _percentLabel(value),
          ),
        );
      } while (_deviceGestureSetQueued);
    } finally {
      _deviceGestureSetInFlight = false;
    }
  }

  void _startTemporarySpeedGesture() {
    if (!mounted || widget.pictureInPicturePresentation) {
      return;
    }
    _resetStageGesture();
    _speedGestureController ??= widget.controller;
    _speedGestureRestoreRate ??= widget.snapshot.playbackRate;
    _reportControllerCall(
      widget.controller.setPlaybackRate(2.0),
      'start temporary speed gesture',
    );
    _showGestureFeedback(
      _StageGestureFeedback(
        kind: _StageGestureKind.speed,
        progress: null,
        label: speedBadge(2.0),
      ),
    );
    _showControls();
  }

  void _endTemporarySpeedGesture() {
    final restoreRate = _speedGestureRestoreRate;
    if (restoreRate == null) {
      return;
    }
    final controller = _speedGestureController!;
    _speedGestureRestoreRate = null;
    _speedGestureController = null;
    _reportControllerCall(
      controller.setPlaybackRate(restoreRate),
      'end temporary speed gesture',
    );
  }

  void _showGestureFeedback(_StageGestureFeedback feedback) {
    if (!mounted || widget.pictureInPicturePresentation) {
      return;
    }
    setState(() {
      _gestureFeedback = feedback;
    });
    _gestureFeedbackTimer?.cancel();
    _gestureFeedbackTimer = Timer(const Duration(milliseconds: 520), () {
      if (!mounted) {
        return;
      }
      setState(() {
        _gestureFeedback = null;
      });
    });
  }

  void _resetStageGesture() {
    _deviceGestureGeneration++;
    _stageGestureKind = null;
    _deviceGestureBaseRatio = null;
    _stageGestureStartX = 0;
    _stageGestureDragDx = 0;
    _deviceGestureDragDy = 0;
    _stageSeekRatio = null;
  }

  void _debugLogDeviceGestureUnavailable(
    _StageAreaGestureKind kind,
    String reason,
  ) {
    assert(() {
      debugPrint('VesperPlayerStage ${kind.name} gesture ignored: $reason.');
      return true;
    }());
  }

  void _updateStageSeekRatio(double dx) {
    if (widget.pictureInPicturePresentation) {
      return;
    }
    final width =
        (context.size?.width ?? 1.0).clamp(1.0, double.infinity).toDouble();
    final targetRatio = (dx / width).clamp(0.0, 1.0).toDouble();
    _stageSeekRatio = targetRatio;
    setState(() {
      _pendingSeekRatio = targetRatio;
    });
    _showControls();
  }

  void _showControls() {
    if (!mounted || widget.pictureInPicturePresentation) {
      return;
    }
    if (!_controlsVisible) {
      setState(() {
        _controlsVisible = true;
      });
    }
    _syncAutoHide();
  }

  void _syncAutoHide() {
    _controlsTimer?.cancel();
    if (!mounted) {
      return;
    }
    if (widget.pictureInPicturePresentation) {
      return;
    }
    final snapshot = widget.snapshot;
    final shouldAutoHide =
        snapshot.playbackState == VesperPlaybackState.playing &&
            !snapshot.isBuffering &&
            _controlsVisible &&
            !widget.sheetOpen &&
            !widget.keepControlsVisible &&
            _pendingSeekRatio == null;

    if (!shouldAutoHide) {
      return;
    }

    _controlsTimer = Timer(const Duration(seconds: 3), () {
      if (!mounted) {
        return;
      }
      if (widget.snapshot.playbackState != VesperPlaybackState.playing ||
          widget.snapshot.isBuffering ||
          widget.sheetOpen ||
          widget.keepControlsVisible ||
          _pendingSeekRatio != null) {
        return;
      }
      setState(() {
        _controlsVisible = false;
      });
    });
  }

  void _reportControllerCall(Future<void> future, String context) {
    unawaited(
      future.catchError((Object error, StackTrace stackTrace) {
        FlutterError.reportError(
          FlutterErrorDetails(
            exception: error,
            stack: stackTrace,
            library: 'vesper_player_ui',
            context: ErrorDescription(context),
          ),
        );
      }),
    );
  }

  // Called during rebuilds as well as presentation changes. Do not setState here.
  void _cancelInteraction() {
    _interactionRevision++;
    _endTemporarySpeedGesture();
    _resetStageGesture();
    _deviceGestureSetQueued = false;
    _pendingSeekRatio = null;
    _gestureFeedback = null;
    _gestureFeedbackTimer?.cancel();
    _syncAutoHide();
  }

  void _enterPictureInPicturePresentation() {
    _controlsTimer?.cancel();
    _gestureFeedbackTimer?.cancel();
    _endTemporarySpeedGesture();
    _resetStageGesture();
    _deviceGestureSetQueued = false;
    if (!mounted) {
      return;
    }
    setState(() {
      _controlsVisible = false;
      _pendingSeekRatio = null;
      _gestureFeedback = null;
    });
  }
}
