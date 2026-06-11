part of 'example_player_sheet.dart';

class ExampleSheetNote extends StatelessWidget {
  const ExampleSheetNote({
    super.key,
    required this.message,
    this.title,
    this.tone = ExampleSheetNoteTone.info,
  });

  final String? title;
  final String message;
  final ExampleSheetNoteTone tone;

  @override
  Widget build(BuildContext context) {
    final accent = switch (tone) {
      ExampleSheetNoteTone.info => const Color(0xFF8EC5FF),
      ExampleSheetNoteTone.warm => const Color(0xFFFFC876),
    };
    final foreground = switch (tone) {
      ExampleSheetNoteTone.info => const Color(0xFFC7DCF7),
      ExampleSheetNoteTone.warm => const Color(0xFFFFE8BF),
    };
    final titleColor = switch (tone) {
      ExampleSheetNoteTone.info => const Color(0xFFE8F3FF),
      ExampleSheetNoteTone.warm => const Color(0xFFFFF4D3),
    };
    final icon = switch (tone) {
      ExampleSheetNoteTone.info => Icons.tips_and_updates_outlined,
      ExampleSheetNoteTone.warm => Icons.auto_awesome_motion_rounded,
    };
    return Padding(
      padding: const EdgeInsets.only(top: 8, bottom: 8),
      child: Container(
        width: double.infinity,
        padding: const EdgeInsets.all(18),
        decoration: BoxDecoration(
          color: accent.withValues(alpha: 0.09),
          borderRadius: BorderRadius.circular(18),
          border: Border.all(color: accent.withValues(alpha: 0.18)),
        ),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Container(
              width: 32,
              height: 32,
              decoration: BoxDecoration(
                color: accent.withValues(alpha: 0.12),
                borderRadius: BorderRadius.circular(999),
              ),
              child: Icon(icon, size: 18, color: accent),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: <Widget>[
                  if (title case final String titleText) ...<Widget>[
                    Text(
                      titleText,
                      style: Theme.of(context).textTheme.labelLarge?.copyWith(
                        color: titleColor,
                        fontWeight: FontWeight.w700,
                        letterSpacing: 0.1,
                      ),
                    ),
                    const SizedBox(height: 4),
                  ],
                  Text(
                    message,
                    style: Theme.of(context).textTheme.bodySmall?.copyWith(
                      color: foreground,
                      height: 1.45,
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}
