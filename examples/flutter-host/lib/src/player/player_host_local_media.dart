part of 'player_host_page.dart';

extension _PlayerHostLocalMediaActions on _PlayerHostPageState {
  Future<void> _pickLocalVideo(VesperPlayerController controller) async {
    try {
      final pickedVideo = await ExampleLocalMediaPicker.pickVideo();
      if (!mounted || pickedVideo == null) {
        return;
      }
      final source = VesperPlayerSource.local(
        uri: pickedVideo.uri,
        label: pickedVideo.label,
      );
      await _activatePlaylistSource(
        controller,
        itemId: flutterLocalPlaylistItemId,
        source: source,
        localSource: source,
      );
      return;
    } on MissingPluginException {
      // Fall back to manual input when the host picker is not wired, which keeps debugging simple.
    } on PlatformException catch (error) {
      if (!mounted || error.code == 'cancelled') {
        return;
      }
    }

    await _promptLocalVideoFallback(controller);
  }

  Future<void> _promptLocalVideoFallback(
    VesperPlayerController controller,
  ) async {
    final localUri = await showDialog<String>(
      context: context,
      builder: (context) {
        final textController = TextEditingController();
        return AlertDialog(
          title: const Text('选择视频'),
          content: TextField(
            controller: textController,
            autofocus: true,
            decoration: const InputDecoration(
              labelText: '本地路径或 URI',
              hintText: 'file:///sdcard/Movies/demo.mp4',
            ),
          ),
          actions: <Widget>[
            TextButton(
              onPressed: () => Navigator.of(context).pop(),
              child: const Text('取消'),
            ),
            FilledButton(
              onPressed: () => Navigator.of(context).pop(textController.text),
              child: const Text('打开'),
            ),
          ],
        );
      },
    );

    if (!mounted || localUri == null || localUri.trim().isEmpty) {
      return;
    }

    final normalizedUri = normalizeLocalUri(localUri);
    final source = VesperPlayerSource.local(
      uri: normalizedUri,
      label: localSourceLabel(normalizedUri),
    );
    await _activatePlaylistSource(
      controller,
      itemId: flutterLocalPlaylistItemId,
      source: source,
      localSource: source,
    );
  }

  String _sourceKindForEvidenceSource(VesperPlayerSource source) {
    return switch (source.protocol) {
      VesperPlayerSourceProtocol.file ||
      VesperPlayerSourceProtocol.content => 'file',
      VesperPlayerSourceProtocol.hls => 'hls',
      VesperPlayerSourceProtocol.dash => 'progressive',
      VesperPlayerSourceProtocol.progressive ||
      VesperPlayerSourceProtocol.rtmp ||
      VesperPlayerSourceProtocol.rtsp ||
      VesperPlayerSourceProtocol.flv => 'progressive',
      VesperPlayerSourceProtocol.unknown =>
        source.kind == VesperPlayerSourceKind.local ? 'file' : 'progressive',
    };
  }

  String _manifestKindForEvidenceSource(VesperPlayerSource source) {
    return switch (source.protocol) {
      VesperPlayerSourceProtocol.hls => 'hls',
      VesperPlayerSourceProtocol.dash => 'dash',
      _ => 'none',
    };
  }

  String _metadataText(Object? value) {
    if (value == null) {
      return '';
    }
    return value.toString();
  }

  Object? _metadataValue(String key, String rawValue) {
    final value = rawValue.trim();
    if (value.isEmpty) {
      return null;
    }
    switch (key) {
      case 'width':
      case 'height':
      case 'bitDepth':
        return int.tryParse(value) ?? value;
      case 'frameRate':
        return double.tryParse(value) ?? value;
      default:
        return value;
    }
  }

  TextInputType _keyboardTypeForMetadata(String key) {
    switch (key) {
      case 'sourceUri':
        return TextInputType.url;
      case 'width':
      case 'height':
      case 'frameRate':
      case 'bitDepth':
        return const TextInputType.numberWithOptions(decimal: true);
      default:
        return TextInputType.text;
    }
  }

  Future<void> _openToolSheet(
    VesperPlayerController controller,
    ExamplePlayerSheet initialSheet,
  ) async {
    if (!mounted) {
      return;
    }
    if (_pictureInPicturePresentation) {
      return;
    }

    _updateState(() {
      _sheetOpen = true;
    });

    try {
      await showExampleSelectionSheet(
        context,
        controller: controller,
        initialSheet: initialSheet,
      );
    } finally {
      if (mounted) {
        _updateState(() {
          _sheetOpen = false;
        });
      }
    }
  }

  Future<void> _toggleFullscreen(Orientation orientation) async {
    if (orientation == Orientation.portrait) {
      await SystemChrome.setPreferredOrientations(const <DeviceOrientation>[
        DeviceOrientation.landscapeLeft,
        DeviceOrientation.landscapeRight,
      ]);
      await SystemChrome.setEnabledSystemUIMode(SystemUiMode.immersiveSticky);
      return;
    }

    await _restoreSystemPresentation();
  }

  Future<void> _restoreSystemPresentation() async {
    // Explicitly leave fullscreen in portrait on mobile, even when Android
    // auto-rotate is locked. Desktop hosts do not have an app orientation
    // contract to restore.
    if (Platform.isAndroid || Platform.isIOS) {
      await SystemChrome.setPreferredOrientations(const <DeviceOrientation>[
        DeviceOrientation.portraitUp,
      ]);
    }
    await SystemChrome.setEnabledSystemUIMode(SystemUiMode.edgeToEdge);
  }
}
