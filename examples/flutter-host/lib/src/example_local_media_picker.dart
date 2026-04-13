import 'package:flutter/services.dart';

const String _mediaPickerChannelName =
    'io.github.ikaros.vesper.example.flutter_host/media_picker';

final class ExamplePickedVideo {
  const ExamplePickedVideo({required this.uri, required this.label});

  factory ExamplePickedVideo.fromMap(Map<Object?, Object?> map) {
    return ExamplePickedVideo(
      uri: map['uri'] as String? ?? '',
      label: map['label'] as String? ?? '本地视频',
    );
  }

  final String uri;
  final String label;
}

abstract final class ExampleLocalMediaPicker {
  static const MethodChannel _channel = MethodChannel(_mediaPickerChannelName);

  static Future<ExamplePickedVideo?> pickVideo() async {
    final response = await _channel.invokeMethod<Object?>('pickVideo');
    if (response == null) {
      return null;
    }
    if (response is Map<Object?, Object?>) {
      return ExamplePickedVideo.fromMap(response);
    }
    throw PlatformException(
      code: 'invalid_result',
      message: 'Native picker returned an unexpected payload.',
    );
  }
}
