part of 'player_host_page.dart';

extension _PlayerHostExternalPlaybackActions on _PlayerHostPageState {
  void _handleExternalRoutes(List<VesperExternalPlaybackRoute> routes) {
    if (!mounted) {
      return;
    }
    _updateState(() {
      _externalRoutes = routes;
    });
  }

  Future<void> _refreshExternalRoutes() async {
    if (!Platform.isAndroid) {
      return;
    }
    await _externalPlaybackController.startDiscovery();
    if (!mounted) {
      return;
    }
    _updateState(() {
      _setExternalPlaybackMessage('正在重新扫描 DLNA 设备。', force: true);
    });
  }

  void _handleExternalEvent(VesperExternalPlaybackSessionEvent event) {
    unawaited(_handleExternalEventAsync(event));
  }

  Future<void> _handleExternalEventAsync(
    VesperExternalPlaybackSessionEvent event,
  ) async {
    if (!mounted) {
      return;
    }

    switch (event.kind) {
      case VesperExternalPlaybackSessionEventKind.routeConnected:
        final routeLabel = event.routeName ?? event.routeId ?? '设备';
        _updateState(() {
          _setExternalPlaybackMessage('外部播放已连接：$routeLabel', force: true);
        });
        if (event.routeId == VesperExternalPlaybackController.castRouteId) {
          await _loadCurrentExternalMedia(routeLabel: routeLabel);
        }
      case VesperExternalPlaybackSessionEventKind.routeDisconnected:
        await _resumeLocalPlaybackFromExternal(event.positionMs);
        if (!mounted) {
          return;
        }
        _updateState(() {
          _externalPlaybackPausedLocalPlayback = false;
          _setExternalPlaybackMessage('外部播放已断开，本地播放已恢复。', force: true);
        });
      case VesperExternalPlaybackSessionEventKind.loaded:
        _updateState(() {
          _setExternalPlaybackMessage('外部播放媒体已加载。');
        });
      case VesperExternalPlaybackSessionEventKind.playing:
        _updateState(() {
          _setExternalPlaybackMessage('外部播放已继续。');
        });
      case VesperExternalPlaybackSessionEventKind.paused:
        _updateState(() {
          _setExternalPlaybackMessage('外部播放已暂停。');
        });
      case VesperExternalPlaybackSessionEventKind.stopped:
        _updateState(() {
          _setExternalPlaybackMessage('外部播放已停止。');
        });
      case VesperExternalPlaybackSessionEventKind.suspended:
        _updateState(() {
          _setExternalPlaybackMessage('外部播放连接已暂停。');
        });
      case VesperExternalPlaybackSessionEventKind.discoveryDiagnostic:
        if (event.details['severity'] != 'info') {
          _updateState(() {
            _setExternalPlaybackMessage(
              _formatExternalPlaybackDiagnostic(event),
              diagnostic: true,
            );
          });
        }
      case VesperExternalPlaybackSessionEventKind.error:
        _updateState(() {
          _setExternalPlaybackMessage(
            event.message ?? '外部播放发生错误。',
            diagnostic: true,
          );
        });
    }
  }

  Future<void> _loadExternalRoute(VesperExternalPlaybackRoute route) async {
    if (mounted) {
      _updateState(() {
        _setExternalPlaybackMessage('正在连接外部播放：${route.name}', force: true);
      });
    }
    final connectResult = await _externalPlaybackController.connect(
      route.routeId,
    );
    if (!connectResult.isSuccess) {
      if (mounted) {
        _updateState(() {
          _setExternalPlaybackMessage(connectResult.message, diagnostic: true);
        });
      }
      return;
    }
    await _loadCurrentExternalMedia(routeLabel: route.name);
  }

  Future<void> _loadCurrentExternalMedia({required String routeLabel}) async {
    final controller = _controller ?? await _controllerFuture;
    final source = _activePlaylistItemId == null
        ? null
        : _playlistSourceForItem(_activePlaylistItemId!);
    if (source == null) {
      return;
    }
    final wasPlaying =
        controller.snapshot.playbackState == VesperPlaybackState.playing;
    final shouldAutoplay = wasPlaying || _externalPlaybackPausedLocalPlayback;
    final loadResult = await _externalPlaybackController.load(
      VesperExternalPlaybackMediaItem(
        sources: <VesperPlayerSource>[source],
        metadata: _systemPlaybackMetadataForSource(source),
        formatAdaptation: _externalFormatAdaptationForSource(source),
      ),
      startPositionMs: controller.snapshot.timeline.positionMs,
      autoplay: shouldAutoplay,
    );
    if (loadResult.isSuccess && wasPlaying) {
      await controller.pause();
    }
    if (!mounted) {
      return;
    }
    _updateState(() {
      if (loadResult.isSuccess && shouldAutoplay) {
        _externalPlaybackPausedLocalPlayback = true;
      }
      _setExternalPlaybackMessage(
        loadResult.isSuccess ? '外部播放已加载：$routeLabel' : loadResult.message,
        diagnostic: !loadResult.isSuccess,
      );
    });
  }

  Future<void> _resumeLocalPlaybackFromExternal(int? positionMs) async {
    if (!_externalPlaybackPausedLocalPlayback) {
      return;
    }
    final controller = _controller;
    if (controller == null) {
      return;
    }
    if (positionMs != null) {
      final deltaMs = positionMs - controller.snapshot.timeline.positionMs;
      await controller.seekBy(deltaMs);
    }
    await controller.play();
  }

  void _setExternalPlaybackMessage(
    String? message, {
    bool diagnostic = false,
    bool force = false,
  }) {
    if (!force && !diagnostic && _externalPlaybackMessageIsDiagnostic) {
      return;
    }
    _externalPlaybackMessage = message;
    _externalPlaybackMessageIsDiagnostic = diagnostic;
  }

  String _formatExternalPlaybackDiagnostic(
    VesperExternalPlaybackSessionEvent event,
  ) {
    final labels = <String>[
      if (event.code != null) 'code=${event.code}',
      if (event.details['httpStatus'] != null)
        "http=${event.details['httpStatus']}",
      if (event.details['fallbackFormat'] != null)
        "fallback=${event.details['fallbackFormat']}",
      if (event.details['inputMode'] != null)
        "mode=${event.details['inputMode']}",
    ];
    final suffix = labels.isEmpty ? '' : ' (${labels.join(', ')})';
    return '${event.message ?? '外部播放诊断事件。'}$suffix';
  }

  VesperSystemPlaybackMetadata _systemPlaybackMetadataForSource(
    VesperPlayerSource source,
  ) {
    return VesperSystemPlaybackMetadata(
      title: source.label,
      artist: 'Vesper Player SDK',
      contentUri: source.uri,
    );
  }

  VesperExternalFormatAdaptationConfig _externalFormatAdaptationForSource(
    VesperPlayerSource source,
  ) {
    if (source.protocol == VesperPlayerSourceProtocol.dash) {
      return const VesperExternalFormatAdaptationConfig.dlnaRemux(
        debugDiagnostics: true,
      );
    }
    return const VesperExternalFormatAdaptationConfig.disabled();
  }
}
