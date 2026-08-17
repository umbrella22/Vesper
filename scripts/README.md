# scripts Directory

`scripts/` contains the thin `scripts/vesper` launcher and checked-in data used by
the Rust CLI. Build, verification, packaging, and release behavior live in the
`player-cli` binary so local and CI execution share one implementation.

## Layout

```text
scripts/
  vesper      Unified task entrypoint
  ffmpeg-profiles.toml       FFmpeg profile declarations
  ffmpeg-source-policy.toml  FFmpeg source and license policy
  ios/bridge-shim/            Generated C bridge fragments and manifest
```

## Common Commands

```sh
./scripts/vesper ffi generate
./scripts/vesper ffi sync
./scripts/vesper ffi verify
./scripts/vesper ffi c-host-smoke

./scripts/vesper ffmpeg --list-profiles
./scripts/vesper ffmpeg --platform android --profile default --abi arm64-v8a
./scripts/vesper ffmpeg --platform ios --profile default --slice ios-arm64 --slice ios-simulator-arm64
./scripts/vesper android jni release arm64-v8a
./scripts/vesper android aar
./scripts/vesper android source-normalizer-plugin /tmp/vesper-android-source-normalizer release --profile default
./scripts/vesper android frame-processor-plugin /tmp/vesper-android-frame-processor release
./scripts/vesper android stage-release
VESPER_ANDROID_INCLUDE_OPTIONAL_PLUGINS=1 ./scripts/vesper android stage-release
./scripts/vesper android sample-apks /tmp/vesper-android-samples arm64-v8a
./scripts/vesper android publish-maven-central vMAJOR.MINOR.PATCH --dry-run

./scripts/vesper ios ffi release
./scripts/vesper ios bootstrap-bridge-shim
./scripts/vesper ios sync-bridge-shim
./scripts/vesper ios verify-bridge-shim
./scripts/vesper ios stage-optional-plugins-release /tmp/vesper-ios-release --profile source-normalizer ios-arm64 ios-simulator-arm64
./scripts/vesper ios verify-optional-plugins-release /tmp/vesper-ios-release
./scripts/vesper ios verify-optional-plugins-device /tmp/vesper-ios-release \
  --device <UDID> \
  --development-team <TEAM_ID> \
  --output-directory /tmp/vesper-ios-device-evidence \
  --allow-provisioning-updates
./scripts/vesper ios verify-app-store-layout /path/to/App.app
VESPER_IOS_OPTIONAL_RELEASE_FIXTURE=/tmp/vesper-ios-release \
  cargo test -p player-cli --test ios_release_regressions \
  ios_optional_release_real_fixture_rejects_policy_drift \
  -- --ignored --exact --nocapture --test-threads=1
./scripts/vesper ios kit-xcframework
./scripts/vesper ios stage-release /tmp/vesper-ios-release
./scripts/vesper ios verify-release /tmp/vesper-ios-release --scope core
./scripts/vesper ios stage-release /tmp/vesper-ios-release --include-optional-plugins
./scripts/vesper ios verify-release /tmp/vesper-ios-release --scope complete
./scripts/vesper ios publish-spm-index \
  vMAJOR.MINOR.PATCH \
  /tmp/vesper-ios-release/VesperPlayerKit.xcframework.zip \
  --source-repository umbrella22/Vesper \
  --dry-run

./scripts/vesper desktop ensure-ffmpeg
./scripts/vesper desktop verify-decoder-diagnostics
./scripts/vesper desktop verify-decoder-videotoolbox loader
./scripts/vesper desktop verify-remux

./scripts/vesper mobile verify-no-remux android
./scripts/vesper mobile verify-no-remux ios
./scripts/vesper mobile verify-binary-names
./scripts/vesper flutter stage-pub /tmp/vesper-flutter-pub
./scripts/vesper flutter pub-dry-run /tmp/vesper-flutter-pub
VESPER_FLUTTER_INCLUDE_OPTIONAL_PLUGINS=1 ./scripts/vesper flutter pub-dry-run /tmp/vesper-flutter-pub
./scripts/vesper release prepare-from-tag vMAJOR.MINOR.PATCH
./scripts/vesper release tag-channel vMAJOR.MINOR.PATCH
./scripts/vesper release verify-current
./scripts/vesper release notes <tag> [output-path]
```

`ios bootstrap-bridge-shim` is an explicit migration/import command. It
reconstructs the checked-in bridge manifest and C fragments from the current
`VesperPlayerKitBridgeShim.c/.h`; it does not regenerate the final C/H output.
Run it when an API was added to the generated bridge before its manifest and
fragments were recorded, then run `ios sync-bridge-shim` and
`ios verify-bridge-shim`. Ordinary sync refuses to remove public bridge
functions unless `--allow-public-api-removal` is passed explicitly.

`desktop ensure-ffmpeg` is an explicit macOS provisioning step. It installs the
repository-local FFmpeg fallback before Cargo builds that need FFmpeg; Cargo no
longer launches a shell provisioning wrapper during dependency discovery. When
using `VESPER_DESKTOP_FFMPEG_DIR`, add that directory's `lib/pkgconfig` child to
`PKG_CONFIG_PATH` for subsequent Cargo commands.

## Gradle Resolution

The Rust CLI resolves Gradle through the same policy in local and CI runs.
GitHub Actions installs Gradle with `gradle/actions/setup-gradle`; local runs
use a cached project distribution under `.gradle/wrapper/dists/**/bin/gradle`
and fail clearly when that cache is absent. Each Android project also keeps its
Gradle service home under `<project>/.gradle/gradle-user-home`; the repository
root does not contain shared Gradle state. An explicit non-empty
`GRADLE_USER_HOME` override still takes precedence. Invoke Android commands
through `./scripts/vesper`; no shell helper is required.

## GitHub Actions

Every workflow that invokes `./scripts/vesper` first runs the local
`.github/actions/setup-vesper-cli` composite action. The action installs Rust
1.97, builds `player-cli` once with `cargo build --locked --release`, and
exports the resulting executable through `VESPER_CLI`. The thin
`scripts/vesper` launcher then executes that prebuilt binary instead of running
Cargo for every command. On Windows, CLI steps that use the launcher explicitly
select Bash while the setup action exports `vesper.exe`.

The workflow split is intentional:

- `boundary-ci.yml` verifies repository contracts and boundary invariants.
- `desktop-ci.yml` runs Rust lint/test gates plus desktop plugin, FFI, and remux
  checks across Linux, macOS, and Windows.
- `mobile-hosts-ci.yml` builds and tests the Android and iOS host integrations;
  its iOS job verifies archives and Simulator behavior without claiming a
  signed physical-device result.
- `flutter-ci.yml` analyzes, tests, and packages the federated Flutter hosts.
- `mobile-lib-release.yml` stages and verifies tagged Android/iOS release
  assets, publishes the GitHub Release, then publishes stable or prerelease
  Android coordinates to Maven Central and the matching iOS binary package
  through its SwiftPM index repository. The Maven set contains seven
  coordinates, including the opt-in SourceNormalizer and remux plugins.
- `flutter-pub-release.yml` publishes stable or prerelease Flutter packages
  after applying tag-derived version metadata and waiting for all hosted Maven
  and SwiftPM dependencies.

The signed iOS physical-device acceptance command remains a release-owner gate
because it requires a connected device, a current Apple Development identity,
and device trust. Its current execution status is tracked in
`CURRENT-CHECKLIST.md`, not inferred from a successful archive-only CI job.

Stable GitHub Releases are assembled as drafts. The workflow reconciles and
hash-verifies every draft asset before making the release public. A published
stable release is immutable: reruns validate its remote asset names and its
published `SHA256SUMS.txt`, then continue any interrupted Maven Central or
SwiftPM publication without replacing release files.

## Flutter Pub Publishing

Repository `pubspec.yaml` files use publish-ready hosted constraints. Local
development uses generated, ignored `pubspec_overrides.yaml` files:

```sh
./scripts/vesper flutter local-overrides
```

Flutter CI generates the same overrides before resolving package or example
dependencies. Source validation therefore does not depend on the checkout's
package version already being available from pub.dev.

The pub helpers stage temporary packages, copy the root license, remove any
local-only publication metadata, and normalize internal package constraints for
the selected version. If no version argument is passed, the staging helper uses
the current `vesper_player` package version.

Default Flutter pub staging, dry-run, and publish commands include only the
main package family. Optional native dependency packages such as
`vesper_player_source_normalizer_ffmpeg` are skipped unless
`VESPER_FLUTTER_INCLUDE_OPTIONAL_PLUGINS=1` is set.

The tag workflow publishes the default six-package Flutter family followed by
`vesper_player_source_normalizer_ffmpeg` and
`vesper_player_remux_ffmpeg`. The two optional packages are published only
after their matching native artifacts, FFmpeg compliance archive, exact
corresponding source archive, seven Maven coordinates, and capability-level
SwiftPM products are available.

Release workflows do not hardcode product versions. They derive the numeric
product version and full publication version from the pushed tag, apply them to
the CI workspace with
`./scripts/vesper release prepare-from-tag "$RELEASE_TAG"`, and verify the
updated metadata before packaging. Stable tags publish `MAJOR.MINOR.PATCH`;
prerelease tags publish the full version such as `MAJOR.MINOR.PATCH-rc.1` to
Maven Central, SwiftPM, and pub.dev while Cargo and platform bundle versions
retain the numeric base.

pub.dev accepts GitHub OIDC publication only from a tag-push workflow. The
first version of each package must be published interactively by its initial
uploader; after that, configure each package for repository
`umbrella22/Vesper`, tag pattern `v{{version}}`, and GitHub environment
`pub.dev` before relying on stable-tag automation.

Publish the first version of the default six-package family from the release
checkout with the final owner account:

```sh
./scripts/vesper flutter pub-publish \
  /tmp/vesper-flutter-pub-MAJOR.MINOR.PATCH \
  MAJOR.MINOR.PATCH \
  --include-optional-plugins=false
```

The publish helper checks the exact package version on pub.dev before every
upload. Re-running the same command after a network, authentication, or
pub.dev rate-limit failure skips versions that were already accepted and
continues in dependency order. During one uninterrupted first publication it
also respects pub.dev's new-package burst window before creating a fifth
package. If pub.dev reports that the account-level creation limit is already
exhausted by an earlier process, wait for the reported window and rerun the
same command.

The release workflow runs `dart-lang/setup-dart` before Flutter so the Dart pub
client receives the short-lived GitHub OIDC credential. It does not use a
long-lived `PUB_TOKEN` secret.

## Mobile FFmpeg Profiles

The public mobile FFmpeg entrypoint is the root command:
`./scripts/vesper ffmpeg --platform android|ios|all --profile <name>`.
Profiles are declared in `scripts/ffmpeg-profiles.toml`. The resolver supports
profile inheritance, platform overrides, validation policy, and command-line
overlays. `download-remux`, `relay-remux`, and `default` keep local remux
semantics by validating `--disable-network` and `--disable-openssl`.

```sh
./scripts/vesper ffmpeg \
  --platform android \
  --profile default \
  --abi arm64-v8a

./scripts/vesper ffmpeg \
  --platform ios \
  --profile download-remux \
  --slice ios-arm64 \
  --slice ios-simulator-arm64
```

Android FFmpeg runtime packaging is split from consumers. The root command builds
`vesper-player-kit-ffmpeg-runtime` by default; pass `--android-artifact prebuilts`
only when a private flow needs raw prebuilts. Default standalone Android release
staging publishes the host kit and Compose AARs only. Stable and prerelease
Maven publication adds external playback, the shared FFmpeg runtime,
SourceNormalizer, and remux as hosted coordinates. SourceNormalizer and remux
remain direct opt-ins; their POMs close over the same-version core and runtime.
Set `VESPER_ANDROID_INCLUDE_OPTIONAL_PLUGINS=1`, or run the dedicated plugin
build commands, when you intentionally want the complete source-staged set,
including Decoder or FrameProcessor extension AARs. `player-remux-ffmpeg`,
`player-source-normalizer-ffmpeg`, and the external-playback relay FFmpeg JNI
library must package only their own plugin/JNI libraries and depend on the
shared runtime AAR. The FrameProcessor diagnostic plugin is not FFmpeg-backed.

```sh
./scripts/vesper ffmpeg --platform android --profile default --abi arm64-v8a
./scripts/vesper android remux-plugin /tmp/vesper-android-remux release --profile download-remux
./scripts/vesper android source-normalizer-plugin /tmp/vesper-android-source-normalizer release --profile default
```

The external-playback relay FFmpeg JNI library is built by the Android
`vesper-player-kit-external-playback` Gradle module through its private
`buildRelayFfmpegAndroidJni` task. Release and example builds use the `default`
profile so the shared runtime and relay JNI profile hashes match.

iOS core kit packaging does not include FFmpeg. Local `stage-release` calls are
core-only unless `--include-optional-plugins` is passed or the lower-priority
`VESPER_IOS_INCLUDE_OPTIONAL_PLUGINS=1` environment setting is present. Tagged
release CI enables that option and publishes the optional FFmpeg runtime, remux,
SourceNormalizer, decoder, and FrameProcessor XCFrameworks. Repository native
and Flutter hosts use one canonical optional staging entrypoint:

```sh
./scripts/vesper ios stage-optional-plugins-release /tmp/vesper-ios-release \
  --profile source-normalizer \
  ios-arm64 ios-simulator-arm64
```

This produces three FFmpeg component XCFrameworks (`VesperFFmpegAVCodec`,
`VesperFFmpegAVFormat`, and `VesperFFmpegAVUtil`) plus Remux,
SourceNormalizer, VideoToolbox Decoder, and diagnostic FrameProcessor plugin
XCFrameworks. The App target embeds and signs them as seven top-level sibling
frameworks. Flat dylibs, nested frameworks, and the legacy umbrella runtime are
not distributable layouts. The release verifier also requires exactly one
device framework slice and one Simulator framework slice, each with exactly one
`arm64` architecture and matching bundle, SDK, and Mach-O platform metadata.
Extra top-level release assets and extra XCFramework slices fail verification.
Optional staging also emits
`VesperPlayerOptionalPlugins-FFmpeg-Compliance.zip` and exactly one versioned
corresponding-source tarball. Verification compares the packaged LGPL and
FFmpeg license files with that source, compares the Vesper notices with the
checkout, and checks the rebuild and relinking instructions. Release staging
forces a fresh FFmpeg source build instead of reusing cached dylibs. Run the
verifier or its release regressions directly with:

```sh
./scripts/vesper ios verify-release /tmp/vesper-ios-release --scope complete
./scripts/vesper ios verify-optional-plugins-release /tmp/vesper-ios-release
VESPER_IOS_OPTIONAL_RELEASE_FIXTURE=/tmp/vesper-ios-release \
  cargo test -p player-cli --test ios_release_regressions \
  ios_optional_release_real_fixture_rejects_policy_drift \
  -- --ignored --exact --nocapture --test-threads=1
```

The physical-device verifier is a separate execution gate from
`verify-release --scope complete`. It retains the verified optional Release
snapshot through project generation, Release XCTest execution, and XCResult
parsing. Acceptance requires exactly 3 passed, 0 failed, 0 skipped, and 0
expected failures. The new output directory retains
`verified-release-inputs.json`, with both original Release ZIP and sanitized
tested ZIP SHA-256 values, plus `VesperOptionalPlugins.xcresult`.

Every FFmpeg-backed framework writes `profile-hash.txt`; staging and app-layout
verification fail if the hashes do not match. Plugin XCFrameworks must not
contain duplicate `libav*`, `libsw*`, `libxml2*`, `libssl*`, or `libcrypto*`
libraries. SourceNormalizer can participate through an explicit normalized
resource route. Decoder and FrameProcessor participation remains opt-in through
the SDK-managed native-frame route; default AVPlayer playback is unchanged.

Supported overlays are:

- `--extra-libraries`
- `--extra-demuxers`
- `--extra-muxers`
- `--extra-protocols`
- `--extra-parsers`
- `--extra-bsfs`
- `--extra-configure-arg`
- `--tls-backend none|openssl` for Android
- `--tls-backend none|securetransport` for Apple

Lists may be comma or space separated. CI and documentation should use the root
`ffmpeg` command for runtime prebuilts; private Gradle/Xcode build phases may
consume the resolved artifacts produced by that command.

Resolved profile outputs are written under
`third_party/ffmpeg/<platform>/profiles/` by default. Every prebuilt slice writes
`vesper-ffmpeg-build-metadata.txt` with the declared profile, profile hash,
external dependencies, license-sensitive flags, source archive SHA-256, full
configure line, and platform linker overrides. Apple builds record the Darwin
shared-library flags used in place of FFmpeg's obsolete `-single_module`
default; the upstream source archive remains unmodified.
Source archives are cached under `third_party/_cache` by default; override this
with `VESPER_THIRD_PARTY_SOURCE_CACHE_DIR` when local automation keeps third-party
tarballs somewhere else.

## Conventions

- The default Android ABI is `arm64-v8a`; override it with command arguments or `RUST_ANDROID_ABIS`.
- The default Android NDK version is `29.0.14206865`. Gradle and CI pin that
  version. The Rust CLI honors explicit overrides and otherwise resolves a
  complete installation from `ANDROID_SDK_ROOT` / `ANDROID_HOME`, including a
  fallback installed NDK when the default version is unavailable.
- The default Apple/iOS slices are `ios-arm64` and `ios-simulator-arm64`; do not reintroduce x86 / x86_64 distribution slices.
- iOS CLI build commands pass the resolved SDK root `Cargo.toml` through
  `--manifest-path` so they can run from Xcode build phases, Flutter plugin
  builds, CI workspaces, or temporary directories.
- FFmpeg, OpenSSL, and libxml2 version, source URL, source archive, and output
  directory overrides continue to use the existing `VESPER_*` environment
  variable semantics.
- FFmpeg source builds default to the shared audited source series declared in
  `scripts/ffmpeg-source-policy.toml`. The resolver selects the highest matching patch from
  `third_party/_cache` before consulting upstream release indexes. Use
  `VESPER_FFMPEG_SERIES` / `VESPER_<PLATFORM>_FFMPEG_SERIES` for intentional
  series moves, and `VESPER_FFMPEG_VERSION` /
  `VESPER_<PLATFORM>_FFMPEG_VERSION` only for exact-version reproduction.
- Android OpenSSL provisioning is opt-in through FFmpeg TLS overlays and uses
  the OpenSSL 3.5 LTS series by default. The resolver uses the same cache-first
  patch selection as FFmpeg. Override `VESPER_ANDROID_OPENSSL_SERIES` only for
  intentional LTS-series moves and `VESPER_ANDROID_OPENSSL_VERSION` only for
  exact-version reproduction.
- Shell implementation helpers are not part of the supported interface. Keep
  argument parsing and build behavior in the Rust CLI.
