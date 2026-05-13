enum VesperPlayerStageSheet { menu, quality, audio, subtitle, speed }

/// Visible labels used by [VesperPlayerStage].
///
/// The default constructor uses English labels. Pass a custom instance when an
/// app needs localized copy without replacing the full stage widget.
final class VesperPlayerStageStrings {
  const VesperPlayerStageStrings({
    this.liveTimelineBadge = 'Live stream',
    this.liveDvrTimelineBadge = 'Live with DVR',
    this.vodTimelineBadge = 'Video on demand',
    this.goLive = 'Go live',
    this.live = 'Live',
    this.liveBehindPrefix = 'Live - ',
    this.liveEdge = 'Live edge',
    this.buffering = 'Buffering',
    this.more = 'More',
    this.play = 'Play',
    this.pause = 'Pause',
    this.fullscreen = 'Fullscreen',
    this.exitFullscreen = 'Exit fullscreen',
    this.quality = 'Quality',
    this.auto = 'Auto',
    this.pinned = 'Pinned',
    this.locking = 'Locking',
    this.qualitySeparator = ' · ',
  });

  const VesperPlayerStageStrings.zhHans()
      : liveTimelineBadge = '直播流',
        liveDvrTimelineBadge = '带 DVR 窗口的直播',
        vodTimelineBadge = '点播视频',
        goLive = '回到直播',
        live = '直播',
        liveBehindPrefix = '直播 - ',
        liveEdge = '直播实时点',
        buffering = '缓冲中',
        more = '更多',
        play = '播放',
        pause = '暂停',
        fullscreen = '全屏',
        exitFullscreen = '退出全屏',
        quality = '画质',
        auto = '自动',
        pinned = '锁定',
        locking = '锁定中',
        qualitySeparator = ' · ';

  final String liveTimelineBadge;
  final String liveDvrTimelineBadge;
  final String vodTimelineBadge;
  final String goLive;
  final String live;
  final String liveBehindPrefix;
  final String liveEdge;
  final String buffering;
  final String more;
  final String play;
  final String pause;
  final String fullscreen;
  final String exitFullscreen;
  final String quality;
  final String auto;
  final String pinned;
  final String locking;
  final String qualitySeparator;
}
