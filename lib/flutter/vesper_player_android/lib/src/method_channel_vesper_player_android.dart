import 'package:flutter/services.dart';
import 'package:vesper_player_platform_interface/method_channel_platform_base.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

class MethodChannelVesperPlayerAndroid extends VesperMethodChannelPlatformBase {
  MethodChannelVesperPlayerAndroid()
      : super(
          methodChannel: _methodChannel,
          eventChannel: _eventChannel,
          downloadEventChannel: _downloadEventChannel,
        ) {
    VesperPlayerPlatform.instance = this;
  }

  static const MethodChannel _methodChannel = MethodChannel(
    'io.github.ikaros.vesper_player',
  );
  static const EventChannel _eventChannel = EventChannel(
    'io.github.ikaros.vesper_player/events',
  );
  static const EventChannel _downloadEventChannel = EventChannel(
    'io.github.ikaros.vesper_player/download_events',
  );
}
