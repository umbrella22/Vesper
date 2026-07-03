part of 'player_host_page.dart';

extension _PlayerHostHdrEvidenceActions on _PlayerHostPageState {
  Future<void> _captureHdrEvidenceBundle(
    VesperPlayerController controller,
  ) async {
    if (_isCapturingHdrEvidence) {
      return;
    }
    final preset = _selectedHdrEvidencePreset;
    final source = _sourceForHdrEvidencePreset(preset);
    if (source == null) {
      _showMessage('请先选择本地文件或远程 URL，再采集该 HDR evidence 样本。');
      return;
    }

    final sourceMetadata = await _confirmHdrEvidenceSourceMetadata(
      preset: preset,
      source: source,
    );
    if (!mounted || sourceMetadata == null) {
      return;
    }

    _updateState(() {
      _isCapturingHdrEvidence = true;
    });
    try {
      final outputRoot =
          await ExampleHdrEvidenceCaptureOutput.defaultOutputRoot();
      final now = DateTime.now();
      final captureDate = [
        now.year.toString().padLeft(4, '0'),
        now.month.toString().padLeft(2, '0'),
        now.day.toString().padLeft(2, '0'),
      ].join('-');
      final deviceId = '${Platform.operatingSystem}-example-host';
      final deviceEvidence =
          await ExampleHdrEvidenceCaptureOutput.deviceEvidence();
      final bundle = await ExampleHdrEvidenceCaptureRecorder(
        controller: controller,
        sampleId: preset.sampleId,
        deviceId: deviceId,
        platform: Platform.isIOS ? 'ios' : 'android',
        captureDate: captureDate,
        sdkCommit: 'local-debug',
        source: source,
        sourceMetadata: sourceMetadata,
        device: <String, Object?>{
          ...deviceEvidence,
          'knownCaveats': <String>[
            'Captured through example-host debug helper; fill device sheet before matrix use.',
          ],
        },
        expectedAxis: preset.expectedAxis,
        captureWindow: preset.sampleId == 'NETWORK-FAILURE-CONTROL'
            ? const Duration(seconds: 30)
            : const Duration(seconds: 3),
      ).capture();
      final directory = await ExampleHdrEvidenceBundleWriter(
        outputRoot: outputRoot,
      ).write(bundle, overwrite: true);
      if (!mounted) {
        return;
      }
      _showMessage('HDR evidence bundle 已写入：${directory.path}');
    } catch (error) {
      if (mounted) {
        _showMessage('HDR evidence capture 失败：$error');
      }
    } finally {
      if (mounted) {
        _updateState(() {
          _isCapturingHdrEvidence = false;
        });
      }
    }
  }

  VesperPlayerSource? _sourceForHdrEvidencePreset(
    ExampleHdrEvidenceSamplePreset preset,
  ) {
    if (preset.sampleId == 'NETWORK-FAILURE-CONTROL') {
      return VesperPlayerSource.remote(
        uri: exampleHdrEvidenceNetworkControlUrl,
        label: 'HDR network failure control',
        protocol: VesperPlayerSourceProtocol.progressive,
      );
    }
    final dolbyPreset = exampleDolbyAcceptancePresetById(preset.sampleId);
    if (dolbyPreset != null) {
      return dolbyPreset.source;
    }
    return _activePlaylistItemId == null
        ? null
        : _playlistSourceForItem(_activePlaylistItemId!);
  }

  Future<Map<String, Object?>?> _confirmHdrEvidenceSourceMetadata({
    required ExampleHdrEvidenceSamplePreset preset,
    required VesperPlayerSource source,
  }) async {
    final defaults = <String, Object?>{
      ...preset.sourceMetadata,
      'sourceUri': source.uri,
      'sourceKind': _sourceKindForEvidenceSource(source),
      'manifestKind': _manifestKindForEvidenceSource(source),
    };
    final controllers = <String, TextEditingController>{
      for (final key in <String>[
        'sourceUri',
        'container',
        'codec',
        'sampleMimeType',
        'width',
        'height',
        'frameRate',
        'bitDepth',
        'hdrKind',
        'colorPrimaries',
        'transferFunction',
        'yCbCrMatrix',
      ])
        key: TextEditingController(text: _metadataText(defaults[key])),
    };
    try {
      final result = await showDialog<Map<String, Object?>?>(
        context: context,
        builder: (context) {
          return AlertDialog(
            title: Text('确认 ${preset.label} evidence metadata'),
            content: SizedBox(
              width: 520,
              child: SingleChildScrollView(
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: <Widget>[
                    for (final entry in controllers.entries)
                      Padding(
                        padding: const EdgeInsets.only(bottom: 10),
                        child: TextField(
                          controller: entry.value,
                          keyboardType: _keyboardTypeForMetadata(entry.key),
                          decoration: InputDecoration(
                            labelText: entry.key,
                            isDense: true,
                          ),
                        ),
                      ),
                  ],
                ),
              ),
            ),
            actions: <Widget>[
              TextButton(
                onPressed: () => Navigator.of(context).pop(),
                child: const Text('取消'),
              ),
              FilledButton(
                onPressed: () {
                  Navigator.of(context).pop(<String, Object?>{
                    ...defaults,
                    for (final entry in controllers.entries)
                      entry.key: _metadataValue(entry.key, entry.value.text),
                  });
                },
                child: const Text('开始采集'),
              ),
            ],
          );
        },
      );
      return result;
    } finally {
      for (final controller in controllers.values) {
        controller.dispose();
      }
    }
  }
}
