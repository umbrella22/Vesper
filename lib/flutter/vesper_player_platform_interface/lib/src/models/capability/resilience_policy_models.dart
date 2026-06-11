part of '../../models.dart';

final class VesperPreloadBudgetPolicy {
  const VesperPreloadBudgetPolicy({
    this.maxConcurrentTasks,
    this.maxMemoryBytes,
    this.maxDiskBytes,
    this.warmupWindowMs,
  });

  factory VesperPreloadBudgetPolicy.fromMap(Map<Object?, Object?> map) {
    return VesperPreloadBudgetPolicy(
      maxConcurrentTasks: _decodeInt(map, 'maxConcurrentTasks'),
      maxMemoryBytes: _decodeInt(map, 'maxMemoryBytes'),
      maxDiskBytes: _decodeInt(map, 'maxDiskBytes'),
      warmupWindowMs: _decodeInt(map, 'warmupWindowMs'),
    );
  }

  final int? maxConcurrentTasks;
  final int? maxMemoryBytes;
  final int? maxDiskBytes;
  final int? warmupWindowMs;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      if (maxConcurrentTasks != null) 'maxConcurrentTasks': maxConcurrentTasks,
      if (maxMemoryBytes != null) 'maxMemoryBytes': maxMemoryBytes,
      if (maxDiskBytes != null) 'maxDiskBytes': maxDiskBytes,
      if (warmupWindowMs != null) 'warmupWindowMs': warmupWindowMs,
    };
  }
}

final class VesperBenchmarkConfiguration {
  const VesperBenchmarkConfiguration({
    this.enabled = false,
    this.maxBufferedEvents = 2048,
    this.includeRawEvents = true,
    this.consoleLogging = false,
    this.pluginLibraryPaths = const <String>[],
  });

  const VesperBenchmarkConfiguration.disabled()
      : enabled = false,
        maxBufferedEvents = 2048,
        includeRawEvents = true,
        consoleLogging = false,
        pluginLibraryPaths = const <String>[];

  factory VesperBenchmarkConfiguration.fromMap(Map<Object?, Object?> map) {
    final normalized = vesperDecodeMap(map);
    return VesperBenchmarkConfiguration(
      enabled: normalized['enabled'] as bool? ?? false,
      maxBufferedEvents: normalized['maxBufferedEvents'] as int? ?? 2048,
      includeRawEvents: normalized['includeRawEvents'] as bool? ?? true,
      consoleLogging: normalized['consoleLogging'] as bool? ?? false,
      pluginLibraryPaths: _decodeStringList(normalized['pluginLibraryPaths']),
    );
  }

  final bool enabled;
  final int maxBufferedEvents;
  final bool includeRawEvents;
  final bool consoleLogging;
  final List<String> pluginLibraryPaths;

  bool get hasOverrides =>
      enabled ||
      maxBufferedEvents != 2048 ||
      !includeRawEvents ||
      consoleLogging ||
      pluginLibraryPaths.isNotEmpty;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'enabled': enabled,
      'maxBufferedEvents': maxBufferedEvents,
      'includeRawEvents': includeRawEvents,
      'consoleLogging': consoleLogging,
      'pluginLibraryPaths': pluginLibraryPaths,
    };
  }
}

final class VesperBufferingPolicy {
  const VesperBufferingPolicy({
    this.preset = VesperBufferingPreset.defaultPreset,
    this.minBufferMs,
    this.maxBufferMs,
    this.bufferForPlaybackMs,
    this.bufferForPlaybackAfterRebufferMs,
  });

  const VesperBufferingPolicy.balanced()
      : preset = VesperBufferingPreset.balanced,
        minBufferMs = null,
        maxBufferMs = null,
        bufferForPlaybackMs = null,
        bufferForPlaybackAfterRebufferMs = null;

  const VesperBufferingPolicy.streaming()
      : preset = VesperBufferingPreset.streaming,
        minBufferMs = null,
        maxBufferMs = null,
        bufferForPlaybackMs = null,
        bufferForPlaybackAfterRebufferMs = null;

  const VesperBufferingPolicy.resilient()
      : preset = VesperBufferingPreset.resilient,
        minBufferMs = null,
        maxBufferMs = null,
        bufferForPlaybackMs = null,
        bufferForPlaybackAfterRebufferMs = null;

  const VesperBufferingPolicy.lowLatency()
      : preset = VesperBufferingPreset.lowLatency,
        minBufferMs = null,
        maxBufferMs = null,
        bufferForPlaybackMs = null,
        bufferForPlaybackAfterRebufferMs = null;

  factory VesperBufferingPolicy.fromMap(Map<Object?, Object?> map) {
    return VesperBufferingPolicy(
      preset: _decodeEnum(
        VesperBufferingPreset.values,
        map['preset'],
        VesperBufferingPreset.defaultPreset,
      ),
      minBufferMs: _decodeInt(map, 'minBufferMs'),
      maxBufferMs: _decodeInt(map, 'maxBufferMs'),
      bufferForPlaybackMs: _decodeInt(map, 'bufferForPlaybackMs'),
      bufferForPlaybackAfterRebufferMs: _decodeInt(
        map,
        'bufferForPlaybackAfterRebufferMs',
      ),
    );
  }

  final VesperBufferingPreset preset;
  final int? minBufferMs;
  final int? maxBufferMs;
  final int? bufferForPlaybackMs;
  final int? bufferForPlaybackAfterRebufferMs;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'preset': preset.name,
      'minBufferMs': minBufferMs,
      'maxBufferMs': maxBufferMs,
      'bufferForPlaybackMs': bufferForPlaybackMs,
      'bufferForPlaybackAfterRebufferMs': bufferForPlaybackAfterRebufferMs,
    };
  }
}

final class VesperRetryPolicy {
  const VesperRetryPolicy({
    Object? maxAttempts = _vesperRetryMaxAttemptsUnset,
    int? baseDelayMs,
    int? maxDelayMs,
    VesperRetryBackoff? backoff,
  })  : _maxAttempts = maxAttempts,
        _baseDelayMs = baseDelayMs,
        _maxDelayMs = maxDelayMs,
        _backoff = backoff;

  const VesperRetryPolicy.aggressive()
      : _maxAttempts = 2,
        _baseDelayMs = 500,
        _maxDelayMs = 2000,
        _backoff = VesperRetryBackoff.fixed;

  const VesperRetryPolicy.resilient()
      : _maxAttempts = 6,
        _baseDelayMs = 1000,
        _maxDelayMs = 8000,
        _backoff = VesperRetryBackoff.exponential;

  factory VesperRetryPolicy.fromMap(Map<Object?, Object?> map) {
    final rawMaxAttempts = map['maxAttempts'];
    final maxAttempts = switch (rawMaxAttempts) {
      int value => value,
      null when map.containsKey('maxAttempts') => null,
      _ => _vesperRetryMaxAttemptsUnset,
    };
    return VesperRetryPolicy(
      maxAttempts: maxAttempts,
      baseDelayMs: _decodeInt(map, 'baseDelayMs'),
      maxDelayMs: _decodeInt(map, 'maxDelayMs'),
      backoff: switch (map['backoff']) {
        'fixed' => VesperRetryBackoff.fixed,
        'linear' => VesperRetryBackoff.linear,
        'exponential' => VesperRetryBackoff.exponential,
        _ => null,
      },
    );
  }

  final Object? _maxAttempts;
  final int? _baseDelayMs;
  final int? _maxDelayMs;
  final VesperRetryBackoff? _backoff;

  int? get maxAttempts => switch (_maxAttempts) {
        _vesperRetryMaxAttemptsUnset => 3,
        int value => value,
        null => null,
        _ => 3,
      };

  bool get hasMaxAttemptsOverride =>
      !identical(_maxAttempts, _vesperRetryMaxAttemptsUnset);

  int get baseDelayMs => _baseDelayMs ?? 1000;
  int get maxDelayMs => _maxDelayMs ?? 5000;
  VesperRetryBackoff get backoff => _backoff ?? VesperRetryBackoff.linear;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      if (hasMaxAttemptsOverride) 'maxAttempts': _maxAttempts as int?,
      'baseDelayMs': _baseDelayMs,
      'maxDelayMs': _maxDelayMs,
      'backoff': _backoff?.name,
    };
  }
}

final class VesperCachePolicy {
  const VesperCachePolicy({
    this.preset = VesperCachePreset.defaultPreset,
    this.maxMemoryBytes,
    this.maxDiskBytes,
  });

  const VesperCachePolicy.disabled()
      : preset = VesperCachePreset.disabled,
        maxMemoryBytes = null,
        maxDiskBytes = null;

  const VesperCachePolicy.streaming()
      : preset = VesperCachePreset.streaming,
        maxMemoryBytes = null,
        maxDiskBytes = null;

  const VesperCachePolicy.resilient()
      : preset = VesperCachePreset.resilient,
        maxMemoryBytes = null,
        maxDiskBytes = null;

  factory VesperCachePolicy.fromMap(Map<Object?, Object?> map) {
    return VesperCachePolicy(
      preset: _decodeEnum(
        VesperCachePreset.values,
        map['preset'],
        VesperCachePreset.defaultPreset,
      ),
      maxMemoryBytes: _decodeInt(map, 'maxMemoryBytes'),
      maxDiskBytes: _decodeInt(map, 'maxDiskBytes'),
    );
  }

  final VesperCachePreset preset;
  final int? maxMemoryBytes;
  final int? maxDiskBytes;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'preset': preset.name,
      'maxMemoryBytes': maxMemoryBytes,
      'maxDiskBytes': maxDiskBytes,
    };
  }
}

final class VesperPlaybackResiliencePolicy {
  const VesperPlaybackResiliencePolicy({
    this.buffering = const VesperBufferingPolicy(),
    this.retry = const VesperRetryPolicy(),
    this.cache = const VesperCachePolicy(),
  });

  const VesperPlaybackResiliencePolicy.balanced()
      : buffering = const VesperBufferingPolicy.balanced(),
        retry = const VesperRetryPolicy(),
        cache = const VesperCachePolicy.streaming();

  const VesperPlaybackResiliencePolicy.streaming()
      : buffering = const VesperBufferingPolicy.streaming(),
        retry = const VesperRetryPolicy(),
        cache = const VesperCachePolicy.streaming();

  const VesperPlaybackResiliencePolicy.resilient()
      : buffering = const VesperBufferingPolicy.resilient(),
        retry = const VesperRetryPolicy.resilient(),
        cache = const VesperCachePolicy.resilient();

  const VesperPlaybackResiliencePolicy.lowLatency()
      : buffering = const VesperBufferingPolicy.lowLatency(),
        retry = const VesperRetryPolicy.aggressive(),
        cache = const VesperCachePolicy.disabled();

  factory VesperPlaybackResiliencePolicy.fromMap(Map<Object?, Object?> map) {
    final rawBuffering = map['buffering'];
    final rawRetry = map['retry'];
    final rawCache = map['cache'];
    final buffering = _rawMap(rawBuffering);
    final retry = _rawMap(rawRetry);
    final cache = _rawMap(rawCache);
    return VesperPlaybackResiliencePolicy(
      buffering: buffering != null
          ? VesperBufferingPolicy.fromMap(buffering)
          : const VesperBufferingPolicy(),
      retry: retry != null
          ? VesperRetryPolicy.fromMap(retry)
          : const VesperRetryPolicy(),
      cache: cache != null
          ? VesperCachePolicy.fromMap(cache)
          : const VesperCachePolicy(),
    );
  }

  final VesperBufferingPolicy buffering;
  final VesperRetryPolicy retry;
  final VesperCachePolicy cache;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'buffering': buffering.toMap(),
      'retry': retry.toMap(),
      'cache': cache.toMap(),
    };
  }
}
