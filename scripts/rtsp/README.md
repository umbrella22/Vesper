# RTSP bridge validation

The optional bridge ingests live RTSP/1.0 unicast and remuxes encoded samples
into fMP4 HLS on an ephemeral IPv4 loopback port. Android delegates the HLS
item to Media3. The iOS and HarmonyOS kits delegate it to system AVPlayer.
All three preserve the original RTSP source identity and live timeline. The
loopback URL remains scoped to the native playback session and bypasses the
SDK media cache.

- Video accepts progressive 8-bit H.264 in RTP packetization mode 0 or 1 and
  single-layer 4:2:0 HEVC with no decoding-order-number extension. Both use
  increasing presentation timestamps. The HLS target duration stays at two
  seconds. Publishable fragments are shorter than 2.5 seconds. An overlong
  GOP is discarded and output can resume at the next random-access point
  with a discontinuity. Configure the source for a GOP shorter than 2.5 seconds.
  A one-second random-access interval is the fixture baseline.
- An optional AAC-LC track uses complete MPEG4-GENERIC AAC-hbr access units.
  The bridge prefers shared `PLAY` RTP-Info anchors for A/V timing. When the
  origin omits them, alignment is estimated from first arrivals. Unsupported
  audio codecs are omitted and video can still play.
- RTSP URL user info supplies credentials for Basic and Digest challenges.
  Digest supports MD5 and SHA-256 with `qop=auth` or legacy no-qop challenges.
  RTSP control requests omit user info from their request targets. Basic
  credentials travel over the origin's cleartext RTSP connection.
- TCP interleaving is preferred. A `461 Unsupported Transport` response
  triggers UDP unicast using the control connection's peer and negotiated
  server ports. The loopback output remains HTTP in either case.

The bridge handles live streams. Reordered video timestamps, multicast,
RTSPS, standalone RTP URLs, and RTSP VOD seeking are outside this preview.

The first playlist requires two complete fragments. After a transient failure
in a ready session, the bridge keeps serving the live playlist during a
20-second recovery window. Two fresh fragments on the same origin connection
restore `Ready`. Media3 allows up to 30 seconds of unchanged playlists on
the local bridge route. If the bridge exhausts its recovery window, new
playlist requests return `410 Gone`.

## Start the controlled source

Run the fixture with Python 3, MediaMTX 1.21.4, and FFmpeg with `libx264`.
It publishes a 640 × 360, 30 fps pattern with a UTC clock and a frame counter.
`--aac` adds a 48 kHz AAC-LC tone. `--video hevc` uses `libx265`.
`--transport udp` makes the origin decline TCP to exercise the fallback.
`--auth basic` and `--auth digest` require the fixture reader credentials. Use
`rtsp://fixture-reader:vesper-fixture-test@127.0.0.1:8554/clock` with either mode.
The server binds to loopback by default. `--port`, `--fps`, and `--gop` change
the transport endpoint and encoder cadence. Set `--listen` to a development
machine's LAN address for a phone on the same trusted network.

```sh
python3 scripts/rtsp/fixture.py --mediamtx /path/to/mediamtx
```

FFmpeg and MediaMTX run on the development machine. The bridge runtime uses
Rust and the platform player on the consuming device.

## Verify the bridge on the host

```sh
cargo test -p player-rtsp-hls-bridge
cargo build -p player-rtsp-hls-bridge --example bridge
python3 scripts/rtsp/verify.py --mediamtx /path/to/mediamtx \
  --output target/rtsp-fixture/host-verification
```

The verification script starts a separate origin on an ephemeral port. It
checks codec metadata through both paths, decodes eight seconds of HLS, and
checks loopback HTTP requests. Add `--auth basic` or `--auth digest` to verify
successful playback and rejection of missing or incorrect credentials. Add
`--video hevc --aac --transport udp` for HEVC and AAC with UDP fallback, or
`--gop 61` to exercise the two-second target's rounding margin. The
UDP integration test drops an RTP fragment and checks that the next complete
GOP starts a new HLS discontinuity on the same origin connection. The restart check verifies that the playlist
remains available for ten seconds with the origin offline. It confirms
republication on the same port in `origin-restarted.log` and checks for a
discontinuity, an advancing media sequence, and a return to `Ready`. A second
outage checks that the bridge fails with `TimedOut` when the origin stays
offline. `bridge.log` records the phase changes and `receipt.json` records the
results.

## Build the optional native routes

| Host kit | Build selection |
| --- | --- |
| Android | `VESPER_ANDROID_RTSP_BRIDGE=1` with the usual Android kit build |
| iOS | `VESPER_IOS_RTSP_BRIDGE=1 ./scripts/vesper ios ffi` |
| HarmonyOS API 26 | `VESPER_HARMONY_RTSP_BRIDGE=1 python3 scripts/harmony/build-kit.py --tools <sdk>` |

The SDK returns `rtsp_bridge_not_bundled` when a host kit was built without
its feature. The `rtsp://` source uses the ordinary controller. On iOS pass
`.rtsp` to `VesperPlayerSource.remoteUrl`. On HarmonyOS register a remote
source with `protocol: 'rtsp'`. Flutter uses `VesperPlayerSource.rtsp(...)`
through the corresponding Android or iOS kit.

The fixture consumer's `Check RTSP playback` scenario exercises a HarmonyOS
HAR through API 26 AVPlayer with port `18554` forwarded to a local fixture.
`Check RTSP feature off` verifies the missing-feature error. The iOS
`VesperRtspPlaybackTests` test consumes `VESPER_RTSP_TEST_URI` from its XCTest
environment and verifies playback progress and teardown. Set
`VESPER_RTSP_EXPECT_AUDIO=1` in that environment to check AVPlayer's audio
selection with the AAC fixture. Set `VESPER_RTSP_EXPECT_BUNDLED=0` for the
disabled kit. Set `VESPER_IOS_RTSP_BRIDGE=1` in the Xcode build environment if
the project rebuilds the Rust resolver. A prebuilt resolver uses
`VESPER_IOS_FFI_PREBUILT=1` with the matching platform archive already staged.
The Flutter `integration_test/rtsp_contract_test.dart` test accepts the same
source through `--dart-define=VESPER_RTSP_TEST_URI=...` and uses
`--dart-define=VESPER_RTSP_EXPECT_BUNDLED=false` for the disabled kit.
`VESPER_RTSP_EXPECT_AUDIO=true` asserts the Android AAC fixture format and
`VESPER_RTSP_TEST_PROGRESS_MS=8000` extends the playback observation for an
origin restart. For a restart check, republish the same fixture on its original
port within the recovery window and require progress beyond the pre-outage
media.

For a physical iPhone, give the test host a `NSLocalNetworkUsageDescription`
and grant Local Network access before timing a LAN trial. If the device
exposes per-app Wi-Fi controls, the test host also needs Wi-Fi access.
`NWPath.UnsatisfiedReason.wifiDenied` identifies a Wi-Fi access failure before
RTSP negotiation. Use `flutter test --no-uninstall` for repeated trials so the
installed test app retains its network settings.

## Run the Flutter Android instrumentation test

Flutter's `VesperRtspFlutterTest` runner reports device results through
`am instrument`. Build the test entry point with the cached Gradle distribution
and install the two APKs on the test device. Android instrumentation receives
Dart defines as a comma-separated list of base64-encoded `NAME=value` strings.

```sh
project="$PWD/examples/flutter-host/android"
gradle_bin=$(find "$project/.gradle/wrapper/dists" -path '*/gradle-9.3.1/bin/gradle' -type f -perm -111 | head -1)
test -n "$gradle_bin" || exit 1
uri=rtsp://127.0.0.1:8554/clock
dart_defines=$(printf %s "VESPER_RTSP_TEST_URI=$uri" | base64 | tr -d '\n')
VESPER_ANDROID_RTSP_BRIDGE=1 GRADLE_USER_HOME="$project/.gradle/gradle-user-home" \
  "$gradle_bin" -p "$project" :app:assembleDebug :app:assembleDebugAndroidTest \
  -Ptarget=integration_test/rtsp_contract_test.dart -Ptarget-platform=android-arm64 \
  -Pdart-defines="$dart_defines"
outputs="$PWD/examples/flutter-host/build/app/outputs/apk"
serial=YOUR_DEVICE_SERIAL
package=io.github.umbrella22.vesper.example.flutterhost
adb -s "$serial" install -r -t "$outputs/debug/app-debug.apk"
adb -s "$serial" install -r -t "$outputs/androidTest/debug/app-debug-androidTest.apk"
adb -s "$serial" reverse tcp:8554 tcp:8554
adb -s "$serial" shell am instrument -w -r -e class "$package.VesperRtspFlutterTest" \
  "$package.test/androidx.test.runner.AndroidJUnitRunner"
adb -s "$serial" reverse --remove tcp:8554
```

UDP trials use the fixture's LAN address so the device can receive negotiated
RTP datagrams on its own sockets. The USB reverse forward carries TCP only.

## Run the Android device comparison

Build the example from the root with `VESPER_ANDROID_RTSP_BRIDGE=1`, the cached
project-local Gradle distribution, and the project-local Gradle user home.
The flag selects the core feature when the build produces the existing Android
JNI library. Apps using the RTSP bridge require a Network Security
Configuration that permits cleartext HTTP to `127.0.0.1`. The debug example
already permits the loopback connection.

```sh
project="$PWD/examples/android-compose-host"
gradle_bin=$(find "$project/.gradle/wrapper/dists" -path '*/gradle-9.7.1/bin/gradle' -type f -perm -111 | head -1)
test -n "$gradle_bin" || exit 1
VESPER_ANDROID_RTSP_BRIDGE=1 GRADLE_USER_HOME="$project/.gradle/gradle-user-home" \
  "$gradle_bin" -p "$project" :app:assembleDebug
```

Use `adb devices -l` to select the phone. The installed Android system must
list `arm64-v8a` in `ro.product.cpu.abilist64`. Run both routes with the same
source on each device. The `direct` route uses
Media3 RTSP with TCP. The `bridge` route uses the Vesper controller and Media3
HLS. Both routes request hardware decoding and display on a `SurfaceView`.

```sh
serial=YOUR_DEVICE_SERIAL
package=io.github.umbrella22.vesper.example.androidcomposehost
adb -s "$serial" shell getprop ro.product.cpu.abilist64
adb -s "$serial" install -r "$project/app/build/outputs/apk/debug/app-debug.apk"
adb -s "$serial" reverse tcp:8554 tcp:8554
adb -s "$serial" shell am start -S -n "$package/.RtspLabActivity" --es route direct
adb -s "$serial" logcat -d -s VesperRtspLab VesperPlayerAndroidHost
adb -s "$serial" shell am start -S -n "$package/.RtspLabActivity" --es route bridge
adb -s "$serial" logcat -d -s VesperRtspLab VesperPlayerAndroidHost
```

On the DS-MDT201 with Android 10, the system can prompt for data and WLAN
access when the debug example connects, including after an APK reinstall.
Allow access for the debug example before the timed trial. Confirm that the
app UID can reach the fixture, then cold-start the app. A successful request
from the device shell alone does not verify the app's network access.

Run each route as a separate trial and leave it playing while collecting
samples. `adb reverse` reaches the host fixture through USB. To test over Wi-Fi,
start the fixture with `--listen` set to the host's LAN address and pass
`--es uri rtsp://HOST:8554/clock` to both activity launches.

Record first-frame time, decoder name, three steady-state latency samples,
visible corruption, and recovery for each route. Estimate live delay from the
visible UTC timestamp and the host time at capture. Keep the screen capture
duration with each estimate. For startup comparisons, record the first rendered
frame and check for a second buffering transition after playback begins.
For recovery, keep playback open and stop the fixture. Measure the outage from
listener closure to confirmed republication on the same port. Test both a
short outage and a ten-second outage on the bridge route. Time recovery from
the new publication to the first displayed clock from the restarted encoder.
Confirm advancement in a second capture.
