import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:vesper_player/vesper_player.dart';

import 'example_player_helpers.dart';
import 'example_local_media_picker.dart';
import 'example_player_models.dart';
import 'example_player_sections.dart';
import 'example_player_sheet.dart';
import 'example_player_stage.dart';

class PlayerHostPage extends StatefulWidget {
  const PlayerHostPage({
    super.key,
    required this.themeMode,
    required this.onThemeModeChange,
  });

  final ExampleThemeMode themeMode;
  final ValueChanged<ExampleThemeMode> onThemeModeChange;

  @override
  State<PlayerHostPage> createState() => _PlayerHostPageState();
}

class _PlayerHostPageState extends State<PlayerHostPage> {
  late final TextEditingController _remoteUrlController;
  late Future<VesperPlayerController> _controllerFuture;

  VesperPlayerController? _controller;
  ExampleResilienceProfile _selectedResilienceProfile =
      ExampleResilienceProfile.balanced;
  bool _isApplyingResilienceProfile = false;
  bool _sheetOpen = false;
  List<String> _playlistItemIds = <String>[flutterHlsPlaylistItemId];
  String? _activePlaylistItemId = flutterHlsPlaylistItemId;
  VesperPlayerSource? _queuedRemoteSource;
  VesperPlayerSource? _queuedLocalSource;

  @override
  void initState() {
    super.initState();
    _remoteUrlController = TextEditingController(text: flutterHlsDemoUrl);
    _controllerFuture = _createController();
  }

  @override
  void dispose() {
    final currentController = _controller;
    if (currentController != null) {
      _disposeSilently(currentController);
    }
    unawaited(
      _controllerFuture
          .then((value) {
            if (!identical(value, currentController)) {
              return value.dispose();
            }
          })
          .catchError((_) {}),
    );
    unawaited(_restoreSystemPresentation());
    _remoteUrlController.dispose();
    super.dispose();
  }

  Future<VesperPlayerController> _createController() async {
    VesperPlayerController? nextController;
    try {
      nextController = await VesperPlayerController.create(
        resiliencePolicy: _selectedResilienceProfile.policy,
      );
      await nextController.initialize();
      await nextController.selectSource(flutterHlsDemoSource());
      _playlistItemIds = <String>[flutterHlsPlaylistItemId];
      _activePlaylistItemId = flutterHlsPlaylistItemId;

      final previous = _controller;
      _controller = nextController;
      if (previous != null && !identical(previous, nextController)) {
        _disposeSilently(previous);
      }
      return nextController;
    } catch (_) {
      if (nextController != null) {
        _disposeSilently(nextController);
      }
      rethrow;
    }
  }

  Future<void> _applyResilienceProfile(ExampleResilienceProfile profile) async {
    if (profile == _selectedResilienceProfile) {
      return;
    }
    final controller = _controller ?? await _controllerFuture;
    final previousProfile = _selectedResilienceProfile;
    setState(() {
      _selectedResilienceProfile = profile;
      _isApplyingResilienceProfile = true;
    });
    try {
      await controller.setResiliencePolicy(profile.policy);
    } catch (_) {
      if (mounted) {
        setState(() {
          _selectedResilienceProfile = previousProfile;
        });
      }
      rethrow;
    } finally {
      if (mounted) {
        setState(() {
          _isApplyingResilienceProfile = false;
        });
      }
    }
  }

  Future<void> _selectSource(
    VesperPlayerController controller,
    VesperPlayerSource source,
  ) async {
    if (source.kind == VesperPlayerSourceKind.remote) {
      _remoteUrlController.text = source.uri;
    }
    await controller.selectSource(source);
  }

  VesperPlayerSource? _playlistSourceForItem(String itemId) {
    return switch (itemId) {
      flutterHlsPlaylistItemId => flutterHlsDemoSource(),
      flutterDashPlaylistItemId => flutterDashDemoSource(),
      flutterLocalPlaylistItemId => _queuedLocalSource,
      flutterRemotePlaylistItemId => _queuedRemoteSource,
      _ => null,
    };
  }

  List<ExamplePlaylistItemViewData> _buildPlaylistItems() {
    final activeIndex = _playlistItemIds.indexOf(_activePlaylistItemId ?? '');
    return _playlistItemIds
        .asMap()
        .entries
        .map((entry) {
          final index = entry.key;
          final itemId = entry.value;
          final source = _playlistSourceForItem(itemId);
          if (source == null) {
            return null;
          }
          final isActive = itemId == _activePlaylistItemId;
          return ExamplePlaylistItemViewData(
            itemId: itemId,
            label: source.label,
            status: playlistItemStatusLabel(
              index: index,
              activeIndex: activeIndex,
            ),
            isActive: isActive,
          );
        })
        .whereType<ExamplePlaylistItemViewData>()
        .toList(growable: false);
  }

  Future<void> _activatePlaylistSource(
    VesperPlayerController controller, {
    required String itemId,
    required VesperPlayerSource source,
    VesperPlayerSource? remoteSource,
    VesperPlayerSource? localSource,
  }) async {
    await _selectSource(controller, source);
    if (!mounted) {
      return;
    }
    setState(() {
      if (remoteSource != null) {
        _queuedRemoteSource = remoteSource;
      }
      if (localSource != null) {
        _queuedLocalSource = localSource;
      }
      _playlistItemIds = enqueuePlaylistItem(_playlistItemIds, itemId);
      _activePlaylistItemId = itemId;
    });
  }

  Future<void> _focusPlaylistItem(
    VesperPlayerController controller,
    String itemId,
  ) async {
    final source = _playlistSourceForItem(itemId);
    if (source == null) {
      return;
    }
    await _selectSource(controller, source);
    if (!mounted) {
      return;
    }
    setState(() {
      _activePlaylistItemId = itemId;
    });
  }

  Future<void> _playCustomUrl(VesperPlayerController controller) async {
    final uri = _remoteUrlController.text.trim();
    if (uri.isEmpty) {
      return;
    }

    final protocol = inferProtocol(uri);
    if (protocol == VesperPlayerSourceProtocol.dash &&
        !controller.capabilities.supportsDash) {
      _showSourceMessage('当前平台宿主暂不支持 DASH 流。');
      return;
    }

    final source = VesperPlayerSource.remote(
      uri: uri,
      label: '自定义远程流',
      protocol: protocol,
    );
    await _activatePlaylistSource(
      controller,
      itemId: flutterRemotePlaylistItemId,
      source: source,
      remoteSource: source,
    );
  }

  Future<void> _pickLocalVideo(VesperPlayerController controller) async {
    try {
      final pickedVideo = await ExampleLocalMediaPicker.pickVideo();
      if (!mounted || pickedVideo == null) {
        return;
      }
      final source = VesperPlayerSource.local(
        uri: pickedVideo.uri,
        label: pickedVideo.label,
      );
      await _activatePlaylistSource(
        controller,
        itemId: flutterLocalPlaylistItemId,
        source: source,
        localSource: source,
      );
      return;
    } on MissingPluginException {
      // 宿主未接 picker 时回退到手动输入，便于调试。
    } on PlatformException catch (error) {
      if (!mounted || error.code == 'cancelled') {
        return;
      }
    }

    await _promptLocalVideoFallback(controller);
  }

  Future<void> _promptLocalVideoFallback(
    VesperPlayerController controller,
  ) async {
    final localUri = await showDialog<String>(
      context: context,
      builder: (context) {
        final textController = TextEditingController();
        return AlertDialog(
          title: const Text('选择视频'),
          content: TextField(
            controller: textController,
            autofocus: true,
            decoration: const InputDecoration(
              labelText: '本地路径或 URI',
              hintText: 'file:///sdcard/Movies/demo.mp4',
            ),
          ),
          actions: <Widget>[
            TextButton(
              onPressed: () => Navigator.of(context).pop(),
              child: const Text('取消'),
            ),
            FilledButton(
              onPressed: () => Navigator.of(context).pop(textController.text),
              child: const Text('打开'),
            ),
          ],
        );
      },
    );

    if (!mounted || localUri == null || localUri.trim().isEmpty) {
      return;
    }

    final normalizedUri = normalizeLocalUri(localUri);
    final source = VesperPlayerSource.local(
      uri: normalizedUri,
      label: localSourceLabel(normalizedUri),
    );
    await _activatePlaylistSource(
      controller,
      itemId: flutterLocalPlaylistItemId,
      source: source,
      localSource: source,
    );
  }

  Future<void> _openToolSheet(
    VesperPlayerController controller,
    ExamplePlayerSheet initialSheet,
  ) async {
    if (!mounted) {
      return;
    }

    setState(() {
      _sheetOpen = true;
    });

    try {
      await showExampleSelectionSheet(
        context,
        controller: controller,
        initialSheet: initialSheet,
      );
    } finally {
      if (mounted) {
        setState(() {
          _sheetOpen = false;
        });
      }
    }
  }

  Future<void> _toggleFullscreen(Orientation orientation) async {
    if (orientation == Orientation.portrait) {
      await SystemChrome.setPreferredOrientations(const <DeviceOrientation>[
        DeviceOrientation.landscapeLeft,
        DeviceOrientation.landscapeRight,
      ]);
      await SystemChrome.setEnabledSystemUIMode(SystemUiMode.immersiveSticky);
      return;
    }

    await _restoreSystemPresentation();
  }

  Future<void> _restoreSystemPresentation() async {
    await SystemChrome.setPreferredOrientations(const <DeviceOrientation>[]);
    await SystemChrome.setEnabledSystemUIMode(SystemUiMode.edgeToEdge);
  }

  void _showSourceMessage(String message) {
    final messenger = ScaffoldMessenger.maybeOf(context);
    if (messenger == null) {
      return;
    }
    messenger
      ..hideCurrentSnackBar()
      ..showSnackBar(SnackBar(content: Text(message)));
  }

  void _disposeSilently(VesperPlayerController controller) {
    unawaited(controller.dispose().catchError((_) {}));
  }

  @override
  Widget build(BuildContext context) {
    final mediaQuery = MediaQuery.of(context);
    final isPortrait = mediaQuery.orientation == Orientation.portrait;
    final useDarkTheme = Theme.of(context).brightness == Brightness.dark;
    final palette = exampleHostPalette(useDarkTheme);

    final body = FutureBuilder<VesperPlayerController>(
      future: _controllerFuture,
      builder: (context, asyncSnapshot) {
        if (asyncSnapshot.hasError && !asyncSnapshot.hasData) {
          return ExampleErrorState(error: asyncSnapshot.error);
        }

        final controller = asyncSnapshot.data ?? _controller;
        if (controller == null) {
          return const ExampleLoadingState();
        }
        final playlistItems = _buildPlaylistItems();

        return ValueListenableBuilder<VesperPlayerSnapshot>(
          valueListenable: controller.snapshotListenable,
          builder: (context, snapshot, _) {
            final content = isPortrait
                ? _buildPortraitLayout(
                    context,
                    controller,
                    snapshot,
                    playlistItems,
                    palette,
                    asyncSnapshot,
                  )
                : _buildLandscapeLayout(controller, snapshot, asyncSnapshot);

            return Stack(
              children: <Widget>[
                Positioned.fill(child: content),
                if (_isApplyingResilienceProfile)
                  const Positioned(
                    top: 18,
                    right: 18,
                    child: ExampleBusyPill(label: '正在应用策略'),
                  ),
              ],
            );
          },
        );
      },
    );

    return Scaffold(
      body: DecoratedBox(
        decoration: BoxDecoration(
          gradient: LinearGradient(
            begin: Alignment.topCenter,
            end: Alignment.bottomCenter,
            colors: <Color>[palette.pageTop, palette.pageBottom],
          ),
        ),
        child: isPortrait ? SafeArea(child: body) : body,
      ),
    );
  }

  Widget _buildPortraitLayout(
    BuildContext context,
    VesperPlayerController controller,
    VesperPlayerSnapshot snapshot,
    List<ExamplePlaylistItemViewData> playlistItems,
    ExampleHostPalette palette,
    AsyncSnapshot<VesperPlayerController> asyncSnapshot,
  ) {
    final transientError = asyncSnapshot.hasError ? asyncSnapshot.error : null;

    return SingleChildScrollView(
      padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 18),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          ExamplePlayerHeader(
            sourceLabel: snapshot.sourceLabel.isEmpty
                ? snapshot.title
                : snapshot.sourceLabel,
            subtitle: snapshot.subtitle,
            palette: palette,
          ),
          if (transientError != null) ...<Widget>[
            const SizedBox(height: 18),
            ExampleInlineControllerError(error: transientError),
          ],
          const SizedBox(height: 18),
          SizedBox(
            width: double.infinity,
            height: 248,
            child: ExamplePlayerStage(
              controller: controller,
              snapshot: snapshot,
              isPortrait: true,
              sheetOpen: _sheetOpen,
              onOpenSheet: (sheet) =>
                  unawaited(_openToolSheet(controller, sheet)),
              onToggleFullscreen: () =>
                  unawaited(_toggleFullscreen(Orientation.portrait)),
            ),
          ),
          const SizedBox(height: 18),
          ExampleSourceSection(
            palette: palette,
            themeMode: widget.themeMode,
            remoteUrlController: _remoteUrlController,
            localFilesEnabled: snapshot.capabilities.supportsLocalFiles,
            dashEnabled: snapshot.capabilities.supportsDash,
            dashUnavailableMessage: snapshot.capabilities.supportsDash
                ? null
                : '当前平台宿主暂不支持 DASH 演示。',
            onThemeModeChange: widget.onThemeModeChange,
            onPickVideo: () => unawaited(_pickLocalVideo(controller)),
            onUseHlsDemo: () => unawaited(
              _activatePlaylistSource(
                controller,
                itemId: flutterHlsPlaylistItemId,
                source: flutterHlsDemoSource(),
              ),
            ),
            onUseDashDemo: () => unawaited(
              _activatePlaylistSource(
                controller,
                itemId: flutterDashPlaylistItemId,
                source: flutterDashDemoSource(),
              ),
            ),
            onOpenRemote: () => unawaited(_playCustomUrl(controller)),
          ),
          const SizedBox(height: 18),
          ExamplePlaylistSection(
            palette: palette,
            playlistItems: playlistItems,
            onSelectItem: (itemId) =>
                unawaited(_focusPlaylistItem(controller, itemId)),
          ),
          const SizedBox(height: 18),
          ExampleResilienceSection(
            palette: palette,
            selectedProfile: _selectedResilienceProfile,
            onApplyProfile: _applyResilienceProfile,
          ),
          if (snapshot.lastError != null) ...<Widget>[
            const SizedBox(height: 18),
            ExampleRecentErrorSection(
              palette: palette,
              error: snapshot.lastError!,
            ),
          ],
        ],
      ),
    );
  }

  Widget _buildLandscapeLayout(
    VesperPlayerController controller,
    VesperPlayerSnapshot snapshot,
    AsyncSnapshot<VesperPlayerController> asyncSnapshot,
  ) {
    return Stack(
      children: <Widget>[
        Positioned.fill(
          child: ExamplePlayerStage(
            controller: controller,
            snapshot: snapshot,
            isPortrait: false,
            sheetOpen: _sheetOpen,
            onOpenSheet: (sheet) =>
                unawaited(_openToolSheet(controller, sheet)),
            onToggleFullscreen: () =>
                unawaited(_toggleFullscreen(Orientation.landscape)),
          ),
        ),
        if (asyncSnapshot.hasError)
          Positioned(
            top: 18,
            left: 18,
            right: 96,
            child: ExampleInlineControllerError(error: asyncSnapshot.error),
          ),
      ],
    );
  }
}
