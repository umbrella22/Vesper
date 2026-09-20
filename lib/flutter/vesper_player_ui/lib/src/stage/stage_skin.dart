import 'package:material_ui/material_ui.dart';

/// The action or feedback represented by a Stage icon.
enum VesperStageIconRole {
  play,
  pause,
  fullscreen,
  exitFullscreen,
  navigateBack,
  more,
  brightness,
  volume,
  speed,
}

/// Visual roles shared by built-in and host-supplied buttons.
enum VesperStageButtonVariant {
  standard,
  toolbar,
  navigation,
  compact,
  compactFullscreen,
  expanded,
  expandedFullscreen,
  primary,
}

/// Returns null to render the configured [VesperPlayerStageIcons] entry.
/// Returned widgets are decorative; the containing control supplies semantics.
typedef VesperStageIconBuilder = Widget? Function(
  BuildContext context,
  VesperStageIconRole role,
  VesperStageIconStyle style,
);

@immutable
class VesperStageIconStyle {
  const VesperStageIconStyle({required this.size, required this.color});
  final double size;
  final Color color;
}

@immutable
class VesperPlayerStageIcons {
  const VesperPlayerStageIcons({
    this.play = Icons.play_arrow_rounded,
    this.pause = Icons.pause_rounded,
    this.fullscreen = Icons.fullscreen_rounded,
    this.exitFullscreen = Icons.fullscreen_exit_rounded,
    this.navigateBack = Icons.arrow_back_rounded,
    this.more = Icons.more_vert_rounded,
    this.brightness = Icons.wb_sunny_rounded,
    this.volume = Icons.volume_up_rounded,
    this.speed = Icons.speed_rounded,
  });

  final IconData play, pause, fullscreen, exitFullscreen, navigateBack, more;
  final IconData brightness, volume, speed;

  IconData resolve(VesperStageIconRole role) => switch (role) {
        VesperStageIconRole.play => play,
        VesperStageIconRole.pause => pause,
        VesperStageIconRole.fullscreen => fullscreen,
        VesperStageIconRole.exitFullscreen => exitFullscreen,
        VesperStageIconRole.navigateBack => navigateBack,
        VesperStageIconRole.more => more,
        VesperStageIconRole.brightness => brightness,
        VesperStageIconRole.volume => volume,
        VesperStageIconRole.speed => speed,
      };
}

@immutable
class VesperStageColors {
  const VesperStageColors({
    this.foreground = Colors.white,
    this.secondaryForeground = const Color(0xFFBFC6D6),
    this.background = Colors.black,
    this.scrim = Colors.black,
    this.buttonBackground = Colors.white,
    this.accent = const Color(0xFFFFB454),
    this.timelineStart = const Color(0xFFFF6B8E),
    this.timelineEnd = const Color(0xFFFFB454),
    this.timelineInactive = Colors.white,
    this.timelineThumb = Colors.white,
    this.hudBackground = const Color(0xB8000000),
    this.hudForeground = Colors.white,
  });
  final Color foreground, secondaryForeground, background, scrim;
  final Color buttonBackground, accent, timelineStart, timelineEnd;
  final Color timelineInactive, timelineThumb, hudBackground, hudForeground;
}

/// Visual size is independent of the minimum 48 logical pixel touch target.
@immutable
class VesperStageButtonStyle {
  const VesperStageButtonStyle({
    this.size = 52,
    this.iconSize = 24,
    this.backgroundOpacity = 0.10,
    this.borderRadius = 999,
  })  : assert(size > 0 && size < double.infinity),
        assert(iconSize > 0 && iconSize < double.infinity),
        assert(backgroundOpacity >= 0 && backgroundOpacity <= 1),
        assert(borderRadius >= 0 && borderRadius < double.infinity);
  final double size, iconSize, backgroundOpacity, borderRadius;
}

@immutable
class VesperStageMetrics {
  const VesperStageMetrics({
    this.standard = const VesperStageButtonStyle(),
    this.toolbar = const VesperStageButtonStyle(size: 38, backgroundOpacity: 0),
    this.navigation = const VesperStageButtonStyle(
        size: 38, iconSize: 23, backgroundOpacity: 0),
    this.compact = const VesperStageButtonStyle(size: 38, backgroundOpacity: 0),
    this.compactFullscreen =
        const VesperStageButtonStyle(size: 38, backgroundOpacity: 0),
    this.expanded = const VesperStageButtonStyle(
        size: 38, iconSize: 22, backgroundOpacity: 0),
    this.expandedFullscreen = const VesperStageButtonStyle(
        size: 34, iconSize: 19, backgroundOpacity: 0),
    this.primary = const VesperStageButtonStyle(
        size: 72, iconSize: 36, backgroundOpacity: 0.14),
    this.buttonSpacing = 8,
    this.hudIconSize = 24,
    this.hudBorderRadius = 999,
    this.timelineTrackHeight = 4,
    this.timelineThumbSize = 11,
    this.timelineLargeThumbSize = 14,
  })  : assert(buttonSpacing >= 0 && buttonSpacing < double.infinity),
        assert(hudIconSize > 0 && hudIconSize < double.infinity),
        assert(hudBorderRadius >= 0 && hudBorderRadius < double.infinity),
        assert(
            timelineTrackHeight > 0 && timelineTrackHeight < double.infinity),
        assert(timelineThumbSize > 0 && timelineThumbSize < double.infinity),
        assert(timelineLargeThumbSize > 0 &&
            timelineLargeThumbSize < double.infinity);
  final VesperStageButtonStyle standard,
      toolbar,
      navigation,
      compact,
      compactFullscreen,
      expanded;
  final VesperStageButtonStyle expandedFullscreen, primary;
  final double buttonSpacing, hudIconSize, hudBorderRadius;
  final double timelineTrackHeight, timelineThumbSize, timelineLargeThumbSize;

  VesperStageButtonStyle button(VesperStageButtonVariant variant) =>
      switch (variant) {
        VesperStageButtonVariant.standard => standard,
        VesperStageButtonVariant.toolbar => toolbar,
        VesperStageButtonVariant.navigation => navigation,
        VesperStageButtonVariant.compact => compact,
        VesperStageButtonVariant.compactFullscreen => compactFullscreen,
        VesperStageButtonVariant.expanded => expanded,
        VesperStageButtonVariant.expandedFullscreen => expandedFullscreen,
        VesperStageButtonVariant.primary => primary,
      };
}

/// Presentation-only configuration. It never crosses a player channel or FFI.
@immutable
class VesperPlayerStageSkin {
  const VesperPlayerStageSkin({
    this.icons = const VesperPlayerStageIcons(),
    this.colors = const VesperStageColors(),
    this.metrics = const VesperStageMetrics(),
    this.iconBuilder,
  });
  final VesperPlayerStageIcons icons;
  final VesperStageColors colors;
  final VesperStageMetrics metrics;
  final VesperStageIconBuilder? iconBuilder;
}

/// Makes a skin available to standalone controls and custom Stage slots.
/// A Stage establishes its own scope; its null skin selects the SDK default.
class VesperPlayerStageTheme extends StatelessWidget {
  const VesperPlayerStageTheme(
      {super.key, required this.skin, required this.child});
  final VesperPlayerStageSkin skin;
  final Widget child;

  static VesperPlayerStageSkin of(BuildContext context) =>
      context
          .dependOnInheritedWidgetOfExactType<_VesperStageSkinScope>()
          ?.skin ??
      const VesperPlayerStageSkin();

  @override
  Widget build(BuildContext context) => _VesperStageSkinScope(
        skin: skin,
        child: IconTheme(
          data: IconThemeData(
              size: skin.metrics.standard.iconSize,
              color: skin.colors.foreground),
          child: child,
        ),
      );
}

class _VesperStageSkinScope extends InheritedWidget {
  const _VesperStageSkinScope({required this.skin, required super.child});
  final VesperPlayerStageSkin skin;

  @override
  bool updateShouldNotify(_VesperStageSkinScope oldWidget) =>
      skin != oldWidget.skin;
}

/// Renders an action icon with the active skin and surrounding IconTheme.
class VesperStageIcon extends StatelessWidget {
  const VesperStageIcon(this.role, {super.key, this.size, this.color});
  final VesperStageIconRole role;
  final double? size;
  final Color? color;

  @override
  Widget build(BuildContext context) {
    final skin = VesperPlayerStageTheme.of(context);
    final theme = IconTheme.of(context);
    final style = VesperStageIconStyle(
      size: size ?? theme.size ?? skin.metrics.standard.iconSize,
      color: color ?? theme.color ?? skin.colors.foreground,
    );
    return IgnorePointer(
      child: ExcludeSemantics(
        child: SizedBox.square(
          dimension: style.size,
          child: IconTheme(
            data: IconThemeData(size: style.size, color: style.color),
            child: skin.iconBuilder?.call(context, role, style) ??
                Icon(skin.icons.resolve(role),
                    size: style.size, color: style.color),
          ),
        ),
      ),
    );
  }
}
