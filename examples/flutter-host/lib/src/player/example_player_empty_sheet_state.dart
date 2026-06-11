part of 'example_player_sheet.dart';

class ExampleEmptySheetState extends StatelessWidget {
  const ExampleEmptySheetState({super.key, required this.message});

  final String message;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(top: 8),
      child: Container(
        width: double.infinity,
        padding: const EdgeInsets.all(18),
        decoration: BoxDecoration(
          color: Colors.white.withValues(alpha: 0.03),
          borderRadius: BorderRadius.circular(18),
        ),
        child: Text(
          message,
          style: Theme.of(context).textTheme.bodySmall?.copyWith(
            color: const Color(0xFF98A1B3),
            height: 1.45,
          ),
        ),
      ),
    );
  }
}
