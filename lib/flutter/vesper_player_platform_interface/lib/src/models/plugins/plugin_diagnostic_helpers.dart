part of '../../models.dart';

bool _decodeBool(
  Map<Object?, Object?> map,
  String key, {
  bool fallback = false,
}) {
  final raw = map[key];
  return raw is bool ? raw : fallback;
}

bool? _decodeOptionalBool(Map<Object?, Object?> map, String key) {
  final raw = map[key];
  return raw is bool ? raw : null;
}

int? _decodeInt(Map<Object?, Object?> map, String key) {
  final raw = map[key];
  return raw is int ? raw : null;
}

double? _decodeDouble(Map<Object?, Object?> map, String key) {
  final raw = map[key];
  if (raw is double) {
    return raw;
  }
  if (raw is int) {
    return raw.toDouble();
  }
  return null;
}

Map<String, Object?> _toStringKeyedMap(Map<Object?, Object?> source) {
  return source.map((key, value) => MapEntry(key.toString(), value));
}

Map<Object?, Object?>? _rawMap(Object? raw) {
  if (raw is Map<Object?, Object?>) {
    return raw;
  }
  if (raw is Map) {
    return Map<Object?, Object?>.from(raw);
  }
  return null;
}

Map<String, String> _decodeStringMap(Object? raw) {
  final map = _rawMap(raw);
  if (map == null || map.isEmpty) {
    return const <String, String>{};
  }

  final decoded = <String, String>{};
  for (final entry in map.entries) {
    final key = entry.key;
    final value = entry.value;
    if (key is String && value is String) {
      decoded[key] = value;
    }
  }
  return decoded;
}

Map<String, Object?> _decodeObjectMap(Object? raw) {
  final map = _rawMap(raw);
  if (map == null || map.isEmpty) {
    return const <String, Object?>{};
  }
  return _toStringKeyedMap(map);
}

Set<String> _decodeStringSet(
  Object? raw, {
  Set<String> fallback = const <String>{},
}) {
  if (raw is! Iterable) {
    return fallback;
  }
  final decoded =
      raw.whereType<String>().where((value) => value.isNotEmpty).toSet();
  return decoded.isEmpty ? fallback : decoded;
}

List<String> _decodeStringList(Object? raw) {
  if (raw is! Iterable) {
    return const <String>[];
  }
  return raw
      .map((value) => value?.toString() ?? '')
      .where((value) => value.isNotEmpty)
      .toList(growable: false);
}

const Object _vesperRetryMaxAttemptsUnset = Object();
