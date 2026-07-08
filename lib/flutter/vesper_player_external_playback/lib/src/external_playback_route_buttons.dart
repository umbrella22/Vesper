part of 'vesper_external_playback_controller.dart';

class VesperExternalRouteButton extends StatelessWidget {
  const VesperExternalRouteButton({
    super.key,
    this.size = 40,
    this.brightness,
    this.controller,
    this.onResult,
  });

  final double size;
  final Brightness? brightness;
  final VesperExternalPlaybackController? controller;
  final ValueChanged<VesperExternalPlaybackResult>? onResult;

  @override
  Widget build(BuildContext context) {
    return VesperExternalRouteIconButton(
      size: size,
      brightness: brightness,
      controller: controller,
      onResult: onResult,
    );
  }
}

class VesperExternalRouteIconButton extends StatelessWidget {
  const VesperExternalRouteIconButton({
    super.key,
    this.size = 38,
    this.brightness,
    this.controller,
    this.onResult,
  });

  final double size;
  final Brightness? brightness;
  final VesperExternalPlaybackController? controller;
  final ValueChanged<VesperExternalPlaybackResult>? onResult;

  @override
  Widget build(BuildContext context) {
    if (kIsWeb || defaultTargetPlatform != TargetPlatform.android) {
      return SizedBox.square(dimension: size);
    }
    final effectiveBrightness = brightness ?? Theme.of(context).brightness;
    return SizedBox.square(
      dimension: size,
      child: IconButton(
        tooltip: 'External route',
        icon: const Icon(Icons.cast),
        iconSize: (size - 14).clamp(18, 32).toDouble(),
        padding: EdgeInsets.zero,
        onPressed: () async {
          final externalController =
              controller ?? VesperExternalPlaybackController();
          var result = _handledRoutePickerResult;
          try {
            final routePickerResult = await externalController.showRoutePicker(
              brightness: effectiveBrightness,
            );
            result = routePickerResult.isSuccess
                ? routePickerResult
                : _handledRoutePickerResult;
          } catch (_) {
            result = _handledRoutePickerResult;
          } finally {
            if (controller == null) {
              externalController.dispose();
            }
          }
          onResult?.call(result);
        },
      ),
    );
  }
}

const _handledRoutePickerResult = VesperExternalPlaybackResult(
  status: VesperExternalPlaybackResultStatus.success,
);

@visibleForTesting
class VesperExternalRoutePlatformViewButton extends StatelessWidget {
  const VesperExternalRoutePlatformViewButton({
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
