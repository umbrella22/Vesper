import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

import 'vesper_player_controller.dart';

part 'view/viewport_binding_observer.dart';
part 'view/platform_view_constants.dart';

class VesperPlayerView extends StatefulWidget {
  const VesperPlayerView({
    super.key,
    required this.controller,
    this.overlay,
    this.visible = true,
  });

  final VesperPlayerController controller;
  final Widget? overlay;
  final bool visible;

  @override
  State<VesperPlayerView> createState() => _VesperPlayerViewState();
}

class _VesperPlayerViewState extends State<VesperPlayerView> {
  final GlobalKey _targetKey = GlobalKey();
  VesperPlayerViewport? _lastViewport;
  ScrollPosition? _scrollPosition;
  bool _reportScheduled = false;

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
      _runViewportOperation(
        oldWidget.controller.clearViewport,
        'clear old viewport',
      );
      _lastViewport = null;
    }
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
    WidgetsBinding.instance.removeObserver(_bindingObserver);
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
              ),
            _ => const ColoredBox(color: Color(0x00000000)),
          }
        : const ColoredBox(color: Color(0x00000000));
  }

  Widget _buildAndroidPlatformView() {
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
        return controller;
      },
    );
  }

  Widget _buildLayeredContent(Widget baseLayer) {
    return Stack(
      fit: StackFit.expand,
      children: <Widget>[
        Positioned.fill(child: baseLayer),
        if (widget.overlay != null) Positioned.fill(child: widget.overlay!),
      ],
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
    _runViewportOperation(
      () => widget.controller.updateViewport(viewport),
      'update viewport',
    );
  }

  void _clearViewportIfNeeded() {
    if (_lastViewport == null) {
      return;
    }
    _lastViewport = null;
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
