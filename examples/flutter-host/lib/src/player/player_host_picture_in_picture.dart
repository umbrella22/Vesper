part of 'player_host_page.dart';

extension _PlayerHostPictureInPictureActions on _PlayerHostPageState {
  static const String _pictureInPictureUnavailableMessage = '当前播放无法启用小窗';

  Future<void> _bindPictureInPicture(VesperPlayerController controller) async {
    await (_pictureInPictureSubscription?.cancel() ?? Future<void>.value());
    _pictureInPictureSubscription = controller.pictureInPictureEvents.listen(
      _handlePictureInPictureEvent,
    );
    await _refreshPictureInPictureAvailability(controller);
  }

  Future<void> _refreshPictureInPictureAvailability(
    VesperPlayerController controller,
  ) async {
    try {
      final availability = await controller.isPictureInPictureAvailable();
      if (!mounted) {
        return;
      }
      _updateState(() {
        _pictureInPictureAvailability = availability;
        _pictureInPictureStatus = availability.isActive
            ? VesperPictureInPictureStatus.active
            : _pictureInPictureStatus;
      });
    } catch (_) {
      if (mounted) {
        _updateState(() {
          _pictureInPictureAvailability =
              const VesperPictureInPictureAvailability(
                isAvailable: false,
                error: VesperPictureInPictureError(
                  code: VesperPictureInPictureErrorCode
                      .pictureInPictureUnavailableForCurrentRoute,
                ),
              );
        });
      }
    }
  }

  Future<void> _requestPictureInPicture(
    VesperPlayerController controller,
  ) async {
    if (!_pictureInPictureEnabled) {
      _showMessage(_pictureInPictureUnavailableMessage);
      return;
    }
    try {
      final availability = await controller.isPictureInPictureAvailable();
      if (!mounted) {
        return;
      }
      _updateState(() {
        _pictureInPictureAvailability = availability;
      });
      if (!availability.isAvailable) {
        _showMessage(
          availability.error?.userMessage ??
              _pictureInPictureUnavailableMessage,
        );
        return;
      }
      _setPictureInPicturePresentation(true);
      await WidgetsBinding.instance.endOfFrame;
      if (!mounted) {
        return;
      }
      await controller.requestPictureInPicture(
        configuration: const VesperPictureInPictureConfiguration(
          enabled: true,
          autoEnter: true,
          preferredAspectRatio: 16 / 9,
        ),
      );
    } on PlatformException catch (error) {
      _setPictureInPicturePresentation(false);
      if (mounted) {
        _showMessage(_pictureInPictureMessageFromDetails(error.details));
      }
    } catch (_) {
      _setPictureInPicturePresentation(false);
      if (mounted) {
        _showMessage(_pictureInPictureUnavailableMessage);
      }
    }
  }

  Future<void> _setPictureInPictureEnabled(
    VesperPlayerController controller,
    bool enabled,
  ) async {
    _updateState(() {
      _pictureInPictureEnabled = enabled;
      if (!enabled) {
        _pictureInPicturePresentation = false;
      }
    });
    try {
      await controller.setPictureInPictureConfiguration(
        VesperPictureInPictureConfiguration(
          enabled: enabled,
          autoEnter: enabled,
          preferredAspectRatio: 16 / 9,
        ),
      );
      if (!enabled &&
          _pictureInPictureStatus == VesperPictureInPictureStatus.active) {
        await controller.exitPictureInPicture();
      }
      await _refreshPictureInPictureAvailability(controller);
    } catch (_) {
      if (mounted) {
        _showMessage(_pictureInPictureUnavailableMessage);
      }
    }
  }

  void _handlePictureInPictureEvent(VesperPlayerPictureInPictureEvent event) {
    if (!mounted) {
      return;
    }
    _updateState(() {
      _pictureInPictureStatus = event.state;
      _pictureInPicturePresentation =
          _isPictureInPicturePresentationState(event.state);
      _pictureInPictureAvailability = VesperPictureInPictureAvailability(
        isAvailable: event.error == null,
        isActive: event.isActive,
        canAutoEnter: event.canAutoEnter ?? false,
        source: event.source,
        error: event.error,
        diagnostics: event.diagnostics,
      );
    });
    if (event.state == VesperPictureInPictureStatus.failed) {
      _showMessage(
        event.error?.userMessage ?? _pictureInPictureUnavailableMessage,
      );
    }
  }

  String _pictureInPictureMessageFromDetails(Object? details) {
    if (details is Map) {
      final userMessage = details['userMessage'];
      if (userMessage is String && userMessage.isNotEmpty) {
        return userMessage;
      }
    }
    return _pictureInPictureUnavailableMessage;
  }

  Future<Object?> _handlePictureInPictureHostCall(MethodCall call) async {
    if (call.method == 'onUserLeaveHint') {
      _handlePictureInPictureUserLeaveHint();
    }
    return null;
  }

  void _handlePictureInPictureUserLeaveHint() {
    if (!Platform.isAndroid || !_pictureInPictureEnabled) {
      return;
    }
    _setPictureInPicturePresentation(true);
    Future<void>.delayed(const Duration(milliseconds: 900), () {
      if (!mounted) {
        return;
      }
      if (_pictureInPictureStatus == VesperPictureInPictureStatus.active ||
          _pictureInPictureStatus == VesperPictureInPictureStatus.exiting) {
        return;
      }
      _updateState(() {
        _pictureInPictureStatus = VesperPictureInPictureStatus.inactive;
        _pictureInPicturePresentation = false;
        _pictureInPictureAvailability =
            _pictureInPictureAvailability?.inactive();
      });
    });
  }

  void _setPictureInPicturePresentation(bool enabled) {
    if (!mounted || _pictureInPicturePresentation == enabled) {
      return;
    }
    _updateState(() {
      _pictureInPicturePresentation = enabled;
      if (enabled) {
        _sheetOpen = false;
      }
    });
  }

  bool _isPictureInPicturePresentationState(
    VesperPictureInPictureStatus state,
  ) {
    return state == VesperPictureInPictureStatus.entering ||
        state == VesperPictureInPictureStatus.active ||
        state == VesperPictureInPictureStatus.exiting;
  }
}

extension on VesperPictureInPictureAvailability {
  VesperPictureInPictureAvailability inactive() {
    return VesperPictureInPictureAvailability(
      isAvailable: isAvailable,
      isActive: false,
      canAutoEnter: false,
      source: source,
      error: error,
      diagnostics: diagnostics,
    );
  }
}
