import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

import 'vesper_player_controller.dart';

part 'view/viewport_binding_observer.dart';
part 'view/platform_view_constants.dart';

const Duration _scrollingViewportReportInterval = Duration(milliseconds: 160);

class VesperPlayerView extends StatefulWidget {
  const VesperPlayerView({
    super.key,
    required this.controller,
    this.overlay,
    this.visible = true,
    this.onGeometryChanged,
  });

  final VesperPlayerController controller;
  final Widget? overlay;
  final bool visible;

  /// Actual picture bounds in this view's local logical pixels. Null means
  /// unknown or detached. This callback is independent of playback snapshots.
  final ValueChanged<VesperVideoSurfaceGeometry?>? onGeometryChanged;

  @override
  State<VesperPlayerView> createState() => _VesperPlayerViewState();
}

class _VesperPlayerViewState extends State<VesperPlayerView> {
  final GlobalKey _targetKey = GlobalKey();
  VesperPlayerViewport? _lastViewport;
  VesperPlayerViewport? _lastReportedViewport;
  VesperPlayerViewport? _pendingViewport;
  ScrollPosition? _scrollPosition;
  Timer? _viewportThrottleTimer;
  bool _reportScheduled = false;
  StreamSubscription<VesperVideoSurfaceGeometry?>? _geometrySubscription;
  int _geometryGeneration = 0;
  int _geometryDelivery = 0;

  void _observeGeometry(int viewId, int expectedGeneration) {
    if (!mounted ||
        !widget.visible ||
        expectedGeneration != _geometryGeneration) {
      return;
    }
    final generation = ++_geometryGeneration;
    _geometrySubscription?.cancel();
    _geometrySubscription =
        widget.controller.videoGeometryForView(viewId).listen(
      (geometry) {
        if (mounted && generation == _geometryGeneration) {
          _deliverGeometry(geometry);
        }
      },
      onDone: () {
        if (mounted && generation == _geometryGeneration) {
          _deliverGeometry(null);
        }
      },
      onError: (Object error, StackTrace stack) {
        if (mounted && generation == _geometryGeneration) {
          _deliverGeometry(null);
          FlutterError.reportError(FlutterErrorDetails(
              exception: error,
              stack: stack,
              library: 'vesper_player',
              context: ErrorDescription('receiving video geometry')));
        }
      },
    );
  }

  void _deliverGeometry(VesperVideoSurfaceGeometry? geometry) {
    final generation = _geometryGeneration;
    final delivery = ++_geometryDelivery;
    void deliver() {
      if (mounted &&
          generation == _geometryGeneration &&
          delivery == _geometryDelivery) {
        widget.onGeometryChanged?.call(geometry);
      }
    }

    if (SchedulerBinding.instance.schedulerPhase ==
        SchedulerPhase.persistentCallbacks) {
      WidgetsBinding.instance.addPostFrameCallback((_) => deliver());
    } else {
      deliver();
    }
  }

  void _clearGeometry({bool notify = true}) {
    _geometryGeneration++;
    _geometrySubscription?.cancel();
    _geometrySubscription = null;
    if (notify) _deliverGeometry(null);
  }

  bool get _usesPlatformView =>
      !kIsWeb &&
      (defaultTargetPlatform == TargetPlatform.android ||
          defaultTargetPlatform == TargetPlatform.iOS);

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(_bindingObserver);
    _scheduleViewportReport();
  }

  @override
  void didUpdateWidget(covariant VesperPlayerView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.controller != widget.controller) {
      _clearGeometry();
      _runViewportOperation(
        oldWidget.controller.clearViewport,
        'clear old viewport',
      );
      _lastViewport = null;
      _lastReportedViewport = null;
      _pendingViewport = null;
      _viewportThrottleTimer?.cancel();
      _viewportThrottleTimer = null;
    }
    if (oldWidget.visible && !widget.visible) _clearGeometry();
    _scheduleViewportReport();
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _bindScrollable();
    _scheduleViewportReport();
  }

  @override
  void dispose() {
    _clearGeometry(notify: false);
    WidgetsBinding.instance.removeObserver(_bindingObserver);
    _viewportThrottleTimer?.cancel();
    _scrollPosition?.removeListener(_scheduleViewportReport);
    _scrollPosition = null;
    _runViewportOperation(widget.controller.clearViewport, 'clear viewport');
    super.dispose();
  }

  late final WidgetsBindingObserver _bindingObserver = _ViewportBindingObserver(
    onMetricsChanged: _scheduleViewportReport,
    onLifecycleChanged: _handleLifecycleChanged,
  );

  @override
  Widget build(BuildContext context) {
    _scheduleViewportReport();
    final baseLayer = _usesPlatformView
        ? _buildPlatformBaseLayer()
        : const ColoredBox(color: Color(0x00000000));

    return SizeChangedLayoutNotifier(
      child: KeyedSubtree(
        key: _targetKey,
        child: _buildLayeredContent(baseLayer),
      ),
    );
  }

  Widget _buildPlatformBaseLayer() {
    final generation = _geometryGeneration;
    return widget.visible
        ? switch (defaultTargetPlatform) {
            TargetPlatform.android => _buildAndroidPlatformView(),
            TargetPlatform.iOS => UiKitView(
                key: ValueKey<String>(
                  'vesper_player_ios_${widget.controller.playerId}',
                ),
                viewType: _platformViewType,
                creationParams: <String, Object?>{
                  'playerId': widget.controller.playerId,
                },
                creationParamsCodec: const StandardMessageCodec(),
                onPlatformViewCreated: (id) => _observeGeometry(id, generation),
              ),
            _ => const ColoredBox(color: Color(0x00000000)),
          }
        : const ColoredBox(color: Color(0x00000000));
  }

  Widget _buildAndroidPlatformView() {
    final generation = _geometryGeneration;
    return PlatformViewLink(
      key: ValueKey<String>(
        'vesper_player_android_${widget.controller.playerId}',
      ),
      viewType: _platformViewType,
      surfaceFactory: (
        BuildContext context,
        PlatformViewController controller,
      ) {
        return AndroidViewSurface(
          controller: controller as AndroidViewController,
          hitTestBehavior: PlatformViewHitTestBehavior.opaque,
          gestureRecognizers: const <Factory<OneSequenceGestureRecognizer>>{},
        );
      },
      onCreatePlatformView: (PlatformViewCreationParams params) {
        final controller = PlatformViewsService.initSurfaceAndroidView(
          id: params.id,
          viewType: params.viewType,
          layoutDirection: Directionality.of(context),
          creationParams: <String, Object?>{
            'playerId': widget.controller.playerId,
          },
          creationParamsCodec: const StandardMessageCodec(),
          onFocus: () => params.onFocusChanged(true),
        );
        controller.addOnPlatformViewCreatedListener(
          params.onPlatformViewCreated,
        );
        controller.addOnPlatformViewCreatedListener(
            (id) => _observeGeometry(id, generation));
        return controller;
      },
    );
  }

  Widget _buildLayeredContent(Widget baseLayer) {
    // Platform-view textures can retain a previous compositor buffer while a
    // scroll moves them. Keep that buffer inside the current player bounds.
    return ClipRect(
      child: Stack(
        fit: StackFit.expand,
        children: <Widget>[
          Positioned.fill(child: baseLayer),
          if (widget.overlay != null) Positioned.fill(child: widget.overlay!),
        ],
      ),
    );
  }

  void _scheduleViewportReport() {
    if (_reportScheduled) {
      return;
    }
    _reportScheduled = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _reportScheduled = false;
      if (!mounted) {
        return;
      }
      _reportViewport();
    });
  }

  void _bindScrollable() {
    final nextPosition = Scrollable.maybeOf(context)?.position;
    if (identical(nextPosition, _scrollPosition)) {
      return;
    }

    _scrollPosition?.removeListener(_scheduleViewportReport);
    _scrollPosition = nextPosition;
    _scrollPosition?.addListener(_scheduleViewportReport);
  }

  void _reportViewport() {
    if (!widget.visible) {
      _clearViewportIfNeeded();
      return;
    }

    final targetContext = _targetKey.currentContext;
    final renderObject = targetContext?.findRenderObject();
    if (renderObject is! RenderBox ||
        !renderObject.hasSize ||
        !renderObject.attached) {
      _clearViewportIfNeeded();
      return;
    }

    final size = renderObject.size;
    if (size.isEmpty) {
      _clearViewportIfNeeded();
      return;
    }

    final origin = renderObject.localToGlobal(Offset.zero);
    final viewport = VesperPlayerViewport(
      left: origin.dx,
      top: origin.dy,
      width: size.width,
      height: size.height,
    );

    if (_sameViewport(_lastViewport, viewport)) {
      return;
    }

    _lastViewport = viewport;
    _reportViewportChange(viewport);
  }

  void _clearViewportIfNeeded() {
    if (_lastViewport == null) {
      return;
    }
    _lastViewport = null;
    _lastReportedViewport = null;
    _pendingViewport = null;
    _viewportThrottleTimer?.cancel();
    _viewportThrottleTimer = null;
    _runViewportOperation(widget.controller.clearViewport, 'clear viewport');
  }

  void _handleLifecycleChanged(AppLifecycleState state) {
    switch (state) {
      case AppLifecycleState.resumed:
        _scheduleViewportReport();
        break;
      case AppLifecycleState.inactive:
      case AppLifecycleState.hidden:
      case AppLifecycleState.paused:
      case AppLifecycleState.detached:
        _clearViewportIfNeeded();
        break;
    }
  }

  bool _sameViewport(
    VesperPlayerViewport? previous,
    VesperPlayerViewport next,
  ) {
    if (previous == null) {
      return false;
    }
    return (previous.left - next.left).abs() < 0.5 &&
        (previous.top - next.top).abs() < 0.5 &&
        (previous.width - next.width).abs() < 0.5 &&
        (previous.height - next.height).abs() < 0.5;
  }

  void _reportViewportChange(VesperPlayerViewport viewport) {
    if (!_isScrolling()) {
      _pendingViewport = null;
      _viewportThrottleTimer?.cancel();
      _viewportThrottleTimer = null;
      _sendViewportIfNeeded(viewport);
      return;
    }

    _pendingViewport = viewport;
    if (_viewportThrottleTimer != null) {
      return;
    }

    _flushPendingViewport();
    _viewportThrottleTimer =
        Timer.periodic(_scrollingViewportReportInterval, (_) {
      if (!mounted) {
        _viewportThrottleTimer?.cancel();
        _viewportThrottleTimer = null;
        return;
      }
      if (!_isScrolling()) {
        _flushPendingViewport();
        _viewportThrottleTimer?.cancel();
        _viewportThrottleTimer = null;
        return;
      }
      _flushPendingViewport();
    });
  }

  bool _isScrolling() => _scrollPosition?.isScrollingNotifier.value ?? false;

  void _flushPendingViewport() {
    final viewport = _pendingViewport;
    if (viewport == null) {
      return;
    }
    _pendingViewport = null;
    _sendViewportIfNeeded(viewport);
  }

  void _sendViewportIfNeeded(VesperPlayerViewport viewport) {
    if (_sameViewport(_lastReportedViewport, viewport)) {
      return;
    }
    _lastReportedViewport = viewport;
    _runViewportOperation(
      () => widget.controller.updateViewport(viewport),
      'update viewport',
    );
  }

  void _runViewportOperation(
    Future<void> Function() operation,
    String context,
  ) {
    late final Future<void> future;
    try {
      future = operation();
    } catch (error, stackTrace) {
      _reportViewportError(error, stackTrace, context);
      return;
    }

    unawaited(
      future.catchError((Object error, StackTrace stackTrace) {
        _reportViewportError(error, stackTrace, context);
      }),
    );
  }

  void _reportViewportError(
    Object error,
    StackTrace stackTrace,
    String context,
  ) {
    if (error is StateError &&
        error.message == 'VesperPlayerController has already been disposed.') {
      return;
    }
    FlutterError.reportError(
      FlutterErrorDetails(
        exception: error,
        stack: stackTrace,
        library: 'vesper_player',
        context: ErrorDescription(context),
      ),
    );
  }
}
