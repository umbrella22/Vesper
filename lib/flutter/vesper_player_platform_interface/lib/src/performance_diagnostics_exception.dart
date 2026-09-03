import 'package:flutter/services.dart';

final class VesperPerformanceDiagnosticsException implements Exception {
  const VesperPerformanceDiagnosticsException({
    required this.code,
    required this.message,
    this.details = const <String, Object?>{},
  });

  factory VesperPerformanceDiagnosticsException.fromPlatformException(
    PlatformException error,
  ) {
    final rawDetails = error.details;
    final details = rawDetails is Map
        ? rawDetails.map((key, value) => MapEntry(key.toString(), value))
        : const <String, Object?>{};
    return VesperPerformanceDiagnosticsException(
      code: details['performanceDiagnosticsCode'] as String? ?? error.code,
      message: error.message ?? 'Performance diagnostics failed.',
      details: details,
    );
  }

  final String code;
  final String message;
  final Map<String, Object?> details;

  @override
  String toString() => 'VesperPerformanceDiagnosticsException($code): $message';
}
