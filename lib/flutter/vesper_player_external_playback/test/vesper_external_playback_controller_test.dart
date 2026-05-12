import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player/vesper_player.dart';
import 'package:vesper_player_external_playback/vesper_player_external_playback.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  const channel = MethodChannel(
    'io.github.ikaros.vesper_player_external_playback_test',
  );
  final calls = <MethodCall>[];

  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, null);
    calls.clear();
  });

  test('media item and route DTOs round trip', () {
    final source = VesperPlayerSource.remote(
      uri: 'https://example.com/video.mp4',
      label: 'MP4',
      headers: const <String, String>{'Referer': 'https://example.com'},
    );
    const metadata = VesperSystemPlaybackMetadata(
      title: 'Episode',
      artworkUri: 'https://example.com/art.jpg',
      durationMs: 60000,
    );
    final item = VesperExternalPlaybackMediaItem(
      sources: <VesperPlayerSource>[source],
      metadata: metadata,
      proxyPolicy: VesperExternalProxyPolicy.always,
    );
    const route = VesperExternalPlaybackRoute(
      routeId: 'uuid:tv',
      name: 'Living Room TV',
      kind: VesperExternalPlaybackRouteKind.dlna,
      manufacturer: 'DemoCorp',
      modelName: 'Model X',
      active: true,
    );

    final decodedItem = VesperExternalPlaybackMediaItem.fromMap(item.toMap());
    final decodedRoute = VesperExternalPlaybackRoute.fromMap(route.toMap());

    expect(decodedItem.sources.single.headers, source.headers);
    expect(decodedItem.proxyPolicy, VesperExternalProxyPolicy.always);
    expect(decodedRoute.kind, VesperExternalPlaybackRouteKind.dlna);
    expect(decodedRoute.manufacturer, 'DemoCorp');
    expect(decodedRoute.active, isTrue);
  });

  test('session event DTO decodes cast metadata and position', () {
    final event = VesperExternalPlaybackSessionEvent.fromMap(
      <Object?, Object?>{
        'kind': 'routeDisconnected',
        'routeId': VesperExternalPlaybackController.castRouteId,
        'routeName': 'Living Room TV',
        'message': 'Disconnected',
        'positionMs': 1234,
      },
    );

    expect(
      event.kind,
      VesperExternalPlaybackSessionEventKind.routeDisconnected,
    );
    expect(event.routeId, VesperExternalPlaybackController.castRouteId);
    expect(event.routeName, 'Living Room TV');
    expect(event.message, 'Disconnected');
    expect(event.positionMs, 1234);
  });

  test('session event DTO decodes discovery diagnostics', () {
    final event = VesperExternalPlaybackSessionEvent.fromMap(
      <Object?, Object?>{
        'kind': 'discoveryDiagnostic',
        'message': 'Timed out while fetching DLNA device description.',
        'code': 'description_timeout',
        'details': <Object?, Object?>{
          'severity': 'warning',
          'location': 'http://192.168.1.10:8000/desc.xml',
        },
      },
    );

    expect(
      event.kind,
      VesperExternalPlaybackSessionEventKind.discoveryDiagnostic,
    );
    expect(event.code, 'description_timeout');
    expect(event.details['severity'], 'warning');
    expect(event.details['location'], 'http://192.168.1.10:8000/desc.xml');
  });

  test('load serializes media item and decodes relay result', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      return <String, Object?>{
        'status': 'success',
        'routeId': 'cast:active',
        'relayEnabled': true,
      };
    });
    final controller = VesperExternalPlaybackController(methodChannel: channel);
    final item = VesperExternalPlaybackMediaItem(
      sources: <VesperPlayerSource>[
        VesperPlayerSource.hls(
          uri: 'https://example.com/video.m3u8',
          label: 'HLS',
          headers: const <String, String>{'Cookie': 'secret'},
        ),
      ],
      metadata: const VesperSystemPlaybackMetadata(title: 'Episode'),
    );

    final result = await controller.load(
      item,
      startPositionMs: 12000,
      autoplay: false,
    );

    expect(result.status, VesperExternalPlaybackResultStatus.success);
    expect(result.routeId, 'cast:active');
    expect(result.relayEnabled, isTrue);
    expect(calls.single.method, 'load');
    expect(
      Map<Object?, Object?>.from(calls.single.arguments as Map),
      <Object?, Object?>{
        'item': item.toMap(),
        'startPositionMs': 12000,
        'autoplay': false,
      },
    );
  });

  test('connect decodes unsupported result', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      return <String, Object?>{
        'status': 'unsupported',
        'message': 'DASH is not supported for DLNA in this MVP.',
      };
    });
    final controller = VesperExternalPlaybackController(methodChannel: channel);

    final result = await controller.connect('uuid:tv');

    expect(result.status, VesperExternalPlaybackResultStatus.unsupported);
    expect(result.message, contains('DASH'));
    expect(calls.single.method, 'connect');
  });

  testWidgets('route button wrapper preserves requested icon hit area',
      (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.iOS;
    try {
      await tester.pumpWidget(const MaterialApp(
        home: VesperExternalRouteButton(
          size: 42,
          brightness: Brightness.dark,
        ),
      ));

      final iconButton = tester.widget<VesperExternalRouteIconButton>(
        find.byType(VesperExternalRouteIconButton),
      );

      expect(iconButton.size, 42);
      expect(iconButton.brightness, Brightness.dark);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });
}
