part of 'vesper_external_playback_controller.dart';

const MethodChannel _defaultMethodChannel = MethodChannel(
  'io.github.umbrella22.vesper_player_external_playback',
);
const EventChannel _defaultRoutesEventChannel = EventChannel(
  'io.github.umbrella22.vesper_player_external_playback/routes',
);
const EventChannel _defaultSessionEventChannel = EventChannel(
  'io.github.umbrella22.vesper_player_external_playback/events',
);
const String _routeButtonViewType =
    'io.github.umbrella22.vesper_player_external_playback/route_button';

final class _VesperExternalPlaybackChannelHub {
  static Stream<List<VesperExternalPlaybackRoute>>? _nativeRoutes;
  static Stream<VesperExternalPlaybackSessionEvent>? _events;
  static List<VesperExternalPlaybackRoute>? _latestRoutes;

  static List<VesperExternalPlaybackRoute>? get latestRoutes => _latestRoutes;

  static Stream<List<VesperExternalPlaybackRoute>> routesStream() {
    return _nativeRoutes ??= _defaultRoutesEventChannel
        .receiveBroadcastStream()
        .map(_decodeRoutes)
        .transform(
      StreamTransformer<List<VesperExternalPlaybackRoute>,
          List<VesperExternalPlaybackRoute>>.fromHandlers(
        handleData: (routes, sink) {
          if (routes.isEmpty) {
            _latestRoutes = null;
            sink.add(routes);
            return;
          }
          if (_sameRouteLists(_latestRoutes, routes)) {
            return;
          }
          _latestRoutes = List<VesperExternalPlaybackRoute>.unmodifiable(
            routes,
          );
          sink.add(routes);
        },
      ),
    ).asBroadcastStream(
      onCancel: (subscription) {
        _nativeRoutes = null;
        unawaited(subscription.cancel());
      },
    );
  }

  static Stream<VesperExternalPlaybackSessionEvent> eventsStream() {
    return _events ??= _defaultSessionEventChannel
        .receiveBroadcastStream()
        .where((event) => event is Map)
        .map(
          (event) => VesperExternalPlaybackSessionEvent.fromMap(
            Map<Object?, Object?>.from(event as Map),
          ),
        )
        .asBroadcastStream(
      onCancel: (subscription) {
        _events = null;
        unawaited(subscription.cancel());
      },
    );
  }

  static void resetForTests() {
    _latestRoutes = null;
    _nativeRoutes = null;
    _events = null;
  }
}

bool _sameRouteLists(
  List<VesperExternalPlaybackRoute>? previous,
  List<VesperExternalPlaybackRoute> current,
) {
  if (previous == null || previous.length != current.length) {
    return false;
  }
  for (var index = 0; index < current.length; index += 1) {
    if (!mapEquals(previous[index].toMap(), current[index].toMap())) {
      return false;
    }
  }
  return true;
}
