/** Native presentation values remain local to each Stage and never cross the player bridge. */
export class VesperStageColors {
  foreground: ResourceColor = Color.White;
  secondaryForeground: ResourceColor = '#BFC6D6';
  background: ResourceColor = Color.Black;
  scrim: ResourceColor = Color.Black;
  buttonBackground: ResourceColor = Color.White;
  accent: ResourceColor = '#FFB454';
  timelineStart: ResourceColor = '#FF6B8E';
  timelineEnd: ResourceColor = '#FFB454';
  timelineInactive: ResourceColor = '#667080';
  timelineThumb: ResourceColor = Color.White;
  settingsBackground: ResourceColor = $r('sys.color.ohos_id_color_dialog_bg');
  settingsForeground: ResourceColor = $r('sys.color.ohos_id_color_text_primary');
  settingsSecondaryForeground: ResourceColor = $r('sys.color.ohos_id_color_text_secondary');
  settingsAccent: ResourceColor = $r('sys.color.ohos_id_color_emphasize');
  error: ResourceColor = '#ffb7b7';
  hudBackground: ResourceColor = '#aa000000';
  hudForeground: ResourceColor = Color.White;
  captionBackground: ResourceColor = '#a0000000';
}

/** Roles describe actions; icon replacement never changes the action or label. */
export enum VesperStageIconRole {
  Play = 'play', Pause = 'pause', Replay = 'replay', Fullscreen = 'fullscreen',
  ExitFullscreen = 'exitFullscreen', NavigateBack = 'navigateBack', More = 'more',
  Brightness = 'brightness', Volume = 'volume', Muted = 'muted', Speed = 'speed',
  Audio = 'audio', Subtitles = 'subtitles', PictureInPicture = 'pictureInPicture',
  Lock = 'lock', Unlock = 'unlock', SkipBack = 'skipBack', SkipForward = 'skipForward'
}

/** Override individual system resources while retaining defaults for other roles. */
export class VesperPlayerStageIcons {
  play: Resource = $r('sys.symbol.play_fill');
  pause: Resource = $r('sys.symbol.pause');
  replay: Resource = $r('sys.symbol.arrow_clockwise');
  fullscreen: Resource = $r('sys.symbol.arrow_up_left_and_arrow_down_right');
  exitFullscreen: Resource = $r('sys.symbol.arrow_down_right_and_arrow_up_left');
  navigateBack: Resource = $r('sys.symbol.chevron_left');
  more: Resource = $r('sys.symbol.more');
  brightness: Resource = $r('sys.symbol.sun_max');
  volume: Resource = $r('sys.symbol.speaker_wave_2');
  muted: Resource = $r('sys.symbol.speaker_slash');
  speed: Resource = $r('sys.symbol.video_speed');
  audio: Resource = $r('sys.symbol.headphones');
  subtitles: Resource = $r('sys.symbol.textformat');
  pictureInPicture: Resource = $r('sys.symbol.smal_window_playback');
  lock: Resource = $r('sys.symbol.lock');
  unlock: Resource = $r('sys.symbol.lock_open');
  skipBack: Resource = $r('sys.symbol.gobackward_10');
  skipForward: Resource = $r('sys.symbol.goforward_10');
  resolve(role: VesperStageIconRole): Resource {
    switch (role) {
      case VesperStageIconRole.Play: return this.play;
      case VesperStageIconRole.Pause: return this.pause;
      case VesperStageIconRole.Replay: return this.replay;
      case VesperStageIconRole.Fullscreen: return this.fullscreen;
      case VesperStageIconRole.ExitFullscreen: return this.exitFullscreen;
      case VesperStageIconRole.NavigateBack: return this.navigateBack;
      case VesperStageIconRole.More: return this.more;
      case VesperStageIconRole.Brightness: return this.brightness;
      case VesperStageIconRole.Volume: return this.volume;
      case VesperStageIconRole.Muted: return this.muted;
      case VesperStageIconRole.Speed: return this.speed;
      case VesperStageIconRole.Audio: return this.audio;
      case VesperStageIconRole.Subtitles: return this.subtitles;
      case VesperStageIconRole.PictureInPicture: return this.pictureInPicture;
      case VesperStageIconRole.Lock: return this.lock;
      case VesperStageIconRole.Unlock: return this.unlock;
      case VesperStageIconRole.SkipBack: return this.skipBack;
      case VesperStageIconRole.SkipForward: return this.skipForward;
      default: throw new Error(`Unknown Stage icon role: ${role}`);
    }
  }
}

export enum VesperStageButtonVariant {
  Standard = 'standard', Toolbar = 'toolbar', Navigation = 'navigation', Compact = 'compact',
  CompactFullscreen = 'compactFullscreen', Expanded = 'expanded',
  ExpandedFullscreen = 'expandedFullscreen', Primary = 'primary'
}

/** Visual geometry is independent of the reserved, non-overlapping hit target. */
export class VesperStageButtonStyle {
  readonly size: number;
  readonly iconSize: number;
  readonly backgroundOpacity: number;
  readonly borderRadius: number;
  readonly hitTarget: number;
  constructor(size: number = 38, iconSize: number = 22, backgroundOpacity: number = 0,
    borderRadius: number = 999, hitTarget: number = 48) {
    if (![size, iconSize, backgroundOpacity, borderRadius, hitTarget].every((value: number): boolean => Number.isFinite(value)) ||
      size <= 0 || iconSize <= 0 || backgroundOpacity < 0 || backgroundOpacity > 1 || borderRadius < 0 || hitTarget < 44) {
      throw new Error('Stage button styles require finite geometry, positive sizes, opacity in [0, 1] and a 44 vp hit target');
    }
    this.size = size; this.iconSize = iconSize; this.backgroundOpacity = backgroundOpacity;
    this.borderRadius = borderRadius; this.hitTarget = Math.max(size, hitTarget);
  }
}

export interface VesperStageMetricConfiguration {
  standard?: VesperStageButtonStyle; toolbar?: VesperStageButtonStyle; navigation?: VesperStageButtonStyle;
  compact?: VesperStageButtonStyle; compactFullscreen?: VesperStageButtonStyle;
  expanded?: VesperStageButtonStyle; expandedFullscreen?: VesperStageButtonStyle; primary?: VesperStageButtonStyle;
  hudIconSize?: number; hudBorderRadius?: number; timelineTrackHeight?: number;
  timelineThumbSize?: number; timelineLargeThumbSize?: number; horizontalInset?: number; topInset?: number;
}

/** Dimensions use viewport units. Buttons retain a minimum 44 vp touch target. */
export class VesperStageMetrics {
  readonly buttonHeight: number;
  readonly buttonSpacing: number;
  readonly buttonRadius: number;
  readonly captionSize: number;
  readonly standard: VesperStageButtonStyle; readonly toolbar: VesperStageButtonStyle;
  readonly navigation: VesperStageButtonStyle; readonly compact: VesperStageButtonStyle;
  readonly compactFullscreen: VesperStageButtonStyle; readonly expanded: VesperStageButtonStyle;
  readonly expandedFullscreen: VesperStageButtonStyle; readonly primary: VesperStageButtonStyle;
  readonly hudIconSize: number; readonly hudBorderRadius: number; readonly timelineTrackHeight: number;
  readonly timelineThumbSize: number; readonly timelineLargeThumbSize: number;
  readonly horizontalInset: number; readonly topInset: number;
  constructor(buttonHeight: number = 48, buttonSpacing: number = 8, buttonRadius: number = 22, captionSize: number = 20,
    configuration: VesperStageMetricConfiguration = {}) {
    if (!Number.isFinite(buttonHeight) || buttonHeight < 44 || !Number.isFinite(buttonSpacing) || buttonSpacing < 0 ||
      !Number.isFinite(buttonRadius) || buttonRadius < 0 || !Number.isFinite(captionSize) || captionSize <= 0) {
      throw new Error('Stage metrics require finite dimensions, a 44 vp button target and nonnegative spacing/radius');
    }
    this.buttonHeight = buttonHeight; this.buttonSpacing = buttonSpacing;
    this.buttonRadius = buttonRadius; this.captionSize = captionSize;
    this.standard = configuration.standard ?? new VesperStageButtonStyle(52, 24, 0.10, buttonRadius, buttonHeight);
    this.toolbar = configuration.toolbar ?? new VesperStageButtonStyle(38, 24, 0, buttonRadius, buttonHeight);
    this.navigation = configuration.navigation ?? new VesperStageButtonStyle(38, 23, 0, buttonRadius, buttonHeight);
    this.compact = configuration.compact ?? new VesperStageButtonStyle(38, 24, 0, buttonRadius, buttonHeight);
    this.compactFullscreen = configuration.compactFullscreen ?? new VesperStageButtonStyle(38, 24, 0, buttonRadius, buttonHeight);
    this.expanded = configuration.expanded ?? new VesperStageButtonStyle(38, 22, 0, buttonRadius, buttonHeight);
    this.expandedFullscreen = configuration.expandedFullscreen ?? new VesperStageButtonStyle(34, 19, 0, buttonRadius, buttonHeight);
    this.primary = configuration.primary ?? new VesperStageButtonStyle(72, 34, 0.14, 36, buttonHeight);
    this.hudIconSize = configuration.hudIconSize ?? 26; this.hudBorderRadius = configuration.hudBorderRadius ?? 16;
    this.timelineTrackHeight = configuration.timelineTrackHeight ?? 4;
    this.timelineThumbSize = configuration.timelineThumbSize ?? 12; this.timelineLargeThumbSize = configuration.timelineLargeThumbSize ?? 14;
    this.horizontalInset = configuration.horizontalInset ?? 18; this.topInset = configuration.topInset ?? 16;
    const positive = [this.hudIconSize, this.timelineTrackHeight, this.timelineThumbSize, this.timelineLargeThumbSize];
    const nonnegative = [this.hudBorderRadius, this.horizontalInset, this.topInset];
    if (positive.some((value: number): boolean => !Number.isFinite(value) || value <= 0) ||
      nonnegative.some((value: number): boolean => !Number.isFinite(value) || value < 0)) {
      throw new Error('Stage metrics require finite positive dimensions and nonnegative insets/radii');
    }
  }
  button(variant: VesperStageButtonVariant): VesperStageButtonStyle {
    switch (variant) {
      case VesperStageButtonVariant.Standard: return this.standard;
      case VesperStageButtonVariant.Toolbar: return this.toolbar;
      case VesperStageButtonVariant.Navigation: return this.navigation;
      case VesperStageButtonVariant.Compact: return this.compact;
      case VesperStageButtonVariant.CompactFullscreen: return this.compactFullscreen;
      case VesperStageButtonVariant.Expanded: return this.expanded;
      case VesperStageButtonVariant.ExpandedFullscreen: return this.expandedFullscreen;
      case VesperStageButtonVariant.Primary: return this.primary;
      default: throw new Error(`Unknown Stage button variant: ${variant}`);
    }
  }
}

export interface VesperStageIconConfiguration { role: VesperStageIconRole; size: number; color: ResourceColor; }
export interface VesperPlayerStageSkinConfiguration {
  icons?: VesperPlayerStageIcons;
  iconContent?: WrappedBuilder<[VesperStageIconConfiguration]>;
  customIconRoles?: VesperStageIconRole[];
  settingsStyle?: string;
}

/** Replace the skin to update appearance while retaining the binding, source and Surface. */
export class VesperPlayerStageSkin {
  readonly colors: VesperStageColors;
  readonly metrics: VesperStageMetrics;
  readonly icons: VesperPlayerStageIcons;
  readonly iconContent?: WrappedBuilder<[VesperStageIconConfiguration]>;
  readonly customIconRoles: VesperStageIconRole[];
  readonly settingsStyle: string;
  constructor(colors: VesperStageColors = new VesperStageColors(), metrics: VesperStageMetrics = new VesperStageMetrics(),
    configuration: VesperPlayerStageSkinConfiguration = {}) {
    if (!['native', 'branded'].includes(configuration.settingsStyle ?? 'native')) { throw new Error('Unknown Stage settings style'); }
    this.colors = colors; this.metrics = metrics; this.icons = configuration.icons ?? new VesperPlayerStageIcons();
    this.iconContent = configuration.iconContent; this.customIconRoles = configuration.customIconRoles?.slice() ?? [];
    this.settingsStyle = configuration.settingsStyle ?? 'native';
  }
}
