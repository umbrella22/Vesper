part of '../../models.dart';

List<String> _decodeCapabilityList(Object? raw) {
  if (raw is String) {
    return raw
        .split(',')
        .map((value) => value.trim())
        .where((value) => value.isNotEmpty)
        .toList(growable: false);
  }
  return _decodeStringList(raw);
}

bool? _decodeOptionalFlexibleBool(Object? raw) {
  if (raw is bool) {
    return raw;
  }
  if (raw is String) {
    return raw == 'true'
        ? true
        : raw == 'false'
            ? false
            : null;
  }
  return null;
}

int? _decodeFlexibleInt(Object? raw) {
  if (raw is int) {
    return raw;
  }
  if (raw is num) {
    return raw.toInt();
  }
  if (raw is String) {
    return int.tryParse(raw);
  }
  return null;
}

double? _decodeFlexibleDouble(Object? raw) {
  if (raw is num) {
    return raw.toDouble();
  }
  if (raw is String) {
    return double.tryParse(raw);
  }
  return null;
}
