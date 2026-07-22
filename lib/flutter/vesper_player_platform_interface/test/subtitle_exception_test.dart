import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

void main() {
  test('decodes subtitle transaction metadata from PlatformException', () {
    final platformError = PlatformException(
      code: 'vesper_subtitle_error',
      message: 'selection did not converge',
      details: <String, Object?>{
        'domain': 'subtitle',
        'code': 'subtitle_selection_timeout',
        'phase': 'selection',
        'trackId': 'subtitle:en',
        'retriable': true,
        'commandId': 12,
        'sourceEpoch': 9,
      },
    );

    final exception = VesperSubtitleException.fromPlatformException(
      platformError,
    );

    expect(exception.code, 'subtitle_selection_timeout');
    expect(exception.phase, VesperSubtitleErrorPhase.selection);
    expect(exception.trackId, 'subtitle:en');
    expect(exception.retriable, isTrue);
    expect(exception.commandId, 12);
    expect(exception.sourceEpoch, 9);
    expect(exception.message, 'selection did not converge');
    expect(
      vesperMapPlatformException(platformError),
      isA<VesperSubtitleException>(),
    );
  });

  test('does not classify an unrelated error by a subtitle-looking code', () {
    final platformError = PlatformException(
      code: 'subtitle_transport_failure',
      message: 'generic platform failure',
      details: <String, Object?>{
        'code': 'subtitle_transport_failure',
        'phase': 'resource',
      },
    );

    expect(vesperMapPlatformException(platformError), same(platformError));
  });

  test('platform error mapping leaves unrelated failures unchanged', () {
    final platformError = PlatformException(
      code: 'unsupported',
      message: 'not available',
    );

    expect(vesperMapPlatformException(platformError), same(platformError));
  });

  test('subtitle domain preserves unknown code and phase raw values', () {
    final platformError = PlatformException(
      code: 'vesper_subtitle_error',
      message: 'future subtitle failure',
      details: <String, Object?>{
        'domain': 'subtitle',
        'code': 'future_subtitle_code',
        'phase': 'future_phase',
        'retriable': false,
      },
    );

    final mapped = vesperMapPlatformException(platformError);
    expect(mapped, isA<VesperSubtitleException>());
    final exception = mapped as VesperSubtitleException;
    expect(exception.code, 'future_subtitle_code');
    expect(exception.phase, VesperSubtitleErrorPhase.unknown);
    expect(exception.phaseRawValue, 'future_phase');
  });
}
