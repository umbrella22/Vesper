import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

import '../integration_test/support/subtitle_media_fixture.dart';

void main() {
  test('subtitle media fixture preserves the accepted M4A bytes', () async {
    final first = decodeTinyAacFixture();
    final second = decodeTinyAacFixture();

    expect(first, hasLength(tinyAacFixtureLength));
    expect(first.sublist(4, 12), orderedEquals(utf8.encode('ftypM4A ')));
    expect(second, orderedEquals(first));

    final directory = await Directory.systemTemp.createTemp(
      'vesper-subtitle-media-fixture-',
    );
    try {
      final file = await writeTinyAacFixture(directory);
      expect(file.path, endsWith('tiny-aac.m4a'));
      expect(await file.readAsBytes(), orderedEquals(first));
    } finally {
      await directory.delete(recursive: true);
    }
  });
}
