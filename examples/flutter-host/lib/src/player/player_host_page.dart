import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:material_ui/material_ui.dart';
import 'package:flutter/services.dart';
import 'package:vesper_player/vesper_player.dart';
import 'package:vesper_player_external_playback/vesper_player_external_playback.dart';
import 'package:vesper_player_ui/vesper_player_ui.dart' as ui;

import '../device/example_device_controls.dart';
import '../download/example_download_planner.dart';
import '../download/example_download_sections.dart';
import '../device/example_local_media_picker.dart';
import 'example_dolby_acceptance_catalog.dart';
import 'example_player_helpers.dart';
import 'example_source_activation.dart';
import 'example_player_models.dart';
import 'example_player_sections.dart';
import 'example_player_sheet.dart';
import 'example_player_stage.dart';
import '../hdr_evidence/hdr_evidence_capture.dart';
import '../hdr_evidence/hdr_evidence_capture_output.dart';
import '../shell/example_host_navigation.dart';
import '../shell/example_host_shell.dart';
import '../shell/example_host_strings.dart';
import '../shell/example_library_catalog.dart';
import '../shell/example_library_pages.dart';
import '../shell/example_open_media_sheet.dart';
import '../shell/example_play_widgets.dart';
import '../shell/example_settings_sheet.dart';

part 'player_host_source.dart';
part 'player_host_shell_actions.dart';
part 'player_host_shell_pages.dart';
part 'player_host_shell_downloads.dart';
part 'player_host_external_playback.dart';
part 'player_host_playlist.dart';
part 'player_host_hdr_evidence.dart';
part 'player_host_local_media.dart';
part 'player_host_downloads.dart';
part 'player_host_picture_in_picture.dart';
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

class _PlayerHostPageState extends State<PlayerHostPage>
    with WidgetsBindingObserver {
  late final TextEditingController _remoteUrlController;
  late final TextEditingController _downloadUrlController;
  final ExampleDeviceControls _deviceControls = ExampleDeviceControls();
  final VesperExternalPlaybackController _externalPlaybackController =
      VesperExternalPlaybackController();
  late Future<VesperPlayerController> _controllerFuture;
  Future<VesperDownloadManager>? _downloadManagerFuture;

  VesperPlayerController? _controller;
  final Set<VesperPlayerController> _pendingControllers =
      <VesperPlayerController>{};
  int _sourceRequestGeneration = 0;
  int _pictureInPictureBindingGeneration = 0;
  bool _isClosing = false;
  VesperDownloadManager? _downloadManager;
  StreamSubscription<VesperDownloadManagerEvent>? _downloadEventsSubscription;
  StreamSubscription<VesperPlayerPictureInPictureEvent>?
  _pictureInPictureSubscription;
  StreamSubscription<List<VesperExternalPlaybackRoute>>?
  _externalRoutesSubscription;
  StreamSubscription<VesperExternalPlaybackSessionEvent>?
  _externalEventsSubscription;
  ExampleHostNavigationState _navigation = const ExampleHostNavigationState();
  double _fabHeight = 0;
  bool _fabExtended = true;
  Set<int> _pendingRemovalTaskIds = <int>{};
  ExampleResilienceProfile _selectedResilienceProfile =
      ExampleResilienceProfile.balanced;
  ExampleSourceNormalizerSetting _sourceNormalizerSetting =
      ExampleSourceNormalizerSetting.preflightOnly;
  ExampleDolbyAcceptanceDrmKind _selectedDolbyDrmKind =
      ExampleDolbyAcceptanceDrmKind.clear;
  ExampleDolbyAcceptanceProfile? _selectedDolbyProfile;
  int? _selectedDolbyFps;
  ExampleHdrEvidenceSamplePreset _selectedHdrEvidencePreset =
      exampleHdrEvidenceP0Presets[1];
  bool _isApplyingResilienceProfile = false;
  bool _isRebuildingController = false;
  bool _isCapturingHdrEvidence = false;
  bool _sheetOpen = false;
  bool _hasHandledFinishedPlayback = false;
  List<String> _playlistItemIds = <String>[flutterHlsPlaylistItemId];
  String? _activePlaylistItemId = flutterHlsPlaylistItemId;
  VesperPlayerSource? _activeDirectSource;
  ExamplePlaybackOrigin? _playbackOrigin = const ExampleQueuePlaybackOrigin(
    flutterHlsPlaylistItemId,
  );
  VesperPlayerController? _observedController;
  String? _downloadMessage;
  String? _externalPlaybackMessage;
  bool _externalPlaybackMessageIsDiagnostic = false;
  bool _isDownloadExportPluginInstalled = false;
  List<VesperPluginReference> _sourceNormalizerPluginReferences =
      const <VesperPluginReference>[];
  List<VesperPluginReference> _frameProcessorPluginReferences =
      const <VesperPluginReference>[];
  bool _externalPlaybackPausedLocalPlayback = false;
  VesperSystemPlaybackPermissionStatus _systemPlaybackPermissionStatus =
      VesperSystemPlaybackPermissionStatus.notRequired;
  bool _pictureInPictureEnabled = false;
  bool _pictureInPicturePresentation = false;
  VesperPictureInPictureAvailability? _pictureInPictureAvailability;
  VesperPictureInPictureStatus _pictureInPictureStatus =
      VesperPictureInPictureStatus.inactive;
  static const MethodChannel _pictureInPictureHostChannel = MethodChannel(
    'io.github.umbrella22.vesper.example.flutter_host/picture_in_picture',
  );
  VesperPlayerSource? _queuedRemoteSource;
  VesperPlayerSource? _queuedLocalSource;
  Set<int> _savingTaskIds = <int>{};
  Map<int, double> _exportProgressByTaskId = <int, double>{};
  List<VesperExternalPlaybackRoute> _externalRoutes =
      <VesperExternalPlaybackRoute>[];
  List<ExamplePendingDownloadTask> _pendingDownloadTasks =
      <ExamplePendingDownloadTask>[];
  List<ExampleHostLogEntry> _hostLogEntries = const <ExampleHostLogEntry>[];
  int _nextHostLogId = 1;

  bool get _selectedHdrEvidencePresetUsesNetworkControl {
    return _selectedHdrEvidencePreset.sampleId == 'NETWORK-FAILURE-CONTROL';
  }

  void _updateState(VoidCallback fn) {
    setState(fn);
  }

  void _appendHostLog({
    ExampleHostLogSeverity severity = ExampleHostLogSeverity.info,
    required String title,
    String? detail,
  }) {
    _hostLogEntries = appendExampleHostLogEntry(
      _hostLogEntries,
      ExampleHostLogEntry(
        id: _nextHostLogId,
        atMillis: DateTime.now().millisecondsSinceEpoch,
        severity: severity,
        title: title,
        detail: detail,
      ),
    );
    _nextHostLogId += 1;
  }

  VesperPlayerSource? _activePlaybackSource() {
    return _activeDirectSource ??
        (_activePlaylistItemId == null
            ? null
            : _playlistSourceForItem(_activePlaylistItemId!));
  }

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _pictureInPictureHostChannel.setMethodCallHandler(
      _handlePictureInPictureHostCall,
    );
    _remoteUrlController = TextEditingController(text: flutterHlsDemoUrl);
    _downloadUrlController = TextEditingController(text: flutterHlsDemoUrl);
    if (Platform.isAndroid) {
      _externalRoutesSubscription = _externalPlaybackController.routes.listen(
        _handleExternalRoutes,
      );
      _externalEventsSubscription = _externalPlaybackController.events.listen(
        _handleExternalEvent,
      );
    }
    _controllerFuture = _createController();
  }

  @override
  void dispose() {
    _isClosing = true;
    _sourceRequestGeneration++;
    _pictureInPictureBindingGeneration++;
    for (final pending in _pendingControllers.toList()) {
      _disposeControllerSilently(pending);
    }
    _pendingControllers.clear();
    WidgetsBinding.instance.removeObserver(this);
    _pictureInPictureHostChannel.setMethodCallHandler(null);
    _observedController?.snapshotListenable.removeListener(
      _handleControllerSnapshotChanged,
    );
    unawaited(_downloadEventsSubscription?.cancel() ?? Future<void>.value());
    unawaited(_pictureInPictureSubscription?.cancel() ?? Future<void>.value());
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
    final autoFullscreen = exampleShouldAutoFullscreen(
      isLandscape: mediaQuery.orientation == Orientation.landscape,
      windowHeight: mediaQuery.size.height,
    );
    final isFullscreen = _navigation.isFullscreen(
      autoFullscreen: autoFullscreen,
    );
    final useDarkTheme = Theme.of(context).brightness == Brightness.dark;
    final palette = exampleHostPalette(useDarkTheme);
    final strings = ExampleHostStrings.of(context);

    if (_pictureInPicturePresentation && Platform.isAndroid) {
      return Scaffold(
        backgroundColor: Colors.black,
        body: _buildPictureInPicturePresentationContent(),
      );
    }

    final listBottomPadding = exampleFabAwareBottomPadding(
      fabHeight: _navigation.tab == ExampleHostTab.play ? _fabHeight : 0,
    );
    final body = switch (_navigation.tab) {
      ExampleHostTab.play => _buildPlayerFutureContent(
        context,
        immersivePlayer: isFullscreen,
        palette: palette,
        bottomPadding: listBottomPadding,
      ),
      ExampleHostTab.library => _buildLibraryContent(listBottomPadding),
      ExampleHostTab.downloads => _buildDownloadFutureContent(palette),
      ExampleHostTab.lab => _buildDiagnosticsFutureContent(
        context,
        palette: palette,
      ),
    };
    final openCollection = _navigation.isCollectionVisible
        ? exampleLibraryCollectionById(_navigation.openCollection!)
        : null;

    return ExampleHostShell(
      navigation: _navigation,
      isFullscreen: isFullscreen,
      title: openCollection != null
          ? strings[openCollection.titleKey]
          : _navigation.tab == ExampleHostTab.play
          ? strings[ExampleHostText.appTitle]
          : strings[_navigation.tab.textKey],
      body: body,
      fabExtended: _fabExtended,
      onFabSize: _handleFabSize,
      onSelectTab: _selectShellTab,
      onSystemBack: _handleSystemBack,
      onTitleBack: openCollection == null
          ? null
          : () => _setNavigation(_navigation.closeCollection()),
      onOpenSettings: () => unawaited(
        showExampleSettingsSheet(
          context,
          themeMode: widget.themeMode,
          onThemeModeChange: widget.onThemeModeChange,
        ),
      ),
      onOpenMedia: () => unawaited(_openMediaSheet()),
    );
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    super.didChangeAppLifecycleState(state);
    if (!Platform.isAndroid) {
      return;
    }
    if (state == AppLifecycleState.resumed &&
        _pictureInPictureStatus != VesperPictureInPictureStatus.active) {
      _setPictureInPicturePresentation(false);
    }
  }

  void _observeControllerSnapshot(VesperPlayerController controller) {
    if (identical(_observedController, controller)) {
      return;
    }
    _observedController?.snapshotListenable.removeListener(
      _handleControllerSnapshotChanged,
    );
    _observedController = controller;
    controller.snapshotListenable.addListener(_handleControllerSnapshotChanged);
  }

  void _handleControllerSnapshotChanged() {
    final controller = _observedController;
    if (controller == null || !mounted) {
      return;
    }
    final snapshot = controller.snapshot;
    if (snapshot.playbackState != VesperPlaybackState.finished) {
      _hasHandledFinishedPlayback = false;
      return;
    }
    if (_hasHandledFinishedPlayback ||
        !shouldAdvancePlaylistOnFinished(
          origin: _playbackOrigin,
          activeItemId: _activePlaylistItemId,
        )) {
      return;
    }
    _hasHandledFinishedPlayback = true;
    final nextItemId = nextPlaylistItemIdOnFinished(
      playlistItemIds: _playlistItemIds,
      activeItemId: _activePlaylistItemId,
    );
    if (nextItemId != null) {
      unawaited(_focusPlaylistItem(controller, nextItemId));
    }
  }
}
