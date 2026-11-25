part of 'player_host_page.dart';

extension _PlayerHostExternalPlaybackActions on _PlayerHostPageState {
  ExampleHostStrings get _hostStrings => ExampleHostStrings.of(context);

  void _handleExternalRoutePickerResult(VesperExternalPlaybackResult result) {
    if (result.isSuccess) {
      return;
    }
    final strings = _hostStrings;
    _updateState(() {
      _setExternalPlaybackMessage(
        result.message ?? strings[ExampleHostText.externalUnavailable],
        diagnostic: true,
        force: true,
      );
      _appendHostLog(
        severity: ExampleHostLogSeverity.warning,
        title: strings[ExampleHostText.logExternalUnavailable],
        detail: result.message,
      );
    });
  }

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
    final strings = _hostStrings;
    _updateState(() {
      _setExternalPlaybackMessage(
        strings[ExampleHostText.externalRescanning],
        force: true,
      );
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
    final strings = _hostStrings;

    switch (event.kind) {
      case VesperExternalPlaybackSessionEventKind.routeConnected:
        final routeLabel =
            event.routeName ??
            event.routeId ??
            strings[ExampleHostText.externalDevice];
        _updateState(() {
          _setExternalPlaybackMessage(
            strings.format(ExampleHostText.externalConnected, <String, Object?>{
              'route': routeLabel,
            }),
            force: true,
          );
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
          _setExternalPlaybackMessage(
            strings[ExampleHostText.externalDisconnected],
            force: true,
          );
          _appendHostLog(
            title: strings[ExampleHostText.logExternalDisconnected],
            detail: event.routeName ?? event.routeId,
          );
        });
      case VesperExternalPlaybackSessionEventKind.loaded:
        _updateState(() {
          _setExternalPlaybackMessage(
            strings[ExampleHostText.externalLoadedMedia],
          );
        });
      case VesperExternalPlaybackSessionEventKind.playing:
        _updateState(() {
          _setExternalPlaybackMessage(strings[ExampleHostText.externalPlaying]);
        });
      case VesperExternalPlaybackSessionEventKind.paused:
        _updateState(() {
          _setExternalPlaybackMessage(strings[ExampleHostText.externalPaused]);
        });
      case VesperExternalPlaybackSessionEventKind.stopped:
        _updateState(() {
          _setExternalPlaybackMessage(strings[ExampleHostText.externalStopped]);
        });
      case VesperExternalPlaybackSessionEventKind.suspended:
        _updateState(() {
          _setExternalPlaybackMessage(
            strings[ExampleHostText.externalSuspended],
          );
        });
      case VesperExternalPlaybackSessionEventKind.discoveryDiagnostic:
        if (event.details['severity'] != 'info') {
          _updateState(() {
            _setExternalPlaybackMessage(
              _formatExternalPlaybackDiagnostic(event, strings),
              diagnostic: true,
            );
          });
        }
      case VesperExternalPlaybackSessionEventKind.error:
        _updateState(() {
          _setExternalPlaybackMessage(
            event.message ?? strings[ExampleHostText.externalError],
            diagnostic: true,
          );
          _appendHostLog(
            severity: ExampleHostLogSeverity.error,
            title: strings[ExampleHostText.logExternalError],
            detail: event.message,
          );
        });
      case VesperExternalPlaybackSessionEventKind.unknown:
        _updateState(() {
          _appendHostLog(
            severity: ExampleHostLogSeverity.warning,
            title: strings[ExampleHostText.logExternalUnknownEvent],
            detail: event.rawKind,
          );
        });
    }
  }

  Future<void> _loadExternalRoute(VesperExternalPlaybackRoute route) async {
    if (mounted) {
      final strings = _hostStrings;
      _updateState(() {
        _setExternalPlaybackMessage(
          strings.format(ExampleHostText.externalConnecting, <String, Object?>{
            'route': route.name,
          }),
          force: true,
        );
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
    final source = _activePlaybackSource();
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
    final strings = _hostStrings;
    _updateState(() {
      if (loadResult.isSuccess && shouldAutoplay) {
        _externalPlaybackPausedLocalPlayback = true;
      }
      _setExternalPlaybackMessage(
        loadResult.isSuccess
            ? strings.format(
                ExampleHostText.externalLoadedRoute,
                <String, Object?>{'route': routeLabel},
              )
            : loadResult.message,
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
    ExampleHostStrings strings,
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
    final message =
        event.message ?? strings[ExampleHostText.externalDiagnostic];
    return '$message$suffix';
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
