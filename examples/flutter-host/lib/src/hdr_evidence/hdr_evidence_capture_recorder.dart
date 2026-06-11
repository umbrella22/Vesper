part of 'hdr_evidence_capture.dart';

final class ExampleHdrEvidenceCaptureRecorder {
  ExampleHdrEvidenceCaptureRecorder({
    required this.controller,
    required this.sampleId,
    required this.deviceId,
    required this.platform,
    required this.captureDate,
    required this.sdkCommit,
    required this.source,
    this.sourceMetadata = const <String, Object?>{},
    this.device = const <String, Object?>{},
    this.expectedAxis = 'inconclusive',
    this.captureWindow = const Duration(seconds: 10),
    this.platformLog = '',
  });

  final VesperPlayerController controller;
  final String sampleId;
  final String deviceId;
  final String platform;
  final String captureDate;
  final String sdkCommit;
  final VesperPlayerSource source;
  final Map<String, Object?> sourceMetadata;
  final Map<String, Object?> device;
  final String expectedAxis;
  final Duration captureWindow;
  final String platformLog;

  Future<ExampleHdrEvidenceBundle> capture() async {
    VesperCapabilityWarning? capabilityWarning;
    VesperPlayerError? playbackError;
    final subscription = controller.events.listen((event) {
      switch (event) {
        case VesperPlayerWarningEvent():
          final warning = event.warning.capability;
          if (warning != null) {
            capabilityWarning = warning;
          }
        case VesperPlayerErrorEvent():
          playbackError = event.error;
        case VesperPlayerSnapshotEvent():
        case VesperPlayerDisposedEvent():
          break;
      }
    });

    late final VesperPlaybackCapabilityProbeResult flutterProbe;
    try {
      flutterProbe = await VesperPlayerController.probePlaybackCapability(
        VesperPlaybackCapabilityProbeRequest(
          source: source,
          codec: sourceMetadata['codec'] as String?,
          width: _intValue(sourceMetadata['width']),
          height: _intValue(sourceMetadata['height']),
          frameRate: _doubleValue(sourceMetadata['frameRate']),
        ),
      );
      await controller.selectSource(source);
      await controller.play();
      await Future<void>.delayed(captureWindow);
    } catch (error) {
      if (error is VesperPlayerError) {
        playbackError = error;
      } else {
        rethrow;
      }
    } finally {
      await subscription.cancel();
    }

    final effectiveSourceMetadata = <String, Object?>{
      'sourceUri': source.uri,
      'sourceKind': _sourceKindFor(source),
      'manifestKind': _manifestKindFor(source),
      ...sourceMetadata,
    };

    return ExampleHdrEvidenceBundle(
      sampleId: sampleId,
      deviceId: deviceId,
      platform: platform,
      captureDate: captureDate,
      sdkCommit: sdkCommit,
      sourceMetadata: effectiveSourceMetadata,
      device: device,
      flutterProbe: flutterProbe,
      playbackOutcome: _playbackOutcome(playbackError, capabilityWarning),
      runtimeWarning: capabilityWarning,
      runtimeError: playbackError,
      expectedAxis: expectedAxis,
      matchesHostProbe: null,
      matchesHostEvidence: null,
      platformLog: platformLog,
    );
  }
}

final class ExampleHdrEvidenceBundleWriter {
  const ExampleHdrEvidenceBundleWriter({required this.outputRoot});

  final Directory outputRoot;

  Future<Directory> write(
    ExampleHdrEvidenceBundle bundle, {
    bool overwrite = false,
  }) async {
    final directory = Directory(
      '${outputRoot.path}/${bundle.captureDate}/${bundle.deviceId}/${bundle.sampleId}',
    );
    if (directory.existsSync()) {
      if (!overwrite) {
        throw StateError('Evidence bundle already exists: ${directory.path}');
      }
    } else {
      await directory.create(recursive: true);
    }

    await _writeJson(
      File('${directory.path}/device.json'),
      bundle.deviceJson(),
    );
    await _writeJson(
      File('${directory.path}/source-metadata.json'),
      bundle.sourceMetadataJson(),
    );
    await _writeJson(
      File('${directory.path}/probe-host.json'),
      bundle.probeHostJson(),
    );
    await _writeJson(
      File('${directory.path}/probe-flutter.json'),
      bundle.probeFlutterJson(),
    );
    await _writeJson(
      File('${directory.path}/runtime-warning.json'),
      bundle.runtimeWarningJson(),
    );
    await _writeJson(
      File('${directory.path}/runtime-error.json'),
      bundle.runtimeErrorJson(),
    );
    await _writeJson(
      File('${directory.path}/typed-evidence.json'),
      bundle.typedEvidenceJson(),
    );
    await File(
      '${directory.path}/platform-log.txt',
    ).writeAsString(bundle.platformLogText());
    await File(
      '${directory.path}/notes.md',
    ).writeAsString(bundle.notesMarkdown(bundlePath: directory.path));
    return directory;
  }

  Future<void> _writeJson(File file, Map<String, Object?> data) {
    const encoder = JsonEncoder.withIndent('  ');
    return file.writeAsString('${encoder.convert(_jsonValue(data))}\n');
  }
}
