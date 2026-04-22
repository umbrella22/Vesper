import 'package:flutter/material.dart';
import 'package:vesper_player/vesper_player.dart';

import 'example_player_helpers.dart';
import 'example_player_models.dart';

Future<void> showExampleSelectionSheet(
  BuildContext context, {
  required VesperPlayerController controller,
  required ExamplePlayerSheet initialSheet,
}) {
  final mediaQuery = MediaQuery.of(context);
  return showModalBottomSheet<void>(
    context: context,
    isScrollControlled: true,
    backgroundColor: Colors.transparent,
    constraints: BoxConstraints(maxWidth: mediaQuery.size.width),
    builder: (_) {
      return ExampleSelectionSheet(
        controller: controller,
        initialSheet: initialSheet,
      );
    },
  );
}

class ExampleSelectionSheet extends StatefulWidget {
  const ExampleSelectionSheet({
    super.key,
    required this.controller,
    required this.initialSheet,
  });

  final VesperPlayerController controller;
  final ExamplePlayerSheet initialSheet;

  @override
  State<ExampleSelectionSheet> createState() => _ExampleSelectionSheetState();
}

class _ExampleSelectionSheetState extends State<ExampleSelectionSheet> {
  late ExamplePlayerSheet _activeSheet;

  @override
  void initState() {
    super.initState();
    _activeSheet = widget.initialSheet;
  }

  @override
  Widget build(BuildContext context) {
    final mediaQuery = MediaQuery.of(context);
    return ValueListenableBuilder<VesperPlayerSnapshot>(
      valueListenable: widget.controller.snapshotListenable,
      builder: (context, snapshot, _) {
        return SafeArea(
          top: false,
          child: DecoratedBox(
            decoration: const BoxDecoration(
              color: Color(0xFF0C1018),
              borderRadius: BorderRadius.vertical(top: Radius.circular(28)),
            ),
            child: ConstrainedBox(
              constraints: BoxConstraints(
                maxHeight: mediaQuery.size.height * 0.82,
              ),
              child: Padding(
                padding: EdgeInsets.fromLTRB(
                  18,
                  18,
                  18,
                  18 + mediaQuery.padding.bottom,
                ),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: <Widget>[
                    Padding(
                      padding: const EdgeInsets.only(
                        left: 4,
                        right: 4,
                        top: 8,
                        bottom: 12,
                      ),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: <Widget>[
                          Text(
                            sheetTitle(_activeSheet),
                            style: Theme.of(context).textTheme.headlineSmall
                                ?.copyWith(
                                  color: Colors.white,
                                  fontWeight: FontWeight.bold,
                                ),
                          ),
                          const SizedBox(height: 6),
                          Text(
                            sheetSubtitle(_activeSheet),
                            style: Theme.of(context).textTheme.bodySmall
                                ?.copyWith(
                                  color: const Color(0xFF98A1B3),
                                  height: 1.45,
                                ),
                          ),
                        ],
                      ),
                    ),
                    Flexible(
                      child: ListView(
                        padding: EdgeInsets.zero,
                        children: _buildRows(snapshot),
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),
        );
      },
    );
  }

  List<Widget> _buildRows(VesperPlayerSnapshot snapshot) {
    switch (_activeSheet) {
      case ExamplePlayerSheet.menu:
        return <Widget>[
          ExampleSelectionRow(
            title: '播放速度',
            subtitle: speedBadge(snapshot.playbackRate),
            onTap: () => setState(() {
              _activeSheet = ExamplePlayerSheet.speed;
            }),
          ),
          ExampleSelectionRow(
            title: '音频',
            subtitle: audioButtonLabel(
              snapshot.trackCatalog,
              snapshot.trackSelection,
            ),
            onTap: () => setState(() {
              _activeSheet = ExamplePlayerSheet.audio;
            }),
          ),
          ExampleSelectionRow(
            title: '字幕',
            subtitle: subtitleButtonLabel(
              snapshot.trackCatalog,
              snapshot.trackSelection,
            ),
            onTap: () => setState(() {
              _activeSheet = ExamplePlayerSheet.subtitle;
            }),
          ),
          ExampleSelectionRow(
            title: '画质',
            subtitle: qualityButtonLabel(
              snapshot.trackCatalog,
              snapshot.trackSelection,
              snapshot.effectiveVideoTrackId,
              snapshot.fixedTrackStatus,
            ),
            onTap: () => setState(() {
              _activeSheet = ExamplePlayerSheet.quality;
            }),
          ),
        ];
      case ExamplePlayerSheet.quality:
        final tracks = snapshot.trackCatalog.videoTracks.toList(growable: false)
          ..sort(
            (left, right) => (right.bitRate ?? 0).compareTo(left.bitRate ?? 0),
          );
        final abrPolicy = snapshot.trackSelection.abrPolicy;
        final supportsFixedTrackAbr = snapshot.capabilities.supportsAbrMode(
          VesperAbrMode.fixedTrack,
        );
        final qualityNotice = qualityCapabilityNotice(snapshot.capabilities);
        final qualityRuntimeNoticeModel = qualityRuntimeNotice(snapshot);
        return <Widget>[
          ExampleSelectionRow(
            title: qualityAutoRowTitle(abrPolicy),
            badgeLabel: qualityAutoRowBadgeLabel(abrPolicy),
            badgeTone: ExampleSelectionBadgeTone.accent,
            subtitle: snapshot.trackCatalog.adaptiveVideo
                ? qualityAutoRowSubtitle(
                    snapshot.trackCatalog,
                    snapshot.trackSelection,
                    snapshot.effectiveVideoTrackId,
                    snapshot.fixedTrackStatus,
                    snapshot.videoVariantObservation,
                  )
                : '当前路径没有暴露自适应视频切换能力。',
            selected:
                abrPolicy.mode == VesperAbrMode.auto ||
                abrPolicy.mode == VesperAbrMode.constrained,
            onTap: () => _applyAndClose(
              widget.controller.setAbrPolicy(const VesperAbrPolicy.auto()),
            ),
          ),
          if (qualityRuntimeNoticeModel
              case final ExampleSheetNoticeModel notice)
            ExampleSheetNote(
              title: notice.title,
              message: notice.message,
              tone: notice.tone,
            ),
          if (qualityNotice case final message?)
            ExampleSheetNote(message: message),
          if (tracks.isEmpty)
            const ExampleEmptySheetState(message: '当前媒体没有暴露可选视频轨。')
          else if (!supportsFixedTrackAbr)
            const ExampleEmptySheetState(message: '当前平台只支持自动或受限 ABR，暂不支持固定视频轨。')
          else
            ...tracks.map((track) {
              final badgeLabel = qualityOptionBadgeLabel(
                track.id,
                trackCatalog: snapshot.trackCatalog,
                trackSelection: snapshot.trackSelection,
                effectiveVideoTrackId: snapshot.effectiveVideoTrackId,
                fixedTrackStatus: snapshot.fixedTrackStatus,
              );
              return ExampleSelectionRow(
                title: qualityLabel(track),
                badgeLabel: badgeLabel,
                badgeTone:
                    badgeLabel == '等待' ||
                        badgeLabel == '锁定' ||
                        badgeLabel == '锁定中'
                    ? ExampleSelectionBadgeTone.warm
                    : ExampleSelectionBadgeTone.accent,
                subtitle: qualityOptionSubtitle(
                  track,
                  snapshot.trackSelection,
                  snapshot.effectiveVideoTrackId,
                  snapshot.fixedTrackStatus,
                  trackCatalog: snapshot.trackCatalog,
                ),
                selected:
                    abrPolicy.mode == VesperAbrMode.fixedTrack &&
                    abrPolicy.trackId == track.id,
                onTap: () => _applyAndClose(
                  widget.controller.setAbrPolicy(
                    VesperAbrPolicy.fixedTrack(track.id),
                  ),
                ),
              );
            }),
        ];
      case ExamplePlayerSheet.audio:
        final tracks = snapshot.trackCatalog.audioTracks;
        return <Widget>[
          ExampleSelectionRow(
            title: '自动',
            subtitle: '使用播放器默认的音频选择。',
            selected:
                snapshot.trackSelection.audio.mode ==
                VesperTrackSelectionMode.auto,
            onTap: () => _applyAndClose(
              widget.controller.setAudioTrackSelection(
                const VesperTrackSelection.auto(),
              ),
            ),
          ),
          if (tracks.isEmpty)
            const ExampleEmptySheetState(message: '当前媒体没有暴露可选音频节目。')
          else
            ...tracks.map((track) {
              return ExampleSelectionRow(
                title: audioLabel(track),
                subtitle: audioSubtitle(track),
                selected:
                    snapshot.trackSelection.audio.mode ==
                        VesperTrackSelectionMode.track &&
                    snapshot.trackSelection.audio.trackId == track.id,
                onTap: () => _applyAndClose(
                  widget.controller.setAudioTrackSelection(
                    VesperTrackSelection.track(track.id),
                  ),
                ),
              );
            }),
        ];
      case ExamplePlayerSheet.subtitle:
        final tracks = snapshot.trackCatalog.subtitleTracks;
        return <Widget>[
          ExampleSelectionRow(
            title: '关闭',
            subtitle: '隐藏字幕渲染。',
            selected:
                snapshot.trackSelection.subtitle.mode ==
                VesperTrackSelectionMode.disabled,
            onTap: () => _applyAndClose(
              widget.controller.setSubtitleTrackSelection(
                const VesperTrackSelection.disabled(),
              ),
            ),
          ),
          ExampleSelectionRow(
            title: '自动',
            subtitle: '使用流的默认字幕行为。',
            selected:
                snapshot.trackSelection.subtitle.mode ==
                VesperTrackSelectionMode.auto,
            onTap: () => _applyAndClose(
              widget.controller.setSubtitleTrackSelection(
                const VesperTrackSelection.auto(),
              ),
            ),
          ),
          if (tracks.isEmpty)
            const ExampleEmptySheetState(message: '当前媒体没有暴露可选字幕轨。')
          else
            ...tracks.map((track) {
              return ExampleSelectionRow(
                title: subtitleLabel(track),
                subtitle: subtitleSubtitle(track),
                selected:
                    snapshot.trackSelection.subtitle.mode ==
                        VesperTrackSelectionMode.track &&
                    snapshot.trackSelection.subtitle.trackId == track.id,
                onTap: () => _applyAndClose(
                  widget.controller.setSubtitleTrackSelection(
                    VesperTrackSelection.track(track.id),
                  ),
                ),
              );
            }),
        ];
      case ExamplePlayerSheet.speed:
        final playbackRates =
            snapshot.capabilities.supportedPlaybackRates.isNotEmpty
            ? snapshot.capabilities.supportedPlaybackRates
            : const <double>[0.75, 1.0, 1.25, 1.5, 2.0];
        return playbackRates
            .map((rate) {
              final selected = (snapshot.playbackRate - rate).abs() < 0.01;
              return ExampleSelectionRow(
                title: speedBadge(rate),
                subtitle: selected ? '当前已生效。' : '立即应用这个速度。',
                selected: selected,
                onTap: () =>
                    _applyAndClose(widget.controller.setPlaybackRate(rate)),
              );
            })
            .toList(growable: false);
    }
  }

  Future<void> _applyAndClose(Future<void> action) async {
    await action;
    if (mounted) {
      Navigator.of(context).pop();
    }
  }
}

class ExampleSheetNote extends StatelessWidget {
  const ExampleSheetNote({
    super.key,
    required this.message,
    this.title,
    this.tone = ExampleSheetNoteTone.info,
  });

  final String? title;
  final String message;
  final ExampleSheetNoteTone tone;

  @override
  Widget build(BuildContext context) {
    final accent = switch (tone) {
      ExampleSheetNoteTone.info => const Color(0xFF8EC5FF),
      ExampleSheetNoteTone.warm => const Color(0xFFFFC876),
    };
    final foreground = switch (tone) {
      ExampleSheetNoteTone.info => const Color(0xFFC7DCF7),
      ExampleSheetNoteTone.warm => const Color(0xFFFFE8BF),
    };
    final titleColor = switch (tone) {
      ExampleSheetNoteTone.info => const Color(0xFFE8F3FF),
      ExampleSheetNoteTone.warm => const Color(0xFFFFF4D3),
    };
    final icon = switch (tone) {
      ExampleSheetNoteTone.info => Icons.tips_and_updates_outlined,
      ExampleSheetNoteTone.warm => Icons.auto_awesome_motion_rounded,
    };
    return Padding(
      padding: const EdgeInsets.only(top: 8, bottom: 8),
      child: Container(
        width: double.infinity,
        padding: const EdgeInsets.all(18),
        decoration: BoxDecoration(
          color: accent.withValues(alpha: 0.09),
          borderRadius: BorderRadius.circular(18),
          border: Border.all(color: accent.withValues(alpha: 0.18)),
        ),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Container(
              width: 32,
              height: 32,
              decoration: BoxDecoration(
                color: accent.withValues(alpha: 0.12),
                borderRadius: BorderRadius.circular(999),
              ),
              child: Icon(icon, size: 18, color: accent),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: <Widget>[
                  if (title case final String titleText) ...<Widget>[
                    Text(
                      titleText,
                      style: Theme.of(context).textTheme.labelLarge?.copyWith(
                        color: titleColor,
                        fontWeight: FontWeight.w700,
                        letterSpacing: 0.1,
                      ),
                    ),
                    const SizedBox(height: 4),
                  ],
                  Text(
                    message,
                    style: Theme.of(context).textTheme.bodySmall?.copyWith(
                      color: foreground,
                      height: 1.45,
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class ExampleSelectionRow extends StatelessWidget {
  const ExampleSelectionRow({
    super.key,
    required this.title,
    required this.subtitle,
    required this.onTap,
    this.badgeLabel,
    this.badgeTone = ExampleSelectionBadgeTone.accent,
    this.selected = false,
    this.enabled = true,
  });

  final String title;
  final String subtitle;
  final VoidCallback onTap;
  final String? badgeLabel;
  final ExampleSelectionBadgeTone badgeTone;
  final bool selected;
  final bool enabled;

  @override
  Widget build(BuildContext context) {
    final titleColor = enabled
        ? Colors.white
        : Colors.white.withValues(alpha: 0.45);
    final subtitleColor = enabled
        ? const Color(0xFF98A1B3)
        : const Color(0xFF98A1B3).withValues(alpha: 0.55);
    final badgeAccent = switch (badgeTone) {
      ExampleSelectionBadgeTone.accent => const Color(0xFF8EC5FF),
      ExampleSelectionBadgeTone.warm => const Color(0xFFFFC876),
    };
    final badgeForeground = enabled
        ? switch (badgeTone) {
            ExampleSelectionBadgeTone.accent => const Color(0xFFDCEEFF),
            ExampleSelectionBadgeTone.warm => const Color(0xFFFFE8BF),
          }
        : switch (badgeTone) {
            ExampleSelectionBadgeTone.accent => const Color(
              0xFFDCEEFF,
            ).withValues(alpha: 0.5),
            ExampleSelectionBadgeTone.warm => const Color(
              0xFFFFE8BF,
            ).withValues(alpha: 0.5),
          };
    final badgeBackground = enabled
        ? badgeAccent.withValues(alpha: selected ? 0.20 : 0.12)
        : badgeAccent.withValues(alpha: 0.06);
    final badgeBorder = enabled
        ? badgeAccent.withValues(alpha: selected ? 0.34 : 0.18)
        : badgeAccent.withValues(alpha: 0.10);
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        Material(
          color: selected
              ? Colors.white.withValues(alpha: 0.10)
              : Colors.transparent,
          borderRadius: BorderRadius.circular(18),
          child: InkWell(
            onTap: enabled ? onTap : null,
            borderRadius: BorderRadius.circular(18),
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: <Widget>[
                  Row(
                    children: <Widget>[
                      Expanded(
                        child: Text(
                          title,
                          style: Theme.of(context).textTheme.titleSmall
                              ?.copyWith(
                                color: titleColor,
                                fontWeight: FontWeight.w600,
                              ),
                        ),
                      ),
                      if (badgeLabel case final label?)
                        Container(
                          padding: const EdgeInsets.symmetric(
                            horizontal: 10,
                            vertical: 4,
                          ),
                          decoration: BoxDecoration(
                            color: badgeBackground,
                            borderRadius: BorderRadius.circular(999),
                            border: Border.all(color: badgeBorder),
                          ),
                          child: Text(
                            label,
                            style: Theme.of(context).textTheme.labelSmall
                                ?.copyWith(
                                  color: badgeForeground,
                                  fontWeight: FontWeight.w700,
                                  letterSpacing: 0.2,
                                ),
                          ),
                        ),
                    ],
                  ),
                  const SizedBox(height: 4),
                  Text(
                    subtitle,
                    style: Theme.of(
                      context,
                    ).textTheme.bodySmall?.copyWith(color: subtitleColor),
                  ),
                ],
              ),
            ),
          ),
        ),
        Divider(color: Colors.white.withValues(alpha: 0.04), height: 1),
      ],
    );
  }
}

enum ExampleSelectionBadgeTone { accent, warm }

class ExampleEmptySheetState extends StatelessWidget {
  const ExampleEmptySheetState({super.key, required this.message});

  final String message;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(top: 8),
      child: Container(
        width: double.infinity,
        padding: const EdgeInsets.all(18),
        decoration: BoxDecoration(
          color: Colors.white.withValues(alpha: 0.03),
          borderRadius: BorderRadius.circular(18),
        ),
        child: Text(
          message,
          style: Theme.of(context).textTheme.bodySmall?.copyWith(
            color: const Color(0xFF98A1B3),
            height: 1.45,
          ),
        ),
      ),
    );
  }
}
