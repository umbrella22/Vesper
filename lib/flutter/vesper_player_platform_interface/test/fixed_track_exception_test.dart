import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

void main() {
  test('decodes fixed-track rejection and revision evidence', () {
    final error = PlatformException(
      code: 'vesper_operation_failed',
      message: 'catalog changed',
      details: <String, Object?>{
        'domain': 'fixedTrack',
        'code': 'staleCatalog',
        'trackId': 'video:720p',
        'expectedCatalogRevision': 4,
        'actualCatalogRevision': 5,
        'message': 'catalog changed',
      },
    );

    final mapped = vesperMapPlatformException(error);
    expect(mapped, isA<VesperFixedTrackSelectionException>());
    final exception = mapped as VesperFixedTrackSelectionException;
    expect(exception.code, VesperFixedTrackSelectionErrorCode.staleCatalog);
    expect(exception.codeRawValue, 'staleCatalog');
    expect(exception.trackId, 'video:720p');
    expect(exception.expectedCatalogRevision, 4);
    expect(exception.actualCatalogRevision, 5);
  });

  test('preserves an unknown fixed-track code and does not infer stale', () {
    final error = PlatformException(
      code: 'vesper_operation_failed',
      details: <String, Object?>{
        'domain': 'fixedTrack',
        'code': 'futureTrackDecision',
        'trackId': 'video:1080p',
        'actualCatalogRevision': 9,
      },
    );

    final mapped = vesperMapPlatformException(error);
    expect(mapped, isA<VesperFixedTrackSelectionException>());
    final exception = mapped as VesperFixedTrackSelectionException;
    expect(exception.code, VesperFixedTrackSelectionErrorCode.unknown);
    expect(exception.codeRawValue, 'futureTrackDecision');
    expect(exception.expectedCatalogRevision, isNull);
    expect(exception.actualCatalogRevision, 9);
  });

  test('does not reinterpret a generic ABR command failure as fixed-track', () {
    final error = PlatformException(
      code: 'vesper_operation_failed',
      message: 'constraints are required',
      details: <String, Object?>{
        'code': 'invalidArgument',
        'category': 'input',
        'details': <String, Object?>{
          'domain': 'abrPolicy',
          'operation': 'setAbrPolicy',
        },
      },
    );

    expect(vesperMapPlatformException(error), same(error));
  });
}
