part of 'player_host_page.dart';

extension _PlayerHostPlaylistActions on _PlayerHostPageState {
  VesperPlayerSource? _playlistSourceForItem(String itemId) {
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
    _updateState(() {
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
    _updateState(() {
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
      _showMessage('当前平台宿主暂不支持 DASH 流。');
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
}
