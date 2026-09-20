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
    final skin = VesperPlayerStageTheme.of(context);
    final icon = switch (feedback.kind) {
      _StageGestureKind.brightness => VesperStageIconRole.brightness,
      _StageGestureKind.volume => VesperStageIconRole.volume,
      _StageGestureKind.speed => VesperStageIconRole.speed,
    };
    final progress = feedback.progress?.clamp(0.0, 1.0).toDouble();

    return Container(
      width: progress == null ? null : 226,
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
      decoration: BoxDecoration(
        color: skin.colors.hudBackground,
        borderRadius: BorderRadius.circular(skin.metrics.hudBorderRadius),
      ),
      child: Row(
        mainAxisSize: progress == null ? MainAxisSize.min : MainAxisSize.max,
        crossAxisAlignment: CrossAxisAlignment.center,
        children: <Widget>[
          VesperStageIcon(icon,
              size: skin.metrics.hudIconSize, color: skin.colors.hudForeground),
          const SizedBox(width: 10),
          if (progress != null) ...<Widget>[
            Expanded(
              child: ClipRRect(
                borderRadius:
                    BorderRadius.circular(skin.metrics.hudBorderRadius),
                child: LinearProgressIndicator(
                  minHeight: 4,
                  value: progress,
                  backgroundColor: skin.colors.hudForeground
                      .withValues(alpha: skin.colors.hudForeground.a * 0.18),
                  valueColor:
                      AlwaysStoppedAnimation<Color>(skin.colors.hudForeground),
                ),
              ),
            ),
            const SizedBox(width: 8),
          ],
          Text(
            feedback.label,
            style: Theme.of(context).textTheme.labelMedium?.copyWith(
              color: skin.colors.hudForeground,
              fontFeatures: const <FontFeature>[FontFeature.tabularFigures()],
            ),
          ),
        ],
      ),
    );
  }
}

String _percentLabel(double value) => '${(value * 100).round()}%';
