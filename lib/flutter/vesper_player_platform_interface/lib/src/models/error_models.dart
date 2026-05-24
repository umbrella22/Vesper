part of '../models.dart';

final class VesperPlayerError {
  const VesperPlayerError({
    required this.message,
    required this.code,
    required this.category,
    required this.retriable,
    this.details = const <String, Object?>{},
  });

  factory VesperPlayerError.fromMap(Map<Object?, Object?> map) {
    return VesperPlayerError(
      message: map['message'] as String? ?? 'Unknown Vesper player error.',
      code: _decodeRequiredEnum(
        VesperPlayerErrorCode.values,
        map['code'],
        'code',
      ),
      category: _decodeRequiredEnum(
        VesperPlayerErrorCategory.values,
        map['category'],
        'category',
      ),
      retriable: _decodeBool(map, 'retriable'),
      details: _decodeObjectMap(map['details']),
    );
  }

  final String message;
  final VesperPlayerErrorCode code;
  final VesperPlayerErrorCategory category;
  final bool retriable;
  final Map<String, Object?> details;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'message': message,
      'code': code.name,
      'category': category.name,
      'retriable': retriable,
      'details': details,
    };
  }
}

