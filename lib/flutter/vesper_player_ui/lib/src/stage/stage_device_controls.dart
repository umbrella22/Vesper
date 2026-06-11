abstract interface class VesperPlayerDeviceControls {
  Future<double?> currentBrightnessRatio();

  Future<double?> setBrightnessRatio(double ratio);

  Future<double?> currentVolumeRatio();

  Future<double?> setVolumeRatio(double ratio);
}
