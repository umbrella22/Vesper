part of 'player_host_page.dart';

/// Shell-level intents: navigation, back handling, the open-media sheet,
/// quick actions, and deferred download removal with undo.
extension _PlayerHostShellActions on _PlayerHostPageState {
  void _setNavigation(ExampleHostNavigationState next) {
    if (next == _navigation) {
      return;
    }
    _updateState(() => _navigation = next);
  }

  void _selectShellTab(ExampleHostTab tab) {
    _setNavigation(_navigation.selectTab(tab));
  }

  void _handleSystemBack(ExampleBackAction action) {
    switch (action) {
      case ExampleBackAction.exitFullscreen:
        unawaited(_toggleFullscreen(Orientation.landscape));
      case ExampleBackAction.closeCollection:
      case ExampleBackAction.goToPlayTab:
        _setNavigation(_navigation.applyBack(action));
      case ExampleBackAction.exitApp:
        break;
    }
  }

  void _handleFabSize(Size size) {
    if ((size.height - _fabHeight).abs() < 0.5) {
      return;
    }
    _updateState(() => _fabHeight = size.height);
  }

  bool _handlePlayScroll(ScrollUpdateNotification notification) {
    if (notification.depth != 0 || notification.metrics.axis != Axis.vertical) {
      return false;
    }
    final extended = exampleFabExtended(notification.metrics.pixels);
    if (extended != _fabExtended) {
      _updateState(() => _fabExtended = extended);
    }
    return false;
  }

  List<ExampleQuickAction> _buildPlayQuickActions(
    BuildContext context,
    VesperPlayerController controller,
    ExampleHostPalette palette,
    ExampleHostStrings strings,
  ) {
    final pipAvailable = _pictureInPictureAvailability?.isAvailable ?? false;
    final castSupported =
        defaultTargetPlatform == TargetPlatform.android ||
        defaultTargetPlatform == TargetPlatform.iOS;
    return <ExampleQuickAction>[
      if (pipAvailable)
        ExampleQuickAction(
          id: 'pip',
          icon: Icons.picture_in_picture_alt_rounded,
          label: strings[ExampleHostText.quickPictureInPicture],
          onTap: () => unawaited(_startPictureInPictureFromShell(controller)),
        ),
      if (castSupported)
        ExampleQuickAction(
          id: 'cast',
          icon: Icons.cast_rounded,
          label: strings[ExampleHostText.quickCast],
          onTap: () => unawaited(_openCastSheet(controller, palette)),
        ),
      ExampleQuickAction(
        id: 'tracks',
        icon: Icons.subtitles_outlined,
        label: strings[ExampleHostText.quickTracks],
        onTap: () =>
            unawaited(_openToolSheet(controller, ExamplePlayerSheet.audio)),
      ),
    ];
  }

  Future<void> _startPictureInPictureFromShell(
    VesperPlayerController controller,
  ) async {
    if (!_pictureInPictureEnabled) {
      await _setPictureInPictureEnabled(controller, true);
    }
    if (!mounted) {
      return;
    }
    await _requestPictureInPicture(controller);
  }

  Future<void> _openMediaSheet() async {
    final controller = _controller;
    if (controller == null) {
      return;
    }
    final opened = await showExampleOpenMediaSheet(
      context,
      sheet: ExampleOpenMediaSheet(
        initialUrl: _remoteUrlController.text,
        localFilesEnabled: controller.capabilities.supportsLocalFiles,
        dashEnabled: controller.capabilities.supportsDash,
        onPickLocal: () async =>
            await _pickLocalVideo(_controller ?? controller) ? null : '',
        onOpenUrl: (url) {
          _remoteUrlController.text = url;
          return _playCustomUrl(_controller ?? controller);
        },
        onOpenSample: (item) =>
            _openLibraryItem(_controller ?? controller, item),
      ),
    );
    if (opened && mounted) {
      _setNavigation(_navigation.showPlayer());
    }
  }

  /// Queues and plays [item]. Returns `null` on success or the error copy.
  Future<String?> _openLibraryItem(
    VesperPlayerController controller,
    ExampleLibraryItem item,
  ) async {
    final strings = ExampleHostStrings.of(context);
    if (item.requiresDash && !controller.capabilities.supportsDash) {
      return strings[ExampleHostText.dashUnavailable];
    }
    final source = _playlistSourceForItem(item.playlistItemId);
    if (source == null) {
      return strings[ExampleHostText.openMediaFailed];
    }
    try {
      await _activatePlaylistSource(
        controller,
        itemId: item.playlistItemId,
        source: source,
      );
    } catch (_) {
      return strings[ExampleHostText.openMediaFailed];
    }
    return null;
  }

  Future<void> _playLibraryItem(ExampleLibraryItem item) async {
    final controller = _controller;
    if (controller == null) {
      return;
    }
    final request = ++_libraryPlayRequest;
    final error = await _openLibraryItem(controller, item);
    if (!mounted || request != _libraryPlayRequest) return;
    if (error != null) {
      _showMessage(error);
      return;
    }
    _setNavigation(_navigation.showPlayer());
  }
}
