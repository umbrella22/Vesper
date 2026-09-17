import 'package:flutter/widgets.dart';
import 'package:vesper_player/vesper_player.dart';
import 'package:vesper_player_ui/vesper_player_ui.dart' as ui;

import '../device/example_device_controls.dart';
import 'example_player_models.dart';

class ExamplePlayerStage extends StatelessWidget {
  const ExamplePlayerStage({
    super.key,
    required this.controller,
    required this.snapshot,
    required this.controlLayout,
    required this.isFullscreen,
    required this.onOpenSheet,
    required this.onToggleFullscreen,
    this.sheetOpen = false,
    this.deviceControls,
    this.contentOverlay,
    this.expandedControlBarLeading,
    this.onNavigateBack,
    this.navigateBackSemanticLabel,
    this.topBarPrimaryAction,
    this.topBarSecondaryAction,
    this.keepControlsVisible = false,
    this.pictureInPicturePresentation = false,
  });

  final VesperPlayerController controller;
  final VesperPlayerSnapshot snapshot;
  final ui.VesperStageControlLayout controlLayout;
  final bool isFullscreen;
  final bool sheetOpen;
  final ExampleDeviceControls? deviceControls;
  final Widget? contentOverlay;
  final Widget? expandedControlBarLeading;
  final VoidCallback? onNavigateBack;
  final String? navigateBackSemanticLabel;
  final Widget? topBarPrimaryAction;
  final Widget? topBarSecondaryAction;
  final bool keepControlsVisible;
  final bool pictureInPicturePresentation;
  final ValueChanged<ExamplePlayerSheet> onOpenSheet;
  final VoidCallback onToggleFullscreen;

  @override
  Widget build(BuildContext context) {
    return ui.VesperPlayerStage(
      controller: controller,
      snapshot: snapshot,
      controlLayout: controlLayout,
      isFullscreen: isFullscreen,
      sheetOpen: sheetOpen,
      deviceControls: deviceControls,
      contentOverlay: contentOverlay,
      expandedControlBarLeading: expandedControlBarLeading,
      onNavigateBack: onNavigateBack,
      navigateBackSemanticLabel: navigateBackSemanticLabel,
      topBarPrimaryAction: topBarPrimaryAction,
      topBarSecondaryAction: topBarSecondaryAction,
      keepControlsVisible: keepControlsVisible,
      pictureInPicturePresentation: pictureInPicturePresentation,
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
