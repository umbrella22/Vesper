part of 'vesper_player_stage.dart';

const double _stageMinimumTapTargetSize = 48;

class VesperStagePrimaryPlayButton extends StatelessWidget {
  const VesperStagePrimaryPlayButton({
    super.key,
    required this.isPlaying,
    required this.onPressed,
    this.strings = const VesperPlayerStageStrings(),
    this.style,
  });
  final bool isPlaying;
  final VoidCallback onPressed;
  final VesperPlayerStageStrings strings;
  final VesperStageButtonStyle? style;

  @override
  Widget build(BuildContext context) => VesperStageIconButton(
        icon: VesperStageIcon(
            isPlaying ? VesperStageIconRole.pause : VesperStageIconRole.play),
        label: isPlaying ? strings.pause : strings.play,
        variant: VesperStageButtonVariant.primary,
        style: style,
        onPressed: onPressed,
      );
}

/// A decorative icon inside a skinned, accessible action button.
/// Use [VesperStageIcon] for built-in actions, or any non-interactive widget
/// for host actions. The button supplies icon size, color, label and hit area.
class VesperStageIconButton extends StatelessWidget {
  const VesperStageIconButton({
    super.key,
    required this.icon,
    required this.label,
    required this.onPressed,
    this.variant = VesperStageButtonVariant.standard,
    this.style,
  });
  final Widget icon;
  final String label;
  final VoidCallback onPressed;
  final VesperStageButtonVariant variant;
  final VesperStageButtonStyle? style;

  @override
  Widget build(BuildContext context) {
    final skin = VesperPlayerStageTheme.of(context);
    final appearance = style ?? skin.metrics.button(variant);
    final hitSize = math.max(appearance.size, _stageMinimumTapTargetSize);
    final shape = RoundedRectangleBorder(
      borderRadius: BorderRadius.circular(appearance.borderRadius),
    );
    return Tooltip(
      message: label,
      child: Semantics(
        label: label,
        button: true,
        child: SizedBox.square(
          dimension: hitSize,
          child: Material(
            type: MaterialType.transparency,
            child: InkWell(
              customBorder: shape,
              onTap: onPressed,
              child: Center(
                child: Ink(
                  width: appearance.size,
                  height: appearance.size,
                  decoration: ShapeDecoration(
                    color: skin.colors.buttonBackground.withValues(
                      alpha: skin.colors.buttonBackground.a *
                          appearance.backgroundOpacity,
                    ),
                    shape: shape,
                  ),
                  child: Center(
                    child: IgnorePointer(
                      child: ExcludeSemantics(
                        child: IconTheme(
                          data: IconThemeData(
                              size: appearance.iconSize,
                              color: skin.colors.foreground),
                          child: SizedBox.square(
                              dimension: appearance.iconSize, child: icon),
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            ),
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
    final skin = VesperPlayerStageTheme.of(context);
    return TextButton(
      onPressed: onPressed,
      style: TextButton.styleFrom(
        foregroundColor: skin.colors.foreground,
        backgroundColor: skin.colors.buttonBackground
            .withValues(alpha: skin.colors.buttonBackground.a * 0.10),
        shape: RoundedRectangleBorder(
            borderRadius:
                BorderRadius.circular(skin.metrics.standard.borderRadius)),
        padding: EdgeInsets.symmetric(
          horizontal: compact ? 10 : 12,
          vertical: compact ? 6 : 8,
        ),
        minimumSize: Size(0, compact ? 30 : 36),
        tapTargetSize: MaterialTapTargetSize.padded,
        visualDensity: VisualDensity.standard,
      ),
      child: Text(
        label,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: Theme.of(
          context,
        ).textTheme.labelSmall?.copyWith(color: skin.colors.foreground),
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
    final skin = VesperPlayerStageTheme.of(context);
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
        color: skin.colors.scrim.withValues(alpha: skin.colors.scrim.a * 0.36),
        borderRadius: BorderRadius.circular(999),
        border: Border.all(
            color: skin.colors.foreground
                .withValues(alpha: skin.colors.foreground.a * 0.08)),
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
                  color: skin.colors.foreground,
                  fontSize: compact ? 11 : null,
                ),
          ),
        ],
      ),
    );
  }
}
