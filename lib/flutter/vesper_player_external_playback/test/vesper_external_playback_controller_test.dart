import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:material_ui/material_ui.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:vesper_player/vesper_player.dart';
import 'package:vesper_player_external_playback/vesper_player_external_playback.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  const channel = MethodChannel(
    'io.github.ikaros.vesper_player_external_playback_test',
  );
  const routesChannel = EventChannel(
    'io.github.ikaros.vesper_player_external_playback/routes',
  );
  const eventsChannel = EventChannel(
    'io.github.ikaros.vesper_player_external_playback/events',
  );
  final calls = <MethodCall>[];

  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, null);
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockStreamHandler(routesChannel, null);
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockStreamHandler(eventsChannel, null);
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
      formatAdaptation: const VesperExternalFormatAdaptationConfig.dlnaRemux(
        preferredFallback: VesperExternalFallbackFormat.hls,
        allowRemoteDashMediaReferences: true,
        remoteDashMediaRequestHeaders: <String>{'User-Agent', 'Referer'},
        debugDiagnostics: true,
      ),
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
    expect(decodedItem.formatAdaptation.enabled, isTrue);
    expect(
      decodedItem.formatAdaptation.preferredFallback,
      VesperExternalFallbackFormat.hls,
    );
    expect(decodedItem.formatAdaptation.allowRemoteDashMediaReferences, isTrue);
    expect(
      decodedItem.formatAdaptation.remoteDashMediaRequestHeaders,
      <String>{'User-Agent', 'Referer'},
    );
    expect(decodedItem.formatAdaptation.debugDiagnostics, isTrue);
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

  test('default event channels are shared across controller instances',
      () async {
    var routeListenCount = 0;
    var routeCancelCount = 0;
    var eventListenCount = 0;
    var eventCancelCount = 0;

    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockStreamHandler(
      routesChannel,
      MockStreamHandler.inline(
        onListen: (_, events) {
          routeListenCount += 1;
          events.success(<Object?>[
            <String, Object?>{
              'routeId': 'uuid:tv',
              'name': 'Living Room TV',
              'kind': 'dlna',
            },
          ]);
        },
        onCancel: (_) {
          routeCancelCount += 1;
        },
      ),
    );
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockStreamHandler(
      eventsChannel,
      MockStreamHandler.inline(
        onListen: (_, events) {
          eventListenCount += 1;
          events.success(<String, Object?>{
            'kind': 'loaded',
            'routeId': 'uuid:tv',
          });
        },
        onCancel: (_) {
          eventCancelCount += 1;
        },
      ),
    );

    final first = VesperExternalPlaybackController();
    final second = VesperExternalPlaybackController();
    final firstRoutes = <List<VesperExternalPlaybackRoute>>[];
    final secondRoutes = <List<VesperExternalPlaybackRoute>>[];
    final firstEvents = <VesperExternalPlaybackSessionEvent>[];
    final secondEvents = <VesperExternalPlaybackSessionEvent>[];

    final subscriptions = <StreamSubscription<Object?>>[
      first.routes.listen(firstRoutes.add),
      second.routes.listen(secondRoutes.add),
      first.events.listen(firstEvents.add),
      second.events.listen(secondEvents.add),
    ];
    await Future<void>.delayed(Duration.zero);

    expect(routeListenCount, 1);
    expect(eventListenCount, 1);
    expect(firstRoutes.single.single.routeId, 'uuid:tv');
    expect(secondRoutes.single.single.routeId, 'uuid:tv');
    expect(
        firstEvents.single.kind, VesperExternalPlaybackSessionEventKind.loaded);
    expect(secondEvents.single.kind,
        VesperExternalPlaybackSessionEventKind.loaded);

    await subscriptions[0].cancel();
    await subscriptions[2].cancel();
    await Future<void>.delayed(Duration.zero);

    expect(routeCancelCount, 0);
    expect(eventCancelCount, 0);

    await subscriptions[1].cancel();
    await subscriptions[3].cancel();
    await Future<void>.delayed(Duration.zero);

    expect(routeCancelCount, 1);
    expect(eventCancelCount, 1);

    final third = VesperExternalPlaybackController();
    final thirdRoutes = <List<VesperExternalPlaybackRoute>>[];
    final thirdEvents = <VesperExternalPlaybackSessionEvent>[];
    final resubscriptions = <StreamSubscription<Object?>>[
      third.routes.listen(thirdRoutes.add),
      third.events.listen(thirdEvents.add),
    ];
    await Future<void>.delayed(Duration.zero);

    expect(routeListenCount, 2);
    expect(eventListenCount, 2);
    expect(thirdRoutes.single.single.routeId, 'uuid:tv');
    expect(
        thirdEvents.single.kind, VesperExternalPlaybackSessionEventKind.loaded);

    await resubscriptions[0].cancel();
    await resubscriptions[1].cancel();
    await Future<void>.delayed(Duration.zero);

    expect(routeCancelCount, 2);
    expect(eventCancelCount, 2);
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

  test('dispose clears cached routes and rejects later operations', () async {
    const customRoutesChannel = EventChannel(
      'io.github.ikaros.vesper_player_external_playback_test/routes',
    );
    var routeListenCount = 0;
    var routeCancelCount = 0;

    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockStreamHandler(
      customRoutesChannel,
      MockStreamHandler.inline(
        onListen: (_, events) {
          routeListenCount += 1;
          events.success(<Object?>[
            <String, Object?>{
              'routeId': 'uuid:tv',
              'name': 'Living Room TV',
              'kind': 'dlna',
            },
          ]);
        },
        onCancel: (_) {
          routeCancelCount += 1;
        },
      ),
    );
    addTearDown(() {
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockStreamHandler(customRoutesChannel, null);
    });

    final controller = VesperExternalPlaybackController(
      methodChannel: channel,
      routesEventChannel: customRoutesChannel,
    );
    final routes = <List<VesperExternalPlaybackRoute>>[];
    final subscription = controller.routes.listen(routes.add);
    await Future<void>.delayed(Duration.zero);

    expect(routeListenCount, 1);
    expect(routes.single.single.routeId, 'uuid:tv');

    await subscription.cancel();
    await Future<void>.delayed(Duration.zero);
    expect(routeCancelCount, 1);

    controller.dispose();
    expect(() => controller.routes, throwsStateError);
    expect(() => controller.connect('uuid:tv'), throwsStateError);
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
