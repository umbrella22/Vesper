import 'package:flutter_test/flutter_test.dart';

import 'package:flutter_host/main.dart';

void main() {
  testWidgets('shows loading then unsupported error in widget test env', (
    WidgetTester tester,
  ) async {
    await tester.pumpWidget(const VesperFlutterHostApp());

    expect(find.text('正在初始化 Vesper Flutter Host...'), findsOneWidget);

    await tester.pump();

    expect(find.text('控制器初始化失败'), findsOneWidget);
  });
}
