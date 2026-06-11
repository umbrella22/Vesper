part of 'vesper_external_playback_controller.dart';

const MethodChannel _defaultMethodChannel = MethodChannel(
  'io.github.ikaros.vesper_player_external_playback',
);
const EventChannel _defaultRoutesEventChannel = EventChannel(
  'io.github.ikaros.vesper_player_external_playback/routes',
);
const EventChannel _defaultSessionEventChannel = EventChannel(
  'io.github.ikaros.vesper_player_external_playback/events',
);
const String _routeButtonViewType =
    'io.github.ikaros.vesper_player_external_playback/route_button';
