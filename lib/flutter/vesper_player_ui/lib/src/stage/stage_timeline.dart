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
