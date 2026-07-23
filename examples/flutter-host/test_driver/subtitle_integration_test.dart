import 'dart:convert';
import 'dart:io';

import 'package:integration_test/integration_test_driver.dart';

Future<void> main() async {
  await integrationDriver(
    writeResponseOnFailure: true,
    responseDataCallback: _writeSubtitleEvidence,
  );
}

Future<void> _writeSubtitleEvidence(Map<String, dynamic>? data) async {
  final outputDirectory = Directory(
    Platform.environment['VESPER_SUBTITLE_EVIDENCE_DIR'] ??
        'build/subtitle-evidence',
  );
  await outputDirectory.create(recursive: true);

  final payload = <String, dynamic>{...?data};
  final configuredName = Platform.environment['VESPER_SUBTITLE_EVIDENCE_NAME'];
  final reportedName = payload['evidenceName'];
  final name = _evidenceFileName(
    configuredName != null && configuredName.isNotEmpty
        ? configuredName
        : reportedName is String
        ? reportedName
        : 'subtitle-integration',
  );
  final pngBase64 = payload.remove('pngBase64');
  if (pngBase64 is String && pngBase64.isNotEmpty) {
    final png = base64Decode(pngBase64);
    await File(
      '${outputDirectory.path}/$name.png',
    ).writeAsBytes(png, flush: true);
    payload['pngFile'] = '$name.png';
  }

  const encoder = JsonEncoder.withIndent('  ');
  await File(
    '${outputDirectory.path}/$name.json',
  ).writeAsString('${encoder.convert(payload)}\n', flush: true);
}

String _evidenceFileName(String value) {
  if (!RegExp(r'^[A-Za-z0-9][A-Za-z0-9._-]*$').hasMatch(value)) {
    throw FormatException('Invalid subtitle evidence file name: $value');
  }
  return value;
}
