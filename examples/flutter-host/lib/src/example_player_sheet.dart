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
        return <Widget>[
          ExampleSelectionRow(
            title: '自动',
            subtitle: snapshot.trackCatalog.adaptiveVideo
                ? '让播放器自动调整画质。'
                : '当前路径没有暴露自适应视频切换能力。',
            selected:
                snapshot.trackSelection.abrPolicy.mode == VesperAbrMode.auto,
            onTap: () => _applyAndClose(
              widget.controller.setAbrPolicy(const VesperAbrPolicy.auto()),
            ),
          ),
          if (tracks.isEmpty)
            const ExampleEmptySheetState(message: '当前媒体没有暴露可选视频轨。')
          else
            ...tracks.map((track) {
              return ExampleSelectionRow(
                title: qualityLabel(track),
                subtitle: qualitySubtitle(track),
                selected:
                    snapshot.trackSelection.abrPolicy.mode ==
                        VesperAbrMode.fixedTrack &&
                    snapshot.trackSelection.abrPolicy.trackId == track.id,
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

class ExampleSelectionRow extends StatelessWidget {
  const ExampleSelectionRow({
    super.key,
    required this.title,
    required this.subtitle,
    required this.onTap,
    this.selected = false,
  });

  final String title;
  final String subtitle;
  final VoidCallback onTap;
  final bool selected;

  @override
  Widget build(BuildContext context) {
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
            onTap: onTap,
            borderRadius: BorderRadius.circular(18),
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: <Widget>[
                  Text(
                    title,
                    style: Theme.of(context).textTheme.titleSmall?.copyWith(
                      color: Colors.white,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                  const SizedBox(height: 4),
                  Text(
                    subtitle,
                    style: Theme.of(context).textTheme.bodySmall?.copyWith(
                      color: const Color(0xFF98A1B3),
                    ),
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
