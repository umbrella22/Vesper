# Flutter pub.dev Consumer

This fixture resolves the published `vesper_player: 0.9.1` package from pub.dev.
It has no `path:` dependencies, `pubspec_overrides.yaml`, or checkout-local
federated implementation packages.

Run the consumer check with a network-enabled Flutter installation:

```sh
flutter pub get
flutter analyze
flutter test
```

The fixture checks hosted package resolution and Dart API compilation. Native
Android/iOS playback remains covered by the platform host and device gates.
