import 'dart:async';

import 'package:flutter/services.dart';

typedef ExampleSubtitleOverlaySnapshotLoader =
    Future<ExampleSubtitleOverlaySnapshot> Function();

class ExampleSubtitleOverlayFrame {
  const ExampleSubtitleOverlayFrame({
    required this.x,
    required this.y,
    required this.width,
    required this.height,
  });

  final double x;
  final double y;
  final double width;
  final double height;

  Map<String, Object?> toJson() => <String, Object?>{
    'x': x,
    'y': y,
    'width': width,
    'height': height,
  };
}

class ExampleSubtitleOverlaySnapshot {
  const ExampleSubtitleOverlaySnapshot({
    required this.text,
    required this.hidden,
    required this.alpha,
    required this.windowAttached,
    required this.frame,
    required this.visible,
  });

  final String text;
  final bool hidden;
  final double alpha;
  final bool windowAttached;
  final ExampleSubtitleOverlayFrame frame;
  final bool visible;

  Map<String, Object?> toJson() => <String, Object?>{
    'text': text,
    'hidden': hidden,
    'alpha': alpha,
    'windowAttached': windowAttached,
    'frame': frame.toJson(),
    'visible': visible,
  };
}

class ExampleSubtitleOverlayEvidence {
  const ExampleSubtitleOverlayEvidence({
    required this.snapshot,
    required this.png,
  });

  final ExampleSubtitleOverlaySnapshot snapshot;
  final Uint8List png;
}

abstract final class ExampleSubtitleOverlayEvidenceChannel {
  static const MethodChannel _channel = MethodChannel(
    'io.github.ikaros.vesper.example.flutter_host/device_controls',
  );

  static Future<ExampleSubtitleOverlaySnapshot> snapshot(
    String playerId,
  ) async {
    final response = await _channel.invokeMethod<Object?>(
      'subtitleOverlaySnapshot',
      <String, Object?>{'playerId': playerId},
    );
    return _decodeSnapshot(_requireMap(response, 'snapshot'));
  }

  static Future<ExampleSubtitleOverlayEvidence> capture(String playerId) async {
    final response = _requireMap(
      await _channel.invokeMethod<Object?>(
        'captureSubtitleOverlayEvidence',
        <String, Object?>{'playerId': playerId},
      ),
      'evidence',
    );
    final png = response['png'];
    if (png is! Uint8List || png.isEmpty) {
      throw const FormatException(
        'Native subtitle overlay evidence did not contain PNG bytes.',
      );
    }
    return ExampleSubtitleOverlayEvidence(
      snapshot: _decodeSnapshot(
        _requireMap(response['snapshot'], 'evidence snapshot'),
      ),
      png: png,
    );
  }

  static ExampleSubtitleOverlaySnapshot _decodeSnapshot(
    Map<Object?, Object?> value,
  ) {
    final frame = _requireMap(value['frame'], 'snapshot frame');
    return ExampleSubtitleOverlaySnapshot(
      text: _requireValue<String>(value, 'text'),
      hidden: _requireValue<bool>(value, 'hidden'),
      alpha: _requireNumber(value, 'alpha'),
      windowAttached: _requireValue<bool>(value, 'windowAttached'),
      frame: ExampleSubtitleOverlayFrame(
        x: _requireNumber(frame, 'x'),
        y: _requireNumber(frame, 'y'),
        width: _requireNumber(frame, 'width'),
        height: _requireNumber(frame, 'height'),
      ),
      visible: _requireValue<bool>(value, 'visible'),
    );
  }

  static Map<Object?, Object?> _requireMap(Object? value, String name) {
    if (value is Map<Object?, Object?>) {
      return value;
    }
    if (value is Map) {
      return Map<Object?, Object?>.from(value);
    }
    throw FormatException('Native subtitle overlay $name is not a map.');
  }

  static T _requireValue<T>(Map<Object?, Object?> value, String key) {
    final field = value[key];
    if (field is T) {
      return field;
    }
    throw FormatException(
      'Native subtitle overlay field $key has the wrong type.',
    );
  }

  static double _requireNumber(Map<Object?, Object?> value, String key) {
    final field = value[key];
    if (field is num) {
      return field.toDouble();
    }
    throw FormatException('Native subtitle overlay field $key is not numeric.');
  }
}

Future<ExampleSubtitleOverlaySnapshot> waitForVisibleExampleSubtitleOverlay({
  required ExampleSubtitleOverlaySnapshotLoader snapshot,
  required String expectedText,
  Duration timeout = const Duration(seconds: 5),
  Duration retryDelay = const Duration(milliseconds: 50),
}) async {
  final deadline = DateTime.now().add(timeout);
  ExampleSubtitleOverlaySnapshot? latest;
  while (DateTime.now().isBefore(deadline)) {
    try {
      latest = await snapshot();
    } on PlatformException catch (error) {
      if (error.code != 'subtitle_overlay_unavailable') {
        rethrow;
      }
    }
    if (latest?.visible == true && latest?.text == expectedText) {
      return latest!;
    }
    await Future<void>.delayed(retryDelay);
  }
  throw TimeoutException(
    'subtitle overlay did not become visible: '
    'text=${latest?.text} visible=${latest?.visible} '
    'windowAttached=${latest?.windowAttached} '
    'frame=${latest?.frame.toJson()}',
    timeout,
  );
}
