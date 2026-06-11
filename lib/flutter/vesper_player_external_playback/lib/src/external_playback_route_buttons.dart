part of 'vesper_external_playback_controller.dart';

class VesperExternalRouteButton extends StatelessWidget {
  const VesperExternalRouteButton({
    super.key,
    this.size = 40,
    this.brightness,
  });

  final double size;
  final Brightness? brightness;

  @override
  Widget build(BuildContext context) {
    return VesperExternalRouteIconButton(
      size: size,
      brightness: brightness,
    );
  }
}

class VesperExternalRouteIconButton extends StatelessWidget {
  const VesperExternalRouteIconButton({
    super.key,
    this.size = 38,
    this.brightness,
  });

  final double size;
  final Brightness? brightness;

  @override
  Widget build(BuildContext context) {
    if (kIsWeb || defaultTargetPlatform != TargetPlatform.android) {
      return SizedBox.square(dimension: size);
    }
    final effectiveBrightness = brightness ?? Theme.of(context).brightness;
    return SizedBox.square(
      dimension: size,
      child: AndroidView(
        key: ValueKey<Brightness>(effectiveBrightness),
        viewType: _routeButtonViewType,
        creationParams: <String, Object?>{
          'brightness': effectiveBrightness.name,
        },
        creationParamsCodec: const StandardMessageCodec(),
      ),
    );
  }
}
