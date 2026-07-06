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
      _appendHostLog(title: '已选择 source', detail: source.label);
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

  Future<void> _activateDolbyAcceptancePreset(
    VesperPlayerController controller,
    ExampleDolbyAcceptancePreset preset, {
    required ExamplePlaybackOrigin origin,
  }) async {
    final unavailableReason = _dolbyAcceptancePresetUnavailableReason(preset);
    if (unavailableReason != null) {
      _showMessage(unavailableReason);
      if (mounted) {
        _updateState(() {
          _appendHostLog(
            severity: ExampleHostLogSeverity.warning,
            title: '已选择 Dolby 预设',
            detail: unavailableReason,
          );
        });
      }
      return;
    }
    if (preset.protocol == VesperPlayerSourceProtocol.dash &&
        !controller.capabilities.supportsDash) {
      _showMessage('当前平台宿主暂不支持 DASH Dolby 验收流。');
      return;
    }
    if (_sourceNormalizerSetting != ExampleSourceNormalizerSetting.disabled &&
        _sourceNormalizerSetting !=
            ExampleSourceNormalizerSetting.diagnosticsOnly) {
      _updateState(() {
        _sourceNormalizerSetting = ExampleSourceNormalizerSetting.disabled;
        _activeDirectSource = preset.source;
        _playbackOrigin = origin;
      });
      _showMessage('Dolby 验收已切回 direct native 路径，SourceNormalizer 已关闭。');
      await _rebuildControllerForSource(
        preset.source,
        shouldResumePlayback:
            controller.snapshot.playbackState == VesperPlaybackState.playing,
      );
    } else {
      await _selectSource(controller, preset.source, origin: origin);
    }
    if (!mounted) {
      return;
    }
    _updateState(() {
      _selectedHdrEvidencePreset = preset.toHdrEvidencePreset();
      _appendHostLog(title: '已选择 Dolby 预设', detail: preset.label);
    });
  }

  void _addDolbyPresetToQueue(ExampleDolbyAcceptancePreset preset) {
    if (!canQueueDolbyAcceptancePresetOnHost(
      preset,
      isAndroid: Platform.isAndroid,
      isIOS: Platform.isIOS,
    )) {
      final reason =
          _dolbyAcceptancePresetUnavailableReason(preset) ??
          '这个 Dolby 预设暂不能加入队列。';
      _showMessage(reason);
      _updateState(() {
        _appendHostLog(
          severity: ExampleHostLogSeverity.warning,
          title: 'Dolby 预设已加入队列',
          detail: reason,
        );
      });
      return;
    }
    final itemId = flutterDolbyPlaylistItemId(preset.id);
    _updateState(() {
      _playlistItemIds = enqueuePlaylistItem(_playlistItemIds, itemId);
      _appendHostLog(title: 'Dolby 预设已加入队列', detail: preset.label);
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
