import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_performance_diagnostics/vesper_player_performance_diagnostics.dart';

void main() {
  test('marker export is available', () {
    expect(vesperPlayerPerformanceDiagnosticsAvailable, isTrue);
  });
}
