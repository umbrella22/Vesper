import 'dart:io' show Platform;

import 'package:flutter/services.dart';
import 'package:vesper_player_ui/vesper_player_ui.dart';

class ExampleDeviceControls implements VesperPlayerDeviceControls {
  static const MethodChannel _channel = MethodChannel(
    'io.github.ikaros.vesper.example.flutter_host/device_controls',
  );

  @override
  Future<double?> currentBrightnessRatio() {
    return _invokeRatio('getBrightness');
  }

  @override
  Future<double?> setBrightnessRatio(double ratio) {
    return _invokeRatio('setBrightness', <String, Object?>{
      'ratio': ratio.clamp(0.0, 1.0),
    });
  }

  @override
  Future<double?> currentVolumeRatio() {
    return _invokeRatio('getVolume');
  }

  @override
  Future<double?> setVolumeRatio(double ratio) {
    return _invokeRatio('setVolume', <String, Object?>{
      'ratio': ratio.clamp(0.0, 1.0),
    });
  }

  Future<double?> _invokeRatio(
    String method, [
    Map<String, Object?>? arguments,
  ]) async {
    if (!Platform.isAndroid && !Platform.isIOS) {
      return null;
    }
    try {
      final value = await _channel.invokeMethod<num>(method, arguments);
      return value?.toDouble().clamp(0.0, 1.0).toDouble();
    } on MissingPluginException {
      return null;
    } on PlatformException {
      return null;
    }
  }
}
