part of '../models.dart';

final class VesperPlayerViewport {
  const VesperPlayerViewport({
    required this.left,
    required this.top,
    required this.width,
    required this.height,
  });

  factory VesperPlayerViewport.fromMap(Map<Object?, Object?> map) {
    return VesperPlayerViewport(
      left: _decodeDouble(map, 'left') ?? 0,
      top: _decodeDouble(map, 'top') ?? 0,
      width: _decodeDouble(map, 'width') ?? 0,
      height: _decodeDouble(map, 'height') ?? 0,
    );
  }

  final double left;
  final double top;
  final double width;
  final double height;

  bool get isEmpty => width <= 0 || height <= 0;

  VesperViewportHint classifyHint({
    required double surfaceWidth,
    required double surfaceHeight,
  }) {
    if (isEmpty || surfaceWidth <= 0 || surfaceHeight <= 0) {
      return const VesperViewportHint.hidden();
    }

    final right = left + width;
    final bottom = top + height;
    final visibleWidth = _overlapExtent(left, right, 0, surfaceWidth);
    final visibleHeight = _overlapExtent(top, bottom, 0, surfaceHeight);
    final visibleArea = visibleWidth * visibleHeight;
    final totalArea = width * height;
    final visibleFraction =
        totalArea <= 0 ? 0.0 : _clampUnit(visibleArea / totalArea);

    if (visibleArea > 0) {
      return VesperViewportHint(
        kind: VesperViewportHintKind.visible,
        visibleFraction: visibleFraction,
      );
    }

    final dx = _axisGap(left, right, 0, surfaceWidth);
    final dy = _axisGap(top, bottom, 0, surfaceHeight);
    final edgeDistance = math.sqrt(dx * dx + dy * dy);
    final reference = math.max(surfaceWidth, surfaceHeight);
    final kind = edgeDistance <= reference * 0.5
        ? VesperViewportHintKind.nearVisible
        : edgeDistance <= reference * 1.5
            ? VesperViewportHintKind.prefetchOnly
            : VesperViewportHintKind.hidden;

    return VesperViewportHint(kind: kind, visibleFraction: 0);
  }

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'left': left,
      'top': top,
      'width': width,
      'height': height,
    };
  }
}

enum VesperViewportHintKind { visible, nearVisible, prefetchOnly, hidden }

final class VesperViewportHint {
  const VesperViewportHint({
    required this.kind,
    this.visibleFraction = 0,
  });

  const VesperViewportHint.hidden()
      : kind = VesperViewportHintKind.hidden,
        visibleFraction = 0;

  factory VesperViewportHint.fromMap(Map<Object?, Object?> map) {
    return VesperViewportHint(
      kind: _decodeEnum(
        VesperViewportHintKind.values,
        map['kind'],
        VesperViewportHintKind.hidden,
      ),
      visibleFraction: _clampUnit(_decodeDouble(map, 'visibleFraction') ?? 0),
    );
  }

  final VesperViewportHintKind kind;
  final double visibleFraction;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'kind': kind.name,
      'visibleFraction': visibleFraction,
    };
  }
}

