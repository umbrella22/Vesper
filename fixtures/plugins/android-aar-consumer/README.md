# Android Staged AAR Consumer

This fixture verifies Vesper Android artifacts from the perspective of an external
application. It contains only one `:app` module and never invokes repository Rust or
Android build tasks. It can consume either raw staged AAR files or the hosted
external-playback Maven coordinate and its transitive core / FFmpeg runtime closure.

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

To verify the hosted external-playback dependency closure without a device:

```sh
GRADLE_USER_HOME="$PWD/.gradle/gradle-user-home" \
  /path/to/cached/gradle -p fixtures/plugins/android-aar-consumer \
  :app:assembleRelease \
  -Pvesper.mavenVersion=0.5.0
```
