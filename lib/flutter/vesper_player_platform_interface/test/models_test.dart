import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

void main() {
  test(
    'default retry policy keeps fallback getters but omits channel overrides',
    () {
      const policy = VesperRetryPolicy();

      expect(policy.maxAttempts, 3);
      expect(policy.baseDelayMs, 1000);
      expect(policy.maxDelayMs, 5000);
      expect(policy.backoff, VesperRetryBackoff.linear);
      expect(policy.toMap(), <String, Object?>{
        'maxAttempts': 3,
        'baseDelayMs': null,
        'maxDelayMs': null,
        'backoff': null,
      });
    },
  );

  test('retry policy fromMap keeps explicit overrides only', () {
    final policy = VesperRetryPolicy.fromMap(<Object?, Object?>{
      'maxAttempts': 6,
      'maxDelayMs': 8000,
      'backoff': 'exponential',
    });

    expect(policy.maxAttempts, 6);
    expect(policy.baseDelayMs, 1000);
    expect(policy.maxDelayMs, 8000);
    expect(policy.backoff, VesperRetryBackoff.exponential);
    expect(policy.toMap(), <String, Object?>{
      'maxAttempts': 6,
      'baseDelayMs': null,
      'maxDelayMs': 8000,
      'backoff': 'exponential',
    });
  });

  test('buffering preset constructors only serialize preset names', () {
    expect(const VesperBufferingPolicy.resilient().toMap(), <String, Object?>{
      'preset': 'resilient',
      'minBufferMs': null,
      'maxBufferMs': null,
      'bufferForPlaybackMs': null,
      'bufferForPlaybackAfterRebufferMs': null,
    });
  });

  test('cache preset constructors only serialize preset names', () {
    expect(const VesperCachePolicy.streaming().toMap(), <String, Object?>{
      'preset': 'streaming',
      'maxMemoryBytes': null,
      'maxDiskBytes': null,
    });
  });

  test(
      'resilience preset serialization keeps shared values out of buffering/cache',
      () {
    expect(
      const VesperPlaybackResiliencePolicy.resilient().toMap(),
      <String, Object?>{
        'buffering': <String, Object?>{
          'preset': 'resilient',
          'minBufferMs': null,
          'maxBufferMs': null,
          'bufferForPlaybackMs': null,
          'bufferForPlaybackAfterRebufferMs': null,
        },
        'retry': <String, Object?>{
          'maxAttempts': 6,
          'baseDelayMs': 1000,
          'maxDelayMs': 8000,
          'backoff': 'exponential',
        },
        'cache': <String, Object?>{
          'preset': 'resilient',
          'maxMemoryBytes': null,
          'maxDiskBytes': null,
        },
      },
    );
  });
}
