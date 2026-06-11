part of 'vesper_external_playback_controller.dart';

Map<Object?, Object?>? _rawMap(Object? raw) {
  if (raw is Map<Object?, Object?>) {
    return raw;
  }
  if (raw is Map) {
    return Map<Object?, Object?>.from(raw);
  }
  return null;
}

List<VesperExternalPlaybackRoute> _decodeRoutes(Object? event) {
  if (event is! Iterable) {
    return const <VesperExternalPlaybackRoute>[];
  }
  return event
      .map(_rawMap)
      .whereType<Map<Object?, Object?>>()
      .map(VesperExternalPlaybackRoute.fromMap)
      .toList(growable: false);
}
