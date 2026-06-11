part of 'example_player_helpers.dart';

String speedBadge(double rate) => '${formatRate(rate)}x';

String formatBitRate(int value) {
  if (value >= 1000000) {
    return '${(value / 1000000).toStringAsFixed(1)} Mbps';
  }
  if (value >= 1000) {
    return '${(value / 1000).toStringAsFixed(0)} kbps';
  }
  return '$value bps';
}

String formatRate(double value) {
  if ((value - value.roundToDouble()).abs() < 0.001) {
    return value.toStringAsFixed(0);
  }
  if ((value * 10 - (value * 10).roundToDouble()).abs() < 0.001) {
    return value.toStringAsFixed(1);
  }
  return value.toStringAsFixed(2);
}

String formatMillis(int value) {
  final totalSeconds = value ~/ 1000;
  final minutes = totalSeconds ~/ 60;
  final seconds = totalSeconds % 60;
  return '${minutes.toString().padLeft(2, '0')}:${seconds.toString().padLeft(2, '0')}';
}

String bufferWindowLabel(VesperBufferingPolicy policy) {
  final min = policy.minBufferMs;
  final max = policy.maxBufferMs;
  if (min == null || max == null) {
    return 'default';
  }
  return '$min-$max ms';
}

String formatBytes(int? value) {
  if (value == null) {
    return 'default';
  }
  if (value == 0) {
    return '0 B';
  }
  if (value >= 1024 * 1024 * 1024) {
    return '${(value / (1024 * 1024 * 1024)).toStringAsFixed(1)} GB';
  }
  if (value >= 1024 * 1024) {
    return '${(value / (1024 * 1024)).toStringAsFixed(0)} MB';
  }
  if (value >= 1024) {
    return '${(value / 1024).toStringAsFixed(0)} KB';
  }
  return '$value B';
}

String formatDownloadBytes(int? value) {
  if (value == null || value <= 0) {
    return '-';
  }
  if (value >= 1024 * 1024 * 1024) {
    return '${(value / (1024 * 1024 * 1024)).toStringAsFixed(1)} GB';
  }
  if (value >= 1024 * 1024) {
    return '${(value / (1024 * 1024)).toStringAsFixed(1)} MB';
  }
  if (value >= 1024) {
    return '${(value / 1024).toStringAsFixed(0)} KB';
  }
  return '$value B';
}

T? firstWhereOrNull<T>(Iterable<T> values, bool Function(T value) test) {
  for (final value in values) {
    if (test(value)) {
      return value;
    }
  }
  return null;
}
