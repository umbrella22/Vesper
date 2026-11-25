part of 'player_host_page.dart';

/// Tab bodies and sheets for the four-tab shell.
extension _PlayerHostShellPages on _PlayerHostPageState {
  Widget _buildSystemPlaybackSection(
    VesperPlayerController controller,
    ExampleHostPalette palette,
  ) {
    return ExampleSystemPlaybackSection(
      palette: palette,
      controller: controller,
      permissionStatus: _systemPlaybackPermissionStatus,
      onRefreshExternalRoutes: () => unawaited(_refreshExternalRoutes()),
      onExternalRoutePickerResult: _handleExternalRoutePickerResult,
      externalRoutes: _externalRoutes
          .where((route) => route.kind == VesperExternalPlaybackRouteKind.dlna)
          .toList(growable: false),
      externalPlaybackMessage: _externalPlaybackMessage,
      onExternalRouteSelected: (route) => unawaited(_loadExternalRoute(route)),
      onRequestPermission: () =>
          unawaited(_requestSystemPlaybackPermissions(controller)),
      pictureInPictureAvailability: _pictureInPictureAvailability,
      pictureInPictureStatus: _pictureInPictureStatus,
      pictureInPictureEnabled: _pictureInPictureEnabled,
      onPictureInPictureEnabledChanged: (enabled) =>
          unawaited(_setPictureInPictureEnabled(controller, enabled)),
      onRequestPictureInPicture: () =>
          unawaited(_requestPictureInPicture(controller)),
    );
  }

  Future<void> _openCastSheet(
    VesperPlayerController controller,
    ExampleHostPalette palette,
  ) async {
    final strings = ExampleHostStrings.of(context);
    _updateState(() => _sheetOpen = true);
    try {
      await showModalBottomSheet<void>(
        context: context,
        isScrollControlled: true,
        useSafeArea: true,
        showDragHandle: true,
        builder: (sheetContext) => ExampleHostStringsScope(
          strings: strings,
          child: StatefulBuilder(
            // Rebuild on parent state changes is not needed: the section
            // reads live controller state through its own listenables.
            builder: (context, _) => SingleChildScrollView(
              padding: const EdgeInsets.fromLTRB(18, 0, 18, 28),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: <Widget>[
                  Text(
                    strings[ExampleHostText.castSheetTitle],
                    style: Theme.of(context).textTheme.titleLarge,
                  ),
                  const SizedBox(height: 12),
                  _buildSystemPlaybackSection(controller, palette),
                ],
              ),
            ),
          ),
        ),
      );
    } finally {
      if (mounted) {
        _updateState(() => _sheetOpen = false);
      }
    }
  }

  Widget _buildDolbyCatalogPanel(
    VesperPlayerController controller,
    ExampleHostPalette palette,
  ) {
    return ExampleDolbyCatalogPanel(
      palette: palette,
      presets: exampleDolbyAcceptanceCatalog,
      selectedDrmKind: _selectedDolbyDrmKind,
      selectedProfile: _selectedDolbyProfile,
      selectedFps: _selectedDolbyFps,
      isPresetPlayable: _isDolbyAcceptancePresetPlayableOnCurrentPlatform,
      disabledReasonForPreset: _dolbyAcceptancePresetUnavailableReason,
      onDrmKindChanged: (value) =>
          _updateState(() => _selectedDolbyDrmKind = value),
      onProfileChanged: (value) =>
          _updateState(() => _selectedDolbyProfile = value),
      onFpsChanged: (value) => _updateState(() => _selectedDolbyFps = value),
      onPresetPlayNow: (preset) => unawaited(() async {
        await _activateDolbyAcceptancePreset(
          controller,
          preset,
          origin: ExampleDolbyAdHocPlaybackOrigin(preset.id),
        );
        if (mounted &&
            _isDolbyAcceptancePresetPlayableOnCurrentPlatform(preset)) {
          _setNavigation(_navigation.showPlayer());
        }
      }()),
      onPresetAddToQueue: (preset) {
        _addDolbyPresetToQueue(preset);
        _showMessage(
          ExampleHostStrings.of(context)[ExampleHostText.libraryAddedToQueue],
        );
      },
    );
  }

  Widget _buildLibraryContent(double bottomPadding) {
    final collectionId = _navigation.openCollection;
    if (collectionId == null) {
      return ExampleLibraryHome(
        bottomPadding: bottomPadding,
        onOpenCollection: (id) =>
            _setNavigation(_navigation.showCollection(id)),
      );
    }
    final collection = exampleLibraryCollectionById(collectionId);
    final controller = _controller;
    final palette = exampleHostPalette(
      Theme.of(context).brightness == Brightness.dark,
    );
    return ExampleLibraryCollectionDetail(
      collection: collection,
      bottomPadding: bottomPadding,
      dashEnabled: controller?.capabilities.supportsDash ?? false,
      onPlayItem: _playLibraryItem,
      dolbyCatalog: controller == null
          ? null
          : _buildDolbyCatalogPanel(controller, palette),
    );
  }
}
