import 'package:flutter/widgets.dart';

import 'stage_strings_scope.dart';

enum VesperPlayerStageSheet { menu, quality, audio, subtitle, speed }

/// Visible and accessibility labels used by [VesperPlayerStage].
///
/// The SDK ships English and Simplified Chinese copy aligned with the Android
/// and iOS host-kit Stage strings. When a Stage receives no explicit strings it
/// resolves them with [VesperPlayerStageStrings.of], which prefers the nearest
/// [VesperPlayerStageStringsScope] and otherwise follows
/// `Localizations.localeOf`. Hosts can override individual fields with
/// [copyWith] or supply a complete instance.
@immutable
final class VesperPlayerStageStrings {
  /// English copy. Every field can be overridden by name.
  const VesperPlayerStageStrings({
    this.liveTimelineBadge = 'Live stream',
    this.liveDvrTimelineBadge = 'Live with DVR window',
    this.vodTimelineBadge = 'Video on demand',
    this.goLive = 'Go live',
    this.live = 'LIVE',
    this.liveBehindPrefix = 'LIVE -',
    this.liveEdge = 'LIVE · Edge',
    this.buffering = 'Buffering',
    this.more = 'More',
    this.play = 'Play',
    this.pause = 'Pause',
    this.navigateBack = 'Back',
    this.fullscreen = 'Fullscreen',
    this.exitFullscreen = 'Exit fullscreen',
    this.quality = 'Quality',
    this.auto = 'Auto',
    this.pinned = 'Locked',
    this.locking = 'Locking',
    this.qualitySeparator = ' · ',
    this.playbackPosition = 'Playback position',
  });

  /// English copy. Equivalent to the default constructor.
  const VesperPlayerStageStrings.en() : this();

  /// Simplified Chinese copy.
  const VesperPlayerStageStrings.zhHans()
      : liveTimelineBadge = '直播流',
        liveDvrTimelineBadge = '带 DVR 窗口的直播',
        vodTimelineBadge = '点播视频',
        goLive = '回到直播',
        live = '直播',
        liveBehindPrefix = '直播 -',
        liveEdge = '直播 · 实时点',
        buffering = '缓冲中',
        more = '更多',
        play = '播放',
        pause = '暂停',
        navigateBack = '返回',
        fullscreen = '全屏',
        exitFullscreen = '退出全屏',
        quality = '画质',
        auto = '自动',
        pinned = '锁定',
        locking = '锁定中',
        qualitySeparator = ' · ',
        playbackPosition = '播放进度';

  /// Returns the built-in copy for [locale].
  ///
  /// Any Chinese locale selects [VesperPlayerStageStrings.zhHans], matching
  /// the Android `values-zh` resources. Other or null locales select English.
  static VesperPlayerStageStrings forLocale(Locale? locale) =>
      locale?.languageCode == 'zh'
          ? const VesperPlayerStageStrings.zhHans()
          : const VesperPlayerStageStrings();

  /// Resolves the copy for [context].
  ///
  /// Uses the nearest [VesperPlayerStageStringsScope] when present, otherwise
  /// [forLocale] with the ambient `Localizations` locale. Without a
  /// `Localizations` ancestor this returns English.
  static VesperPlayerStageStrings of(BuildContext context) =>
      VesperPlayerStageStringsScope.maybeOf(context) ??
      forLocale(Localizations.maybeLocaleOf(context));

  /// Returns a copy with the supplied fields replaced.
  VesperPlayerStageStrings copyWith({
    String? liveTimelineBadge,
    String? liveDvrTimelineBadge,
    String? vodTimelineBadge,
    String? goLive,
    String? live,
    String? liveBehindPrefix,
    String? liveEdge,
    String? buffering,
    String? more,
    String? play,
    String? pause,
    String? navigateBack,
    String? fullscreen,
    String? exitFullscreen,
    String? quality,
    String? auto,
    String? pinned,
    String? locking,
    String? qualitySeparator,
    String? playbackPosition,
  }) =>
      VesperPlayerStageStrings(
        liveTimelineBadge: liveTimelineBadge ?? this.liveTimelineBadge,
        liveDvrTimelineBadge: liveDvrTimelineBadge ?? this.liveDvrTimelineBadge,
        vodTimelineBadge: vodTimelineBadge ?? this.vodTimelineBadge,
        goLive: goLive ?? this.goLive,
        live: live ?? this.live,
        liveBehindPrefix: liveBehindPrefix ?? this.liveBehindPrefix,
        liveEdge: liveEdge ?? this.liveEdge,
        buffering: buffering ?? this.buffering,
        more: more ?? this.more,
        play: play ?? this.play,
        pause: pause ?? this.pause,
        navigateBack: navigateBack ?? this.navigateBack,
        fullscreen: fullscreen ?? this.fullscreen,
        exitFullscreen: exitFullscreen ?? this.exitFullscreen,
        quality: quality ?? this.quality,
        auto: auto ?? this.auto,
        pinned: pinned ?? this.pinned,
        locking: locking ?? this.locking,
        qualitySeparator: qualitySeparator ?? this.qualitySeparator,
        playbackPosition: playbackPosition ?? this.playbackPosition,
      );

  List<String> get _values => <String>[
        liveTimelineBadge,
        liveDvrTimelineBadge,
        vodTimelineBadge,
        goLive,
        live,
        liveBehindPrefix,
        liveEdge,
        buffering,
        more,
        play,
        pause,
        navigateBack,
        fullscreen,
        exitFullscreen,
        quality,
        auto,
        pinned,
        locking,
        qualitySeparator,
        playbackPosition,
      ];

  @override
  bool operator ==(Object other) {
    if (identical(this, other)) return true;
    if (other is! VesperPlayerStageStrings) return false;
    final a = _values;
    final b = other._values;
    for (var i = 0; i < a.length; i++) {
      if (a[i] != b[i]) return false;
    }
    return true;
  }

  @override
  int get hashCode => Object.hashAll(_values);

  /// Badge for live timelines without a seekable window.
  final String liveTimelineBadge;

  /// Badge for live timelines with a DVR window.
  final String liveDvrTimelineBadge;

  /// Badge for video-on-demand timelines.
  final String vodTimelineBadge;

  /// Live button text when the live edge position is unknown.
  final String goLive;

  /// Live indicator shown at the live edge.
  final String live;

  /// Prefix placed directly before the behind-live time, such as `LIVE -`.
  final String liveBehindPrefix;

  /// Prefix placed before the live edge time, separated by a space.
  final String liveEdge;
  final String buffering;
  final String more;
  final String play;
  final String pause;
  final String navigateBack;
  final String fullscreen;
  final String exitFullscreen;
  final String quality;
  final String auto;

  /// Quality prefix for a fixed track that is applied.
  final String pinned;

  /// Quality prefix for a fixed track that is pending or falling back.
  final String locking;

  /// Separator between a quality prefix and the track label.
  final String qualitySeparator;

  /// Accessibility label for the timeline scrubber.
  final String playbackPosition;
}
