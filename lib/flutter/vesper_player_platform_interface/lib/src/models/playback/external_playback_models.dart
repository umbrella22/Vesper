part of '../../models.dart';

final class VesperExternalPlaybackRouteSnapshot {
  const VesperExternalPlaybackRouteSnapshot({
    this.kind = VesperExternalPlaybackRouteKind.none,
    this.routeId,
    this.routeName,
    this.active = false,
    this.available = false,
  });

  factory VesperExternalPlaybackRouteSnapshot.fromMap(
    Map<Object?, Object?> map,
  ) {
    return VesperExternalPlaybackRouteSnapshot(
      kind: _decodeEnum(
        VesperExternalPlaybackRouteKind.values,
        map['kind'],
        VesperExternalPlaybackRouteKind.none,
      ),
      routeId: map['routeId'] as String?,
      routeName: map['routeName'] as String?,
      active: _decodeBool(map, 'active'),
      available: _decodeBool(map, 'available'),
    );
  }

  final VesperExternalPlaybackRouteKind kind;
  final String? routeId;
  final String? routeName;
  final bool active;
  final bool available;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'kind': kind.name,
      'routeId': routeId,
      'routeName': routeName,
      'active': active,
      'available': available,
    };
  }
}

final class VesperExternalPlaybackAvailability {
  const VesperExternalPlaybackAvailability({
    this.airPlayAvailable = false,
    this.castAvailable = false,
    this.activeRoute = const VesperExternalPlaybackRouteSnapshot(),
  });

  factory VesperExternalPlaybackAvailability.fromMap(
    Map<Object?, Object?> map,
  ) {
    final rawRoute = _rawMap(map['activeRoute']);
    return VesperExternalPlaybackAvailability(
      airPlayAvailable: _decodeBool(map, 'airPlayAvailable'),
      castAvailable: _decodeBool(map, 'castAvailable'),
      activeRoute: rawRoute == null
          ? const VesperExternalPlaybackRouteSnapshot()
          : VesperExternalPlaybackRouteSnapshot.fromMap(rawRoute),
    );
  }

  final bool airPlayAvailable;
  final bool castAvailable;
  final VesperExternalPlaybackRouteSnapshot activeRoute;

  bool get hasAvailableRoute => airPlayAvailable || castAvailable;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'airPlayAvailable': airPlayAvailable,
      'castAvailable': castAvailable,
      'activeRoute': activeRoute.toMap(),
    };
  }
}

final class VesperExternalPlaybackRoute {
  const VesperExternalPlaybackRoute({
    required this.routeId,
    required this.name,
    required this.kind,
    this.manufacturer,
    this.modelName,
    this.active = false,
    this.available = true,
  });

  factory VesperExternalPlaybackRoute.fromMap(Map<Object?, Object?> map) {
    return VesperExternalPlaybackRoute(
      routeId: map['routeId'] as String? ?? '',
      name: map['name'] as String? ?? map['routeName'] as String? ?? '',
      kind: _decodeEnum(
        VesperExternalPlaybackRouteKind.values,
        map['kind'],
        VesperExternalPlaybackRouteKind.none,
      ),
      manufacturer: map['manufacturer'] as String?,
      modelName: map['modelName'] as String?,
      active: _decodeBool(map, 'active'),
      available: _decodeBool(map, 'available', fallback: true),
    );
  }

  final String routeId;
  final String name;
  final VesperExternalPlaybackRouteKind kind;
  final String? manufacturer;
  final String? modelName;
  final bool active;
  final bool available;

  VesperExternalPlaybackRouteSnapshot toSnapshot() {
    return VesperExternalPlaybackRouteSnapshot(
      kind: kind,
      routeId: routeId,
      routeName: name,
      active: active,
      available: available,
    );
  }

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'routeId': routeId,
      'name': name,
      'kind': kind.name,
      'manufacturer': manufacturer,
      'modelName': modelName,
      'active': active,
      'available': available,
    };
  }
}

final class VesperExternalPlaybackMediaItem {
  const VesperExternalPlaybackMediaItem({
    required this.sources,
    required this.metadata,
    this.proxyPolicy = VesperExternalProxyPolicy.auto,
    this.formatAdaptation =
        const VesperExternalFormatAdaptationConfig.disabled(),
  });

  factory VesperExternalPlaybackMediaItem.fromMap(Map<Object?, Object?> map) {
    final rawSources = map['sources'];
    final rawMetadata = _rawMap(map['metadata']);
    final rawFormatAdaptation = _rawMap(map['formatAdaptation']);
    return VesperExternalPlaybackMediaItem(
      sources: rawSources is Iterable
          ? rawSources
              .map(_rawMap)
              .whereType<Map<Object?, Object?>>()
              .map(VesperPlayerSource.fromMap)
              .toList(growable: false)
          : const <VesperPlayerSource>[],
      metadata: VesperSystemPlaybackMetadata.fromMap(
        rawMetadata ?? const <Object?, Object?>{},
      ),
      proxyPolicy: _decodeEnum(
        VesperExternalProxyPolicy.values,
        map['proxyPolicy'],
        VesperExternalProxyPolicy.auto,
      ),
      formatAdaptation: rawFormatAdaptation == null
          ? const VesperExternalFormatAdaptationConfig.disabled()
          : VesperExternalFormatAdaptationConfig.fromMap(rawFormatAdaptation),
    );
  }

  final List<VesperPlayerSource> sources;
  final VesperSystemPlaybackMetadata metadata;
  final VesperExternalProxyPolicy proxyPolicy;
  final VesperExternalFormatAdaptationConfig formatAdaptation;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'sources':
          sources.map((source) => source.toMap()).toList(growable: false),
      'metadata': metadata.toMap(),
      'proxyPolicy': proxyPolicy.name,
      if (formatAdaptation.hasOverrides)
        'formatAdaptation': formatAdaptation.toMap(),
    };
  }
}

final class VesperExternalPlaybackResult {
  const VesperExternalPlaybackResult({
    required this.status,
    this.message,
    this.routeId,
    this.relayEnabled = false,
    this.code,
    this.category,
    this.retriable = false,
    this.details = const <String, String>{},
  });

  factory VesperExternalPlaybackResult.fromMap(Map<Object?, Object?> map) {
    return VesperExternalPlaybackResult(
      status: _decodeEnum(
        VesperExternalPlaybackResultStatus.values,
        map['status'],
        VesperExternalPlaybackResultStatus.failed,
      ),
      message: map['message'] as String?,
      routeId: map['routeId'] as String?,
      relayEnabled: _decodeBool(map, 'relayEnabled'),
      code: map['code'] as String?,
      category: map['category'] as String?,
      retriable: _decodeBool(map, 'retriable'),
      details: _decodeStringMap(map['details']),
    );
  }

  final VesperExternalPlaybackResultStatus status;
  final String? message;
  final String? routeId;
  final bool relayEnabled;
  final String? code;
  final String? category;
  final bool retriable;
  final Map<String, String> details;

  bool get isSuccess => status == VesperExternalPlaybackResultStatus.success;
}

final class VesperExternalPlaybackSessionEvent {
  const VesperExternalPlaybackSessionEvent({
    required this.kind,
    this.routeId,
    this.routeName,
    this.message,
    this.positionMs,
    this.code,
    this.details = const <String, String>{},
  });

  factory VesperExternalPlaybackSessionEvent.fromMap(
    Map<Object?, Object?> map,
  ) {
    return VesperExternalPlaybackSessionEvent(
      kind: _decodeEnum(
        VesperExternalPlaybackSessionEventKind.values,
        map['kind'],
        VesperExternalPlaybackSessionEventKind.error,
      ),
      routeId: map['routeId'] as String?,
      routeName: map['routeName'] as String?,
      message: map['message'] as String?,
      positionMs: (map['positionMs'] as num?)?.toInt(),
      code: map['code'] as String?,
      details: _decodeStringMap(map['details']),
    );
  }

  final VesperExternalPlaybackSessionEventKind kind;
  final String? routeId;
  final String? routeName;
  final String? message;
  final int? positionMs;
  final String? code;
  final Map<String, String> details;
}

final class VesperRoutePickerConfiguration {
  const VesperRoutePickerConfiguration({
    this.prioritizesVideoDevices = true,
  });

  factory VesperRoutePickerConfiguration.fromMap(Map<Object?, Object?> map) {
    return VesperRoutePickerConfiguration(
      prioritizesVideoDevices: _decodeBool(
        map,
        'prioritizesVideoDevices',
        fallback: true,
      ),
    );
  }

  final bool prioritizesVideoDevices;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'prioritizesVideoDevices': prioritizesVideoDevices,
    };
  }
}
