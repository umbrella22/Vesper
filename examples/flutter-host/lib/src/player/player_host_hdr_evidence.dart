part of 'player_host_page.dart';

extension _PlayerHostHdrEvidenceActions on _PlayerHostPageState {
  Future<void> _captureHdrEvidenceBundle(
    VesperPlayerController controller,
  ) async {
    if (_isCapturingHdrEvidence) {
      return;
    }
    final strings = _hostStrings;
    final preset = _selectedHdrEvidencePreset;
    final source = _sourceForHdrEvidencePreset(preset);
    if (source == null) {
      _showMessage(strings[ExampleHostText.hdrEvidenceNeedsSource]);
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
      _showMessage(
        strings.format(ExampleHostText.hdrEvidenceWritten, <String, Object?>{
          'path': directory.path,
        }),
      );
      _updateState(() {
        _appendHostLog(
          title: strings[ExampleHostText.logHdrEvidenceResult],
          detail: directory.path,
        );
      });
    } catch (error) {
      if (mounted) {
        _showMessage(
          strings.format(ExampleHostText.hdrEvidenceFailed, <String, Object?>{
            'error': error,
          }),
        );
        _updateState(() {
          _appendHostLog(
            severity: ExampleHostLogSeverity.error,
            title: strings[ExampleHostText.logHdrEvidenceResult],
            detail: '$error',
          );
        });
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
    return _activePlaybackSource();
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
          final strings = ExampleHostStrings.of(context);
          return AlertDialog(
            title: Text(
              strings.format(
                ExampleHostText.hdrEvidenceConfirm,
                <String, Object?>{'label': preset.label},
              ),
            ),
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
                child: Text(strings[ExampleHostText.cancel]),
              ),
              FilledButton(
                onPressed: () {
                  Navigator.of(context).pop(<String, Object?>{
                    ...defaults,
                    for (final entry in controllers.entries)
                      entry.key: _metadataValue(entry.key, entry.value.text),
                  });
                },
                child: Text(strings[ExampleHostText.hdrEvidenceStart]),
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
