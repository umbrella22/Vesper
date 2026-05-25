import 'dart:math' as math;
part 'models/system_models.dart';
part 'models/external_playback_models.dart';
part 'models/playback_models.dart';
part 'models/resilience_models.dart';
part 'models/runtime_warning_models.dart';
part 'models/plugin_diagnostic_models.dart';
part 'models/mobile_plugin_configuration_models.dart';
part 'models/source_models.dart';
part 'models/capability_models.dart';
part 'models/viewport_models.dart';
part 'models/error_models.dart';
part 'models/snapshot_models.dart';

enum VesperPlayerSourceKind { local, remote }

enum VesperPlayerSourceProtocol {
  unknown,
  file,
  content,
  progressive,
  hls,
  dash,
}

enum VesperPlaybackState { ready, playing, paused, finished }

enum VesperTimelineKind { vod, live, liveDvr }

enum VesperPlayerBackendFamily {
  unknown,
  androidHostKit,
  iosHostKit,
  macosFfi,
  softwareFallback,
  fakeDemo,
}

enum VesperPlayerRenderSurfaceKind { auto, textureView, surfaceView }

Map<String, Object?> vesperDecodeMap(Object? raw) {
  final decoded = _rawMap(raw);
  if (decoded != null) {
    return _toStringKeyedMap(decoded);
  }
  return <String, Object?>{};
}

double _overlapExtent(
  double startA,
  double endA,
  double startB,
  double endB,
) {
  return math.max(0, math.min(endA, endB) - math.max(startA, startB));
}

double _axisGap(
  double startA,
  double endA,
  double startB,
  double endB,
) {
  if (endA < startB) {
    return startB - endA;
  }
  if (endB < startA) {
    return startA - endB;
  }
  return 0;
}

double _clampUnit(double value) => value.clamp(0.0, 1.0).toDouble();
