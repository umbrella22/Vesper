part of '../vesper_player_view.dart';

final class _ViewportBindingObserver with WidgetsBindingObserver {
  _ViewportBindingObserver({
    required this.onMetricsChanged,
    required this.onLifecycleChanged,
  });

  final VoidCallback onMetricsChanged;
  final ValueChanged<AppLifecycleState> onLifecycleChanged;

  @override
  void didChangeMetrics() {
    onMetricsChanged();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    onLifecycleChanged(state);
  }
}
