part of 'example_player_sheet.dart';

class ExampleSelectionRow extends StatelessWidget {
  const ExampleSelectionRow({
    super.key,
    required this.title,
    required this.subtitle,
    required this.onTap,
    this.badgeLabel,
    this.badgeTone = ExampleSelectionBadgeTone.accent,
    this.selected = false,
    this.enabled = true,
  });

  final String title;
  final String subtitle;
  final VoidCallback onTap;
  final String? badgeLabel;
  final ExampleSelectionBadgeTone badgeTone;
  final bool selected;
  final bool enabled;

  @override
  Widget build(BuildContext context) {
    final titleColor = enabled
        ? Colors.white
        : Colors.white.withValues(alpha: 0.45);
    final subtitleColor = enabled
        ? const Color(0xFF98A1B3)
        : const Color(0xFF98A1B3).withValues(alpha: 0.55);
    final badgeAccent = switch (badgeTone) {
      ExampleSelectionBadgeTone.accent => const Color(0xFF8EC5FF),
      ExampleSelectionBadgeTone.warm => const Color(0xFFFFC876),
    };
    final badgeForeground = enabled
        ? switch (badgeTone) {
            ExampleSelectionBadgeTone.accent => const Color(0xFFDCEEFF),
            ExampleSelectionBadgeTone.warm => const Color(0xFFFFE8BF),
          }
        : switch (badgeTone) {
            ExampleSelectionBadgeTone.accent => const Color(
              0xFFDCEEFF,
            ).withValues(alpha: 0.5),
            ExampleSelectionBadgeTone.warm => const Color(
              0xFFFFE8BF,
            ).withValues(alpha: 0.5),
          };
    final badgeBackground = enabled
        ? badgeAccent.withValues(alpha: selected ? 0.20 : 0.12)
        : badgeAccent.withValues(alpha: 0.06);
    final badgeBorder = enabled
        ? badgeAccent.withValues(alpha: selected ? 0.34 : 0.18)
        : badgeAccent.withValues(alpha: 0.10);
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        Material(
          color: selected
              ? Colors.white.withValues(alpha: 0.10)
              : Colors.transparent,
          borderRadius: BorderRadius.circular(18),
          child: InkWell(
            onTap: enabled ? onTap : null,
            borderRadius: BorderRadius.circular(18),
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: <Widget>[
                  Row(
                    children: <Widget>[
                      Expanded(
                        child: Text(
                          title,
                          style: Theme.of(context).textTheme.titleSmall
                              ?.copyWith(
                                color: titleColor,
                                fontWeight: FontWeight.w600,
                              ),
                        ),
                      ),
                      if (badgeLabel case final label?)
                        Container(
                          padding: const EdgeInsets.symmetric(
                            horizontal: 10,
                            vertical: 4,
                          ),
                          decoration: BoxDecoration(
                            color: badgeBackground,
                            borderRadius: BorderRadius.circular(999),
                            border: Border.all(color: badgeBorder),
                          ),
                          child: Text(
                            label,
                            style: Theme.of(context).textTheme.labelSmall
                                ?.copyWith(
                                  color: badgeForeground,
                                  fontWeight: FontWeight.w700,
                                  letterSpacing: 0.2,
                                ),
                          ),
                        ),
                    ],
                  ),
                  const SizedBox(height: 4),
                  Text(
                    subtitle,
                    style: Theme.of(
                      context,
                    ).textTheme.bodySmall?.copyWith(color: subtitleColor),
                  ),
                ],
              ),
            ),
          ),
        ),
        Divider(color: Colors.white.withValues(alpha: 0.04), height: 1),
      ],
    );
  }
}

enum ExampleSelectionBadgeTone { accent, warm }
