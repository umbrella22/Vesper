part of 'vesper_player_stage.dart';

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
  final GlobalKey _scrubberKey = GlobalKey();
  int? _activePointer;
  Offset? _pointerDownPosition;
  bool _dragging = false;

  @override
  void didUpdateWidget(covariant VesperTimelineScrubber oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.enabled && !widget.enabled) {
      _cancelPointer();
    }
  }

  void _handlePointerDown(PointerDownEvent event) {
    if (!widget.enabled || _activePointer != null) {
      return;
    }
    _activePointer = event.pointer;
    _pointerDownPosition = event.position;
    _dragging = false;
  }

  void _handlePointerMove(PointerMoveEvent event) {
    if (!widget.enabled || event.pointer != _activePointer) {
      return;
    }
    final downPosition = _pointerDownPosition;
    if (downPosition == null) {
      return;
    }
    if (!_dragging && (event.position - downPosition).distance <= 8.0) {
      return;
    }
    _dragging = true;
    final targetRatio = _ratioForGlobalPosition(event.position);
    widget.onSeekPreview(targetRatio);
  }

  void _handlePointerUp(PointerUpEvent event) {
    if (event.pointer != _activePointer) {
      return;
    }
    if (_dragging) {
      // A touch platform may coalesce the final move before dispatching up.
      // Re-sample the release coordinate so the committed seek reflects where
      // the user actually lifted their finger, rather than the last move.
      final targetRatio = _ratioForGlobalPosition(event.position);
      widget.onSeekPreview(targetRatio);
      widget.onSeekCommit(targetRatio);
    } else {
      final targetRatio = _ratioForGlobalPosition(event.position);
      widget.onSeekPreview(targetRatio);
      widget.onSeekCommit(targetRatio);
    }
    _resetPointer();
  }

  void _handlePointerCancel(PointerCancelEvent event) {
    if (event.pointer != _activePointer) {
      return;
    }
    _cancelPointer();
  }

  void _cancelPointer() {
    if (_activePointer != null && _dragging) {
      widget.onSeekCancel();
    }
    _resetPointer();
  }

  void _resetPointer() {
    _activePointer = null;
    _pointerDownPosition = null;
    _dragging = false;
  }

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

        Widget scrubber = Listener(
          behavior: HitTestBehavior.opaque,
          onPointerDown: enabled ? _handlePointerDown : null,
          onPointerMove: enabled ? _handlePointerMove : null,
          onPointerUp: enabled ? _handlePointerUp : null,
          onPointerCancel: enabled ? _handlePointerCancel : null,
          child: SizedBox(
            key: _scrubberKey,
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
        if (enabled) {
          scrubber = RawGestureDetector(
            behavior: HitTestBehavior.opaque,
            gestures: <Type, GestureRecognizerFactory>{
              EagerGestureRecognizer:
                  GestureRecognizerFactoryWithHandlers<EagerGestureRecognizer>(
                EagerGestureRecognizer.new,
                (recognizer) {},
              ),
            },
            child: scrubber,
          );
        }
        return scrubber;
      },
    );
  }

  double _ratioForGlobalPosition(Offset globalPosition) {
    final renderObject = _scrubberKey.currentContext?.findRenderObject();
    if (renderObject is RenderBox &&
        renderObject.attached &&
        renderObject.hasSize &&
        renderObject.size.width > 0) {
      final localPosition = renderObject.globalToLocal(globalPosition);
      return (localPosition.dx / renderObject.size.width)
          .clamp(0.0, 1.0)
          .toDouble();
    }
    return 0.0;
  }
}
