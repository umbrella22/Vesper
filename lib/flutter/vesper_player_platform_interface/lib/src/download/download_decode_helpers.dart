part of '../download_models.dart';

String? _unknownEnumRawValue(Object? raw, {required bool isUnknown}) {
  return isUnknown && raw is String ? raw : null;
}

VesperDownloadContentFormat _decodeContentFormat(Object? raw) {
  if (raw is String) {
    for (final value in VesperDownloadContentFormat.values) {
      if (value.name == raw) {
        return value;
      }
    }
  }
  return VesperDownloadContentFormat.unknown;
}

VesperDownloadOutputFormat? _decodeOutputFormat(Object? raw) {
  if (raw is String) {
    for (final value in VesperDownloadOutputFormat.values) {
      if (value.name == raw) {
        return value;
      }
    }
  }
  return null;
}

VesperDownloadStreamKind _decodeStreamKind(Object? raw) {
  if (raw == null) {
    return VesperDownloadStreamKind.combined;
  }
  if (raw is String) {
    for (final value in VesperDownloadStreamKind.values) {
      if (value.name == raw) {
        return value;
      }
    }
  }
  return VesperDownloadStreamKind.unknown;
}

VesperDownloadState _decodeDownloadState(Object? raw) {
  if (raw is String) {
    for (final value in VesperDownloadState.values) {
      if (value.name == raw) {
        return value;
      }
    }
  }
  return VesperDownloadState.unknown;
}

VesperDownloadStaleResourcePhase _decodeStaleResourcePhase(Object? raw) {
  if (raw is String) {
    for (final value in VesperDownloadStaleResourcePhase.values) {
      if (value.name == raw) {
        return value;
      }
    }
  }
  return VesperDownloadStaleResourcePhase.prepare;
}

int? _decodeInt(Object? raw) {
  return switch (raw) {
    final int value => value,
    _ => null,
  };
}

List<String> _decodeStringList(Object? raw, String field) {
  if (raw == null) return const <String>[];
  if (raw is! List) throw FormatException('$field must be a list of strings');
  return List<String>.unmodifiable(raw.indexed.map((entry) {
    final (index, value) = entry;
    if (value is! String) {
      throw FormatException('$field[$index] must be a string');
    }
    return value;
  }));
}

List<T> _decodeDownloadRecordList<T>(
  Object? raw,
  String field,
  T Function(Map<Object?, Object?>) decode,
) {
  if (raw == null) return <T>[];
  if (raw is! List) throw FormatException('$field must be a list of maps');
  return List<T>.unmodifiable(raw.indexed.map((entry) {
    final (index, value) = entry;
    if (value is! Map) {
      throw FormatException('$field[$index] must be a map');
    }
    return decode(_decodeRequiredDownloadMap(value, '$field[$index]'));
  }));
}

Map<String, String> _decodeStringMap(Object? raw) {
  if (raw == null) {
    return const <String, String>{};
  }
  if (raw is! Map) throw const FormatException('metadata must be a string map');
  final result = <String, String>{};
  for (final entry in raw.entries) {
    final key = entry.key;
    final value = entry.value;
    if (key is! String || value is! String) {
      throw const FormatException('metadata keys and values must be strings');
    }
    result[key] = value;
  }
  return Map<String, String>.unmodifiable(result);
}

String _decodeRequiredDownloadString(Object? raw, String field) {
  if (raw is! String) throw FormatException('$field must be a string');
  return raw;
}

Map<String, Object?> _decodeDownloadMap(Object? raw, String field) {
  if (raw == null) return const <String, Object?>{};
  return _decodeRequiredDownloadMap(raw, field);
}

Map<String, Object?> _decodeRequiredDownloadMap(Object? raw, String field) {
  if (raw is! Map) throw FormatException('$field must be a map');
  if (raw.keys.any((key) => key is! String)) {
    throw FormatException('$field keys must be strings');
  }
  return Map<String, Object?>.from(raw);
}

int _decodeByteRangeInt(Object? raw, String field) {
  if (raw == null) return 0;
  if (raw is! int) throw FormatException('byteRange.$field must be an integer');
  return raw;
}
