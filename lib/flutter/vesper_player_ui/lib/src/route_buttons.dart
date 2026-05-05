import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:vesper_player/vesper_player.dart';

class VesperAirPlayRouteButton extends StatelessWidget {
  const VesperAirPlayRouteButton({
    super.key,
    required this.controller,
    this.configuration = const VesperRoutePickerConfiguration(),
    this.tintColor,
    this.activeTintColor,
    this.bordered = false,
    this.size = 40,
  });

  final VesperPlayerController controller;
  final VesperRoutePickerConfiguration configuration;
  final Color? tintColor;
  final Color? activeTintColor;
  final bool bordered;
  final double size;

  @override
  Widget build(BuildContext context) {
    if (kIsWeb || defaultTargetPlatform != TargetPlatform.iOS) {
      return SizedBox.square(dimension: size);
    }

    final effectiveTint = tintColor ?? IconTheme.of(context).color;
    final effectiveActiveTint = activeTintColor ?? effectiveTint;
    return SizedBox.square(
      dimension: size,
      child: UiKitView(
        viewType: _airPlayRouteButtonViewType,
        creationParams: <String, Object?>{
          'playerId': controller.playerId,
          ...configuration.toMap(),
          'tintColor': effectiveTint?.toARGB32(),
          'activeTintColor': effectiveActiveTint?.toARGB32(),
          'bordered': bordered,
        },
        creationParamsCodec: const StandardMessageCodec(),
      ),
    );
  }
}

const String _airPlayRouteButtonViewType =
    'io.github.ikaros.vesper_player/airplay_route_button';
