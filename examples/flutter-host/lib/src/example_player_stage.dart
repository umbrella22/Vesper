import 'package:flutter/widgets.dart';
import 'package:vesper_player/vesper_player.dart';
import 'package:vesper_player_ui/vesper_player_ui.dart' as ui;

import 'example_device_controls.dart';
import 'example_player_models.dart';

class ExamplePlayerStage extends StatelessWidget {
  const ExamplePlayerStage({
    super.key,
    required this.controller,
    required this.snapshot,
    required this.isPortrait,
    required this.onOpenSheet,
    required this.onToggleFullscreen,
    this.sheetOpen = false,
    this.deviceControls,
  });

  final VesperPlayerController controller;
  final VesperPlayerSnapshot snapshot;
  final bool isPortrait;
  final bool sheetOpen;
  final ExampleDeviceControls? deviceControls;
  final ValueChanged<ExamplePlayerSheet> onOpenSheet;
  final VoidCallback onToggleFullscreen;

  @override
  Widget build(BuildContext context) {
    return ui.VesperPlayerStage(
      controller: controller,
      snapshot: snapshot,
      isPortrait: isPortrait,
      sheetOpen: sheetOpen,
      deviceControls: deviceControls,
      onOpenSheet: (sheet) => onOpenSheet(sheet.toExamplePlayerSheet()),
      onToggleFullscreen: onToggleFullscreen,
    );
  }
}

extension on ui.VesperPlayerStageSheet {
  ExamplePlayerSheet toExamplePlayerSheet() {
    return switch (this) {
      ui.VesperPlayerStageSheet.menu => ExamplePlayerSheet.menu,
      ui.VesperPlayerStageSheet.quality => ExamplePlayerSheet.quality,
      ui.VesperPlayerStageSheet.audio => ExamplePlayerSheet.audio,
      ui.VesperPlayerStageSheet.subtitle => ExamplePlayerSheet.subtitle,
      ui.VesperPlayerStageSheet.speed => ExamplePlayerSheet.speed,
    };
  }
}
