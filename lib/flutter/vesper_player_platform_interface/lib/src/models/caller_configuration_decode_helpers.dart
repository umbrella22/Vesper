part of '../models.dart';

// Caller configurations use closed choices; event/snapshot decoders retain
// their separate unknown/raw-value compatibility behavior.
T _decodeCallerEnum<T extends Enum>(
  Iterable<T> values,
  Object? raw,
  T fallback,
  String field,
) {
  if (raw == null) return fallback;
  if (raw is String) {
    for (final value in values) {
      if (value.name == raw) return value;
    }
  }
  throw FormatException('Invalid $field: $raw');
}

Map<Object?, Object?>? _decodeCallerMap(Object? raw, String field) {
  if (raw == null) return null;
  if (raw is! Map) throw FormatException('$field must be a map');
  final map = Map<Object?, Object?>.from(raw);
  _validateCallerMapKeys(map, field);
  return map;
}

void _validateCallerMapKeys(Map<Object?, Object?> map, String field) {
  if (map.keys.any((key) => key is! String)) {
    throw FormatException('$field keys must be strings');
  }
}

List<T> _decodeCallerMapList<T>(
  Object? raw,
  String field,
  T Function(Map<Object?, Object?>) decode,
) {
  if (raw == null) return <T>[];
  if (raw is! List) throw FormatException('$field must be a list of maps');
  return List<T>.unmodifiable(raw.indexed.map((entry) {
    final (index, value) = entry;
    if (value is! Map) throw FormatException('$field[$index] must be a map');
    return decode(_decodeCallerMap(value, '$field[$index]')!);
  }));
}
