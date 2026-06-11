part of 'vesper_player_stage.dart';

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
    return Tooltip(
      message: label,
      child: Semantics(
        label: label,
        button: true,
        child: SizedBox(
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
