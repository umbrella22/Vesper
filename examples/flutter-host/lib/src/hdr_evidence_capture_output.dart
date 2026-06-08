import 'dart:io';

import 'package:flutter/services.dart';

const String _mediaPickerChannelName =
    'io.github.ikaros.vesper.example.flutter_host/media_picker';

abstract final class ExampleHdrEvidenceCaptureOutput {
  static const MethodChannel _channel = MethodChannel(_mediaPickerChannelName);

  static Future<Directory> defaultOutputRoot() async {
    final Object? response;
    try {
      response = await _channel.invokeMethod<Object?>('hdrEvidenceOutputRoot');
    } on MissingPluginException {
      return Directory.systemTemp.createTemp('vesper-hdr-evidence-');
    }
    if (response is String && response.trim().isNotEmpty) {
      return Directory(response.trim());
    }
    throw PlatformException(
      code: 'invalid_result',
      message: 'Native HDR evidence output root returned an invalid payload.',
    );
  }

  static Future<Map<String, Object?>> deviceEvidence() async {
    final Object? response;
    try {
      response = await _channel.invokeMethod<Object?>('hdrEvidenceDevice');
    } on MissingPluginException {
      return const <String, Object?>{};
    }
    if (response == null) {
      return const <String, Object?>{};
    }
    if (response is Map) {
      return <String, Object?>{
        for (final entry in response.entries)
          entry.key.toString(): _jsonValue(entry.value),
      };
    }
    throw PlatformException(
      code: 'invalid_result',
      message: 'Native HDR evidence device sheet returned an invalid payload.',
    );
  }
}

Object? _jsonValue(Object? value) {
  if (value == null || value is num || value is bool || value is String) {
    return value;
  }
  if (value is Iterable) {
    return value.map(_jsonValue).toList(growable: false);
  }
  if (value is Map) {
    return <String, Object?>{
      for (final entry in value.entries)
        entry.key.toString(): _jsonValue(entry.value),
    };
  }
  return value.toString();
}
