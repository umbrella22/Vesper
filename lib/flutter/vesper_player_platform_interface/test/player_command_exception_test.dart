import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

void main() {
  test('decodes structured player command failures', () {
    final mapped = vesperMapPlatformException(
      PlatformException(
        code: 'vesper_operation_failed',
        message: 'source command was superseded',
        details: <String, Object?>{
          'message': 'source command was superseded',
          'code': 'cancelled',
          'category': 'source',
          'retriable': true,
          'details': <String, Object?>{
            'reason': 'networkUnavailable',
            'commandReason': 'sourceCommandSuperseded',
            'obsolete': true,
            'commandId': 12,
            'sourceEpoch': 4,
          },
        },
      ),
    );

    expect(mapped, isA<VesperPlayerCommandException>());
    final exception = mapped as VesperPlayerCommandException;
    expect(exception.code, VesperPlayerErrorCode.cancelled);
    expect(exception.category, VesperPlayerErrorCategory.source);
    expect(exception.retriable, isTrue);
    expect(exception.details['reason'], 'networkUnavailable');
    expect(exception.details['commandReason'], 'sourceCommandSuperseded');
    expect(exception.details['commandId'], 12);
    expect(exception.isObsolete, isTrue);
  });

  test('does not treat an unnormalized native obsolete string as boolean', () {
    final mapped = vesperMapPlatformException(
      PlatformException(
        code: 'vesper_operation_failed',
        details: <String, Object?>{
          'message': 'source command was superseded',
          'code': 'cancelled',
          'category': 'source',
          'retriable': true,
          'details': <String, Object?>{
            'commandReason': 'sourceCommandSuperseded',
            'obsolete': 'true',
            'commandId': '12',
            'sourceEpoch': '4',
          },
        },
      ),
    ) as VesperPlayerCommandException;

    expect(mapped.details['obsolete'], 'true');
    expect(mapped.isObsolete, isFalse);
  });

  test('preserves unknown command code and category raw values', () {
    final mapped = vesperMapPlatformException(
      PlatformException(
        code: 'vesper_operation_failed',
        details: <String, Object?>{
          'message': 'future command failure',
          'code': 'futureCommandCode',
          'category': 'futureCategory',
          'retriable': false,
          'details': <String, Object?>{
            'reason': 'futureReason',
            'commandReason': 'futureCommandReason',
            'commandId': '18446744073709551615',
            'sourceEpoch': '7',
          },
        },
      ),
    ) as VesperPlayerCommandException;

    expect(mapped.code, VesperPlayerErrorCode.unknown);
    expect(mapped.category, VesperPlayerErrorCategory.unknown);
    expect(mapped.codeRawValue, 'futureCommandCode');
    expect(mapped.categoryRawValue, 'futureCategory');
    expect(mapped.details['commandId'], '18446744073709551615');
  });

  test('requires complete nested command metadata', () {
    final errors = <PlatformException>[
      PlatformException(
        code: 'vesper_operation_failed',
        details: <String, Object?>{
          'code': 'cancelled',
          'category': 'source',
          'details': <String, Object?>{
            'commandId': 12,
            'sourceEpoch': 4,
          },
        },
      ),
      PlatformException(
        code: 'vesper_operation_failed',
        details: <String, Object?>{
          'code': 'cancelled',
          'category': 'source',
          'details': <String, Object?>{
            'commandReason': 'sourceCommandSuperseded',
            'commandId': 12,
          },
        },
      ),
      PlatformException(
        code: 'vesper_operation_failed',
        details: <String, Object?>{
          'code': 'cancelled',
          'category': 'source',
          'details': <String, Object?>{
            'commandReason': 'sourceCommandSuperseded',
            'commandId': 'not-an-id',
            'sourceEpoch': 4,
          },
        },
      ),
    ];

    for (final error in errors) {
      expect(vesperMapPlatformException(error), same(error));
    }
  });

  test('leaves unstructured platform failures unchanged', () {
    final error = PlatformException(
      code: 'vesper_operation_failed',
      message: 'legacy failure',
    );

    expect(vesperMapPlatformException(error), same(error));
  });
}
