# Android Staged AAR Consumer

This fixture verifies the Android artifacts produced by `vesper android stage-release`
from the perspective of an external application. It contains only one `:app` module,
uses raw AAR file dependencies, and never invokes repository Rust or Android build tasks.

Run the Release instrumentation tests with a staged artifact directory:

```sh
GRADLE_USER_HOME="$PWD/.gradle/gradle-user-home" \
  /path/to/cached/gradle -p fixtures/plugins/android-aar-consumer \
  connectedReleaseAndroidTest \
  -Pvesper.releaseDir=/path/to/staged-aars
```

The tests require an Android API 26+ arm64-v8a device. They verify packaged registry
fragments and native libraries, load SourceNormalizer, FrameProcessor, and MediaCodec
decoder capabilities through the public `VesperPluginReference` API, and execute the
native-frame route against a real `SurfaceView` until decoded, processed, and presented
frame counters advance.
