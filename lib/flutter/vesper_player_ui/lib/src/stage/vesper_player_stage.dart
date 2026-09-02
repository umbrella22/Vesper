import 'dart:async';

import 'package:flutter/gestures.dart';
import 'package:material_ui/material_ui.dart';
import 'package:vesper_player/vesper_player.dart';

import 'stage_device_controls.dart';
import 'stage_helpers.dart';
import 'stage_models.dart';

part 'stage_body.dart';
part 'stage_gestures.dart';
part 'stage_timeline.dart';
part 'stage_controls.dart';

class VesperPlayerStage extends StatefulWidget {
  const VesperPlayerStage({
    super.key,
    required this.controller,
    required this.snapshot,
    required this.isPortrait,
    required this.onOpenSheet,
    required this.onToggleFullscreen,
    this.sheetOpen = false,
    this.deviceControls,
    this.contentOverlay,
    this.landscapeControlBarLeading,
    this.onNavigateBack,
    this.navigateBackSemanticLabel,
    this.topBarPrimaryAction,
    this.topBarSecondaryAction,
    this.keepControlsVisible = false,
    this.pictureInPicturePresentation = false,
    this.strings = const VesperPlayerStageStrings(),
  });

  final VesperPlayerController controller;
  final VesperPlayerSnapshot snapshot;
  final bool isPortrait;
  final bool sheetOpen;
  final VesperPlayerDeviceControls? deviceControls;

  /// Host-rendered visual content above video and below Stage interaction.
  ///
  /// Pointer and accessibility input are disabled for this layer. The layer is
  /// not built during [pictureInPicturePresentation].
  final Widget? contentOverlay;

  /// A direct landscape control-row child inserted after the play button.
  ///
  /// The host can provide fixed-size content or a flex widget. A null value
  /// adds no child or spacing.
  final Widget? landscapeControlBarLeading;

  /// Adds a leading top-bar navigation action when non-null.
  final VoidCallback? onNavigateBack;

  /// Describes the current navigation action to accessibility services.
  final String? navigateBackSemanticLabel;
  final Widget? topBarPrimaryAction;
  final Widget? topBarSecondaryAction;

  /// Prevents the Stage controls from auto-hiding while true.
  ///
  /// Changing this to false restarts the normal auto-hide interval.
  final bool keepControlsVisible;
  final bool pictureInPicturePresentation;
  final VesperPlayerStageStrings strings;
  final ValueChanged<VesperPlayerStageSheet> onOpenSheet;
  final VoidCallback onToggleFullscreen;

  @override
  State<VesperPlayerStage> createState() => _VesperPlayerStageState();
}
