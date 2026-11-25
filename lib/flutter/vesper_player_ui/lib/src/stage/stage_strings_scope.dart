import 'package:flutter/widgets.dart';

import 'stage_models.dart';

/// Provides [VesperPlayerStageStrings] to every Stage widget below it.
///
/// A Stage or standalone control that receives explicit `strings` uses those
/// instead. Without a scope, Stage copy follows `Localizations.localeOf`.
class VesperPlayerStageStringsScope extends InheritedWidget {
  const VesperPlayerStageStringsScope({
    super.key,
    required this.strings,
    required super.child,
  });

  final VesperPlayerStageStrings strings;

  /// Returns the nearest scoped strings, or null when no scope exists.
  static VesperPlayerStageStrings? maybeOf(BuildContext context) => context
      .dependOnInheritedWidgetOfExactType<VesperPlayerStageStringsScope>()
      ?.strings;

  @override
  bool updateShouldNotify(VesperPlayerStageStringsScope oldWidget) =>
      strings != oldWidget.strings;
}
