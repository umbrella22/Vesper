part of 'player_host_page.dart';

extension _PlayerHostPlaylistActions on _PlayerHostPageState {
  VesperPlayerSource? _playlistSourceForItem(String itemId) {
    final dolbyPresetId = flutterDolbyPresetIdFromPlaylistItemId(itemId);
    final dolbyPreset = dolbyPresetId == null
        ? null
        : exampleDolbyAcceptancePresetById(dolbyPresetId);
    if (dolbyPreset != null) {
      return dolbyPreset.source;
    }
    return switch (itemId) {
      flutterHlsPlaylistItemId => flutterHlsDemoSource(),
      flutterDashPlaylistItemId => flutterDashDemoSource(),
      flutterLiveDvrPlaylistItemId => flutterLiveDvrAcceptanceSource(),
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
              strings: ExampleHostStrings.of(context),
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
    await _selectSource(
      controller,
      source,
      origin: ExampleQueuePlaybackOrigin(itemId),
    );
    if (!mounted) {
      return;
    }
    _updateState(() {
      if (remoteSource != null) {
        _queuedRemoteSource = remoteSource;
      }
      if (localSource != null) {
        _queuedLocalSource = localSource;
      }
      _playlistItemIds = enqueuePlaylistItem(_playlistItemIds, itemId);
      _activePlaylistItemId = itemId;
      _appendHostLog(
        title: _hostStrings[ExampleHostText.logSourceSelected],
        detail: source.label,
      );
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
    final dolbyPresetId = flutterDolbyPresetIdFromPlaylistItemId(itemId);
    if (dolbyPresetId != null) {
      final preset = exampleDolbyAcceptancePresetById(dolbyPresetId);
      if (preset != null) {
        await _activateDolbyAcceptancePreset(
          controller,
          preset,
          origin: ExampleQueuePlaybackOrigin(itemId),
        );
      }
    } else {
      await _selectSource(
        controller,
        source,
        origin: ExampleQueuePlaybackOrigin(itemId),
      );
    }
    if (!mounted) {
      return;
    }
    _updateState(() {
      _activePlaylistItemId = itemId;
    });
  }

  /// Plays the URL in [_remoteUrlController]. Returns `null` on success or
  /// the error copy to show in the open-media sheet.
  Future<String?> _playCustomUrl(VesperPlayerController controller) async {
    final strings = ExampleHostStrings.of(context);
    final uri = _remoteUrlController.text.trim();
    final errorKey = exampleValidateStreamUrl(
      uri,
      dashSupported: controller.capabilities.supportsDash,
    );
    if (errorKey != null) {
      return strings[errorKey];
    }

    final protocol = inferProtocol(uri);
    if (protocol == VesperPlayerSourceProtocol.dash &&
        !controller.capabilities.supportsDash) {
      return strings[ExampleHostText.urlErrorDashUnsupported];
    }

    final source = VesperPlayerSource.remote(
      uri: uri,
      label: strings[ExampleHostText.sourceLabelCustomRemote],
      protocol: protocol,
    );
    await _activatePlaylistSource(
      controller,
      itemId: flutterRemotePlaylistItemId,
      source: source,
      remoteSource: source,
    );
    return null;
  }

  Future<void> _activateDolbyAcceptancePreset(
    VesperPlayerController controller,
    ExampleDolbyAcceptancePreset preset, {
    required ExamplePlaybackOrigin origin,
  }) async {
    final strings = _hostStrings;
    final unavailableReason = _dolbyAcceptancePresetUnavailableReason(preset);
    if (unavailableReason != null) {
      _showMessage(unavailableReason);
      if (mounted) {
        _updateState(() {
          _appendHostLog(
            severity: ExampleHostLogSeverity.warning,
            title: strings[ExampleHostText.logDolbySelected],
            detail: unavailableReason,
          );
        });
      }
      return;
    }
    if (preset.protocol == VesperPlayerSourceProtocol.dash &&
        !controller.capabilities.supportsDash) {
      _showMessage(strings[ExampleHostText.dolbyDashUnsupported]);
      return;
    }
    await _selectSource(controller, preset.source, origin: origin);
    if (!mounted) {
      return;
    }
    _updateState(() {
      _selectedHdrEvidencePreset = preset.toHdrEvidencePreset();
      _appendHostLog(
        title: strings[ExampleHostText.logDolbySelected],
        detail: preset.label,
      );
    });
  }

  void _addDolbyPresetToQueue(ExampleDolbyAcceptancePreset preset) {
    final strings = _hostStrings;
    if (!canQueueDolbyAcceptancePresetOnHost(
      preset,
      isAndroid: Platform.isAndroid,
      isIOS: Platform.isIOS,
    )) {
      final reason =
          _dolbyAcceptancePresetUnavailableReason(preset) ??
          strings[ExampleHostText.dolbyQueueUnavailable];
      _showMessage(reason);
      _updateState(() {
        _appendHostLog(
          severity: ExampleHostLogSeverity.warning,
          title: strings[ExampleHostText.logDolbyQueued],
          detail: reason,
        );
      });
      return;
    }
    final itemId = flutterDolbyPlaylistItemId(preset.id);
    _updateState(() {
      _playlistItemIds = enqueuePlaylistItem(_playlistItemIds, itemId);
      _appendHostLog(
        title: strings[ExampleHostText.logDolbyQueued],
        detail: preset.label,
      );
    });
  }

  bool _isDolbyAcceptancePresetPlayableOnCurrentPlatform(
    ExampleDolbyAcceptancePreset preset,
  ) {
    return _dolbyAcceptancePresetUnavailableReason(preset) == null;
  }

  String? _dolbyAcceptancePresetUnavailableReason(
    ExampleDolbyAcceptancePreset preset,
  ) {
    return exampleDolbyAcceptancePresetUnavailableReasonOnHost(
      preset,
      isAndroid: Platform.isAndroid,
      isIOS: Platform.isIOS,
    );
  }
}
