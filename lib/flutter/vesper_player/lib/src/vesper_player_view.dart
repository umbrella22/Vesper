import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

import 'vesper_player_controller.dart';

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
    if (_usesPlatformView) {
      return;
    }
    WidgetsBinding.instance.addObserver(_bindingObserver);
    _scheduleViewportReport();
  }

  @override
  void didUpdateWidget(covariant VesperPlayerView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (_usesPlatformView) {
      return;
    }
    if (oldWidget.controller != widget.controller) {
      unawaited(oldWidget.controller.clearViewport());
      _lastViewport = null;
    }
    _scheduleViewportReport();
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (_usesPlatformView) {
      return;
    }
    _bindScrollable();
    _scheduleViewportReport();
  }

  @override
  void dispose() {
    if (_usesPlatformView) {
      super.dispose();
      return;
    }
    WidgetsBinding.instance.removeObserver(_bindingObserver);
    _scrollPosition?.removeListener(_scheduleViewportReport);
    _scrollPosition = null;
    unawaited(widget.controller.clearViewport());
    super.dispose();
  }

  late final WidgetsBindingObserver _bindingObserver = _ViewportBindingObserver(
    onMetricsChanged: _scheduleViewportReport,
  );

  @override
  Widget build(BuildContext context) {
    if (_usesPlatformView) {
      return _buildPlatformView();
    }

    _scheduleViewportReport();
    return SizeChangedLayoutNotifier(
      child: KeyedSubtree(
        key: _targetKey,
        child: _buildLayeredContent(const ColoredBox(color: Color(0x00000000))),
      ),
    );
  }

  Widget _buildPlatformView() {
    final Widget baseLayer = widget.visible
        ? switch (defaultTargetPlatform) {
            TargetPlatform.android => AndroidView(
              key: ValueKey<String>(
                'vesper_player_android_${widget.controller.playerId}',
              ),
              viewType: _platformViewType,
              creationParams: <String, Object?>{
                'playerId': widget.controller.playerId,
              },
              creationParamsCodec: const StandardMessageCodec(),
            ),
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

    return _buildLayeredContent(baseLayer);
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
    unawaited(widget.controller.updateViewport(viewport));
  }

  void _clearViewportIfNeeded() {
    if (_lastViewport == null) {
      return;
    }
    _lastViewport = null;
    unawaited(widget.controller.clearViewport());
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
}

final class _ViewportBindingObserver with WidgetsBindingObserver {
  _ViewportBindingObserver({required this.onMetricsChanged});

  final VoidCallback onMetricsChanged;

  @override
  void didChangeMetrics() {
    onMetricsChanged();
  }
}

const String _platformViewType = 'io.github.ikaros.vesper_player/platform_view';
