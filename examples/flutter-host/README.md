# Vesper Flutter Host Demo

This example is the runnable Flutter host app for the Vesper Player SDK federated plugin.

It intentionally lives under `examples/` so it can serve as:

- a cross-platform host-integration reference
- a runnable validation app for Flutter consumers
- a thin shell over the Android / iOS host kits

## Stack

- `Flutter`
- federated `vesper_player` packages
- Android and iOS implementations both route into the native host kits

## Current Status

This host app now covers:

- source selection
- quality / audio / subtitle / speed sheets
- resilience policy entry points
- Android / iOS platform routing through the existing host kits
- LiveDvr helper regression cases at the Flutter host layer

## Host Regression

The executable regression path for this example is:

1. install dependencies:
   - `cd examples/flutter-host && flutter pub get`
2. run the host regression suite:
   - `cd examples/flutter-host && flutter test`
3. optionally build Android / iOS host artifacts:
   - `cd examples/flutter-host && flutter build apk --release`
   - `cd examples/flutter-host && flutter build ios --release --no-codesign`

The current host regression cases cover:

- `Go Live` fallback to the seekable window end
- live edge tolerance / offset behavior
- pending seek ratio clamp
- stale position clamp after DVR window shrink

## CI

- `.github/workflows/flutter-ci.yml`
  - `flutter analyze`
  - `flutter test`
  - Android release APK build
  - iOS release build
