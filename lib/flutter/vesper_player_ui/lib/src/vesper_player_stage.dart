import 'dart:async';

import 'package:flutter/material.dart';
import 'package:vesper_player/vesper_player.dart';

import 'stage_device_controls.dart';
import 'stage_helpers.dart';
import 'stage_models.dart';

class VesperPlayerStage extends StatefulWidget {
  const VesperPlayerStage({
    super.key,
    required this.controller,
    required this.snapshot,
    required this.isPortrait,
    required this.onOpenSheet,
    required this.onToggleFullscreen,
    this.sheetOpen = false,
    this.deviceControls,
  });

  final VesperPlayerController controller;
  final VesperPlayerSnapshot snapshot;
  final bool isPortrait;
  final bool sheetOpen;
  final VesperPlayerDeviceControls? deviceControls;
  final ValueChanged<VesperPlayerStageSheet> onOpenSheet;
  final VoidCallback onToggleFullscreen;

  @override
  State<VesperPlayerStage> createState() => _VesperPlayerStageState();
}

class _VesperPlayerStageState extends State<VesperPlayerStage> {
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
    final playbackChanged =
        oldWidget.snapshot.playbackState != widget.snapshot.playbackState;
    final bufferingChanged =
        oldWidget.snapshot.isBuffering != widget.snapshot.isBuffering;
    final sheetChanged = oldWidget.sheetOpen != widget.sheetOpen;

    if (sheetChanged && widget.sheetOpen) {
      _showControls();
    }

    if (playbackChanged || bufferingChanged || sheetChanged) {
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
    final snapshot = widget.snapshot;
    final timeline = snapshot.timeline;
    final displayedRatio = (_pendingSeekRatio ?? timeline.displayedRatio ?? 0.0)
        .clamp(0.0, 1.0);
    final showControls =
        _controlsVisible ||
        snapshot.playbackState != VesperPlaybackState.playing ||
        widget.sheetOpen;
    final stageRadius = BorderRadius.circular(widget.isPortrait ? 20 : 0);
    final title = snapshot.sourceLabel.isEmpty
        ? snapshot.title
        : snapshot.sourceLabel;

    return ClipRRect(
      borderRadius: stageRadius,
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: Colors.black,
          border: widget.isPortrait
              ? Border.all(color: Colors.white.withValues(alpha: 0.08))
              : null,
        ),
        child: Stack(
          fit: StackFit.expand,
          children: <Widget>[
            Positioned.fill(
              child: VesperPlayerView(controller: widget.controller),
            ),
            Positioned.fill(
              child: GestureDetector(
                behavior: HitTestBehavior.opaque,
                onTap: _handleTap,
                onDoubleTap: _togglePause,
                onLongPressStart: (_) => _startTemporarySpeedGesture(),
                onLongPressEnd: (_) => _endTemporarySpeedGesture(),
                onLongPressCancel: _endTemporarySpeedGesture,
                onPanStart: _handleStagePanStart,
                onPanUpdate: _handleStagePanUpdate,
                onPanEnd: _handleStagePanEnd,
                onPanCancel: _handleStagePanCancel,
              ),
            ),
            IgnorePointer(
              ignoring: !showControls,
              child: AnimatedOpacity(
                duration: const Duration(milliseconds: 180),
                opacity: showControls ? 1 : 0,
                child: DecoratedBox(
                  decoration: BoxDecoration(
                    gradient: LinearGradient(
                      begin: Alignment.topCenter,
                      end: Alignment.bottomCenter,
                      colors: <Color>[
                        Colors.black.withValues(alpha: 0.68),
                        Colors.transparent,
                        Colors.transparent,
                        Colors.black.withValues(alpha: 0.82),
                      ],
                    ),
                  ),
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
                        left: widget.isPortrait ? 18 : 12,
                        right: widget.isPortrait ? 18 : 12,
                        bottom: widget.isPortrait ? 18 : 14,
                        child: widget.isPortrait
                            ? _buildPortraitTimeline(
                                context,
                                snapshot,
                                displayedRatio,
                              )
                            : _buildLandscapeTimeline(
                                context,
                                snapshot,
                                displayedRatio,
                              ),
                      ),
                    ],
                  ),
                ),
              ),
            ),
            if (_gestureFeedback != null)
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

  Widget _buildTopBar(
    BuildContext context,
    VesperPlayerSnapshot snapshot,
    String title,
  ) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
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
                        color: Colors.white,
                        fontWeight: FontWeight.bold,
                      ),
                    ),
                  ),
                  if (snapshot.isBuffering) ...<Widget>[
                    const SizedBox(width: 8),
                    const VesperStageChip(
                      label: '缓冲中',
                      accent: Color(0xFFFFB454),
                      compact: true,
                    ),
                  ],
                ],
              ),
              const SizedBox(height: 4),
              Text(
                stageBadgeText(snapshot.timeline),
                style: Theme.of(
                  context,
                ).textTheme.bodySmall?.copyWith(color: const Color(0xFFBFC6D6)),
              ),
            ],
          ),
        ),
        const SizedBox(width: 10),
        VesperStageIconButton(
          icon: Icons.more_vert_rounded,
          label: '更多',
          size: 38,
          iconSize: 24,
          containerAlpha: 0,
          onPressed: () => widget.onOpenSheet(VesperPlayerStageSheet.menu),
        ),
      ],
    );
  }

  Widget _buildPortraitTimeline(
    BuildContext context,
    VesperPlayerSnapshot snapshot,
    double displayedRatio,
  ) {
    final isPlaying = snapshot.playbackState == VesperPlaybackState.playing;
    return Row(
      crossAxisAlignment: CrossAxisAlignment.center,
      children: <Widget>[
        VesperStageIconButton(
          icon: isPlaying ? Icons.pause_rounded : Icons.play_arrow_rounded,
          label: isPlaying ? '暂停' : '播放',
          size: 38,
          iconSize: 24,
          containerAlpha: 0,
          onPressed: _togglePause,
        ),
        const SizedBox(width: 8),
        Expanded(
          child: VesperTimelineScrubber(
            displayedRatio: displayedRatio,
            compact: true,
            enabled: snapshot.timeline.isSeekable,
            onSeekPreview: _handleSeekPreview,
            onSeekCommit: _handleSeekCommit,
            onSeekCancel: _handleSeekCancel,
          ),
        ),
        const SizedBox(width: 8),
        Text(
          compactTimelineSummary(snapshot.timeline, _pendingSeekRatio),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: Theme.of(context).textTheme.labelSmall?.copyWith(
            color: const Color(0xFFF7F8FC),
            fontFeatures: const <FontFeature>[FontFeature.tabularFigures()],
          ),
        ),
        if (snapshot.timeline.kind == VesperTimelineKind.liveDvr) ...<Widget>[
          const SizedBox(width: 8),
          VesperStagePillButton(
            label: liveButtonLabel(snapshot.timeline),
            compact: true,
            onPressed: _seekToLiveEdge,
          ),
        ],
        const SizedBox(width: 6),
        VesperStageIconButton(
          icon: Icons.fullscreen_rounded,
          label: '全屏',
          size: 38,
          iconSize: 24,
          containerAlpha: 0,
          onPressed: widget.onToggleFullscreen,
        ),
      ],
    );
  }

  Widget _buildLandscapeTimeline(
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
    );

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        Text(
          timelineSummary(snapshot.timeline, _pendingSeekRatio),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: Theme.of(context).textTheme.labelLarge?.copyWith(
            color: const Color(0xFFF7F8FC),
            fontFeatures: const <FontFeature>[FontFeature.tabularFigures()],
          ),
        ),
        const SizedBox(height: 4),
        VesperTimelineScrubber(
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
              icon: isPlaying ? Icons.pause_rounded : Icons.play_arrow_rounded,
              label: isPlaying ? '暂停' : '播放',
              size: 38,
              iconSize: 22,
              containerAlpha: 0,
              onPressed: _togglePause,
            ),
            const Spacer(),
            if (snapshot.timeline.kind ==
                VesperTimelineKind.liveDvr) ...<Widget>[
              VesperStagePillButton(
                label: liveButtonLabel(snapshot.timeline),
                compact: true,
                onPressed: _seekToLiveEdge,
              ),
              const SizedBox(width: 8),
            ],
            VesperStagePillButton(
              label: speedBadge(snapshot.playbackRate),
              compact: true,
              onPressed: () => widget.onOpenSheet(VesperPlayerStageSheet.speed),
            ),
            const SizedBox(width: 8),
            VesperStagePillButton(
              label: qualityLabelText,
              compact: true,
              onPressed: () =>
                  widget.onOpenSheet(VesperPlayerStageSheet.quality),
            ),
            const SizedBox(width: 6),
            VesperStageIconButton(
              icon: Icons.fullscreen_exit_rounded,
              label: '退出全屏',
              size: 34,
              iconSize: 19,
              containerAlpha: 0,
              onPressed: widget.onToggleFullscreen,
            ),
          ],
        ),
      ],
    );
  }

  void _handleSeekPreview(double ratio) {
    setState(() {
      _pendingSeekRatio = ratio;
    });
    _showControls();
  }

  void _handleSeekCommit(double ratio) {
    setState(() {
      _pendingSeekRatio = null;
    });
    unawaited(widget.controller.seekToRatio(ratio));
    _showControls();
  }

  void _handleSeekCancel() {
    setState(() {
      _pendingSeekRatio = null;
    });
    _syncAutoHide();
  }

  void _handleTap() {
    setState(() {
      _controlsVisible = !_controlsVisible;
    });
    _syncAutoHide();
  }

  void _togglePause() {
    unawaited(widget.controller.togglePause());
    _showControls();
  }

  void _seekToLiveEdge() {
    unawaited(widget.controller.seekToLiveEdge());
    _showControls();
  }

  void _handleStagePanStart(DragStartDetails details) {
    _stageGestureKind = null;
    _deviceGestureBaseRatio = null;
    _stageGestureStartX = details.localPosition.dx;
    _stageGestureDragDx = 0;
    _deviceGestureDragDy = 0;
    _stageSeekRatio = null;
  }

  void _handleStagePanUpdate(DragUpdateDetails details) {
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
        final width = (context.size?.width ?? 1.0)
            .clamp(1.0, double.infinity)
            .toDouble();
        final kind = _stageGestureStartX < width / 2
            ? _StageAreaGestureKind.brightness
            : _StageAreaGestureKind.volume;
        if (widget.deviceControls == null) {
          _stageGestureKind = _StageAreaGestureKind.ignored;
          return;
        }
        _stageGestureKind = kind;
        unawaited(_loadDeviceGestureBaseRatio(kind));
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
    if (_stageGestureKind == _StageAreaGestureKind.seek) {
      _handleSeekCancel();
    }
    _resetStageGesture();
  }

  Future<void> _loadDeviceGestureBaseRatio(_StageAreaGestureKind kind) async {
    final controls = widget.deviceControls;
    if (controls == null) {
      return;
    }
    final ratio = switch (kind) {
      _StageAreaGestureKind.brightness =>
        await controls.currentBrightnessRatio(),
      _StageAreaGestureKind.volume => await controls.currentVolumeRatio(),
      _StageAreaGestureKind.seek || _StageAreaGestureKind.ignored => null,
    };
    if (!mounted || _stageGestureKind != kind || ratio == null) {
      return;
    }
    _deviceGestureBaseRatio = ratio.clamp(0.0, 1.0).toDouble();
    _scheduleDeviceGestureSet();
  }

  void _scheduleDeviceGestureSet() {
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
    unawaited(_applyDeviceGestureRatio());
  }

  Future<void> _applyDeviceGestureRatio() async {
    if (_deviceGestureSetInFlight) {
      return;
    }
    _deviceGestureSetInFlight = true;
    try {
      do {
        _deviceGestureSetQueued = false;
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
        final actualRatio = switch (kind) {
          _StageAreaGestureKind.brightness => await controls.setBrightnessRatio(
            requestedRatio,
          ),
          _StageAreaGestureKind.volume => await controls.setVolumeRatio(
            requestedRatio,
          ),
          _StageAreaGestureKind.seek || _StageAreaGestureKind.ignored => null,
        };
        if (!mounted || _stageGestureKind != kind || actualRatio == null) {
          continue;
        }
        final value = actualRatio.clamp(0.0, 1.0).toDouble();
        _showGestureFeedback(
          _StageGestureFeedback(
            kind: switch (kind) {
              _StageAreaGestureKind.brightness => _StageGestureKind.brightness,
              _StageAreaGestureKind.volume => _StageGestureKind.volume,
              _StageAreaGestureKind.seek ||
              _StageAreaGestureKind.ignored => _StageGestureKind.speed,
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
    _resetStageGesture();
    _speedGestureRestoreRate ??= widget.snapshot.playbackRate;
    unawaited(widget.controller.setPlaybackRate(2.0));
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
    _speedGestureRestoreRate = null;
    unawaited(widget.controller.setPlaybackRate(restoreRate));
  }

  void _showGestureFeedback(_StageGestureFeedback feedback) {
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
    _stageGestureKind = null;
    _deviceGestureBaseRatio = null;
    _stageGestureStartX = 0;
    _stageGestureDragDx = 0;
    _deviceGestureDragDy = 0;
    _stageSeekRatio = null;
  }

  void _updateStageSeekRatio(double dx) {
    final width = (context.size?.width ?? 1.0)
        .clamp(1.0, double.infinity)
        .toDouble();
    final targetRatio = (dx / width).clamp(0.0, 1.0).toDouble();
    _stageSeekRatio = targetRatio;
    setState(() {
      _pendingSeekRatio = targetRatio;
    });
    _showControls();
  }

  void _showControls() {
    if (!_controlsVisible) {
      setState(() {
        _controlsVisible = true;
      });
    }
    _syncAutoHide();
  }

  void _syncAutoHide() {
    _controlsTimer?.cancel();
    final snapshot = widget.snapshot;
    final shouldAutoHide =
        snapshot.playbackState == VesperPlaybackState.playing &&
        !snapshot.isBuffering &&
        _controlsVisible &&
        !widget.sheetOpen &&
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
          _pendingSeekRatio != null) {
        return;
      }
      setState(() {
        _controlsVisible = false;
      });
    });
  }
}

enum _StageAreaGestureKind { brightness, volume, seek, ignored }

enum _StageGestureKind { brightness, volume, speed }

class _StageGestureFeedback {
  const _StageGestureFeedback({
    required this.kind,
    required this.progress,
    required this.label,
  });

  final _StageGestureKind kind;
  final double? progress;
  final String label;
}

class _StageGestureFeedbackView extends StatelessWidget {
  const _StageGestureFeedbackView({super.key, required this.feedback});

  final _StageGestureFeedback feedback;

  @override
  Widget build(BuildContext context) {
    final icon = switch (feedback.kind) {
      _StageGestureKind.brightness => Icons.wb_sunny_rounded,
      _StageGestureKind.volume => Icons.volume_up_rounded,
      _StageGestureKind.speed => Icons.speed_rounded,
    };
    final progress = feedback.progress?.clamp(0.0, 1.0).toDouble();

    return Container(
      width: progress == null ? null : 226,
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
      decoration: BoxDecoration(
        color: Colors.black.withValues(alpha: 0.72),
        borderRadius: BorderRadius.circular(999),
      ),
      child: Row(
        mainAxisSize: progress == null ? MainAxisSize.min : MainAxisSize.max,
        crossAxisAlignment: CrossAxisAlignment.center,
        children: <Widget>[
          Icon(icon, size: 24, color: Colors.white),
          const SizedBox(width: 10),
          if (progress != null) ...<Widget>[
            Expanded(
              child: ClipRRect(
                borderRadius: BorderRadius.circular(999),
                child: LinearProgressIndicator(
                  minHeight: 4,
                  value: progress,
                  backgroundColor: Colors.white.withValues(alpha: 0.18),
                  valueColor: const AlwaysStoppedAnimation<Color>(Colors.white),
                ),
              ),
            ),
            const SizedBox(width: 8),
          ],
          Text(
            feedback.label,
            style: Theme.of(context).textTheme.labelMedium?.copyWith(
              color: Colors.white,
              fontFeatures: const <FontFeature>[FontFeature.tabularFigures()],
            ),
          ),
        ],
      ),
    );
  }
}

String _percentLabel(double value) => '${(value * 100).round()}%';

class VesperTimelineScrubber extends StatefulWidget {
  const VesperTimelineScrubber({
    super.key,
    required this.displayedRatio,
    required this.onSeekPreview,
    required this.onSeekCommit,
    required this.onSeekCancel,
    this.compact = false,
    this.enabled = true,
  });

  final double displayedRatio;
  final bool compact;
  final bool enabled;
  final ValueChanged<double> onSeekPreview;
  final ValueChanged<double> onSeekCommit;
  final VoidCallback onSeekCancel;

  @override
  State<VesperTimelineScrubber> createState() => _VesperTimelineScrubberState();
}

class _VesperTimelineScrubberState extends State<VesperTimelineScrubber> {
  double? _dragRatio;

  @override
  Widget build(BuildContext context) {
    final knobSize = widget.compact ? 11.0 : 14.0;
    final touchHeight = widget.compact ? 22.0 : 28.0;
    final visualHeight = widget.compact ? 14.0 : 18.0;
    final trackHeight = 4.0;
    final ratio = widget.displayedRatio.clamp(0.0, 1.0);
    final enabled = widget.enabled;
    final inactiveTrackColor = Colors.white.withValues(
      alpha: enabled ? 0.16 : 0.10,
    );
    final activeStart = const Color(
      0xFFFF6B8E,
    ).withValues(alpha: enabled ? 1 : 0.42);
    final activeEnd = const Color(
      0xFFFFB454,
    ).withValues(alpha: enabled ? 1 : 0.42);
    final knobColor = Colors.white.withValues(alpha: enabled ? 1 : 0.42);

    return LayoutBuilder(
      builder: (context, constraints) {
        final width = constraints.maxWidth <= 1 ? 1.0 : constraints.maxWidth;

        double ratioForDx(double dx) {
          return (dx / width).clamp(0.0, 1.0);
        }

        return GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTapDown: enabled
              ? (details) {
                  final targetRatio = ratioForDx(details.localPosition.dx);
                  widget.onSeekPreview(targetRatio);
                  widget.onSeekCommit(targetRatio);
                }
              : null,
          onHorizontalDragStart: enabled
              ? (details) {
                  final targetRatio = ratioForDx(details.localPosition.dx);
                  _dragRatio = targetRatio;
                  widget.onSeekPreview(targetRatio);
                }
              : null,
          onHorizontalDragUpdate: enabled
              ? (details) {
                  final targetRatio = ratioForDx(details.localPosition.dx);
                  _dragRatio = targetRatio;
                  widget.onSeekPreview(targetRatio);
                }
              : null,
          onHorizontalDragCancel: enabled
              ? () {
                  _dragRatio = null;
                  widget.onSeekCancel();
                }
              : null,
          onHorizontalDragEnd: enabled
              ? (_) {
                  final targetRatio = _dragRatio;
                  _dragRatio = null;
                  if (targetRatio != null) {
                    widget.onSeekCommit(targetRatio);
                  } else {
                    widget.onSeekCancel();
                  }
                }
              : null,
          child: SizedBox(
            width: double.infinity,
            height: touchHeight,
            child: Align(
              alignment: Alignment.center,
              child: SizedBox(
                height: visualHeight,
                child: Stack(
                  clipBehavior: Clip.none,
                  children: <Widget>[
                    Center(
                      child: Container(
                        width: double.infinity,
                        height: trackHeight,
                        decoration: BoxDecoration(
                          color: inactiveTrackColor,
                          borderRadius: BorderRadius.circular(999),
                        ),
                      ),
                    ),
                    Center(
                      child: Align(
                        alignment: Alignment.centerLeft,
                        child: Container(
                          width: width * ratio,
                          height: trackHeight,
                          decoration: BoxDecoration(
                            gradient: LinearGradient(
                              colors: <Color>[activeStart, activeEnd],
                            ),
                            borderRadius: BorderRadius.circular(999),
                          ),
                        ),
                      ),
                    ),
                    Positioned(
                      left: (width - knobSize) * ratio,
                      top: (visualHeight - knobSize) / 2,
                      child: Container(
                        width: knobSize,
                        height: knobSize,
                        decoration: BoxDecoration(
                          color: knobColor,
                          shape: BoxShape.circle,
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),
        );
      },
    );
  }
}

class VesperStagePrimaryPlayButton extends StatelessWidget {
  const VesperStagePrimaryPlayButton({
    super.key,
    required this.isPlaying,
    required this.onPressed,
    this.size = 72,
    this.iconSize = 36,
  });

  final bool isPlaying;
  final double size;
  final double iconSize;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: size,
      height: size,
      child: Material(
        color: Colors.white.withValues(alpha: 0.14),
        shape: const CircleBorder(),
        child: InkWell(
          customBorder: const CircleBorder(),
          onTap: onPressed,
          child: Center(
            child: Icon(
              isPlaying ? Icons.pause_rounded : Icons.play_arrow_rounded,
              size: iconSize,
              color: Colors.white,
            ),
          ),
        ),
      ),
    );
  }
}

class VesperStageIconButton extends StatelessWidget {
  const VesperStageIconButton({
    super.key,
    required this.icon,
    required this.label,
    required this.onPressed,
    this.size = 52,
    this.iconSize = 24,
    this.containerAlpha = 0.10,
  });

  final IconData icon;
  final String label;
  final double size;
  final double iconSize;
  final double containerAlpha;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: size,
      height: size,
      child: Material(
        color: Colors.white.withValues(alpha: containerAlpha),
        shape: const CircleBorder(),
        child: InkWell(
          customBorder: const CircleBorder(),
          onTap: onPressed,
          child: Center(
            child: Icon(icon, size: iconSize, color: Colors.white),
          ),
        ),
      ),
    );
  }
}

class VesperStagePillButton extends StatelessWidget {
  const VesperStagePillButton({
    super.key,
    required this.label,
    required this.onPressed,
    this.compact = false,
  });

  final String label;
  final bool compact;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    return TextButton(
      onPressed: onPressed,
      style: TextButton.styleFrom(
        foregroundColor: Colors.white,
        backgroundColor: Colors.white.withValues(alpha: 0.10),
        padding: EdgeInsets.symmetric(
          horizontal: compact ? 10 : 12,
          vertical: compact ? 6 : 8,
        ),
        minimumSize: Size(0, compact ? 30 : 36),
        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
      ),
      child: Text(
        label,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: Theme.of(
          context,
        ).textTheme.labelSmall?.copyWith(color: Colors.white),
      ),
    );
  }
}

class VesperStageChip extends StatelessWidget {
  const VesperStageChip({
    super.key,
    required this.label,
    required this.accent,
    this.compact = false,
  });

  final String label;
  final Color accent;
  final bool compact;

  @override
  Widget build(BuildContext context) {
    final dotSize = compact ? 6.0 : 8.0;
    final horizontalPadding = compact ? 8.0 : 10.0;
    final verticalPadding = compact ? 5.0 : 7.0;
    final gap = compact ? 6.0 : 8.0;
    return Container(
      padding: EdgeInsets.symmetric(
        horizontal: horizontalPadding,
        vertical: verticalPadding,
      ),
      decoration: BoxDecoration(
        color: Colors.black.withValues(alpha: 0.36),
        borderRadius: BorderRadius.circular(999),
        border: Border.all(color: Colors.white.withValues(alpha: 0.08)),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: <Widget>[
          Container(
            width: dotSize,
            height: dotSize,
            decoration: BoxDecoration(color: accent, shape: BoxShape.circle),
          ),
          SizedBox(width: gap),
          Text(
            label,
            style: Theme.of(context).textTheme.labelMedium?.copyWith(
              color: Colors.white,
              fontSize: compact ? 11 : null,
            ),
          ),
        ],
      ),
    );
  }
}
