import 'dart:async';

import 'package:flutter/material.dart';
import 'package:vesper_player/vesper_player.dart';

import 'example_player_helpers.dart';
import 'example_player_models.dart';

class ExamplePlayerStage extends StatefulWidget {
  const ExamplePlayerStage({
    super.key,
    required this.controller,
    required this.snapshot,
    required this.isPortrait,
    required this.onOpenSheet,
    required this.onToggleFullscreen,
    this.sheetOpen = false,
  });

  final VesperPlayerController controller;
  final VesperPlayerSnapshot snapshot;
  final bool isPortrait;
  final bool sheetOpen;
  final ValueChanged<ExamplePlayerSheet> onOpenSheet;
  final VoidCallback onToggleFullscreen;

  @override
  State<ExamplePlayerStage> createState() => _ExamplePlayerStageState();
}

class _ExamplePlayerStageState extends State<ExamplePlayerStage> {
  Timer? _controlsTimer;
  bool _controlsVisible = true;
  double? _pendingSeekRatio;

  @override
  void initState() {
    super.initState();
    _syncAutoHide();
  }

  @override
  void didUpdateWidget(covariant ExamplePlayerStage oldWidget) {
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
    _controlsTimer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final snapshot = widget.snapshot;
    final displayedRatio =
        (_pendingSeekRatio ?? snapshot.timeline.displayedRatio ?? 0.0).clamp(
          0.0,
          1.0,
        );
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
                onDoubleTapDown: (details) =>
                    _handleDoubleTap(details.localPosition.dx),
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
                        child: Row(
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
                                          style: Theme.of(context)
                                              .textTheme
                                              .titleMedium
                                              ?.copyWith(
                                                color: Colors.white,
                                                fontWeight: FontWeight.bold,
                                              ),
                                        ),
                                      ),
                                      if (snapshot.isBuffering) ...<Widget>[
                                        const SizedBox(width: 8),
                                        const ExampleStageChip(
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
                                    style: Theme.of(context).textTheme.bodySmall
                                        ?.copyWith(
                                          color: const Color(0xFFBFC6D6),
                                        ),
                                  ),
                                ],
                              ),
                            ),
                            const SizedBox(width: 10),
                            if (widget.isPortrait)
                              ExampleStageIconButton(
                                icon: Icons.more_vert_rounded,
                                label: '更多',
                                size: 38,
                                iconSize: 24,
                                containerAlpha: 0,
                                onPressed: () =>
                                    widget.onOpenSheet(ExamplePlayerSheet.menu),
                              )
                            else
                              Row(
                                children: <Widget>[
                                  ExampleStageIconButton(
                                    icon: Icons.tune_rounded,
                                    label: '画质',
                                    containerAlpha: 0,
                                    onPressed: () => widget.onOpenSheet(
                                      ExamplePlayerSheet.quality,
                                    ),
                                  ),
                                  const SizedBox(width: 10),
                                  ExampleStageIconButton(
                                    icon: Icons.graphic_eq_rounded,
                                    label: '音频',
                                    containerAlpha: 0,
                                    onPressed: () => widget.onOpenSheet(
                                      ExamplePlayerSheet.audio,
                                    ),
                                  ),
                                  const SizedBox(width: 10),
                                  ExampleStageIconButton(
                                    icon: Icons.closed_caption_rounded,
                                    label: '字幕',
                                    containerAlpha: 0,
                                    onPressed: () => widget.onOpenSheet(
                                      ExamplePlayerSheet.subtitle,
                                    ),
                                  ),
                                  const SizedBox(width: 10),
                                  ExampleStageIconButton(
                                    icon: Icons.speed_rounded,
                                    label: '播放速度',
                                    containerAlpha: 0,
                                    onPressed: () => widget.onOpenSheet(
                                      ExamplePlayerSheet.speed,
                                    ),
                                  ),
                                ],
                              ),
                          ],
                        ),
                      ),
                      Align(
                        alignment: Alignment.center,
                        child: Row(
                          mainAxisSize: MainAxisSize.min,
                          children: <Widget>[
                            ExampleStageIconButton(
                              icon: Icons.replay_10_rounded,
                              label: '后退 10 秒',
                              size: widget.isPortrait ? 52 : 44,
                              iconSize: widget.isPortrait ? 24 : 20,
                              onPressed: _seekBackward,
                            ),
                            const SizedBox(width: 16),
                            ExampleStagePrimaryPlayButton(
                              isPlaying:
                                  snapshot.playbackState ==
                                  VesperPlaybackState.playing,
                              size: widget.isPortrait ? 72 : 60,
                              iconSize: widget.isPortrait ? 36 : 28,
                              onPressed: _togglePause,
                            ),
                            const SizedBox(width: 16),
                            ExampleStageIconButton(
                              icon: Icons.forward_10_rounded,
                              label: '前进 10 秒',
                              size: widget.isPortrait ? 52 : 44,
                              iconSize: widget.isPortrait ? 24 : 20,
                              onPressed: _seekForward,
                            ),
                          ],
                        ),
                      ),
                      Positioned(
                        left: widget.isPortrait ? 18 : 12,
                        right: widget.isPortrait ? 18 : 12,
                        bottom: widget.isPortrait ? 18 : 8,
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: <Widget>[
                            ExampleTimelineScrubber(
                              displayedRatio: displayedRatio,
                              compact: !widget.isPortrait,
                              onSeekPreview: (ratio) {
                                setState(() {
                                  _pendingSeekRatio = ratio;
                                });
                                _showControls();
                              },
                              onSeekCommit: (ratio) {
                                setState(() {
                                  _pendingSeekRatio = null;
                                });
                                unawaited(widget.controller.seekToRatio(ratio));
                                _showControls();
                              },
                              onSeekCancel: () {
                                setState(() {
                                  _pendingSeekRatio = null;
                                });
                                _syncAutoHide();
                              },
                            ),
                            const SizedBox(height: 6),
                            Row(
                              mainAxisAlignment: MainAxisAlignment.spaceBetween,
                              crossAxisAlignment: CrossAxisAlignment.center,
                              children: <Widget>[
                                Expanded(
                                  child: Text(
                                    timelineSummary(
                                      snapshot.timeline,
                                      _pendingSeekRatio,
                                    ),
                                    maxLines: 1,
                                    overflow: TextOverflow.ellipsis,
                                    style: Theme.of(context)
                                        .textTheme
                                        .labelLarge
                                        ?.copyWith(
                                          color: const Color(0xFFF7F8FC),
                                        ),
                                  ),
                                ),
                                const SizedBox(width: 8),
                                Row(
                                  mainAxisSize: MainAxisSize.min,
                                  children: <Widget>[
                                    if (snapshot.timeline.kind ==
                                        VesperTimelineKind.liveDvr) ...<Widget>[
                                      ExampleStagePillButton(
                                        label: liveButtonLabel(
                                          snapshot.timeline,
                                        ),
                                        onPressed: _seekToLiveEdge,
                                      ),
                                      const SizedBox(width: 8),
                                    ],
                                    ExampleStageIconButton(
                                      icon: widget.isPortrait
                                          ? Icons.fullscreen_rounded
                                          : Icons.fullscreen_exit_rounded,
                                      label: widget.isPortrait ? '全屏' : '退出全屏',
                                      size: widget.isPortrait ? 38 : 32,
                                      iconSize: widget.isPortrait ? 24 : 18,
                                      containerAlpha: 0,
                                      onPressed: widget.onToggleFullscreen,
                                    ),
                                  ],
                                ),
                              ],
                            ),
                          ],
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }

  void _handleTap() {
    setState(() {
      _controlsVisible = !_controlsVisible;
    });
    _syncAutoHide();
  }

  void _handleDoubleTap(double dx) {
    final width = context.size?.width ?? 0;
    if (width <= 0) {
      return;
    }
    if (dx < width / 2) {
      _seekBackward();
      return;
    }
    _seekForward();
  }

  void _seekBackward() {
    unawaited(widget.controller.seekBy(-10000));
    _showControls();
  }

  void _seekForward() {
    unawaited(widget.controller.seekBy(10000));
    _showControls();
  }

  void _togglePause() {
    unawaited(widget.controller.togglePause());
    _showControls();
  }

  void _seekToLiveEdge() {
    unawaited(widget.controller.seekToLiveEdge());
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

class ExampleTimelineScrubber extends StatefulWidget {
  const ExampleTimelineScrubber({
    super.key,
    required this.displayedRatio,
    required this.onSeekPreview,
    required this.onSeekCommit,
    required this.onSeekCancel,
    this.compact = false,
  });

  final double displayedRatio;
  final bool compact;
  final ValueChanged<double> onSeekPreview;
  final ValueChanged<double> onSeekCommit;
  final VoidCallback onSeekCancel;

  @override
  State<ExampleTimelineScrubber> createState() =>
      _ExampleTimelineScrubberState();
}

class _ExampleTimelineScrubberState extends State<ExampleTimelineScrubber> {
  double? _dragRatio;

  @override
  Widget build(BuildContext context) {
    final knobSize = widget.compact ? 11.0 : 14.0;
    final touchHeight = widget.compact ? 22.0 : 28.0;
    final visualHeight = widget.compact ? 14.0 : 18.0;
    final trackHeight = 4.0;
    final ratio = widget.displayedRatio.clamp(0.0, 1.0);

    return LayoutBuilder(
      builder: (context, constraints) {
        final width = constraints.maxWidth <= 1 ? 1.0 : constraints.maxWidth;

        double ratioForDx(double dx) {
          return (dx / width).clamp(0.0, 1.0);
        }

        return GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTapDown: (details) {
            final targetRatio = ratioForDx(details.localPosition.dx);
            widget.onSeekPreview(targetRatio);
            widget.onSeekCommit(targetRatio);
          },
          onHorizontalDragStart: (details) {
            final targetRatio = ratioForDx(details.localPosition.dx);
            _dragRatio = targetRatio;
            widget.onSeekPreview(targetRatio);
          },
          onHorizontalDragUpdate: (details) {
            final targetRatio = ratioForDx(details.localPosition.dx);
            _dragRatio = targetRatio;
            widget.onSeekPreview(targetRatio);
          },
          onHorizontalDragCancel: () {
            _dragRatio = null;
            widget.onSeekCancel();
          },
          onHorizontalDragEnd: (_) {
            final targetRatio = _dragRatio;
            _dragRatio = null;
            if (targetRatio != null) {
              widget.onSeekCommit(targetRatio);
            } else {
              widget.onSeekCancel();
            }
          },
          child: SizedBox(
            width: double.infinity,
            height: touchHeight,
            child: Align(
              alignment: Alignment.bottomCenter,
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
                          color: Colors.white.withValues(alpha: 0.16),
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
                            gradient: const LinearGradient(
                              colors: <Color>[
                                Color(0xFFFF6B8E),
                                Color(0xFFFFB454),
                              ],
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
                        decoration: const BoxDecoration(
                          color: Colors.white,
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

class ExampleStagePrimaryPlayButton extends StatelessWidget {
  const ExampleStagePrimaryPlayButton({
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

class ExampleStageIconButton extends StatelessWidget {
  const ExampleStageIconButton({
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

class ExampleStagePillButton extends StatelessWidget {
  const ExampleStagePillButton({
    super.key,
    required this.label,
    required this.onPressed,
  });

  final String label;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    return TextButton(
      onPressed: onPressed,
      style: TextButton.styleFrom(
        foregroundColor: Colors.white,
        backgroundColor: Colors.white.withValues(alpha: 0.10),
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
      ),
      child: Text(label, maxLines: 1, overflow: TextOverflow.ellipsis),
    );
  }
}

class ExampleStageChip extends StatelessWidget {
  const ExampleStageChip({
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
