import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player_platform_interface/method_channel_platform_base.dart';
import 'package:vesper_player_platform_interface/vesper_player_platform_interface.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  test(
      'method channel carries independent probe and policy; older hosts return unknown',
      () async {
    const channel = MethodChannel('audio-diagnostics-test');
    final platform = _TestPlatform();
    final calls = <MethodCall>[];
    final messenger =
        TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      if (call.method == 'probeAudioDecoderCapability') {
        return {
          'request': call.arguments,
          'status': 'unknown',
          'reason': 'formatQueryFailed',
          'evidence': 'mediaCodecList'
        };
      }
      return null;
    });
    addTearDown(() => messenger.setMockMethodCallHandler(channel, null));
    const request = VesperAudioDecoderCapabilityRequest(
        codec: 'mp4a.40.5', channels: 2, sampleRate: 48000);
    final probe = await platform.probeAudioDecoderCapability(request);
    expect(probe.reason, 'formatQueryFailed');
    expect(calls.single.arguments, request.toMap());
    await platform.setPlaybackStallPolicy(
        'p', const VesperPlaybackStallPolicy(enabled: false));
    expect(calls.last.arguments, {
      'playerId': 'p',
      'policy': {
        'enabled': false,
        'positionThresholdMs': 5000,
        'bufferingThresholdMs': 15000
      }
    });
    messenger.setMockMethodCallHandler(
        channel, (call) async => throw MissingPluginException());
    expect((await platform.probeAudioDecoderCapability(request)).reason,
        'platformProbeNotImplemented');
  });

  test(
      'stall warnings retain unknown reason and captured audio across forwarding',
      () {
    final warning = VesperRuntimeWarning.fromMap({
      'domain': 'playback',
      'playback': {
        'playbackEpoch': 4,
        'kind': 'futureStall',
        'stalledForMs': 8000,
        'elapsedSinceLoadStartMs': 9500,
        'mediaPositionMs': 60000,
        'audio': {'codec': 'ec-3', 'evidence': 'runtimeFormat'},
      },
    });
    expect(warning.domain, VesperRuntimeWarningDomain.playback);
    expect(warning.audio, isNull);
    final decoded = VesperRuntimeWarning.fromMap(warning.toMap()).playback!;
    expect(decoded.kind, VesperPlaybackStallKind.unknown);
    expect(decoded.kindRawValue, 'futureStall');
    expect(decoded.audio.codec, 'ec-3');
    final snapshot = VesperPlaybackDiagnosticsSnapshot.fromMap({
      'playbackEpoch': 4,
      'lastStall': decoded.toMap(),
    });
    expect(snapshot.lastStall!.mediaPositionMs, 60000);
    expect(snapshot.toMap()['lastStall'], decoded.toMap());
  });

  test(
      'audio decoder probes preserve unknown status, inputs and candidate failures',
      () {
    final result = VesperAudioDecoderCapabilityResult.fromMap({
      'request': {
        'codec': 'future.audio.2',
        'channels': 6,
        'sampleRate': 48000
      },
      'status': 'futureSupport',
      'evidence': 'futureQuery',
      'reason': 'partialQuery',
      'candidates': [
        {'name': 'decoder', 'status': 'unknown', 'reason': 'formatQueryFailed'}
      ],
    });
    expect(result.status, VesperAudioDecoderSupport.unknown);
    final forwarded =
        VesperAudioDecoderCapabilityResult.fromMap(result.toMap());
    expect(forwarded.statusRawValue, 'futureSupport');
    expect(forwarded.request.codec, 'future.audio.2');
    expect(forwarded.request.channels, 6);
    expect(forwarded.candidates.single.reason, 'formatQueryFailed');
  });

  test(
      'invalid thresholds and format constraints fail before crossing the channel',
      () {
    expect(
        () => const VesperPlaybackStallPolicy(positionThresholdMs: 0).toMap(),
        throwsArgumentError);
    expect(
        () => const VesperAudioDecoderCapabilityRequest(channels: -1).toMap(),
        throwsArgumentError);
    expect(
        () => const VesperAudioDecoderCapabilityRequest(sampleRate: 0).toMap(),
        throwsArgumentError);
  });
}

class _TestPlatform extends VesperMethodChannelPlatformBase {
  _TestPlatform()
      : super(
            methodChannel: const MethodChannel('audio-diagnostics-test'),
            eventChannel: const EventChannel('events-test'),
            downloadEventChannel: const EventChannel('downloads-test'),
            sequenceEventChannel: const EventChannel('sequence-test'));
}
