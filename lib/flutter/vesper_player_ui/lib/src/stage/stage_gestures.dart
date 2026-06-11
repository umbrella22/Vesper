part of 'vesper_player_stage.dart';

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
