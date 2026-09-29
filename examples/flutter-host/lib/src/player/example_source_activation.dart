import 'package:vesper_player/vesper_player.dart';

/// Registers one source and transfers playback ownership to the controller.
/// Closing the registration afterward preserves the native playback lease.
Future<VesperSourceActivation> activateExampleSource(
  VesperPlayerController controller,
  VesperPlayerSource source, {
  required VesperSourceActivationOptions options,
  bool Function()? isCurrent,
}) async {
  void checkCurrent() {
    if (isCurrent != null && !isCurrent()) {
      throw StateError('Source activation was superseded or disposed.');
    }
  }

  checkCurrent();
  final session = await VesperSourceSession.create();
  try {
    checkCurrent();
    final handle = await session.register(source);
    checkCurrent();
    final activation = await controller.activate(handle, options: options);
    checkCurrent();
    return activation;
  } finally {
    // Session disposal closes every handle, including a registration whose
    // asynchronous response arrived after the owning page was disposed.
    await session.dispose();
  }
}
