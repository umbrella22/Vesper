import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:vesper_player/vesper_player.dart';
import 'package:vesper_player_external_playback/vesper_player_external_playback.dart';
import 'package:vesper_player_ui/vesper_player_ui.dart' as ui;

import '../device/example_device_controls.dart';
import '../download/example_download_planner.dart';
import '../download/example_download_sections.dart';
import '../device/example_local_media_picker.dart';
import 'example_player_helpers.dart';
import 'example_player_models.dart';
import 'example_player_sections.dart';
import 'example_player_sheet.dart';
import 'example_player_stage.dart';
import '../hdr_evidence/hdr_evidence_capture.dart';
import '../hdr_evidence/hdr_evidence_capture_output.dart';

part 'player_host_source.dart';
part 'player_host_external_playback.dart';
part 'player_host_playlist.dart';
part 'player_host_hdr_evidence.dart';
part 'player_host_local_media.dart';
part 'player_host_downloads.dart';
part 'player_host_layout.dart';

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
  late final TextEditingController _downloadUrlController;
  final ExampleDeviceControls _deviceControls = ExampleDeviceControls();
  final VesperExternalPlaybackController _externalPlaybackController =
      VesperExternalPlaybackController();
  late Future<VesperPlayerController> _controllerFuture;
  Future<VesperDownloadManager>? _downloadManagerFuture;

  VesperPlayerController? _controller;
  VesperDownloadManager? _downloadManager;
  StreamSubscription<VesperDownloadManagerEvent>? _downloadEventsSubscription;
  StreamSubscription<List<VesperExternalPlaybackRoute>>?
  _externalRoutesSubscription;
  StreamSubscription<VesperExternalPlaybackSessionEvent>?
  _externalEventsSubscription;
  ExampleHostTab _selectedTab = ExampleHostTab.player;
  ExampleResilienceProfile _selectedResilienceProfile =
      ExampleResilienceProfile.balanced;
  ExampleSourceNormalizerSetting _sourceNormalizerSetting =
      ExampleSourceNormalizerSetting.preflightOnly;
  ExampleHdrEvidenceSamplePreset _selectedHdrEvidencePreset =
      exampleHdrEvidenceP0Presets[1];
  bool _isApplyingResilienceProfile = false;
  bool _isRebuildingController = false;
  bool _isCapturingHdrEvidence = false;
  bool _sheetOpen = false;
  List<String> _playlistItemIds = <String>[flutterHlsPlaylistItemId];
  String? _activePlaylistItemId = flutterHlsPlaylistItemId;
  String? _downloadMessage;
  String? _externalPlaybackMessage;
  bool _externalPlaybackMessageIsDiagnostic = false;
  bool _isDownloadExportPluginInstalled = false;
  List<String> _sourceNormalizerPluginLibraryPaths = const <String>[];
  List<String> _frameProcessorPluginLibraryPaths = const <String>[];
  bool _externalPlaybackPausedLocalPlayback = false;
  VesperSystemPlaybackPermissionStatus _systemPlaybackPermissionStatus =
      VesperSystemPlaybackPermissionStatus.notRequired;
  VesperPlayerSource? _queuedRemoteSource;
  VesperPlayerSource? _queuedLocalSource;
  Set<int> _savingTaskIds = <int>{};
  Map<int, double> _exportProgressByTaskId = <int, double>{};
  List<VesperExternalPlaybackRoute> _externalRoutes =
      <VesperExternalPlaybackRoute>[];
  List<ExamplePendingDownloadTask> _pendingDownloadTasks =
      <ExamplePendingDownloadTask>[];

  bool get _selectedHdrEvidencePresetUsesNetworkControl {
    return _selectedHdrEvidencePreset.sampleId == 'NETWORK-FAILURE-CONTROL';
  }

  void _updateState(VoidCallback fn) {
    setState(fn);
  }

  @override
  void initState() {
    super.initState();
    _remoteUrlController = TextEditingController(text: flutterHlsDemoUrl);
    _downloadUrlController = TextEditingController(text: flutterHlsDemoUrl);
    if (Platform.isAndroid) {
      _externalRoutesSubscription = _externalPlaybackController.routes.listen(
        _handleExternalRoutes,
      );
      _externalEventsSubscription = _externalPlaybackController.events.listen(
        _handleExternalEvent,
      );
      unawaited(_externalPlaybackController.startDiscovery());
    }
    _controllerFuture = _createController();
  }

  @override
  void dispose() {
    unawaited(_downloadEventsSubscription?.cancel() ?? Future<void>.value());
    unawaited(_externalRoutesSubscription?.cancel() ?? Future<void>.value());
    unawaited(_externalEventsSubscription?.cancel() ?? Future<void>.value());
    if (Platform.isAndroid) {
      unawaited(_externalPlaybackController.stopDiscovery());
      unawaited(_externalPlaybackController.disconnect());
    }
    final currentController = _controller;
    if (currentController != null) {
      _disposeControllerSilently(currentController);
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

    final currentDownloadManager = _downloadManager;
    if (currentDownloadManager != null) {
      _disposeDownloadManagerSilently(currentDownloadManager);
    }
    final downloadManagerFuture = _downloadManagerFuture;
    if (downloadManagerFuture != null) {
      unawaited(
        downloadManagerFuture
            .then((value) {
              if (!identical(value, currentDownloadManager)) {
                return value.dispose();
              }
            })
            .catchError((_) {}),
      );
    }

    unawaited(_restoreSystemPresentation());
    _remoteUrlController.dispose();
    _downloadUrlController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final mediaQuery = MediaQuery.of(context);
    final immersivePlayer =
        mediaQuery.orientation == Orientation.landscape &&
        _selectedTab == ExampleHostTab.player;
    final useDarkTheme = Theme.of(context).brightness == Brightness.dark;
    final palette = exampleHostPalette(useDarkTheme);

    final body = switch (_selectedTab) {
      ExampleHostTab.player => _buildPlayerFutureContent(
        context,
        immersivePlayer: immersivePlayer,
        palette: palette,
      ),
      ExampleHostTab.downloads => _buildDownloadFutureContent(palette),
    };

    return Scaffold(
      body: DecoratedBox(
        decoration: BoxDecoration(
          gradient: LinearGradient(
            begin: Alignment.topCenter,
            end: Alignment.bottomCenter,
            colors: <Color>[palette.pageTop, palette.pageBottom],
          ),
        ),
        child: immersivePlayer ? body : SafeArea(child: body),
      ),
      bottomNavigationBar: immersivePlayer
          ? null
          : NavigationBar(
              selectedIndex: _selectedTab.index,
              onDestinationSelected: (index) {
                setState(() {
                  _selectedTab = ExampleHostTab.values[index];
                });
              },
              destinations: const <Widget>[
                NavigationDestination(
                  icon: Icon(Icons.video_library_rounded),
                  label: '播放器',
                ),
                NavigationDestination(
                  icon: Icon(Icons.download_rounded),
                  label: '下载',
                ),
              ],
            ),
    );
  }
}

enum ExampleHostTab { player, downloads }
