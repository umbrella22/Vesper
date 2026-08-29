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
    this.topBarPrimaryAction,
    this.topBarSecondaryAction,
    this.pictureInPicturePresentation = false,
    this.strings = const VesperPlayerStageStrings(),
  });

  final VesperPlayerController controller;
  final VesperPlayerSnapshot snapshot;
  final bool isPortrait;
  final bool sheetOpen;
  final VesperPlayerDeviceControls? deviceControls;
  final Widget? topBarPrimaryAction;
  final Widget? topBarSecondaryAction;
  final bool pictureInPicturePresentation;
  final VesperPlayerStageStrings strings;
  final ValueChanged<VesperPlayerStageSheet> onOpenSheet;
  final VoidCallback onToggleFullscreen;

  @override
  State<VesperPlayerStage> createState() => _VesperPlayerStageState();
}
