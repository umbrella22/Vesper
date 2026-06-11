part of '../download_models.dart';

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
  if (raw is String) {
    for (final value in VesperDownloadStreamKind.values) {
      if (value.name == raw) {
        return value;
      }
    }
  }
  return VesperDownloadStreamKind.combined;
}

VesperDownloadState _decodeDownloadState(Object? raw) {
  if (raw is String) {
    for (final value in VesperDownloadState.values) {
      if (value.name == raw) {
        return value;
      }
    }
  }
  return VesperDownloadState.queued;
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

List<String> _decodeStringList(Object? raw) {
  return switch (raw) {
    final List<dynamic> values =>
      values.whereType<String>().toList(growable: false),
    _ => const <String>[],
  };
}

Map<String, String> _decodeStringMap(Object? raw) {
  if (raw == null) {
    return const <String, String>{};
  }
  final normalized = vesperDecodeMap(raw);
  return normalized.map(
    (key, value) => MapEntry(key.toString(), value?.toString() ?? ''),
  );
}
