part of '../models.dart';

/// Native presentation dimensions after rotation and pixel aspect correction.
/// Independent of the size and lifetime of a playback view.
final class VesperVideoPresentation {
  const VesperVideoPresentation({
    required this.displayWidth,
    required this.displayHeight,
  });

  factory VesperVideoPresentation.fromMap(Map<Object?, Object?> map) {
    return VesperVideoPresentation(
      displayWidth: _videoGeometryNumber(map, 'displayWidth', positive: true),
      displayHeight: _videoGeometryNumber(map, 'displayHeight', positive: true),
    );
  }

  final double displayWidth;
  final double displayHeight;

  double get displayAspectRatio => displayWidth / displayHeight;

  Map<String, Object?> toMap() => <String, Object?>{
        'displayWidth': displayWidth,
        'displayHeight': displayHeight,
      };
}

/// Geometry in logical pixels from the attached player's top-left corner.
final class VesperVideoSurfaceGeometry {
  const VesperVideoSurfaceGeometry({
    required this.width,
    required this.height,
    required this.contentRect,
  });

  factory VesperVideoSurfaceGeometry.fromMap(Map<Object?, Object?> map) =>
      VesperVideoSurfaceGeometry(
        width: _videoGeometryNumber(map, 'width', positive: true),
        height: _videoGeometryNumber(map, 'height', positive: true),
        contentRect:
            VesperVideoRect.fromMap(vesperDecodeMap(map['contentRect'])),
      );

  final double width;
  final double height;
  final VesperVideoRect contentRect;

  Map<String, Object?> toMap() => <String, Object?>{
        'width': width,
        'height': height,
        'contentRect': contentRect.toMap(),
      };
}

/// The displayed picture, excluding black bars added by the renderer.
final class VesperVideoRect {
  const VesperVideoRect({
    required this.left,
    required this.top,
    required this.width,
    required this.height,
  });

  factory VesperVideoRect.fromMap(Map<Object?, Object?> map) => VesperVideoRect(
        left: _videoGeometryNumber(map, 'left'),
        top: _videoGeometryNumber(map, 'top'),
        width: _videoGeometryNumber(map, 'width', positive: true),
        height: _videoGeometryNumber(map, 'height', positive: true),
      );

  final double left;
  final double top;
  final double width;
  final double height;

  bool contains(double x, double y) =>
      x >= left && y >= top && x < left + width && y < top + height;

  Map<String, Object?> toMap() => <String, Object?>{
        'left': left,
        'top': top,
        'width': width,
        'height': height,
      };
}

// Unknown native geometry is represented by a null event, never a zero-sized
// rectangle. Reject malformed channel values before clients divide by them.
double _videoGeometryNumber(Map<Object?, Object?> map, String key,
    {bool positive = false}) {
  final value = _decodeDouble(map, key);
  if (value == null || !value.isFinite || (positive && value <= 0)) {
    throw FormatException('Invalid video geometry field: $key', map[key]);
  }
  return value;
}
