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

/// Handles a confirmed content tap in logical pixels from the Stage's top left.
///
/// Return true to consume the tap, or false to use the normal controls behavior.
typedef VesperStageContentTapHandler = bool Function(Offset localPosition);

/// Control density is independent of video orientation and fullscreen state.
enum VesperStageControlLayout { compact, expanded }

class VesperPlayerStage extends StatefulWidget {
  const VesperPlayerStage({
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
    this.onContentTap,
    this.onGeometryChanged,
    this.expandedControlBarLeading,
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
  final VesperStageControlLayout controlLayout;
  final bool isFullscreen;

  /// Picture bounds in Stage-local logical pixels, excluding renderer bars.
  final ValueChanged<VesperVideoSurfaceGeometry?>? onGeometryChanged;
  final bool sheetOpen;
  final VesperPlayerDeviceControls? deviceControls;

  /// Host-rendered visual content above video and below Stage interaction.
  ///
  /// Pointer and accessibility input are disabled for this layer. The layer is
  /// not built during [pictureInPicturePresentation].
  final Widget? contentOverlay;

  /// Offers a confirmed single tap to the host before toggling the controls.
  ///
  /// Coordinates use the complete Stage / [contentOverlay] canvas, including
  /// video letterboxing. They are not normalized to the smaller gesture area.
  /// Visible controls and the reserved bottom control area take precedence.
  /// Double taps, drags, long presses and cancelled gestures do not invoke this
  /// callback. It is also disabled during [pictureInPicturePresentation].
  ///
  /// Return true synchronously to consume the tap without changing controls or
  /// playback. Return false (or leave this null) for the existing tap behavior.
  /// The host owns content hit testing, playback-state gating and any subsequent
  /// asynchronous action. This callback does not add accessibility nodes; hosts
  /// should provide a separate focusable entry point for content actions.
  final VesperStageContentTapHandler? onContentTap;

  /// A direct expanded control-row child inserted after the play button.
  ///
  /// The host can provide fixed-size content or a flex widget. A null value
  /// adds no child or spacing.
  final Widget? expandedControlBarLeading;

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
