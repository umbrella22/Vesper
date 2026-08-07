#![cfg(vesper_source_checkout)]

use std::fs;
#[cfg(unix)]
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use player_cli::{
    EmbeddedRegistryFragment, EmbeddedRegistryTarget, PluginDescriptor, PluginProjectManifest,
};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

fn manifest(plugin_id: &str) -> String {
    format!(
        r#"
schema_version = 1

[plugin]
id = "{plugin_id}"
name = "Fixture"
version = "1.2.3"
description = "Fixture plugin"
license = "Apache-2.0"
publisher = "dev.vesper.publisher"

[compatibility]
host_sdk = ">=0.4.0, <0.5.0"
abi_major = 1
abi_minor_min = 0
abi_minor_max = 0

[[capabilities]]
interface_id = "e9479dbc-42d2-575e-b39e-a24bc512fbc7"
instance_id = "dev.vesper.fixture.post-download"
interface_major = 1
interface_minor = 0
stability = "stable"
"#
    )
}

fn temporary_manifest(source: &str) -> PathBuf {
    let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "vesper-cli-manifest-{}-{id}.toml",
        std::process::id()
    ));
    fs::write(&path, source).expect("write manifest");
    path
}

fn package_manifest() -> String {
    format!(
        r#"{}

[[artifacts]]
transport = "native"
target = "aarch64-apple-darwin"
format = "dylib"
source = "fixture plugin.dylib"
path = "artifacts/aarch64-apple-darwin/fixture plugin.dylib"
architecture = "arm64"
minimum_os = "13.0"
capabilities = [{{ interface_id = "e9479dbc-42d2-575e-b39e-a24bc512fbc7", instance_id = "dev.vesper.fixture.post-download" }}]

[[package_files]]
source = "LICENSE"
path = "licenses/LICENSE"
kind = "license"

[[package_files]]
source = "NOTICE"
path = "notices/NOTICE"
kind = "notice"
"#,
        manifest("dev.vesper.fixture")
    )
}

#[cfg(unix)]
fn wasm_build_manifest(source: &str) -> String {
    format!(
        r#"{}

[[artifacts]]
transport = "wasm"
target = "wasm32-wasip2"
format = "wasm-component"
source = {source:?}
path = "artifacts/wasm32-wasip2/fixture.wasm"
architecture = "wasm32"
capabilities = [{{ interface_id = "e9479dbc-42d2-575e-b39e-a24bc512fbc7", instance_id = "dev.vesper.fixture.post-download" }}]
"#,
        manifest("dev.vesper.fixture")
    )
}

#[cfg(unix)]
fn write_executable_script(path: &std::path::Path, source: &str) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, source).expect("write executable script");
    let mut permissions = fs::metadata(path).expect("script metadata").permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions).expect("make script executable");
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
struct NativeAndroidFfmpegFixture {
    path: std::ffi::OsString,
    tools: PathBuf,
    sdk: PathBuf,
    rust_target_libdir: PathBuf,
    curl_marker: PathBuf,
    shell_marker: PathBuf,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn prepare_native_android_ffmpeg_fixture(
    root: &std::path::Path,
    tools: &std::path::Path,
    version: &str,
) -> NativeAndroidFfmpegFixture {
    use sha2::{Digest, Sha256};

    let scripts = root.join("scripts");
    let android_scripts = scripts.join("android");
    let cache = root.join("third_party/_cache");
    let sdk = root.join("android-sdk");
    let ndk = sdk.join("ndk/29.0.14206865");
    #[cfg(target_os = "macos")]
    let host_tag = "darwin-x86_64";
    #[cfg(target_os = "linux")]
    let host_tag = "linux-x86_64";
    let toolchain = ndk.join("toolchains/llvm/prebuilt").join(host_tag);
    let bin = toolchain.join("bin");
    let rust_target_libdir = root.join("fake-rust-target/lib");
    fs::create_dir_all(&android_scripts).expect("create fake Android worker directory");
    fs::create_dir_all(&cache).expect("create fake FFmpeg source cache");
    fs::create_dir_all(toolchain.join("sysroot/usr/include")).expect("create fake Android sysroot");
    fs::create_dir_all(&bin).expect("create fake Android toolchain bin");
    fs::create_dir_all(&rust_target_libdir).expect("create fake Rust target libdir");
    fs::create_dir_all(tools).expect("create fake Android tools");
    fs::write(ndk.join("source.properties"), "Pkg.Desc = Android NDK\n")
        .expect("write fake NDK metadata");
    fs::write(root.join("Cargo.toml"), "[workspace]\nresolver = \"2\"\n")
        .expect("write fake repository manifest");
    fs::write(
        scripts.join("ffmpeg-profiles.toml"),
        "[profile.default]\nlibraries = [\"avcodec\"]\nprotocols = [\"file\"]\ntls = \"none\"\n\n[profile.download-remux]\nlibraries = [\"avcodec\"]\nprotocols = [\"file\"]\ntls = \"none\"\n",
    )
    .expect("write fake FFmpeg profiles");

    let archive_path = cache.join(format!("ffmpeg-{version}.tar.xz"));
    let archive_file = fs::File::create(&archive_path).expect("create fake FFmpeg source archive");
    let encoder = xz2::write::XzEncoder::new(archive_file, 6);
    let mut archive = tar::Builder::new(encoder);
    let configure = b"#!/bin/sh\nset -eu\nprefix=\nfor argument in \"$@\"; do\n  case \"$argument\" in --prefix=*) prefix=${argument#--prefix=} ;; esac\ndone\n[ -n \"$prefix\" ]\nprintf '%s\\n' \"$prefix\" > .vesper-prefix\n";
    let mut header = tar::Header::new_gnu();
    header.set_size(configure.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    archive
        .append_data(
            &mut header,
            format!("ffmpeg-{version}/configure"),
            &configure[..],
        )
        .expect("append fake FFmpeg configure script");
    let encoder = archive.into_inner().expect("finish fake FFmpeg tar");
    encoder.finish().expect("finish fake FFmpeg xz stream");
    let libxml2_archive_path = cache.join("libxml2-2.14.6.tar.xz");
    let libxml2_archive_file =
        fs::File::create(&libxml2_archive_path).expect("create fake libxml2 source archive");
    let libxml2_encoder = xz2::write::XzEncoder::new(libxml2_archive_file, 6);
    let mut libxml2_archive = tar::Builder::new(libxml2_encoder);
    let mut libxml2_header = tar::Header::new_gnu();
    libxml2_header.set_size(configure.len() as u64);
    libxml2_header.set_mode(0o755);
    libxml2_header.set_cksum();
    libxml2_archive
        .append_data(
            &mut libxml2_header,
            "libxml2-2.14.6/configure",
            &configure[..],
        )
        .expect("append fake libxml2 configure script");
    let libxml2_encoder = libxml2_archive
        .into_inner()
        .expect("finish fake libxml2 tar");
    libxml2_encoder
        .finish()
        .expect("finish fake libxml2 xz stream");
    let source_sha256 = hex::encode(Sha256::digest(
        fs::read(&archive_path).expect("read fake FFmpeg archive"),
    ));
    fs::write(
        scripts.join("ffmpeg-source-policy.toml"),
        format!(
            "schema_version = 1\n\n[compatibility]\nrequirement = \">=8.1.0, <8.2.0\"\ndefault_series = \"8.1\"\n\n[release]\nversion = \"{version}\"\nsource_url = \"https://ffmpeg.org/releases/ffmpeg-{version}.tar.xz\"\nsource_sha256 = \"{source_sha256}\"\n"
        ),
    )
    .expect("write fake FFmpeg source policy");

    for tool in [
        "aarch64-linux-android26-clang",
        "aarch64-linux-android26-clang++",
        "llvm-ar",
        "llvm-nm",
        "llvm-ranlib",
        "llvm-strip",
    ] {
        write_executable_script(&bin.join(tool), "#!/bin/sh\nexit 0\n");
    }
    write_executable_script(
        &tools.join("rustc"),
        "#!/bin/sh\nset -eu\nif [ \"${1:-}\" = --print ]; then printf '%s\\n' \"$VESPER_FAKE_RUST_TARGET_LIBDIR\"; exit 0; fi\nexit 0\n",
    );
    write_executable_script(
        &tools.join("make"),
        r#"#!/bin/sh
set -eu
install=0
for argument in "$@"; do
  case "$argument" in install|install_sw) install=1 ;; esac
done
if [ "$install" = 1 ]; then
  prefix=$(cat .vesper-prefix)
  destination=${DESTDIR:?}/${prefix#/}
  mkdir -p "$destination/lib/pkgconfig" "$destination/lib"
  if [ -n "${ANDROID_OPTIONAL_LOG:-}" ]; then
    printf 'ffmpeg-native version=8.1.2\n' >> "$ANDROID_OPTIONAL_LOG"
  fi
  if [ -n "${ANDROID_SAMPLE_LOG:-}" ]; then
    printf 'ffmpeg version=8.1.2 url=https://ffmpeg.org/releases/ffmpeg-8.1.2.tar.xz sha=native-fixture\n' >> "$ANDROID_SAMPLE_LOG"
  fi
  case "$prefix" in
    */libxml2/*)
      printf 'prefix=%s\nName: libxml2\nVersion: 2.14.6\n' "$prefix" > "$destination/lib/pkgconfig/libxml-2.0.pc"
      printf 'fixture libxml2 runtime\n' > "$destination/lib/libxml2.so"
      ;;
    *)
      for library in avcodec avformat avutil; do
        printf 'prefix=%s\nName: lib%s\nVersion: 8.1.0\n' "$prefix" "$library" > "$destination/lib/pkgconfig/lib$library.pc"
        printf 'fixture ffmpeg runtime %s\n' "$library" > "$destination/lib/lib$library.so"
      done
      ;;
  esac
fi
"#,
    );
    let curl_marker = root.join("curl-called");
    write_executable_script(
        &tools.join("curl"),
        "#!/bin/sh\nprintf 'called\\n' > \"$VESPER_CURL_MARKER\"\nexit 97\n",
    );
    let shell_marker = root.join("shell-worker-called");
    let mut path_entries = vec![tools.to_path_buf()];
    path_entries.extend(std::env::split_paths(
        &std::env::var_os("PATH").expect("PATH"),
    ));
    NativeAndroidFfmpegFixture {
        path: std::env::join_paths(path_entries).expect("join fake Android tool PATH"),
        tools: tools.to_path_buf(),
        sdk,
        rust_target_libdir,
        curl_marker,
        shell_marker,
    }
}

#[cfg(unix)]
fn write_test_zip(path: &std::path::Path, entries: &[(&str, &[u8])]) {
    use zip::CompressionMethod;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create test ZIP parent");
    }
    let file = fs::File::create(path).expect("create test ZIP");
    let mut archive = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o644);
    for (name, bytes) in entries {
        archive
            .start_file(*name, options)
            .expect("start test ZIP entry");
        archive.write_all(bytes).expect("write test ZIP entry");
    }
    archive.finish().expect("finish test ZIP");
}

#[test]
fn flutter_android_release_verification_rejects_fixture_markers() {
    let directory = tempfile::tempdir().expect("create Flutter release APK fixture");
    let apk = directory.path().join("app release.apk");
    write_test_zip(
        &apk,
        &[
            ("AndroidManifest.xml", b"release manifest"),
            ("lib/arm64-v8a/libapp.so", b"flutter release aot"),
        ],
    );

    let valid = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["flutter", "verify-android-release"])
        .arg(&apk)
        .arg("--root")
        .arg(workspace_root())
        .output()
        .expect("verify Flutter Android release APK");
    assert_eq!(valid.status.code(), Some(0));
    assert!(valid.stderr.is_empty());
    assert!(String::from_utf8_lossy(&valid.stdout).contains("Verified Flutter Android release"));

    write_test_zip(
        &apk,
        &[
            ("AndroidManifest.xml", b"release manifest"),
            (
                "lib/arm64-v8a/libapp.so",
                b"flutter release aot fixtures/contracts",
            ),
        ],
    );
    let invalid = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["flutter", "verify-android-release"])
        .arg(&apk)
        .arg("--root")
        .arg(workspace_root())
        .output()
        .expect("reject Flutter Android fixture marker");
    assert_eq!(invalid.status.code(), Some(5));
    assert!(invalid.stdout.is_empty());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("test fixture markers"));
}

#[cfg(unix)]
#[test]
fn media_fixture_generation_runs_through_rust_and_replaces_the_output_tree() {
    let directory = tempfile::tempdir().expect("create media generator fixture");
    let root = directory.path().join("media fixture repository");
    let media = root.join("fixtures/media");
    fs::create_dir_all(media.join("generated")).expect("create previous generated fixtures");
    fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("write fixture manifest");
    fs::write(media.join("tiny-h264-aac.m4v"), b"source media").expect("write fixture source");
    fs::write(media.join("generated/previous.txt"), b"previous")
        .expect("write previous generated fixture");
    let ffmpeg = root.join("tools/mock ffmpeg");
    fs::create_dir_all(ffmpeg.parent().expect("mock FFmpeg parent"))
        .expect("create mock FFmpeg directory");
    write_executable_script(
        &ffmpeg,
        r#"#!/bin/sh
set -eu
for argument in "$@"; do
  output="$argument"
done
mkdir -p "$(dirname "$output")"
printf 'generated\n' > "$output"
case "$output" in
  */nonstandard/index.m3u8)
    printf 'init\n' > "$(dirname "$output")/init.mp4"
    printf 'segment\n' > "$(dirname "$output")/segment_00000.m4s"
    ;;
  */weird-dash/manifest.mpd)
    printf 'init\n' > "$(dirname "$output")/init-video.mp4"
    printf 'segment\n' > "$(dirname "$output")/chunk-video-00001.m4s"
    ;;
esac
"#,
    );

    let result = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("FFMPEG_BIN", &ffmpeg)
        .args(["media", "--root"])
        .arg(&root)
        .arg("generate-source-normalizer-fixtures")
        .output()
        .expect("generate SourceNormalizer fixtures through Rust CLI");

    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stderr.is_empty());
    assert!(
        String::from_utf8_lossy(&result.stdout)
            .contains(&media.join("generated").display().to_string())
    );
    assert!(!media.join("generated/previous.txt").exists());
    for relative in [
        "tiny-h264-aac.flv",
        "tiny-hevc-aac.flv",
        "tiny-broken-progressive.mp4",
        "nonstandard/index.m3u8",
        "nonstandard/init.mp4",
        "weird-dash/manifest.mpd",
    ] {
        assert!(
            media.join("generated").join(relative).is_file(),
            "{relative}"
        );
    }
}

#[cfg(unix)]
#[test]
fn media_fixture_generation_reports_bounded_ffmpeg_diagnostics() {
    let directory = tempfile::tempdir().expect("create media generator failure fixture");
    let root = directory.path().join("media failure repository");
    let media = root.join("fixtures/media");
    fs::create_dir_all(&media).expect("create media fixture directory");
    fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("write fixture manifest");
    fs::write(media.join("tiny-h264-aac.m4v"), b"source media").expect("write fixture source");
    let ffmpeg = root.join("tools/failing ffmpeg");
    fs::create_dir_all(ffmpeg.parent().expect("mock FFmpeg parent"))
        .expect("create mock FFmpeg directory");
    write_executable_script(
        &ffmpeg,
        "#!/bin/sh\nprintf 'fixture encoder rejected input\\n' >&2\nexit 17\n",
    );

    let result = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("FFMPEG_BIN", &ffmpeg)
        .args(["media", "--root"])
        .arg(&root)
        .arg("generate-source-normalizer-fixtures")
        .output()
        .expect("reject failed SourceNormalizer fixture generation");

    assert_eq!(result.status.code(), Some(5));
    assert!(result.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("H.264 FLV fixture generation exited unsuccessfully"));
    assert!(stderr.contains("fixture encoder rejected input"));
    assert!(!media.join("generated").exists());
}

#[cfg(target_os = "macos")]
fn write_core_ios_release_zip(path: &std::path::Path, root: &str) {
    use zip::CompressionMethod;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    let file = fs::File::create(path).expect("create core iOS release ZIP");
    let mut archive = ZipWriter::new(file);
    archive
        .add_directory(
            format!("{root}/"),
            SimpleFileOptions::default()
                .compression_method(CompressionMethod::Stored)
                .unix_permissions(0o040755),
        )
        .expect("write core iOS release root");
    archive
        .start_file(
            format!("{root}/Info.plist"),
            SimpleFileOptions::default()
                .compression_method(CompressionMethod::Stored)
                .unix_permissions(0o100644),
        )
        .expect("write core iOS release file");
    archive
        .write_all(b"fixture\n")
        .expect("write core iOS release payload");
    archive.finish().expect("finish core iOS release ZIP");
}

#[cfg(target_os = "macos")]
fn ios_verify_release_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let directory = tempfile::tempdir().expect("temporary iOS release verifier fixture");
    let root = directory.path().join("repository");
    let release = root.join("release output");
    fs::create_dir_all(&release).expect("create iOS verifier release directory");
    fs::write(root.join("Cargo.toml"), "[workspace]\n")
        .expect("write iOS verifier repository manifest");
    fs::create_dir_all(root.join("scripts")).expect("create iOS verifier policy directory");
    fs::copy(
        workspace_root().join("scripts/ffmpeg-source-policy.toml"),
        root.join("scripts/ffmpeg-source-policy.toml"),
    )
    .expect("copy iOS verifier FFmpeg source policy");
    write_core_ios_release_zip(
        &release.join("VesperPlayerKit-ios-arm64.framework.zip"),
        "VesperPlayerKit.framework",
    );
    write_core_ios_release_zip(
        &release.join("VesperPlayerKit-ios-simulator-arm64.framework.zip"),
        "VesperPlayerKit.framework",
    );
    write_core_ios_release_zip(
        &release.join("VesperPlayerKit.xcframework.zip"),
        "VesperPlayerKit.xcframework",
    );
    (directory, root, release)
}

#[cfg(target_os = "macos")]
struct IosNativeFrameFixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
    tools: PathBuf,
    source: PathBuf,
    actions: PathBuf,
    evidence: PathBuf,
}

#[cfg(target_os = "macos")]
fn ios_native_frame_fixture() -> IosNativeFrameFixture {
    let directory = tempfile::tempdir().expect("temporary iOS native-frame verifier fixture");
    let root = directory.path().join("repository");
    let tools = directory.path().join("tools");
    let source = root.join("native frame source.mp4");
    let actions = root.join("native-frame-actions.log");
    let evidence = root.join("native-frame-evidence.log");
    let ffmpeg_output = root.join("third_party/ffmpeg/apple");
    let ffmpeg_runtime = ffmpeg_output.join("ios-simulator/lib/arm64");
    fs::create_dir_all(root.join("scripts")).expect("create iOS native-frame scripts directory");
    fs::create_dir_all(root.join("lib/ios/VesperPlayerKit"))
        .expect("create iOS native-frame Xcode project directory");
    fs::create_dir_all(&ffmpeg_runtime).expect("create iOS native-frame FFmpeg runtime directory");
    fs::create_dir_all(&tools).expect("create iOS native-frame tools directory");
    fs::write(root.join("Cargo.toml"), "[workspace]\n")
        .expect("write iOS native-frame repository manifest");
    fs::write(
        root.join("lib/ios/VesperPlayerKit/project.yml"),
        "name: VesperPlayerKit\n",
    )
    .expect("write iOS native-frame XcodeGen manifest");
    fs::write(&source, b"native-frame fixture\n").expect("write iOS native-frame smoke source");
    fs::write(
        ffmpeg_runtime.join("libavutil.60.dylib"),
        b"fixture FFmpeg dylib\n",
    )
    .expect("write iOS native-frame FFmpeg runtime");

    write_executable_script(
        &root.join("scripts/vesper"),
        r#"#!/bin/sh
set -eu
[ "${1:-}" = ios ] || exit 73
case "${2:-}" in
  source-normalizer-plugin) dylib=libvesper_source_normalizer_ffmpeg.dylib ;;
  decoder-videotoolbox-plugin) dylib=libvesper_decoder_videotoolbox.dylib ;;
  frame-processor-plugin) dylib=libvesper_frame_processor_diagnostic.dylib ;;
  *) exit 74 ;;
esac
printf 'plugin command=%s root=%s profile=%s slice=%s staging=%s config=%s bash_env=%s env=%s cdpath=%s ffi_prebuilt=%s\n' \
  "$2" "$VESPER_REPO_ROOT" "${4:-}" "${5:-}" \
  "${VESPER_IOS_NATIVE_FRAME_STAGING_DIR:-unset}" \
  "${VESPER_IOS_NATIVE_FRAME_SMOKE_CONFIG:-unset}" \
  "${BASH_ENV:-unset}" "${ENV:-unset}" "${CDPATH:-unset}" \
  "${VESPER_IOS_FFI_PREBUILT:-unset}" \
  >> "$VESPER_TEST_NATIVE_FRAME_ACTIONS"
if [ -n "${VESPER_TEST_PLUGIN_CLI_STATUS:-}" ]; then
  printf 'fixture plugin CLI failure\n' >&2
  exit "$VESPER_TEST_PLUGIN_CLI_STATUS"
fi
/bin/mkdir -p "$3/iphonesimulator"
printf 'fixture plugin\n' > "$3/iphonesimulator/$dylib"
"#,
    );
    write_executable_script(
        &tools.join("install_name_tool"),
        "#!/bin/sh\nset -eu\nprintf 'install_name_tool %s\\n' \"$*\" >> \"$VESPER_TEST_NATIVE_FRAME_ACTIONS\"\nexit \"${VESPER_TEST_INSTALL_NAME_STATUS:-0}\"\n",
    );
    write_executable_script(
        &tools.join("otool"),
        r#"#!/bin/sh
set -eu
printf 'otool %s\n' "$*" >> "$VESPER_TEST_NATIVE_FRAME_ACTIONS"
case "${VESPER_TEST_OTOOL_RPATH:-exact}" in
  exact) printf 'path @loader_path (offset 12)\n' ;;
  parent) printf 'path @loader_path/.. (offset 12)\n' ;;
  none) printf 'cmd LC_ID_DYLIB\n' ;;
  *) exit 85 ;;
esac
"#,
    );
    write_executable_script(
        &tools.join("xcodegen"),
        r#"#!/bin/sh
set -eu
printf 'xcodegen %s bash_env=%s env=%s cdpath=%s ffi_prebuilt=%s\n' \
  "$*" "${BASH_ENV:-unset}" "${ENV:-unset}" "${CDPATH:-unset}" \
  "${VESPER_IOS_FFI_PREBUILT:-unset}" \
  >> "$VESPER_TEST_NATIVE_FRAME_ACTIONS"
/bin/mkdir -p VesperPlayerKit.xcodeproj
"#,
    );
    write_executable_script(
        &tools.join("xcodebuild"),
        r#"#!/bin/sh
set -eu
action="${1:-}"
shift || true
printf 'xcodebuild %s bash_env=%s env=%s cdpath=%s ffi_prebuilt=%s\n' \
  "$action" "${BASH_ENV:-unset}" "${ENV:-unset}" "${CDPATH:-unset}" \
  "${VESPER_IOS_FFI_PREBUILT:-unset}" \
  >> "$VESPER_TEST_NATIVE_FRAME_ACTIONS"
case "$action" in
  build-for-testing)
    derived_data=""
    while [ "$#" -gt 0 ]; do
      if [ "$1" = -derivedDataPath ]; then
        shift
        derived_data="${1:-}"
      fi
      shift || true
    done
    [ -n "$derived_data" ] || exit 75
    products="$derived_data/Build/Products"
    /bin/mkdir -p "$products"
    xctestrun="$products/VesperPlayerKit_fixture.xctestrun"
    /usr/bin/plutil -create xml1 "$xctestrun"
    /usr/libexec/PlistBuddy -c 'Add :VesperPlayerKitTests dict' "$xctestrun"
    /usr/libexec/PlistBuddy -c 'Add :VesperPlayerKitTests:EnvironmentVariables dict' "$xctestrun"
    extra="${VESPER_TEST_XCTESTRUN_EXTRA_ENTRIES:-0}"
    i=0
    while [ "$i" -lt "$extra" ]; do
      : > "$products/extra-$i"
      i=$((i + 1))
    done
    ;;
  test-without-building)
    xctestrun=""
    while [ "$#" -gt 0 ]; do
      if [ "$1" = -xctestrun ]; then
        shift
        xctestrun="${1:-}"
      fi
      shift || true
    done
    [ -f "$xctestrun" ] || exit 76
    config="$(/usr/bin/plutil -extract VesperPlayerKitTests.EnvironmentVariables.VESPER_IOS_NATIVE_FRAME_SMOKE_CONFIG raw -o - "$xctestrun")"
    mode="$(/usr/bin/plutil -extract VesperPlayerKitTests.EnvironmentVariables.VESPER_FRAME_PROCESSOR_DIAGNOSTIC_MODE raw -o - "$xctestrun")"
    [ "$mode" = noop ] || exit 77
    [ -f "$config" ] || exit 78
    source="$(/usr/bin/plutil -extract VESPER_IOS_NATIVE_FRAME_SMOKE_SOURCE raw -o - "$config")"
    source_plugin="$(/usr/bin/plutil -extract VESPER_IOS_SOURCE_NORMALIZER_PLUGIN_PATH raw -o - "$config")"
    decoder_plugin="$(/usr/bin/plutil -extract VESPER_IOS_DECODER_VIDEOTOOLBOX_PLUGIN_PATH raw -o - "$config")"
    frame_plugin="$(/usr/bin/plutil -extract VESPER_IOS_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH raw -o - "$config")"
    [ -f "$source" ] || exit 79
    [ -f "$source_plugin" ] || exit 80
    [ -f "$decoder_plugin" ] || exit 81
    [ -f "$frame_plugin" ] || exit 82
    printf 'config=%s\nsource=%s\nsource_plugin=%s\ndecoder_plugin=%s\nframe_plugin=%s\n' \
      "$config" "$source" "$source_plugin" "$decoder_plugin" "$frame_plugin" \
      > "$VESPER_TEST_NATIVE_FRAME_EVIDENCE"
    case "${VESPER_TEST_XCODE_MODE:-success}" in
      success)
        printf 'real iOS native-frame smoke presentedFrames=3 processedFrames=3 skippedAudioPackets=5\n'
        ;;
      success-with-diagnostic)
        printf 'real iOS native-frame smoke presentedFrames=3 processedFrames=3 skippedAudioPackets=5\n'
        printf 'fixture xcode diagnostic\n' >&2
        ;;
      duplicate-summary)
        printf 'real iOS native-frame smoke presentedFrames=3 processedFrames=3 skippedAudioPackets=5\n'
        printf 'real iOS native-frame smoke presentedFrames=4 processedFrames=4 skippedAudioPackets=5\n'
        ;;
      compatibility)
        printf 'Missing FFmpeg build input provenance under: fixture\n' >&2
        exit 4
        ;;
      conformance)
        printf 'partial native-frame result\n'
        printf 'native-frame conformance failure\n' >&2
        exit 1
        ;;
      arbitrary)
        printf 'fixture: command not found\n' >&2
        exit 1
        ;;
      no-summary)
        printf 'Xcode smoke finished without a playback summary\n'
        ;;
      release-failure)
        printf 'native-frame release failed\n'
        printf 'real iOS native-frame smoke presentedFrames=3 processedFrames=3 skippedAudioPackets=5\n'
        ;;
      signal)
        kill -KILL "$$"
        ;;
      wait)
        printf '%s' "$$" > "$VESPER_TEST_NATIVE_FRAME_WORKER_PID"
        /bin/sleep 30 &
        printf '%s' "$!" > "$VESPER_TEST_NATIVE_FRAME_DESCENDANT_PID"
        wait
        ;;
      *)
        exit 83
        ;;
    esac
    ;;
  *)
    exit 84
    ;;
esac
"#,
    );

    IosNativeFrameFixture {
        _directory: directory,
        root,
        tools,
        source,
        actions,
        evidence,
    }
}

#[cfg(target_os = "macos")]
fn ios_native_frame_command(fixture: &IosNativeFrameFixture) -> Command {
    let path = format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", fixture.tools.display());
    let mut command = Command::new(env!("CARGO_BIN_EXE_vesper"));
    command
        .env("PATH", path)
        .env("VESPER_IOS_NATIVE_FRAME_SMOKE_SOURCE", &fixture.source)
        .env(
            "VESPER_IOS_NATIVE_FRAME_DESTINATION",
            "platform=iOS Simulator,id=00000000-0000-0000-0000-000000000001",
        )
        .env("VESPER_TEST_NATIVE_FRAME_ACTIONS", &fixture.actions)
        .env("VESPER_TEST_NATIVE_FRAME_EVIDENCE", &fixture.evidence)
        .args(["ios", "--root"])
        .arg(&fixture.root)
        .arg("verify-native-frame");
    command
}

#[cfg(target_os = "macos")]
fn native_frame_evidence(path: &std::path::Path, key: &str) -> PathBuf {
    let contents = fs::read_to_string(path).expect("read iOS native-frame fixture evidence");
    PathBuf::from(
        contents
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{key}=")))
            .unwrap_or_else(|| panic!("missing iOS native-frame evidence key: {key}")),
    )
}

#[cfg(target_os = "macos")]
struct IosFfiFixture {
    directory: tempfile::TempDir,
    root: PathBuf,
    tools: PathBuf,
    log: PathBuf,
    target_libdir: PathBuf,
    artifact_root: PathBuf,
}

#[cfg(target_os = "macos")]
fn ios_ffi_fixture() -> IosFfiFixture {
    let directory = tempfile::tempdir().expect("temporary iOS FFI fixture");
    let root = directory.path().join("repository");
    let tools = directory.path().join("tools");
    let log = directory.path().join("tool.log");
    let target_libdir = directory.path().join("rust target libdir");
    let artifact_root = directory.path().join("Cargo artifacts");
    fs::create_dir_all(root.join("lib/ios/VesperPlayerKit/Artifacts"))
        .expect("create iOS FFI artifacts directory");
    fs::create_dir_all(
        root.join("lib/ios/VesperPlayerKit/Sources/VesperPlayerFFIResolver/include"),
    )
    .expect("create iOS FFI headers directory");
    fs::write(
        root.join(
            "lib/ios/VesperPlayerKit/Sources/VesperPlayerFFIResolver/include/player_ffi_resolver.h",
        ),
        b"/* fixture iOS FFI resolver header */\n",
    )
    .expect("write iOS FFI resolver header");
    fs::create_dir_all(&tools).expect("create iOS FFI fake tools directory");
    fs::create_dir_all(&target_libdir).expect("create fake Rust target libdir");
    fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("write iOS FFI fixture manifest");

    write_executable_script(
        &tools.join("rustc"),
        "#!/bin/sh\nif [ \"${1:-}\" = \"--print\" ]; then\n  printf '%s\\n' \"$VESPER_TEST_RUST_TARGET_LIBDIR\"\nfi\n",
    );
    write_executable_script(
        &tools.join("cargo"),
        "#!/bin/sh\nset -eu\nprintf 'cargo %s\\n' \"$*\" >> \"$VESPER_TEST_TOOL_LOG\"\n/bin/mkdir -p \"$VESPER_TEST_ARTIFACT_ROOT\"\nprintf 'fixture\\n' > \"$VESPER_TEST_ARTIFACT_ROOT/libvesper_player_ffi_ios.a\"\nprintf '%s\\n' '{\"reason\":\"compiler-artifact\",\"target\":{\"name\":\"vesper_player_ffi_ios\",\"kind\":[\"staticlib\"],\"crate_types\":[\"staticlib\"]},\"filenames\":[\"'\"$VESPER_TEST_ARTIFACT_ROOT/libvesper_player_ffi_ios.a\"'\"]}'\n",
    );
    write_executable_script(
        &tools.join("xcrun"),
        r#"#!/bin/sh
set -eu
printf 'xcrun %s\n' "$*" >> "$VESPER_TEST_TOOL_LOG"
if [ "${1:-}" = lipo ] && [ "${2:-}" = -archs ]; then
  printf '%s\n' "${VESPER_TEST_LIPO_ARCHS:-arm64}"
  exit 0
fi
if [ "${1:-}" = plutil ]; then
  shift
  exec /usr/bin/plutil "$@"
fi
if [ "${1:-}" = vtool ] && [ "${2:-}" = -show-build ]; then
  case "${3:-}" in
    */iphonesimulator/*) platform=IOSSIMULATOR ;;
    *) platform=IOS ;;
  esac
  printf 'Load command 9\n      cmd LC_BUILD_VERSION\n platform %s\n    minos 17.0\n' "$platform"
  exit 0
fi
exit 0
"#,
    );
    write_executable_script(
        &tools.join("xcodebuild"),
        r#"#!/bin/bash
set -eu
printf 'xcodebuild %s\n' "$*" >> "$VESPER_TEST_TOOL_LOG"
device_library=
simulator_library=
headers=
output=
while [ "$#" -gt 0 ]; do
  case "$1" in
    -library)
      if [ -z "$device_library" ]; then
        device_library=$2
      else
        simulator_library=$2
      fi
      shift 2
      ;;
    -headers) headers=$2; shift 2 ;;
    -output) output=$2; shift 2 ;;
    *) shift ;;
  esac
done
[ -n "$device_library" ]
[ -n "$simulator_library" ]
[ -n "$headers" ]
[ -n "$output" ]
/bin/mkdir -p "$output/ios-arm64/Headers" "$output/ios-arm64-simulator/Headers"
if [ "${VESPER_TEST_FFI_XCFRAMEWORK_MALFORMED:-}" = 1 ]; then
  printf 'not a plist\n' > "$output/Info.plist"
  exit 0
fi
/bin/cp "$device_library" "$output/ios-arm64/libvesper_player_ffi_ios.a"
/bin/cp "$simulator_library" "$output/ios-arm64-simulator/libvesper_player_ffi_ios.a"
if [ "${VESPER_TEST_FFI_XCFRAMEWORK_MISSING_HEADER:-}" != 1 ]; then
  /bin/cp -R "$headers/." "$output/ios-arm64/Headers"
  /bin/cp -R "$headers/." "$output/ios-arm64-simulator/Headers"
fi
if [ "${VESPER_TEST_FFI_XCFRAMEWORK_EXTRA_SLICE:-}" = 1 ]; then
  /bin/mkdir -p "$output/ios-arm64-extra"
fi
/bin/cat > "$output/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>AvailableLibraries</key><array>
<dict><key>BinaryPath</key><string>libvesper_player_ffi_ios.a</string><key>HeadersPath</key><string>Headers</string><key>LibraryIdentifier</key><string>ios-arm64</string><key>LibraryPath</key><string>libvesper_player_ffi_ios.a</string><key>SupportedArchitectures</key><array><string>arm64</string></array><key>SupportedPlatform</key><string>ios</string></dict>
<dict><key>BinaryPath</key><string>libvesper_player_ffi_ios.a</string><key>HeadersPath</key><string>Headers</string><key>LibraryIdentifier</key><string>ios-arm64-simulator</string><key>LibraryPath</key><string>libvesper_player_ffi_ios.a</string><key>SupportedArchitectures</key><array><string>arm64</string></array><key>SupportedPlatform</key><string>ios</string><key>SupportedPlatformVariant</key><string>simulator</string></dict>
</array>
<key>CFBundlePackageType</key><string>XFWK</string>
<key>XCFrameworkFormatVersion</key><string>1.0</string>
</dict></plist>
EOF
"#,
    );

    IosFfiFixture {
        directory,
        root,
        tools,
        log,
        target_libdir,
        artifact_root,
    }
}

#[cfg(target_os = "macos")]
fn run_ios_ffi_fixture(
    fixture: &IosFfiFixture,
    arguments: &[&str],
    extra_environment: &[(&str, &str)],
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_vesper"));
    command
        .current_dir(&fixture.directory)
        .env("PATH", &fixture.tools)
        .env("VESPER_TEST_TOOL_LOG", &fixture.log)
        .env("VESPER_TEST_RUST_TARGET_LIBDIR", &fixture.target_libdir)
        .env("VESPER_TEST_ARTIFACT_ROOT", &fixture.artifact_root)
        .args(["ios", "--root"])
        .arg(&fixture.root)
        .args(arguments);
    for (name, value) in extra_environment {
        command.env(name, value);
    }
    command.output().expect("run iOS FFI fixture command")
}

#[cfg(target_os = "macos")]
struct IosPluginFixture {
    directory: tempfile::TempDir,
    root: PathBuf,
    tools: PathBuf,
    log: PathBuf,
    target_libdir: PathBuf,
}

#[cfg(target_os = "macos")]
fn ios_plugin_fixture() -> IosPluginFixture {
    let directory = tempfile::tempdir().expect("temporary iOS plugin fixture");
    let root = directory.path().join("repository");
    let tools = directory.path().join("tools");
    let log = directory.path().join("tool.log");
    let target_libdir = directory.path().join("rust target libdir");
    fs::create_dir_all(&root).expect("create iOS plugin fixture repository");
    fs::create_dir_all(root.join("lib/ios/VesperPlayerKit"))
        .expect("create iOS plugin fixture project directory");
    fs::create_dir_all(&tools).expect("create iOS plugin fixture tools");
    fs::create_dir_all(&target_libdir).expect("create iOS plugin fixture target libdir");
    fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("write iOS plugin fixture manifest");
    let repository = workspace_root();
    for plugin in [
        "remux-ffmpeg",
        "source-normalizer-ffmpeg",
        "decoder-videotoolbox",
        "frame-processor-diagnostic",
    ] {
        let destination = root.join("plugins").join(plugin);
        fs::create_dir_all(&destination).expect("create plugin project fixture directory");
        fs::copy(
            repository
                .join("plugins")
                .join(plugin)
                .join("vesper-plugin.toml"),
            destination.join("vesper-plugin.toml"),
        )
        .expect("copy plugin project manifest into fixture");
    }
    fs::write(
        root.join("lib/ios/VesperPlayerKit/project.yml"),
        "settings:\n  base:\n    CFBundleShortVersionString: \"0.4.0\"\n    CFBundleVersion: \"4\"\n",
    )
    .expect("write iOS plugin fixture project metadata");
    write_executable_script(
        &tools.join("rustc"),
        "#!/bin/sh\nif [ \"${1:-}\" = \"--print\" ]; then\n  printf '%s\\n' \"$VESPER_TEST_RUST_TARGET_LIBDIR\"\nfi\n",
    );
    write_executable_script(
        &tools.join("cargo"),
        r#"#!/bin/sh
set -eu
printf 'cargo:%s\n' "$*" >> "$VESPER_TEST_TOOL_LOG"
printf 'ffmpeg_dir:%s\n' "${FFMPEG_DIR:-}" >> "$VESPER_TEST_TOOL_LOG"
printf 'cargo_target_dir:%s\n' "$CARGO_TARGET_DIR" >> "$VESPER_TEST_TOOL_LOG"
printf 'rustflags:%s\n' "${RUSTFLAGS:-}" >> "$VESPER_TEST_TOOL_LOG"
if [ "${VESPER_TEST_PLUGIN_CARGO_STATUS:-0}" != 0 ]; then
  exit "$VESPER_TEST_PLUGIN_CARGO_STATUS"
fi
if [ -n "${VESPER_TEST_PLUGIN_CARGO_READY:-}" ]; then
  printf '%s' "$$" > "$VESPER_TEST_PLUGIN_CARGO_READY"
  while [ ! -e "$VESPER_TEST_PLUGIN_CARGO_RELEASE" ]; do
    /bin/sleep 0.05
  done
fi
target=
profile=debug
while [ "$#" -gt 0 ]; do
  case "$1" in
    --target) target=$2; shift 2 ;;
    --release) profile=release; shift ;;
    *) shift ;;
  esac
done
[ -n "$target" ]
artifact="$CARGO_TARGET_DIR/$target/$profile/$VESPER_TEST_PLUGIN_DYLIB"
/bin/mkdir -p "$(/usr/bin/dirname "$artifact")"
printf '%s\n' "${VESPER_TEST_PLUGIN_PAYLOAD:-fixture iOS plugin}" > "$artifact"
"#,
    );
    write_executable_script(
        &tools.join("install_name_tool"),
        r#"#!/bin/sh
set -eu
printf 'install_name_tool:%s\n' "$*" >> "$VESPER_TEST_TOOL_LOG"
last=
for argument in "$@"; do last=$argument; done
if [ "$(/usr/bin/basename "$last")" = VesperPlayerFrameProcessorDiagnosticPlugin ] &&
   [ -n "${VESPER_TEST_PLUGIN_CONSUMER_READY:-}" ]; then
  printf '%s' "$$" > "$VESPER_TEST_PLUGIN_CONSUMER_READY"
  while [ ! -e "$VESPER_TEST_PLUGIN_CONSUMER_RELEASE" ]; do
    /bin/sleep 0.05
  done
fi
"#,
    );
    write_executable_script(
        &tools.join("otool"),
        r#"#!/bin/sh
set -eu
printf 'otool:%s\n' "$*" >> "$VESPER_TEST_TOOL_LOG"
case "${1:-}" in
  -l)
    printf '          path @loader_path/.. (offset 12)\n'
    ;;
  -L)
    printf '%s:\n' "$2"
    ;;
  -D)
    name=$(/usr/bin/basename "$2")
    printf '%s:\n@rpath/%s.framework/%s\n' "$2" "$name" "$name"
    ;;
esac
"#,
    );
    write_executable_script(
        &tools.join("lipo"),
        "#!/bin/sh\nprintf 'lipo:%s\\n' \"$*\" >> \"$VESPER_TEST_TOOL_LOG\"\nif [ \"${1:-}\" = -archs ]; then printf '%s\\n' \"${VESPER_TEST_PLUGIN_LIPO_ARCHS:-arm64}\"; fi\nexit \"${VESPER_TEST_PLUGIN_LIPO_STATUS:-0}\"\n",
    );
    write_executable_script(
        &tools.join("xcrun"),
        r#"#!/bin/sh
set -eu
printf 'xcrun:%s\n' "$*" >> "$VESPER_TEST_TOOL_LOG"
if [ "${1:-}" = plutil ]; then
  shift
  exec /usr/bin/plutil "$@"
fi
if [ "${1:-}" = --sdk ] && [ "${3:-}" = --show-sdk-path ]; then
  printf '%s\n' "$VESPER_TEST_APPLE_SDK_PATH"
  exit 0
fi
if [ "${1:-}" = --sdk ] && [ "${3:-}" = -f ] && [ "${4:-}" = clang ]; then
  printf '%s\n' "$VESPER_TEST_APPLE_CLANG_PATH"
  exit 0
fi
if [ "${1:-}" = lipo ] && [ "${2:-}" = -archs ]; then
  printf 'arm64\n'
  exit 0
fi
binary=
for argument do binary=$argument; done
if [ -f "$binary" ] && /usr/bin/grep -q 'ios-sim' "$binary"; then
  platform=IOSSIMULATOR
else
  case "$binary" in
    */iphonesimulator/*) platform=IOSSIMULATOR ;;
    *) platform=IOS ;;
  esac
fi
printf 'Load command 9\n      cmd LC_BUILD_VERSION\n platform %s\n    minos 17.0\n' "$platform"
"#,
    );
    write_executable_script(
        &tools.join("xcodebuild"),
        r#"#!/bin/bash
set -eu
printf 'xcodebuild:%s\n' "$*" >> "$VESPER_TEST_TOOL_LOG"
if [ "${1:-}" = -version ]; then
  printf 'Xcode 26.0\nBuild version 17A000\n'
  exit 0
fi
if [ "${1:-}" = -sdk ]; then
  case "${4:-}" in
    SDKVersion) printf '26.0\n' ;;
    ProductBuildVersion) printf '23A000\n' ;;
  esac
  exit 0
fi
if [ "${1:-}" != -create-xcframework ]; then
  exit 1
fi
if [ "${VESPER_TEST_PLUGIN_XCODEBUILD_STATUS:-0}" != 0 ]; then
  exit "$VESPER_TEST_PLUGIN_XCODEBUILD_STATUS"
fi
frameworks=()
output=
while [ "$#" -gt 0 ]; do
  case "$1" in
    -framework) frameworks+=("$2"); shift 2 ;;
    -output) output=$2; shift 2 ;;
    *) shift ;;
  esac
done
[ "${#frameworks[@]}" -gt 0 ]
[ -n "$output" ]
name=$(/usr/bin/basename "${frameworks[0]}" .framework)
entries=
separator=
for framework in "${frameworks[@]}"; do
  case "$framework" in
    */ios-simulator-arm64/*)
      identifier=ios-arm64-simulator
      variant=',"SupportedPlatformVariant":"simulator"'
      ;;
    *)
      identifier=ios-arm64
      variant=
      ;;
  esac
  /bin/mkdir -p "$output/$identifier"
  /bin/cp -R "$framework" "$output/$identifier/$name.framework"
  entry='{"BinaryPath":"'"$name"'.framework/'"$name"'","LibraryIdentifier":"'"$identifier"'","LibraryPath":"'"$name"'.framework","SupportedArchitectures":["arm64"],"SupportedPlatform":"ios"'"$variant"'}'
  entries="$entries$separator$entry"
  separator=,
done
printf '%s\n' '{"AvailableLibraries":['"$entries"'],"XCFrameworkFormatVersion":"1.0"}' > "$output/Info.plist"
"#,
    );

    IosPluginFixture {
        directory,
        root,
        tools,
        log,
        target_libdir,
    }
}

#[cfg(target_os = "macos")]
fn run_ios_plugin_fixture(
    fixture: &IosPluginFixture,
    arguments: &[&str],
    extra_environment: &[(&str, &str)],
) -> std::process::Output {
    run_ios_plugin_fixture_for(
        fixture,
        "frame-processor-plugin",
        "libvesper_frame_processor_diagnostic.dylib",
        arguments,
        extra_environment,
    )
}

#[cfg(target_os = "macos")]
fn run_ios_plugin_fixture_for(
    fixture: &IosPluginFixture,
    command_name: &str,
    dylib_name: &str,
    arguments: &[&str],
    extra_environment: &[(&str, &str)],
) -> std::process::Output {
    ios_plugin_fixture_command(
        fixture,
        command_name,
        dylib_name,
        arguments,
        extra_environment,
    )
    .output()
    .expect("run iOS plugin fixture command")
}

#[cfg(target_os = "macos")]
fn ios_plugin_fixture_command(
    fixture: &IosPluginFixture,
    command_name: &str,
    dylib_name: &str,
    arguments: &[&str],
    extra_environment: &[(&str, &str)],
) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_vesper"));
    let path = std::env::join_paths([
        fixture.tools.as_path(),
        std::path::Path::new("/usr/bin"),
        std::path::Path::new("/bin"),
    ])
    .expect("join iOS plugin fixture PATH");
    command
        .current_dir(&fixture.directory)
        .env("PATH", path)
        .env("VESPER_TEST_TOOL_LOG", &fixture.log)
        .env("VESPER_TEST_RUST_TARGET_LIBDIR", &fixture.target_libdir)
        .env("VESPER_TEST_PLUGIN_DYLIB", dylib_name)
        .args(["ios", "--root"])
        .arg(&fixture.root)
        .arg(command_name)
        .args(arguments);
    for name in [
        "VESPER_FFMPEG_PROFILE_CONFIG_PATH",
        "VESPER_FFMPEG_PROFILE",
        "VESPER_APPLE_FFMPEG_PROFILE",
        "VESPER_FFMPEG_TLS_BACKEND",
        "VESPER_APPLE_FFMPEG_TLS_BACKEND",
        "VESPER_FFMPEG_ENABLE_DASH",
        "VESPER_APPLE_FFMPEG_ENABLE_DASH",
        "VESPER_FFMPEG_FORCE",
        "VESPER_APPLE_FFMPEG_FORCE",
        "VESPER_FFMPEG_ACKNOWLEDGE_GPL_NONFREE",
        "VESPER_APPLE_FFMPEG_ACKNOWLEDGE_GPL_NONFREE",
        "VESPER_FFMPEG_ENABLE_LIBRARIES",
        "VESPER_APPLE_FFMPEG_ENABLE_LIBRARIES",
        "VESPER_FFMPEG_ENABLE_DEMUXERS",
        "VESPER_APPLE_FFMPEG_ENABLE_DEMUXERS",
        "VESPER_FFMPEG_ENABLE_MUXERS",
        "VESPER_APPLE_FFMPEG_ENABLE_MUXERS",
        "VESPER_FFMPEG_ENABLE_PROTOCOLS",
        "VESPER_APPLE_FFMPEG_ENABLE_PROTOCOLS",
        "VESPER_FFMPEG_ENABLE_DECODERS",
        "VESPER_APPLE_FFMPEG_ENABLE_DECODERS",
        "VESPER_FFMPEG_ENABLE_PARSERS",
        "VESPER_APPLE_FFMPEG_ENABLE_PARSERS",
        "VESPER_FFMPEG_ENABLE_BSFS",
        "VESPER_APPLE_FFMPEG_ENABLE_BSFS",
        "VESPER_FFMPEG_EXTRA_CONFIGURE_ARGS",
        "VESPER_APPLE_FFMPEG_EXTRA_CONFIGURE_ARGS",
        "VESPER_FFMPEG_OUTPUT_DIR",
        "VESPER_APPLE_FFMPEG_OUTPUT_DIR",
        "VESPER_IOS_FFMPEG_RUNTIME_USE_EXISTING_INPUTS",
        "VESPER_IOS_FFMPEG_RUNTIME_PREPARE_ONLY",
        "VESPER_IOS_FFMPEG_INPUT_FINGERPRINT_IOS_ARM64",
        "VESPER_IOS_FFMPEG_INPUT_FINGERPRINT_IOS_SIMULATOR_ARM64",
    ] {
        command.env_remove(name);
    }
    for (name, value) in extra_environment {
        command.env(name, value);
    }
    command
}

#[cfg(target_os = "macos")]
fn frame_processor_release_xcframework(fixture: &IosPluginFixture) -> PathBuf {
    fixture.root.join(
        "lib/ios/VesperPlayerKit/.build/player-frame-processor-diagnostic-plugin/VesperPlayerFrameProcessorDiagnosticPlugin.xcframework",
    )
}

#[cfg(target_os = "macos")]
fn frame_processor_release_binary(fixture: &IosPluginFixture) -> PathBuf {
    frame_processor_release_xcframework(fixture).join(
        "ios-arm64/VesperPlayerFrameProcessorDiagnosticPlugin.framework/VesperPlayerFrameProcessorDiagnosticPlugin",
    )
}

#[cfg(target_os = "macos")]
fn remux_release_xcframework(fixture: &IosPluginFixture) -> PathBuf {
    fixture.root.join(
        "lib/ios/VesperPlayerKit/.build/player-remux-ffmpeg-plugin/\
         VesperPlayerRemuxFfmpegPlugin.xcframework",
    )
}

#[cfg(target_os = "macos")]
fn assert_no_remux_plugin_release_outputs(
    fixture: &IosPluginFixture,
    release_output: &std::path::Path,
) {
    assert!(
        !remux_release_xcframework(fixture).exists(),
        "failed release published the canonical remux plugin XCFramework"
    );
    assert!(
        !release_output
            .join("VesperPlayerRemuxFfmpegPlugin.xcframework.zip")
            .exists(),
        "failed release published the remux plugin ZIP"
    );
}

#[cfg(target_os = "macos")]
fn ios_plugin_release_journal(fixture: &IosPluginFixture) -> PathBuf {
    fixture
        .root
        .join("lib/ios/VesperPlayerKit/.build/vesper-cli-state/ios-plugin-release-transaction.json")
}

#[cfg(target_os = "macos")]
fn ios_plugin_prepared_owner_journal(fixture: &IosPluginFixture) -> PathBuf {
    fixture
        .root
        .join("lib/ios/VesperPlayerKit/.build/vesper-cli-state/ios-plugin-release-prepared.json")
}

#[cfg(target_os = "macos")]
fn wait_for_fixture_marker(path: &std::path::Path) -> bool {
    use std::time::{Duration, Instant};

    let deadline = Instant::now() + Duration::from_secs(30);
    while !path.is_file() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    path.is_file()
}

#[cfg(target_os = "macos")]
fn appended_path_suffix(path: &std::path::Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .expect("path with a file name")
        .to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

#[cfg(target_os = "macos")]
fn ios_plugin_release_staging_entries(
    fixture: &IosPluginFixture,
    release_output: &std::path::Path,
) -> Vec<PathBuf> {
    let mut retained = Vec::new();
    for (directory, prefix) in [
        (
            fixture.root.join("lib/ios/VesperPlayerKit/.build"),
            ".vesper-ios-plugin-release-",
        ),
        (release_output.to_path_buf(), ".vesper-ios-plugin-assets-"),
    ] {
        retained.extend(
            fs::read_dir(&directory)
                .expect("inspect iOS plugin release staging parent")
                .map(|entry| entry.expect("read iOS plugin release staging entry").path())
                .filter(|path| {
                    path.file_name()
                        .is_some_and(|name| name.to_string_lossy().starts_with(prefix))
                }),
        );
    }
    retained
}

#[cfg(target_os = "macos")]
fn assert_no_ios_plugin_release_staging(
    fixture: &IosPluginFixture,
    release_output: &std::path::Path,
) {
    let prepared_journal = ios_plugin_prepared_owner_journal(fixture);
    assert!(
        !prepared_journal.exists(),
        "prepared-owner journal remained after iOS plugin release cleanup"
    );
    assert!(
        !appended_path_suffix(&prepared_journal, ".removing").exists(),
        "prepared-owner journal removal quarantine remained after iOS plugin release cleanup"
    );
    let retained = ios_plugin_release_staging_entries(fixture, release_output);
    assert!(
        retained.is_empty(),
        "retained iOS plugin release staging entries: {retained:?}"
    );
}

#[cfg(target_os = "macos")]
fn stage_frame_processor_release(
    fixture: &IosPluginFixture,
    release_output: &std::path::Path,
    environment: &[(&str, &str)],
) -> std::process::Output {
    run_ios_plugin_fixture_for(
        fixture,
        "stage-frame-processor-plugin-release",
        "libvesper_frame_processor_diagnostic.dylib",
        &[
            release_output
                .to_str()
                .expect("UTF-8 frame processor release output"),
            "ios-arm64",
            "ios-simulator-arm64",
        ],
        environment,
    )
}

#[cfg(target_os = "macos")]
fn ios_ffmpeg_remux_release_command(
    fixture: &IosPluginFixture,
    release_output: &std::path::Path,
    environment: &[(&str, &str)],
) -> Command {
    ios_plugin_fixture_command(
        fixture,
        "stage-remux-plugin-release",
        "libvesper_remux_ffmpeg.dylib",
        &[
            release_output
                .to_str()
                .expect("UTF-8 FFmpeg plugin release output"),
            "--profile",
            "default",
            "ios-arm64",
            "ios-simulator-arm64",
        ],
        environment,
    )
}

#[cfg(target_os = "macos")]
fn seed_owned_frame_processor_output(output: &std::path::Path) {
    let device = output.join("iphoneos");
    fs::create_dir_all(&device).expect("create owned iOS plugin output");
    fs::write(
        device.join("libvesper_frame_processor_diagnostic.dylib"),
        b"previous plugin\n",
    )
    .expect("write previous iOS plugin library");
    fs::write(
        output.join(".vesper-ios-plugin-output"),
        concat!(
            "{\"format\":\"vesper-ios-plugin-output\",",
            "\"plugin_id\":\"frame-processor-diagnostic\",",
            "\"cargo_profile\":\"debug\",",
            "\"slices\":[\"ios-arm64\"],",
            "\"ffmpeg_profile\":null,",
            "\"ffmpeg_inputs\":[]}\n"
        ),
    )
    .expect("write iOS plugin output ownership marker");
}

#[cfg(target_os = "macos")]
fn configure_ios_plugin_ffmpeg_fixture(fixture: &IosPluginFixture) {
    use sha2::{Digest, Sha256};

    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    for relative in [
        "scripts/ffmpeg-source-policy.toml",
        "scripts/ffmpeg-profiles.toml",
    ] {
        let destination = fixture.root.join(relative);
        fs::create_dir_all(destination.parent().expect("FFmpeg fixture file parent"))
            .expect("create FFmpeg fixture directory");
        fs::copy(repository.join(relative), &destination)
            .unwrap_or_else(|error| panic!("copy {relative} into FFmpeg fixture: {error}"));
    }
    let python3 = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|directory| directory.join("python3"))
        .find(|candidate| fs::metadata(candidate).is_ok_and(|metadata| metadata.is_file()))
        .expect("find fixture Python 3.11 or newer");
    std::os::unix::fs::symlink(&python3, fixture.tools.join("python3"))
        .expect("link fixture Python 3.11 or newer");
    write_executable_script(
        &fixture.tools.join("shasum"),
        "#!/bin/sh\nexec /usr/bin/shasum \"$@\"\n",
    );
    for (base, metadata_profile, metadata_declared_profile, metadata_profile_hash) in [
        (
            fixture.root.join("third_party/ffmpeg/apple"),
            "legacy",
            "legacy",
            "legacy",
        ),
        (
            fixture
                .root
                .join("third_party/ffmpeg/apple/profiles/custom-e1eabc3db7bc"),
            "custom",
            "default",
            "custom-e1eabc3db7bc",
        ),
    ] {
        for platform in ["ios", "ios-simulator"] {
            let directory = base.join(platform);
            fs::create_dir_all(&directory).expect("create FFmpeg fixture slice directory");
            let target = if platform == "ios" {
                "ios-arm64"
            } else {
                "ios-simulator-arm64"
            };
            fs::write(
                directory.join("vesper-ffmpeg-build-metadata.txt"),
                format!(
                    concat!(
                        "Vesper FFmpeg build metadata v2\n",
                        "platform=apple\n",
                        "target={}\n",
                        "profile={}\n",
                        "declared_profile={}\n",
                        "declared_platform=ios\n",
                        "profile_hash={}\n",
                        "tls_backend=none\n",
                        "enable_dash=0\n",
                        "libraries=avcodec,avformat,avutil\n",
                        "demuxers=\n",
                        "muxers=\n",
                        "protocols=file\n",
                        "decoders=\n",
                        "parsers=\n",
                        "bsfs=\n",
                        "external_dependencies=none\n",
                        "license_flags=\n",
                        "ffmpeg_version=8.1.2\n",
                        "source_archive=fixture.tar.xz\n",
                        "source_url=https://ffmpeg.org/releases/ffmpeg-8.1.2.tar.xz\n",
                        "source_sha256=464beb5e7bf0c311e68b45ae2f04e9cc2af88851abb4082231742a74d97b524c\n",
                        "configure_line=--enable-shared\n"
                    ),
                    target,
                    metadata_profile,
                    metadata_declared_profile,
                    metadata_profile_hash,
                ),
            )
            .expect("write FFmpeg fixture metadata");
            let library_directory = directory.join("lib/arm64");
            fs::create_dir_all(&library_directory)
                .expect("create FFmpeg fixture library directory");
            fs::create_dir_all(directory.join("include/libavutil"))
                .expect("create FFmpeg fixture include directory");
            fs::write(
                directory.join("include/libavutil/avutil.h"),
                format!("fixture header for {platform}\n"),
            )
            .expect("write FFmpeg fixture header");
            let mut checksum_records = String::new();
            for library in ["avcodec", "avformat", "avutil"] {
                let versioned = format!("lib{library}.1.dylib");
                let payload = format!("fixture {platform} {library}\n");
                fs::write(library_directory.join(&versioned), payload.as_bytes())
                    .expect("write FFmpeg fixture library");
                std::os::unix::fs::symlink(
                    &versioned,
                    library_directory.join(format!("lib{library}.dylib")),
                )
                .expect("create FFmpeg fixture library alias");
                checksum_records.push_str(&format!(
                    "{library}_sha256={}\n",
                    hex::encode(Sha256::digest(payload.as_bytes()))
                ));
            }
            fs::write(
                directory.join("vesper-ffmpeg-library-sha256.txt"),
                checksum_records,
            )
            .expect("write FFmpeg fixture checksums");
        }
    }
}

#[cfg(target_os = "macos")]
fn configure_ios_plugin_native_ffmpeg_build_fixture(fixture: &IosPluginFixture) {
    use sha2::{Digest, Sha256};

    let version = "8.1.2";
    let cache = fixture.root.join("third_party/_cache");
    fs::create_dir_all(&cache).expect("create Apple FFmpeg source cache");
    let archive_path = cache.join(format!("ffmpeg-{version}.tar.xz"));
    let archive_file = fs::File::create(&archive_path).expect("create Apple FFmpeg source archive");
    let encoder = xz2::write::XzEncoder::new(archive_file, 6);
    let mut archive = tar::Builder::new(encoder);
    let configure = br#"#!/bin/sh
set -eu
prefix=
printf 'apple_ffmpeg_configure_begin\n' >> "$VESPER_TEST_TOOL_LOG"
for argument in "$@"; do
  printf 'apple_ffmpeg_configure_arg:%s\n' "$argument" >> "$VESPER_TEST_TOOL_LOG"
  case "$argument" in --prefix=*) prefix=${argument#--prefix=} ;; esac
done
[ -n "$prefix" ]
printf '%s\n' "$prefix" > .vesper-prefix
"#;
    let mut header = tar::Header::new_gnu();
    header.set_size(configure.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    archive
        .append_data(
            &mut header,
            format!("ffmpeg-{version}/configure"),
            &configure[..],
        )
        .expect("append Apple FFmpeg configure fixture");
    let encoder = archive.into_inner().expect("finish Apple FFmpeg tar");
    encoder.finish().expect("finish Apple FFmpeg xz stream");
    let source_sha256 = hex::encode(Sha256::digest(
        fs::read(&archive_path).expect("read Apple FFmpeg source archive"),
    ));
    fs::write(
        fixture.root.join("scripts/ffmpeg-source-policy.toml"),
        format!(
            "schema_version = 1\n\n[compatibility]\nrequirement = \">=8.1.0, <8.2.0\"\ndefault_series = \"8.1\"\n\n[release]\nversion = \"{version}\"\nsource_url = \"https://ffmpeg.org/releases/ffmpeg-{version}.tar.xz\"\nsource_sha256 = \"{source_sha256}\"\n"
        ),
    )
    .expect("write Apple FFmpeg source policy fixture");

    fs::create_dir_all(fixture.root.join("fake Apple SDK")).expect("create fake Apple SDK");
    write_executable_script(&fixture.tools.join("clang"), "#!/bin/sh\nexit 0\n");
    write_executable_script(
        &fixture.tools.join("make"),
        r#"#!/bin/sh
set -eu
printf 'apple_ffmpeg_make:%s\n' "$*" >> "$VESPER_TEST_TOOL_LOG"
install=0
destdir=
for argument in "$@"; do
  case "$argument" in
    install) install=1 ;;
    DESTDIR=*) destdir=${argument#DESTDIR=} ;;
  esac
done
if [ "$install" = 1 ]; then
  [ -n "$destdir" ]
  prefix=$(/bin/cat .vesper-prefix)
  destination="$destdir/${prefix#/}"
  /bin/mkdir -p "$destination/include" "$destination/lib"
  for library in avcodec avformat avutil; do
    /bin/mkdir -p "$destination/include/lib$library"
    printf 'fixture %s header\n' "$library" > "$destination/include/lib$library/$library.h"
    printf 'fixture %s static library\n' "$library" > "$destination/lib/lib$library.a"
    printf 'fixture %s shared library\n' "$library" > "$destination/lib/lib$library.dylib"
  done
fi
"#,
    );
}

#[cfg(target_os = "macos")]
fn rewrite_ios_ffmpeg_fixture_profile_hash(
    fixture: &IosPluginFixture,
    relative_base: &str,
    profile_hash: &str,
) {
    for platform in ["ios", "ios-simulator"] {
        let metadata = fixture
            .root
            .join(relative_base)
            .join(platform)
            .join("vesper-ffmpeg-build-metadata.txt");
        let text = fs::read_to_string(&metadata).expect("read FFmpeg fixture metadata");
        let old_line = text
            .lines()
            .find(|line| line.starts_with("profile_hash="))
            .expect("find FFmpeg fixture profile hash");
        let updated = text.replace(old_line, &format!("profile_hash={profile_hash}"));
        fs::write(metadata, updated).expect("rewrite FFmpeg fixture profile hash");
    }
}

#[cfg(target_os = "macos")]
fn seed_ios_ffmpeg_runtime_profiles(
    fixture: &IosPluginFixture,
    output_directory: &std::path::Path,
    slices: &[&str],
) {
    seed_ios_ffmpeg_runtime_profiles_root(&fixture.root, output_directory, slices);
}

#[cfg(target_os = "macos")]
fn seed_ios_ffmpeg_runtime_profiles_root(
    root_path: &std::path::Path,
    output_directory: &std::path::Path,
    slices: &[&str],
) {
    use sha2::{Digest, Sha256};
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    fs::create_dir_all(output_directory).expect("create FFmpeg runtime release fixture");
    let directory_options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o040755);
    let file_options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o100644);
    let executable_options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o100755);
    for framework in [
        "VesperFFmpegAVCodec",
        "VesperFFmpegAVFormat",
        "VesperFFmpegAVUtil",
    ] {
        let archive_path = output_directory.join(format!("{framework}.xcframework.zip"));
        let archive = fs::File::create(&archive_path).expect("create FFmpeg runtime ZIP fixture");
        let mut archive = ZipWriter::new(archive);
        let root = format!("{framework}.xcframework");
        archive
            .add_directory(format!("{root}/"), directory_options)
            .expect("write runtime XCFramework root");
        archive
            .start_file(format!("{root}/Info.plist"), file_options)
            .expect("write runtime XCFramework manifest");
        archive
            .write_all(b"fixture XCFramework plist\n")
            .expect("write runtime XCFramework manifest payload");
        for slice in slices {
            let (identifier, metadata_name, source_directory) = match *slice {
                "ios-arm64" => (
                    "ios-arm64",
                    "ios-arm64-vesper-ffmpeg-build-metadata.txt",
                    "ios",
                ),
                "ios-simulator-arm64" => (
                    "ios-arm64-simulator",
                    "ios-simulator-arm64-vesper-ffmpeg-build-metadata.txt",
                    "ios-simulator",
                ),
                value => panic!("unsupported runtime fixture slice: {value}"),
            };
            let source = root_path
                .join("third_party/ffmpeg/apple/profiles/custom-e1eabc3db7bc")
                .join(source_directory);
            let metadata = fs::read(source.join("vesper-ffmpeg-build-metadata.txt"))
                .expect("read FFmpeg fixture metadata");
            let fingerprint = format!("{}\n", ios_ffmpeg_input_fingerprint(root_path, slice),);
            let slice_root = format!("{root}/{identifier}");
            let framework_root = format!("{slice_root}/{framework}.framework");
            for directory in [
                format!("{slice_root}/"),
                format!("{framework_root}/"),
                format!("{framework_root}/Headers/"),
                format!("{framework_root}/Modules/"),
            ] {
                archive
                    .add_directory(directory, directory_options)
                    .expect("write runtime framework directory");
            }
            for (path, payload, options) in [
                (
                    format!("{framework_root}/Headers/{framework}.h"),
                    b"fixture header\n".as_slice(),
                    file_options,
                ),
                (
                    format!("{framework_root}/Info.plist"),
                    b"fixture framework plist\n".as_slice(),
                    file_options,
                ),
                (
                    format!("{framework_root}/Modules/module.modulemap"),
                    b"fixture module map\n".as_slice(),
                    file_options,
                ),
            ] {
                archive
                    .start_file(path, options)
                    .expect("write runtime framework file");
                archive
                    .write_all(payload)
                    .expect("write runtime framework file payload");
            }
            let binary = format!("fixture {framework} {slice}\n").into_bytes();
            archive
                .start_file(format!("{framework_root}/{framework}"), executable_options)
                .expect("write runtime framework binary");
            archive
                .write_all(&binary)
                .expect("write runtime framework binary payload");
            for (name, payload) in [
                (
                    "binary-sha256.txt",
                    format!("{}\n", hex::encode(Sha256::digest(&binary))).into_bytes(),
                ),
                ("input-fingerprint.txt", fingerprint.clone().into_bytes()),
                (metadata_name, metadata.clone()),
                ("profile-hash.txt", b"custom-e1eabc3db7bc\n".to_vec()),
            ] {
                archive
                    .start_file(format!("{framework_root}/{name}"), file_options)
                    .expect("write runtime provenance record");
                archive
                    .write_all(&payload)
                    .expect("write runtime provenance payload");
            }
        }
        archive.finish().expect("finish FFmpeg runtime ZIP fixture");
    }
}

#[cfg(target_os = "macos")]
fn rewrite_ios_ffmpeg_root_declared_profile(root: &std::path::Path, declared_profile: &str) {
    for platform in ["ios", "ios-simulator"] {
        let metadata = root
            .join("third_party/ffmpeg/apple/profiles/custom-e1eabc3db7bc")
            .join(platform)
            .join("vesper-ffmpeg-build-metadata.txt");
        let text = fs::read_to_string(&metadata).expect("read iOS release FFmpeg metadata");
        let old_line = text
            .lines()
            .find(|line| line.starts_with("declared_profile="))
            .expect("find iOS release FFmpeg declared profile");
        fs::write(
            metadata,
            text.replace(old_line, &format!("declared_profile={declared_profile}")),
        )
        .expect("rewrite iOS release FFmpeg declared profile");
    }
}

#[cfg(target_os = "macos")]
fn configure_ios_release_source_fixture(root: &std::path::Path) {
    use sha2::{Digest, Sha256};

    let version = "8.1.2";
    let relative_archive = format!("third_party/_cache/ffmpeg-{version}.tar.xz");
    let archive_path = root.join(&relative_archive);
    fs::create_dir_all(archive_path.parent().expect("source archive parent"))
        .expect("create iOS release source cache");
    let archive_file = fs::File::create(&archive_path).expect("create iOS release source archive");
    let encoder = xz2::write::XzEncoder::new(archive_file, 6);
    let mut archive = tar::Builder::new(encoder);
    for (name, contents) in [
        (
            "COPYING.LGPLv2.1",
            b"GNU LESSER GENERAL PUBLIC LICENSE Version 2.1 fixture\n".as_slice(),
        ),
        (
            "COPYING.LGPLv3",
            b"GNU LESSER GENERAL PUBLIC LICENSE Version 3 fixture\n".as_slice(),
        ),
        (
            "LICENSE.md",
            b"FFmpeg fixture license summary for LGPL shared builds.\n".as_slice(),
        ),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, format!("ffmpeg-{version}/{name}"), contents)
            .expect("append iOS release source license");
    }
    let encoder = archive.into_inner().expect("finish iOS release source tar");
    encoder
        .finish()
        .expect("finish iOS release source xz stream");
    let source_sha256 = hex::encode(Sha256::digest(
        fs::read(&archive_path).expect("read iOS release source archive"),
    ));
    fs::write(
        root.join("scripts/ffmpeg-source-policy.toml"),
        format!(
            "schema_version = 1\n\n[compatibility]\nrequirement = \">=8.1.0, <8.2.0\"\ndefault_series = \"8.1\"\n\n[release]\nversion = \"{version}\"\nsource_url = \"https://ffmpeg.org/releases/ffmpeg-{version}.tar.xz\"\nsource_sha256 = \"{source_sha256}\"\n"
        ),
    )
    .expect("write iOS release source policy");
    fs::write(
        root.join("THIRD_PARTY_NOTICES.md"),
        "# Third-party notices\n\nFFmpeg is distributed under LGPL-2.1-or-later.\n",
    )
    .expect("write iOS release third-party notices");

    for platform in ["ios", "ios-simulator"] {
        let metadata = root
            .join("third_party/ffmpeg/apple/profiles/custom-e1eabc3db7bc")
            .join(platform)
            .join("vesper-ffmpeg-build-metadata.txt");
        let mut text = fs::read_to_string(&metadata).expect("read iOS release source metadata");
        for (key, value) in [
            ("source_archive", relative_archive.as_str()),
            ("source_sha256", source_sha256.as_str()),
        ] {
            let old_line = text
                .lines()
                .find(|line| line.starts_with(&format!("{key}=")))
                .expect("find iOS release source metadata field")
                .to_owned();
            text = text.replace(&old_line, &format!("{key}={value}"));
        }
        fs::write(metadata, text).expect("rewrite iOS release source metadata");
    }
}

#[cfg(target_os = "macos")]
fn ffmpeg_fixture_content_fingerprint(root: &std::path::Path) -> String {
    use sha2::{Digest, Sha256};
    use std::collections::VecDeque;
    use std::os::unix::fs::MetadataExt;

    let root_metadata = fs::symlink_metadata(root).expect("inspect FFmpeg fixture root");
    let mut digest = Sha256::new();
    digest.update(b"vesper-ios-plugin-ffmpeg-build-content-v1\0");
    digest.update(root_metadata.mode().to_le_bytes());
    let mut pending = VecDeque::from([(root.to_path_buf(), PathBuf::new())]);
    while let Some((directory, relative_directory)) = pending.pop_front() {
        let mut children = fs::read_dir(&directory)
            .expect("scan FFmpeg fixture content tree")
            .map(|entry| entry.expect("read FFmpeg fixture content entry"))
            .collect::<Vec<_>>();
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            let relative = relative_directory.join(child.file_name());
            let metadata =
                fs::symlink_metadata(child.path()).expect("inspect FFmpeg fixture content entry");
            let effective = if metadata.file_type().is_symlink() {
                fs::metadata(child.path()).expect("resolve FFmpeg fixture file alias")
            } else {
                metadata
            };
            let bytes = relative.as_os_str().as_encoded_bytes();
            digest.update((bytes.len() as u64).to_le_bytes());
            digest.update(bytes);
            digest.update(effective.mode().to_le_bytes());
            if effective.file_type().is_dir() {
                digest.update(b"D");
                pending.push_back((child.path(), relative));
            } else {
                assert!(effective.file_type().is_file());
                digest.update(b"F");
                digest.update(effective.len().to_le_bytes());
                digest.update(fs::read(child.path()).expect("read FFmpeg fixture file"));
            }
        }
    }
    hex::encode(digest.finalize())
}

#[cfg(target_os = "macos")]
fn ios_ffmpeg_input_fingerprint(root: &std::path::Path, slice: &str) -> String {
    use sha2::{Digest, Sha256};

    let platform = match slice {
        "ios-arm64" => "ios",
        "ios-simulator-arm64" => "ios-simulator",
        value => panic!("unsupported FFmpeg fingerprint slice: {value}"),
    };
    let source = root
        .join("third_party/ffmpeg/apple/profiles/custom-e1eabc3db7bc")
        .join(platform);
    let metadata = fs::read(source.join("vesper-ffmpeg-build-metadata.txt"))
        .expect("read FFmpeg fingerprint metadata");
    format!(
        "{}-{}",
        hex::encode(Sha256::digest(&metadata)),
        ffmpeg_fixture_content_fingerprint(&source)
    )
}

#[cfg(target_os = "macos")]
fn apply_ios_ffmpeg_runtime_reuse_environment(command: &mut Command, root: &std::path::Path) {
    command
        .env("VESPER_IOS_FFMPEG_RUNTIME_USE_EXISTING_INPUTS", "1")
        .env(
            "VESPER_IOS_FFMPEG_INPUT_FINGERPRINT_IOS_ARM64",
            ios_ffmpeg_input_fingerprint(root, "ios-arm64"),
        )
        .env(
            "VESPER_IOS_FFMPEG_INPUT_FINGERPRINT_IOS_SIMULATOR_ARM64",
            ios_ffmpeg_input_fingerprint(root, "ios-simulator-arm64"),
        );
}

#[cfg(target_os = "macos")]
fn ios_ffmpeg_runtime_fixture_command(
    fixture: &IosPluginFixture,
    output: &std::path::Path,
) -> Command {
    ios_plugin_fixture_command(
        fixture,
        "ffmpeg-runtime-release",
        "unused-runtime-fixture.dylib",
        &[
            output.to_str().expect("UTF-8 FFmpeg runtime output"),
            "--profile",
            "default",
            "ios-arm64",
            "ios-simulator-arm64",
        ],
        &[],
    )
}

#[cfg(target_os = "macos")]
fn read_zip_fixture_entry(archive: &std::path::Path, name: &str) -> Vec<u8> {
    use std::io::Read;

    let file = fs::File::open(archive).expect("open fixture ZIP archive");
    let mut archive = zip::ZipArchive::new(file).expect("parse fixture ZIP archive");
    let mut entry = archive.by_name(name).expect("find fixture ZIP entry");
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .expect("read fixture ZIP entry");
    bytes
}

#[cfg(target_os = "macos")]
fn configure_ios_kit_fixture(fixture: &IosFfiFixture) {
    configure_ios_kit_root(&fixture.root, &fixture.tools);
}

#[cfg(target_os = "macos")]
fn configure_ios_kit_root(root: &std::path::Path, tools: &std::path::Path) {
    let project = root.join("lib/ios/VesperPlayerKit");
    fs::write(
        project.join("project.yml"),
        concat!(
            "name: VesperPlayerKit\n",
            "settings:\n",
            "  base:\n",
            "    CFBundleShortVersionString: \"0.4.0\"\n",
            "    CFBundleVersion: \"400\"\n",
        ),
    )
    .expect("write iOS kit XcodeGen manifest");
    write_executable_script(
        &tools.join("xcodegen"),
        r#"#!/bin/sh
set -eu
printf 'xcodegen:%s\n' "$*" >> "$VESPER_TEST_TOOL_LOG"
if [ -n "${VESPER_IOS_RELEASE_BUILD_STATUS:-}" ]; then
  exit "$VESPER_IOS_RELEASE_BUILD_STATUS"
fi
if [ -n "${VESPER_IOS_RELEASE_BUILD_PID_FILE:-}" ]; then
  printf '%s' "$$" > "$VESPER_IOS_RELEASE_BUILD_PID_FILE"
  /bin/sh -c 'trap "" INT TERM; printf "%s" "$$" > "$1"; while :; do /bin/sleep 1; done' sh "$VESPER_IOS_RELEASE_DESCENDANT_PID_FILE" &
  trap '' INT TERM
  while :; do /bin/sleep 1; done
fi
if [ -n "${VESPER_IOS_RELEASE_LOCK_READY:-}" ]; then
  : > "$VESPER_IOS_RELEASE_LOCK_READY"
  while [ ! -f "$VESPER_IOS_RELEASE_LOCK_GATE" ]; do /bin/sleep 0.05; done
fi
if [ -n "${VESPER_IOS_IDENTITY_READY:-}" ]; then
  : > "$VESPER_IOS_IDENTITY_READY"
  while [ ! -f "$VESPER_IOS_IDENTITY_GATE" ]; do /bin/sleep 0.05; done
fi
/bin/mkdir -p "$PWD/VesperPlayerKit.xcodeproj"
"#,
    );
    write_executable_script(
        &tools.join("xcodebuild"),
        r#"#!/bin/bash
set -eu
printf 'xcodebuild:%s\n' "$*" >> "$VESPER_TEST_TOOL_LOG"
if [ "${1:-}" = -version ]; then
  printf 'Xcode 26.0\nBuild version 17A000\n'
  exit 0
fi
if [ "${1:-}" = -sdk ]; then
  case "${4:-}" in
    SDKVersion) printf '26.0\n' ;;
    ProductBuildVersion) printf '23A000\n' ;;
  esac
  exit 0
fi
if [ "${1:-}" = archive ]; then
  sdk=
  archive=
  ffi_prebuilt=0
  while [ "$#" -gt 0 ]; do
    case "$1" in
      -sdk) sdk=$2; shift 2 ;;
      -archivePath) archive=$2; shift 2 ;;
      VESPER_IOS_FFI_PREBUILT=1) ffi_prebuilt=1; shift ;;
      *) shift ;;
    esac
  done
  [ -n "$sdk" ]
  [ -n "$archive" ]
  [ "$ffi_prebuilt" = 1 ] || exit 25
  if [ "${VESPER_TEST_XCODEBUILD_FAIL_SDK:-}" = "$sdk" ]; then
    exit 23
  fi
  if [ "${VESPER_TEST_XCODEBUILD_WAIT_SDK:-}" = "$sdk" ]; then
    printf '%s' "$$" > "$VESPER_TEST_XCODEBUILD_PID_FILE"
    /bin/sh -c 'trap "" INT TERM; printf "%s" "$$" > "$1"; while :; do /bin/sleep 1; done' sh "$VESPER_TEST_XCODEBUILD_DESCENDANT_PID_FILE" &
    trap '' INT TERM
    while :; do /bin/sleep 1; done
  fi
  framework="$archive/Products/Library/Frameworks/VesperPlayerKit.framework"
  /bin/mkdir -p "$framework"
  printf 'fixture %s framework\n' "$sdk" > "$framework/VesperPlayerKit"
  printf 'fixture archive\n' > "$archive/Info.plist"
  exit 0
fi
if [ "${1:-}" = -create-xcframework ]; then
  output=
  device_framework=
  simulator_framework=
  device_library=
  simulator_library=
  headers=
  while [ "$#" -gt 0 ]; do
    case "$1" in
      -output) output=$2; shift 2 ;;
      -framework)
        if [ -z "$device_framework" ]; then
          device_framework=$2
        else
          simulator_framework=$2
        fi
        shift 2
        ;;
      -library)
        if [ -z "$device_library" ]; then
          device_library=$2
        else
          simulator_library=$2
        fi
        shift 2
        ;;
      -headers) headers=$2; shift 2 ;;
      *) shift ;;
    esac
  done
  [ -n "$output" ]
  if [ -z "$device_framework" ]; then
    [ -n "$device_library" ]
    [ -n "$simulator_library" ]
    [ -n "$headers" ]
    /bin/mkdir -p "$output/ios-arm64/Headers" "$output/ios-arm64-simulator/Headers"
    if [ "${VESPER_TEST_FFI_XCFRAMEWORK_MALFORMED:-}" = 1 ]; then
      printf 'not a plist\n' > "$output/Info.plist"
      exit 0
    fi
    /bin/cp "$device_library" "$output/ios-arm64/libvesper_player_ffi_ios.a"
    /bin/cp "$simulator_library" "$output/ios-arm64-simulator/libvesper_player_ffi_ios.a"
    /bin/cp -R "$headers/." "$output/ios-arm64/Headers"
    /bin/cp -R "$headers/." "$output/ios-arm64-simulator/Headers"
    /bin/cat > "$output/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>AvailableLibraries</key><array>
<dict><key>BinaryPath</key><string>libvesper_player_ffi_ios.a</string><key>HeadersPath</key><string>Headers</string><key>LibraryIdentifier</key><string>ios-arm64</string><key>LibraryPath</key><string>libvesper_player_ffi_ios.a</string><key>SupportedArchitectures</key><array><string>arm64</string></array><key>SupportedPlatform</key><string>ios</string></dict>
<dict><key>BinaryPath</key><string>libvesper_player_ffi_ios.a</string><key>HeadersPath</key><string>Headers</string><key>LibraryIdentifier</key><string>ios-arm64-simulator</string><key>LibraryPath</key><string>libvesper_player_ffi_ios.a</string><key>SupportedArchitectures</key><array><string>arm64</string></array><key>SupportedPlatform</key><string>ios</string><key>SupportedPlatformVariant</key><string>simulator</string></dict>
</array>
<key>CFBundlePackageType</key><string>XFWK</string>
<key>XCFrameworkFormatVersion</key><string>1.0</string>
</dict></plist>
EOF
    exit 0
  fi
  [ -n "$device_framework" ]
  [ -n "$simulator_framework" ]
  framework_name=$(/usr/bin/basename "$device_framework" .framework)
  if [ "${VESPER_TEST_XCODEBUILD_MALFORMED_XCFRAMEWORK:-}" = 1 ]; then
    /bin/mkdir -p "$output/ios-arm64" "$output/ios-arm64-simulator"
    printf 'not a plist\n' > "$output/Info.plist"
    exit 0
  fi
  /bin/mkdir -p "$output/ios-arm64" "$output/ios-arm64-simulator"
  /bin/cp -R "$device_framework" "$output/ios-arm64/$framework_name.framework"
  /bin/cp -R "$simulator_framework" "$output/ios-arm64-simulator/$framework_name.framework"
  /bin/cat > "$output/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>AvailableLibraries</key><array>
<dict><key>BinaryPath</key><string>$framework_name.framework/$framework_name</string><key>LibraryIdentifier</key><string>ios-arm64</string><key>LibraryPath</key><string>$framework_name.framework</string><key>SupportedArchitectures</key><array><string>arm64</string></array><key>SupportedPlatform</key><string>ios</string></dict>
<dict><key>BinaryPath</key><string>$framework_name.framework/$framework_name</string><key>LibraryIdentifier</key><string>ios-arm64-simulator</string><key>LibraryPath</key><string>$framework_name.framework</string><key>SupportedArchitectures</key><array><string>arm64</string></array><key>SupportedPlatform</key><string>ios</string><key>SupportedPlatformVariant</key><string>simulator</string></dict>
</array>
<key>CFBundlePackageType</key><string>XFWK</string>
<key>XCFrameworkFormatVersion</key><string>1.0</string>
</dict></plist>
EOF
  exit 0
fi
exit 24
"#,
    );
    write_executable_script(
        &tools.join("xcrun"),
        r#"#!/bin/sh
set -eu
printf 'xcrun %s\n' "$*" >> "$VESPER_TEST_TOOL_LOG"
if [ "${1:-}" = lipo ] && [ "${2:-}" = -archs ]; then
  printf '%s\n' "${VESPER_TEST_LIPO_ARCHS:-arm64}"
  exit 0
fi
if [ "${1:-}" = plutil ]; then
  shift
  exec /usr/bin/plutil "$@"
fi
exit 0
"#,
    );
    write_executable_script(
        &tools.join("ditto"),
        r#"#!/bin/sh
set -eu
printf 'ditto:%s\n' "$*" >> "$VESPER_TEST_TOOL_LOG"
/bin/cp -R "$1" "$2"
"#,
    );
}

#[cfg(target_os = "macos")]
fn ios_kit_output(fixture: &IosFfiFixture) -> PathBuf {
    fixture
        .root
        .join("lib/ios/VesperPlayerKit/.build/xcframework")
}

#[cfg(target_os = "macos")]
fn seed_previous_ios_kit_output(fixture: &IosFfiFixture) {
    let output = ios_kit_output(fixture);
    for name in [
        "VesperPlayerKit-iOS.xcarchive",
        "VesperPlayerKit-iOS-Simulator.xcarchive",
        "VesperPlayerKit.xcframework",
    ] {
        let artifact = output.join(name);
        fs::create_dir_all(&artifact).expect("create previous iOS kit artifact");
        fs::write(artifact.join("previous.txt"), b"previous iOS kit output\n")
            .expect("write previous iOS kit marker");
    }
}

#[cfg(target_os = "macos")]
fn run_ios_kit_fixture(
    fixture: &IosFfiFixture,
    extra_environment: &[(&str, &str)],
) -> std::process::Output {
    run_ios_ffi_fixture(fixture, &["kit-xcframework"], extra_environment)
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffi_full_build_uses_locked_unique_cargo_artifacts_and_stable_output() {
    let fixture = ios_ffi_fixture();
    let result = run_ios_ffi_fixture(&fixture, &["ffi", "debug"], &[]);

    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = fixture
        .root
        .canonicalize()
        .expect("canonical iOS FFI fixture root")
        .join("lib/ios/VesperPlayerKit/Artifacts/rust-player-ffi");
    assert_eq!(
        result.stdout,
        format!(
            "\nBuilt player-ffi Apple artifacts into:\n  {}\n",
            output.display()
        )
        .as_bytes()
    );
    assert!(result.stderr.is_empty());
    for platform in ["iphoneos", "iphonesimulator"] {
        assert!(
            output
                .join(platform)
                .join("libvesper_player_ffi_ios.a")
                .is_file()
        );
    }
    assert!(
        output
            .join("VesperPlayerFFI.xcframework/Info.plist")
            .is_file()
    );
    let log = fs::read_to_string(&fixture.log).expect("read iOS FFI fake tool log");
    assert!(log.contains("--locked"));
    assert!(log.contains("--target-dir"));
    assert!(log.contains("aarch64-apple-ios"));
    assert!(log.contains("aarch64-apple-ios-sim"));
    assert!(log.contains("xcodebuild"));
    assert!(log.contains("xcrun plutil -convert json -o -"));
    assert!(log.contains("xcrun lipo -archs"));
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffi_rejects_unusable_xcframeworks_before_publication() {
    for (variable, value, expected_diagnostic) in [
        (
            "VESPER_TEST_FFI_XCFRAMEWORK_MALFORMED",
            "1",
            "XCFramework manifest parsing",
        ),
        (
            "VESPER_TEST_LIPO_ARCHS",
            "x86_64",
            "must contain only arm64",
        ),
        (
            "VESPER_TEST_FFI_XCFRAMEWORK_MISSING_HEADER",
            "1",
            "unexpected header set",
        ),
        (
            "VESPER_TEST_FFI_XCFRAMEWORK_EXTRA_SLICE",
            "1",
            "contains more than 3 entries",
        ),
    ] {
        let fixture = ios_ffi_fixture();
        let output = fixture
            .root
            .join("lib/ios/VesperPlayerKit/Artifacts/rust-player-ffi");
        fs::create_dir_all(&output).expect("create previous iOS FFI distribution");
        fs::write(output.join("previous.txt"), b"previous distribution\n")
            .expect("write previous iOS FFI distribution marker");

        let result = run_ios_ffi_fixture(&fixture, &["ffi", "release"], &[(variable, value)]);

        assert_eq!(
            result.status.code(),
            Some(5),
            "{variable}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.stdout.is_empty(), "{variable}");
        assert!(
            String::from_utf8_lossy(&result.stderr).contains(expected_diagnostic),
            "{variable}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            fs::read(output.join("previous.txt"))
                .expect("read preserved iOS FFI distribution marker"),
            b"previous distribution\n"
        );
        assert!(
            fs::read_dir(output.parent().expect("iOS FFI distribution parent"))
                .expect("inspect iOS FFI distribution parent")
                .filter_map(Result::ok)
                .all(|entry| !entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".vesper-ios-ffi-stage-"))
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffi_platform_build_replaces_only_the_selected_slice() {
    let fixture = ios_ffi_fixture();
    let output = fixture
        .root
        .join("lib/ios/VesperPlayerKit/Artifacts/rust-player-ffi");
    fs::create_dir_all(output.join("iphoneos")).expect("create existing device output");
    fs::create_dir_all(output.join("iphonesimulator")).expect("create existing simulator output");
    fs::create_dir_all(output.join("VesperPlayerFFI.xcframework"))
        .expect("create existing XCFramework output");
    fs::write(output.join("iphoneos/old.txt"), b"keep device\n").expect("write old device marker");
    fs::write(
        output.join("iphonesimulator/old.txt"),
        b"replace simulator\n",
    )
    .expect("write old simulator marker");
    fs::write(
        output.join("VesperPlayerFFI.xcframework/old.txt"),
        b"keep xcframework\n",
    )
    .expect("write old XCFramework marker");

    let result = run_ios_ffi_fixture(
        &fixture,
        &["ffi", "release"],
        &[
            ("VESPER_BUILD_IOS_PLAYER_FFI_MODE", "platform"),
            ("PLATFORM_NAME", "iphonesimulator"),
            ("CARGO_TARGET_DIR", "custom target"),
        ],
    );
    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(output.join("iphoneos/old.txt").is_file());
    assert!(output.join("VesperPlayerFFI.xcframework/old.txt").is_file());
    assert!(!output.join("iphonesimulator/old.txt").exists());
    assert!(
        output
            .join("iphonesimulator/libvesper_player_ffi_ios.a")
            .is_file()
    );
    let log = fs::read_to_string(&fixture.log).expect("read platform FFI tool log");
    assert!(
        log.contains(
            &fixture
                .directory
                .path()
                .join("custom target")
                .display()
                .to_string()
        )
    );
    assert!(!log.contains("xcodebuild"));
    let strip = log
        .find("xcrun strip -S -x")
        .expect("release iOS FFI build strips the archive");
    let ranlib = log
        .find("xcrun ranlib")
        .expect("release iOS FFI build reindexes the archive");
    assert!(strip < ranlib, "strip must run before ranlib: {log}");
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffi_failure_preserves_the_previous_distribution() {
    let fixture = ios_ffi_fixture();
    let output = fixture
        .root
        .join("lib/ios/VesperPlayerKit/Artifacts/rust-player-ffi");
    fs::create_dir_all(&output).expect("create previous FFI distribution");
    fs::write(output.join("old-release.txt"), b"previous release\n")
        .expect("write previous FFI marker");
    write_executable_script(
        &fixture.tools.join("xcrun"),
        "#!/bin/sh\nprintf 'xcrun %s\\n' \"$*\" >> \"$VESPER_TEST_TOOL_LOG\"\nexit 19\n",
    );

    let result = run_ios_ffi_fixture(&fixture, &["ffi", "release"], &[]);
    assert_eq!(result.status.code(), Some(5));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("strip iphoneos"));
    assert_eq!(
        fs::read(output.join("old-release.txt")).expect("read preserved previous FFI marker"),
        b"previous release\n"
    );
    assert_eq!(
        fs::read_dir(output.parent().expect("FFI output parent"))
            .expect("inspect FFI output parent")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".vesper-ios-ffi-stage-")
            })
            .count(),
        0
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffi_rejects_a_symlinked_repository_lock() {
    use std::os::unix::fs::symlink;

    let fixture = ios_ffi_fixture();
    let lock = fixture.root.join(".vesper-ios-ffi.lock");
    let external_lock = fixture.directory.path().join("external lock");
    fs::write(&external_lock, b"external\n").expect("write external iOS FFI lock target");
    symlink(&external_lock, &lock).expect("create symlinked iOS FFI repository lock");

    let result = run_ios_ffi_fixture(&fixture, &["ffi", "debug"], &[]);

    assert_eq!(result.status.code(), Some(4));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("regular non-symlink"));
    assert_eq!(
        fs::read(&external_lock).expect("read external iOS FFI lock target"),
        b"external\n"
    );
    assert!(
        fs::read_dir(fixture.root.join("lib/ios/VesperPlayerKit/Artifacts"))
            .expect("inspect iOS FFI artifacts after lock rejection")
            .filter_map(Result::ok)
            .all(|entry| !entry
                .file_name()
                .to_string_lossy()
                .starts_with(".vesper-ios-ffi-stage-"))
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffi_lock_and_ctrl_c_reap_the_build_process_group_and_preserve_output() {
    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let fixture = ios_ffi_fixture();
    let output = fixture
        .root
        .join("lib/ios/VesperPlayerKit/Artifacts/rust-player-ffi");
    fs::create_dir_all(&output).expect("create previous iOS FFI distribution");
    fs::write(output.join("previous.txt"), b"previous distribution\n")
        .expect("write previous iOS FFI marker");
    let cargo_pid_path = fixture.directory.path().join("ios-ffi-cargo.pid");
    let descendant_pid_path = fixture.directory.path().join("ios-ffi-descendant.pid");
    let first_tmp = fixture.directory.path().join("first tmp");
    let second_tmp = fixture.directory.path().join("second tmp");
    fs::create_dir_all(&first_tmp).expect("create first iOS FFI temporary directory");
    fs::create_dir_all(&second_tmp).expect("create second iOS FFI temporary directory");
    write_executable_script(
        &fixture.tools.join("cargo"),
        r#"#!/bin/sh
printf '%s' "$$" > "$VESPER_TEST_CARGO_PID_FILE"
/bin/sh -c 'trap "" INT TERM; printf "%s" "$$" > "$1"; while :; do /bin/sleep 1; done' sh "$VESPER_TEST_DESCENDANT_PID_FILE" &
trap '' INT TERM
while :; do /bin/sleep 1; done
"#,
    );
    let path = std::env::join_paths([
        fixture.tools.as_path(),
        std::path::Path::new("/usr/bin"),
        std::path::Path::new("/bin"),
    ])
    .expect("compose cancellable iOS FFI PATH");

    let mut first = Command::new(env!("CARGO_BIN_EXE_vesper"));
    first
        .current_dir(&fixture.directory)
        .env("PATH", &path)
        .env("VESPER_TEST_TOOL_LOG", &fixture.log)
        .env("VESPER_TEST_RUST_TARGET_LIBDIR", &fixture.target_libdir)
        .env("VESPER_TEST_ARTIFACT_ROOT", &fixture.artifact_root)
        .env("VESPER_TEST_CARGO_PID_FILE", &cargo_pid_path)
        .env("VESPER_TEST_DESCENDANT_PID_FILE", &descendant_pid_path)
        .env("TMPDIR", &first_tmp)
        .args(["ios", "--root"])
        .arg(&fixture.root)
        .args(["ffi", "debug"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let first = first.spawn().expect("start cancellable iOS FFI build");
    let first_pid = i32::try_from(first.id()).expect("Vesper iOS FFI parent pid");

    let ready_deadline = Instant::now() + Duration::from_secs(15);
    while (!cargo_pid_path.is_file() || !descendant_pid_path.is_file())
        && Instant::now() < ready_deadline
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    if !cargo_pid_path.is_file() || !descendant_pid_path.is_file() {
        let _ = kill(Pid::from_raw(first_pid), Signal::SIGINT);
        let result = first
            .wait_with_output()
            .expect("wait after iOS FFI readiness failure");
        panic!(
            "iOS FFI Cargo process group was not ready; stderr: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let cargo_pid: i32 = fs::read_to_string(&cargo_pid_path)
        .expect("read iOS FFI Cargo pid")
        .parse()
        .expect("parse iOS FFI Cargo pid");
    let descendant_pid: i32 = fs::read_to_string(&descendant_pid_path)
        .expect("read iOS FFI Cargo descendant pid")
        .parse()
        .expect("parse iOS FFI Cargo descendant pid");

    let second = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .current_dir(&fixture.directory)
        .env("PATH", &path)
        .env("VESPER_TEST_TOOL_LOG", &fixture.log)
        .env("VESPER_TEST_RUST_TARGET_LIBDIR", &fixture.target_libdir)
        .env("VESPER_TEST_ARTIFACT_ROOT", &fixture.artifact_root)
        .env("TMPDIR", &second_tmp)
        .args(["ios", "--root"])
        .arg(&fixture.root)
        .args(["ffi", "debug"])
        .output()
        .expect("run competing iOS FFI build");
    kill(Pid::from_raw(first_pid), Signal::SIGINT).expect("cancel first iOS FFI build");
    let first = first
        .wait_with_output()
        .expect("wait for cancelled iOS FFI build");
    assert_eq!(second.status.code(), Some(4));
    assert!(second.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&second.stderr).contains("another iOS FFI build is already active")
    );
    assert_eq!(first.status.code(), Some(6));
    assert!(first.stdout.is_empty());
    assert!(String::from_utf8_lossy(&first.stderr).contains("was cancelled"));
    assert_eq!(
        fs::read(output.join("previous.txt")).expect("read preserved iOS FFI marker"),
        b"previous distribution\n"
    );
    assert_eq!(
        fs::read_dir(output.parent().expect("iOS FFI output parent"))
            .expect("inspect iOS FFI output parent after cancellation")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".vesper-ios-ffi-stage-")
            })
            .count(),
        0
    );

    for pid in [cargo_pid, descendant_pid] {
        let reap_deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match kill(Pid::from_raw(pid), None) {
                Err(Errno::ESRCH) => break,
                _ if Instant::now() < reap_deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                result => panic!("iOS FFI process {pid} is still alive: {result:?}"),
            }
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffi_rejects_ambiguous_cargo_staticlib_artifacts() {
    let fixture = ios_ffi_fixture();
    let output = fixture
        .root
        .join("lib/ios/VesperPlayerKit/Artifacts/rust-player-ffi");
    fs::create_dir_all(&output).expect("create previous FFI distribution");
    fs::write(output.join("old-release.txt"), b"previous release\n")
        .expect("write previous FFI marker");
    write_executable_script(
        &fixture.tools.join("cargo"),
        "#!/bin/sh\nset -eu\nfirst=\"$VESPER_TEST_ARTIFACT_ROOT/first/libvesper_player_ffi_ios.a\"\nsecond=\"$VESPER_TEST_ARTIFACT_ROOT/second/libvesper_player_ffi_ios.a\"\n/bin/mkdir -p \"$(/usr/bin/dirname \"$first\")\" \"$(/usr/bin/dirname \"$second\")\"\nprintf 'first\\n' > \"$first\"\nprintf 'second\\n' > \"$second\"\nprintf '{\"reason\":\"compiler-artifact\",\"target\":{\"name\":\"vesper_player_ffi_ios\",\"kind\":[\"staticlib\"]},\"filenames\":[\"%s\"]}\\n' \"$first\"\nprintf '{\"reason\":\"compiler-artifact\",\"target\":{\"name\":\"vesper_player_ffi_ios\",\"kind\":[\"staticlib\"]},\"filenames\":[\"%s\"]}\\n' \"$second\"\n",
    );

    let result = run_ios_ffi_fixture(&fixture, &["ffi", "debug"], &[]);
    assert_eq!(result.status.code(), Some(5));
    assert!(result.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("2 matching staticlib artifacts"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(output.join("old-release.txt").is_file());
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffi_classifies_an_abnormal_cargo_exit_before_parsing_output() {
    let fixture = ios_ffi_fixture();
    let output = fixture
        .root
        .join("lib/ios/VesperPlayerKit/Artifacts/rust-player-ffi");
    fs::create_dir_all(&output).expect("create previous iOS FFI distribution");
    fs::write(output.join("previous.txt"), b"previous distribution\n")
        .expect("write previous iOS FFI marker");
    write_executable_script(
        &fixture.tools.join("cargo"),
        "#!/bin/sh\nprintf '%s\\n' '{malformed Cargo JSON'\nprintf 'Cargo crashed\\n' >&2\nexit 143\n",
    );

    let result = run_ios_ffi_fixture(&fixture, &["ffi", "debug"], &[]);

    assert_eq!(result.status.code(), Some(6));
    assert!(result.stdout.is_empty());
    let diagnostics = String::from_utf8_lossy(&result.stderr);
    assert!(diagnostics.contains("Cargo crashed"));
    assert!(diagnostics.contains("terminated abnormally"));
    assert!(!diagnostics.contains("malformed JSON"));
    assert_eq!(
        fs::read(output.join("previous.txt")).expect("read preserved iOS FFI marker"),
        b"previous distribution\n"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffi_rejects_missing_tools_targets_and_catalyst_before_writing() {
    let fixture = ios_ffi_fixture();
    let output = fixture
        .root
        .join("lib/ios/VesperPlayerKit/Artifacts/rust-player-ffi");
    fs::create_dir_all(&output).expect("create previous FFI distribution");
    fs::write(output.join("old-release.txt"), b"previous release\n")
        .expect("write previous FFI marker");

    fs::remove_file(fixture.tools.join("xcodebuild")).expect("remove fake xcodebuild");
    let missing_tool = run_ios_ffi_fixture(&fixture, &["ffi", "release"], &[]);
    assert_eq!(missing_tool.status.code(), Some(4));
    assert!(missing_tool.stdout.is_empty());
    assert!(String::from_utf8_lossy(&missing_tool.stderr).contains("xcodebuild"));

    write_executable_script(&fixture.tools.join("xcodebuild"), "#!/bin/sh\nexit 0\n");
    write_executable_script(&fixture.tools.join("rustc"), "#!/bin/sh\nexit 1\n");
    let missing_target = run_ios_ffi_fixture(&fixture, &["ffi", "release"], &[]);
    assert_eq!(missing_target.status.code(), Some(4));
    assert!(missing_target.stdout.is_empty());
    assert!(String::from_utf8_lossy(&missing_target.stderr).contains("aarch64-apple-ios"));

    let catalyst = run_ios_ffi_fixture(
        &fixture,
        &["ffi", "release"],
        &[("VESPER_BUILD_APPLE_CATALYST", "1")],
    );
    assert_eq!(catalyst.status.code(), Some(4));
    assert!(catalyst.stdout.is_empty());
    assert!(String::from_utf8_lossy(&catalyst.stderr).contains("Mac Catalyst"));
    assert_eq!(
        fs::read(output.join("old-release.txt")).expect("read preserved FFI marker"),
        b"previous release\n"
    );
    assert!(!fixture.log.exists());
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffi_rejects_invalid_mode_and_platform_tokens_as_compatibility_errors() {
    let fixture = ios_ffi_fixture();
    for environment in [
        vec![("VESPER_BUILD_IOS_PLAYER_FFI_MODE", "invalid")],
        vec![("VESPER_BUILD_IOS_PLAYER_FFI_MODE", "platform")],
        vec![
            ("VESPER_BUILD_IOS_PLAYER_FFI_MODE", "platform"),
            ("PLATFORM_NAME", "macosx"),
        ],
    ] {
        let result = run_ios_ffi_fixture(&fixture, &["ffi", "release"], &environment);
        assert_eq!(result.status.code(), Some(4));
        assert!(result.stdout.is_empty());
    }
    assert!(!fixture.log.exists());
    assert!(
        fs::read_dir(fixture.root.join("lib/ios/VesperPlayerKit/Artifacts"))
            .expect("inspect untouched FFI artifacts parent")
            .next()
            .is_none()
    );
}

#[test]
fn ios_ffi_keeps_typed_help_and_usage_contracts() {
    let help = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "ffi", "--help"])
        .output()
        .expect("show iOS FFI help");
    assert_eq!(help.status.code(), Some(0));
    assert!(help.stderr.is_empty());
    let help = String::from_utf8(help.stdout).expect("UTF-8 iOS FFI help");
    assert!(help.contains("debug"));
    assert!(help.contains("release"));

    for arguments in [
        ["ios", "ffi", "unsupported", ""],
        ["ios", "ffi", "debug", "extra"],
    ] {
        let arguments = arguments
            .iter()
            .copied()
            .filter(|argument| !argument.is_empty());
        let invalid = Command::new(env!("CARGO_BIN_EXE_vesper"))
            .args(arguments)
            .output()
            .expect("reject invalid iOS FFI arguments");
        assert_eq!(invalid.status.code(), Some(2));
        assert!(invalid.stdout.is_empty());
    }
}

#[test]
fn ios_plugin_build_commands_are_public_but_videotoolbox_remains_internal() {
    let help = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "--help"])
        .output()
        .expect("show iOS plugin build help");
    assert_eq!(help.status.code(), Some(0));
    assert!(help.stderr.is_empty());
    let help = String::from_utf8(help.stdout).expect("UTF-8 iOS plugin build help");
    for command in [
        "remux-plugin",
        "source-normalizer-plugin",
        "frame-processor-plugin",
        "stage-remux-plugin-release",
        "stage-source-normalizer-plugin-release",
        "stage-frame-processor-plugin-release",
    ] {
        assert!(help.contains(command), "missing iOS command {command}");
    }
    assert!(!help.contains("decoder-videotoolbox-plugin"));
    assert!(!help.contains("stage-decoder-videotoolbox-plugin-release"));

    let command_help = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "frame-processor-plugin", "--help"])
        .output()
        .expect("show frame processor plugin help");
    assert_eq!(command_help.status.code(), Some(0));
    let command_help =
        String::from_utf8(command_help.stdout).expect("UTF-8 frame processor plugin help");
    assert!(command_help.contains("<OUTPUT_DIRECTORY>"));
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffmpeg_runtime_reuse_requires_both_snapshot_fingerprints() {
    let fixture = ios_plugin_fixture();
    configure_ios_plugin_ffmpeg_fixture(&fixture);
    let output_path = fixture.directory.path().join("runtime reuse release");
    let result = ios_ffmpeg_runtime_fixture_command(&fixture, &output_path)
        .env("VESPER_IOS_FFMPEG_RUNTIME_USE_EXISTING_INPUTS", "1")
        .output()
        .expect("reject FFmpeg runtime reuse without snapshot fingerprints");

    assert_eq!(result.status.code(), Some(4));
    assert!(result.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("Missing required FFmpeg input fingerprint override for ios-arm64")
    );
    assert!(!output_path.exists());
}

#[test]
fn ios_optional_release_dry_run_ignores_ambient_source_and_runtime_skip_overrides() {
    let directory = tempfile::tempdir().expect("temporary optional release dry-run fixture");
    let root = directory.path().join("repository");
    fs::create_dir_all(root.join("scripts")).expect("create optional release fixture scripts");
    fs::copy(
        workspace_root().join("scripts/ffmpeg-source-policy.toml"),
        root.join("scripts/ffmpeg-source-policy.toml"),
    )
    .expect("copy optional release FFmpeg source policy");
    fs::write(root.join("Cargo.toml"), "[workspace]\n")
        .expect("write optional release fixture manifest");
    let output = directory.path().join("release output");

    let result = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "--root"])
        .arg(&root)
        .arg("stage-optional-plugins-release")
        .arg(&output)
        .args([
            "--profile",
            "source-normalizer",
            "--dry-run",
            "ios-arm64",
            "ios-simulator-arm64",
        ])
        .env("VESPER_FFMPEG_SOURCE_POLICY_FILE", "ambient-override.toml")
        .env("VESPER_IOS_OPTIONAL_FFMPEG_VERSION", "8.1.999")
        .env("VESPER_SKIP_IOS_FFMPEG_RUNTIME_STAGE", "1")
        .output()
        .expect("run optional release dry-run fixture");

    assert_eq!(result.status.code(), Some(0));
    assert!(result.stderr.is_empty());
    let stdout = String::from_utf8(result.stdout).expect("UTF-8 optional release dry-run");
    assert!(stdout.contains(&format!("output={}", output.display())));
    assert!(stdout.contains("profile=source-normalizer"));
    assert!(stdout.contains("slices=ios-arm64,ios-simulator-arm64"));
    assert!(!output.exists());
    assert!(!root.join("lib").exists());
}

#[cfg(target_os = "macos")]
#[test]
fn ios_plugin_release_dry_run_preserves_declared_profiles_and_does_not_write() {
    let directory = tempfile::tempdir().expect("temporary iOS plugin release dry-run output");
    let root = workspace_root();
    let cases = [
        (
            "stage-remux-plugin-release",
            Some("profile=default"),
            "VesperPlayerRemuxFfmpegPlugin.xcframework.zip",
        ),
        (
            "stage-source-normalizer-plugin-release",
            Some("profile=source-normalizer"),
            "VesperPlayerSourceNormalizerFfmpegPlugin.xcframework.zip",
        ),
        (
            "stage-frame-processor-plugin-release",
            None,
            "VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip",
        ),
    ];
    for (index, (command, declared_profile, archive)) in cases.into_iter().enumerate() {
        let output_directory = directory.path().join(format!("release output {index}"));
        let result = Command::new(env!("CARGO_BIN_EXE_vesper"))
            .env_remove("VESPER_RELEASE_VERSION")
            .env_remove("VESPER_RELEASE_BUILD")
            .env_remove("VESPER_RELEASE_IOS_BUILD")
            .env_remove("VESPER_APPLE_FFMPEG_OUTPUT_DIR")
            .env_remove("VESPER_FFMPEG_OUTPUT_DIR")
            .args(["ios", "--root"])
            .arg(&root)
            .arg(command)
            .arg(&output_directory)
            .args(["--dry-run", "ios-arm64", "ios-simulator-arm64"])
            .output()
            .expect("run iOS plugin release dry-run");
        assert_eq!(
            result.status.code(),
            Some(0),
            "{command} stderr: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.stderr.is_empty());
        let stdout = String::from_utf8(result.stdout).expect("UTF-8 release dry-run output");
        assert!(stdout.contains("Selected slices:\n  ios-arm64\n  ios-simulator-arm64\n"));
        assert!(stdout.contains(archive));
        match declared_profile {
            Some(profile) => {
                assert!(stdout.contains(profile));
                assert!(stdout.contains("profile_hash=custom-e1eabc3db7bc"));
            }
            None => assert!(!stdout.contains("profile_hash=")),
        }
        assert!(
            !output_directory.exists(),
            "dry-run must not create the release output"
        );
    }

    let duplicate = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "--root"])
        .arg(&root)
        .arg("stage-frame-processor-plugin-release")
        .args(["--dry-run", "ios-arm64", "ios-arm64"])
        .output()
        .expect("reject duplicate iOS plugin release slices");
    assert_eq!(duplicate.status.code(), Some(4));
    assert!(duplicate.stdout.is_empty());
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("must not be repeated"));

    let incomplete = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "--root"])
        .arg(&root)
        .arg("stage-frame-processor-plugin-release")
        .args(["--dry-run", "ios-arm64"])
        .output()
        .expect("reject incomplete iOS plugin release slices");
    assert_eq!(incomplete.status.code(), Some(4));
    assert!(incomplete.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&incomplete.stderr);
    assert!(stderr.contains("ios-arm64"));
    assert!(stderr.contains("ios-simulator-arm64"));
}

#[cfg(target_os = "macos")]
#[test]
fn ios_plugin_release_holds_the_raw_guard_through_consumption_and_commits_atomically() {
    use std::time::{Duration, Instant};

    let fixture = ios_plugin_fixture();
    let release_output = fixture.directory.path().join("release output with spaces");
    let ready = fixture.directory.path().join("plugin-consumer.ready");
    let release = fixture.directory.path().join("plugin-consumer.release");
    let release_arguments = [
        release_output
            .to_str()
            .expect("UTF-8 iOS plugin release output"),
        "ios-arm64",
        "ios-simulator-arm64",
    ];
    let release_environment = [
        (
            "VESPER_TEST_PLUGIN_CONSUMER_READY",
            ready.to_str().expect("UTF-8 consumer ready path"),
        ),
        (
            "VESPER_TEST_PLUGIN_CONSUMER_RELEASE",
            release.to_str().expect("UTF-8 consumer release path"),
        ),
    ];
    let mut release_command = ios_plugin_fixture_command(
        &fixture,
        "stage-frame-processor-plugin-release",
        "libvesper_frame_processor_diagnostic.dylib",
        &release_arguments,
        &release_environment,
    );
    release_command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let release_process = release_command
        .spawn()
        .expect("spawn held-lock iOS plugin release");
    let deadline = Instant::now() + Duration::from_secs(60);
    while !ready.is_file() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    if !ready.is_file() {
        let _ = fs::write(&release, b"release\n");
        let output = release_process
            .wait_with_output()
            .expect("wait after iOS plugin consumer readiness timeout");
        panic!(
            "iOS plugin release did not reach its consumer gate; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let competing_output = fixture.directory.path().join("competing raw output");
    let competing = run_ios_plugin_fixture(
        &fixture,
        &[
            competing_output
                .to_str()
                .expect("UTF-8 competing raw output"),
            "debug",
            "ios-arm64",
        ],
        &[],
    );
    assert_eq!(competing.status.code(), Some(4));
    assert!(competing.stdout.is_empty());
    assert!(String::from_utf8_lossy(&competing.stderr).contains("already active"));
    assert!(!competing_output.exists());

    fs::write(&release, b"release\n").expect("release iOS plugin consumer gate");
    let staged = release_process
        .wait_with_output()
        .expect("wait for held-lock iOS plugin release");
    assert_eq!(
        staged.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&staged.stderr)
    );
    assert!(staged.stderr.is_empty());
    let staged_stdout = String::from_utf8(staged.stdout).expect("UTF-8 iOS plugin release stdout");
    assert!(!staged_stdout.contains(".vesper-ios-plugin-"));
    assert!(!staged_stdout.contains("ffmpeg-snapshot"));
    let archive = release_output.join("VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip");
    let canonical = fixture
        .root
        .join("lib/ios/VesperPlayerKit/.build/player-frame-processor-diagnostic-plugin/VesperPlayerFrameProcessorDiagnosticPlugin.xcframework");
    assert!(archive.is_file());
    assert!(canonical.join("Info.plist").is_file());
    assert!(
        canonical
            .join("ios-arm64/VesperPlayerFrameProcessorDiagnosticPlugin.framework/VesperPlayerFrameProcessorDiagnosticPlugin")
            .is_file()
    );
    let archive_before = fs::read(&archive).expect("read committed iOS plugin release archive");

    let failed = run_ios_plugin_fixture_for(
        &fixture,
        "stage-frame-processor-plugin-release",
        "libvesper_frame_processor_diagnostic.dylib",
        &release_arguments,
        &[("VESPER_TEST_PLUGIN_XCODEBUILD_STATUS", "1")],
    );
    assert_eq!(failed.status.code(), Some(5));
    assert!(failed.stdout.is_empty());
    assert_eq!(
        fs::read(&archive).expect("read preserved iOS plugin release archive"),
        archive_before
    );
    assert!(canonical.join("Info.plist").is_file());

    let cargo_count = fs::read_to_string(&fixture.log)
        .expect("read iOS plugin release tool log")
        .lines()
        .filter(|line| line.starts_with("cargo:"))
        .count();
    assert_eq!(cargo_count, 4, "the competing builder must not reach Cargo");
    assert!(!ios_plugin_release_journal(&fixture).exists());
    assert_no_ios_plugin_release_staging(&fixture, &release_output);
}

#[cfg(target_os = "macos")]
#[test]
fn ios_plugin_release_rolls_back_every_output_after_a_partial_promotion_failure() {
    let fixture = ios_plugin_fixture();
    let release_output = fixture.directory.path().join("partial promotion output");
    let initial = stage_frame_processor_release(&fixture, &release_output, &[]);
    assert_eq!(
        initial.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&initial.stderr)
    );
    let archive = release_output.join("VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip");
    let previous_archive = fs::read(&archive).expect("read previous plugin archive");
    let previous_binary = fs::read(frame_processor_release_binary(&fixture))
        .expect("read previous canonical plugin binary");

    let failed = stage_frame_processor_release(
        &fixture,
        &release_output,
        &[
            ("VESPER_TEST_PLUGIN_PAYLOAD", "replacement generation"),
            ("VESPER_TEST_PLUGIN_COMMIT_FAIL_AFTER", "1"),
        ],
    );
    assert_eq!(failed.status.code(), Some(3));
    assert!(failed.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&failed.stderr)
            .contains("injected iOS plugin release failure after 1 promotions")
    );
    assert_eq!(
        fs::read(&archive).expect("read rolled-back plugin archive"),
        previous_archive
    );
    assert_eq!(
        fs::read(frame_processor_release_binary(&fixture))
            .expect("read rolled-back canonical plugin binary"),
        previous_binary
    );
    assert!(!ios_plugin_release_journal(&fixture).exists());
    assert_no_ios_plugin_release_staging(&fixture, &release_output);
}

#[cfg(target_os = "macos")]
#[test]
fn ios_plugin_release_does_not_overwrite_a_replaced_commit_journal() {
    let fixture = ios_plugin_fixture();
    let release_output = fixture
        .directory
        .path()
        .join("commit journal replacement output");
    let initial = stage_frame_processor_release(&fixture, &release_output, &[]);
    assert_eq!(initial.status.code(), Some(0));
    let archive = release_output.join("VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip");
    let previous_archive = fs::read(&archive).expect("read pre-replacement plugin archive");
    let previous_binary = fs::read(frame_processor_release_binary(&fixture))
        .expect("read pre-replacement canonical plugin binary");
    let ready = fixture
        .directory
        .path()
        .join("commit-journal-replacement.ready");
    let release = fixture
        .directory
        .path()
        .join("commit-journal-replacement.release");
    let ready_argument = ready.to_str().expect("UTF-8 commit journal marker");
    let release_argument = release.to_str().expect("UTF-8 commit journal gate");
    let mut command = ios_plugin_fixture_command(
        &fixture,
        "stage-frame-processor-plugin-release",
        "libvesper_frame_processor_diagnostic.dylib",
        &[
            release_output
                .to_str()
                .expect("UTF-8 commit journal replacement output"),
            "ios-arm64",
            "ios-simulator-arm64",
        ],
        &[
            (
                "VESPER_TEST_PLUGIN_PAYLOAD",
                "journal replacement generation",
            ),
            ("VESPER_TEST_PLUGIN_COMMIT_READY", ready_argument),
            ("VESPER_TEST_PLUGIN_COMMIT_RELEASE", release_argument),
        ],
    );
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut process = command
        .spawn()
        .expect("spawn commit journal replacement release");
    if !wait_for_fixture_marker(&ready) {
        let _ = process.kill();
        let output = process
            .wait_with_output()
            .expect("wait after commit journal replacement timeout");
        panic!(
            "iOS plugin release did not reach commit journal replacement gate; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let journal = ios_plugin_release_journal(&fixture);
    let preserved = appended_path_suffix(&journal, ".preserved");
    fs::rename(&journal, &preserved).expect("preserve rollback promotion journal");
    fs::write(&journal, b"replacement promotion journal sentinel\n")
        .expect("write promotion journal replacement sentinel");
    fs::write(&release, b"release\n").expect("release commit journal replacement gate");
    let rejected = process
        .wait_with_output()
        .expect("wait for commit journal replacement release");
    assert_eq!(rejected.status.code(), Some(3));
    assert!(rejected.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("changed before atomic replacement"),
        "stderr: {}",
        String::from_utf8_lossy(&rejected.stderr)
    );
    assert_eq!(
        fs::read(&journal).expect("read promotion journal replacement sentinel"),
        b"replacement promotion journal sentinel\n"
    );

    fs::remove_file(&journal).expect("remove promotion journal replacement sentinel");
    fs::rename(&preserved, &journal).expect("restore rollback promotion journal");
    let recovered_then_failed = stage_frame_processor_release(
        &fixture,
        &release_output,
        &[("VESPER_TEST_PLUGIN_XCODEBUILD_STATUS", "1")],
    );
    assert_eq!(recovered_then_failed.status.code(), Some(5));
    assert_eq!(
        fs::read(&archive).expect("read rollback-restored plugin archive"),
        previous_archive
    );
    assert_eq!(
        fs::read(frame_processor_release_binary(&fixture))
            .expect("read rollback-restored canonical plugin binary"),
        previous_binary
    );
    assert!(!journal.exists());
    assert_no_ios_plugin_release_staging(&fixture, &release_output);
}

#[cfg(target_os = "macos")]
#[test]
fn ios_plugin_release_recovers_a_sigkill_before_starting_the_next_build() {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let fixture = ios_plugin_fixture();
    let release_output = fixture.directory.path().join("crash recovery output");
    let initial = stage_frame_processor_release(&fixture, &release_output, &[]);
    assert_eq!(initial.status.code(), Some(0));
    let archive = release_output.join("VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip");
    let previous_archive = fs::read(&archive).expect("read pre-crash plugin archive");
    let previous_binary = fs::read(frame_processor_release_binary(&fixture))
        .expect("read pre-crash canonical plugin binary");
    let ready = fixture.directory.path().join("partial-promotion.ready");
    let release = fixture.directory.path().join("partial-promotion.release");
    let output_argument = release_output
        .to_str()
        .expect("UTF-8 crash recovery output");
    let ready_argument = ready.to_str().expect("UTF-8 partial promotion marker");
    let release_argument = release.to_str().expect("UTF-8 partial promotion gate");
    let mut command = ios_plugin_fixture_command(
        &fixture,
        "stage-frame-processor-plugin-release",
        "libvesper_frame_processor_diagnostic.dylib",
        &[output_argument, "ios-arm64", "ios-simulator-arm64"],
        &[
            ("VESPER_TEST_PLUGIN_PAYLOAD", "crashed generation"),
            ("VESPER_TEST_PLUGIN_COMMIT_READY", ready_argument),
            ("VESPER_TEST_PLUGIN_COMMIT_RELEASE", release_argument),
        ],
    );
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut process = command
        .spawn()
        .expect("spawn crash-interrupted iOS plugin release");
    if !wait_for_fixture_marker(&ready) {
        let _ = process.kill();
        let output = process
            .wait_with_output()
            .expect("wait after partial promotion timeout");
        panic!(
            "iOS plugin release did not reach partial promotion; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(ios_plugin_release_journal(&fixture).is_file());
    assert_eq!(
        fs::read(frame_processor_release_binary(&fixture))
            .expect("read partially promoted canonical plugin binary"),
        b"crashed generation\n"
    );
    assert_eq!(
        fs::read(&archive).expect("read unpromoted plugin archive"),
        previous_archive
    );
    kill(Pid::from_raw(process.id() as i32), Signal::SIGKILL)
        .expect("kill partially promoted iOS plugin release");
    let killed = process
        .wait_with_output()
        .expect("wait for killed iOS plugin release");
    assert!(killed.status.code().is_none());
    let journal = ios_plugin_release_journal(&fixture);
    let journal_removal = appended_path_suffix(&journal, ".removing");
    fs::rename(&journal, &journal_removal)
        .expect("simulate a crash after quarantining the promotion journal");
    assert!(!journal.exists());
    assert!(journal_removal.is_file());

    let recovered_then_failed = stage_frame_processor_release(
        &fixture,
        &release_output,
        &[("VESPER_TEST_PLUGIN_XCODEBUILD_STATUS", "1")],
    );
    assert_eq!(recovered_then_failed.status.code(), Some(5));
    assert!(recovered_then_failed.stdout.is_empty());
    assert!(!journal_removal.exists());
    assert_eq!(
        fs::read(frame_processor_release_binary(&fixture))
            .expect("read recovered canonical plugin binary"),
        previous_binary
    );
    assert_eq!(
        fs::read(&archive).expect("read recovered plugin archive"),
        previous_archive
    );
    assert!(!ios_plugin_release_journal(&fixture).exists());
    assert_no_ios_plugin_release_staging(&fixture, &release_output);
}

#[cfg(target_os = "macos")]
#[test]
fn ios_plugin_release_recovers_prepared_owners_after_a_sigkill_before_build() {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let fixture = ios_plugin_fixture();
    let release_output = fixture.directory.path().join("prepared crash output");
    let ready = fixture.directory.path().join("prepared-owner.ready");
    let release = fixture.directory.path().join("prepared-owner.release");
    let output_argument = release_output
        .to_str()
        .expect("UTF-8 prepared-owner release output");
    let ready_argument = ready.to_str().expect("UTF-8 prepared-owner marker");
    let release_argument = release.to_str().expect("UTF-8 prepared-owner gate");
    let mut command = ios_plugin_fixture_command(
        &fixture,
        "stage-frame-processor-plugin-release",
        "libvesper_frame_processor_diagnostic.dylib",
        &[output_argument, "ios-arm64", "ios-simulator-arm64"],
        &[
            ("VESPER_TEST_PLUGIN_PREPARED_READY", ready_argument),
            ("VESPER_TEST_PLUGIN_PREPARED_RELEASE", release_argument),
        ],
    );
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut process = command
        .spawn()
        .expect("spawn prepared-owner crash test release");
    if !wait_for_fixture_marker(&ready) {
        let _ = process.kill();
        let output = process
            .wait_with_output()
            .expect("wait after prepared-owner gate timeout");
        panic!(
            "iOS plugin release did not reach prepared-owner gate; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let prepared_journal = ios_plugin_prepared_owner_journal(&fixture);
    assert!(prepared_journal.is_file());
    let build_parent = fixture.root.join("lib/ios/VesperPlayerKit/.build");
    let build_owners = fs::read_dir(&build_parent)
        .expect("inspect prepared build owners")
        .map(|entry| entry.expect("read prepared build owner").path())
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .starts_with(".vesper-ios-plugin-release-")
            })
        })
        .collect::<Vec<_>>();
    let asset_owners = fs::read_dir(&release_output)
        .expect("inspect prepared asset owners")
        .map(|entry| entry.expect("read prepared asset owner").path())
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .starts_with(".vesper-ios-plugin-assets-")
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(build_owners.len(), 1);
    assert_eq!(asset_owners.len(), 1);
    kill(Pid::from_raw(process.id() as i32), Signal::SIGKILL)
        .expect("kill prepared-owner iOS plugin release");
    let killed = process
        .wait_with_output()
        .expect("wait for killed prepared-owner iOS plugin release");
    assert!(killed.status.code().is_none());
    let build_owner_removal = appended_path_suffix(&build_owners[0], ".removing");
    fs::rename(&build_owners[0], &build_owner_removal)
        .expect("simulate a crash after quarantining a prepared owner");
    let prepared_journal_removal = appended_path_suffix(&prepared_journal, ".removing");
    fs::rename(&prepared_journal, &prepared_journal_removal)
        .expect("simulate a crash after quarantining the prepared-owner journal");
    assert!(!build_owners[0].exists());
    assert!(build_owner_removal.is_dir());
    assert!(!prepared_journal.exists());
    assert!(prepared_journal_removal.is_file());

    let recovered_then_failed = stage_frame_processor_release(
        &fixture,
        &release_output,
        &[("VESPER_TEST_PLUGIN_CARGO_STATUS", "1")],
    );
    assert_eq!(recovered_then_failed.status.code(), Some(5));
    assert!(recovered_then_failed.stdout.is_empty());
    assert!(!ios_plugin_prepared_owner_journal(&fixture).exists());
    assert_no_ios_plugin_release_staging(&fixture, &release_output);
    let cargo_count = fs::read_to_string(&fixture.log)
        .expect("read prepared-owner recovery tool log")
        .lines()
        .filter(|line| line.starts_with("cargo:"))
        .count();
    assert_eq!(cargo_count, 1, "the killed process must not reach Cargo");
    assert!(build_owners.iter().all(|path| !path.exists()));
    assert!(asset_owners.iter().all(|path| !path.exists()));
    assert!(!build_owner_removal.exists());
    assert!(!prepared_journal_removal.exists());
}

#[cfg(target_os = "macos")]
#[test]
fn ios_plugin_release_does_not_overwrite_a_replaced_prepared_journal() {
    let fixture = ios_plugin_fixture();
    let release_output = fixture
        .directory
        .path()
        .join("prepared journal replacement output");
    let ready = fixture
        .directory
        .path()
        .join("prepared-journal-replacement.ready");
    let release = fixture
        .directory
        .path()
        .join("prepared-journal-replacement.release");
    let ready_argument = ready.to_str().expect("UTF-8 prepared journal marker");
    let release_argument = release.to_str().expect("UTF-8 prepared journal gate");
    let mut command = ios_plugin_fixture_command(
        &fixture,
        "stage-frame-processor-plugin-release",
        "libvesper_frame_processor_diagnostic.dylib",
        &[
            release_output
                .to_str()
                .expect("UTF-8 prepared journal replacement output"),
            "ios-arm64",
            "ios-simulator-arm64",
        ],
        &[
            (
                "VESPER_TEST_PLUGIN_PREPARED_JOURNAL_REPLACE_READY",
                ready_argument,
            ),
            (
                "VESPER_TEST_PLUGIN_PREPARED_JOURNAL_REPLACE_RELEASE",
                release_argument,
            ),
        ],
    );
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut process = command
        .spawn()
        .expect("spawn prepared journal replacement release");
    if !wait_for_fixture_marker(&ready) {
        let _ = process.kill();
        let output = process
            .wait_with_output()
            .expect("wait after prepared journal replacement timeout");
        panic!(
            "iOS plugin release did not reach prepared journal replacement gate; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let journal = ios_plugin_prepared_owner_journal(&fixture);
    let preserved = appended_path_suffix(&journal, ".preserved");
    fs::rename(&journal, &preserved).expect("preserve prepared-owner journal");
    fs::write(&journal, b"replacement prepared journal sentinel\n")
        .expect("write prepared journal replacement sentinel");
    fs::write(&release, b"release\n").expect("release prepared journal replacement gate");
    let rejected = process
        .wait_with_output()
        .expect("wait for prepared journal replacement release");
    assert_eq!(rejected.status.code(), Some(3));
    assert!(rejected.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("changed before atomic replacement"),
        "stderr: {}",
        String::from_utf8_lossy(&rejected.stderr)
    );
    assert_eq!(
        fs::read(&journal).expect("read prepared journal replacement sentinel"),
        b"replacement prepared journal sentinel\n"
    );

    fs::remove_file(&journal).expect("remove prepared journal replacement sentinel");
    fs::rename(&preserved, &journal).expect("restore prepared-owner journal");
    let recovered_then_failed = stage_frame_processor_release(
        &fixture,
        &release_output,
        &[("VESPER_TEST_PLUGIN_CARGO_STATUS", "1")],
    );
    assert_eq!(recovered_then_failed.status.code(), Some(5));
    assert!(!journal.exists());
    assert_no_ios_plugin_release_staging(&fixture, &release_output);
}

#[cfg(target_os = "macos")]
#[test]
fn ios_plugin_release_fails_closed_when_a_prepared_owner_changes_identity() {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let fixture = ios_plugin_fixture();
    let release_output = fixture.directory.path().join("prepared identity output");
    let ready = fixture.directory.path().join("prepared-identity.ready");
    let release = fixture.directory.path().join("prepared-identity.release");
    let output_argument = release_output
        .to_str()
        .expect("UTF-8 prepared identity output");
    let ready_argument = ready.to_str().expect("UTF-8 prepared identity marker");
    let release_argument = release.to_str().expect("UTF-8 prepared identity gate");
    let mut command = ios_plugin_fixture_command(
        &fixture,
        "stage-frame-processor-plugin-release",
        "libvesper_frame_processor_diagnostic.dylib",
        &[output_argument, "ios-arm64", "ios-simulator-arm64"],
        &[
            ("VESPER_TEST_PLUGIN_PREPARED_READY", ready_argument),
            ("VESPER_TEST_PLUGIN_PREPARED_RELEASE", release_argument),
        ],
    );
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut process = command
        .spawn()
        .expect("spawn prepared-owner identity test release");
    if !wait_for_fixture_marker(&ready) {
        let _ = process.kill();
        let output = process
            .wait_with_output()
            .expect("wait after prepared-owner identity gate timeout");
        panic!(
            "iOS plugin release did not reach prepared-owner identity gate; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let build_parent = fixture.root.join("lib/ios/VesperPlayerKit/.build");
    let build_owner = ios_plugin_release_staging_entries(&fixture, &release_output)
        .into_iter()
        .find(|path| path.parent() == Some(build_parent.as_path()))
        .expect("prepared build owner");
    let preserved_owner = appended_path_suffix(&build_owner, ".preserved");
    fs::rename(&build_owner, &preserved_owner).expect("preserve the expected prepared owner");
    fs::create_dir(&build_owner).expect("create a replacement prepared owner");
    let sentinel = build_owner.join("must-not-be-removed.txt");
    fs::write(&sentinel, b"replacement owner\n").expect("write replacement owner sentinel");
    kill(Pid::from_raw(process.id() as i32), Signal::SIGKILL)
        .expect("kill prepared-owner identity test release");
    let killed = process
        .wait_with_output()
        .expect("wait for killed prepared-owner identity test release");
    assert!(killed.status.code().is_none());

    let rejected = stage_frame_processor_release(&fixture, &release_output, &[]);
    assert_eq!(rejected.status.code(), Some(3));
    assert!(rejected.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("changed identity"),
        "stderr: {}",
        String::from_utf8_lossy(&rejected.stderr)
    );
    assert!(ios_plugin_prepared_owner_journal(&fixture).is_file());
    assert_eq!(
        fs::read(&sentinel).expect("read preserved replacement owner sentinel"),
        b"replacement owner\n"
    );
    assert!(preserved_owner.is_dir());
    let cargo_count = fs::read_to_string(&fixture.log)
        .unwrap_or_default()
        .lines()
        .filter(|line| line.starts_with("cargo:"))
        .count();
    assert_eq!(cargo_count, 0, "identity recovery must fail before Cargo");
}

#[cfg(target_os = "macos")]
#[test]
fn ios_plugin_release_rolls_back_a_partial_commit_before_reporting_cancellation() {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let fixture = ios_plugin_fixture();
    let release_output = fixture.directory.path().join("cancelled commit output");
    let initial = stage_frame_processor_release(&fixture, &release_output, &[]);
    assert_eq!(initial.status.code(), Some(0));
    let archive = release_output.join("VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip");
    let previous_archive = fs::read(&archive).expect("read pre-cancellation plugin archive");
    let previous_binary = fs::read(frame_processor_release_binary(&fixture))
        .expect("read pre-cancellation canonical plugin binary");
    let ready = fixture.directory.path().join("cancelled-commit.ready");
    let release = fixture.directory.path().join("cancelled-commit.release");
    let output_argument = release_output
        .to_str()
        .expect("UTF-8 cancelled commit output");
    let ready_argument = ready.to_str().expect("UTF-8 cancelled commit marker");
    let release_argument = release.to_str().expect("UTF-8 cancelled commit gate");
    let mut command = ios_plugin_fixture_command(
        &fixture,
        "stage-frame-processor-plugin-release",
        "libvesper_frame_processor_diagnostic.dylib",
        &[output_argument, "ios-arm64", "ios-simulator-arm64"],
        &[
            ("VESPER_TEST_PLUGIN_PAYLOAD", "cancelled generation"),
            ("VESPER_TEST_PLUGIN_COMMIT_READY", ready_argument),
            ("VESPER_TEST_PLUGIN_COMMIT_RELEASE", release_argument),
        ],
    );
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut process = command
        .spawn()
        .expect("spawn cancellable iOS plugin release");
    if !wait_for_fixture_marker(&ready) {
        let _ = process.kill();
        let output = process
            .wait_with_output()
            .expect("wait after cancelled commit timeout");
        panic!(
            "iOS plugin release did not reach partial commit; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(
        fs::read(frame_processor_release_binary(&fixture))
            .expect("read partially committed canonical plugin binary"),
        b"cancelled generation\n"
    );
    assert_eq!(
        fs::read(&archive).expect("read uncommitted plugin archive"),
        previous_archive
    );
    kill(Pid::from_raw(process.id() as i32), Signal::SIGINT)
        .expect("cancel partial iOS plugin release commit");
    let cancelled = process
        .wait_with_output()
        .expect("wait for cancelled iOS plugin release commit");
    assert_eq!(cancelled.status.code(), Some(6));
    assert!(cancelled.stdout.is_empty());
    assert!(String::from_utf8_lossy(&cancelled.stderr).contains("cancelled"));
    assert_eq!(
        fs::read(frame_processor_release_binary(&fixture))
            .expect("read cancellation-restored canonical plugin binary"),
        previous_binary
    );
    assert_eq!(
        fs::read(&archive).expect("read cancellation-restored plugin archive"),
        previous_archive
    );
    assert!(!ios_plugin_release_journal(&fixture).exists());
    assert_no_ios_plugin_release_staging(&fixture, &release_output);
}

#[cfg(target_os = "macos")]
#[test]
fn ios_plugin_release_finishes_a_durable_commit_before_reporting_cancellation() {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let fixture = ios_plugin_fixture();
    let release_output = fixture.directory.path().join("durable commit output");
    let initial = stage_frame_processor_release(&fixture, &release_output, &[]);
    assert_eq!(initial.status.code(), Some(0));
    let ready = fixture.directory.path().join("durable-commit.ready");
    let release = fixture.directory.path().join("durable-commit.release");
    let output_argument = release_output
        .to_str()
        .expect("UTF-8 durable commit output");
    let ready_argument = ready.to_str().expect("UTF-8 durable commit marker");
    let release_argument = release.to_str().expect("UTF-8 durable commit gate");
    let mut command = ios_plugin_fixture_command(
        &fixture,
        "stage-frame-processor-plugin-release",
        "libvesper_frame_processor_diagnostic.dylib",
        &[output_argument, "ios-arm64", "ios-simulator-arm64"],
        &[
            ("VESPER_TEST_PLUGIN_PAYLOAD", "durably committed generation"),
            ("VESPER_TEST_PLUGIN_DURABLE_COMMIT_READY", ready_argument),
            (
                "VESPER_TEST_PLUGIN_DURABLE_COMMIT_RELEASE",
                release_argument,
            ),
        ],
    );
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut process = command.spawn().expect("spawn durable iOS plugin release");
    if !wait_for_fixture_marker(&ready) {
        let _ = process.kill();
        let output = process
            .wait_with_output()
            .expect("wait after durable commit timeout");
        panic!(
            "iOS plugin release did not reach durable commit; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(ios_plugin_release_journal(&fixture).is_file());
    kill(Pid::from_raw(process.id() as i32), Signal::SIGINT)
        .expect("cancel durably committed iOS plugin release");
    fs::write(&release, b"release\n").expect("release durable commit gate");
    let cancelled = process
        .wait_with_output()
        .expect("wait for cancelled durable iOS plugin release");
    assert_eq!(cancelled.status.code(), Some(6));
    assert!(cancelled.stdout.is_empty());
    assert!(String::from_utf8_lossy(&cancelled.stderr).contains("cancelled"));
    assert_eq!(
        fs::read(frame_processor_release_binary(&fixture))
            .expect("read durably committed canonical plugin binary"),
        b"durably committed generation\n"
    );
    assert!(!ios_plugin_release_journal(&fixture).exists());
    assert_no_ios_plugin_release_staging(&fixture, &release_output);
}

#[cfg(target_os = "macos")]
#[test]
fn ios_plugin_release_stops_recovery_when_committed_content_changes_in_place() {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let fixture = ios_plugin_fixture();
    let release_output = fixture.directory.path().join("content drift output");
    let initial = stage_frame_processor_release(&fixture, &release_output, &[]);
    assert_eq!(initial.status.code(), Some(0));
    let ready = fixture.directory.path().join("content-drift.ready");
    let release = fixture.directory.path().join("content-drift.release");
    let output_argument = release_output.to_str().expect("UTF-8 content drift output");
    let ready_argument = ready.to_str().expect("UTF-8 content drift marker");
    let release_argument = release.to_str().expect("UTF-8 content drift gate");
    let mut command = ios_plugin_fixture_command(
        &fixture,
        "stage-frame-processor-plugin-release",
        "libvesper_frame_processor_diagnostic.dylib",
        &[output_argument, "ios-arm64", "ios-simulator-arm64"],
        &[
            ("VESPER_TEST_PLUGIN_PAYLOAD", "committed content generation"),
            ("VESPER_TEST_PLUGIN_DURABLE_COMMIT_READY", ready_argument),
            (
                "VESPER_TEST_PLUGIN_DURABLE_COMMIT_RELEASE",
                release_argument,
            ),
        ],
    );
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut process = command
        .spawn()
        .expect("spawn content-drift iOS plugin release");
    if !wait_for_fixture_marker(&ready) {
        let _ = process.kill();
        let output = process
            .wait_with_output()
            .expect("wait after content drift gate timeout");
        panic!(
            "iOS plugin release did not reach durable commit; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    kill(Pid::from_raw(process.id() as i32), Signal::SIGKILL)
        .expect("kill durably committed iOS plugin release before cleanup");
    let killed = process
        .wait_with_output()
        .expect("wait for killed durable iOS plugin release");
    assert!(killed.status.code().is_none());
    let binary = frame_processor_release_binary(&fixture);
    let committed = fs::read(&binary).expect("read committed plugin binary before drift");
    assert_eq!(committed, b"committed content generation\n");
    fs::write(&binary, vec![b'x'; committed.len()])
        .expect("replace committed plugin content without changing its length");

    let rejected = stage_frame_processor_release(&fixture, &release_output, &[]);
    assert_eq!(rejected.status.code(), Some(3));
    assert!(rejected.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&rejected.stderr);
    assert!(
        stderr.contains("cannot satisfy its durable decision")
            || stderr.contains("unknown identities"),
        "stderr: {stderr}"
    );
    assert!(ios_plugin_release_journal(&fixture).is_file());
    assert_eq!(
        fs::read(&binary).expect("read preserved drifted plugin binary"),
        vec![b'x'; committed.len()]
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_plugin_release_preserves_its_journal_when_rollback_cleanup_fails() {
    let fixture = ios_plugin_fixture();
    let release_output = fixture.directory.path().join("rollback cleanup output");
    let initial = stage_frame_processor_release(&fixture, &release_output, &[]);
    assert_eq!(initial.status.code(), Some(0));
    let archive = release_output.join("VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip");
    let previous_archive = fs::read(&archive).expect("read previous plugin archive");
    let previous_binary = fs::read(frame_processor_release_binary(&fixture))
        .expect("read previous canonical plugin binary");

    let failed = stage_frame_processor_release(
        &fixture,
        &release_output,
        &[
            ("VESPER_TEST_PLUGIN_PAYLOAD", "cleanup failure generation"),
            ("VESPER_TEST_PLUGIN_COMMIT_FAIL_AFTER", "1"),
            ("VESPER_TEST_PLUGIN_RECOVERY_CLEANUP_FAIL", "1"),
        ],
    );
    assert_eq!(failed.status.code(), Some(3));
    assert!(failed.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&failed.stderr)
            .contains("injected iOS plugin release recovery cleanup failure")
    );
    assert!(ios_plugin_release_journal(&fixture).is_file());
    let quarantined_owners = ios_plugin_release_staging_entries(&fixture, &release_output)
        .into_iter()
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(".removing"))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        quarantined_owners.len(),
        1,
        "cleanup failure must retain exactly one quarantined owner"
    );
    assert_eq!(
        fs::read(frame_processor_release_binary(&fixture))
            .expect("read rollback-restored canonical plugin binary"),
        previous_binary
    );
    assert_eq!(
        fs::read(&archive).expect("read rollback-restored plugin archive"),
        previous_archive
    );

    let recovered_then_failed = stage_frame_processor_release(
        &fixture,
        &release_output,
        &[("VESPER_TEST_PLUGIN_XCODEBUILD_STATUS", "1")],
    );
    assert_eq!(recovered_then_failed.status.code(), Some(5));
    assert!(recovered_then_failed.stdout.is_empty());
    assert!(!ios_plugin_release_journal(&fixture).exists());
    assert_no_ios_plugin_release_staging(&fixture, &release_output);
}

#[cfg(target_os = "macos")]
#[test]
fn ios_plugin_release_does_not_delete_a_replaced_owner_quarantine() {
    let fixture = ios_plugin_fixture();
    let release_output = fixture
        .directory
        .path()
        .join("owner quarantine replacement output");
    let ready = fixture.directory.path().join("owner-cleanup.ready");
    let release = fixture.directory.path().join("owner-cleanup.release");
    let ready_argument = ready.to_str().expect("UTF-8 owner cleanup marker");
    let release_argument = release.to_str().expect("UTF-8 owner cleanup gate");
    let mut command = ios_plugin_fixture_command(
        &fixture,
        "stage-frame-processor-plugin-release",
        "libvesper_frame_processor_diagnostic.dylib",
        &[
            release_output
                .to_str()
                .expect("UTF-8 owner quarantine replacement output"),
            "ios-arm64",
            "ios-simulator-arm64",
        ],
        &[
            ("VESPER_TEST_PLUGIN_PAYLOAD", "owner cleanup generation"),
            ("VESPER_TEST_PLUGIN_OWNER_CLEANUP_READY", ready_argument),
            ("VESPER_TEST_PLUGIN_OWNER_CLEANUP_RELEASE", release_argument),
        ],
    );
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut process = command
        .spawn()
        .expect("spawn owner quarantine replacement release");
    if !wait_for_fixture_marker(&ready) {
        let _ = process.kill();
        let output = process
            .wait_with_output()
            .expect("wait after owner cleanup timeout");
        panic!(
            "iOS plugin release did not reach owner cleanup gate; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let quarantine = ios_plugin_release_staging_entries(&fixture, &release_output)
        .into_iter()
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(".removing"))
        })
        .expect("opened owner cleanup quarantine");
    let preserved = appended_path_suffix(&quarantine, ".preserved");
    fs::rename(&quarantine, &preserved).expect("preserve opened owner quarantine");
    fs::create_dir(&quarantine).expect("create replacement owner quarantine");
    let sentinel = quarantine.join("must-not-be-removed.txt");
    fs::write(&sentinel, b"replacement owner quarantine\n")
        .expect("write replacement owner quarantine sentinel");
    fs::write(&release, b"release\n").expect("release owner cleanup gate");
    let rejected = process
        .wait_with_output()
        .expect("wait for owner quarantine replacement release");
    assert_eq!(rejected.status.code(), Some(3));
    assert!(rejected.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("moved from"),
        "stderr: {}",
        String::from_utf8_lossy(&rejected.stderr)
    );
    assert_eq!(
        fs::read(&sentinel).expect("read replacement owner quarantine sentinel"),
        b"replacement owner quarantine\n"
    );
    assert!(preserved.is_dir());
    assert!(ios_plugin_release_journal(&fixture).is_file());

    fs::remove_file(&sentinel).expect("remove owner quarantine replacement sentinel");
    fs::remove_dir(&quarantine).expect("remove replacement owner quarantine");
    fs::rename(&preserved, &quarantine).expect("restore opened owner quarantine");
    let recovered_then_failed = stage_frame_processor_release(
        &fixture,
        &release_output,
        &[("VESPER_TEST_PLUGIN_XCODEBUILD_STATUS", "1")],
    );
    assert_eq!(recovered_then_failed.status.code(), Some(5));
    assert_eq!(
        fs::read(frame_processor_release_binary(&fixture))
            .expect("read committed canonical plugin binary"),
        b"owner cleanup generation\n"
    );
    assert!(!ios_plugin_release_journal(&fixture).exists());
    assert_no_ios_plugin_release_staging(&fixture, &release_output);
}

#[cfg(target_os = "macos")]
#[test]
fn ios_plugin_release_does_not_unlink_a_replaced_journal_quarantine() {
    let fixture = ios_plugin_fixture();
    let release_output = fixture
        .directory
        .path()
        .join("journal quarantine replacement output");
    let ready = fixture.directory.path().join("journal-cleanup.ready");
    let release = fixture.directory.path().join("journal-cleanup.release");
    let ready_argument = ready.to_str().expect("UTF-8 journal cleanup marker");
    let release_argument = release.to_str().expect("UTF-8 journal cleanup gate");
    let mut command = ios_plugin_fixture_command(
        &fixture,
        "stage-frame-processor-plugin-release",
        "libvesper_frame_processor_diagnostic.dylib",
        &[
            release_output
                .to_str()
                .expect("UTF-8 journal quarantine replacement output"),
            "ios-arm64",
            "ios-simulator-arm64",
        ],
        &[
            ("VESPER_TEST_PLUGIN_PAYLOAD", "journal cleanup generation"),
            (
                "VESPER_TEST_PLUGIN_PROMOTION_JOURNAL_CLEANUP_READY",
                ready_argument,
            ),
            (
                "VESPER_TEST_PLUGIN_PROMOTION_JOURNAL_CLEANUP_RELEASE",
                release_argument,
            ),
        ],
    );
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut process = command
        .spawn()
        .expect("spawn journal quarantine replacement release");
    if !wait_for_fixture_marker(&ready) {
        let _ = process.kill();
        let output = process
            .wait_with_output()
            .expect("wait after journal cleanup timeout");
        panic!(
            "iOS plugin release did not reach journal cleanup gate; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let journal = ios_plugin_release_journal(&fixture);
    let quarantine = appended_path_suffix(&journal, ".removing");
    let preserved = appended_path_suffix(&quarantine, ".preserved");
    fs::rename(&quarantine, &preserved).expect("preserve opened journal quarantine");
    fs::write(&quarantine, b"replacement journal quarantine sentinel\n")
        .expect("write replacement journal quarantine sentinel");
    fs::write(&release, b"release\n").expect("release journal cleanup gate");
    let rejected = process
        .wait_with_output()
        .expect("wait for journal quarantine replacement release");
    assert_eq!(rejected.status.code(), Some(3));
    assert!(rejected.stdout.is_empty());
    assert_eq!(
        fs::read(&quarantine).expect("read replacement journal quarantine sentinel"),
        b"replacement journal quarantine sentinel\n"
    );
    assert!(preserved.is_file());

    fs::remove_file(&quarantine).expect("remove replacement journal quarantine sentinel");
    fs::rename(&preserved, &quarantine).expect("restore opened journal quarantine");
    let recovered_then_failed = stage_frame_processor_release(
        &fixture,
        &release_output,
        &[("VESPER_TEST_PLUGIN_XCODEBUILD_STATUS", "1")],
    );
    assert_eq!(recovered_then_failed.status.code(), Some(5));
    assert_eq!(
        fs::read(frame_processor_release_binary(&fixture))
            .expect("read committed canonical plugin binary"),
        b"journal cleanup generation\n"
    );
    assert!(!journal.exists());
    assert!(!quarantine.exists());
    assert_no_ios_plugin_release_staging(&fixture, &release_output);
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffmpeg_plugin_release_applies_environment_overlays_once_after_config() {
    let fixture = ios_plugin_fixture();
    configure_ios_plugin_ffmpeg_fixture(&fixture);
    let release_output = fixture.directory.path().join("overlay dry-run output");
    let result = run_ios_plugin_fixture_for(
        &fixture,
        "stage-remux-plugin-release",
        "libvesper_remux_ffmpeg.dylib",
        &[
            release_output
                .to_str()
                .expect("UTF-8 overlay dry-run output"),
            "--profile",
            "default",
            "--dry-run",
            "ios-arm64",
            "ios-simulator-arm64",
        ],
        &[
            ("VESPER_FFMPEG_TLS_BACKEND", "none"),
            ("VESPER_APPLE_FFMPEG_TLS_BACKEND", "securetransport"),
            ("VESPER_FFMPEG_ENABLE_DASH", "0"),
            ("VESPER_APPLE_FFMPEG_ENABLE_DASH", "1"),
            ("VESPER_FFMPEG_ENABLE_DECODERS", "h264"),
            ("VESPER_APPLE_FFMPEG_ENABLE_DECODERS", "hevc,h264"),
            ("VESPER_FFMPEG_EXTRA_CONFIGURE_ARGS", "--disable-doc"),
            (
                "VESPER_APPLE_FFMPEG_EXTRA_CONFIGURE_ARGS",
                "--disable-debug",
            ),
        ],
    );

    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stderr.is_empty());
    let stdout = String::from_utf8(result.stdout).expect("UTF-8 overlay release dry-run");
    assert!(stdout.contains("profile_hash=custom-41a785688541"));
    assert!(stdout.contains("  --tls-backend\n  securetransport\n"));
    assert!(stdout.contains("  --enable-decoders\n  h264,hevc\n"));
    assert!(stdout.contains("  --enable-dash\n"));
    assert_eq!(stdout.matches("  --disable-doc\n").count(), 1);
    assert_eq!(stdout.matches("  --disable-debug\n").count(), 1);
    assert!(!release_output.exists());
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffmpeg_plugin_release_roots_relative_config_and_output_overrides() {
    let fixture = ios_plugin_fixture();
    configure_ios_plugin_ffmpeg_fixture(&fixture);
    let relative_config = fixture.root.join("config/relative-ffmpeg-profiles.toml");
    fs::create_dir_all(relative_config.parent().expect("relative config parent"))
        .expect("create relative FFmpeg config parent");
    fs::copy(
        fixture.root.join("scripts/ffmpeg-profiles.toml"),
        &relative_config,
    )
    .expect("copy relative FFmpeg profile config");
    let release_output = fixture
        .directory
        .path()
        .join("relative path dry-run output");
    let result = run_ios_plugin_fixture_for(
        &fixture,
        "stage-remux-plugin-release",
        "libvesper_remux_ffmpeg.dylib",
        &[
            release_output
                .to_str()
                .expect("UTF-8 relative path dry-run output"),
            "--profile",
            "default",
            "--dry-run",
            "ios-arm64",
            "ios-simulator-arm64",
        ],
        &[
            (
                "VESPER_FFMPEG_PROFILE_CONFIG_PATH",
                "config/relative-ffmpeg-profiles.toml",
            ),
            ("VESPER_APPLE_FFMPEG_OUTPUT_DIR", "relative FFmpeg output"),
        ],
    );

    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8(result.stdout).expect("UTF-8 relative path release dry-run");
    assert!(stdout.contains("profile_hash=custom-e1eabc3db7bc"));
    let canonical_root =
        fs::canonicalize(&fixture.root).expect("canonical relative path fixture root");
    assert!(stdout.contains(&format!(
        "ffmpeg_output_directory={}",
        canonical_root.join("relative FFmpeg output").display()
    )));

    let empty_config = run_ios_plugin_fixture_for(
        &fixture,
        "stage-remux-plugin-release",
        "libvesper_remux_ffmpeg.dylib",
        &[
            release_output
                .to_str()
                .expect("UTF-8 empty-config dry-run output"),
            "--profile",
            "default",
            "--dry-run",
            "ios-arm64",
            "ios-simulator-arm64",
        ],
        &[("VESPER_FFMPEG_PROFILE_CONFIG_PATH", "")],
    );
    assert_eq!(
        empty_config.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&empty_config.stderr)
    );
    assert!(
        String::from_utf8_lossy(&empty_config.stdout).contains("profile_hash=custom-e1eabc3db7bc")
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffmpeg_plugin_release_preserves_declared_profile_and_framework_provenance() {
    let fixture = ios_plugin_fixture();
    configure_ios_plugin_ffmpeg_fixture(&fixture);
    let release_output = fixture.directory.path().join("FFmpeg release output");
    seed_ios_ffmpeg_runtime_profiles(
        &fixture,
        &release_output,
        &["ios-arm64", "ios-simulator-arm64"],
    );
    let result = run_ios_plugin_fixture_for(
        &fixture,
        "stage-remux-plugin-release",
        "libvesper_remux_ffmpeg.dylib",
        &[
            release_output
                .to_str()
                .expect("UTF-8 FFmpeg plugin release output"),
            "--profile",
            "default",
            "ios-arm64",
            "ios-simulator-arm64",
        ],
        &[("VESPER_SKIP_IOS_FFMPEG_RUNTIME_STAGE", "1")],
    );
    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stderr.is_empty());
    let archive = release_output.join("VesperPlayerRemuxFfmpegPlugin.xcframework.zip");
    assert!(archive.is_file());
    let framework = fixture
        .root
        .join("lib/ios/VesperPlayerKit/.build/player-remux-ffmpeg-plugin/VesperPlayerRemuxFfmpegPlugin.xcframework/ios-arm64/VesperPlayerRemuxFfmpegPlugin.framework");
    assert_eq!(
        fs::read_to_string(framework.join("profile-hash.txt"))
            .expect("read remux framework profile hash"),
        "custom-e1eabc3db7bc\n"
    );
    assert_eq!(
        fs::read_to_string(framework.join("ios-arm64-vesper-ffmpeg-build-metadata.txt"))
            .expect("read remux framework build metadata"),
        fs::read_to_string(
            fixture
                .root
                .join("third_party/ffmpeg/apple/profiles/custom-e1eabc3db7bc/ios/vesper-ffmpeg-build-metadata.txt")
        )
        .expect("read source FFmpeg fixture metadata")
    );
    let binary_checksum = fs::read_to_string(framework.join("binary-sha256.txt"))
        .expect("read remux framework binary checksum");
    assert_eq!(binary_checksum.trim().len(), 64);

    let log = fs::read_to_string(&fixture.log).expect("read FFmpeg plugin release tool log");
    assert!(
        log.lines().any(|line| {
            line.starts_with("ffmpeg_dir:") && line.contains("ffmpeg-snapshot/ios")
        })
    );
    assert!(log.contains("headerpad_max_install_names"));
    assert!(
        log.lines()
            .any(|line| line.starts_with("cargo_target_dir:"))
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffmpeg_plugin_release_builds_runtime_and_plugin_from_one_snapshot() {
    let fixture = ios_plugin_fixture();
    configure_ios_plugin_ffmpeg_fixture(&fixture);
    let release_output = fixture.directory.path().join("full FFmpeg release output");
    let mut command = ios_ffmpeg_remux_release_command(&fixture, &release_output, &[]);
    apply_ios_ffmpeg_runtime_reuse_environment(&mut command, &fixture.root);
    let result = command
        .output()
        .expect("build one Rust-native FFmpeg runtime and plugin release");
    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stderr.is_empty());
    let stdout = String::from_utf8(result.stdout).expect("UTF-8 FFmpeg release stdout");
    assert!(!stdout.contains(".vesper-ios-plugin-"));
    assert!(!stdout.contains("ffmpeg-snapshot"));

    let source = fixture
        .root
        .join("third_party/ffmpeg/apple/profiles/custom-e1eabc3db7bc/ios");
    let metadata = fs::read(source.join("vesper-ffmpeg-build-metadata.txt"))
        .expect("read full release FFmpeg metadata");
    let expected_fingerprint = format!(
        "{}\n",
        ios_ffmpeg_input_fingerprint(&fixture.root, "ios-arm64")
    );
    let framework = fixture
        .root
        .join("lib/ios/VesperPlayerKit/.build/player-remux-ffmpeg-plugin/VesperPlayerRemuxFfmpegPlugin.xcframework/ios-arm64/VesperPlayerRemuxFfmpegPlugin.framework");
    assert_eq!(
        fs::read(framework.join("input-fingerprint.txt")).expect("read plugin input fingerprint"),
        expected_fingerprint.as_bytes()
    );
    let canonical_runtime = fixture
        .root
        .join("lib/ios/VesperPlayerKit/.build/player-ffmpeg-runtime");
    for framework in [
        "VesperFFmpegAVCodec",
        "VesperFFmpegAVFormat",
        "VesperFFmpegAVUtil",
    ] {
        let archive = release_output.join(format!("{framework}.xcframework.zip"));
        assert!(
            archive.is_file(),
            "missing Rust-generated {framework} archive"
        );
        assert!(
            canonical_runtime
                .join(format!("{framework}.xcframework"))
                .is_dir(),
            "missing canonical {framework} XCFramework"
        );
        let framework_root = format!("{framework}.xcframework/ios-arm64/{framework}.framework");
        assert_eq!(
            read_zip_fixture_entry(&archive, &format!("{framework_root}/input-fingerprint.txt")),
            expected_fingerprint.as_bytes()
        );
        assert_eq!(
            read_zip_fixture_entry(
                &archive,
                &format!("{framework_root}/ios-arm64-vesper-ffmpeg-build-metadata.txt"),
            ),
            metadata
        );
        assert_eq!(
            read_zip_fixture_entry(&archive, &format!("{framework_root}/profile-hash.txt")),
            b"custom-e1eabc3db7bc\n"
        );
    }
    let log = fs::read_to_string(&fixture.log).expect("read full FFmpeg release tool log");
    assert!(!log.contains("apple_ffmpeg_configure_begin"));
    assert_eq!(
        log.lines()
            .filter(|line| line.starts_with("xcodebuild:-create-xcframework"))
            .count(),
        4
    );
    assert!(
        log.lines()
            .filter(|line| line.starts_with("ffmpeg_dir:"))
            .all(|line| line.contains("ffmpeg-snapshot"))
    );
    assert!(!ios_plugin_release_journal(&fixture).exists());
    assert_no_ios_plugin_release_staging(&fixture, &release_output);
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffmpeg_plugin_release_rejects_a_symlinked_slice_directory() {
    use std::os::unix::fs::symlink;

    let fixture = ios_plugin_fixture();
    configure_ios_plugin_ffmpeg_fixture(&fixture);
    let release_output = fixture.directory.path().join("symlinked FFmpeg output");
    seed_ios_ffmpeg_runtime_profiles(
        &fixture,
        &release_output,
        &["ios-arm64", "ios-simulator-arm64"],
    );
    let slice = fixture
        .root
        .join("third_party/ffmpeg/apple/profiles/custom-e1eabc3db7bc/ios");
    fs::remove_dir_all(&slice).expect("remove the regular FFmpeg slice fixture");
    symlink("../../ios", &slice).expect("replace the FFmpeg slice with an alias");

    let result = ios_ffmpeg_remux_release_command(
        &fixture,
        &release_output,
        &[("VESPER_SKIP_IOS_FFMPEG_RUNTIME_STAGE", "1")],
    )
    .output()
    .expect("run symlinked FFmpeg slice release");
    assert_eq!(result.status.code(), Some(5));
    assert!(result.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("regular non-symlink directory"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_no_remux_plugin_release_outputs(&fixture, &release_output);
    assert!(!ios_plugin_release_journal(&fixture).exists());
    assert_no_ios_plugin_release_staging(&fixture, &release_output);
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffmpeg_plugin_release_rejects_runtime_archive_replacement_before_promotion() {
    let fixture = ios_plugin_fixture();
    configure_ios_plugin_ffmpeg_fixture(&fixture);
    let release_output = fixture.directory.path().join("runtime replacement output");
    seed_ios_ffmpeg_runtime_profiles(
        &fixture,
        &release_output,
        &["ios-arm64", "ios-simulator-arm64"],
    );
    let ready = fixture.directory.path().join("runtime-replacement.ready");
    let release = fixture.directory.path().join("runtime-replacement.release");
    let ready_argument = ready.to_str().expect("UTF-8 runtime replacement marker");
    let release_argument = release.to_str().expect("UTF-8 runtime replacement gate");
    let mut command = ios_ffmpeg_remux_release_command(
        &fixture,
        &release_output,
        &[
            ("VESPER_SKIP_IOS_FFMPEG_RUNTIME_STAGE", "1"),
            ("VESPER_TEST_PLUGIN_CARGO_READY", ready_argument),
            ("VESPER_TEST_PLUGIN_CARGO_RELEASE", release_argument),
        ],
    );
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut process = command
        .spawn()
        .expect("spawn runtime archive replacement release");
    if !wait_for_fixture_marker(&ready) {
        let _ = process.kill();
        let output = process
            .wait_with_output()
            .expect("wait after runtime archive replacement gate timeout");
        panic!(
            "iOS plugin release did not reach the runtime archive gate; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let archive = release_output.join("VesperFFmpegAVCodec.xcframework.zip");
    let preserved = fixture
        .directory
        .path()
        .join("preserved runtime archive.zip");
    fs::rename(&archive, &preserved).expect("preserve the snapshotted runtime archive");
    fs::copy(&preserved, &archive).expect("replace the runtime archive with a new inode");
    fs::write(&release, b"release\n").expect("release runtime archive replacement gate");
    let result = process
        .wait_with_output()
        .expect("wait for runtime archive replacement release");
    assert_eq!(result.status.code(), Some(3));
    assert!(result.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("changed after it was snapshotted"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read(&archive).expect("read replacement runtime archive"),
        fs::read(&preserved).expect("read preserved runtime archive")
    );
    assert_no_remux_plugin_release_outputs(&fixture, &release_output);
    assert!(!ios_plugin_release_journal(&fixture).exists());
    assert_no_ios_plugin_release_staging(&fixture, &release_output);
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffmpeg_plugin_release_preserves_runtime_replacement_after_partial_commit() {
    let fixture = ios_plugin_fixture();
    configure_ios_plugin_ffmpeg_fixture(&fixture);
    let release_output = fixture
        .directory
        .path()
        .join("runtime partial promotion output");
    seed_ios_ffmpeg_runtime_profiles(
        &fixture,
        &release_output,
        &["ios-arm64", "ios-simulator-arm64"],
    );

    let initial = ios_ffmpeg_remux_release_command(
        &fixture,
        &release_output,
        &[("VESPER_SKIP_IOS_FFMPEG_RUNTIME_STAGE", "1")],
    )
    .output()
    .expect("run initial FFmpeg release");
    assert_eq!(
        initial.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&initial.stderr)
    );

    let plugin_archive = release_output.join("VesperPlayerRemuxFfmpegPlugin.xcframework.zip");
    let previous_plugin_archive = fs::read(&plugin_archive).expect("read previous plugin archive");
    let plugin_framework = fixture.root.join(
        "lib/ios/VesperPlayerKit/.build/player-remux-ffmpeg-plugin/".to_owned()
            + "VesperPlayerRemuxFfmpegPlugin.xcframework/ios-arm64/"
            + "VesperPlayerRemuxFfmpegPlugin.framework/VesperPlayerRemuxFfmpegPlugin",
    );
    let previous_plugin_binary = fs::read(&plugin_framework).expect("read previous plugin binary");

    let ready = fixture
        .directory
        .path()
        .join("runtime-partial-promotion.ready");
    let release = fixture
        .directory
        .path()
        .join("runtime-partial-promotion.release");
    let ready_argument = ready.to_str().expect("UTF-8 runtime partial marker");
    let release_argument = release.to_str().expect("UTF-8 runtime partial gate");
    let mut command = ios_ffmpeg_remux_release_command(
        &fixture,
        &release_output,
        &[
            ("VESPER_SKIP_IOS_FFMPEG_RUNTIME_STAGE", "1"),
            (
                "VESPER_TEST_PLUGIN_PAYLOAD",
                "runtime replacement generation",
            ),
            ("VESPER_TEST_PLUGIN_COMMIT_READY", ready_argument),
            ("VESPER_TEST_PLUGIN_COMMIT_RELEASE", release_argument),
        ],
    );
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut process = command
        .spawn()
        .expect("spawn runtime partial promotion release");
    if !wait_for_fixture_marker(&ready) {
        let _ = process.kill();
        let output = process
            .wait_with_output()
            .expect("wait after runtime partial promotion gate timeout");
        panic!(
            "iOS plugin release did not reach runtime partial promotion gate; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let runtime_archive = release_output.join("VesperFFmpegAVCodec.xcframework.zip");
    let preserved = fixture
        .directory
        .path()
        .join("preserved runtime partial replacement.zip");
    fs::rename(&runtime_archive, &preserved).expect("preserve runtime archive before replacement");
    fs::write(&runtime_archive, b"replacement runtime archive\n")
        .expect("write replacement runtime archive");
    fs::write(&release, b"release\n").expect("release runtime partial promotion gate");

    let rejected = process
        .wait_with_output()
        .expect("wait for runtime partial promotion release");
    assert_eq!(rejected.status.code(), Some(3));
    assert!(rejected.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&rejected.stderr);
    assert!(
        stderr.contains("unknown identities") || stderr.contains("not ready for promotion"),
        "stderr: {stderr}"
    );
    assert_eq!(
        fs::read(&plugin_archive).expect("read rolled-back plugin archive"),
        previous_plugin_archive
    );
    assert_eq!(
        fs::read(&plugin_framework).expect("read rolled-back plugin binary"),
        previous_plugin_binary
    );
    assert_eq!(
        fs::read(&runtime_archive).expect("read preserved replacement runtime archive"),
        b"replacement runtime archive\n"
    );
    assert!(!ios_plugin_release_journal(&fixture).exists());
    assert_no_ios_plugin_release_staging(&fixture, &release_output);
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffmpeg_plugin_release_rejects_an_immutable_snapshot_file_change() {
    let fixture = ios_plugin_fixture();
    configure_ios_plugin_ffmpeg_fixture(&fixture);
    let release_output = fixture.directory.path().join("snapshot file change output");
    seed_ios_ffmpeg_runtime_profiles(
        &fixture,
        &release_output,
        &["ios-arm64", "ios-simulator-arm64"],
    );
    let ready = fixture.directory.path().join("snapshot-file.ready");
    let release = fixture.directory.path().join("snapshot-file.release");
    let ready_argument = ready.to_str().expect("UTF-8 snapshot file marker");
    let release_argument = release.to_str().expect("UTF-8 snapshot file gate");
    let mut command = ios_ffmpeg_remux_release_command(
        &fixture,
        &release_output,
        &[
            ("VESPER_SKIP_IOS_FFMPEG_RUNTIME_STAGE", "1"),
            ("VESPER_TEST_PLUGIN_RAW_OUTPUT_READY", ready_argument),
            ("VESPER_TEST_PLUGIN_RAW_OUTPUT_RELEASE", release_argument),
        ],
    );
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut process = command
        .spawn()
        .expect("spawn immutable snapshot file test release");
    if !wait_for_fixture_marker(&ready) {
        let _ = process.kill();
        let output = process
            .wait_with_output()
            .expect("wait after immutable snapshot file gate timeout");
        panic!(
            "iOS plugin release did not reach the raw output gate; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let snapshot_file = ios_plugin_release_staging_entries(&fixture, &release_output)
        .into_iter()
        .map(|owner| owner.join("ffmpeg-snapshot/ios/include/libavutil/avutil.h"))
        .find(|path| path.is_file())
        .expect("immutable FFmpeg snapshot header");
    let original = fs::read(&snapshot_file).expect("read immutable FFmpeg snapshot header");
    fs::write(&snapshot_file, vec![b'x'; original.len()])
        .expect("change immutable FFmpeg snapshot header in place");
    fs::write(&release, b"release\n").expect("release immutable snapshot file gate");
    let result = process
        .wait_with_output()
        .expect("wait for immutable snapshot file test release");
    assert_eq!(result.status.code(), Some(3));
    assert!(result.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("immutable FFmpeg release snapshot changed"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_no_remux_plugin_release_outputs(&fixture, &release_output);
    assert!(!ios_plugin_release_journal(&fixture).exists());
    assert_no_ios_plugin_release_staging(&fixture, &release_output);
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffmpeg_plugin_release_rejects_an_immutable_snapshot_directory_replacement() {
    let fixture = ios_plugin_fixture();
    configure_ios_plugin_ffmpeg_fixture(&fixture);
    let release_output = fixture.directory.path().join("snapshot directory output");
    seed_ios_ffmpeg_runtime_profiles(
        &fixture,
        &release_output,
        &["ios-arm64", "ios-simulator-arm64"],
    );
    let ready = fixture.directory.path().join("snapshot-directory.ready");
    let release = fixture.directory.path().join("snapshot-directory.release");
    let ready_argument = ready.to_str().expect("UTF-8 snapshot directory marker");
    let release_argument = release.to_str().expect("UTF-8 snapshot directory gate");
    let mut command = ios_ffmpeg_remux_release_command(
        &fixture,
        &release_output,
        &[
            ("VESPER_SKIP_IOS_FFMPEG_RUNTIME_STAGE", "1"),
            ("VESPER_TEST_PLUGIN_RAW_OUTPUT_READY", ready_argument),
            ("VESPER_TEST_PLUGIN_RAW_OUTPUT_RELEASE", release_argument),
        ],
    );
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut process = command
        .spawn()
        .expect("spawn immutable snapshot directory test release");
    if !wait_for_fixture_marker(&ready) {
        let _ = process.kill();
        let output = process
            .wait_with_output()
            .expect("wait after immutable snapshot directory gate timeout");
        panic!(
            "iOS plugin release did not reach the raw output gate; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let snapshot_directory = ios_plugin_release_staging_entries(&fixture, &release_output)
        .into_iter()
        .map(|owner| owner.join("ffmpeg-snapshot/ios/include/libavutil"))
        .find(|path| path.is_dir())
        .expect("immutable FFmpeg snapshot include directory");
    let preserved = fixture
        .directory
        .path()
        .join("preserved snapshot include directory");
    fs::rename(&snapshot_directory, &preserved)
        .expect("preserve immutable FFmpeg snapshot directory");
    fs::create_dir(&snapshot_directory).expect("replace immutable FFmpeg snapshot directory");
    fs::copy(
        preserved.join("avutil.h"),
        snapshot_directory.join("avutil.h"),
    )
    .expect("copy unchanged content into replacement snapshot directory");
    fs::write(&release, b"release\n").expect("release immutable snapshot directory gate");
    let result = process
        .wait_with_output()
        .expect("wait for immutable snapshot directory test release");
    assert_eq!(result.status.code(), Some(3));
    assert!(result.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("immutable FFmpeg release snapshot changed"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_no_remux_plugin_release_outputs(&fixture, &release_output);
    assert!(!ios_plugin_release_journal(&fixture).exists());
    assert_no_ios_plugin_release_staging(&fixture, &release_output);
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffmpeg_plugin_release_rejects_a_raw_marker_fingerprint_mismatch() {
    let fixture = ios_plugin_fixture();
    configure_ios_plugin_ffmpeg_fixture(&fixture);
    let release_output = fixture.directory.path().join("raw marker mismatch output");
    seed_ios_ffmpeg_runtime_profiles(
        &fixture,
        &release_output,
        &["ios-arm64", "ios-simulator-arm64"],
    );
    let ready = fixture.directory.path().join("raw-marker.ready");
    let release = fixture.directory.path().join("raw-marker.release");
    let ready_argument = ready.to_str().expect("UTF-8 raw marker ready path");
    let release_argument = release.to_str().expect("UTF-8 raw marker gate");
    let mut command = ios_ffmpeg_remux_release_command(
        &fixture,
        &release_output,
        &[
            ("VESPER_SKIP_IOS_FFMPEG_RUNTIME_STAGE", "1"),
            ("VESPER_TEST_PLUGIN_RAW_OUTPUT_READY", ready_argument),
            ("VESPER_TEST_PLUGIN_RAW_OUTPUT_RELEASE", release_argument),
        ],
    );
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut process = command
        .spawn()
        .expect("spawn raw marker fingerprint test release");
    if !wait_for_fixture_marker(&ready) {
        let _ = process.kill();
        let output = process
            .wait_with_output()
            .expect("wait after raw marker gate timeout");
        panic!(
            "iOS plugin release did not reach the raw output gate; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let marker_path = ios_plugin_release_staging_entries(&fixture, &release_output)
        .into_iter()
        .map(|owner| owner.join("raw/.vesper-ios-plugin-output"))
        .find(|path| path.is_file())
        .expect("raw FFmpeg plugin output marker");
    let mut marker: serde_json::Value = serde_json::from_slice(
        &fs::read(&marker_path).expect("read raw FFmpeg plugin output marker"),
    )
    .expect("parse raw FFmpeg plugin output marker");
    let fingerprint_length = marker["ffmpeg_inputs"][0]["input_fingerprint"]
        .as_str()
        .expect("raw marker input fingerprint")
        .len();
    marker["ffmpeg_inputs"][0]["input_fingerprint"] =
        serde_json::Value::String("0".repeat(fingerprint_length));
    fs::write(
        &marker_path,
        serde_json::to_vec(&marker).expect("serialize mismatched raw output marker"),
    )
    .expect("write mismatched raw output marker");
    fs::write(&release, b"release\n").expect("release raw marker gate");
    let result = process
        .wait_with_output()
        .expect("wait for raw marker fingerprint test release");
    assert_eq!(result.status.code(), Some(5));
    assert!(result.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("raw output does not match its immutable FFmpeg snapshot"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_no_remux_plugin_release_outputs(&fixture, &release_output);
    assert!(!ios_plugin_release_journal(&fixture).exists());
    assert_no_ios_plugin_release_staging(&fixture, &release_output);
}

#[cfg(target_os = "macos")]
#[test]
fn ios_plugin_builder_atomically_publishes_the_simulator_layout() {
    let fixture = ios_plugin_fixture();
    let output = fixture.directory.path().join("插件 output with spaces");
    let legacy_device = output.join("iphoneos");
    fs::create_dir_all(&legacy_device).expect("create legacy iOS plugin output");
    fs::write(
        legacy_device.join("libvesper_frame_processor_diagnostic.dylib"),
        b"legacy plugin\n",
    )
    .expect("write legacy iOS plugin library");

    let result = run_ios_plugin_fixture(
        &fixture,
        &[
            output.to_str().expect("UTF-8 iOS plugin output"),
            "release",
            "ios-simulator-arm64",
        ],
        &[],
    );
    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stderr.is_empty());
    assert!(String::from_utf8_lossy(&result.stdout).contains("Built iOS"));
    assert!(!output.join("iphoneos").exists());
    let dylib = "libvesper_frame_processor_diagnostic.dylib";
    assert_eq!(
        fs::read(output.join("iphonesimulator").join(dylib))
            .expect("read flat Simulator plugin library"),
        b"fixture iOS plugin\n"
    );
    assert_eq!(
        fs::read(
            output
                .join("iphonesimulator/aarch64-apple-ios-sim")
                .join(dylib)
        )
        .expect("read target-specific Simulator plugin library"),
        b"fixture iOS plugin\n"
    );
    let root_entries = fs::read_dir(&output)
        .expect("read iOS plugin output")
        .map(|entry| entry.expect("iOS plugin output entry").file_name())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        root_entries,
        std::collections::BTreeSet::from([
            std::ffi::OsString::from(".vesper-ios-plugin-output"),
            std::ffi::OsString::from("iphonesimulator"),
        ])
    );
    let log = fs::read_to_string(&fixture.log).expect("read iOS plugin tool log");
    assert!(log.contains("--target aarch64-apple-ios-sim"));
    assert!(log.contains("--release"));
    assert!(
        log.contains("install_name_tool:-id @rpath/libvesper_frame_processor_diagnostic.dylib")
    );
    assert!(log.contains(" -verify_arch arm64"));
}

#[cfg(target_os = "macos")]
#[test]
fn ios_plugin_builder_preserves_the_previous_output_when_cargo_fails() {
    let fixture = ios_plugin_fixture();
    let output = fixture.directory.path().join("previous plugin output");
    seed_owned_frame_processor_output(&output);

    let result = run_ios_plugin_fixture(
        &fixture,
        &[
            output.to_str().expect("UTF-8 iOS plugin output"),
            "debug",
            "ios-arm64",
        ],
        &[("VESPER_TEST_PLUGIN_CARGO_STATUS", "1")],
    );
    assert_eq!(result.status.code(), Some(5));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Cargo build"));
    assert_eq!(
        fs::read(output.join("iphoneos/libvesper_frame_processor_diagnostic.dylib"))
            .expect("read preserved iOS plugin library"),
        b"previous plugin\n"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_plugin_builder_refuses_to_replace_an_unowned_repository_directory() {
    let fixture = ios_plugin_fixture();
    let source_directory = fixture.root.join("crates");
    fs::create_dir(&source_directory).expect("create protected repository directory");
    fs::write(
        source_directory.join("source.rs"),
        b"pub fn retained() {}\n",
    )
    .expect("write protected repository source");

    let result = run_ios_plugin_fixture(
        &fixture,
        &[
            source_directory
                .to_str()
                .expect("UTF-8 protected repository directory"),
            "debug",
            "ios-arm64",
        ],
        &[],
    );

    assert_eq!(result.status.code(), Some(4));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("unowned iOS plugin output"));
    assert_eq!(
        fs::read(source_directory.join("source.rs")).expect("read protected repository source"),
        b"pub fn retained() {}\n"
    );
    assert!(
        !fixture.log.exists(),
        "Cargo must not start for an unowned output"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffmpeg_plugin_builder_preserves_raw_profile_and_provenance_semantics() {
    let fixture = ios_plugin_fixture();
    configure_ios_plugin_ffmpeg_fixture(&fixture);
    let remux_output = fixture.directory.path().join("remux raw output");
    let normalizer_output = fixture.directory.path().join("normalizer raw output");
    let skip_prebuilds = [("VESPER_SKIP_APPLE_FFMPEG_PREBUILDS", "1")];

    let remux = run_ios_plugin_fixture_for(
        &fixture,
        "remux-plugin",
        "libvesper_remux_ffmpeg.dylib",
        &[
            remux_output.to_str().expect("UTF-8 remux output"),
            "release",
            "ios-arm64",
        ],
        &skip_prebuilds,
    );
    assert_eq!(
        remux.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&remux.stderr)
    );
    let remux_stdout = String::from_utf8(remux.stdout).expect("UTF-8 remux output");
    assert!(remux_stdout.contains("FFmpeg profile:\n  legacy"));
    assert!(
        remux_output
            .join("iphoneos/libvesper_remux_ffmpeg.dylib")
            .is_file()
    );

    let normalizer = run_ios_plugin_fixture_for(
        &fixture,
        "source-normalizer-plugin",
        "libvesper_source_normalizer_ffmpeg.dylib",
        &[
            normalizer_output
                .to_str()
                .expect("UTF-8 source normalizer output"),
            "debug",
            "ios-simulator-arm64",
        ],
        &skip_prebuilds,
    );
    assert_eq!(
        normalizer.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&normalizer.stderr)
    );
    let normalizer_stdout =
        String::from_utf8(normalizer.stdout).expect("UTF-8 source normalizer output");
    assert!(normalizer_stdout.contains("FFmpeg profile:\n  legacy"));
    assert!(
        normalizer_output
            .join("iphonesimulator/libvesper_source_normalizer_ffmpeg.dylib")
            .is_file()
    );

    let log = fs::read_to_string(&fixture.log).expect("read FFmpeg plugin tool log");
    assert!(
        log.contains("ffmpeg_dir:")
            && log.contains("third_party/ffmpeg/apple/ios")
            && log.contains("third_party/ffmpeg/apple/ios-simulator")
    );
    assert!(log.contains("cargo_target_dir:"));
    assert!(log.contains("player-remux-ffmpeg-ios"));
    assert!(log.contains("player-source-normalizer-ffmpeg-ios"));
    let fingerprinted_target_lines = log
        .lines()
        .filter(|line| line.starts_with("cargo_target_dir:"))
        .collect::<Vec<_>>();
    assert_eq!(fingerprinted_target_lines.len(), 2);
    assert!(fingerprinted_target_lines.iter().all(|line| {
        line.rsplit('/').next().is_some_and(|fingerprint| {
            fingerprint.len() == 129 && fingerprint.as_bytes()[64] == b'-'
        })
    }));
    let headerpad_lines = log
        .lines()
        .filter(|line| line.contains("headerpad_max_install_names"))
        .collect::<Vec<_>>();
    assert_eq!(headerpad_lines.len(), 1);
    assert!(headerpad_lines[0].starts_with("rustflags:"));
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffmpeg_plugin_builder_rejects_raw_profile_hash_mismatch() {
    let fixture = ios_plugin_fixture();
    configure_ios_plugin_ffmpeg_fixture(&fixture);
    rewrite_ios_ffmpeg_fixture_profile_hash(
        &fixture,
        "third_party/ffmpeg/apple",
        "custom-e1eabc3db7bc",
    );
    let output = fixture.directory.path().join("mismatched raw output");
    let result = run_ios_plugin_fixture_for(
        &fixture,
        "remux-plugin",
        "libvesper_remux_ffmpeg.dylib",
        &[
            output.to_str().expect("UTF-8 mismatched raw output"),
            "debug",
            "ios-arm64",
        ],
        &[("VESPER_SKIP_APPLE_FFMPEG_PREBUILDS", "1")],
    );
    assert_eq!(result.status.code(), Some(5));
    assert!(result.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("profile_hash"),
        "unexpected stderr: {stderr}"
    );
    assert!(
        stderr.contains("expected 'legacy'"),
        "unexpected stderr: {stderr}"
    );
    assert!(!output.exists());
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffmpeg_plugin_builder_prefers_the_apple_profile_over_the_generic_profile() {
    let fixture = ios_plugin_fixture();
    configure_ios_plugin_ffmpeg_fixture(&fixture);
    let output = fixture
        .directory
        .path()
        .join("Apple scalar precedence output");
    let arguments = [
        output.to_str().expect("UTF-8 iOS plugin output"),
        "debug",
        "ios-arm64",
    ];
    let skip_prebuilds = [("VESPER_SKIP_APPLE_FFMPEG_PREBUILTS", "1")];
    let mut command = ios_plugin_fixture_command(
        &fixture,
        "remux-plugin",
        "libvesper_remux_ffmpeg.dylib",
        &arguments,
        &skip_prebuilds,
    );
    let result = command
        .env("VESPER_FFMPEG_PROFILE", "remux-local")
        .env("VESPER_APPLE_FFMPEG_PROFILE", "legacy")
        .env("VESPER_SKIP_APPLE_FFMPEG_PREBUILDS", "1")
        .output()
        .expect("run Apple scalar precedence fixture");

    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("FFmpeg profile:\n  legacy"));
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffmpeg_plugin_builder_uses_the_rust_native_prebuild_contract() {
    let fixture = ios_plugin_fixture();
    configure_ios_plugin_ffmpeg_fixture(&fixture);
    configure_ios_plugin_native_ffmpeg_build_fixture(&fixture);
    let sdk = fixture.root.join("fake Apple SDK");
    let clang = fixture.tools.join("clang");
    let sdk = sdk.to_str().expect("UTF-8 fake Apple SDK");
    let clang = clang.to_str().expect("UTF-8 fake Apple clang");
    let output = fixture.directory.path().join("canonical prebuild output");
    let result = run_ios_plugin_fixture_for(
        &fixture,
        "remux-plugin",
        "libvesper_remux_ffmpeg.dylib",
        &[
            output.to_str().expect("UTF-8 canonical plugin output"),
            "debug",
            "--profile",
            "custom",
            "--tls-backend",
            "none",
            "--enable-protocols",
            "data,file",
            "--disable-dash",
            "ios-arm64",
        ],
        &[
            ("VESPER_FFMPEG_PROFILE", "remux-local"),
            ("VESPER_APPLE_FFMPEG_PROFILE", "legacy"),
            ("VESPER_FFMPEG_TLS_BACKEND", "none"),
            ("VESPER_APPLE_FFMPEG_TLS_BACKEND", "securetransport"),
            ("VESPER_FFMPEG_ENABLE_DASH", "0"),
            ("VESPER_APPLE_FFMPEG_ENABLE_DASH", "1"),
            ("VESPER_FFMPEG_ENABLE_PROTOCOLS", "file"),
            ("VESPER_APPLE_FFMPEG_ENABLE_PROTOCOLS", "pipe"),
            ("VESPER_APPLE_FFMPEG_OUTPUT_DIR", "third_party/ffmpeg/apple"),
            ("VESPER_TEST_APPLE_SDK_PATH", sdk),
            ("VESPER_TEST_APPLE_CLANG_PATH", clang),
        ],
    );
    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("Building Apple FFmpeg prebuilt for ios-arm64"),
        "unexpected native prebuild diagnostics: {stderr}"
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("FFmpeg profile:\n  custom"));

    let log = fs::read_to_string(&fixture.log).expect("read canonical prebuild log");
    assert_eq!(log.matches("apple_ffmpeg_configure_begin").count(), 1);
    for entry in [
        "apple_ffmpeg_configure_arg:--enable-network",
        "apple_ffmpeg_configure_arg:--disable-openssl",
        "apple_ffmpeg_configure_arg:--disable-securetransport",
        "apple_ffmpeg_configure_arg:--enable-protocol=file",
        "apple_ffmpeg_configure_arg:--enable-protocol=pipe",
        "apple_ffmpeg_configure_arg:--enable-protocol=data",
        "apple_ffmpeg_make:DESTDIR=",
    ] {
        assert!(
            log.contains(entry),
            "missing native prebuild log entry: {entry}\n{log}"
        );
    }
    let metadata = fs::read_to_string(
        fixture
            .root
            .join("third_party/ffmpeg/apple/ios/vesper-ffmpeg-build-metadata.txt"),
    )
    .expect("read native Apple FFmpeg metadata");
    for record in [
        "profile=custom\n",
        "declared_profile=custom\n",
        "profile_hash=custom-bfb4080caa65\n",
        "tls_backend=none\n",
        "enable_dash=0\n",
        "protocols=file,pipe,data\n",
        "ffmpeg_version=8.1.2\n",
    ] {
        assert!(
            metadata.contains(record),
            "missing native metadata record: {record}\n{metadata}"
        );
    }
    assert!(
        output
            .join("iphoneos/libvesper_remux_ffmpeg.dylib")
            .is_file()
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffmpeg_plugin_builder_rejects_duplicate_slices_before_workers() {
    let fixture = ios_plugin_fixture();
    configure_ios_plugin_ffmpeg_fixture(&fixture);
    let output = fixture.directory.path().join("duplicate slice output");
    let result = run_ios_plugin_fixture_for(
        &fixture,
        "remux-plugin",
        "libvesper_remux_ffmpeg.dylib",
        &[
            output.to_str().expect("UTF-8 duplicate slice output"),
            "debug",
            "ios-arm64",
            "ios-arm64",
        ],
        &[],
    );
    assert_eq!(result.status.code(), Some(5));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Duplicate FFmpeg iOS slice"));
    assert!(
        !fixture.log.exists(),
        "prebuild or Cargo started for duplicate slices"
    );
    assert!(!output.exists());
}

#[cfg(target_os = "macos")]
#[test]
fn ios_plugin_builder_rejects_a_concurrent_build_before_starting_cargo() {
    use std::time::{Duration, Instant};

    let fixture = ios_plugin_fixture();
    let first_output = fixture.directory.path().join("first plugin output");
    let second_output = fixture.directory.path().join("second plugin output");
    let ready = fixture.directory.path().join("cargo.ready");
    let release = fixture.directory.path().join("cargo.release");
    let first_arguments = [
        first_output.to_str().expect("UTF-8 first plugin output"),
        "debug",
        "ios-arm64",
    ];
    let first_environment = [
        (
            "VESPER_TEST_PLUGIN_CARGO_READY",
            ready.to_str().expect("UTF-8 Cargo ready path"),
        ),
        (
            "VESPER_TEST_PLUGIN_CARGO_RELEASE",
            release.to_str().expect("UTF-8 Cargo release path"),
        ),
    ];
    let mut first_command = ios_plugin_fixture_command(
        &fixture,
        "frame-processor-plugin",
        "libvesper_frame_processor_diagnostic.dylib",
        &first_arguments,
        &first_environment,
    );
    first_command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let first = first_command.spawn().expect("spawn first iOS plugin build");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready.is_file() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        ready.is_file(),
        "first iOS plugin Cargo process did not start"
    );

    let second = run_ios_plugin_fixture(
        &fixture,
        &[
            second_output.to_str().expect("UTF-8 second plugin output"),
            "debug",
            "ios-arm64",
        ],
        &[],
    );
    assert_eq!(second.status.code(), Some(4));
    assert!(second.stdout.is_empty());
    assert!(String::from_utf8_lossy(&second.stderr).contains("already active"));
    assert!(!second_output.exists());

    fs::write(&release, b"release\n").expect("release first iOS plugin Cargo process");
    let first = first
        .wait_with_output()
        .expect("wait for first iOS plugin build");
    assert_eq!(
        first.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(first_output.join("iphoneos").is_dir());
    let cargo_build_count = fs::read_to_string(&fixture.log)
        .expect("read concurrent iOS plugin tool log")
        .lines()
        .filter(|line| line.starts_with("cargo:"))
        .count();
    assert_eq!(cargo_build_count, 1);
}

#[cfg(target_os = "macos")]
#[test]
fn ios_plugin_builder_cancellation_preserves_the_previous_output() {
    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    use std::time::{Duration, Instant};

    let fixture = ios_plugin_fixture();
    let output = fixture.directory.path().join("cancelled plugin output");
    seed_owned_frame_processor_output(&output);
    let ready = fixture.directory.path().join("cancel-cargo.ready");
    let release = fixture.directory.path().join("cancel-cargo.release");
    let arguments = [
        output.to_str().expect("UTF-8 cancelled plugin output"),
        "debug",
        "ios-arm64",
    ];
    let environment = [
        (
            "VESPER_TEST_PLUGIN_CARGO_READY",
            ready.to_str().expect("UTF-8 cancellation ready path"),
        ),
        (
            "VESPER_TEST_PLUGIN_CARGO_RELEASE",
            release.to_str().expect("UTF-8 cancellation release path"),
        ),
    ];
    let mut command = ios_plugin_fixture_command(
        &fixture,
        "frame-processor-plugin",
        "libvesper_frame_processor_diagnostic.dylib",
        &arguments,
        &environment,
    );
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let child = command.spawn().expect("spawn cancellable iOS plugin build");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready.is_file() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        ready.is_file(),
        "cancellable iOS plugin Cargo process did not start"
    );
    let cargo_pid = fs::read_to_string(&ready)
        .expect("read iOS plugin Cargo pid")
        .parse::<i32>()
        .expect("parse iOS plugin Cargo pid");
    kill(
        Pid::from_raw(i32::try_from(child.id()).expect("CLI pid fits i32")),
        Signal::SIGINT,
    )
    .expect("cancel iOS plugin build");
    let result = child
        .wait_with_output()
        .expect("wait for cancelled iOS plugin build");
    assert_eq!(result.status.code(), Some(6));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("cancelled"));
    assert_eq!(
        fs::read(output.join("iphoneos/libvesper_frame_processor_diagnostic.dylib"))
            .expect("read preserved plugin library"),
        b"previous plugin\n"
    );
    let reap_deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match kill(Pid::from_raw(cargo_pid), None) {
            Err(Errno::ESRCH) => break,
            _ if Instant::now() < reap_deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            result => panic!("iOS plugin Cargo process is still alive: {result:?}"),
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffi_public_wrapper_routes_to_the_rust_cli() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = ios_ffi_fixture();
    let wrapper = fixture.root.join("scripts/vesper");
    fs::create_dir_all(wrapper.parent().expect("wrapper parent")).expect("create wrapper parent");
    fs::copy(workspace_root().join("scripts/vesper"), &wrapper).expect("copy Vesper wrapper");
    let mut permissions = fs::metadata(&wrapper)
        .expect("inspect Vesper wrapper")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&wrapper, permissions).expect("make Vesper wrapper executable");
    let path = format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", fixture.tools.display());

    let result = Command::new(&wrapper)
        .current_dir(&fixture.directory)
        .env("PATH", path)
        .env("VESPER_CLI", env!("CARGO_BIN_EXE_vesper"))
        .env("VESPER_TEST_TOOL_LOG", &fixture.log)
        .env("VESPER_TEST_RUST_TARGET_LIBDIR", &fixture.target_libdir)
        .env("VESPER_TEST_ARTIFACT_ROOT", &fixture.artifact_root)
        .args(["ios", "ffi", "debug"])
        .output()
        .expect("run Vesper wrapper for iOS FFI");
    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        String::from_utf8_lossy(&result.stdout).contains("Built player-ffi Apple artifacts into:")
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_kit_build_replaces_all_public_artifacts_in_one_transaction() {
    let fixture = ios_ffi_fixture();
    configure_ios_kit_fixture(&fixture);
    seed_previous_ios_kit_output(&fixture);

    let result = run_ios_kit_fixture(&fixture, &[]);

    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = ios_kit_output(&fixture);
    let expected_output = fixture
        .root
        .canonicalize()
        .expect("canonical iOS kit fixture root")
        .join("lib/ios/VesperPlayerKit/.build/xcframework/VesperPlayerKit.xcframework");
    let stdout = String::from_utf8(result.stdout).expect("UTF-8 iOS kit output");
    assert!(stdout.contains("Built player-ffi Apple artifacts into:"));
    assert!(stdout.contains("Built VesperPlayerKit XCFramework at:"));
    assert!(stdout.contains(&expected_output.display().to_string()));
    assert!(result.stderr.is_empty());
    for (artifact, required_file) in [
        (
            "VesperPlayerKit-iOS.xcarchive",
            "Products/Library/Frameworks/VesperPlayerKit.framework/VesperPlayerKit",
        ),
        (
            "VesperPlayerKit-iOS-Simulator.xcarchive",
            "Products/Library/Frameworks/VesperPlayerKit.framework/VesperPlayerKit",
        ),
        ("VesperPlayerKit.xcframework", "Info.plist"),
    ] {
        assert!(output.join(artifact).join(required_file).is_file());
        assert!(!output.join(artifact).join("previous.txt").exists());
    }
    let mut artifact_names = fs::read_dir(&output)
        .expect("inspect published iOS kit output")
        .map(|entry| entry.expect("read published iOS kit artifact").file_name())
        .collect::<Vec<_>>();
    artifact_names.sort();
    assert_eq!(
        artifact_names,
        [
            "VesperPlayerKit-iOS-Simulator.xcarchive",
            "VesperPlayerKit-iOS.xcarchive",
            "VesperPlayerKit.xcframework",
        ]
        .map(std::ffi::OsString::from)
    );

    let log = fs::read_to_string(&fixture.log).expect("read iOS kit fake tool log");
    assert!(log.contains("xcodegen:generate --spec"));
    assert!(log.contains("archive -project"));
    assert!(log.contains("-sdk iphoneos"));
    assert!(log.contains("-sdk iphonesimulator"));
    assert!(log.contains("VESPER_IOS_FFI_PREBUILT=1"));
    assert!(log.contains("xcodebuild:-create-xcframework"));
    assert!(log.contains("xcrun lipo -archs"));
    assert!(log.contains("xcrun plutil -convert json -o -"));
}

#[cfg(target_os = "macos")]
#[test]
fn ios_kit_build_failure_preserves_all_previous_public_artifacts() {
    let fixture = ios_ffi_fixture();
    configure_ios_kit_fixture(&fixture);
    seed_previous_ios_kit_output(&fixture);

    let result = run_ios_kit_fixture(
        &fixture,
        &[("VESPER_TEST_XCODEBUILD_FAIL_SDK", "iphonesimulator")],
    );

    assert_eq!(result.status.code(), Some(5));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("iphonesimulator archive build"));
    let output = ios_kit_output(&fixture);
    for artifact in [
        "VesperPlayerKit-iOS.xcarchive",
        "VesperPlayerKit-iOS-Simulator.xcarchive",
        "VesperPlayerKit.xcframework",
    ] {
        assert_eq!(
            fs::read(output.join(artifact).join("previous.txt"))
                .expect("read preserved iOS kit artifact"),
            b"previous iOS kit output\n"
        );
    }
    assert!(
        fs::read_dir(output.parent().expect("iOS kit output parent"))
            .expect("inspect iOS kit output parent")
            .filter_map(Result::ok)
            .all(|entry| !entry
                .file_name()
                .to_string_lossy()
                .starts_with(".vesper-ios-kit-stage-"))
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_kit_rejects_non_arm64_simulator_architecture_before_writing() {
    let fixture = ios_ffi_fixture();
    seed_previous_ios_kit_output(&fixture);

    let result = run_ios_kit_fixture(&fixture, &[("VESPER_IOS_SIMULATOR_ARCHS", "x86_64")]);

    assert_eq!(result.status.code(), Some(4));
    assert!(result.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("unsupported iOS simulator architecture: x86_64")
    );
    assert!(!fixture.log.exists());
    let output = ios_kit_output(&fixture);
    for artifact in [
        "VesperPlayerKit-iOS.xcarchive",
        "VesperPlayerKit-iOS-Simulator.xcarchive",
        "VesperPlayerKit.xcframework",
    ] {
        assert!(output.join(artifact).join("previous.txt").is_file());
    }
}

#[cfg(target_os = "macos")]
#[test]
fn ios_kit_rejects_unusable_xcframeworks_before_publication() {
    for (variable, value, expected_diagnostic) in [
        (
            "VESPER_TEST_XCODEBUILD_MALFORMED_XCFRAMEWORK",
            "1",
            "XCFramework manifest parsing",
        ),
        (
            "VESPER_TEST_LIPO_ARCHS",
            "x86_64",
            "must contain only arm64",
        ),
    ] {
        let fixture = ios_ffi_fixture();
        configure_ios_kit_fixture(&fixture);
        seed_previous_ios_kit_output(&fixture);

        let result = run_ios_kit_fixture(&fixture, &[(variable, value)]);

        assert_eq!(
            result.status.code(),
            Some(5),
            "{variable}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.stdout.is_empty(), "{variable}");
        assert!(
            String::from_utf8_lossy(&result.stderr).contains(expected_diagnostic),
            "{variable}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let output = ios_kit_output(&fixture);
        for artifact in [
            "VesperPlayerKit-iOS.xcarchive",
            "VesperPlayerKit-iOS-Simulator.xcarchive",
            "VesperPlayerKit.xcframework",
        ] {
            assert!(
                output.join(artifact).join("previous.txt").is_file(),
                "{variable}: {artifact}"
            );
        }
    }
}

#[test]
fn ios_kit_keeps_typed_help_and_usage_contracts() {
    let help = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "kit-xcframework", "--help"])
        .output()
        .expect("show iOS kit help");
    assert_eq!(help.status.code(), Some(0));
    assert!(help.stderr.is_empty());
    assert!(String::from_utf8_lossy(&help.stdout).contains("ios kit-xcframework"));

    let invalid = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "kit-xcframework", "extra"])
        .output()
        .expect("reject invalid iOS kit arguments");
    assert_eq!(invalid.status.code(), Some(2));
    assert!(invalid.stdout.is_empty());
}

#[cfg(target_os = "macos")]
#[test]
fn ios_kit_public_wrapper_routes_to_the_rust_cli() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = ios_ffi_fixture();
    configure_ios_kit_fixture(&fixture);
    let wrapper = fixture.root.join("scripts/vesper");
    fs::create_dir_all(wrapper.parent().expect("iOS kit wrapper parent"))
        .expect("create iOS kit wrapper parent");
    fs::copy(workspace_root().join("scripts/vesper"), &wrapper)
        .expect("copy Vesper wrapper for iOS kit");
    let mut permissions = fs::metadata(&wrapper)
        .expect("inspect iOS kit Vesper wrapper")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&wrapper, permissions).expect("make iOS kit wrapper executable");
    let path = format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", fixture.tools.display());

    let result = Command::new(&wrapper)
        .current_dir(&fixture.directory)
        .env("PATH", path)
        .env("VESPER_CLI", env!("CARGO_BIN_EXE_vesper"))
        .env("VESPER_TEST_TOOL_LOG", &fixture.log)
        .env("VESPER_TEST_RUST_TARGET_LIBDIR", &fixture.target_libdir)
        .env("VESPER_TEST_ARTIFACT_ROOT", &fixture.artifact_root)
        .args(["ios", "kit-xcframework"])
        .output()
        .expect("run Vesper wrapper for iOS kit");

    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        String::from_utf8_lossy(&result.stdout).contains("Built VesperPlayerKit XCFramework at:")
    );
    assert!(
        ios_kit_output(&fixture)
            .join("VesperPlayerKit.xcframework/Info.plist")
            .is_file()
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_kit_lock_and_ctrl_c_reap_the_xcode_process_group_and_preserve_output() {
    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let fixture = ios_ffi_fixture();
    configure_ios_kit_fixture(&fixture);
    seed_previous_ios_kit_output(&fixture);
    let xcodebuild_pid_path = fixture.directory.path().join("ios-kit-xcodebuild.pid");
    let descendant_pid_path = fixture
        .directory
        .path()
        .join("ios-kit-xcodebuild-descendant.pid");
    let first_tmp = fixture.directory.path().join("first tmp");
    let second_tmp = fixture.directory.path().join("second tmp");
    fs::create_dir_all(&first_tmp).expect("create first iOS kit temporary directory");
    fs::create_dir_all(&second_tmp).expect("create second iOS kit temporary directory");

    let mut first = Command::new(env!("CARGO_BIN_EXE_vesper"));
    first
        .current_dir(&fixture.directory)
        .env("PATH", &fixture.tools)
        .env("VESPER_TEST_TOOL_LOG", &fixture.log)
        .env("VESPER_TEST_RUST_TARGET_LIBDIR", &fixture.target_libdir)
        .env("VESPER_TEST_ARTIFACT_ROOT", &fixture.artifact_root)
        .env("VESPER_TEST_XCODEBUILD_WAIT_SDK", "iphonesimulator")
        .env("VESPER_TEST_XCODEBUILD_PID_FILE", &xcodebuild_pid_path)
        .env(
            "VESPER_TEST_XCODEBUILD_DESCENDANT_PID_FILE",
            &descendant_pid_path,
        )
        .env("TMPDIR", &first_tmp)
        .args(["ios", "--root"])
        .arg(&fixture.root)
        .arg("kit-xcframework")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let first = first.spawn().expect("start cancellable iOS kit build");
    let first_pid = i32::try_from(first.id()).expect("Vesper iOS kit parent pid");

    let ready_deadline = Instant::now() + Duration::from_secs(15);
    while (!xcodebuild_pid_path.is_file() || !descendant_pid_path.is_file())
        && Instant::now() < ready_deadline
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    if !xcodebuild_pid_path.is_file() || !descendant_pid_path.is_file() {
        let _ = kill(Pid::from_raw(first_pid), Signal::SIGINT);
        let result = first
            .wait_with_output()
            .expect("wait after iOS kit readiness failure");
        panic!(
            "iOS kit Xcode process group was not ready; stderr: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let xcodebuild_pid: i32 = fs::read_to_string(&xcodebuild_pid_path)
        .expect("read iOS kit Xcode pid")
        .parse()
        .expect("parse iOS kit Xcode pid");
    let descendant_pid: i32 = fs::read_to_string(&descendant_pid_path)
        .expect("read iOS kit Xcode descendant pid")
        .parse()
        .expect("parse iOS kit Xcode descendant pid");

    let second_tmp_text = second_tmp.to_string_lossy().into_owned();
    let second = run_ios_kit_fixture(&fixture, &[("TMPDIR", &second_tmp_text)]);
    assert_eq!(second.status.code(), Some(4));
    assert!(second.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&second.stderr).contains("another iOS kit build is already active")
    );
    let competing_ffi = run_ios_ffi_fixture(&fixture, &["ffi", "release"], &[]);
    assert_eq!(competing_ffi.status.code(), Some(4));
    assert!(competing_ffi.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&competing_ffi.stderr)
            .contains("another iOS FFI build is already active")
    );

    kill(Pid::from_raw(first_pid), Signal::SIGINT).expect("interrupt iOS kit build");
    let result = first
        .wait_with_output()
        .expect("wait for cancelled iOS kit build");
    assert_eq!(
        result.status.code(),
        Some(6),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("cancelled"));
    let output = ios_kit_output(&fixture);
    for artifact in [
        "VesperPlayerKit-iOS.xcarchive",
        "VesperPlayerKit-iOS-Simulator.xcarchive",
        "VesperPlayerKit.xcframework",
    ] {
        assert!(output.join(artifact).join("previous.txt").is_file());
    }

    for pid in [xcodebuild_pid, descendant_pid] {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match kill(Pid::from_raw(pid), None) {
                Err(Errno::ESRCH) => break,
                Ok(()) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                status => panic!("iOS kit process {pid} survived cancellation: {status:?}"),
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
#[test]
fn ios_ffi_rejects_non_macos_before_root_resolution_or_writes() {
    let directory = tempfile::tempdir().expect("temporary non-macOS iOS FFI fixture");
    let root = directory.path().join("missing repository");
    let result = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("TMPDIR", directory.path())
        .args(["ios", "--root"])
        .arg(&root)
        .args(["ffi", "release"])
        .output()
        .expect("run non-macOS iOS FFI command");
    assert_eq!(result.status.code(), Some(4));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("requires macOS"));
    assert!(!root.exists());
}

#[cfg(target_os = "macos")]
fn write_invalid_optional_ios_release_assets(release: &std::path::Path) {
    for asset in [
        "VesperFFmpegAVCodec.xcframework.zip",
        "VesperFFmpegAVFormat.xcframework.zip",
        "VesperFFmpegAVUtil.xcframework.zip",
        "VesperPlayerRemuxFfmpegPlugin.xcframework.zip",
        "VesperPlayerSourceNormalizerFfmpegPlugin.xcframework.zip",
        "VesperPlayerDecoderVideoToolboxPlugin.xcframework.zip",
        "VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip",
        "VesperPlayerOptionalPlugins-FFmpeg-Compliance.zip",
        "VesperPlayerOptionalPlugins-FFmpeg-8.1.2-source.tar.xz",
    ] {
        fs::write(release.join(asset), b"invalid preflight fixture\n")
            .expect("write malformed optional iOS release asset");
    }
}

const FLUTTER_CORE_PACKAGES: [&str; 6] = [
    "vesper_player_platform_interface",
    "vesper_player_android",
    "vesper_player_ios",
    "vesper_player",
    "vesper_player_external_playback",
    "vesper_player_ui",
];
const FLUTTER_OPTIONAL_PACKAGE: &str = "vesper_player_source_normalizer_ffmpeg";

fn flutter_fixture(include_example: bool) -> tempfile::TempDir {
    let parent = tempfile::tempdir().expect("temporary Flutter fixture parent");
    let root = parent.path().join("Flutter fixture with spaces - 测试");
    fs::create_dir_all(&root).expect("create Flutter fixture root");
    fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("write workspace manifest");
    for package in FLUTTER_CORE_PACKAGES
        .iter()
        .copied()
        .chain(std::iter::once(FLUTTER_OPTIONAL_PACKAGE))
    {
        let package_directory = root.join("lib/flutter").join(package);
        fs::create_dir_all(&package_directory).expect("create Flutter package directory");
        fs::write(
            package_directory.join("pubspec.yaml"),
            format!("name: {package}\n"),
        )
        .expect("write Flutter package pubspec");
    }
    if include_example {
        let example = root.join("examples/flutter-host");
        fs::create_dir_all(&example).expect("create Flutter example directory");
        fs::write(example.join("pubspec.yaml"), "name: flutter_host\n")
            .expect("write Flutter example pubspec");
    }
    parent
}

fn flutter_fixture_root(directory: &tempfile::TempDir) -> PathBuf {
    directory.path().join("Flutter fixture with spaces - 测试")
}

fn prepare_flutter_pub_fixture(root: &std::path::Path) {
    fs::write(root.join("LICENSE"), "fixture license\n").expect("write Flutter fixture license");
    for package in FLUTTER_CORE_PACKAGES
        .iter()
        .copied()
        .chain(std::iter::once(FLUTTER_OPTIONAL_PACKAGE))
    {
        let package_directory = root.join("lib/flutter").join(package);
        fs::write(
            package_directory.join("pubspec.yaml"),
            format!(
                "name: {package}\npublish_to: none\nversion: 0.4.0\nhomepage: https://example.invalid/old\nrepository: https://example.invalid/repository\nissue_tracker: https://example.invalid/issues\ndependencies:\n  vesper_player_platform_interface:\n    path: ../vesper_player_platform_interface\n"
            ),
        )
        .expect("write publishable Flutter pubspec");
        fs::write(package_directory.join("LICENSE"), "package-local license\n")
            .expect("write package-local license");
        fs::create_dir_all(package_directory.join("lib/src")).expect("create staged Dart source");
        fs::write(
            package_directory.join("lib/src/fixture.dart"),
            format!("const packageName = '{package}';\n"),
        )
        .expect("write staged Dart source");
    }
}

fn prepare_optional_ios_pub_fixture(root: &std::path::Path) {
    const ARTIFACTS: [&str; 7] = [
        "VesperFFmpegAVCodec",
        "VesperFFmpegAVFormat",
        "VesperFFmpegAVUtil",
        "VesperPlayerRemuxFfmpegPlugin",
        "VesperPlayerSourceNormalizerFfmpegPlugin",
        "VesperPlayerDecoderVideoToolboxPlugin",
        "VesperPlayerFrameProcessorDiagnosticPlugin",
    ];
    let package = root.join("lib/ios/VesperPlayerOptionalPlugins");
    fs::create_dir_all(package.join("Sources/FixtureProduct"))
        .expect("create optional iOS product source");
    fs::write(package.join("Package.swift"), "// fixture package\n")
        .expect("write optional iOS package manifest");
    fs::write(
        package.join("Sources/FixtureProduct/Fixture.swift"),
        "public enum Fixture {}\n",
    )
    .expect("write optional iOS product source");
    for artifact in ARTIFACTS {
        let framework = package
            .join("Artifacts")
            .join(format!("{artifact}.xcframework"));
        fs::create_dir_all(&framework).expect("create optional iOS XCFramework");
        fs::write(framework.join("Info.plist"), format!("{artifact}\n"))
            .expect("write optional iOS XCFramework metadata");
    }
    fs::create_dir_all(package.join(".build")).expect("create excluded Swift build directory");
    fs::write(package.join(".build/cache"), "local\n").expect("write excluded Swift build cache");
    fs::create_dir_all(package.join(".swiftpm")).expect("create excluded SwiftPM directory");
}

#[cfg(unix)]
fn ios_bridge_cli_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let directory = tempfile::tempdir().expect("temporary iOS bridge CLI fixture parent");
    let root = directory
        .path()
        .join("iOS bridge fixture with spaces - 测试");
    let manifest_directory = root.join("scripts/ios/bridge-shim");
    let fragment_directory = manifest_directory.join("fragments");
    let shim_parent = root.join("lib/ios/VesperPlayerKit/Sources");
    let rust_source_parent = root.join("crates/ffi/player-ffi-ios/src");
    let tools = root.join("fake iOS tools");
    for path in [
        &fragment_directory,
        &shim_parent,
        &rust_source_parent,
        &tools,
    ] {
        fs::create_dir_all(path).expect("create iOS bridge fixture directory");
    }
    fs::write(root.join("Cargo.toml"), "[workspace]\n")
        .expect("write iOS bridge fixture workspace manifest");
    fs::write(
        manifest_directory.join("manifest.json"),
        r#"{
  "header_guard": "VESPER_PLAYER_KIT_BRIDGE_SHIM_H",
  "header_includes": ["<stdbool.h>"],
  "c_includes": ["\"include/VesperPlayerKitBridgeShim.h\""],
  "public_declarations": [
    {
      "kind": "function",
      "return_type": "bool",
      "name": "vesper_runtime_fixture",
      "parameters": []
    }
  ],
  "private_declarations": [
    {
      "kind": "function",
      "return_type": "bool",
      "name": "player_ffi_fixture",
      "parameters": [],
      "storage": "extern"
    }
  ],
  "source_items": [
    {
      "kind": "wrapper",
      "function": "vesper_runtime_fixture",
      "ffi_symbols": ["player_ffi_fixture"],
      "ownership": [],
      "body_fragment": "fragments/fixture.c.inc"
    }
  ]
}
"#,
    )
    .expect("write iOS bridge fixture manifest");
    fs::write(
        fragment_directory.join("fixture.c.inc"),
        "{\n    return player_ffi_fixture();\n}\n",
    )
    .expect("write iOS bridge fixture fragment");
    fs::write(
        rust_source_parent.join("lib.rs"),
        "#[unsafe(no_mangle)]\npub unsafe extern \"C\" fn player_ffi_fixture() -> bool { true }\n",
    )
    .expect("write iOS bridge fixture Rust source");
    write_executable_script(&tools.join("clang"), "#!/bin/sh\nexit 0\n");
    (directory, root, tools)
}

#[cfg(unix)]
fn ios_bridge_command(root: &std::path::Path, tools: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_vesper"));
    command
        .env("CLANG", tools.join("clang"))
        .env_remove("NM")
        .env_remove("VESPER_IOS_FFI_ARCHIVE")
        .args(["ios", "--root"])
        .arg(root);
    command
}

#[cfg(unix)]
fn sync_ios_bridge_fixture(root: &std::path::Path, tools: &std::path::Path) {
    let output = ios_bridge_command(root, tools)
        .arg("sync-bridge-shim")
        .output()
        .expect("synchronize iOS bridge fixture");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"VesperPlayerKit bridge shim synchronized.\n"
    );
    assert!(output.stderr.is_empty());
}

#[cfg(unix)]
const IOS_APP_STORE_FRAMEWORKS: [&str; 7] = [
    "VesperFFmpegAVCodec",
    "VesperFFmpegAVFormat",
    "VesperFFmpegAVUtil",
    "VesperPlayerRemuxFfmpegPlugin",
    "VesperPlayerSourceNormalizerFfmpegPlugin",
    "VesperPlayerDecoderVideoToolboxPlugin",
    "VesperPlayerFrameProcessorDiagnosticPlugin",
];

#[cfg(unix)]
const IOS_APP_STORE_FFMPEG_FRAMEWORKS: [&str; 5] = [
    "VesperFFmpegAVCodec",
    "VesperFFmpegAVFormat",
    "VesperFFmpegAVUtil",
    "VesperPlayerRemuxFfmpegPlugin",
    "VesperPlayerSourceNormalizerFfmpegPlugin",
];

#[cfg(unix)]
fn ios_app_store_plugin_descriptor(
    framework_name: &str,
) -> Option<player_cli::CanonicalPluginDescriptor> {
    let (plugin_id, capabilities) = match framework_name {
        "VesperPlayerRemuxFfmpegPlugin" => (
            "io.github.ikaros.vesper.remux-ffmpeg",
            vec![player_cli::PluginCapabilityDescriptor {
                interface_id: "e9479dbc-42d2-575e-b39e-a24bc512fbc7".to_owned(),
                instance_id: "io.github.ikaros.vesper.remux-ffmpeg.post-download".to_owned(),
                interface_major: 1,
                interface_minor: 0,
                stability: player_cli::PluginStability::Stable,
            }],
        ),
        "VesperPlayerSourceNormalizerFfmpegPlugin" => (
            "io.github.ikaros.vesper.source-normalizer-ffmpeg",
            vec![
                player_cli::PluginCapabilityDescriptor {
                    interface_id: "a2d653fa-d6ce-5f14-93b8-a818a7a77fdf".to_owned(),
                    instance_id: "io.github.ikaros.vesper.source-normalizer-ffmpeg.packet"
                        .to_owned(),
                    interface_major: 1,
                    interface_minor: 0,
                    stability: player_cli::PluginStability::Experimental,
                },
                player_cli::PluginCapabilityDescriptor {
                    interface_id: "b76d1f06-62d7-5d71-aa06-2780e4b4fd0d".to_owned(),
                    instance_id: "io.github.ikaros.vesper.source-normalizer-ffmpeg.resource"
                        .to_owned(),
                    interface_major: 1,
                    interface_minor: 0,
                    stability: player_cli::PluginStability::Experimental,
                },
            ],
        ),
        "VesperPlayerDecoderVideoToolboxPlugin" => (
            "io.github.ikaros.vesper.decoder-videotoolbox",
            vec![player_cli::PluginCapabilityDescriptor {
                interface_id: "d68be0ed-1958-5922-8b7a-bc6778a26b43".to_owned(),
                instance_id: "io.github.ikaros.vesper.decoder-videotoolbox.native".to_owned(),
                interface_major: 1,
                interface_minor: 0,
                stability: player_cli::PluginStability::Experimental,
            }],
        ),
        "VesperPlayerFrameProcessorDiagnosticPlugin" => (
            "dev.vesper.frame-processor-diagnostic",
            vec![player_cli::PluginCapabilityDescriptor {
                interface_id: "fc050597-b7b7-5c81-83b9-b42555f8b825".to_owned(),
                instance_id: "dev.vesper.frame-processor-diagnostic.frame".to_owned(),
                interface_major: 1,
                interface_minor: 0,
                stability: player_cli::PluginStability::Experimental,
            }],
        ),
        _ => return None,
    };
    let descriptor = player_cli::PluginDescriptor {
        schema_version: 1,
        plugin: player_cli::PluginIdentityDescriptor {
            id: plugin_id.to_owned(),
            name: framework_name.to_owned(),
            version: "0.4.0".to_owned(),
            description: format!("App Store layout fixture for {framework_name}"),
            license: "Apache-2.0".to_owned(),
            publisher: "io.github.ikaros".to_owned(),
        },
        compatibility: player_cli::PluginCompatibilityDescriptor {
            host_sdk: ">=0.4.0, <0.5.0".to_owned(),
            abi_major: 1,
            abi_minor_min: 0,
            abi_minor_max: 0,
        },
        capabilities,
        redistribution: Vec::new(),
    };

    Some(
        descriptor
            .canonicalize()
            .expect("canonicalize App Store plugin descriptor"),
    )
}

#[cfg(unix)]
fn ios_app_store_bundle_identifier(framework_name: &str) -> String {
    format!(
        "io.github.ikaros.fixture.{}",
        framework_name.to_ascii_lowercase()
    )
}

#[cfg(unix)]
fn ios_app_store_cli_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let directory = tempfile::tempdir().expect("temporary iOS App Store fixture parent");
    let root = directory
        .path()
        .join("iOS App Store fixture with spaces - 测试");
    let app = root.join("build/Release iphoneos/Vesper Sample 测试.app");
    let frameworks = app.join("Frameworks");
    let tools = root.join("fake Apple tools");
    fs::create_dir_all(&frameworks).expect("create iOS App Store Frameworks directory");
    fs::create_dir_all(&tools).expect("create fake Apple tools directory");
    fs::write(root.join("Cargo.toml"), "[workspace]\n")
        .expect("write iOS App Store fixture workspace manifest");

    let app_executable_name = "Vesper Sample 测试";
    let app_info = serde_json::json!({
        "CFBundlePackageType": "APPL",
        "CFBundleExecutable": app_executable_name,
        "CFBundleIdentifier": "io.github.ikaros.fixture.vesper-sample",
        "CFBundleName": "Vesper Sample",
        "CFBundleVersion": "1",
        "CFBundleShortVersionString": "0.4.0",
        "MinimumOSVersion": "17.0",
        "CFBundleSupportedPlatforms": ["iPhoneOS"],
        "DTPlatformName": "iphoneos"
    });
    fs::write(
        app.join("Info.plist"),
        serde_json::to_vec_pretty(&app_info).expect("serialize fixture App Info.plist"),
    )
    .expect("write fixture App Info.plist");
    fs::write(
        app.join(".codesign-identifier"),
        "io.github.ikaros.fixture.vesper-sample\n",
    )
    .expect("write fixture App code-signing identity");
    let app_executable = app.join(app_executable_name);
    fs::write(&app_executable, b"fixture App Mach-O\n").expect("write fixture App executable");
    fs::write(format!("{}.dependencies", app_executable.display()), "")
        .expect("write fixture App dependencies");

    let flutter_framework = frameworks.join("App.framework");
    fs::create_dir_all(&flutter_framework).expect("create Flutter App framework");
    fs::write(
        flutter_framework.join("App"),
        b"fixture Flutter AOT binary\n",
    )
    .expect("write Flutter AOT fixture binary");

    for framework_name in IOS_APP_STORE_FRAMEWORKS {
        let framework = frameworks.join(format!("{framework_name}.framework"));
        fs::create_dir_all(&framework).expect("create optional iOS framework");
        let bundle_identifier = ios_app_store_bundle_identifier(framework_name);
        let info = serde_json::json!({
            "CFBundlePackageType": "FMWK",
            "CFBundleExecutable": framework_name,
            "CFBundleIdentifier": bundle_identifier,
            "CFBundleName": framework_name,
            "CFBundleVersion": "1",
            "CFBundleShortVersionString": "0.4.0",
            "MinimumOSVersion": "17.0",
            "CFBundleSupportedPlatforms": ["iPhoneOS"],
            "DTPlatformName": "iphoneos"
        });
        fs::write(
            framework.join("Info.plist"),
            serde_json::to_vec_pretty(&info).expect("serialize fixture Info.plist"),
        )
        .expect("write optional iOS framework Info.plist");
        fs::write(
            framework.join(".codesign-identifier"),
            format!("{bundle_identifier}\n"),
        )
        .expect("write optional framework code-signing identity");
        let binary = framework.join(framework_name);
        fs::write(&binary, b"fixture Mach-O\n").expect("write optional framework binary");
        if let Some(descriptor) = ios_app_store_plugin_descriptor(framework_name) {
            let target = player_cli::EmbeddedRegistryTarget::AppleFramework {
                target: "aarch64-apple-ios".to_owned(),
                architecture: "arm64".to_owned(),
                minimum_os: "17.0".to_owned(),
                framework_name: framework_name.to_owned(),
                bundle_identifier: bundle_identifier.clone(),
            };
            let fragment = player_cli::EmbeddedRegistryFragment::generate(&descriptor, &target)
                .expect("generate optional plugin registry fragment");
            fs::write(
                framework.join("vesper-plugin-registry.json"),
                fragment.canonical_json(),
            )
            .expect("write optional plugin registry fragment");
        }
        let dependencies = match framework_name {
            "VesperFFmpegAVCodec" => vec!["VesperFFmpegAVUtil"],
            "VesperFFmpegAVFormat" => {
                vec!["VesperFFmpegAVCodec", "VesperFFmpegAVUtil"]
            }
            "VesperPlayerRemuxFfmpegPlugin" | "VesperPlayerSourceNormalizerFfmpegPlugin" => vec![
                "VesperFFmpegAVCodec",
                "VesperFFmpegAVFormat",
                "VesperFFmpegAVUtil",
            ],
            _ => Vec::new(),
        };
        fs::write(
            format!("{}.dependencies", binary.display()),
            dependencies
                .into_iter()
                .map(|dependency| format!("@rpath/{dependency}.framework/{dependency}\n"))
                .collect::<String>(),
        )
        .expect("write optional framework dependencies");
    }
    for framework_name in IOS_APP_STORE_FFMPEG_FRAMEWORKS {
        fs::write(
            frameworks
                .join(format!("{framework_name}.framework"))
                .join("profile-hash.txt"),
            "fixture-profile-hash\n",
        )
        .expect("write FFmpeg profile hash");
    }

    write_executable_script(
        &tools.join("plutil"),
        "#!/bin/sh\nfor argument do input=$argument; done\ncat \"$input\"\n",
    );
    write_executable_script(
        &tools.join("otool"),
        r#"#!/bin/sh
mode=$1
binary=$2
name=${binary##*/}
case "$mode" in
  -D)
    printf '%s:\n' "$binary"
    if [ -f "${binary}.install-name" ]; then
      cat "${binary}.install-name"
    else
      printf '@rpath/%s.framework/%s\n' "$name" "$name"
    fi
    ;;
  -L)
    printf '%s:\n' "$binary"
    printf '\t@rpath/%s.framework/%s (compatibility version 1.0.0, current version 1.0.0)\n' "$name" "$name"
    if [ -f "${binary}.dependencies" ]; then
      while IFS= read -r dependency; do
        [ -n "$dependency" ] || continue
        printf '\t%s (compatibility version 1.0.0, current version 1.0.0)\n' "$dependency"
      done < "${binary}.dependencies"
    fi
    printf '\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1.0.0)\n'
    ;;
  *) exit 64 ;;
esac
"#,
    );
    write_executable_script(
        &tools.join("lipo"),
        "#!/bin/sh\nfor argument do binary=$argument; done\nif [ -f \"${binary}.archs\" ]; then cat \"${binary}.archs\"; else printf 'arm64\\n'; fi\n",
    );
    write_executable_script(
        &tools.join("xcrun"),
        "#!/bin/sh\nfor argument do binary=$argument; done\nif [ -f \"${binary}.platform\" ]; then platform=$(cat \"${binary}.platform\"); else platform=IOS; fi\nprintf 'Load command 9\\n      cmd LC_BUILD_VERSION\\n platform %s\\n    minos 17.0\\n' \"$platform\"\n",
    );
    write_executable_script(
        &tools.join("strings"),
        "#!/bin/sh\nfor argument do binary=$argument; done\nif [ -f \"${binary}.strings\" ]; then cat \"${binary}.strings\"; fi\n",
    );
    write_executable_script(
        &tools.join("swift"),
        "#!/bin/sh\n[ -n \"$CLANG_MODULE_CACHE_PATH\" ] || exit 91\n[ \"$CLANG_MODULE_CACHE_PATH\" = \"$SWIFT_MODULECACHE_PATH\" ] || exit 92\n[ -d \"$CLANG_MODULE_CACHE_PATH\" ] || exit 93\nexit 0\n",
    );
    write_executable_script(
        &tools.join("codesign"),
        r#"#!/bin/sh
if [ "$1" = --display ]; then
  for path do :; done
  identifier=$(/bin/cat "$path/.codesign-identifier")
  printf 'Identifier=%s\nTeamIdentifier=ABCDE12345\n' "$identifier" >&2
  exit "${VESPER_FAKE_CODESIGN_STATUS:-0}"
fi
printf '%s\n' "$*" >> "$VESPER_FAKE_CODESIGN_LOG"
exit "${VESPER_FAKE_CODESIGN_STATUS:-0}"
"#,
    );

    (directory, root, app, tools)
}

#[cfg(unix)]
fn ios_app_store_command(
    root: &std::path::Path,
    app: &std::path::Path,
    tools: &std::path::Path,
) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_vesper"));
    command
        .current_dir(root.parent().expect("iOS fixture parent"))
        .env("PLUTIL", tools.join("plutil"))
        .env("OTOOL", tools.join("otool"))
        .env("LIPO", tools.join("lipo"))
        .env("XCRUN", tools.join("xcrun"))
        .env("STRINGS", tools.join("strings"))
        .env("SWIFT", tools.join("swift"))
        .env("CODESIGN", tools.join("codesign"))
        .args(["ios", "verify-app-store-layout"])
        .arg(app);
    command
}

#[cfg(target_os = "macos")]
const IOS_RELEASE_OPTIONAL_FRAMEWORKS: [&str; 7] = [
    "VesperFFmpegAVCodec",
    "VesperFFmpegAVFormat",
    "VesperFFmpegAVUtil",
    "VesperPlayerRemuxFfmpegPlugin",
    "VesperPlayerSourceNormalizerFfmpegPlugin",
    "VesperPlayerDecoderVideoToolboxPlugin",
    "VesperPlayerFrameProcessorDiagnosticPlugin",
];

#[cfg(target_os = "macos")]
fn ios_stage_release_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let directory = tempfile::tempdir().expect("temporary iOS release fixture parent");
    let root = directory
        .path()
        .join("iOS release fixture with spaces - 测试");
    let tools = root.join("fake Apple release tools");
    let build = root.join("lib/ios/VesperPlayerKit/.build/xcframework");
    let device = build.join(
        "VesperPlayerKit-iOS.xcarchive/Products/Library/Frameworks/VesperPlayerKit.framework",
    );
    let simulator = build.join(
        "VesperPlayerKit-iOS-Simulator.xcarchive/Products/Library/Frameworks/VesperPlayerKit.framework",
    );
    let xcframework = build.join("VesperPlayerKit.xcframework");
    for framework in [&device, &simulator] {
        fs::create_dir_all(framework.join("Modules/VesperPlayerKit.swiftmodule"))
            .expect("create staged framework modules");
        fs::write(framework.join("VesperPlayerKit"), b"fixture Mach-O\n")
            .expect("write staged framework binary");
        fs::write(
            framework.join("Modules/VesperPlayerKit.swiftmodule/arm64.swiftmodule"),
            b"private module\n",
        )
        .expect("write private Swift module");
        fs::write(
            framework.join("Modules/VesperPlayerKit.swiftmodule/arm64.swiftdoc"),
            b"public Swift documentation\n",
        )
        .expect("write Swift documentation");
    }
    fs::create_dir_all(&xcframework).expect("create staged XCFramework");
    fs::write(xcframework.join("Info.plist"), b"fixture XCFramework\n")
        .expect("write staged XCFramework marker");
    let artifacts = root.join("lib/ios/VesperPlayerKit/Artifacts");
    let headers = root.join("lib/ios/VesperPlayerKit/Sources/VesperPlayerFFIResolver/include");
    let target_libdir = root.join("fake iOS Rust target libdir");
    let cargo_artifacts = root.join("fake iOS Cargo artifacts");
    fs::create_dir_all(&artifacts).expect("create iOS release FFI artifacts parent");
    fs::create_dir_all(&headers).expect("create iOS release FFI headers");
    fs::write(
        headers.join("player_ffi_resolver.h"),
        b"/* fixture iOS release FFI resolver header */\n",
    )
    .expect("write iOS release FFI resolver header");
    fs::create_dir_all(&target_libdir).expect("create iOS release Rust target libdir");
    fs::create_dir_all(&cargo_artifacts).expect("create iOS release Cargo artifacts");
    fs::create_dir_all(root.join("scripts/ios")).expect("create iOS scripts directory");
    for name in ["ffmpeg-source-policy.toml", "ffmpeg-profiles.toml"] {
        fs::copy(
            workspace_root().join("scripts").join(name),
            root.join("scripts").join(name),
        )
        .unwrap_or_else(|error| panic!("copy iOS release {name}: {error}"));
    }
    for plugin in [
        "remux-ffmpeg",
        "source-normalizer-ffmpeg",
        "decoder-videotoolbox",
        "frame-processor-diagnostic",
    ] {
        let destination = root.join("plugins").join(plugin);
        fs::create_dir_all(&destination).expect("create iOS release plugin fixture directory");
        fs::copy(
            workspace_root()
                .join("plugins")
                .join(plugin)
                .join("vesper-plugin.toml"),
            destination.join("vesper-plugin.toml"),
        )
        .unwrap_or_else(|error| panic!("copy iOS release {plugin} manifest: {error}"));
    }
    fs::create_dir_all(&tools).expect("create fake Apple release tools");
    fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("write workspace manifest");
    fs::write(
        root.join("lib/ios/VesperPlayerKit/project.yml"),
        "settings:\n  base:\n    CFBundleShortVersionString: \"0.4.0\"\n    CFBundleVersion: \"400\"\n",
    )
    .expect("write iOS release fixture project metadata");
    configure_ios_kit_root(&root, &tools);
    write_executable_script(
        &tools.join("install_name_tool"),
        "#!/bin/sh\nprintf 'install_name_tool:%s\\n' \"$*\" >> \"$VESPER_TEST_TOOL_LOG\"\n",
    );
    write_executable_script(
        &tools.join("otool"),
        r#"#!/bin/sh
set -eu
binary=$2
name=$(/usr/bin/basename "$binary")
case "${1:-}" in
  -l) printf '          path @loader_path/.. (offset 12)\n' ;;
  -D) printf '%s:\n@rpath/%s.framework/%s\n' "$binary" "$name" "$name" ;;
  -L)
    printf '%s:\n' "$binary"
    printf '\t@rpath/%s.framework/%s (compatibility version 1.0.0, current version 1.0.0)\n' "$name" "$name"
    case "$name" in
      VesperFFmpegAVCodec)
        dependencies='VesperFFmpegAVUtil'
        ;;
      VesperFFmpegAVFormat)
        dependencies='VesperFFmpegAVCodec VesperFFmpegAVUtil'
        ;;
      VesperPlayerRemuxFfmpegPlugin|VesperPlayerSourceNormalizerFfmpegPlugin)
        dependencies='VesperFFmpegAVCodec VesperFFmpegAVFormat VesperFFmpegAVUtil'
        ;;
      *) dependencies='' ;;
    esac
    for dependency in $dependencies; do
      printf '\t@rpath/%s.framework/%s (compatibility version 1.0.0, current version 1.0.0)\n' "$dependency" "$dependency"
    done
    printf '\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1.0.0)\n'
    ;;
esac
"#,
    );
    write_executable_script(
        &tools.join("lipo"),
        "#!/bin/sh\nprintf 'lipo:%s\\n' \"$*\" >> \"$VESPER_TEST_TOOL_LOG\"\nif [ \"${1:-}\" = -archs ]; then printf 'arm64\\n'; fi\nexit 0\n",
    );
    write_executable_script(
        &tools.join("plutil"),
        "#!/bin/sh\nexec /usr/bin/plutil \"$@\"\n",
    );
    write_executable_script(
        &tools.join("xcrun"),
        r#"#!/bin/sh
set -eu
printf 'xcrun:%s\n' "$*" >> "$VESPER_TEST_TOOL_LOG"
if [ "${1:-}" = plutil ]; then
  shift
  exec /usr/bin/plutil "$@"
fi
if [ "${1:-}" = lipo ] && [ "${2:-}" = -archs ]; then
  printf 'arm64\n'
  exit 0
fi
binary=
for argument do binary=$argument; done
if [ -f "$binary" ] && /usr/bin/grep -q 'ios-sim' "$binary"; then
  platform=IOSSIMULATOR
else
  case "$binary" in
    */iphonesimulator/*) platform=IOSSIMULATOR ;;
    *) platform=IOS ;;
  esac
fi
printf 'Load command 9\n      cmd LC_BUILD_VERSION\n platform %s\n    minos 17.0\n' "$platform"
"#,
    );
    let plugin_fixture = IosPluginFixture {
        directory: tempfile::tempdir().expect("temporary nested iOS plugin fixture owner"),
        root: root.clone(),
        tools: tools.clone(),
        log: root.join("stage-release.log"),
        target_libdir: root.join("fake iOS Rust target libdir"),
    };
    configure_ios_plugin_ffmpeg_fixture(&plugin_fixture);
    rewrite_ios_ffmpeg_root_declared_profile(&root, "source-normalizer");
    configure_ios_release_source_fixture(&root);
    write_executable_script(
        &tools.join("rustc"),
        "#!/bin/sh\nprintf '%s\\n' \"$VESPER_TEST_RUST_TARGET_LIBDIR\"\n",
    );
    write_executable_script(
        &tools.join("cargo"),
        r#"#!/bin/sh
set -eu
printf 'cargo:%s\n' "$*" >> "$VESPER_TEST_TOOL_LOG"
target=
profile=debug
package=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --target) target=$2; shift 2 ;;
    --release) profile=release; shift ;;
    -p) package=$2; shift 2 ;;
    *) shift ;;
  esac
done
if [ "$package" = player-ffi-ios ]; then
  /bin/mkdir -p "$VESPER_TEST_ARTIFACT_ROOT"
  printf 'fixture\n' > "$VESPER_TEST_ARTIFACT_ROOT/libvesper_player_ffi_ios.a"
  printf '%s\n' '{"reason":"compiler-artifact","target":{"name":"vesper_player_ffi_ios","kind":["staticlib"],"crate_types":["staticlib"]},"filenames":["'"$VESPER_TEST_ARTIFACT_ROOT/libvesper_player_ffi_ios.a"'"]}'
  exit 0
fi
if [ "${VESPER_TEST_PLUGIN_CARGO_STATUS:-0}" != 0 ]; then
  exit "$VESPER_TEST_PLUGIN_CARGO_STATUS"
fi
[ -n "$target" ]
case "$package" in
  player-remux-ffmpeg) dylib=libvesper_remux_ffmpeg.dylib ;;
  player-source-normalizer-ffmpeg) dylib=libvesper_source_normalizer_ffmpeg.dylib ;;
  player-decoder-videotoolbox) dylib=libvesper_decoder_videotoolbox.dylib ;;
  player-frame-processor-diagnostic) dylib=libvesper_frame_processor_diagnostic.dylib ;;
  *) exit 91 ;;
esac
artifact="$CARGO_TARGET_DIR/$target/$profile/$dylib"
/bin/mkdir -p "$(/usr/bin/dirname "$artifact")"
printf 'fixture %s %s\n' "$package" "$target" > "$artifact"
"#,
    );
    write_executable_script(
        &tools.join("ditto"),
        r#"#!/bin/sh
printf 'ditto:%s\n' "$*" >> "$VESPER_IOS_RELEASE_TEST_LOG"
if [ "$#" = 2 ]; then
  /bin/cp -R "$1" "$2"
  exit 0
fi
if [ "$1" = --norsrc ] && [ "$2" != -c ]; then
  cp -R "$2" "$3"
  exit 0
fi
if [ "$1" = -c ] && [ "$2" = -k ]; then
  for last do :; done
  source_path=$5
  [ -d "$source_path" ]
  source_parent=$(/usr/bin/dirname "$source_path")
  source_name=$(/usr/bin/basename "$source_path")
  (cd "$source_parent" && /usr/bin/zip -qry "$last" "$source_name")
  exit 0
fi
for last do :; done
source_path=$5
if [ "${VESPER_IOS_RELEASE_DITTO_FAIL_MATCH:-}" = "${source_path##*/}" ]; then
  exit 77
fi
if [ "${source_path##*.}" = framework ] && find "$source_path" -type f -name '*.swiftmodule' -print -quit | grep -q .; then
  exit 78
fi
printf 'archive:%s\n' "$source_path" > "$last"
"#,
    );
    write_executable_script(
        &tools.join("lipo"),
        r#"#!/bin/sh
printf 'lipo:%s\n' "$*" >> "$VESPER_IOS_RELEASE_TEST_LOG"
case "$*" in
  *" -verify_arch arm64") exit 0 ;;
esac
if [ "$1" = -archs ]; then
  printf 'arm64\n'
  exit 0
fi
if [ "$1" = -info ]; then
  case "$2" in
    *Simulator*) printf 'Architectures in the fat file: %s are: arm64 x86_64\n' "$2" ;;
    *) printf 'Non-fat file: %s is architecture: arm64\n' "$2" ;;
  esac
  exit 0
fi
while [ "$#" -gt 0 ]; do
  if [ "$1" = -output ]; then
    printf 'arm64 extracted\n' > "$2"
    exit 0
  fi
  shift
done
exit 79
"#,
    );
    (directory, root, tools)
}

#[cfg(target_os = "macos")]
fn ios_stage_release_command(
    root: &std::path::Path,
    tools: &std::path::Path,
    output: &std::path::Path,
) -> Command {
    ios_release_stage_command(root, tools, "stage-release", output)
}

#[cfg(target_os = "macos")]
fn ios_optional_stage_release_command(
    root: &std::path::Path,
    tools: &std::path::Path,
    output: &std::path::Path,
) -> Command {
    ios_release_stage_command(root, tools, "stage-optional-plugins-release", output)
}

#[cfg(target_os = "macos")]
fn ios_release_stage_command(
    root: &std::path::Path,
    tools: &std::path::Path,
    subcommand: &str,
    output: &std::path::Path,
) -> Command {
    let path = std::env::join_paths([
        tools,
        std::path::Path::new("/usr/bin"),
        std::path::Path::new("/bin"),
        std::path::Path::new("/usr/sbin"),
        std::path::Path::new("/sbin"),
    ])
    .expect("compose iOS release fixture PATH");
    let mut command = Command::new(env!("CARGO_BIN_EXE_vesper"));
    command
        .env("PATH", path)
        .env("DITTO", tools.join("ditto"))
        .env("LIPO", tools.join("lipo"))
        .env(
            "VESPER_IOS_RELEASE_TEST_LOG",
            root.join("stage-release.log"),
        )
        .env("VESPER_TEST_TOOL_LOG", root.join("stage-release.log"))
        .env(
            "VESPER_TEST_RUST_TARGET_LIBDIR",
            root.join("fake iOS Rust target libdir"),
        )
        .env(
            "VESPER_TEST_ARTIFACT_ROOT",
            root.join("fake iOS Cargo artifacts"),
        )
        .env(
            "VESPER_FFMPEG_PROFILE_CONFIG_PATH",
            root.join("scripts/ffmpeg-profiles.toml"),
        )
        .env(
            "VESPER_APPLE_FFMPEG_OUTPUT_DIR",
            root.join("third_party/ffmpeg/apple/profiles/custom-e1eabc3db7bc"),
        )
        .args(["ios", "--root"])
        .arg(root)
        .arg(subcommand)
        .arg(output);
    apply_ios_ffmpeg_runtime_reuse_environment(&mut command, root);
    command
}

fn expected_package_overrides(package: &str, packages: &[&str]) -> String {
    let mut expected = String::from("dependency_overrides:\n");
    for dependency in packages {
        if *dependency != package {
            expected.push_str(&format!("  {dependency}:\n    path: ../{dependency}\n"));
        }
    }
    expected
}

fn expected_example_overrides(packages: &[&str]) -> String {
    let mut expected = String::from("dependency_overrides:\n");
    for dependency in packages {
        expected.push_str(&format!(
            "  {dependency}:\n    path: ../../lib/flutter/{dependency}\n"
        ));
    }
    expected
}

#[cfg(unix)]
fn flutter_android_gradle_fixture(version: &str) -> (tempfile::TempDir, PathBuf) {
    let parent = tempfile::tempdir().expect("temporary Flutter Android fixture parent");
    let root = parent.path().join("Flutter Android fixture - 测试 space");
    let wrapper_directory = root.join("examples/flutter-host/android/gradle/wrapper");
    fs::create_dir_all(&wrapper_directory).expect("create Gradle wrapper directory");
    fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("write workspace manifest");
    fs::write(
        wrapper_directory.join("gradle-wrapper.properties"),
        format!(
            "distributionUrl=https\\://services.gradle.org/distributions/gradle-{version}-bin.zip\n"
        ),
    )
    .expect("write Gradle wrapper properties");
    (parent, root)
}

#[cfg(unix)]
fn write_cached_gradle(
    root: &std::path::Path,
    version: &str,
    cache_key: &str,
    source: &str,
) -> PathBuf {
    let gradle = root
        .join("examples/flutter-host/android/.gradle/wrapper/dists")
        .join(format!("gradle-{version}-bin"))
        .join(cache_key)
        .join(format!("gradle-{version}/bin/gradle"));
    fs::create_dir_all(gradle.parent().expect("cached Gradle parent"))
        .expect("create cached Gradle directory");
    write_executable_script(&gradle, source);
    gradle
}

#[cfg(unix)]
fn prepare_android_project_fixture(root: &std::path::Path) {
    let wrapper_directory = root.join("lib/android/gradle/wrapper");
    fs::create_dir_all(&wrapper_directory).expect("create Android Gradle wrapper directory");
    fs::write(
        wrapper_directory.join("gradle-wrapper.properties"),
        "distributionUrl=https\\://services.gradle.org/distributions/gradle-9.6.0-bin.zip\n",
    )
    .expect("write Android Gradle wrapper properties");
    fs::create_dir_all(root.join("lib/android/vesper-player-kit/src/main"))
        .expect("create Android JNI output parent");
    fs::create_dir_all(root.join("examples/android-compose-host"))
        .expect("create Android fallback project");
}

#[cfg(unix)]
fn write_android_cached_gradle(root: &std::path::Path, project: &str, source: &str) -> PathBuf {
    let gradle = root
        .join(project)
        .join(".gradle/wrapper/dists/gradle-9.6.0-bin/fixture/gradle-9.6.0/bin/gradle");
    fs::create_dir_all(gradle.parent().expect("Android cached Gradle parent"))
        .expect("create Android cached Gradle directory");
    let source = source.strip_prefix("#!/bin/sh\n").unwrap_or(source);
    let wrapped = format!(
        r#"#!/bin/sh
set -eu
runtime_task=0
task_count=0
for argument in "$@"; do
  case "$argument" in :*) task_count=$((task_count + 1)) ;; esac
  if [ "$argument" = ':vesper-player-kit-ffmpeg-runtime:assembleRelease' ]; then
    runtime_task=1
  fi
done
if [ "$runtime_task" = 1 ] && [ "$task_count" = 1 ]; then
  aar="$PWD/lib/android/vesper-player-kit-ffmpeg-runtime/build/outputs/aar/vesper-player-kit-ffmpeg-runtime-release.aar"
  stage="$(mktemp -d "${{TMPDIR:-/tmp}}/vesper-test-runtime-aar.XXXXXX")"
  trap 'rm -rf "$stage"' EXIT
  mkdir -p "$stage/jni" "$stage/assets"
  cp -R "${{VESPER_ANDROID_FFMPEG_RUNTIME_JNI_LIBS:?}}/." "$stage/jni/"
  cp -R "${{VESPER_ANDROID_FFMPEG_RUNTIME_ASSETS:?}}/." "$stage/assets/"
  mkdir -p "$(dirname "$aar")"
  rm -f "$aar"
  (cd "$stage" && zip -q -r "$aar" jni assets)
  exit 0
fi
{source}"#
    );
    write_executable_script(&gradle, &wrapped);
    gradle
}

#[cfg(unix)]
fn android_cli_fixture() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().expect("temporary Android CLI fixture parent");
    let root = directory.path().join("Android CLI fixture - 测试 space");
    fs::create_dir_all(&root).expect("create Android CLI fixture root");
    fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("write Android manifest");
    prepare_android_project_fixture(&root);
    (directory, root)
}

#[cfg(unix)]
fn prepare_android_sample_apk_fixture(
    root: &std::path::Path,
) -> (PathBuf, NativeAndroidFfmpegFixture) {
    let fixture =
        prepare_native_android_ffmpeg_fixture(root, &root.join("sample Android tools"), "8.1.2");
    fs::create_dir_all(root.join("examples/flutter-host/android"))
        .expect("create Flutter Android sample fixture");
    fs::copy(
        workspace_root().join("scripts/ffmpeg-profiles.toml"),
        root.join("scripts/ffmpeg-profiles.toml"),
    )
    .expect("copy Android sample FFmpeg profiles");
    write_android_cached_gradle(
        root,
        "examples/android-compose-host",
        r#"#!/bin/sh
printf 'gradle home=%s' "$GRADLE_USER_HOME" >> "$ANDROID_SAMPLE_LOG"
for argument in "$@"; do
  printf ' arg=%s' "$argument" >> "$ANDROID_SAMPLE_LOG"
done
printf '\n' >> "$ANDROID_SAMPLE_LOG"
exit "${ANDROID_SAMPLE_GRADLE_STATUS:-0}"
"#,
    );
    let compose_apk = root
        .join("examples/android-compose-host/app/build/outputs/apk/release/compose-release.apk");
    write_test_zip(
        &compose_apk,
        &[
            ("AndroidManifest.xml", b"compose manifest"),
            (
                "lib/arm64-v8a/libvesper_player_android.so",
                b"compose native library",
            ),
        ],
    );
    let flutter_apk =
        root.join("examples/flutter-host/build/app/outputs/flutter-apk/app-arm64-v8a-release.apk");
    write_test_zip(
        &flutter_apk,
        &[
            ("AndroidManifest.xml", b"flutter manifest"),
            ("lib/arm64-v8a/libapp.so", b"flutter release aot"),
        ],
    );
    (root.join("android-sample.log"), fixture)
}

#[cfg(unix)]
fn android_optional_output_paths(root: &std::path::Path) -> [PathBuf; 8] {
    [
        root.join("lib/android/vesper-player-kit-decoder-mediacodec/src/main/jniLibs"),
        root.join("lib/android/vesper-player-kit-source-normalizer-ffmpeg/src/main/jniLibs"),
        root.join(
            "lib/android/vesper-player-kit-source-normalizer-ffmpeg/src/main/assets/vesper-source-normalizer-ffmpeg",
        ),
        root.join(
            "lib/android/vesper-player-kit-frame-processor-diagnostic/src/main/jniLibs",
        ),
        root.join("lib/android/vesper-player-kit-ffmpeg-runtime/src/main/jniLibs"),
        root.join(
            "lib/android/vesper-player-kit-ffmpeg-runtime/src/main/assets/vesper-ffmpeg-runtime",
        ),
        root.join("lib/android/vesper-player-kit-external-playback/src/main/jniLibs"),
        root.join(
            "lib/android/vesper-player-kit-external-playback/src/main/assets/vesper-relay-ffmpeg",
        ),
    ]
}

#[cfg(unix)]
fn prepare_android_optional_project_fixture(root: &std::path::Path) -> [PathBuf; 8] {
    let outputs = android_optional_output_paths(root);
    for output in &outputs {
        fs::create_dir_all(output.parent().expect("optional Android output parent"))
            .expect("create optional Android output parent");
    }
    outputs
}

#[cfg(unix)]
fn prepare_android_optional_toolchain(root: &std::path::Path, tools: &std::path::Path) -> PathBuf {
    let native = prepare_native_android_ffmpeg_fixture(root, tools, "8.1.2");
    let target_libdir = root.join("fake rust target/lib");
    fs::create_dir_all(&target_libdir).expect("create fake Rust target library directory");
    let sdk = root.join("Android SDK");
    let ndk = root.join("Android NDK");
    fs::create_dir_all(&sdk).expect("create Android SDK fixture");
    std::os::unix::fs::symlink(native.sdk.join("ndk"), sdk.join("ndk"))
        .expect("link Android SDK NDK fixture");
    std::os::unix::fs::symlink(native.sdk.join("ndk/29.0.14206865"), &ndk)
        .expect("link Android NDK fixture");
    write_executable_script(
        &tools.join("rustc"),
        "#!/bin/sh\nprintf '%s\\n' \"$ANDROID_TEST_TARGET_LIBDIR\"\n",
    );
    write_executable_script(&tools.join("cargo"), "#!/bin/sh\nexit 0\n");
    write_executable_script(
        &tools.join("cargo-ndk"),
        r#"#!/bin/sh
output=
crate=
while [ "$#" -gt 0 ]; do
  if [ "$1" = '-o' ]; then
    shift
    output="$1"
  elif [ "$1" = '-p' ]; then
    shift
    crate="$1"
  fi
  shift
done
if [ -n "${ANDROID_OPTIONAL_LOG:-}" ]; then
  printf 'cargo-ndk crate=%s output=%s\n' "$crate" "$output" >> "$ANDROID_OPTIONAL_LOG"
fi
if [ "$crate" = "${ANDROID_TEST_CARGO_NDK_FAIL_CRATE:-}" ]; then
  exit "${ANDROID_TEST_CARGO_NDK_STATUS:-23}"
fi
if [ "${ANDROID_TEST_CARGO_NDK_EMPTY:-0}" = 1 ]; then
  exit 0
fi
case "$crate" in
  player-jni-android) library=libvesper_player_android.so ;;
  player-decoder-mediacodec) library=libvesper_decoder_mediacodec.so ;;
  player-source-normalizer-ffmpeg) library=libvesper_source_normalizer_ffmpeg.so ;;
  player-frame-processor-diagnostic) library=libvesper_frame_processor_diagnostic.so ;;
  player-relay-ffmpeg-android) library=libvesper_player_relay_ffmpeg.so ;;
  player-remux-ffmpeg) library=libvesper_remux_ffmpeg.so ;;
  *) exit 97 ;;
esac
/bin/mkdir -p "$output/arm64-v8a"
printf '%s\n' "$crate" > "$output/arm64-v8a/$library"
"#,
    );
    fs::copy(
        workspace_root().join("scripts/ffmpeg-profiles.toml"),
        root.join("scripts/ffmpeg-profiles.toml"),
    )
    .expect("copy FFmpeg profile fixture");
    std::env::join_paths([
        tools,
        PathBuf::from("/usr/bin").as_path(),
        PathBuf::from("/bin").as_path(),
    ])
    .expect("compose Android optional toolchain PATH")
    .into()
}

#[cfg(unix)]
fn prepare_android_stage_release_fixture(root: &std::path::Path) -> [String; 8] {
    let _ = prepare_android_optional_project_fixture(root);
    fs::create_dir_all(root.join("scripts")).expect("create Android release script directory");
    fs::copy(
        workspace_root().join("scripts/ffmpeg-source-policy.toml"),
        root.join("scripts/ffmpeg-source-policy.toml"),
    )
    .expect("copy Android release FFmpeg source policy");
    for plugin in [
        "decoder-mediacodec",
        "source-normalizer-ffmpeg",
        "frame-processor-diagnostic",
    ] {
        let source = workspace_root()
            .join("plugins")
            .join(plugin)
            .join("vesper-plugin.toml");
        let target = root.join("plugins").join(plugin).join("vesper-plugin.toml");
        fs::create_dir_all(target.parent().expect("Android plugin descriptor parent"))
            .expect("create Android plugin descriptor directory");
        fs::copy(source, target).expect("copy Android plugin descriptor");
    }
    let abi = "arm64-v8a";
    let profile_hash = "custom-adf812db3806";
    let runtime_metadata = b"ffmpeg_version=8.1.2\nsource_url=https://ffmpeg.org/releases/ffmpeg-8.1.2.tar.xz\nsource_sha256=464beb5e7bf0c311e68b45ae2f04e9cc2af88851abb4082231742a74d97b524c\n";
    let plugin_specs = [
        ("decoder-mediacodec", "vesper_decoder_mediacodec"),
        (
            "source-normalizer-ffmpeg",
            "vesper_source_normalizer_ffmpeg",
        ),
        (
            "frame-processor-diagnostic",
            "vesper_frame_processor_diagnostic",
        ),
    ];

    let artifacts = [
        "VesperPlayerKit-android-arm64-v8a.aar".to_owned(),
        "VesperPlayerKitCompose-android-arm64-v8a.aar".to_owned(),
        "VesperPlayerKitComposeUi-android-arm64-v8a.aar".to_owned(),
        "VesperPlayerKitExternalPlayback-android-arm64-v8a.aar".to_owned(),
        "VesperPlayerKitFfmpegRuntime-android-arm64-v8a.aar".to_owned(),
        "VesperPlayerKitDecoderMediaCodec-android-arm64-v8a.aar".to_owned(),
        "VesperPlayerKitSourceNormalizerFfmpeg-android-arm64-v8a.aar".to_owned(),
        "VesperPlayerKitFrameProcessorDiagnostic-android-arm64-v8a.aar".to_owned(),
    ];
    let aar_root = root.join("lib/android");
    let core = [
        (
            "vesper-player-kit",
            "vesper-player-kit-release.aar",
            vec![(
                "jni/arm64-v8a/libvesper_player_android.so",
                b"host\n".as_slice(),
            )],
        ),
        (
            "vesper-player-kit-compose",
            "vesper-player-kit-compose-release.aar",
            Vec::new(),
        ),
        (
            "vesper-player-kit-compose-ui",
            "vesper-player-kit-compose-ui-release.aar",
            Vec::new(),
        ),
    ];
    for (module, name, mut entries) in core {
        entries.insert(0, ("AndroidManifest.xml", b"manifest\n".as_slice()));
        write_test_zip(
            &aar_root.join(module).join("build/outputs/aar").join(name),
            &entries,
        );
    }

    let optional_root =
        |module: &str, name: &str| aar_root.join(module).join("build/outputs/aar").join(name);
    write_test_zip(
        &optional_root(
            "vesper-player-kit-external-playback",
            "vesper-player-kit-external-playback-release.aar",
        ),
        &[
            ("AndroidManifest.xml", b"manifest\n"),
            ("jni/arm64-v8a/libvesper_player_relay_ffmpeg.so", b"relay\n"),
            (
                "assets/vesper-relay-ffmpeg/profile-hash.txt",
                b"custom-adf812db3806\n",
            ),
        ],
    );
    write_test_zip(
        &optional_root(
            "vesper-player-kit-ffmpeg-runtime",
            "vesper-player-kit-ffmpeg-runtime-release.aar",
        ),
        &[
            ("AndroidManifest.xml", b"manifest\n"),
            ("jni/arm64-v8a/libavcodec.so", b"runtime codec\n"),
            ("jni/arm64-v8a/libavformat.so", b"runtime format\n"),
            ("jni/arm64-v8a/libavutil.so", b"runtime util\n"),
            (
                "assets/vesper-ffmpeg-runtime/profile-hash.txt",
                b"custom-adf812db3806\n",
            ),
            (
                "assets/vesper-ffmpeg-runtime/arm64-v8a-metadata.txt",
                runtime_metadata,
            ),
        ],
    );

    for (plugin, library_name) in plugin_specs {
        let binary = format!("{plugin}\n").into_bytes();
        let binary_path = root.join(format!(".android-stage-{plugin}.so"));
        fs::write(&binary_path, &binary).expect("write Android registry fixture binary");
        let descriptor_path = root.join("plugins").join(plugin).join("vesper-plugin.toml");
        let descriptor_source =
            fs::read_to_string(&descriptor_path).expect("read Android plugin descriptor fixture");
        let project = PluginProjectManifest::from_toml(&descriptor_source)
            .expect("parse Android plugin project manifest fixture");
        let descriptor = project
            .descriptor()
            .clone()
            .canonicalize()
            .expect("canonicalize Android plugin descriptor fixture");
        let fragment = EmbeddedRegistryFragment::generate(
            &descriptor,
            &EmbeddedRegistryTarget::AndroidNativeLibrary {
                target: "aarch64-linux-android".to_owned(),
                architecture: abi.to_owned(),
                minimum_os: "26".to_owned(),
                library_name: library_name.to_owned(),
                artifact_path: binary_path,
            },
        )
        .expect("generate Android plugin registry fixture");
        let aar_name = match plugin {
            "decoder-mediacodec" => "vesper-player-kit-decoder-mediacodec-release.aar",
            "source-normalizer-ffmpeg" => "vesper-player-kit-source-normalizer-ffmpeg-release.aar",
            "frame-processor-diagnostic" => {
                "vesper-player-kit-frame-processor-diagnostic-release.aar"
            }
            _ => unreachable!("known Android plugin fixture"),
        };
        let module = match plugin {
            "decoder-mediacodec" => "vesper-player-kit-decoder-mediacodec",
            "source-normalizer-ffmpeg" => "vesper-player-kit-source-normalizer-ffmpeg",
            "frame-processor-diagnostic" => "vesper-player-kit-frame-processor-diagnostic",
            _ => unreachable!("known Android plugin fixture"),
        };
        let mut entries = vec![
            ("AndroidManifest.xml", b"manifest\n".as_slice()),
            (
                Box::leak(format!("jni/{abi}/lib{library_name}.so").into_boxed_str()),
                binary.as_slice(),
            ),
            (
                Box::leak(
                    format!("assets/vesper/plugins/{abi}/{}", fragment.file_name())
                        .into_boxed_str(),
                ),
                fragment.canonical_json(),
            ),
        ];
        if plugin == "source-normalizer-ffmpeg" {
            entries.push((
                "assets/vesper-source-normalizer-ffmpeg/profile-hash.txt",
                profile_hash.as_bytes(),
            ));
        }
        write_test_zip(&optional_root(module, aar_name), &entries);
    }
    artifacts
}

#[cfg(unix)]
fn ffi_fixture(header: &str) -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("temporary FFI fixture");
    let root = directory.path();
    fs::create_dir_all(root.join("crates/ffi/player-ffi")).expect("create FFI crate");
    fs::create_dir_all(root.join("include")).expect("create FFI include");
    fs::create_dir_all(root.join("examples/c-host")).expect("create C host");
    fs::create_dir_all(root.join("fixtures/media")).expect("create media fixture");
    fs::create_dir_all(root.join("target/debug")).expect("create debug output");
    fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("write fixture manifest");
    fs::write(root.join("Cargo.lock"), "version = 4\n").expect("write fixture lockfile");
    fs::write(
        root.join("crates/ffi/player-ffi/cbindgen.toml"),
        "language = \"C\"\n",
    )
    .expect("write cbindgen config");
    fs::write(root.join("include/player_ffi.h"), header).expect("write checked-in header");
    fs::write(
        root.join("examples/c-host/main.c"),
        "int main(void) { return 0; }\n",
    )
    .expect("write C host source");
    fs::write(root.join("fixtures/media/tiny-h264-aac.m4v"), b"fixture")
        .expect("write media fixture");

    let tools = root.join("fake-tools");
    fs::create_dir_all(&tools).expect("create fake tools");
    write_executable_script(
        &tools.join("cbindgen"),
        r##"#!/bin/sh
output=
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--output" ]; then
    shift
    output="$1"
  fi
  shift
done
[ -n "$output" ] || exit 9
printf '%s' "$FAKE_CBINDGEN_HEADER" > "$output"
"##,
    );
    write_executable_script(&tools.join("cargo"), "#!/bin/sh\nexit 0\n");
    write_executable_script(
        &tools.join("cc"),
        r##"#!/bin/sh
output=
while [ "$#" -gt 0 ]; do
  if [ "$1" = "-o" ]; then
    shift
    output="$1"
  fi
  shift
done
[ -n "$output" ] || exit 9
{
  printf '%s\n' '#!/bin/sh'
  printf '%s\n' 'printf "smoke:%s\\n" "$1"'
} > "$output"
chmod 700 "$output"
"##,
    );
    directory
}

#[cfg(unix)]
fn ffi_path_with_tools(tools: &std::path::Path) -> String {
    let current = std::env::var_os("PATH").unwrap_or_default();
    format!("{}:{}", tools.display(), current.to_string_lossy())
}

fn native_fixture_manifest() -> String {
    r#"
schema_version = 1

[plugin]
id = "dev.vesper.plugin-fixture"
name = "Vesper Plugin Fixture"
version = "0.4.0"
description = "Safe Rust Plugin fixture"
license = "Apache-2.0"
publisher = "dev.vesper"

[compatibility]
host_sdk = ">=0.4.0, <0.5.0"
abi_major = 1
abi_minor_min = 0
abi_minor_max = 0

[[capabilities]]
interface_id = "e9479dbc-42d2-575e-b39e-a24bc512fbc7"
instance_id = "dev.vesper.plugin-fixture.post-download"
interface_major = 1
interface_minor = 0
stability = "stable"

[[capabilities]]
interface_id = "c7a69475-79b2-5b5e-a477-08844a5da5d1"
instance_id = "dev.vesper.plugin-fixture.event-hook"
interface_major = 1
interface_minor = 0
stability = "stable"

[[capabilities]]
interface_id = "2d8e5be8-b1de-5e83-8fe0-6118aabc5118"
instance_id = "dev.vesper.plugin-fixture.benchmark"
interface_major = 1
interface_minor = 0
stability = "stable"
"#
    .to_owned()
}

fn native_fixture_artifact() -> Option<PathBuf> {
    let target_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target/debug");
    #[cfg(target_os = "macos")]
    let name = "libvesper_plugin_fixture.dylib";
    #[cfg(target_os = "linux")]
    let name = "libvesper_plugin_fixture.so";
    #[cfg(target_os = "windows")]
    let name = "vesper_plugin_fixture.dll";
    let path = target_dir.join(name);
    path.is_file().then_some(path)
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn ffmpeg_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_vesper"));
    command.args(["ffmpeg", "--root"]).arg(workspace_root());
    for name in [
        "VESPER_FFMPEG_PROFILE_CONFIG_PATH",
        "VESPER_FFMPEG_PROFILE",
        "VESPER_ANDROID_FFMPEG_PROFILE",
        "VESPER_APPLE_FFMPEG_PROFILE",
        "VESPER_FFMPEG_TLS_BACKEND",
        "VESPER_ANDROID_FFMPEG_TLS_BACKEND",
        "VESPER_APPLE_FFMPEG_TLS_BACKEND",
        "VESPER_FFMPEG_ENABLE_DASH",
        "VESPER_ANDROID_FFMPEG_ENABLE_DASH",
        "VESPER_APPLE_FFMPEG_ENABLE_DASH",
        "VESPER_FFMPEG_ENABLE_PROTOCOLS",
        "VESPER_ANDROID_FFMPEG_ENABLE_PROTOCOLS",
        "VESPER_APPLE_FFMPEG_ENABLE_PROTOCOLS",
        "VESPER_FFMPEG_EXTRA_CONFIGURE_ARGS",
        "VESPER_ANDROID_FFMPEG_EXTRA_CONFIGURE_ARGS",
        "VESPER_APPLE_FFMPEG_EXTRA_CONFIGURE_ARGS",
        "VESPER_FFMPEG_OUTPUT_DIR",
        "VESPER_ANDROID_FFMPEG_OUTPUT_DIR",
        "VESPER_APPLE_FFMPEG_OUTPUT_DIR",
        "BASH_ENV",
        "ENV",
        "CDPATH",
    ] {
        command.env_remove(name);
    }
    command
}

fn expected_ffmpeg_dry_run(profile: &str, platform: &str, hash: &str) -> String {
    let unindented = format!(
        "Resolved FFmpeg profile:\n\
profile={profile}\n\
platform={platform}\n\
libraries=avcodec,avformat,avutil\n\
demuxers=hls,dash,concat,flv,mov,matroska,mpegts,aac\n\
muxers=mp4,mov,matroska,hls,mpegts\n\
protocols=file,pipe\n\
decoders=\n\
parsers=aac,ac3,av1,flac,h264,hevc,mpeg4video,opus,vp8,vp9\n\
bsfs=aac_adtstoasc,extract_extradata,h264_metadata,hevc_metadata,h264_mp4toannexb,hevc_mp4toannexb\n\
tls=none\n\
validation.forbid_network=true\n\
validation.forbid_openssl=true\n\
profile_hash={hash}\n\
Build arguments:\n\
  --ffmpeg-profile\n\
  custom\n\
  --tls-backend\n\
  none\n\
  --enable-libraries\n\
  avcodec\\,avformat\\,avutil\n\
  --enable-demuxers\n\
  hls\\,dash\\,concat\\,flv\\,mov\\,matroska\\,mpegts\\,aac\n\
  --enable-muxers\n\
  mp4\\,mov\\,matroska\\,hls\\,mpegts\n\
  --enable-protocols\n\
  file\\,pipe\n\
  --enable-parsers\n\
  aac\\,ac3\\,av1\\,flac\\,h264\\,hevc\\,mpeg4video\\,opus\\,vp8\\,vp9\n\
  --enable-bsfs\n\
  aac_adtstoasc\\,extract_extradata\\,h264_metadata\\,hevc_metadata\\,h264_mp4toannexb\\,hevc_mp4toannexb\n\
  --enable-dash\n"
    );
    let (header, arguments) = unindented
        .split_once("Build arguments:\n")
        .expect("dry-run fixture has an argument section");
    let mut expected = String::with_capacity(unindented.len() + arguments.len() * 2);
    expected.push_str(header);
    expected.push_str("Build arguments:\n");
    for argument in arguments.lines() {
        expected.push_str("  ");
        expected.push_str(argument);
        expected.push('\n');
    }
    expected
}

#[test]
fn ffmpeg_profile_list_and_dry_run_match_golden_contract() {
    let list = ffmpeg_command()
        .arg("--list-profiles")
        .output()
        .expect("list FFmpeg profiles");
    assert_eq!(list.status.code(), Some(0));
    assert!(list.stderr.is_empty());
    assert_eq!(
        String::from_utf8(list.stdout).expect("UTF-8 profile list"),
        "base\nrelay-remux\ndownload-remux\ndefault\nsource-normalizer\n"
    );

    let android = ffmpeg_command()
        .args(["--platform", "android", "--profile", "default", "--dry-run"])
        .output()
        .expect("run Android FFmpeg dry-run");
    assert_eq!(android.status.code(), Some(0));
    assert!(android.stderr.is_empty());
    assert_eq!(
        String::from_utf8(android.stdout).expect("UTF-8 Android dry-run"),
        expected_ffmpeg_dry_run("default", "android", "custom-adf812db3806")
    );

    let ios = ffmpeg_command()
        .args([
            "--platform",
            "ios",
            "--profile",
            "source-normalizer",
            "--dry-run",
        ])
        .output()
        .expect("run iOS FFmpeg dry-run");
    assert_eq!(ios.status.code(), Some(0));
    assert!(ios.stderr.is_empty());
    assert_eq!(
        String::from_utf8(ios.stdout).expect("UTF-8 iOS dry-run"),
        expected_ffmpeg_dry_run("source-normalizer", "ios", "custom-e1eabc3db7bc")
    );
}

#[test]
fn ffmpeg_usage_policy_and_selector_failures_keep_exit_contracts() {
    let missing_platform = ffmpeg_command().output().expect("run missing platform");
    assert_eq!(missing_platform.status.code(), Some(2));
    assert!(missing_platform.stdout.is_empty());
    assert!(String::from_utf8_lossy(&missing_platform.stderr).contains("--platform"));

    let cases = [
        (
            vec!["--platform", "android", "--dry-run"],
            ("VESPER_ANDROID_FFMPEG_ENABLE_PROTOCOLS", "http"),
            "network",
        ),
        (
            vec![
                "--platform",
                "android",
                "--dry-run",
                "--extra-configure-arg=--enable-protocol=http,https",
            ],
            ("VESPER_FFMPEG_PROFILE", "default"),
            "network",
        ),
        (
            vec!["--platform", "ios", "--dry-run"],
            (
                "VESPER_APPLE_FFMPEG_EXTRA_CONFIGURE_ARGS",
                "--enable-openssl",
            ),
            "OpenSSL",
        ),
    ];
    for (arguments, (variable, value), diagnostic) in cases {
        let mut command = ffmpeg_command();
        command.env(variable, value).args(arguments);
        let output = command.output().expect("run FFmpeg policy failure");
        assert_eq!(output.status.code(), Some(5));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains(diagnostic));
    }

    let unsupported_android = ffmpeg_command()
        .args(["--platform", "android", "--dry-run", "--abi", "x86_64"])
        .output()
        .expect("run unsupported Android ABI");
    assert_eq!(unsupported_android.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&unsupported_android.stderr).contains("x86_64"));

    let unsupported_ios = ffmpeg_command()
        .args([
            "--platform",
            "ios",
            "--dry-run",
            "--slice",
            "ios-simulator-x86_64",
        ])
        .output()
        .expect("run unsupported iOS slice");
    assert_eq!(unsupported_ios.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&unsupported_ios.stderr).contains("ios-simulator-x86_64"));
}

#[test]
fn ffmpeg_cli_profile_precedence_beats_platform_environment() {
    let output = ffmpeg_command()
        .env("VESPER_FFMPEG_PROFILE", "relay-remux")
        .env("VESPER_ANDROID_FFMPEG_PROFILE", "source-normalizer")
        .args(["--platform", "android", "--profile", "default", "--dry-run"])
        .output()
        .expect("run FFmpeg precedence dry-run");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 precedence dry-run");
    assert!(stdout.contains("profile=default\n"));
    assert!(stdout.contains("profile_hash=custom-adf812db3806\n"));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn ffmpeg_native_executor_honors_cli_output_without_shell_evaluation() {
    let directory = tempfile::tempdir().expect("create native FFmpeg fixture");
    let root = directory.path().join("repository");
    let tools = directory.path().join("tools");
    let fixture = prepare_native_android_ffmpeg_fixture(&root, &tools, "8.1.2");
    let output_directory = root.join("cli output");
    let hostile_bash_env = root.join("hostile-bash-env.sh");
    fs::write(&hostile_bash_env, "exit 91\n").expect("write hostile BASH_ENV");

    let result = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", &fixture.path)
        .env("ANDROID_SDK_ROOT", &fixture.sdk)
        .env(
            "VESPER_FAKE_RUST_TARGET_LIBDIR",
            &fixture.rust_target_libdir,
        )
        .env("VESPER_CURL_MARKER", &fixture.curl_marker)
        .env("VESPER_SHELL_MARKER", &fixture.shell_marker)
        .env("BASH_ENV", &hostile_bash_env)
        .env("VESPER_ANDROID_FFMPEG_OUTPUT_DIR", "losing android output")
        .env("VESPER_FFMPEG_OUTPUT_DIR", "losing generic output")
        .args(["ffmpeg", "--root"])
        .arg(&root)
        .args([
            "--platform",
            "android",
            "--android-artifact",
            "prebuilts",
            "--output-dir",
        ])
        .arg(&output_directory)
        .output()
        .expect("run native FFmpeg fixture");
    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        output_directory
            .join("arm64-v8a/lib/libavcodec.so")
            .is_file()
    );
    let metadata =
        fs::read_to_string(output_directory.join("arm64-v8a/vesper-ffmpeg-build-metadata.txt"))
            .expect("read native FFmpeg metadata");
    assert!(metadata.contains("ffmpeg_version=8.1.2\n"));
    assert!(metadata.contains(&format!(
        "--prefix={}",
        output_directory.join("arm64-v8a").display()
    )));
    assert!(!fixture.shell_marker.exists());
    assert!(!fixture.curl_marker.exists());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn ffmpeg_native_source_resolution_is_cache_first_and_records_the_exact_patch() {
    let directory = tempfile::tempdir().expect("create native FFmpeg source fixture");
    let root = directory.path().join("repository");
    let tools = directory.path().join("tools");
    let fixture = prepare_native_android_ffmpeg_fixture(&root, &tools, "8.1.10");
    let output_directory = root.join("resolved output");

    let result = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", &fixture.path)
        .env("ANDROID_SDK_ROOT", &fixture.sdk)
        .env(
            "VESPER_FAKE_RUST_TARGET_LIBDIR",
            &fixture.rust_target_libdir,
        )
        .env("VESPER_CURL_MARKER", &fixture.curl_marker)
        .env("VESPER_SHELL_MARKER", &fixture.shell_marker)
        .args(["ffmpeg", "--root"])
        .arg(&root)
        .args([
            "--platform",
            "android",
            "--android-artifact",
            "prebuilts",
            "--output-dir",
        ])
        .arg(&output_directory)
        .output()
        .expect("run native FFmpeg source fixture");
    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let metadata =
        fs::read_to_string(output_directory.join("arm64-v8a/vesper-ffmpeg-build-metadata.txt"))
            .expect("read resolved FFmpeg metadata");
    assert!(metadata.contains("ffmpeg_version=8.1.10\n"));
    assert!(metadata.contains("source_url=https://ffmpeg.org/releases/ffmpeg-8.1.10.tar.xz\n"));
    assert!(!fixture.curl_marker.exists());
    assert!(!fixture.shell_marker.exists());
}

#[cfg(unix)]
#[test]
fn android_worker_commands_route_through_the_rust_cli() {
    let directory = tempfile::tempdir().expect("create Android worker routing fixture");
    let root = directory.path().join("repository");
    let scripts = root.join("scripts");
    fs::create_dir_all(scripts.join("android")).expect("create Android worker scripts");
    fs::create_dir_all(scripts.join("mobile")).expect("create mobile worker scripts");
    fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("write Android worker manifest");
    let wrapper = scripts.join("vesper");
    fs::copy(workspace_root().join("scripts/vesper"), &wrapper)
        .expect("copy Vesper wrapper into Android fixture");
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700))
            .expect("make Android wrapper executable");
    }

    let tools = root.join("Android plugin routing tools");
    let path = prepare_android_optional_toolchain(&root, &tools);
    let commands = [
        ("remux-plugin", "remux output", vec!["release"]),
        (
            "source-normalizer-plugin",
            "source normalizer output",
            vec!["release", "--profile", "default"],
        ),
        (
            "decoder-mediacodec-plugin",
            "decoder output",
            vec!["release"],
        ),
        (
            "frame-processor-plugin",
            "frame processor output",
            vec!["release"],
        ),
    ];
    for (command_name, output_name, trailing_arguments) in commands {
        let output_directory = root.join(output_name);
        let mut command = Command::new(&wrapper);
        command
            .env("VESPER_CLI", env!("CARGO_BIN_EXE_vesper"))
            .env("PATH", &path)
            .env(
                "ANDROID_TEST_TARGET_LIBDIR",
                root.join("fake rust target/lib"),
            )
            .env("ANDROID_SDK_ROOT", root.join("Android SDK"))
            .env("ANDROID_NDK_ROOT", root.join("Android NDK"))
            .args(["android", command_name, "--root"])
            .arg(&root)
            .arg(&output_directory)
            .args(trailing_arguments);
        let output = command
            .output()
            .expect("run Android plugin through Vesper wrapper");
        assert_eq!(
            output.status.code(),
            Some(0),
            "{} stderr: {}",
            command_name,
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!output.stdout.is_empty());
        assert!(output.stderr.is_empty());
        assert!(output_directory.is_dir(), "output for {command_name}");
    }
}

#[cfg(unix)]
#[test]
fn android_worker_commands_preserve_stable_cli_exit_codes() {
    let (_directory, root) = android_cli_fixture();
    let tools = root.join("Android status tools");
    let path = prepare_android_optional_toolchain(&root, &tools);

    for expected in [2, 3, 4, 5, 6] {
        let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
            .env("PATH", &path)
            .env(
                "ANDROID_TEST_TARGET_LIBDIR",
                root.join("fake rust target/lib"),
            )
            .env("ANDROID_SDK_ROOT", root.join("Android SDK"))
            .env("ANDROID_NDK_ROOT", root.join("Android NDK"))
            .env("ANDROID_TEST_CARGO_NDK_FAIL_CRATE", "player-jni-android")
            .env("ANDROID_TEST_CARGO_NDK_STATUS", expected.to_string())
            .args(["android", "jni", "release", "arm64-v8a", "--root"])
            .arg(&root)
            .output()
            .expect("run Android status fixture");
        assert_eq!(
            output.status.code(),
            Some(expected),
            "expected status {expected}, stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("exited unsuccessfully"));
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn android_ffmpeg_plugin_worker_uses_shared_profile_and_preserves_outputs_on_failure() {
    let directory = tempfile::tempdir().expect("create Android FFmpeg worker fixture");
    let root = directory.path().join("repository");
    let tools = directory.path().join("tools");
    let fixture = prepare_native_android_ffmpeg_fixture(&root, &tools, "8.1.2");
    let output = root.join("plugin output");
    let metadata = root.join("metadata output");
    write_executable_script(&tools.join("cargo"), "#!/bin/sh\nexit 0\n");
    write_executable_script(
        &tools.join("cargo-ndk"),
        "#!/bin/sh\nset -eu\noutput=\"\"\nabi=\"\"\npackage=\"\"\nwhile [ \"$#\" -gt 0 ]; do\n  case \"$1\" in\n    -o) output=\"$2\"; shift 2 ;;\n    -t) abi=\"$2\"; shift 2 ;;\n    -p) package=\"$2\"; shift 2 ;;\n    *) shift ;;\n  esac\ndone\nif [ \"${VESPER_FAKE_CARGO_NDK_FAIL:-}\" = 1 ]; then exit 23; fi\nmkdir -p \"$output/$abi\"\ncase \"$package\" in\n  player-remux-ffmpeg) artifact=libvesper_remux_ffmpeg.so ;;\n  player-source-normalizer-ffmpeg) artifact=libvesper_source_normalizer_ffmpeg.so ;;\n  *) exit 24 ;;\nesac\nprintf 'plugin\\n' > \"$output/$abi/$artifact\"\n",
    );
    let success = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", &fixture.path)
        .env(
            "VESPER_FAKE_RUST_TARGET_LIBDIR",
            &fixture.rust_target_libdir,
        )
        .env("ANDROID_SDK_ROOT", &fixture.sdk)
        .args(["android", "remux-plugin", "--root"])
        .arg(&root)
        .arg(&output)
        .arg("release")
        .output()
        .expect("run Android FFmpeg remux worker");
    assert_eq!(
        success.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&success.stderr)
    );
    assert!(output.join("arm64-v8a/libvesper_remux_ffmpeg.so").is_file());

    fs::create_dir_all(output.join("arm64-v8a")).expect("recreate Android plugin output");
    fs::write(output.join("arm64-v8a/previous.txt"), b"previous\n")
        .expect("seed previous Android plugin output");
    fs::create_dir_all(&metadata).expect("create previous Android metadata");
    fs::write(metadata.join("previous.txt"), b"previous metadata\n")
        .expect("seed previous Android metadata");
    let failure = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", &fixture.path)
        .env(
            "VESPER_FAKE_RUST_TARGET_LIBDIR",
            &fixture.rust_target_libdir,
        )
        .env("ANDROID_SDK_ROOT", &fixture.sdk)
        .env("VESPER_FAKE_CARGO_NDK_FAIL", "1")
        .args(["android", "source-normalizer-plugin", "--root"])
        .arg(&root)
        .arg(&output)
        .arg("release")
        .arg("--metadata-dir")
        .arg(&metadata)
        .output()
        .expect("run failing Android FFmpeg SourceNormalizer worker");
    assert_eq!(failure.status.code(), Some(5));
    assert_eq!(
        fs::read(output.join("arm64-v8a/previous.txt")).unwrap(),
        b"previous\n"
    );
    assert_eq!(
        fs::read(metadata.join("previous.txt")).unwrap(),
        b"previous metadata\n"
    );
}

#[cfg(unix)]
#[test]
fn ios_subtitle_cli_is_typed_and_does_not_execute_a_shell_worker() {
    let directory = tempfile::tempdir().expect("create iOS subtitle CLI fixture");
    let root = directory.path().join("repository");
    let scripts = root.join("scripts");
    fs::create_dir_all(&scripts).expect("create Vesper script directory");
    fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("write iOS worker manifest");
    fs::write(
        scripts.join("ffmpeg-source-policy.toml"),
        "invalid policy\n",
    )
    .expect("write unrelated invalid FFmpeg release policy");
    let wrapper = scripts.join("vesper");
    fs::copy(workspace_root().join("scripts/vesper"), &wrapper)
        .expect("copy Vesper wrapper into iOS fixture");
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700))
            .expect("make iOS wrapper executable");
    }

    let output = Command::new(&wrapper)
        .env("VESPER_CLI", env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "verify-subtitles", "--root"])
        .arg(&root)
        .args(["--scope", "regression", "--device", "physical-device"])
        .output()
        .expect("run typed iOS subtitle CLI through Vesper wrapper");
    assert_eq!(
        output.status.code(),
        Some(2),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("--device is not used by iOS subtitle scope 'regression'")
    );
    let help = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "verify-subtitles", "--help"])
        .output()
        .expect("read typed iOS subtitle help");
    assert_eq!(help.status.code(), Some(0));
    assert!(help.stderr.is_empty());
    let help = String::from_utf8_lossy(&help.stdout);
    for option in [
        "--scope",
        "--device",
        "--simulator",
        "--evidence-dir",
        "--development-team",
    ] {
        assert!(help.contains(option), "missing help option {option}");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffmpeg_runtime_release_routes_directly_to_rust() {
    let fixture = ios_plugin_fixture();
    configure_ios_plugin_ffmpeg_fixture(&fixture);
    let output = fixture.directory.path().join("runtime dry-run output");

    let result = ios_ffmpeg_runtime_fixture_command(&fixture, &output)
        .arg("--dry-run")
        .output()
        .expect("run Rust-native FFmpeg runtime dry-run");

    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stderr.is_empty());
    let stdout = String::from_utf8(result.stdout).expect("UTF-8 runtime dry-run output");
    assert!(stdout.contains("Resolved iOS FFmpeg component framework release:"));
    assert!(stdout.contains("profile_hash=custom-e1eabc3db7bc"));
    assert!(stdout.contains("ios-arm64"));
    assert!(stdout.contains("ios-simulator-arm64"));
    assert!(!output.exists());
}

#[cfg(unix)]
#[test]
fn ios_optional_release_routes_to_rust_without_a_shell_worker() {
    let directory = tempfile::tempdir().expect("create optional iOS routing fixture");
    let root = directory.path().join("repository");
    let scripts = root.join("scripts");
    fs::create_dir_all(&scripts).expect("create optional iOS routing scripts");
    fs::write(root.join("Cargo.toml"), "[workspace]\n")
        .expect("write optional iOS routing manifest");
    fs::copy(
        workspace_root().join("scripts/ffmpeg-source-policy.toml"),
        scripts.join("ffmpeg-source-policy.toml"),
    )
    .expect("copy optional iOS routing source policy");
    let wrapper = scripts.join("vesper");
    fs::copy(workspace_root().join("scripts/vesper"), &wrapper)
        .expect("copy Vesper wrapper into optional iOS routing fixture");
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700))
            .expect("make optional iOS routing wrapper executable");
    }
    assert!(
        !root
            .join("scripts/ios/stage-player-optional-plugins-release.sh")
            .exists()
    );
    let output = directory.path().join("optional release output");

    let result = Command::new(&wrapper)
        .env("VESPER_CLI", env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "stage-optional-plugins-release", "--root"])
        .arg(&root)
        .arg(&output)
        .arg("--dry-run")
        .output()
        .expect("route optional iOS release through Vesper wrapper");

    assert_eq!(result.status.code(), Some(0));
    assert!(result.stderr.is_empty());
    assert!(
        String::from_utf8_lossy(&result.stdout).contains("Resolved optional iOS plugin release:")
    );
    assert!(!output.exists());
}

fn run_native_fixture(
    manifest_path: &std::path::Path,
    artifact: &std::path::Path,
    behavior: &str,
    timeout_ms: u64,
) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("VESPER_PLUGIN_FIXTURE_WORKER_BEHAVIOR", behavior)
        .args(["plugin", "inspect"])
        .arg(manifest_path)
        .arg("--artifact")
        .arg(artifact)
        .args(["--transport", "native", "--timeout-ms"])
        .arg(timeout_ms.to_string())
        .output()
        .expect("run native fixture behavior")
}

#[test]
fn descriptor_command_separates_results_from_diagnostics() {
    let path = temporary_manifest(&manifest("dev.vesper.fixture"));
    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["plugin", "descriptor"])
        .arg(&path)
        .arg("--hash-only")
        .output()
        .expect("run vesper");
    let _ = fs::remove_file(path);

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert_eq!(stdout.trim().len(), 64);
    assert!(stdout.trim().bytes().all(|byte| byte.is_ascii_hexdigit()));
}

#[cfg(unix)]
#[test]
fn registry_fragment_output_is_exact_canonical_json() {
    let source = manifest("dev.vesper.fixture");
    let manifest_path = temporary_manifest(&source);
    let directory = tempfile::tempdir().expect("temporary registry output directory");
    let artifact_path = directory.path().join("libvesper_fixture.so");
    let output_path = directory.path().join("dev.vesper.fixture.json");
    fs::write(&artifact_path, b"fixture plugin binary").expect("write fixture plugin binary");

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["plugin", "registry-fragment"])
        .arg(&manifest_path)
        .args([
            "--platform",
            "android",
            "--target",
            "aarch64-linux-android",
            "--architecture",
            "arm64-v8a",
            "--minimum-os",
            "26",
            "--locator-name",
            "vesper_fixture",
        ])
        .arg("--artifact")
        .arg(&artifact_path)
        .arg("--output")
        .arg(&output_path)
        .output()
        .expect("generate registry fragment");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());

    let descriptor = PluginDescriptor::from_toml(&source)
        .expect("parse plugin descriptor")
        .canonicalize()
        .expect("canonicalize plugin descriptor");
    let expected = EmbeddedRegistryFragment::generate(
        &descriptor,
        &EmbeddedRegistryTarget::AndroidNativeLibrary {
            target: "aarch64-linux-android".to_owned(),
            architecture: "arm64-v8a".to_owned(),
            minimum_os: "26".to_owned(),
            library_name: "vesper_fixture".to_owned(),
            artifact_path,
        },
    )
    .expect("generate expected registry fragment");
    let actual = fs::read(&output_path).expect("read registry fragment output");
    let _ = fs::remove_file(manifest_path);

    assert_eq!(actual, expected.canonical_json());
    assert!(!actual.ends_with(b"\n"));
}

#[test]
fn invalid_manifest_uses_the_manifest_exit_code() {
    let path = temporary_manifest(&manifest("Invalid Plugin"));
    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["plugin", "descriptor"])
        .arg(&path)
        .output()
        .expect("run vesper");
    let _ = fs::remove_file(path);

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 stderr")
            .contains("plugin.id")
    );
}

#[test]
fn clap_usage_errors_keep_exit_code_two() {
    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["plugin", "registry-fragment"])
        .output()
        .expect("run vesper");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 stderr")
            .contains("Usage:")
    );
}

#[test]
fn ios_commands_are_exposed_by_clap() {
    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "--help"])
        .output()
        .expect("show iOS CLI help");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 iOS CLI help");
    assert!(stdout.contains("sync-bridge-shim"));
    assert!(stdout.contains("verify-bridge-shim"));
    assert!(stdout.contains("verify-app-store-layout"));
    assert!(stdout.contains("verify-native-frame"));
    assert!(stdout.contains("verify-optional-plugins-release"));
    assert!(stdout.contains("verify-optional-plugins-device"));
    assert!(stdout.contains("verify-release"));
    assert!(stdout.contains("stage-release"));
}

#[test]
fn ios_optional_plugin_device_verifier_exposes_explicit_device_contract() {
    let help = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "verify-optional-plugins-device", "--help"])
        .output()
        .expect("show iOS optional-plugin device verifier help");

    assert_eq!(help.status.code(), Some(0));
    assert!(help.stderr.is_empty());
    let stdout = String::from_utf8(help.stdout).expect("UTF-8 iOS device verifier help");
    assert!(stdout.contains("<RELEASE_DIRECTORY>"));
    for argument in [
        "--device",
        "--development-team",
        "--output-directory",
        "--allow-provisioning-updates",
    ] {
        assert!(stdout.contains(argument), "missing {argument} in help");
    }

    let missing = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "verify-optional-plugins-device"])
        .output()
        .expect("run iOS device verifier without required arguments");
    assert_eq!(missing.status.code(), Some(2));
    assert!(missing.stdout.is_empty());
}

#[cfg(target_os = "macos")]
fn run_ios_optional_device_preflight_fixture(
    mutate: impl FnOnce(&std::path::Path),
) -> (std::process::Output, tempfile::TempDir, PathBuf, PathBuf) {
    let (directory, root, release) = ios_verify_release_fixture();
    write_invalid_optional_ios_release_assets(&release);
    mutate(&release);
    let example = root.join("examples/ios-swift-host");
    fs::create_dir_all(&example).expect("create iOS device example fixture");
    fs::write(example.join("project.yml"), "name: Fixture\n")
        .expect("write iOS device XcodeGen fixture");
    fs::create_dir_all(root.join("lib/ios/VesperPlayerKit"))
        .expect("create iOS device package fixture");

    let tools = directory.path().join("device verifier tools");
    let tool_log = directory.path().join("device verifier tool log");
    fs::create_dir(&tools).expect("create iOS device tool fixture");
    for tool in ["xcodegen", "xcodebuild", "xcrun"] {
        write_executable_script(
            &tools.join(tool),
            "#!/bin/sh\nprintf '%s\\n' \"$0 $*\" >> \"$VESPER_TEST_TOOL_LOG\"\nexit 99\n",
        );
    }
    let path = std::env::join_paths([
        tools.as_path(),
        std::path::Path::new("/usr/bin"),
        std::path::Path::new("/bin"),
    ])
    .expect("compose iOS device verifier fixture PATH");
    let output_directory = directory.path().join("device output must not exist");
    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", path)
        .env("VESPER_TEST_TOOL_LOG", &tool_log)
        .args(["ios", "--root"])
        .arg(&root)
        .arg("verify-optional-plugins-device")
        .arg(&release)
        .args(["--device", "00008140-000471243E29801C"])
        .args(["--development-team", "983LPXU7G4"])
        .arg("--output-directory")
        .arg(&output_directory)
        .output()
        .expect("run iOS optional-plugin device preflight fixture");
    assert!(
        !tool_log.exists(),
        "an Apple tool ran before release preflight"
    );
    assert!(
        !output_directory.exists(),
        "device output was created before release preflight"
    );
    (output, directory, release, tool_log)
}

#[cfg(target_os = "macos")]
#[test]
fn ios_optional_plugin_device_rejects_malformed_release_before_apple_tools() {
    let (output, _directory, _release, _tool_log) =
        run_ios_optional_device_preflight_fixture(|_| {});
    assert_eq!(
        output.status.code(),
        Some(5),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("optional iOS FFmpeg source SHA-256 is")
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_optional_plugin_device_rejects_extra_release_asset_before_apple_tools() {
    let (output, _directory, _release, _tool_log) =
        run_ios_optional_device_preflight_fixture(|release| {
            fs::write(release.join("unexpected.txt"), b"unexpected\n")
                .expect("write unexpected release fixture asset");
        });
    assert_eq!(
        output.status.code(),
        Some(5),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Unexpected top-level iOS release asset")
    );
}

#[test]
fn ios_native_frame_verifier_keeps_clap_usage_and_help_contracts() {
    let help = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "verify-native-frame", "--help"])
        .output()
        .expect("show iOS native-frame verifier help");
    assert_eq!(help.status.code(), Some(0));
    assert!(help.stderr.is_empty());
    let stdout = String::from_utf8(help.stdout).expect("UTF-8 iOS native-frame verifier help");
    for value in ["debug", "release", "swift-smoke"] {
        assert!(stdout.contains(value));
    }

    let invalid = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "verify-native-frame", "unsupported"])
        .output()
        .expect("run iOS native-frame verifier with an invalid token");
    assert_eq!(invalid.status.code(), Some(2));
    assert!(invalid.stdout.is_empty());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("invalid value 'unsupported'"));
}

#[cfg(target_os = "macos")]
#[test]
fn ios_native_frame_missing_tool_is_compatibility() {
    let fixture = ios_native_frame_fixture();
    fs::remove_file(fixture.tools.join("xcodebuild"))
        .expect("remove required iOS native-frame tool");

    let result = ios_native_frame_command(&fixture)
        .env("PATH", &fixture.tools)
        .output()
        .expect("run iOS native-frame verifier without xcodebuild");

    assert_eq!(result.status.code(), Some(4));
    assert!(result.stdout.is_empty());
    assert_eq!(
        String::from_utf8(result.stderr).expect("UTF-8 missing-tool diagnostic"),
        "Missing required command: xcodebuild\n"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_native_frame_rust_pipeline_owns_state_and_clears_shell_environment() {
    let fixture = ios_native_frame_fixture();
    let resolved_root =
        fs::canonicalize(&fixture.root).expect("canonical iOS native-frame fixture root");
    let protected_staging = fixture.root.join("protected staging");
    let ambient_config = fixture.root.join("ambient-smoke.plist");
    let bash_env = fixture.root.join("ambient-bash-env.sh");
    let bash_env_marker = fixture.root.join("bash-env-ran");
    fs::create_dir(&protected_staging).expect("create protected native-frame staging directory");
    fs::write(protected_staging.join("preserved.txt"), b"preserved\n")
        .expect("write protected native-frame staging marker");
    fs::write(&ambient_config, b"ambient config\n").expect("write ambient smoke config");
    fs::write(
        &bash_env,
        "printf 'injected\\n' > \"$VESPER_TEST_BASH_ENV_MARKER\"\n",
    )
    .expect("write ambient BASH_ENV script");

    let result = ios_native_frame_command(&fixture)
        .env("VESPER_REPO_ROOT", fixture.root.join("stale ambient root"))
        .env("VESPER_IOS_NATIVE_FRAME_STAGING_DIR", &protected_staging)
        .env("VESPER_IOS_NATIVE_FRAME_SMOKE_CONFIG", &ambient_config)
        .env("BASH_ENV", &bash_env)
        .env("ENV", &bash_env)
        .env("CDPATH", &fixture.root)
        .env("VESPER_TEST_BASH_ENV_MARKER", &bash_env_marker)
        .env("VESPER_APPLE_SH_INCLUDED", "poisoned")
        .env("VESPER_COMMON_SH_INCLUDED", "poisoned")
        .env("VESPER_FFMPEG_SH_INCLUDED", "poisoned")
        .env("VESPER_FFMPEG_PROFILE_SH_INCLUDED", "poisoned")
        .env("VESPER_FFMPEG_VALIDATE_SH_INCLUDED", "poisoned")
        .env("VESPER_IOS_FRAMEWORK_SH_INCLUDED", "poisoned")
        .env("VESPER_IOS_FFI_PREBUILT", "1")
        .env("VESPER_IOS_NATIVE_FRAME_SMOKE_ENABLED", "0")
        .env("VESPER_IOS_SOURCE_NORMALIZER_PLUGIN_PATH", &ambient_config)
        .env(
            "VESPER_IOS_DECODER_VIDEOTOOLBOX_PLUGIN_PATH",
            &ambient_config,
        )
        .env(
            "VESPER_IOS_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH",
            &ambient_config,
        )
        .args(["release", "swift-smoke", "debug"])
        .output()
        .expect("run Rust iOS native-frame verification");

    assert_eq!(
        result.status.code(),
        Some(0),
        "stdout: {}; stderr: {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stderr.is_empty());
    let stdout = String::from_utf8(result.stdout).expect("UTF-8 native-frame output");
    assert!(stdout.contains("Using iOS native-frame smoke source:"));
    assert!(stdout.contains("real iOS native-frame smoke presentedFrames=3"));
    assert!(stdout.contains("iOS native-frame Swift smoke passed"));

    let actions =
        fs::read_to_string(&fixture.actions).expect("read native-frame external action log");
    let plugin_lines = actions
        .lines()
        .filter(|line| line.starts_with("plugin command="))
        .collect::<Vec<_>>();
    assert_eq!(plugin_lines.len(), 3);
    for line in plugin_lines {
        assert!(line.contains(&format!("root={}", resolved_root.display())));
        assert!(line.contains("profile=debug"));
        assert!(line.contains("slice=ios-simulator-arm64"));
        assert!(line.contains("staging=unset"));
        assert!(line.contains("config=unset"));
        assert!(line.contains("ffi_prebuilt=unset"));
    }
    for line in actions.lines().filter(|line| {
        line.starts_with("plugin ")
            || line.starts_with("xcodegen ")
            || line.starts_with("xcodebuild ")
    }) {
        assert!(line.contains("bash_env=unset"), "{line}");
        assert!(line.contains("env=unset"), "{line}");
        assert!(line.contains("cdpath=unset"), "{line}");
        if line.starts_with("xcodegen ") || line.starts_with("xcodebuild ") {
            assert!(line.contains("ffi_prebuilt=unset"), "{line}");
        }
    }
    let generate = actions.find("xcodegen generate").expect("XcodeGen action");
    let build = actions
        .find("xcodebuild build-for-testing")
        .expect("Xcode build action");
    let test = actions
        .find("xcodebuild test-without-building")
        .expect("Xcode test action");
    assert!(generate < build && build < test);

    assert_eq!(
        native_frame_evidence(&fixture.evidence, "source"),
        fixture.source
    );
    for key in ["config", "source_plugin", "decoder_plugin", "frame_plugin"] {
        let path = native_frame_evidence(&fixture.evidence, key);
        assert!(
            !path.exists(),
            "owned native-frame path survived: {}",
            path.display()
        );
    }
    assert_eq!(
        fs::read(protected_staging.join("preserved.txt"))
            .expect("read protected native-frame staging marker"),
        b"preserved\n"
    );
    assert_eq!(
        fs::read(&ambient_config).expect("read preserved ambient smoke config"),
        b"ambient config\n"
    );
    assert!(!bash_env_marker.exists());
}

#[cfg(target_os = "macos")]
#[test]
fn ios_native_frame_ignores_ambient_staging_symlink() {
    use std::os::unix::fs::symlink;

    let fixture = ios_native_frame_fixture();
    let protected_staging = fixture.root.join("protected staging");
    let protected_link = fixture.root.join("protected staging link");
    fs::create_dir(&protected_staging).expect("create protected native-frame staging directory");
    fs::write(protected_staging.join("preserved.txt"), b"preserved\n")
        .expect("write protected native-frame staging marker");
    symlink(&protected_staging, &protected_link)
        .expect("create protected native-frame staging symlink");

    let result = ios_native_frame_command(&fixture)
        .env("VESPER_IOS_NATIVE_FRAME_STAGING_DIR", &protected_link)
        .output()
        .expect("run native-frame verifier with an ambient staging symlink");

    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read(protected_staging.join("preserved.txt"))
            .expect("read protected native-frame staging marker"),
        b"preserved\n"
    );
    assert!(
        fs::symlink_metadata(&protected_link)
            .expect("inspect protected native-frame staging symlink")
            .file_type()
            .is_symlink()
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_native_frame_rejects_symlinked_cached_smoke_media() {
    use std::os::unix::fs::symlink;

    let fixture = ios_native_frame_fixture();
    let generated = fixture
        .root
        .join("target/ios-native-frame-smoke-h264-aac.mp4");
    fs::create_dir_all(generated.parent().expect("generated media parent"))
        .expect("create generated media parent");
    symlink(&fixture.source, &generated).expect("create cached smoke media symlink");

    let result = ios_native_frame_command(&fixture)
        .env_remove("VESPER_IOS_NATIVE_FRAME_SMOKE_SOURCE")
        .output()
        .expect("run native-frame verifier with symlinked cached media");

    assert_eq!(result.status.code(), Some(5));
    assert!(result.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("cached iOS native-frame smoke source is not a bounded regular file")
    );
    assert!(
        fs::symlink_metadata(&generated)
            .expect("inspect cached smoke media symlink")
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read(&fixture.source).expect("read preserved source target"),
        b"native-frame fixture\n"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_native_frame_failure_keeps_stdout_empty_and_removes_owned_config() {
    let fixture = ios_native_frame_fixture();

    let result = ios_native_frame_command(&fixture)
        .env("VESPER_TEST_XCODE_MODE", "conformance")
        .output()
        .expect("run a failing Rust iOS native-frame verifier");

    assert_eq!(result.status.code(), Some(6));
    assert!(result.stdout.is_empty());
    let stderr = String::from_utf8(result.stderr).expect("UTF-8 native-frame failure diagnostic");
    assert!(stderr.contains("partial native-frame result"));
    assert!(stderr.contains("native-frame conformance failure"));
    let log = stderr
        .lines()
        .find_map(|line| line.strip_prefix("Log: "))
        .map(PathBuf::from)
        .expect("preserved native-frame failure log path");
    assert!(log.is_file());
    let log_contents = fs::read_to_string(&log).expect("read preserved native-frame failure log");
    assert!(log_contents.contains("partial native-frame result"));
    assert!(log_contents.contains("native-frame conformance failure"));
    fs::remove_file(log).expect("remove native-frame failure log fixture");
    let config = native_frame_evidence(&fixture.evidence, "config");
    assert!(!config.exists());
}

#[cfg(target_os = "macos")]
#[test]
fn ios_native_frame_missing_runtime_is_compatibility() {
    let fixture = ios_native_frame_fixture();
    fs::remove_file(
        fixture
            .root
            .join("third_party/ffmpeg/apple/ios-simulator/lib/arm64/libavutil.60.dylib"),
    )
    .expect("remove fixture FFmpeg runtime");

    let result = ios_native_frame_command(&fixture)
        .output()
        .expect("run native-frame verifier without FFmpeg runtime");

    assert_eq!(result.status.code(), Some(4));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Missing FFmpeg runtime dylibs"));
}

#[cfg(target_os = "macos")]
#[test]
fn ios_native_frame_rejects_runtime_directory_entry_overflow() {
    let fixture = ios_native_frame_fixture();
    let runtime = fixture
        .root
        .join("third_party/ffmpeg/apple/ios-simulator/lib/arm64");
    for index in 0..256 {
        fs::write(runtime.join(format!("ignored-{index}")), b"fixture\n")
            .expect("write excess FFmpeg runtime entry");
    }

    let result = ios_native_frame_command(&fixture)
        .output()
        .expect("run native-frame verifier with excess runtime entries");

    assert_eq!(result.status.code(), Some(5));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("contains more than 256 entries"));
}

#[cfg(target_os = "macos")]
#[test]
fn ios_native_frame_rejects_runtime_dylib_symlink_outside_runtime() {
    use std::os::unix::fs::symlink;

    let fixture = ios_native_frame_fixture();
    let runtime = fixture
        .root
        .join("third_party/ffmpeg/apple/ios-simulator/lib/arm64");
    let dylib = runtime.join("libavutil.60.dylib");
    let outside = fixture._directory.path().join("outside-libavutil.60.dylib");
    fs::write(&outside, b"outside runtime\n").expect("write outside runtime dylib");
    fs::remove_file(&dylib).expect("remove contained runtime dylib");
    symlink(&outside, &dylib).expect("link runtime dylib outside its root");

    let result = ios_native_frame_command(&fixture)
        .output()
        .expect("run native-frame verifier with escaped runtime dylib");

    assert_eq!(result.status.code(), Some(5));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("must resolve inside"));
    assert_eq!(
        fs::read(outside).expect("read preserved outside runtime dylib"),
        b"outside runtime\n"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_native_frame_rejects_xcode_product_directory_entry_overflow() {
    let fixture = ios_native_frame_fixture();
    let result = ios_native_frame_command(&fixture)
        .env("VESPER_TEST_XCTESTRUN_EXTRA_ENTRIES", "256")
        .output()
        .expect("run native-frame verifier with excess Xcode products");

    assert_eq!(result.status.code(), Some(5));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("contain more than 256 entries"));
}

#[cfg(target_os = "macos")]
#[test]
fn ios_native_frame_adds_exact_loader_rpath_when_only_parent_is_present() {
    let fixture = ios_native_frame_fixture();
    let result = ios_native_frame_command(&fixture)
        .env("VESPER_TEST_OTOOL_RPATH", "parent")
        .output()
        .expect("run native-frame verifier with a parent-only loader rpath");

    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let actions = fs::read_to_string(&fixture.actions).expect("read native-frame action log");
    assert_eq!(
        actions
            .lines()
            .filter(|line| line.contains("install_name_tool -add_rpath @loader_path "))
            .count(),
        4
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_native_frame_cli_child_exit_codes_keep_cli_semantics() {
    for status in [3, 4, 5, 6] {
        let fixture = ios_native_frame_fixture();
        let result = ios_native_frame_command(&fixture)
            .env("VESPER_TEST_PLUGIN_CLI_STATUS", status.to_string())
            .output()
            .expect("run native-frame CLI child status fixture");

        assert_eq!(
            result.status.code(),
            Some(status),
            "child status {status}; stderr: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8_lossy(&result.stderr).contains("fixture plugin CLI failure"));
    }
}

#[cfg(target_os = "macos")]
#[test]
fn ios_native_frame_external_tool_failures_are_worker_failures() {
    for (mode, diagnostic) in [
        ("compatibility", "Missing FFmpeg build input provenance"),
        ("arbitrary", "fixture: command not found"),
        ("signal", "terminated without an exit code"),
    ] {
        let fixture = ios_native_frame_fixture();
        let result = ios_native_frame_command(&fixture)
            .env("VESPER_TEST_XCODE_MODE", mode)
            .output()
            .expect("run native-frame child status fixture");

        assert_eq!(
            result.status.code(),
            Some(6),
            "mode {mode}; stderr: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&result.stderr).contains(diagnostic),
            "mode {mode}; stderr: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn ios_native_frame_rejects_missing_or_failed_smoke_markers() {
    for (mode, diagnostic) in [
        ("no-summary", "did not report its real playback summary"),
        ("release-failure", "reported a frame release failure"),
        (
            "duplicate-summary",
            "playback summaries; expected exactly one",
        ),
    ] {
        let fixture = ios_native_frame_fixture();
        let result = ios_native_frame_command(&fixture)
            .env("VESPER_TEST_XCODE_MODE", mode)
            .output()
            .expect("run native-frame smoke marker fixture");

        assert_eq!(
            result.status.code(),
            Some(5),
            "mode {mode}; stderr: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&result.stderr).contains(diagnostic),
            "mode {mode}; stderr: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn ios_native_frame_keeps_success_results_and_diagnostics_on_separate_streams() {
    let fixture = ios_native_frame_fixture();
    let result = ios_native_frame_command(&fixture)
        .env("VESPER_TEST_XCODE_MODE", "success-with-diagnostic")
        .output()
        .expect("run native-frame stdout and stderr fixture");

    assert_eq!(result.status.code(), Some(0));
    let stdout = String::from_utf8(result.stdout).expect("UTF-8 native-frame success output");
    let stderr = String::from_utf8(result.stderr).expect("UTF-8 native-frame diagnostics");
    assert!(stdout.contains("real iOS native-frame smoke presentedFrames=3"));
    assert!(!stdout.contains("fixture xcode diagnostic"));
    assert!(stderr.contains("fixture xcode diagnostic"));
}

#[cfg(target_os = "macos")]
#[test]
fn ios_native_frame_ctrl_c_reaps_xcode_descendants_and_owned_state() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let fixture = ios_native_frame_fixture();
    let worker_pid_path = fixture.root.join("native-frame-worker.pid");
    let descendant_pid_path = fixture.root.join("native-frame-descendant.pid");
    let process = ios_native_frame_command(&fixture)
        .env("VESPER_TEST_XCODE_MODE", "wait")
        .env("VESPER_TEST_NATIVE_FRAME_WORKER_PID", &worker_pid_path)
        .env(
            "VESPER_TEST_NATIVE_FRAME_DESCENDANT_PID",
            &descendant_pid_path,
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cancellable iOS native-frame verification");
    let parent_pid = i32::try_from(process.id()).expect("iOS CLI parent pid fits i32");
    let ready_deadline = Instant::now() + Duration::from_secs(10);
    while (!worker_pid_path.is_file()
        || !descendant_pid_path.is_file()
        || !fixture.evidence.is_file())
        && Instant::now() < ready_deadline
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    if !worker_pid_path.is_file() || !descendant_pid_path.is_file() || !fixture.evidence.is_file() {
        let _ = kill(Pid::from_raw(parent_pid), Signal::SIGINT);
        let output = process
            .wait_with_output()
            .expect("wait after native-frame readiness timeout");
        panic!(
            "native-frame Xcode worker did not start; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let worker_pid: i32 = fs::read_to_string(&worker_pid_path)
        .expect("read native-frame Xcode worker pid")
        .parse()
        .expect("parse native-frame Xcode worker pid");
    let descendant_pid: i32 = fs::read_to_string(&descendant_pid_path)
        .expect("read native-frame Xcode descendant pid")
        .parse()
        .expect("parse native-frame Xcode descendant pid");
    let config = native_frame_evidence(&fixture.evidence, "config");
    assert!(config.is_file());

    kill(Pid::from_raw(parent_pid), Signal::SIGINT).expect("cancel iOS native-frame verification");
    let output = process
        .wait_with_output()
        .expect("wait for cancelled iOS native-frame verification");
    assert_eq!(output.status.code(), Some(6));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("was cancelled"));
    assert!(!config.exists());

    for pid in [worker_pid, descendant_pid] {
        let reap_deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match kill(Pid::from_raw(pid), None) {
                Err(Errno::ESRCH) => break,
                _ if Instant::now() < reap_deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                result => panic!("iOS native-frame process {pid} is still alive: {result:?}"),
            }
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn vesper_wrapper_routes_ios_native_frame_to_the_rust_cli() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = ios_native_frame_fixture();
    let public_scripts = fixture.root.join("public-scripts");
    fs::create_dir(&public_scripts).expect("create public wrapper fixture directory");
    let wrapper = public_scripts.join("vesper");
    fs::copy(workspace_root().join("scripts/vesper"), &wrapper)
        .expect("copy Vesper wrapper into iOS native-frame fixture");
    let mut permissions = fs::metadata(&wrapper)
        .expect("inspect fixture Vesper wrapper")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&wrapper, permissions).expect("make fixture Vesper wrapper executable");
    let path = format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", fixture.tools.display());

    let result = Command::new(&wrapper)
        .env("PATH", path)
        .env("VESPER_CLI", env!("CARGO_BIN_EXE_vesper"))
        .env("VESPER_IOS_NATIVE_FRAME_SMOKE_SOURCE", &fixture.source)
        .env(
            "VESPER_IOS_NATIVE_FRAME_DESTINATION",
            "platform=iOS Simulator,id=00000000-0000-0000-0000-000000000001",
        )
        .env("VESPER_TEST_NATIVE_FRAME_ACTIONS", &fixture.actions)
        .env("VESPER_TEST_NATIVE_FRAME_EVIDENCE", &fixture.evidence)
        .args(["ios", "verify-native-frame", "release", "swift-smoke"])
        .output()
        .expect("route iOS native-frame verification through the public wrapper");

    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        String::from_utf8_lossy(&result.stdout).contains("iOS native-frame Swift smoke passed")
    );
    assert!(result.stderr.is_empty());
}
#[cfg(not(target_os = "macos"))]
#[test]
fn ios_native_frame_rejects_non_macos_without_parsing_root_or_writing() {
    let directory = tempfile::tempdir().expect("temporary non-macOS native-frame fixture");
    let root = directory.path().join("repository");
    let scratch = root.join("command temporary directory");

    let result = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("TMPDIR", &scratch)
        .env("TMP", &scratch)
        .env("TEMP", &scratch)
        .args(["ios", "--root"])
        .arg(&root)
        .arg("verify-native-frame")
        .output()
        .expect("reject iOS native-frame verification on a non-macOS host");

    assert_eq!(result.status.code(), Some(4));
    assert!(result.stdout.is_empty());
    assert_eq!(
        String::from_utf8(result.stderr).expect("UTF-8 non-macOS native-frame verifier diagnostic"),
        "iOS native-frame pipeline verification requires macOS\n"
    );
    for path in [root, scratch] {
        assert!(!path.exists(), "unexpected write: {}", path.display());
    }
}

#[test]
fn ios_optional_release_verifier_keeps_clap_usage_and_help_contracts() {
    let help = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "verify-optional-plugins-release", "--help"])
        .output()
        .expect("show optional iOS release verifier help");
    assert_eq!(help.status.code(), Some(0));
    assert!(help.stderr.is_empty());
    assert!(
        String::from_utf8(help.stdout)
            .expect("UTF-8 optional iOS verifier help")
            .contains("[RELEASE_DIRECTORY]")
    );

    let invalid = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "verify-optional-plugins-release", "first", "second"])
        .output()
        .expect("run invalid optional iOS release verifier command");
    assert_eq!(invalid.status.code(), Some(2));
    assert!(invalid.stdout.is_empty());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("Usage:"));
}

#[test]
fn ios_release_verifier_keeps_clap_usage_and_help_contracts() {
    let help = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "verify-release", "--help"])
        .output()
        .expect("show iOS release verifier help");
    assert_eq!(help.status.code(), Some(0));
    assert!(help.stderr.is_empty());
    let stdout = String::from_utf8(help.stdout).expect("UTF-8 iOS release verifier help");
    assert!(stdout.contains("[RELEASE_DIRECTORY]"));
    assert!(stdout.contains("--scope <SCOPE>"));
    assert!(stdout.contains("core"));
    assert!(stdout.contains("complete"));

    let invalid = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "verify-release", "--scope", "unsupported"])
        .output()
        .expect("run iOS release verifier with an invalid scope");
    assert_eq!(invalid.status.code(), Some(2));
    assert!(invalid.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&invalid.stderr);
    assert!(stderr.contains("invalid value 'unsupported'"));
    assert!(stderr.contains("possible values: core, complete"));
}

#[cfg(target_os = "macos")]
#[test]
fn ios_release_rejects_wrong_core_zip_root_during_rust_preflight() {
    let (_directory, root, release) = ios_verify_release_fixture();
    write_core_ios_release_zip(
        &release.join("VesperPlayerKit-ios-arm64.framework.zip"),
        "Unexpected.framework",
    );
    let result = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "--root"])
        .arg(&root)
        .arg("verify-release")
        .arg(&release)
        .output()
        .expect("reject a core iOS release ZIP with the wrong root");

    assert_eq!(result.status.code(), Some(5));
    assert!(result.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("must contain exactly one VesperPlayerKit.framework root")
    );
}

#[cfg(target_os = "macos")]
#[test]
fn complete_ios_release_checks_locked_source_before_archive_preflight() {
    let (_directory, root, release) = ios_verify_release_fixture();
    write_invalid_optional_ios_release_assets(&release);

    let result = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "--root"])
        .arg(&root)
        .arg("verify-release")
        .arg(&release)
        .args(["--scope", "complete"])
        .output()
        .expect("preflight a malformed complete iOS release");

    assert_eq!(result.status.code(), Some(5));
    assert!(result.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("optional iOS FFmpeg source SHA-256 is")
    );
}

#[cfg(target_os = "macos")]
#[test]
fn complete_ios_release_applies_core_policy_in_rust() {
    let (_directory, root, release) = ios_verify_release_fixture();
    write_invalid_optional_ios_release_assets(&release);
    write_core_ios_release_zip(
        &release.join("VesperPlayerKit.xcframework.zip"),
        "Unexpected.xcframework",
    );
    let result = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "--root"])
        .arg(&root)
        .arg("verify-release")
        .arg(&release)
        .args(["--scope", "complete"])
        .output()
        .expect("preflight a complete iOS release with a wrong core root");

    assert_eq!(result.status.code(), Some(5));
    assert!(result.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("must contain exactly one VesperPlayerKit.xcframework root")
    );
}

#[cfg(not(target_os = "macos"))]
#[test]
fn ios_release_rejects_non_macos_without_parsing_root_or_writing() {
    let directory = tempfile::tempdir().expect("temporary non-macOS iOS verifier fixture");
    let root = directory.path().join("repository");
    let release = root.join("release output");
    let scratch = root.join("command temporary directory");

    let result = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("TMPDIR", &scratch)
        .env("TMP", &scratch)
        .env("TEMP", &scratch)
        .args(["ios", "--root"])
        .arg(&root)
        .arg("verify-release")
        .arg(&release)
        .output()
        .expect("reject iOS release verification on a non-macOS host");

    assert_eq!(result.status.code(), Some(4));
    assert!(result.stdout.is_empty());
    assert_eq!(
        String::from_utf8(result.stderr).expect("UTF-8 non-macOS iOS verifier diagnostic"),
        "iOS release verification requires macOS\n"
    );
    for path in [root, release, scratch] {
        assert!(!path.exists(), "unexpected write: {}", path.display());
    }
}

#[cfg(target_os = "macos")]
#[test]
fn vesper_wrapper_routes_ios_verify_release_help_to_the_rust_cli() {
    let result = Command::new(workspace_root().join("scripts/vesper"))
        .env("VESPER_CLI", env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "verify-release", "--help"])
        .output()
        .expect("route iOS release verification through the public wrapper");

    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stderr.is_empty());
    assert!(String::from_utf8_lossy(&result.stdout).contains("--scope <SCOPE>"));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn ios_optional_release_rejects_non_macos_without_writing() {
    let directory = tempfile::tempdir().expect("temporary non-macOS optional verifier fixture");
    let root = directory.path().join("repository");
    let release = root.join("release output");
    let scratch = root.join("command temporary directory");

    let result = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("TMPDIR", &scratch)
        .env("TMP", &scratch)
        .env("TEMP", &scratch)
        .args(["ios", "--root"])
        .arg(&root)
        .arg("verify-optional-plugins-release")
        .arg(&release)
        .output()
        .expect("reject optional verifier on a non-macOS host");

    assert_eq!(result.status.code(), Some(4));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("requires macOS"));
    for path in [root, release, scratch] {
        assert!(!path.exists(), "unexpected write: {}", path.display());
    }
}

#[test]
fn ios_stage_release_keeps_clap_usage_and_help_contracts() {
    let help = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "stage-release", "--help"])
        .output()
        .expect("show iOS release staging help");
    assert_eq!(help.status.code(), Some(0));
    assert!(help.stderr.is_empty());
    let stdout = String::from_utf8(help.stdout).expect("UTF-8 iOS release staging help");
    assert!(stdout.contains("[OUTPUT_DIRECTORY]"));
    assert!(stdout.contains("--include-optional-plugins"));
    assert!(stdout.contains("--package-artifacts-directory"));

    let invalid = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "stage-release", "--unknown"])
        .output()
        .expect("run invalid iOS release staging command");
    assert_eq!(invalid.status.code(), Some(2));
    assert!(invalid.stdout.is_empty());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("Usage:"));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn ios_stage_release_rejects_non_macos_without_writing() {
    let directory = tempfile::tempdir().expect("temporary non-macOS iOS release fixture");
    let root = directory.path().join("repository");
    fs::create_dir(&root).expect("create non-macOS repository fixture");
    fs::write(root.join("Cargo.toml"), "[workspace]\n")
        .expect("write non-macOS repository manifest");
    let output = root.join("release output");
    let package = root.join("dist/package/Artifacts");
    let scratch = root.join("command temporary directory");

    let result = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("TMPDIR", &scratch)
        .env("TMP", &scratch)
        .env("TEMP", &scratch)
        .args(["ios", "--root"])
        .arg(&root)
        .arg("stage-release")
        .arg(&output)
        .arg("--include-optional-plugins")
        .arg("--package-artifacts-directory")
        .arg(&package)
        .output()
        .expect("reject iOS release staging on a non-macOS host");

    assert_eq!(result.status.code(), Some(4));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("requires macOS"));
    for path in [
        output,
        package,
        root.join("lib/ios/VesperPlayerKit/.build/vesper-cli-state"),
        root.join("target"),
        root.join("stage-release.log"),
        scratch,
    ] {
        assert!(!path.exists(), "unexpected write: {}", path.display());
    }
}

#[cfg(target_os = "macos")]
#[test]
fn ios_stage_release_stages_core_and_optional_outputs_transactionally() {
    let (_directory, root, tools) = ios_stage_release_fixture();
    let output = root.join("release output - 产物");
    let package_artifacts = root.join("dist/package output - 产物/Artifacts");
    fs::create_dir_all(&output).expect("create previous iOS release output");
    fs::write(
        output.join("VesperPlayerKit-ios-arm64.framework.zip"),
        b"previous core archive\n",
    )
    .expect("write previous core archive");
    fs::write(
        output.join("VesperPlayerOptionalPlugins-FFmpeg-old-source.tar.xz"),
        b"stale optional source\n",
    )
    .expect("write stale optional source");
    fs::write(output.join("unrelated.txt"), b"preserve me\n")
        .expect("write unrelated release file");

    let result = ios_stage_release_command(&root, &tools, &output)
        .env("VESPER_IOS_INCLUDE_OPTIONAL_PLUGINS", "yes")
        .env(
            "VESPER_IOS_OPTIONAL_PACKAGE_ARTIFACTS_DIR",
            root.join("unsafe ambient target"),
        )
        .env("CARGO_TARGET_DIR", root.join("stale custom cargo target"))
        .env("VESPER_BUILD_IOS_PLAYER_FFI_MODE", "platform")
        .env("VESPER_BUILD_APPLE_CATALYST", "1")
        .env("VESPER_IOS_SIMULATOR_ARCHS", "x86_64")
        .env("PLATFORM_NAME", "macosx")
        .arg("--package-artifacts-directory")
        .arg(&package_artifacts)
        .output()
        .expect("stage complete iOS release fixture");
    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}\ntool log:\n{}",
        String::from_utf8_lossy(&result.stderr),
        fs::read_to_string(root.join("stage-release.log"))
            .unwrap_or_else(|error| format!("<unavailable: {error}>"))
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("Staged VesperPlayerKit"));

    for asset in [
        "VesperPlayerKit-ios-arm64.framework.zip",
        "VesperPlayerKit-ios-simulator-arm64.framework.zip",
        "VesperPlayerKit.xcframework.zip",
        "VesperPlayerOptionalPlugins-FFmpeg-Compliance.zip",
        "VesperPlayerOptionalPlugins-FFmpeg-8.1.2-source.tar.xz",
    ] {
        let metadata =
            fs::symlink_metadata(output.join(asset)).expect("inspect staged release asset");
        assert!(metadata.file_type().is_file(), "{asset}");
        assert!(metadata.len() > 0, "{asset}");
    }
    for framework in IOS_RELEASE_OPTIONAL_FRAMEWORKS {
        assert!(
            output
                .join(format!("{framework}.xcframework.zip"))
                .is_file()
        );
        assert!(
            package_artifacts
                .join(format!("{framework}.xcframework"))
                .is_dir()
        );
    }
    assert!(
        !output
            .join("VesperPlayerOptionalPlugins-FFmpeg-old-source.tar.xz")
            .exists()
    );
    assert_eq!(
        fs::read(output.join("unrelated.txt")).expect("read unrelated release file"),
        b"preserve me\n"
    );
    assert!(!root.join("unsafe ambient target").exists());

    let log =
        fs::read_to_string(root.join("stage-release.log")).expect("read iOS release staging log");
    assert!(log.contains("xcodegen:generate --spec"));
    let canonical_root = root
        .canonicalize()
        .expect("canonical iOS release fixture root");
    assert!(log.contains(&format!(
        "--target-dir {}",
        canonical_root.join("target").display()
    )));
    assert!(log.contains("aarch64-apple-ios"));
    assert!(log.contains("aarch64-apple-ios-sim"));
    assert!(log.contains("lipo:-info"));
    assert!(log.contains("-extract arm64"));
    assert!(
        log.lines()
            .filter(|line| line.starts_with("xcodebuild:-create-xcframework"))
            .count()
            >= 7
    );
    for framework in [
        "VesperFFmpegAVCodec",
        "VesperFFmpegAVFormat",
        "VesperFFmpegAVUtil",
    ] {
        assert!(
            root.join("lib/ios/VesperPlayerKit/.build/player-ffmpeg-runtime")
                .join(format!("{framework}.xcframework"))
                .is_dir(),
            "missing canonical aggregate runtime framework {framework}"
        );
    }

    let compliance_asset = output.join("VesperPlayerOptionalPlugins-FFmpeg-Compliance.zip");
    let first_compliance = fs::read(&compliance_asset).expect("read first compliance archive");
    let repeated = ios_stage_release_command(&root, &tools, &output)
        .env("VESPER_IOS_INCLUDE_OPTIONAL_PLUGINS", "yes")
        .env(
            "VESPER_IOS_OPTIONAL_PACKAGE_ARTIFACTS_DIR",
            root.join("unsafe ambient target"),
        )
        .env("CARGO_TARGET_DIR", root.join("stale custom cargo target"))
        .env("VESPER_BUILD_IOS_PLAYER_FFI_MODE", "platform")
        .env("VESPER_BUILD_APPLE_CATALYST", "1")
        .env("VESPER_IOS_SIMULATOR_ARCHS", "x86_64")
        .env("PLATFORM_NAME", "macosx")
        .arg("--package-artifacts-directory")
        .arg(&package_artifacts)
        .output()
        .expect("repeat complete iOS release fixture");
    assert_eq!(
        repeated.status.code(),
        Some(0),
        "repeat stderr: {}",
        String::from_utf8_lossy(&repeated.stderr)
    );
    assert_eq!(
        fs::read(&compliance_asset).expect("read repeated compliance archive"),
        first_compliance,
        "identical release inputs must produce a byte-identical compliance archive"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_stage_release_preserves_previous_outputs_when_build_or_optional_staging_fails() {
    for (failure_variable, failure_value) in [
        ("VESPER_IOS_RELEASE_BUILD_STATUS", "31"),
        ("VESPER_TEST_PLUGIN_CARGO_STATUS", "32"),
    ] {
        let (_directory, root, tools) = ios_stage_release_fixture();
        let output = root.join("release output");
        let package_artifacts = root.join("dist/external package output/Artifacts");
        fs::create_dir_all(&output).expect("create previous release output");
        fs::create_dir_all(&package_artifacts).expect("create previous package artifacts");
        let core = output.join("VesperPlayerKit-ios-arm64.framework.zip");
        let optional = output.join("VesperFFmpegAVCodec.xcframework.zip");
        let package_marker = package_artifacts
            .join("VesperFFmpegAVCodec.xcframework")
            .join("previous.txt");
        fs::write(&core, b"previous core\n").expect("write previous core");
        fs::write(&optional, b"previous optional\n").expect("write previous optional");
        fs::create_dir_all(
            package_marker
                .parent()
                .expect("previous package marker parent"),
        )
        .expect("create previous managed package artifact");
        fs::write(&package_marker, b"previous package\n").expect("write previous package marker");

        let result = ios_stage_release_command(&root, &tools, &output)
            .arg("--include-optional-plugins")
            .arg("--package-artifacts-directory")
            .arg(&package_artifacts)
            .env(failure_variable, failure_value)
            .output()
            .expect("run failing iOS release staging fixture");
        assert_eq!(
            result.status.code(),
            Some(5),
            "{failure_variable}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.stdout.is_empty(), "{failure_variable}");
        assert_eq!(
            fs::read(&core).expect("read previous core"),
            b"previous core\n"
        );
        assert_eq!(
            fs::read(&optional).expect("read previous optional"),
            b"previous optional\n"
        );
        assert_eq!(
            fs::read(&package_marker).expect("read previous package marker"),
            b"previous package\n"
        );
        assert_eq!(
            fs::read_dir(&output)
                .expect("scan release output")
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().starts_with(".vesper-"))
                .count(),
            0
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn ios_stage_release_preserves_previous_outputs_when_ffmpeg_source_drifts() {
    let (_directory, root, tools) = ios_stage_release_fixture();
    let source = root.join("third_party/_cache/ffmpeg-8.1.2.tar.xz");
    fs::OpenOptions::new()
        .append(true)
        .open(&source)
        .expect("open locked FFmpeg source archive")
        .write_all(b"tampered after metadata generation\n")
        .expect("tamper locked FFmpeg source archive");

    let output = root.join("release output");
    let package_artifacts = root.join("dist/external package output/Artifacts");
    fs::create_dir_all(&output).expect("create previous release output");
    fs::create_dir_all(&package_artifacts).expect("create previous package artifacts");
    let core = output.join("VesperPlayerKit-ios-arm64.framework.zip");
    let optional = output.join("VesperFFmpegAVCodec.xcframework.zip");
    let package_marker = package_artifacts
        .join("VesperFFmpegAVCodec.xcframework")
        .join("previous.txt");
    fs::write(&core, b"previous core\n").expect("write previous core");
    fs::write(&optional, b"previous optional\n").expect("write previous optional");
    fs::create_dir_all(
        package_marker
            .parent()
            .expect("previous package marker parent"),
    )
    .expect("create previous managed package artifact");
    fs::write(&package_marker, b"previous package\n").expect("write previous package marker");

    let result = ios_stage_release_command(&root, &tools, &output)
        .arg("--include-optional-plugins")
        .arg("--package-artifacts-directory")
        .arg(&package_artifacts)
        .output()
        .expect("run source-drift iOS release staging fixture");
    assert_eq!(
        result.status.code(),
        Some(5),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stdout.is_empty());
    let diagnostics = String::from_utf8_lossy(&result.stderr);
    assert!(diagnostics.contains("FFmpeg source archive"));
    assert!(diagnostics.contains("SHA-256"));
    assert_eq!(
        fs::read(&core).expect("read previous core"),
        b"previous core\n"
    );
    assert_eq!(
        fs::read(&optional).expect("read previous optional"),
        b"previous optional\n"
    );
    assert_eq!(
        fs::read(&package_marker).expect("read previous package marker"),
        b"previous package\n"
    );
    assert_eq!(
        fs::read_dir(&output)
            .expect("scan release output")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".vesper-"))
            .count(),
        0
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_optional_release_retains_a_recoverable_nested_owner_parent_after_failure() {
    let (_directory, root, tools) = ios_stage_release_fixture();
    let output = root.join("recoverable optional release output");
    let package_artifacts = root.join("dist/recoverable optional package/Artifacts");
    let aggregate_inputs =
        root.join("lib/ios/VesperPlayerKit/.build/ios-optional-release-inputs-v1");
    let prepared_journal = root
        .join("lib/ios/VesperPlayerKit/.build/vesper-cli-state/ios-plugin-release-prepared.json");

    let first = ios_optional_stage_release_command(&root, &tools, &output)
        .env(
            "VESPER_IOS_OPTIONAL_PACKAGE_ARTIFACTS_DIR",
            &package_artifacts,
        )
        .env("VESPER_TEST_PLUGIN_CARGO_STATUS", "71")
        .env("VESPER_TEST_PLUGIN_RECOVERY_CLEANUP_FAIL", "1")
        .output()
        .expect("fail optional iOS release with a retained nested journal");
    assert_eq!(
        first.status.code(),
        Some(5),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(prepared_journal.is_file());
    assert!(aggregate_inputs.is_dir());
    let canonical_aggregate_inputs = aggregate_inputs
        .canonicalize()
        .expect("resolve persistent aggregate inputs");
    assert!(
        !output.exists()
            || fs::read_dir(&output)
                .expect("scan failed output")
                .next()
                .is_none()
    );
    let journal: serde_json::Value = serde_json::from_slice(
        &fs::read(&prepared_journal).expect("read retained prepared-owner journal"),
    )
    .expect("parse retained prepared-owner journal");
    assert_eq!(
        journal
            .get("output_directory")
            .and_then(serde_json::Value::as_str),
        canonical_aggregate_inputs.to_str()
    );
    assert!(
        journal
            .get("owners")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|owners| owners.iter().any(|owner| {
                owner.get("role").and_then(serde_json::Value::as_str) == Some("assets")
                    && owner.get("parent").and_then(serde_json::Value::as_str)
                        == canonical_aggregate_inputs.to_str()
            }))
    );

    let second = ios_optional_stage_release_command(&root, &tools, &output)
        .env(
            "VESPER_IOS_OPTIONAL_PACKAGE_ARTIFACTS_DIR",
            &package_artifacts,
        )
        .env("VESPER_TEST_PLUGIN_CARGO_STATUS", "72")
        .output()
        .expect("recover the retained nested journal before the next expected failure");
    assert_eq!(
        second.status.code(),
        Some(5),
        "stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(!prepared_journal.exists());
    assert!(aggregate_inputs.is_dir());
    assert_eq!(
        fs::read_dir(&aggregate_inputs)
            .expect("scan recovered aggregate inputs")
            .count(),
        0
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_optional_release_holds_one_plugin_guard_through_final_asset_copy() {
    use std::io::Read;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let (_directory, root, tools) = ios_stage_release_fixture();
    let output = root.join("guarded optional release output");
    let package_artifacts = root.join("dist/guarded optional package/Artifacts");
    let ready = root.join("optional-aggregate-copy.ready");
    let release = root.join("optional-aggregate-copy.release");
    let mut aggregate = ios_optional_stage_release_command(&root, &tools, &output)
        .env(
            "VESPER_IOS_OPTIONAL_PACKAGE_ARTIFACTS_DIR",
            &package_artifacts,
        )
        .env("VESPER_TEST_IOS_OPTIONAL_AGGREGATE_COPY_READY", &ready)
        .env("VESPER_TEST_IOS_OPTIONAL_AGGREGATE_COPY_RELEASE", &release)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start guarded optional iOS release");
    let deadline = Instant::now() + Duration::from_secs(60);
    while !ready.is_file() && Instant::now() < deadline {
        if aggregate
            .try_wait()
            .expect("poll guarded optional iOS release")
            .is_some()
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !ready.is_file() {
        let _ = fs::write(&release, b"release\n");
        let result = aggregate
            .wait_with_output()
            .expect("wait after optional aggregate gate timeout");
        panic!(
            "optional iOS release did not reach its package-copy gate; stderr: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    let path = std::env::join_paths([
        tools.as_path(),
        std::path::Path::new("/usr/bin"),
        std::path::Path::new("/bin"),
    ])
    .expect("compose competing iOS plugin PATH");
    let run_competing =
        |command_name: &str, competing_output: &std::path::Path, arguments: &[&str]| {
            Command::new(env!("CARGO_BIN_EXE_vesper"))
                .env("PATH", &path)
                .env("VESPER_TEST_TOOL_LOG", root.join("stage-release.log"))
                .env(
                    "VESPER_TEST_RUST_TARGET_LIBDIR",
                    root.join("fake iOS Rust target libdir"),
                )
                .env(
                    "VESPER_TEST_PLUGIN_DYLIB",
                    "libvesper_frame_processor_diagnostic.dylib",
                )
                .args(["ios", "--root"])
                .arg(&root)
                .arg(command_name)
                .arg(competing_output)
                .args(arguments)
                .output()
                .unwrap_or_else(|error| panic!("run competing {command_name}: {error}"))
        };
    let competing_build_output = root.join("competing plugin build output");
    let competing_build = run_competing(
        "frame-processor-plugin",
        &competing_build_output,
        &["debug", "ios-arm64"],
    );
    let competing_release_output = root.join("competing plugin release output");
    let competing_release = run_competing(
        "stage-frame-processor-plugin-release",
        &competing_release_output,
        &["ios-arm64", "ios-simulator-arm64"],
    );

    fs::write(&release, b"release\n").expect("release optional aggregate copy gate");
    let aggregate = aggregate
        .wait_with_output()
        .expect("wait for guarded optional iOS release");

    for (label, competing, competing_output) in [
        ("build", competing_build, competing_build_output),
        ("release", competing_release, competing_release_output),
    ] {
        assert_eq!(competing.status.code(), Some(4), "{label}");
        assert!(competing.stdout.is_empty(), "{label}");
        assert!(
            String::from_utf8_lossy(&competing.stderr)
                .contains("another iOS plugin build is already active"),
            "{label}: {}",
            String::from_utf8_lossy(&competing.stderr)
        );
        assert!(!competing_output.exists(), "{label}");
    }
    assert_eq!(
        aggregate.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&aggregate.stderr)
    );

    let framework = "VesperPlayerFrameProcessorDiagnosticPlugin";
    let relative_binary =
        format!("{framework}.xcframework/ios-arm64/{framework}.framework/{framework}");
    let package_binary = package_artifacts.join(&relative_binary);
    let package_bytes = fs::read(&package_binary).expect("read packaged plugin binary");
    let archive_path = output.join(format!("{framework}.xcframework.zip"));
    let archive_file = fs::File::open(&archive_path).expect("open staged plugin archive");
    let mut archive = zip::ZipArchive::new(archive_file).expect("read staged plugin archive");
    let mut archived_bytes = Vec::new();
    archive
        .by_name(&relative_binary)
        .expect("find staged plugin binary")
        .read_to_end(&mut archived_bytes)
        .expect("read staged plugin binary");
    assert_eq!(package_bytes, archived_bytes);
    assert!(String::from_utf8_lossy(&package_bytes).contains("player-frame-processor-diagnostic"));
}

#[cfg(target_os = "macos")]
#[test]
fn ios_ffmpeg_runtime_release_holds_plugin_guard_through_release_exit() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let fixture = ios_plugin_fixture();
    configure_ios_plugin_ffmpeg_fixture(&fixture);
    let ready = fixture.root.join("standalone-runtime.ready");
    let release = fixture.root.join("standalone-runtime.release");
    let output = fixture.root.join("runtime release output");
    let mut command = ios_ffmpeg_runtime_fixture_command(&fixture, &output);
    apply_ios_ffmpeg_runtime_reuse_environment(&mut command, &fixture.root);
    let mut runtime = command
        .env("VESPER_TEST_IOS_FFMPEG_RUNTIME_READY", &ready)
        .env("VESPER_TEST_IOS_FFMPEG_RUNTIME_RELEASE", &release)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start standalone FFmpeg runtime release");

    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready.is_file() && Instant::now() < deadline {
        if runtime
            .try_wait()
            .expect("poll standalone FFmpeg runtime release")
            .is_some()
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !ready.is_file() {
        let _ = fs::write(&release, b"release\n");
        let result = runtime
            .wait_with_output()
            .expect("wait after standalone runtime gate timeout");
        panic!(
            "standalone FFmpeg runtime release did not reach its guard gate; stderr: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    let competing_output = fixture.root.join("competing plugin output");
    let competing = ios_plugin_fixture_command(
        &fixture,
        "frame-processor-plugin",
        "libvesper_frame_processor_diagnostic.dylib",
        &[
            competing_output
                .to_str()
                .expect("UTF-8 competing plugin output"),
            "debug",
            "ios-arm64",
        ],
        &[],
    )
    .output()
    .expect("run plugin build competing with standalone runtime release");

    fs::write(&release, b"release\n").expect("release standalone runtime guard gate");
    let runtime = runtime
        .wait_with_output()
        .expect("wait for standalone FFmpeg runtime release");

    assert_eq!(
        competing.status.code(),
        Some(4),
        "stderr: {}",
        String::from_utf8_lossy(&competing.stderr)
    );
    assert!(competing.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&competing.stderr)
            .contains("another iOS plugin build is already active")
    );
    assert!(!competing_output.exists());
    assert_eq!(
        runtime.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&runtime.stderr)
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_stage_release_rolls_back_release_files_when_package_promotion_fails() {
    let (_directory, root, tools) = ios_stage_release_fixture();
    let output = root.join("rollback release output");
    let package_artifacts = root.join("dist/rollback package output/Artifacts");
    fs::create_dir_all(&output).expect("create rollback release output");
    fs::create_dir_all(package_artifacts.parent().expect("package parent"))
        .expect("create rollback package parent");
    fs::write(&package_artifacts, b"not a directory\n")
        .expect("write invalid package artifacts target");
    let previous_core = output.join("VesperPlayerKit-ios-arm64.framework.zip");
    let stale_optional = output.join("VesperPlayerOptionalPlugins-FFmpeg-old-source.tar.xz");
    fs::write(&previous_core, b"previous core\n").expect("write previous core");
    fs::write(&stale_optional, b"previous optional\n").expect("write previous optional");

    let result = ios_stage_release_command(&root, &tools, &output)
        .arg("--include-optional-plugins")
        .arg("--package-artifacts-directory")
        .arg(&package_artifacts)
        .output()
        .expect("run package-promotion rollback fixture");
    assert_eq!(
        result.status.code(),
        Some(3),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stdout.is_empty());
    assert_eq!(
        fs::read(&previous_core).expect("read restored core"),
        b"previous core\n"
    );
    assert_eq!(
        fs::read(&stale_optional).expect("read restored optional"),
        b"previous optional\n"
    );
    assert_eq!(
        fs::read(&package_artifacts).expect("read invalid package target"),
        b"not a directory\n"
    );
    for asset in CORE_RELEASE_ASSETS_FOR_TEST
        .into_iter()
        .filter(|name| *name != "VesperPlayerKit-ios-arm64.framework.zip")
    {
        assert!(!output.join(asset).exists(), "{asset}");
    }
    for framework in IOS_RELEASE_OPTIONAL_FRAMEWORKS {
        assert!(!output.join(format!("{framework}.xcframework.zip")).exists());
    }
}

#[cfg(target_os = "macos")]
#[test]
fn ios_stage_release_ctrl_c_reaps_the_build_tree_and_preserves_previous_outputs() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let (_directory, root, tools) = ios_stage_release_fixture();
    let output = root.join("cancelled release output");
    fs::create_dir_all(&output).expect("create cancelled release output");
    let previous = output.join("VesperPlayerKit-ios-arm64.framework.zip");
    fs::write(&previous, b"previous release\n").expect("write previous release output");
    let build_pid_path = root.join("cancelled-build.pid");
    let descendant_pid_path = root.join("cancelled-build-descendant.pid");
    let mut process = ios_stage_release_command(&root, &tools, &output)
        .env("VESPER_IOS_RELEASE_BUILD_PID_FILE", &build_pid_path)
        .env(
            "VESPER_IOS_RELEASE_DESCENDANT_PID_FILE",
            &descendant_pid_path,
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start cancellable iOS release staging");
    let parent_pid = i32::try_from(process.id()).expect("iOS release parent pid");
    let pid_file_ready = |path: &std::path::Path| {
        fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
    };
    let ready_deadline = Instant::now() + Duration::from_secs(10);
    while (!pid_file_ready(&build_pid_path) || !pid_file_ready(&descendant_pid_path))
        && Instant::now() < ready_deadline
    {
        if process
            .try_wait()
            .expect("poll cancellable iOS release staging")
            .is_some()
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !pid_file_ready(&build_pid_path) || !pid_file_ready(&descendant_pid_path) {
        let _ = kill(Pid::from_raw(parent_pid), Signal::SIGINT);
        let output = process
            .wait_with_output()
            .expect("wait after iOS release cancellation readiness failure");
        panic!(
            "iOS release build tree did not start; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let build_pid: i32 = fs::read_to_string(&build_pid_path)
        .expect("read iOS release build pid")
        .parse()
        .expect("parse iOS release build pid");
    let descendant_pid: i32 = fs::read_to_string(&descendant_pid_path)
        .expect("read iOS release descendant pid")
        .parse()
        .expect("parse iOS release descendant pid");

    kill(Pid::from_raw(parent_pid), Signal::SIGINT).expect("cancel iOS release staging");
    let result = process
        .wait_with_output()
        .expect("wait for cancelled iOS release staging");
    assert_eq!(result.status.code(), Some(6));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("was cancelled"));
    assert_eq!(
        fs::read(&previous).expect("read preserved release output"),
        b"previous release\n"
    );
    assert_eq!(
        fs::read_dir(&output)
            .expect("scan cancelled release output")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".vesper-"))
            .count(),
        0
    );

    for pid in [build_pid, descendant_pid] {
        let reap_deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match kill(Pid::from_raw(pid), None) {
                Err(Errno::ESRCH) => break,
                _ if Instant::now() < reap_deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                outcome => panic!("iOS release process {pid} is still alive: {outcome:?}"),
            }
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn ios_stage_release_ctrl_c_allows_nested_ffi_cleanup_before_reaping() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let (_directory, root, tools) = ios_stage_release_fixture();
    let output = root.join("nested FFI cancellation release output");
    fs::create_dir_all(&output).expect("create nested FFI release output");
    let previous_release = output.join("VesperPlayerKit-ios-arm64.framework.zip");
    fs::write(&previous_release, b"previous release\n")
        .expect("write previous nested FFI release output");

    let artifacts = root.join("lib/ios/VesperPlayerKit/Artifacts");
    let ffi_output = artifacts.join("rust-player-ffi");
    let headers = root.join("lib/ios/VesperPlayerKit/Sources/VesperPlayerFFIResolver/include");
    let target_libdir = root.join("fake iOS Rust target libdir");
    let artifact_root = root.join("fake iOS Cargo artifacts");
    fs::create_dir_all(&ffi_output).expect("create previous nested iOS FFI output");
    fs::create_dir_all(&headers).expect("create nested iOS FFI headers");
    fs::create_dir_all(&target_libdir).expect("create nested fake Rust target libdir");
    fs::write(ffi_output.join("previous.txt"), b"previous FFI\n")
        .expect("write previous nested iOS FFI output");

    let cargo_pid_path = root.join("nested-ios-ffi-cargo.pid");
    let descendant_pid_path = root.join("nested-ios-ffi-descendant.pid");
    write_executable_script(
        &tools.join("rustc"),
        "#!/bin/sh\nprintf '%s\\n' \"$VESPER_TEST_RUST_TARGET_LIBDIR\"\n",
    );
    write_executable_script(
        &tools.join("cargo"),
        r#"#!/bin/sh
printf '%s' "$$" > "$VESPER_TEST_CARGO_PID_FILE"
/bin/sh -c 'trap "" INT TERM; printf "%s" "$$" > "$1"; while :; do /bin/sleep 1; done' sh "$VESPER_TEST_DESCENDANT_PID_FILE" &
trap '' INT TERM
while :; do /bin/sleep 1; done
"#,
    );
    write_executable_script(&tools.join("xcrun"), "#!/bin/sh\nexit 0\n");
    let path = std::env::join_paths([
        tools.as_path(),
        std::path::Path::new("/usr/bin"),
        std::path::Path::new("/bin"),
    ])
    .expect("compose nested iOS FFI PATH");

    let mut process = ios_stage_release_command(&root, &tools, &output)
        .env("PATH", path)
        .env("VESPER_TEST_RUST_TARGET_LIBDIR", &target_libdir)
        .env("VESPER_TEST_ARTIFACT_ROOT", &artifact_root)
        .env("VESPER_TEST_CARGO_PID_FILE", &cargo_pid_path)
        .env("VESPER_TEST_DESCENDANT_PID_FILE", &descendant_pid_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start nested iOS FFI release staging");
    let parent_pid = i32::try_from(process.id()).expect("nested iOS release parent pid");
    let ready_deadline = Instant::now() + Duration::from_secs(15);
    while (!cargo_pid_path.is_file() || !descendant_pid_path.is_file())
        && Instant::now() < ready_deadline
    {
        if process
            .try_wait()
            .expect("poll nested iOS FFI release staging")
            .is_some()
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !cargo_pid_path.is_file() || !descendant_pid_path.is_file() {
        let _ = kill(Pid::from_raw(parent_pid), Signal::SIGINT);
        let result = process
            .wait_with_output()
            .expect("wait after nested iOS FFI readiness failure");
        panic!(
            "nested iOS FFI build did not start; stderr: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let cargo_pid: i32 = fs::read_to_string(&cargo_pid_path)
        .expect("read nested iOS FFI Cargo pid")
        .parse()
        .expect("parse nested iOS FFI Cargo pid");
    let descendant_pid: i32 = fs::read_to_string(&descendant_pid_path)
        .expect("read nested iOS FFI descendant pid")
        .parse()
        .expect("parse nested iOS FFI descendant pid");

    kill(Pid::from_raw(parent_pid), Signal::SIGINT).expect("cancel nested iOS FFI release staging");
    let result = process
        .wait_with_output()
        .expect("wait for cancelled nested iOS FFI release staging");

    assert_eq!(result.status.code(), Some(6));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("was cancelled"));
    assert_eq!(
        fs::read(&previous_release).expect("read preserved nested FFI release output"),
        b"previous release\n"
    );
    assert_eq!(
        fs::read(ffi_output.join("previous.txt")).expect("read preserved nested iOS FFI output"),
        b"previous FFI\n"
    );
    assert_eq!(
        fs::read_dir(&artifacts)
            .expect("inspect nested iOS FFI artifacts after cancellation")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".vesper-ios-ffi-stage-")
            })
            .count(),
        0
    );

    for pid in [cargo_pid, descendant_pid] {
        let reap_deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match kill(Pid::from_raw(pid), None) {
                Err(Errno::ESRCH) => break,
                _ if Instant::now() < reap_deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                outcome => panic!("nested iOS FFI process {pid} is still alive: {outcome:?}"),
            }
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn ios_stage_release_rejects_a_concurrent_build_for_the_same_repository() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let (_directory, root, tools) = ios_stage_release_fixture();
    let first_output = root.join("first release output");
    let second_output = root.join("second release output");
    let ready = root.join("release-lock-ready");
    let gate = root.join("release-lock-gate");
    let mut first = ios_stage_release_command(&root, &tools, &first_output)
        .env("VESPER_IOS_RELEASE_LOCK_READY", &ready)
        .env("VESPER_IOS_RELEASE_LOCK_GATE", &gate)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start first iOS release staging command");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready.is_file() && Instant::now() < deadline {
        if first
            .try_wait()
            .expect("poll first iOS release staging command")
            .is_some()
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        ready.is_file(),
        "first iOS release staging command did not acquire the lock"
    );

    let second = ios_stage_release_command(&root, &tools, &second_output)
        .output()
        .expect("run concurrent iOS release staging command");
    assert_eq!(second.status.code(), Some(4));
    assert!(second.stdout.is_empty());
    assert!(String::from_utf8_lossy(&second.stderr).contains("already active"));
    assert!(!second_output.exists());

    fs::write(&gate, b"continue\n").expect("release first iOS staging command");
    let first = first
        .wait_with_output()
        .expect("wait for first iOS release staging command");
    assert_eq!(
        first.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_stage_release_rejects_output_identity_replacement_during_build() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let (_directory, root, tools) = ios_stage_release_fixture();
    let output = root.join("identity-bound release output");
    let displaced = root.join("identity-bound release output.old");
    let ready = root.join("identity-build-ready");
    let gate = root.join("identity-build-gate");
    fs::create_dir_all(&output).expect("create identity-bound release output");
    fs::write(output.join("previous.txt"), b"previous\n")
        .expect("write previous identity-bound output");
    let mut process = ios_stage_release_command(&root, &tools, &output)
        .env("VESPER_IOS_IDENTITY_READY", &ready)
        .env("VESPER_IOS_IDENTITY_GATE", &gate)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start identity-bound iOS release staging");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready.is_file() && Instant::now() < deadline {
        if process
            .try_wait()
            .expect("poll identity-bound iOS release staging")
            .is_some()
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(ready.is_file(), "identity-bound build did not start");

    fs::rename(&output, &displaced).expect("displace validated release output");
    fs::create_dir(&output).expect("create replacement release output");
    fs::write(output.join("replacement.txt"), b"replacement\n")
        .expect("write replacement release output");
    fs::write(&gate, b"continue\n").expect("release identity-bound build");

    let result = process
        .wait_with_output()
        .expect("wait for identity-bound iOS release staging");
    assert_eq!(result.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&result.stderr).contains("changed after validation"));
    assert_eq!(
        fs::read(output.join("replacement.txt")).expect("read replacement release output"),
        b"replacement\n"
    );
    assert_eq!(
        fs::read(displaced.join("previous.txt")).expect("read displaced release output"),
        b"previous\n"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_stage_release_rejects_package_parent_replacement_during_build() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let (_directory, root, tools) = ios_stage_release_fixture();
    let output = root.join("package identity release output");
    let package_parent = root.join("dist/package identity");
    let displaced_parent = root.join("dist/package identity.old");
    let package_target = package_parent.join("Artifacts");
    let ready = root.join("package-identity-build-ready");
    let gate = root.join("package-identity-build-gate");
    fs::create_dir_all(&output).expect("create package identity release output");
    fs::create_dir_all(&package_target).expect("create package identity target");
    let mut process = ios_stage_release_command(&root, &tools, &output)
        .args([
            "--include-optional-plugins",
            "--package-artifacts-directory",
        ])
        .arg(&package_target)
        .env("VESPER_IOS_IDENTITY_READY", &ready)
        .env("VESPER_IOS_IDENTITY_GATE", &gate)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start package identity iOS release staging");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready.is_file() && Instant::now() < deadline {
        if process
            .try_wait()
            .expect("poll package identity iOS release staging")
            .is_some()
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(ready.is_file(), "package identity build did not start");

    fs::rename(&package_parent, &displaced_parent).expect("displace validated package parent");
    let replacement = package_target.join("VesperFFmpegAVCodec.xcframework");
    fs::create_dir_all(&replacement).expect("create replacement package target");
    fs::write(replacement.join("replacement.txt"), b"replacement\n")
        .expect("write replacement package marker");
    fs::write(&gate, b"continue\n").expect("release package identity build");

    let result = process
        .wait_with_output()
        .expect("wait for package identity iOS release staging");
    assert_eq!(result.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&result.stderr).contains("changed after validation"));
    assert_eq!(
        fs::read(replacement.join("replacement.txt")).expect("read replacement package marker"),
        b"replacement\n"
    );
    assert!(displaced_parent.join("Artifacts").is_dir());
}

#[cfg(target_os = "macos")]
#[test]
fn ios_stage_release_rejects_state_directory_replacement_during_build() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let (_directory, root, tools) = ios_stage_release_fixture();
    let output = root.join("state identity release output");
    let state = root.join("lib/ios/VesperPlayerKit/.build/vesper-cli-state");
    let displaced_state = root.join("lib/ios/VesperPlayerKit/.build/vesper-cli-state.old");
    let ready = root.join("state-identity-build-ready");
    let gate = root.join("state-identity-build-gate");
    fs::create_dir_all(&output).expect("create state identity release output");
    fs::write(output.join("previous.txt"), b"previous\n")
        .expect("write previous state identity output");
    let mut process = ios_stage_release_command(&root, &tools, &output)
        .env("VESPER_IOS_IDENTITY_READY", &ready)
        .env("VESPER_IOS_IDENTITY_GATE", &gate)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start state-bound iOS release staging");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready.is_file() && Instant::now() < deadline {
        if process
            .try_wait()
            .expect("poll state-bound iOS release staging")
            .is_some()
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(ready.is_file(), "state-bound build did not start");
    assert!(
        state.is_dir(),
        "transaction state directory was not prepared"
    );

    fs::rename(&state, &displaced_state).expect("displace transaction state directory");
    fs::create_dir(&state).expect("create replacement transaction state directory");
    fs::write(&gate, b"continue\n").expect("release state-bound build");

    let result = process
        .wait_with_output()
        .expect("wait for state-bound iOS release staging");
    assert_eq!(result.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&result.stderr).contains("state directory"));
    assert_eq!(
        fs::read(output.join("previous.txt")).expect("read preserved release output"),
        b"previous\n"
    );
    assert!(!state.join("ios-stage-release-transaction.json").exists());
    assert!(
        !displaced_state
            .join("ios-stage-release-transaction.json")
            .exists()
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ios_stage_release_classifies_missing_crashed_and_unbounded_tools() {
    let (_directory, root, tools) = ios_stage_release_fixture();
    let output = root.join("missing tool release output");
    fs::create_dir_all(&output).expect("create missing-tool release output");
    let previous = output.join("VesperPlayerKit-ios-arm64.framework.zip");
    fs::write(&previous, b"previous\n").expect("write missing-tool previous output");
    let missing = ios_stage_release_command(&root, &tools, &output)
        .env("DITTO", tools.join("missing-ditto"))
        .output()
        .expect("run iOS release staging without ditto");
    assert_eq!(missing.status.code(), Some(4));
    assert!(missing.stdout.is_empty());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("failed to spawn"));
    assert_eq!(
        fs::read(&previous).expect("read preserved output"),
        b"previous\n"
    );

    let (_directory, root, tools) = ios_stage_release_fixture();
    let output = root.join("crashed tool release output");
    fs::create_dir_all(&output).expect("create crashed-tool release output");
    let previous = output.join("VesperPlayerKit-ios-arm64.framework.zip");
    fs::write(&previous, b"previous\n").expect("write crashed-tool previous output");
    write_executable_script(&tools.join("lipo"), "#!/bin/sh\nkill -SEGV $$\n");
    let crashed = ios_stage_release_command(&root, &tools, &output)
        .output()
        .expect("run iOS release staging with crashed lipo");
    assert_eq!(crashed.status.code(), Some(6));
    assert!(crashed.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&crashed.stderr).contains("terminated without an exit code"),
        "{}",
        String::from_utf8_lossy(&crashed.stderr)
    );
    assert_eq!(
        fs::read(&previous).expect("read preserved output"),
        b"previous\n"
    );

    let (_directory, root, tools) = ios_stage_release_fixture();
    let output = root.join("unbounded tool release output");
    fs::create_dir_all(&output).expect("create unbounded-tool release output");
    let previous = output.join("VesperPlayerKit-ios-arm64.framework.zip");
    fs::write(&previous, b"previous\n").expect("write unbounded-tool previous output");
    write_executable_script(
        &tools.join("ditto"),
        "#!/bin/sh\nwhile :; do printf 0123456789abcdef0123456789abcdef; done\n",
    );
    let unbounded = ios_stage_release_command(&root, &tools, &output)
        .output()
        .expect("run iOS release staging with unbounded ditto output");
    assert_eq!(unbounded.status.code(), Some(6));
    assert!(unbounded.stdout.is_empty());
    assert!(String::from_utf8_lossy(&unbounded.stderr).contains("exceeds"));
    assert_eq!(
        fs::read(&previous).expect("read preserved output"),
        b"previous\n"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn vesper_wrapper_routes_ios_stage_release_to_the_rust_cli() {
    use std::os::unix::fs::PermissionsExt;

    let (_directory, root, tools) = ios_stage_release_fixture();
    let wrapper = root.join("scripts/vesper");
    fs::copy(workspace_root().join("scripts/vesper"), &wrapper)
        .expect("copy Vesper wrapper into iOS release fixture");
    let mut permissions = fs::metadata(&wrapper)
        .expect("inspect fixture Vesper wrapper")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&wrapper, permissions).expect("make fixture Vesper wrapper executable");
    let output_directory = root.join("wrapper release output");
    let path = std::env::join_paths([
        tools.as_path(),
        std::path::Path::new("/usr/bin"),
        std::path::Path::new("/bin"),
        std::path::Path::new("/usr/sbin"),
        std::path::Path::new("/sbin"),
    ])
    .expect("compose wrapper iOS release fixture PATH");

    let result = Command::new(&wrapper)
        .env("PATH", path)
        .env("VESPER_CLI", env!("CARGO_BIN_EXE_vesper"))
        .env("DITTO", tools.join("ditto"))
        .env("LIPO", tools.join("lipo"))
        .env(
            "VESPER_IOS_RELEASE_TEST_LOG",
            root.join("stage-release.log"),
        )
        .env("VESPER_TEST_TOOL_LOG", root.join("stage-release.log"))
        .env(
            "VESPER_TEST_RUST_TARGET_LIBDIR",
            root.join("fake iOS Rust target libdir"),
        )
        .env(
            "VESPER_TEST_ARTIFACT_ROOT",
            root.join("fake iOS Cargo artifacts"),
        )
        .args(["ios", "stage-release"])
        .arg(&output_directory)
        .output()
        .expect("route iOS release staging through public wrapper");
    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        output_directory
            .join("VesperPlayerKit.xcframework.zip")
            .is_file()
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("Staged VesperPlayerKit"));
}

#[cfg(target_os = "macos")]
#[test]
fn ios_stage_release_cli_false_overrides_environment_and_removes_stale_optional_assets() {
    let (_directory, root, tools) = ios_stage_release_fixture();
    let output = root.join("core release output");
    fs::create_dir_all(&output).expect("create core release output");
    fs::write(
        output.join("VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip"),
        b"stale optional\n",
    )
    .expect("write stale optional archive");

    let result = ios_stage_release_command(&root, &tools, &output)
        .env("VESPER_IOS_INCLUDE_OPTIONAL_PLUGINS", "true")
        .args(["--include-optional-plugins=false"])
        .output()
        .expect("stage core-only iOS release fixture");
    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("Skipped optional"));
    assert!(
        !output
            .join("VesperPlayerFrameProcessorDiagnosticPlugin.xcframework.zip")
            .exists()
    );
    for asset in CORE_RELEASE_ASSETS_FOR_TEST {
        assert!(output.join(asset).is_file(), "{asset}");
    }
    let log =
        fs::read_to_string(root.join("stage-release.log")).expect("read core release staging log");
    assert!(!log.contains("optional:"));
}

#[cfg(target_os = "macos")]
const CORE_RELEASE_ASSETS_FOR_TEST: [&str; 3] = [
    "VesperPlayerKit-ios-arm64.framework.zip",
    "VesperPlayerKit-ios-simulator-arm64.framework.zip",
    "VesperPlayerKit.xcframework.zip",
];

#[cfg(target_os = "macos")]
#[test]
fn ios_stage_release_rejects_symlinked_output_and_unsafe_ambient_package_target() {
    use std::os::unix::fs::symlink;

    let (directory, root, tools) = ios_stage_release_fixture();
    let outside = directory.path().join("outside release");
    fs::create_dir_all(&outside).expect("create outside release directory");
    fs::write(outside.join("marker.txt"), b"outside\n").expect("write outside marker");
    let output_link = root.join("release symlink");
    symlink(&outside, &output_link).expect("create release output symlink");

    let symlinked = ios_stage_release_command(&root, &tools, &output_link)
        .output()
        .expect("reject symlinked iOS release output");
    assert_eq!(symlinked.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&symlinked.stderr).contains("must not contain symlinks"));
    assert_eq!(
        fs::read(outside.join("marker.txt")).expect("read outside marker"),
        b"outside\n"
    );

    let output = root.join("release output");
    let ambient = root.join("devnotes/Artifacts");
    fs::create_dir_all(&ambient).expect("create unsafe ambient package target");
    fs::write(ambient.join("marker.txt"), b"preserve\n").expect("write ambient marker");
    let rejected = ios_stage_release_command(&root, &tools, &output)
        .env("VESPER_IOS_INCLUDE_OPTIONAL_PLUGINS", "1")
        .env("VESPER_IOS_OPTIONAL_PACKAGE_ARTIFACTS_DIR", &ambient)
        .output()
        .expect("reject unsafe ambient package target");
    assert_eq!(rejected.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("ambient repository-local"));
    assert_eq!(
        fs::read(ambient.join("marker.txt")).expect("read ambient marker"),
        b"preserve\n"
    );
    assert!(!root.join("stage-release.log").exists());

    let wide_target = root.join("dist");
    fs::create_dir_all(wide_target.join("android")).expect("create wide package target");
    fs::write(wide_target.join("android/sentinel.txt"), b"preserve\n")
        .expect("write wide package sentinel");
    let wide = ios_stage_release_command(&root, &tools, &output)
        .arg("--include-optional-plugins")
        .arg("--package-artifacts-directory")
        .arg(&wide_target)
        .output()
        .expect("reject wide package artifacts target");
    assert_eq!(wide.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&wide.stderr).contains("dedicated 'Artifacts'"));
    assert_eq!(
        fs::read(wide_target.join("android/sentinel.txt")).expect("read wide package sentinel"),
        b"preserve\n"
    );

    let unmanaged = root.join("dist/unmanaged/Artifacts");
    fs::create_dir_all(&unmanaged).expect("create unmanaged package target");
    fs::write(unmanaged.join("unmanaged.txt"), b"preserve\n")
        .expect("write unmanaged package marker");
    let unmanaged_result = ios_stage_release_command(&root, &tools, &output)
        .arg("--include-optional-plugins")
        .arg("--package-artifacts-directory")
        .arg(&unmanaged)
        .output()
        .expect("reject unmanaged package artifacts target");
    assert_eq!(unmanaged_result.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&unmanaged_result.stderr).contains("unmanaged entry"));
    assert_eq!(
        fs::read(unmanaged.join("unmanaged.txt")).expect("read unmanaged package marker"),
        b"preserve\n"
    );
}

#[cfg(unix)]
#[test]
fn ios_app_store_layout_accepts_the_complete_flat_arm64_bundle() {
    let (_directory, root, app, tools) = ios_app_store_cli_fixture();
    let signature_log = root.join("codesign.log");
    let module_cache = root.join("Swift module cache - 缓存");

    let output = ios_app_store_command(&root, &app, &tools)
        .env("VESPER_FAKE_CODESIGN_LOG", &signature_log)
        .env("VESPER_SWIFT_MODULE_CACHE_DIR", &module_cache)
        .output()
        .expect("verify valid iOS App Store fixture");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let expected_stdout = format!(
        "Verified App Store-compatible optional iOS framework layout:\n  {}\n  FFmpeg profile hash: fixture-profile-hash\n{}",
        app.display(),
        IOS_APP_STORE_FRAMEWORKS
            .iter()
            .map(|framework_name| format!("  {framework_name}.framework\n"))
            .collect::<String>()
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 App Store verification output"),
        expected_stdout
    );
    assert!(!signature_log.exists());
    assert!(module_cache.is_dir());
}

#[cfg(unix)]
#[test]
fn ios_app_store_layout_rejects_dependency_and_profile_drift() {
    let (_directory, root, app, tools) = ios_app_store_cli_fixture();
    let frameworks = app.join("Frameworks");
    let decoder = frameworks
        .join("VesperPlayerDecoderVideoToolboxPlugin.framework")
        .join("VesperPlayerDecoderVideoToolboxPlugin");
    fs::write(
        format!("{}.dependencies", decoder.display()),
        "@rpath/Unexpected Name.framework/Unexpected Name\n",
    )
    .expect("write unexpected optional framework dependency");

    let dependency_drift = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify dependency drift");
    assert_eq!(dependency_drift.status.code(), Some(5));
    assert!(dependency_drift.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&dependency_drift.stderr)
            .contains("unexpected non-system dynamic dependency")
    );
    assert!(
        String::from_utf8_lossy(&dependency_drift.stderr)
            .contains("@rpath/Unexpected Name.framework/Unexpected Name")
    );

    fs::write(format!("{}.dependencies", decoder.display()), "")
        .expect("restore optional framework dependencies");
    let codec = frameworks
        .join("VesperFFmpegAVCodec.framework")
        .join("VesperFFmpegAVCodec");
    fs::write(format!("{}.dependencies", codec.display()), "")
        .expect("remove required FFmpeg dependency");
    let missing_dependency = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify missing FFmpeg dependency");
    assert_eq!(missing_dependency.status.code(), Some(5));
    assert!(
        String::from_utf8_lossy(&missing_dependency.stderr).contains(
            "missing dynamic dependency @rpath/VesperFFmpegAVUtil.framework/VesperFFmpegAVUtil"
        )
    );
    fs::write(
        format!("{}.dependencies", codec.display()),
        "@rpath/VesperFFmpegAVUtil.framework/VesperFFmpegAVUtil\n",
    )
    .expect("restore required FFmpeg dependency");

    fs::write(
        format!("{}.dependencies", decoder.display()),
        "@rpath/VesperFFmpegGhost.framework/VesperFFmpegGhost\n",
    )
    .expect("write missing sibling dependency");
    let missing_sibling = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify missing sibling framework");
    assert_eq!(missing_sibling.status.code(), Some(5));
    assert!(
        String::from_utf8_lossy(&missing_sibling.stderr)
            .contains("Missing sibling FFmpeg framework")
    );
    fs::write(format!("{}.dependencies", decoder.display()), "")
        .expect("restore decoder dependencies");

    fs::write(
        frameworks
            .join("VesperFFmpegAVUtil.framework")
            .join("profile-hash.txt"),
        "different-profile\n",
    )
    .expect("write mismatched FFmpeg profile hash");
    let profile_drift = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify profile hash drift");
    assert_eq!(profile_drift.status.code(), Some(5));
    assert!(profile_drift.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&profile_drift.stderr).contains("FFmpeg profile hash mismatch")
    );
}

#[cfg(unix)]
#[test]
fn ios_app_store_layout_verifies_all_signatures_only_when_requested() {
    let (_directory, root, app, tools) = ios_app_store_cli_fixture();
    let signature_log = root.join("codesign.log");
    let output = ios_app_store_command(&root, &app, &tools)
        .env("VESPER_FAKE_CODESIGN_LOG", &signature_log)
        .arg("--verify-signatures")
        .output()
        .expect("verify signed iOS App Store fixture");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let invocations = fs::read_to_string(signature_log).expect("read codesign invocation log");
    let mut framework_names = IOS_APP_STORE_FRAMEWORKS;
    framework_names.sort_unstable();
    let mut expected_invocations = framework_names
        .iter()
        .map(|framework_name| {
            format!(
                "--verify --strict {}/Frameworks/{framework_name}.framework\n",
                app.display()
            )
        })
        .collect::<String>();
    expected_invocations.push_str(&format!("--verify --strict --deep {}\n", app.display()));
    assert_eq!(invocations, expected_invocations);

    let before_path_log = root.join("before-path-codesign.log");
    let before_path = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .current_dir(root.parent().expect("iOS fixture parent"))
        .env("PLUTIL", tools.join("plutil"))
        .env("SWIFT", tools.join("swift"))
        .env("OTOOL", tools.join("otool"))
        .env("XCRUN", tools.join("xcrun"))
        .env("LIPO", tools.join("lipo"))
        .env("CODESIGN", tools.join("codesign"))
        .env("VESPER_FAKE_CODESIGN_LOG", &before_path_log)
        .args(["ios", "verify-app-store-layout", "--verify-signatures"])
        .arg(&app)
        .output()
        .expect("verify signature option before app path");
    assert_eq!(
        before_path.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&before_path.stderr)
    );

    let failed = ios_app_store_command(&root, &app, &tools)
        .env("VESPER_FAKE_CODESIGN_LOG", root.join("failed-codesign.log"))
        .env("VESPER_FAKE_CODESIGN_STATUS", "1")
        .arg("--verify-signatures")
        .output()
        .expect("verify rejected iOS signatures");
    assert_eq!(failed.status.code(), Some(5));
    assert!(
        String::from_utf8_lossy(&failed.stderr)
            .contains("signature verification exited unsuccessfully")
    );

    let missing = ios_app_store_command(&root, &app, &tools)
        .env("CODESIGN", tools.join("missing-codesign"))
        .arg("--verify-signatures")
        .output()
        .expect("verify iOS signatures without codesign");
    assert_eq!(missing.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("failed to spawn"));
}

#[test]
fn ios_app_store_layout_keeps_clap_usage_and_help_contracts() {
    let help = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "verify-app-store-layout", "--help"])
        .output()
        .expect("show iOS App Store layout help");
    assert_eq!(help.status.code(), Some(0));
    assert!(help.stderr.is_empty());
    assert!(String::from_utf8_lossy(&help.stdout).contains("<APP_PATH>"));

    for arguments in [
        vec!["ios", "verify-app-store-layout"],
        vec!["ios", "verify-app-store-layout", "--unknown"],
        vec![
            "ios",
            "verify-app-store-layout",
            "/missing.app",
            "--unknown",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
            .args(arguments)
            .output()
            .expect("run invalid iOS App Store layout command");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
    }
}

#[cfg(unix)]
#[test]
fn ios_app_store_layout_rejects_missing_inputs_and_symlinks() {
    use std::os::unix::fs::symlink;

    let (directory, root, app, tools) = ios_app_store_cli_fixture();
    let missing = ios_app_store_command(&root, &root.join("missing.app"), &tools)
        .output()
        .expect("verify missing iOS app bundle");
    assert_eq!(missing.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("App bundle is unavailable"));

    let non_directory = root.join("not-an-app.app");
    fs::write(&non_directory, "not a directory\n").expect("write non-directory app fixture");
    let invalid_type = ios_app_store_command(&root, &non_directory, &tools)
        .output()
        .expect("verify non-directory iOS app bundle");
    assert_eq!(invalid_type.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&invalid_type.stderr).contains("non-symlink directory"));

    let app_alias = directory.path().join("symlinked app.app");
    symlink(&app, &app_alias).expect("symlink iOS app bundle");
    let symlinked_app = ios_app_store_command(&root, &app_alias, &tools)
        .output()
        .expect("verify symlinked iOS app bundle");
    assert_eq!(symlinked_app.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&symlinked_app.stderr).contains("non-symlink directory"));

    let framework = app.join("Frameworks/VesperPlayerDecoderVideoToolboxPlugin.framework");
    let outside = directory.path().join("outside decoder framework");
    fs::rename(&framework, &outside).expect("move framework outside app bundle");
    symlink(&outside, &framework).expect("symlink framework into app bundle");
    let symlinked_framework = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify symlinked optional framework");
    assert_eq!(symlinked_framework.status.code(), Some(5));
    assert!(
        String::from_utf8_lossy(&symlinked_framework.stderr).contains("must not contain symlinks")
    );
}

#[cfg(unix)]
#[test]
fn ios_app_store_layout_rejects_fixture_paths_and_streamed_aot_markers() {
    let (_directory, root, app, tools) = ios_app_store_cli_fixture();
    let fixture = app.join("assets/subtitle_contract/fixture.vtt");
    fs::create_dir_all(fixture.parent().expect("fixture path parent"))
        .expect("create forbidden fixture directory");
    fs::write(&fixture, "fixture\n").expect("write forbidden fixture resource");
    let fixture_path = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify fixture resource path");
    assert_eq!(fixture_path.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&fixture_path.stderr).contains("test fixture resources"));
    fs::remove_dir_all(app.join("assets")).expect("remove forbidden fixture resource");

    let mut aot = vec![b'x'; (64 * 1024) - 8];
    aot.extend_from_slice(b"fixtures/media/hidden-fixture");
    fs::write(app.join("Frameworks/App.framework/App"), aot)
        .expect("write cross-chunk Flutter AOT marker");
    let binary_marker = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify Flutter AOT marker");
    assert_eq!(binary_marker.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&binary_marker.stderr).contains("test fixture markers"));
}

#[cfg(unix)]
#[test]
fn ios_app_store_layout_rejects_invalid_framework_topology() {
    let (_directory, root, app, tools) = ios_app_store_cli_fixture();
    let frameworks = app.join("Frameworks");

    let dylib = frameworks.join("libunexpected.dylib.1");
    fs::write(&dylib, "fixture\n").expect("write standalone dylib");
    let standalone = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify standalone dylib");
    assert_eq!(standalone.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&standalone.stderr).contains("standalone dylibs"));
    fs::remove_file(dylib).expect("remove standalone dylib");

    let nested = frameworks.join("App.framework/Frameworks/Nested.framework");
    fs::create_dir_all(&nested).expect("create nested framework");
    let nested_output = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify nested framework");
    assert_eq!(nested_output.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&nested_output.stderr).contains("nested framework"));
    fs::remove_dir_all(frameworks.join("App.framework/Frameworks"))
        .expect("remove nested framework");

    let legacy = frameworks.join("VesperPlayerFfmpegRuntime.framework");
    fs::create_dir_all(&legacy).expect("create legacy FFmpeg umbrella");
    let legacy_output = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify legacy FFmpeg umbrella");
    assert_eq!(legacy_output.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&legacy_output.stderr).contains("legacy umbrella"));
    fs::remove_dir_all(legacy).expect("remove legacy FFmpeg umbrella");

    let missing_framework = frameworks.join("VesperPlayerFrameProcessorDiagnosticPlugin.framework");
    fs::remove_dir_all(&missing_framework).expect("remove required optional framework");
    let missing_output = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify missing optional framework");
    assert_eq!(missing_output.status.code(), Some(5));
    assert!(
        String::from_utf8_lossy(&missing_output.stderr)
            .contains("Missing required top-level optional framework")
    );

    fs::remove_dir_all(&frameworks).expect("remove app Frameworks directory");
    let missing_frameworks = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify app without Frameworks directory");
    assert_eq!(missing_frameworks.status.code(), Some(5));
    assert!(
        String::from_utf8_lossy(&missing_frameworks.stderr)
            .contains("missing its Frameworks directory")
    );
}

#[cfg(unix)]
#[test]
fn ios_app_store_layout_rejects_metadata_install_platform_and_architecture_drift() {
    let (_directory, root, app, tools) = ios_app_store_cli_fixture();
    let framework_name = "VesperFFmpegAVCodec";
    let framework = app
        .join("Frameworks")
        .join(format!("{framework_name}.framework"));
    let binary = framework.join(framework_name);
    let info_path = framework.join("Info.plist");
    let original_info = fs::read(&info_path).expect("read original framework metadata");

    let mut info: serde_json::Value =
        serde_json::from_slice(&original_info).expect("parse fixture framework metadata");
    info["CFBundleExecutable"] = serde_json::Value::String("WrongExecutable".to_owned());
    fs::write(
        &info_path,
        serde_json::to_vec(&info).expect("serialize invalid framework metadata"),
    )
    .expect("write invalid framework metadata");
    let metadata = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify invalid framework metadata");
    assert_eq!(metadata.status.code(), Some(5));
    assert!(
        String::from_utf8_lossy(&metadata.stderr).contains("Invalid framework bundle metadata")
    );
    fs::write(&info_path, &original_info).expect("restore framework metadata");

    fs::create_dir_all(framework.join("Resources")).expect("create forbidden Resources directory");
    let resources = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify forbidden framework Resources directory");
    assert_eq!(resources.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&resources.stderr).contains("shallow frameworks"));
    fs::remove_dir_all(framework.join("Resources")).expect("remove forbidden Resources directory");

    let mut info: serde_json::Value =
        serde_json::from_slice(&original_info).expect("parse fixture framework metadata");
    info["DTPlatformName"] = serde_json::Value::String("iphonesimulator".to_owned());
    fs::write(
        &info_path,
        serde_json::to_vec(&info).expect("serialize invalid framework platform metadata"),
    )
    .expect("write invalid framework platform metadata");
    let bundle_platform = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify invalid framework bundle platform");
    assert_eq!(bundle_platform.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&bundle_platform.stderr).contains("bundle platform metadata"));
    fs::write(&info_path, &original_info).expect("restore framework metadata");

    fs::write(
        format!("{}.install-name", binary.display()),
        "@rpath/Wrong.framework/Wrong\n",
    )
    .expect("write invalid install name");
    let install_name = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify invalid install name");
    assert_eq!(install_name.status.code(), Some(5));
    assert!(
        String::from_utf8_lossy(&install_name.stderr).contains("Unexpected framework install name")
    );
    fs::remove_file(format!("{}.install-name", binary.display()))
        .expect("remove invalid install name");

    fs::write(format!("{}.platform", binary.display()), "IOSSIMULATOR\n")
        .expect("write invalid Mach-O platform");
    let platform = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify invalid Mach-O platform");
    assert_eq!(platform.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&platform.stderr).contains("Mach-O build platform"));
    fs::remove_file(format!("{}.platform", binary.display()))
        .expect("remove invalid Mach-O platform");

    fs::write(format!("{}.archs", binary.display()), "arm64 x86_64\n")
        .expect("write invalid framework architectures");
    let architectures = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify invalid framework architectures");
    assert_eq!(architectures.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&architectures.stderr).contains("only arm64"));
}

#[cfg(unix)]
#[test]
fn ios_app_store_layout_treats_profile_hash_as_one_exact_record() {
    let (_directory, root, app, tools) = ios_app_store_cli_fixture();
    let frameworks = app.join("Frameworks");
    for framework_name in IOS_APP_STORE_FFMPEG_FRAMEWORKS {
        fs::write(
            frameworks.join(format!("{framework_name}.framework/profile-hash.txt")),
            "legacy\r\n",
        )
        .expect("write valid legacy profile record");
    }
    let legacy = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify legacy profile record");
    assert_eq!(legacy.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&legacy.stdout).contains("FFmpeg profile hash: legacy"));

    let profile = frameworks.join("VesperFFmpegAVCodec.framework/profile-hash.txt");
    fs::write(&profile, "leg acy\n").expect("write internally spaced profile record");
    let whitespace = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify internally spaced profile record");
    assert_eq!(whitespace.status.code(), Some(5));
    assert!(
        String::from_utf8_lossy(&whitespace.stderr).contains("one exact non-whitespace record")
    );

    fs::write(&profile, vec![b'x'; (4 * 1024) + 1]).expect("write oversized profile record");
    let oversized = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify oversized profile record");
    assert_eq!(oversized.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&oversized.stderr).contains("exceeds 4096 bytes"));

    fs::remove_file(&profile).expect("remove required profile record");
    let missing = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify missing profile record");
    assert_eq!(missing.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("Missing FFmpeg profile hash"));
}

#[cfg(unix)]
#[test]
fn ios_app_store_layout_enforces_bounded_filesystem_inputs() {
    let (_directory, root, app, tools) = ios_app_store_cli_fixture();
    let info = app.join("Frameworks/VesperFFmpegAVCodec.framework/Info.plist");
    fs::write(&info, vec![b'x'; (1024 * 1024) + 1]).expect("write oversized framework plist");
    let oversized_plist = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify oversized framework plist");
    assert_eq!(oversized_plist.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&oversized_plist.stderr).contains("exceeds 1048576 bytes"));

    let (_directory, root, app, tools) = ios_app_store_cli_fixture();
    let mut deep = app.join("deep");
    for _ in 0..=64 {
        deep.push("level");
    }
    fs::create_dir_all(&deep).expect("create over-depth app bundle fixture");
    let over_depth = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify over-depth app bundle");
    assert_eq!(over_depth.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&over_depth.stderr).contains("exceeds depth 64"));

    let (_directory, root, app, tools) = ios_app_store_cli_fixture();
    let aot = app.join("Frameworks/App.framework/App");
    fs::File::create(&aot)
        .and_then(|file| file.set_len((512 * 1024 * 1024) + 1))
        .expect("write oversized sparse Flutter AOT binary");
    let oversized_aot = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify oversized Flutter AOT binary");
    assert_eq!(oversized_aot.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&oversized_aot.stderr).contains("exceeds 536870912 bytes"));
}

#[cfg(unix)]
#[test]
fn ios_app_store_layout_classifies_missing_and_failed_apple_tools() {
    for variable in ["PLUTIL", "SWIFT", "OTOOL", "XCRUN", "LIPO"] {
        let (_directory, root, app, tools) = ios_app_store_cli_fixture();
        let output = ios_app_store_command(&root, &app, &tools)
            .env(variable, tools.join(format!("missing-{variable}")))
            .output()
            .expect("verify app with missing Apple tool");
        assert_eq!(
            output.status.code(),
            Some(4),
            "tool: {variable}; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("failed to spawn"));
    }

    let (_directory, root, app, tools) = ios_app_store_cli_fixture();
    write_executable_script(&tools.join("plutil"), "#!/bin/sh\nprintf 'not-json\\n'\n");
    let malformed = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify app with malformed plist tool output");
    assert_eq!(malformed.status.code(), Some(5));
    assert!(
        String::from_utf8_lossy(&malformed.stderr).contains("Unable to parse framework metadata")
    );

    write_executable_script(
        &tools.join("plutil"),
        "#!/bin/sh\nwhile :; do printf 0123456789abcdef0123456789abcdef; done\n",
    );
    let oversized = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify app with oversized Apple tool output");
    assert_eq!(oversized.status.code(), Some(6));
    assert!(String::from_utf8_lossy(&oversized.stderr).contains("stdout exceeds 1048576 bytes"));

    write_executable_script(&tools.join("plutil"), "#!/bin/sh\nkill -SEGV $$\n");
    let crashed = ios_app_store_command(&root, &app, &tools)
        .output()
        .expect("verify app with crashed Apple tool");
    assert_eq!(crashed.status.code(), Some(6));
    assert!(String::from_utf8_lossy(&crashed.stderr).contains("terminated without an exit code"));
}

#[cfg(unix)]
#[test]
fn ios_app_store_layout_ctrl_c_reaps_the_apple_tool_process_group() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let (_directory, root, app, tools) = ios_app_store_cli_fixture();
    let tool_pid_path = root.join("plutil.pid");
    let descendant_pid_path = root.join("plutil-descendant.pid");
    write_executable_script(
        &tools.join("plutil"),
        r#"#!/bin/sh
printf '%s' "$$" > "$VESPER_FAKE_PLUTIL_PID_PATH"
sleep 30 &
printf '%s' "$!" > "$VESPER_FAKE_PLUTIL_DESCENDANT_PID_PATH"
wait
"#,
    );
    let process = ios_app_store_command(&root, &app, &tools)
        .env("VESPER_FAKE_PLUTIL_PID_PATH", &tool_pid_path)
        .env(
            "VESPER_FAKE_PLUTIL_DESCENDANT_PID_PATH",
            &descendant_pid_path,
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cancellable iOS App Store verification");
    let cli_pid = i32::try_from(process.id()).expect("iOS CLI parent pid fits i32");
    let ready_deadline = Instant::now() + Duration::from_secs(10);
    while (!tool_pid_path.is_file() || !descendant_pid_path.is_file())
        && Instant::now() < ready_deadline
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    if !tool_pid_path.is_file() || !descendant_pid_path.is_file() {
        let _ = kill(Pid::from_raw(cli_pid), Signal::SIGINT);
        let output = process
            .wait_with_output()
            .expect("wait after fake plutil readiness timeout");
        panic!(
            "fake plutil did not start; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let tool_pid: i32 = fs::read_to_string(&tool_pid_path)
        .expect("read fake plutil pid")
        .parse()
        .expect("parse fake plutil pid");
    let descendant_pid: i32 = fs::read_to_string(&descendant_pid_path)
        .expect("read fake plutil descendant pid")
        .parse()
        .expect("parse fake plutil descendant pid");

    kill(Pid::from_raw(cli_pid), Signal::SIGINT).expect("cancel iOS App Store layout verification");
    let output = process
        .wait_with_output()
        .expect("wait for cancelled iOS App Store verification");
    assert_eq!(output.status.code(), Some(6));
    assert!(String::from_utf8_lossy(&output.stderr).contains("was cancelled"));

    for pid in [tool_pid, descendant_pid] {
        let reap_deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match kill(Pid::from_raw(pid), None) {
                Err(Errno::ESRCH) => break,
                _ if Instant::now() < reap_deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                result => panic!("iOS App Store verification process {pid} is alive: {result:?}"),
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn vesper_wrapper_routes_ios_app_store_layout_to_the_rust_cli() {
    use std::os::unix::fs::PermissionsExt;

    let (_directory, root, app, tools) = ios_app_store_cli_fixture();
    let scripts = root.join("scripts");
    fs::create_dir_all(&scripts).expect("create iOS wrapper fixture scripts directory");
    let wrapper = scripts.join("vesper");
    fs::copy(workspace_root().join("scripts/vesper"), &wrapper)
        .expect("copy Vesper wrapper into iOS App Store fixture");
    let mut permissions = fs::metadata(&wrapper)
        .expect("inspect iOS App Store fixture wrapper")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&wrapper, permissions)
        .expect("make iOS App Store fixture wrapper executable");

    let output = Command::new(&wrapper)
        .env("VESPER_CLI", env!("CARGO_BIN_EXE_vesper"))
        .env("PLUTIL", tools.join("plutil"))
        .env("SWIFT", tools.join("swift"))
        .env("OTOOL", tools.join("otool"))
        .env("XCRUN", tools.join("xcrun"))
        .env("LIPO", tools.join("lipo"))
        .args(["ios", "verify-app-store-layout"])
        .arg(&app)
        .output()
        .expect("route iOS App Store layout verification");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("Verified App Store-compatible optional iOS framework layout")
    );
}

#[cfg(unix)]
#[test]
fn ios_bridge_sync_publishes_both_files_and_noop_preserves_them() {
    use std::os::unix::fs::MetadataExt;

    let (_directory, root, tools) = ios_bridge_cli_fixture();
    sync_ios_bridge_fixture(&root, &tools);

    let shim = root.join("lib/ios/VesperPlayerKit/Sources/VesperPlayerKitBridgeShim");
    let header = shim.join("include/VesperPlayerKitBridgeShim.h");
    let source = shim.join("VesperPlayerKitBridgeShim.c");
    let header_contents = fs::read(&header).expect("read synchronized iOS bridge header");
    let source_contents = fs::read(&source).expect("read synchronized iOS bridge source");
    assert!(String::from_utf8_lossy(&header_contents).contains("vesper_runtime_fixture"));
    assert!(String::from_utf8_lossy(&source_contents).contains("player_ffi_fixture"));
    let header_inode = fs::metadata(&header)
        .expect("inspect synchronized iOS bridge header")
        .ino();
    let source_inode = fs::metadata(&source)
        .expect("inspect synchronized iOS bridge source")
        .ino();

    sync_ios_bridge_fixture(&root, &tools);

    assert_eq!(
        fs::metadata(&header)
            .expect("inspect no-op iOS bridge header")
            .ino(),
        header_inode
    );
    assert_eq!(
        fs::metadata(&source)
            .expect("inspect no-op iOS bridge source")
            .ino(),
        source_inode
    );
    assert_eq!(
        fs::read(header).expect("read no-op iOS bridge header"),
        header_contents
    );
    assert_eq!(
        fs::read(source).expect("read no-op iOS bridge source"),
        source_contents
    );
    assert!(!root.join(".vesper-build-locks").exists());
}

#[cfg(unix)]
#[test]
fn ios_bridge_verify_reports_header_drift_before_source_drift() {
    let (_directory, root, tools) = ios_bridge_cli_fixture();
    sync_ios_bridge_fixture(&root, &tools);
    let shim = root.join("lib/ios/VesperPlayerKit/Sources/VesperPlayerKitBridgeShim");
    let header = shim.join("include/VesperPlayerKitBridgeShim.h");
    let source = shim.join("VesperPlayerKitBridgeShim.c");
    fs::write(&header, "stale header\n").expect("write stale iOS bridge header");
    fs::write(&source, "stale source\n").expect("write stale iOS bridge source");

    let header_drift = ios_bridge_command(&root, &tools)
        .arg("verify-bridge-shim")
        .output()
        .expect("verify stale iOS bridge header");
    assert_eq!(header_drift.status.code(), Some(5));
    let stdout = String::from_utf8(header_drift.stdout).expect("UTF-8 header drift output");
    assert!(stdout.contains("generated/include/VesperPlayerKitBridgeShim.h"));
    assert!(!stdout.contains("generated/VesperPlayerKitBridgeShim.c"));
    assert!(
        String::from_utf8_lossy(&header_drift.stderr)
            .contains("VesperPlayerKitBridgeShim.h is out of sync")
    );

    sync_ios_bridge_fixture(&root, &tools);
    fs::write(&source, "stale source\n").expect("write stale iOS bridge source");
    let source_drift = ios_bridge_command(&root, &tools)
        .arg("verify-bridge-shim")
        .output()
        .expect("verify stale iOS bridge source");
    assert_eq!(source_drift.status.code(), Some(5));
    assert!(
        String::from_utf8_lossy(&source_drift.stdout)
            .contains("generated/VesperPlayerKitBridgeShim.c")
    );
    assert!(
        String::from_utf8_lossy(&source_drift.stderr)
            .contains("VesperPlayerKitBridgeShim.c is out of sync")
    );
}

#[cfg(unix)]
#[test]
fn ios_bridge_sync_rejects_forbidden_download_dto_pointer_casts() {
    let (_directory, root, tools) = ios_bridge_cli_fixture();
    fs::write(
        root.join("scripts/ios/bridge-shim/fragments/fixture.c.inc"),
        "{\n    const PlayerFfiDownloadTask *task = (const PlayerFfiDownloadTask *)0;\n    (void)task;\n    return player_ffi_fixture();\n}\n",
    )
    .expect("write forbidden iOS bridge cast fixture");

    let output = ios_bridge_command(&root, &tools)
        .arg("sync-bridge-shim")
        .output()
        .expect("synchronize forbidden iOS bridge cast fixture");
    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Download bridge DTO pointer casts are not allowed")
    );
    assert!(
        !root
            .join("lib/ios/VesperPlayerKit/Sources/VesperPlayerKitBridgeShim")
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn ios_bridge_verify_rejects_a_missing_rust_source_export() {
    let (_directory, root, tools) = ios_bridge_cli_fixture();
    sync_ios_bridge_fixture(&root, &tools);
    fs::write(
        root.join("crates/ffi/player-ffi-ios/src/lib.rs"),
        "#[unsafe(no_mangle)]\npub unsafe extern \"C\" fn player_ffi_other() -> bool { true }\n",
    )
    .expect("remove required iOS bridge Rust source export");

    let output = ios_bridge_command(&root, &tools)
        .arg("verify-bridge-shim")
        .output()
        .expect("verify missing iOS bridge Rust source export");
    assert_eq!(output.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&output.stderr).contains("player_ffi_fixture"));
}

#[cfg(unix)]
#[test]
fn ios_bridge_verify_ignores_commented_and_cfg_disabled_rust_exports() {
    let (_directory, root, tools) = ios_bridge_cli_fixture();
    sync_ios_bridge_fixture(&root, &tools);
    let rust_source = root.join("crates/ffi/player-ffi-ios/src/lib.rs");
    for source in [
        "/*\n#[unsafe(no_mangle)]\npub unsafe extern \"C\" fn player_ffi_fixture() -> bool { true }\n*/\n",
        "#[cfg(any())]\n#[unsafe(no_mangle)]\npub unsafe extern \"C\" fn player_ffi_fixture() -> bool { true }\n",
    ] {
        fs::write(&rust_source, source).expect("write inactive iOS Rust export fixture");
        let output = ios_bridge_command(&root, &tools)
            .arg("verify-bridge-shim")
            .output()
            .expect("verify inactive iOS Rust export fixture");
        assert_eq!(
            output.status.code(),
            Some(5),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("player_ffi_fixture"));
    }
}

#[cfg(unix)]
#[test]
fn ios_bridge_commands_reject_a_symlinked_generated_directory() {
    use std::os::unix::fs::symlink;

    let (directory, root, tools) = ios_bridge_cli_fixture();
    sync_ios_bridge_fixture(&root, &tools);
    let shim = root.join("lib/ios/VesperPlayerKit/Sources/VesperPlayerKitBridgeShim");
    let outside = directory.path().join("outside iOS bridge shim");
    fs::rename(&shim, &outside).expect("move iOS bridge shim outside fixture repository");
    symlink(&outside, &shim).expect("symlink iOS bridge shim outside fixture repository");

    for command in ["sync-bridge-shim", "verify-bridge-shim"] {
        let output = ios_bridge_command(&root, &tools)
            .arg(command)
            .output()
            .expect("run iOS bridge command with symlinked generated directory");
        assert_eq!(
            output.status.code(),
            Some(3),
            "command: {command}; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("symlink"));
    }
}

#[cfg(unix)]
#[test]
fn ios_bridge_verify_treats_missing_generated_files_as_drift() {
    let (_directory, root, tools) = ios_bridge_cli_fixture();
    sync_ios_bridge_fixture(&root, &tools);
    let shim = root.join("lib/ios/VesperPlayerKit/Sources/VesperPlayerKitBridgeShim");
    for relative in [
        "include/VesperPlayerKitBridgeShim.h",
        "VesperPlayerKitBridgeShim.c",
    ] {
        fs::remove_file(shim.join(relative)).expect("remove generated iOS bridge fixture file");
        let output = ios_bridge_command(&root, &tools)
            .arg("verify-bridge-shim")
            .output()
            .expect("verify missing generated iOS bridge fixture file");
        assert_eq!(
            output.status.code(),
            Some(5),
            "missing: {relative}; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("out of sync"));
        sync_ios_bridge_fixture(&root, &tools);
    }

    fs::remove_dir_all(&shim).expect("remove generated iOS bridge fixture directory");
    let output = ios_bridge_command(&root, &tools)
        .arg("verify-bridge-shim")
        .output()
        .expect("verify missing generated iOS bridge fixture directory");
    assert_eq!(
        output.status.code(),
        Some(5),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("out of sync"));
}

#[cfg(unix)]
#[test]
fn ios_bridge_verify_succeeds_in_source_only_mode() {
    let (_directory, root, tools) = ios_bridge_cli_fixture();
    sync_ios_bridge_fixture(&root, &tools);

    let output = ios_bridge_command(&root, &tools)
        .arg("verify-bridge-shim")
        .output()
        .expect("verify source-only iOS bridge fixture");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        b"No Rust FFI archive found; source-level bridge symbol verification only.\nVesperPlayerKit bridge shim is valid.\n"
    );
}

#[cfg(unix)]
#[test]
fn ios_bridge_verify_uses_stable_storage_and_compatibility_exit_codes() {
    let (_directory, root, tools) = ios_bridge_cli_fixture();
    sync_ios_bridge_fixture(&root, &tools);
    let missing_archive = root.join("missing archive.a");
    let missing = ios_bridge_command(&root, &tools)
        .args(["verify-bridge-shim", "--archive"])
        .arg(&missing_archive)
        .output()
        .expect("verify missing explicit iOS archive");
    assert_eq!(missing.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("Rust FFI archive is unavailable"));

    let missing_clang = ios_bridge_command(&root, &tools)
        .env("CLANG", tools.join("missing-cancelled-clang"))
        .arg("verify-bridge-shim")
        .output()
        .expect("verify iOS bridge fixture without clang");
    assert_eq!(missing_clang.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&missing_clang.stderr).contains("failed to spawn"));

    let archive = root.join("fixture archive.a");
    fs::write(&archive, b"fixture archive\n").expect("write iOS archive fixture");
    let missing_nm = ios_bridge_command(&root, &tools)
        .env("NM", tools.join("missing-nm"))
        .args(["verify-bridge-shim", "--archive"])
        .arg(&archive)
        .output()
        .expect("verify iOS bridge fixture without nm");
    assert_eq!(missing_nm.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&missing_nm.stderr).contains("failed to spawn"));
}

#[cfg(unix)]
#[test]
fn ios_bridge_archive_verification_ignores_matching_plain_strings() {
    let (_directory, root, tools) = ios_bridge_cli_fixture();
    sync_ios_bridge_fixture(&root, &tools);
    let archive = root.join("archive containing player_ffi_fixture.a");
    fs::write(&archive, b"player_ffi_fixture\n").expect("write string-only iOS archive fixture");
    let nm = tools.join("nm");
    write_executable_script(
        &nm,
        "#!/bin/sh\nprintf 'archive contains player_ffi_fixture\\n'\n",
    );

    let string_only = ios_bridge_command(&root, &tools)
        .env("NM", &nm)
        .args(["verify-bridge-shim", "--archive"])
        .arg(&archive)
        .output()
        .expect("verify string-only iOS archive fixture");
    assert_eq!(string_only.status.code(), Some(5));
    assert!(
        String::from_utf8_lossy(&string_only.stderr)
            .contains("Rust FFI archive is missing symbols")
    );

    let symbol_table_nm = if cfg!(target_os = "macos") {
        "#!/bin/sh\nif [ \"$1\" != \"--no-llvm-bc\" ]; then exit 87; fi\nprintf '0000000000000000 T _player_ffi_fixture\\n'\n"
    } else {
        "#!/bin/sh\nprintf '0000000000000000 T _player_ffi_fixture\\n'\n"
    };
    write_executable_script(&nm, symbol_table_nm);
    let symbol_table = ios_bridge_command(&root, &tools)
        .env("NM", &nm)
        .args(["verify-bridge-shim", "--archive"])
        .arg(&archive)
        .output()
        .expect("verify symbol-table iOS archive fixture");
    assert_eq!(
        symbol_table.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&symbol_table.stderr)
    );
    assert_eq!(
        symbol_table.stdout,
        b"VesperPlayerKit bridge shim is valid.\n"
    );
}

#[cfg(unix)]
#[test]
fn ios_bridge_verify_ctrl_c_reaps_the_clang_process_group() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let (_directory, root, tools) = ios_bridge_cli_fixture();
    sync_ios_bridge_fixture(&root, &tools);
    let clang_pid_path = root.join("clang.pid");
    let descendant_pid_path = root.join("clang-descendant.pid");
    write_executable_script(
        &tools.join("clang"),
        r#"#!/bin/sh
printf '%s' "$$" > "$VESPER_FAKE_IOS_CLANG_PID_PATH"
sleep 30 &
printf '%s' "$!" > "$VESPER_FAKE_IOS_CLANG_DESCENDANT_PID_PATH"
wait
"#,
    );
    let process = ios_bridge_command(&root, &tools)
        .env("VESPER_FAKE_IOS_CLANG_PID_PATH", &clang_pid_path)
        .env(
            "VESPER_FAKE_IOS_CLANG_DESCENDANT_PID_PATH",
            &descendant_pid_path,
        )
        .arg("verify-bridge-shim")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cancellable iOS bridge verification");
    let parent_pid = i32::try_from(process.id()).expect("iOS CLI parent pid fits i32");
    let ready_deadline = Instant::now() + Duration::from_secs(10);
    while (!clang_pid_path.is_file() || !descendant_pid_path.is_file())
        && Instant::now() < ready_deadline
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    if !clang_pid_path.is_file() || !descendant_pid_path.is_file() {
        let _ = kill(Pid::from_raw(parent_pid), Signal::SIGINT);
        let output = process
            .wait_with_output()
            .expect("wait after fake iOS clang readiness timeout");
        panic!(
            "fake iOS clang did not start; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let clang_pid: i32 = fs::read_to_string(&clang_pid_path)
        .expect("read fake iOS clang pid")
        .parse()
        .expect("parse fake iOS clang pid");
    let descendant_pid: i32 = fs::read_to_string(&descendant_pid_path)
        .expect("read fake iOS clang descendant pid")
        .parse()
        .expect("parse fake iOS clang descendant pid");

    kill(Pid::from_raw(parent_pid), Signal::SIGINT).expect("cancel iOS bridge verification");
    let output = process
        .wait_with_output()
        .expect("wait for cancelled iOS bridge verification");
    assert_eq!(output.status.code(), Some(6));
    assert!(String::from_utf8_lossy(&output.stderr).contains("was cancelled"));

    for pid in [clang_pid, descendant_pid] {
        let reap_deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match kill(Pid::from_raw(pid), None) {
                Err(Errno::ESRCH) => break,
                _ if Instant::now() < reap_deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                result => {
                    panic!("iOS bridge verification process {pid} is still alive: {result:?}")
                }
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn vesper_wrapper_routes_ios_bridge_root_forms_to_the_rust_cli() {
    use std::os::unix::fs::PermissionsExt;

    let (_directory, root, tools) = ios_bridge_cli_fixture();
    let scripts = root.join("scripts");
    fs::create_dir_all(&scripts).expect("create iOS wrapper fixture scripts directory");
    let wrapper = scripts.join("vesper");
    fs::copy(workspace_root().join("scripts/vesper"), &wrapper)
        .expect("copy Vesper wrapper into iOS bridge fixture");
    let mut permissions = fs::metadata(&wrapper)
        .expect("inspect iOS bridge fixture wrapper")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&wrapper, permissions).expect("make iOS bridge fixture wrapper executable");

    let implicit = Command::new(&wrapper)
        .env("VESPER_CLI", env!("CARGO_BIN_EXE_vesper"))
        .env("CLANG", tools.join("clang"))
        .env_remove("NM")
        .env_remove("VESPER_IOS_FFI_ARCHIVE")
        .args(["ios", "sync-bridge-shim"])
        .output()
        .expect("route implicit-root iOS bridge synchronization");
    assert_eq!(
        implicit.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&implicit.stderr)
    );
    assert_eq!(
        implicit.stdout,
        b"VesperPlayerKit bridge shim synchronized.\n"
    );

    let separate_root = Command::new(&wrapper)
        .env("VESPER_CLI", env!("CARGO_BIN_EXE_vesper"))
        .env("CLANG", tools.join("clang"))
        .env_remove("NM")
        .env_remove("VESPER_IOS_FFI_ARCHIVE")
        .args(["ios", "--root"])
        .arg(&root)
        .arg("verify-bridge-shim")
        .output()
        .expect("route separate-root iOS bridge verification");
    assert_eq!(
        separate_root.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&separate_root.stderr)
    );
    assert_eq!(
        separate_root.stdout,
        b"No Rust FFI archive found; source-level bridge symbol verification only.\nVesperPlayerKit bridge shim is valid.\n"
    );

    let joined_root = Command::new(&wrapper)
        .env("VESPER_CLI", env!("CARGO_BIN_EXE_vesper"))
        .env("CLANG", tools.join("clang"))
        .env_remove("NM")
        .env_remove("VESPER_IOS_FFI_ARCHIVE")
        .arg("ios")
        .arg(format!("--root={}", root.display()))
        .arg("sync-bridge-shim")
        .output()
        .expect("route joined-root iOS bridge synchronization");
    assert_eq!(
        joined_root.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&joined_root.stderr)
    );
    assert_eq!(
        joined_root.stdout,
        b"VesperPlayerKit bridge shim synchronized.\n"
    );
}

#[test]
fn contract_verify_preserves_the_cross_language_golden_output() {
    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["contract", "verify", "--root"])
        .arg(workspace_root())
        .output()
        .expect("run contract verification");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 stdout"),
        concat!(
            "checked subtitle error fields\n",
            "checked subtitle state fields\n",
            "checked player error code/category: unsupported, capability\n",
            "checked plugin diagnostics: decoderSupported, participated, decoder, frameProcessorSupported, available, frameProcessor, sourceNormalizerSupported, bypassed, sourceNormalizer\n",
            "checked download snapshot: dashSegments, downloading, video\n",
            "checked download output format: mp4\n",
            "checked system playback: continueAudio, seekBack, playPause, seekForward\n",
            "checked external playback: cast, dlna, auto, always, never, hls, routeConnected, routeDisconnected, loaded, playing, paused, stopped, suspended, error, discoveryDiagnostic\n",
            "checked external fallback default: mpegTs\n",
            "checked external playback result status: success, unavailable, unsupported, failed\n",
            "DTO contract drift check passed.\n",
            "Verified Rust and mobile distribution binary library names use libvesper_* outputs.\n",
            "Contract fixtures match the checked Rust, Android, iOS, and Flutter wire names.\n",
        )
    );
}

#[test]
fn contract_verify_reports_drift_on_stderr_with_exit_code_five() {
    let root = tempfile::tempdir().expect("temporary incomplete repository");
    fs::write(root.path().join("Cargo.toml"), "[workspace]\n").expect("write workspace manifest");
    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["contract", "verify", "--root"])
        .arg(root.path())
        .output()
        .expect("run incomplete contract verification");

    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 stderr")
            .contains("Missing contract source: fixtures/contracts/player_error.json")
    );
}

#[test]
fn contract_boundary_preserves_default_and_warning_output_channels() {
    let root = workspace_root();
    let clean = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["contract", "boundary", "--root"])
        .arg(&root)
        .output()
        .expect("run boundary scan");
    assert_eq!(clean.status.code(), Some(0));
    assert!(clean.stderr.is_empty());
    assert_eq!(
        String::from_utf8(clean.stdout).expect("UTF-8 clean output"),
        "Boundary invariant scan passed. Re-run with --warnings to inspect 17 focused warning candidates, or --all-warnings for the broad scan.\n"
    );

    let warnings = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["contract", "boundary", "--warnings", "--root"])
        .arg(root)
        .output()
        .expect("run boundary warning scan");
    assert_eq!(warnings.status.code(), Some(0));
    assert!(warnings.stderr.is_empty());
    let stdout = String::from_utf8(warnings.stdout).expect("UTF-8 warning output");
    assert_eq!(
        stdout
            .lines()
            .filter(|line| line.starts_with("WARN: "))
            .count(),
        17
    );
    assert!(stdout.starts_with(
        "WARN: crates/plugin/player-plugin/src/sdk/session.rs:142: Review release ownership ordering;"
    ));
    assert!(stdout.ends_with("Boundary invariant scan passed with 17 warning candidates.\n"));
}

#[test]
fn contract_boundary_reports_unknown_capability_drift_on_stderr() {
    let root = tempfile::tempdir().expect("temporary boundary repository");
    fs::write(root.path().join("Cargo.toml"), "[workspace]\n").expect("write workspace manifest");
    let kind_path = root.path().join(
        "lib/flutter/vesper_player_platform_interface/lib/src/models/plugin_diagnostic_models.dart",
    );
    let capability_path = root.path().join(
        "lib/flutter/vesper_player_platform_interface/lib/src/models/plugins/plugin_capability_models.dart",
    );
    let test_path = root
        .path()
        .join("lib/flutter/vesper_player_platform_interface/test/events_test.dart");
    fs::create_dir_all(kind_path.parent().expect("kind parent")).expect("create kind parent");
    fs::create_dir_all(capability_path.parent().expect("capability parent"))
        .expect("create capability parent");
    fs::create_dir_all(test_path.parent().expect("test parent")).expect("create test parent");
    fs::write(&kind_path, "enum VesperPluginCapabilityKind { decoder }\n")
        .expect("write invalid capability kind");
    fs::write(
        &capability_path,
        "const VesperPluginCapability._unknown() : kind = VesperPluginCapabilityKind.unknown;\n",
    )
    .expect("write capability constructor");
    fs::write(&test_path, "VesperPluginCapabilityKind.unknown\n").expect("write capability test");

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["contract", "boundary", "--root"])
        .arg(root.path())
        .output()
        .expect("run failing boundary scan");

    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 failure output")
            .contains("VesperPluginCapabilityKind must expose an unknown case")
    );
}

#[test]
fn release_metadata_preserves_output_and_ci_environment_contracts() {
    let directory = tempfile::tempdir().expect("temporary release metadata");
    let github_output = directory.path().join("github-output.txt");
    let github_env = directory.path().join("github-env.txt");
    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("GITHUB_OUTPUT", &github_output)
        .env("GITHUB_ENV", &github_env)
        .env("VESPER_RELEASE_IOS_BUILD", "777")
        .env("VESPER_RELEASE_ANDROID_VERSION_CODE", "778")
        .args([
            "release",
            "metadata-from-tag",
            "refs/tags/v1.2.34-rc.2",
            "--date",
            "2026-01-01",
            "--root",
        ])
        .arg(workspace_root())
        .output()
        .expect("resolve release metadata");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 metadata output"),
        concat!(
            "version=1.2.34\n",
            "ios_build=777\n",
            "android_version_code=778\n",
            "release_date=2026-01-01\n",
        )
    );
    assert_eq!(
        fs::read_to_string(github_output).expect("GitHub output"),
        concat!(
            "version=1.2.34\n",
            "ios_build=777\n",
            "android_version_code=778\n",
            "release_date=2026-01-01\n",
        )
    );
    assert_eq!(
        fs::read_to_string(github_env).expect("GitHub environment"),
        concat!(
            "VESPER_RELEASE_VERSION=1.2.34\n",
            "VESPER_RELEASE_BUILD=777\n",
            "VESPER_RELEASE_IOS_BUILD=777\n",
            "VESPER_RELEASE_ANDROID_VERSION_CODE=778\n",
            "VESPER_RELEASE_DATE=2026-01-01\n",
        )
    );
}

#[test]
fn release_version_validation_uses_stable_exit_codes() {
    let invalid = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args([
            "release",
            "metadata-from-tag",
            "v0.100.0",
            "--date",
            "2026-01-01",
            "--root",
        ])
        .arg(workspace_root())
        .output()
        .expect("reject release metadata");
    assert_eq!(invalid.status.code(), Some(3));
    assert!(invalid.stdout.is_empty());
    assert!(
        String::from_utf8(invalid.stderr)
            .expect("UTF-8 validation error")
            .contains("minor and patch in 0..99")
    );

    let verified = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["release", "verify-current", "--root"])
        .arg(workspace_root())
        .output()
        .expect("verify current release metadata");
    assert_eq!(
        verified.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&verified.stderr)
    );
    assert!(verified.stderr.is_empty());
    assert_eq!(
        String::from_utf8(verified.stdout).expect("UTF-8 verification output"),
        "Verified Vesper product version 0.4.0.\n"
    );
}

#[cfg(unix)]
#[test]
fn ffi_header_commands_keep_sync_verify_and_failure_contracts() {
    let fixture = ffi_fixture("old header\n");
    let root = fixture.path();
    let canonical_root = root.canonicalize().expect("canonical FFI fixture root");
    let tools = root.join("fake-tools");
    let path = ffi_path_with_tools(&tools);

    let stale = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", &path)
        .env("FAKE_CBINDGEN_HEADER", "generated header\n")
        .args(["ffi", "verify", "--root"])
        .arg(root)
        .output()
        .expect("verify stale FFI header");
    assert_eq!(stale.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&stale.stdout).contains("-old header"));
    assert!(String::from_utf8_lossy(&stale.stdout).contains("+generated header"));
    assert!(String::from_utf8_lossy(&stale.stderr).contains("Run scripts/vesper ffi sync."));
    assert_eq!(
        fs::read(root.join("include/player_ffi.h")).expect("read stale header"),
        b"old header\n"
    );

    let synced = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", &path)
        .env("FAKE_CBINDGEN_HEADER", "generated header\n")
        .args(["ffi", "sync", "--root"])
        .arg(root)
        .output()
        .expect("sync FFI header");
    assert_eq!(synced.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(synced.stdout).expect("UTF-8 sync output"),
        format!(
            "Synced {}\n",
            canonical_root.join("include/player_ffi.h").display()
        )
    );
    assert!(synced.stderr.is_empty());

    let verified = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", &path)
        .env("FAKE_CBINDGEN_HEADER", "generated header\n")
        .args(["ffi", "verify", "--root"])
        .arg(root)
        .output()
        .expect("verify current FFI header");
    assert_eq!(verified.status.code(), Some(0));
    assert_eq!(verified.stdout, b"include/player_ffi.h is up to date.\n");
    assert!(verified.stderr.is_empty());

    let generated = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", &path)
        .env("FAKE_CBINDGEN_HEADER", "regenerated header\n")
        .args(["ffi", "generate", "--root"])
        .arg(root)
        .output()
        .expect("generate FFI header");
    assert_eq!(generated.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(generated.stdout).expect("UTF-8 generate output"),
        format!(
            "Generated {}\n",
            canonical_root.join("include/player_ffi.h").display()
        )
    );
    assert_eq!(
        fs::read(root.join("include/player_ffi.h")).expect("read generated header"),
        b"regenerated header\n"
    );

    let empty_tools = root.join("empty-tools");
    fs::create_dir(&empty_tools).expect("create empty tool directory");
    let missing = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", &empty_tools)
        .args(["ffi", "verify", "--root"])
        .arg(root)
        .output()
        .expect("run missing cbindgen");
    assert_eq!(missing.status.code(), Some(3));
    assert!(missing.stdout.is_empty());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("cbindgen is required to verify"));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("cargo install cbindgen"));
}

#[cfg(unix)]
#[test]
fn ffi_c_host_smoke_preserves_build_only_and_source_selection() {
    let fixture = ffi_fixture("stable header\n");
    let root = fixture.path();
    let tools = root.join("fake-tools");
    let path = ffi_path_with_tools(&tools);

    let build = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", &path)
        .env("CARGO", tools.join("cargo"))
        .env("CC", tools.join("cc"))
        .env("FAKE_CBINDGEN_HEADER", "stable header\n")
        .args(["ffi", "c-host-smoke", "--build-only", "--root"])
        .arg(root)
        .output()
        .expect("build C host smoke");
    assert_eq!(
        build.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    assert!(build.stderr.is_empty());
    assert_eq!(
        String::from_utf8(build.stdout).expect("UTF-8 C host build output"),
        concat!(
            "[c-host] syncing generated FFI header\n",
            "include/player_ffi.h is up to date.\n",
            "[c-host] building player-ffi\n",
            "[c-host] compiling examples/c-host/main.c\n",
            "[c-host] built target/debug/c-host-smoke\n",
        )
    );

    let run = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", &path)
        .env("CARGO", tools.join("cargo"))
        .env("CC", tools.join("cc"))
        .env("FAKE_CBINDGEN_HEADER", "stable header\n")
        .args(["ffi", "c-host-smoke", "custom source.mp4", "--root"])
        .arg(root)
        .output()
        .expect("run C host smoke");
    assert_eq!(
        run.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(run.stderr.is_empty());
    assert!(String::from_utf8_lossy(&run.stdout).contains(
        "[c-host] running target/debug/c-host-smoke custom source.mp4\nsmoke:custom source.mp4\n"
    ));
}

#[cfg(unix)]
#[test]
fn ffi_c_host_ctrl_c_reaps_the_external_process_group() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let fixture = ffi_fixture("stable header\n");
    let root = fixture.path();
    let tools = root.join("fake-tools");
    let cargo_pid_path = root.join("cargo.pid");
    let descendant_pid_path = root.join("cargo-descendant.pid");
    write_executable_script(
        &tools.join("cargo"),
        r#"#!/bin/sh
printf '%s' "$$" > "$VESPER_FAKE_FFI_CARGO_PID_PATH"
sleep 30 &
printf '%s' "$!" > "$VESPER_FAKE_FFI_DESCENDANT_PID_PATH"
wait
"#,
    );
    let process = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", ffi_path_with_tools(&tools))
        .env("CARGO", tools.join("cargo"))
        .env("CC", tools.join("cc"))
        .env("FAKE_CBINDGEN_HEADER", "stable header\n")
        .env("VESPER_FAKE_FFI_CARGO_PID_PATH", &cargo_pid_path)
        .env("VESPER_FAKE_FFI_DESCENDANT_PID_PATH", &descendant_pid_path)
        .args(["ffi", "c-host-smoke", "--build-only", "--root"])
        .arg(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cancellable FFI command");
    let parent_pid = i32::try_from(process.id()).expect("parent pid fits i32");
    let ready_deadline = Instant::now() + Duration::from_secs(10);
    while (!cargo_pid_path.is_file() || !descendant_pid_path.is_file())
        && Instant::now() < ready_deadline
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    if !cargo_pid_path.is_file() || !descendant_pid_path.is_file() {
        let _ = kill(Pid::from_raw(parent_pid), Signal::SIGINT);
        let output = process
            .wait_with_output()
            .expect("wait after fake FFI Cargo readiness timeout");
        panic!(
            "fake FFI Cargo did not start; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let cargo_pid: i32 = fs::read_to_string(&cargo_pid_path)
        .expect("read FFI Cargo pid")
        .parse()
        .expect("parse FFI Cargo pid");
    let descendant_pid: i32 = fs::read_to_string(&descendant_pid_path)
        .expect("read FFI Cargo descendant pid")
        .parse()
        .expect("parse FFI Cargo descendant pid");

    kill(Pid::from_raw(parent_pid), Signal::SIGINT).expect("cancel FFI command");
    let output = process
        .wait_with_output()
        .expect("wait for cancelled FFI command");
    assert_eq!(output.status.code(), Some(6));
    assert!(String::from_utf8_lossy(&output.stdout).contains("[c-host] building player-ffi\n"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("was cancelled"));

    for pid in [cargo_pid, descendant_pid] {
        let reap_deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match kill(Pid::from_raw(pid), None) {
                Err(Errno::ESRCH) => break,
                _ if Instant::now() < reap_deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                result => panic!("FFI command process {pid} is still alive: {result:?}"),
            }
        }
    }
}

#[test]
fn mobile_binary_name_verification_uses_the_rust_cli_contract() {
    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["mobile", "verify-binary-names", "--root"])
        .arg(workspace_root())
        .output()
        .expect("run mobile binary name verification");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        b"Verified Rust and mobile distribution binary names use libvesper_* outputs.\n"
    );
}

#[cfg(unix)]
#[test]
fn android_jni_build_preserves_profile_abi_and_ndk_contracts() {
    let (_directory, root) = android_cli_fixture();
    let tools = root.join("fake Android tools");
    fs::create_dir_all(&tools).expect("create fake Android tools");
    let target_libdir = root.join("fake rust target/lib");
    fs::create_dir_all(&target_libdir).expect("create fake Rust target library directory");
    write_executable_script(
        &tools.join("rustc"),
        "#!/bin/sh\nprintf '%s\\n' \"$ANDROID_TEST_TARGET_LIBDIR\"\n",
    );
    write_executable_script(&tools.join("cargo"), "#!/bin/sh\nexit 0\n");
    write_executable_script(
        &tools.join("cargo-ndk"),
        r#"#!/bin/sh
{
  printf 'cwd=%s\n' "$PWD"
  printf 'cargo=%s\n' "$CARGO"
  printf 'sdk=%s\n' "$ANDROID_SDK_ROOT"
  printf 'ndk_root=%s\n' "$ANDROID_NDK_ROOT"
  printf 'ndk_home=%s\n' "$ANDROID_NDK_HOME"
  for argument in "$@"; do
    printf 'arg=%s\n' "$argument"
  done
} > "$ANDROID_CARGO_LOG"
output=''
abi=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o)
      shift
      output="$1"
      ;;
    -t)
      shift
      abi="$1"
      ;;
  esac
  shift
done
/bin/mkdir -p "$output/$abi"
printf 'new JNI artifact\n' > "$output/$abi/libvesper_player_android.so"
"#,
    );
    let sdk = root.join("Android SDK");
    let ndk = sdk.join("ndk/30.1.2");
    fs::create_dir_all(&ndk).expect("create selected Android NDK");
    fs::write(ndk.join("source.properties"), "Pkg.Revision = 30.1.2\n")
        .expect("write Android NDK metadata");
    let jni_libs = root.join("lib/android/vesper-player-kit/src/main/jniLibs");
    fs::create_dir_all(&jni_libs).expect("create previous Android JNI output");
    fs::write(jni_libs.join("stale.txt"), "stale\n").expect("write stale JNI output");
    let cargo_log = root.join("android-cargo.log");
    let system_usr_bin = PathBuf::from("/usr/bin");
    let system_bin = PathBuf::from("/bin");
    let path = std::env::join_paths([
        tools.as_path(),
        system_usr_bin.as_path(),
        system_bin.as_path(),
    ])
    .expect("compose fake Android PATH");

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", path)
        .env("ANDROID_TEST_TARGET_LIBDIR", &target_libdir)
        .env("ANDROID_CARGO_LOG", &cargo_log)
        .env("ANDROID_SDK_ROOT", &sdk)
        .env("ANDROID_NDK_VERSION", "30.1.2")
        .env_remove("ANDROID_NDK_ROOT")
        .env("RUST_ANDROID_ABIS", "x86_64")
        .args(["android", "jni", "release", "arm64-v8a", "--root"])
        .arg(&root)
        .output()
        .expect("build Android JNI fixture");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let canonical_root = root.canonicalize().expect("resolve Android CLI root");
    let canonical_jni = canonical_root.join("lib/android/vesper-player-kit/src/main/jniLibs");
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 Android JNI output"),
        format!(
            "\nBuilt Android JNI libraries into:\n  {}\nSelected Android ABIs:\n  arm64-v8a\n",
            canonical_jni.display()
        )
    );
    assert!(!canonical_jni.join("stale.txt").exists());
    assert_eq!(
        fs::read_to_string(canonical_jni.join("arm64-v8a/libvesper_player_android.so"))
            .expect("read staged Android JNI artifact"),
        "new JNI artifact\n"
    );
    let log = fs::read_to_string(cargo_log).expect("read Android cargo log");
    assert!(log.contains(&format!("cwd={}\n", canonical_root.display())));
    assert!(log.contains(&format!(
        "cargo={}\n",
        tools.join("cargo").canonicalize().expect("resolve fake Cargo").display()
    )));
    assert!(log.contains(&format!("sdk={}\n", sdk.display())));
    assert!(log.contains(&format!("ndk_root={}\n", ndk.display())));
    assert!(log.contains(&format!("ndk_home={}\n", ndk.display())));
    assert!(log.contains("arg=ndk\n"));
    assert!(log.contains("arg=-t\narg=arm64-v8a\n"));
    assert!(log.ends_with("arg=--release\n"));
}

#[cfg(unix)]
#[test]
fn android_jni_build_failure_preserves_previous_output_and_cleans_staging() {
    let (_directory, root) = android_cli_fixture();
    let tools = root.join("failing Android tools");
    fs::create_dir_all(&tools).expect("create failing Android tools");
    let target_libdir = root.join("fake rust target/lib");
    fs::create_dir_all(&target_libdir).expect("create fake Rust target library directory");
    write_executable_script(
        &tools.join("rustc"),
        "#!/bin/sh\nprintf '%s\\n' \"$ANDROID_TEST_TARGET_LIBDIR\"\n",
    );
    write_executable_script(&tools.join("cargo"), "#!/bin/sh\nexit 0\n");
    write_executable_script(
        &tools.join("cargo-ndk"),
        r#"#!/bin/sh
output=''
abi=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o)
      shift
      output="$1"
      ;;
    -t)
      shift
      abi="$1"
      ;;
  esac
  shift
done
/bin/mkdir -p "$output/$abi"
printf 'partial JNI artifact\n' > "$output/$abi/libvesper_player_android.so"
exit 23
"#,
    );
    let sdk = root.join("Android SDK");
    let ndk = root.join("Android NDK");
    fs::create_dir_all(&ndk).expect("create explicit Android NDK");
    let jni_parent = root.join("lib/android/vesper-player-kit/src/main");
    let jni_libs = jni_parent.join("jniLibs");
    fs::create_dir_all(&jni_libs).expect("create previous Android JNI output");
    fs::write(jni_libs.join("previous.txt"), "previous\n")
        .expect("write previous Android JNI output");
    let system_bin = PathBuf::from("/bin");
    let system_usr_bin = PathBuf::from("/usr/bin");
    let path = std::env::join_paths([
        tools.as_path(),
        system_usr_bin.as_path(),
        system_bin.as_path(),
    ])
    .expect("compose failing Android PATH");

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", path)
        .env("ANDROID_TEST_TARGET_LIBDIR", &target_libdir)
        .env("ANDROID_SDK_ROOT", &sdk)
        .env("ANDROID_NDK_ROOT", &ndk)
        .args(["android", "jni", "release", "arm64-v8a", "--root"])
        .arg(&root)
        .output()
        .expect("run failing Android JNI build");

    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Android JNI library build exited unsuccessfully")
    );
    assert_eq!(
        fs::read_to_string(jni_libs.join("previous.txt"))
            .expect("read preserved Android JNI output"),
        "previous\n"
    );
    assert!(!jni_libs.join("arm64-v8a").exists());
    let retained_stages = fs::read_dir(jni_parent)
        .expect("inspect Android JNI output parent")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".vesper-android-jni-stage-")
        })
        .count();
    assert_eq!(retained_stages, 0);
}

#[cfg(unix)]
#[test]
fn android_jni_canonicalizes_relative_path_tools_before_changing_directory() {
    use std::os::unix::fs::symlink;

    let (directory, root) = android_cli_fixture();
    let relative_tools = PathBuf::from("relative Android tools");
    let tools = directory.path().join(&relative_tools);
    fs::create_dir_all(&tools).expect("create relative Android tools");
    let target_libdir = root.join("fake rust target/lib");
    fs::create_dir_all(&target_libdir).expect("create fake Rust target library directory");
    write_executable_script(
        &tools.join("tool-proxy"),
        r#"#!/bin/sh
case "${0##*/}" in
  rustc)
    printf '%s\n' "$ANDROID_TEST_TARGET_LIBDIR"
    ;;
  cargo)
    exit 0
    ;;
  *)
    exit 98
    ;;
esac
"#,
    );
    symlink("tool-proxy", tools.join("rustc")).expect("link relative rustc proxy");
    symlink("tool-proxy", tools.join("cargo")).expect("link relative Cargo proxy");
    write_executable_script(
        &tools.join("cargo-ndk"),
        r#"#!/bin/sh
test "$CARGO" = "$EXPECTED_ANDROID_CARGO" || exit 97
output=''
abi=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o)
      shift
      output="$1"
      ;;
    -t)
      shift
      abi="$1"
      ;;
  esac
  shift
done
/bin/mkdir -p "$output/$abi"
printf 'canonical PATH tool\n' > "$output/$abi/libvesper_player_android.so"
"#,
    );
    let sdk = root.join("Android SDK");
    let ndk = root.join("Android NDK");
    fs::create_dir_all(&ndk).expect("create explicit Android NDK");

    let vesper = std::env::var_os("VESPER_TEST_EXECUTABLE")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_vesper").into());
    let output = Command::new(vesper)
        .current_dir(directory.path())
        .env("PATH", &relative_tools)
        .env(
            "EXPECTED_ANDROID_CARGO",
            tools
                .canonicalize()
                .expect("resolve relative tools directory")
                .join("cargo"),
        )
        .env("ANDROID_TEST_TARGET_LIBDIR", &target_libdir)
        .env("ANDROID_SDK_ROOT", &sdk)
        .env("ANDROID_NDK_ROOT", &ndk)
        .args(["android", "jni", "debug", "arm64-v8a", "--root"])
        .arg(&root)
        .output()
        .expect("build Android JNI with relative PATH tools");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(root.join(
            "lib/android/vesper-player-kit/src/main/jniLibs/arm64-v8a/libvesper_player_android.so"
        ))
        .expect("read Android JNI artifact built by relative PATH tool"),
        "canonical PATH tool\n"
    );
}

#[cfg(unix)]
#[test]
fn android_jni_bounds_ndk_scans_by_all_directory_entries() {
    let (_directory, root) = android_cli_fixture();
    let tools = root.join("bounded NDK tools");
    fs::create_dir_all(&tools).expect("create bounded NDK tools");
    let target_libdir = root.join("fake rust target/lib");
    fs::create_dir_all(&target_libdir).expect("create fake Rust target library directory");
    write_executable_script(
        &tools.join("rustc"),
        "#!/bin/sh\nprintf '%s\\n' \"$ANDROID_TEST_TARGET_LIBDIR\"\n",
    );
    write_executable_script(&tools.join("cargo-ndk"), "#!/bin/sh\nexit 0\n");
    write_executable_script(&tools.join("cargo"), "#!/bin/sh\nexit 99\n");
    let sdk = root.join("Android SDK");
    let ndk_parent = sdk.join("ndk");
    fs::create_dir_all(&ndk_parent).expect("create Android NDK parent");
    for index in 0..129 {
        fs::write(
            ndk_parent.join(format!("unrelated-{index:03}")),
            b"not an NDK",
        )
        .expect("write unrelated Android NDK entry");
    }

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", &tools)
        .env("ANDROID_TEST_TARGET_LIBDIR", &target_libdir)
        .env("ANDROID_SDK_ROOT", &sdk)
        .env("ANDROID_NDK_VERSION", "missing")
        .env_remove("ANDROID_NDK_ROOT")
        .args(["android", "jni", "debug", "arm64-v8a", "--root"])
        .arg(&root)
        .output()
        .expect("reject an unbounded Android NDK scan");

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("contains more than 128 entries; refusing an unbounded installation scan")
    );
}

#[cfg(unix)]
#[test]
fn android_jni_ctrl_c_reaps_the_build_process_group_and_preserves_output() {
    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let (_directory, root) = android_cli_fixture();
    let tools = root.join("cancelled Android tools");
    fs::create_dir_all(&tools).expect("create cancelled Android tools");
    let target_libdir = root.join("fake rust target/lib");
    fs::create_dir_all(&target_libdir).expect("create fake Rust target library directory");
    write_executable_script(
        &tools.join("rustc"),
        "#!/bin/sh\nprintf '%s\\n' \"$ANDROID_TEST_TARGET_LIBDIR\"\n",
    );
    write_executable_script(&tools.join("cargo"), "#!/bin/sh\nexit 0\n");
    write_executable_script(
        &tools.join("cargo-ndk"),
        r#"#!/bin/sh
printf '%s' "$$" > "$ANDROID_JNI_CARGO_PID_FILE"
sh -c 'trap "" INT TERM; printf "%s" "$$" > "$1"; while :; do /bin/sleep 1; done' sh "$ANDROID_JNI_DESCENDANT_PID_FILE" &
trap '' INT TERM
while :; do /bin/sleep 1; done
"#,
    );
    let sdk = root.join("Android SDK");
    let ndk = root.join("Android NDK");
    fs::create_dir_all(&ndk).expect("create explicit Android NDK");
    let jni_parent = root.join("lib/android/vesper-player-kit/src/main");
    let jni_libs = jni_parent.join("jniLibs");
    fs::create_dir_all(&jni_libs).expect("create previous Android JNI output");
    fs::write(jni_libs.join("previous.txt"), "previous\n")
        .expect("write previous Android JNI output");
    let cargo_pid_path = root.join("cancelled-jni-cargo.pid");
    let descendant_pid_path = root.join("cancelled-jni-descendant.pid");
    let system_bin = PathBuf::from("/bin");
    let system_usr_bin = PathBuf::from("/usr/bin");
    let path = std::env::join_paths([
        tools.as_path(),
        system_usr_bin.as_path(),
        system_bin.as_path(),
    ])
    .expect("compose cancelled Android PATH");

    let mut process = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", path)
        .env("ANDROID_TEST_TARGET_LIBDIR", &target_libdir)
        .env("ANDROID_SDK_ROOT", &sdk)
        .env("ANDROID_NDK_ROOT", &ndk)
        .env("ANDROID_JNI_CARGO_PID_FILE", &cargo_pid_path)
        .env("ANDROID_JNI_DESCENDANT_PID_FILE", &descendant_pid_path)
        .env_remove("VESPER_ANDROID_PARENT_SUPERVISES_PROCESS_GROUP")
        .args(["android", "jni", "release", "arm64-v8a", "--root"])
        .arg(&root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start cancellable Android JNI build");
    let parent_pid = i32::try_from(process.id()).expect("Vesper parent pid");
    let ready_deadline = Instant::now() + Duration::from_secs(15);
    while (!cargo_pid_path.is_file() || !descendant_pid_path.is_file())
        && Instant::now() < ready_deadline
    {
        if process
            .try_wait()
            .expect("poll cancellable Android JNI build")
            .is_some()
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !cargo_pid_path.is_file() || !descendant_pid_path.is_file() {
        let _ = kill(Pid::from_raw(parent_pid), Signal::SIGINT);
        let output = process
            .wait_with_output()
            .expect("wait after Android JNI readiness failure");
        panic!(
            "Android JNI build process group was not ready; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let cargo_pid: i32 = fs::read_to_string(&cargo_pid_path)
        .expect("read Android JNI Cargo pid")
        .parse()
        .expect("parse Android JNI Cargo pid");
    let descendant_pid: i32 = fs::read_to_string(&descendant_pid_path)
        .expect("read Android JNI descendant pid")
        .parse()
        .expect("parse Android JNI descendant pid");

    kill(Pid::from_raw(parent_pid), Signal::SIGINT).expect("cancel Android JNI build");
    let output = process
        .wait_with_output()
        .expect("wait for cancelled Android JNI build");
    assert_eq!(output.status.code(), Some(6));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Android JNI library build was cancelled")
    );
    assert_eq!(
        fs::read_to_string(jni_libs.join("previous.txt"))
            .expect("read preserved Android JNI output after cancellation"),
        "previous\n"
    );
    assert_eq!(
        fs::read_dir(jni_parent)
            .expect("inspect Android JNI output parent after cancellation")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".vesper-android-jni-stage-")
            })
            .count(),
        0
    );

    for pid in [cargo_pid, descendant_pid] {
        let reap_deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match kill(Pid::from_raw(pid), None) {
                Err(Errno::ESRCH) => break,
                _ if Instant::now() < reap_deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                result => panic!("Android JNI process {pid} is still alive: {result:?}"),
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn android_jni_rejects_incomplete_or_untrusted_parent_staging_protocols() {
    let (_directory, root) = android_cli_fixture();
    let output_parent = root.join("lib/android/vesper-player-kit/src/main");
    let staged = output_parent.join(".vesper-android-host-jni-stage-fixture");
    let real_output = output_parent.join("jniLibs");
    fs::create_dir_all(&staged).expect("create parent-managed JNI stage fixture");
    fs::create_dir_all(&real_output).expect("create real JNI output fixture");

    let run = |output: Option<&std::path::Path>, marker: Option<&str>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_vesper"));
        command
            .env_remove("VESPER_ANDROID_HOST_JNI_LIBS")
            .env_remove("VESPER_ANDROID_PARENT_HOLDS_JNI_LOCK")
            .args(["android", "jni", "release", "arm64-v8a", "--root"])
            .arg(&root);
        if let Some(output) = output {
            command.env("VESPER_ANDROID_HOST_JNI_LIBS", output);
        }
        if let Some(marker) = marker {
            command.env("VESPER_ANDROID_PARENT_HOLDS_JNI_LOCK", marker);
        }
        command
            .output()
            .expect("run parent staging protocol fixture")
    };

    let override_only = run(Some(&staged), None);
    assert_eq!(override_only.status.code(), Some(4));
    assert!(override_only.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&override_only.stderr)
            .contains("requires VESPER_ANDROID_PARENT_HOLDS_JNI_LOCK=1")
    );

    let marker_only = run(None, Some("1"));
    assert_eq!(marker_only.status.code(), Some(4));
    assert!(
        String::from_utf8_lossy(&marker_only.stderr)
            .contains("requires VESPER_ANDROID_HOST_JNI_LIBS")
    );

    let invalid_marker = run(Some(&staged), Some("true"));
    assert_eq!(invalid_marker.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&invalid_marker.stderr).contains("must be exactly 1"));

    let real_output_override = run(Some(&real_output), Some("1"));
    assert_eq!(real_output_override.status.code(), Some(4));
    assert!(
        String::from_utf8_lossy(&real_output_override.stderr)
            .contains("parent-created host JNI staging directory")
    );
}

#[cfg(unix)]
#[test]
fn android_sample_apks_use_release_lock_and_publish_both_apps_atomically() {
    let (_directory, root) = android_cli_fixture();
    let (log, fixture) = prepare_android_sample_apk_fixture(&root);
    let output_directory = root.join("release output/android samples");
    fs::create_dir_all(&output_directory).expect("create previous Android sample output");
    fs::write(output_directory.join("previous.apk"), b"previous")
        .expect("write previous Android sample output");
    let path = std::env::join_paths([
        fixture.tools.as_path(),
        std::path::Path::new("/usr/bin"),
        std::path::Path::new("/bin"),
    ])
    .expect("compose Android sample PATH");

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", path)
        .env("ANDROID_SAMPLE_LOG", &log)
        .env("ANDROID_SDK_ROOT", &fixture.sdk)
        .env(
            "VESPER_FAKE_RUST_TARGET_LIBDIR",
            &fixture.rust_target_libdir,
        )
        .env_remove("CI")
        .env_remove("RUST_ANDROID_ABIS")
        .args(["android", "sample-apks"])
        .arg(&output_directory)
        .arg("arm64-v8a")
        .arg("--root")
        .arg(&root)
        .output()
        .expect("stage Android sample APKs");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert!(!output_directory.join("previous.apk").exists());
    let compose =
        output_directory.join("VesperPlayerAndroidComposeHost-android-arm64-v8a-debug-signed.apk");
    let flutter =
        output_directory.join("VesperPlayerFlutterHost-android-arm64-v8a-debug-signed.apk");
    assert!(compose.is_file());
    assert!(flutter.is_file());
    let stdout = String::from_utf8(output.stdout).expect("Android sample stdout");
    assert!(stdout.contains(&compose.display().to_string()));
    assert!(stdout.contains(&flutter.display().to_string()));

    let log = fs::read_to_string(log).expect("read Android sample build log");
    let canonical_root = root
        .canonicalize()
        .expect("resolve Android sample fixture root");
    assert!(log.contains("ffmpeg version=8.1.2"));
    assert!(log.contains("url=https://ffmpeg.org/releases/ffmpeg-8.1.2.tar.xz"));
    assert!(!log.contains("sha=unset"));
    assert!(log.contains("arg=-Pvesper.player.android.abis=arm64-v8a"));
    assert!(log.contains("arg=-Pvesper.player.android.app.abis=arm64-v8a"));
    assert!(log.contains(&format!(
        "gradle home={}",
        canonical_root.join("examples/android-compose-host/.gradle/gradle-user-home")
            .display()
    )));
    assert!(log.contains(&format!(
        "gradle home={}",
        canonical_root.join("examples/flutter-host/android/.gradle/gradle-user-home")
            .display()
    )));
    assert_eq!(
        log.lines()
            .filter(|line| line.starts_with("gradle home="))
            .count(),
        2,
        "log:\n{log}"
    );
    assert!(!root.join(".gradle").exists());
}

#[cfg(unix)]
#[test]
fn android_sample_apks_reject_missing_flutter_aot_without_replacing_previous_output() {
    let (_directory, root) = android_cli_fixture();
    let (log, fixture) = prepare_android_sample_apk_fixture(&root);
    let flutter_apk =
        root.join("examples/flutter-host/build/app/outputs/flutter-apk/app-arm64-v8a-release.apk");
    write_test_zip(
        &flutter_apk,
        &[("AndroidManifest.xml", b"flutter manifest without aot")],
    );
    let output_directory = root.join("release output/android samples");
    fs::create_dir_all(&output_directory).expect("create previous Android sample output");
    fs::write(output_directory.join("previous.apk"), b"previous")
        .expect("write previous Android sample output");
    let path = std::env::join_paths([
        fixture.tools.as_path(),
        std::path::Path::new("/usr/bin"),
        std::path::Path::new("/bin"),
    ])
    .expect("compose Android sample PATH");

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", path)
        .env("ANDROID_SAMPLE_LOG", &log)
        .env("ANDROID_SDK_ROOT", &fixture.sdk)
        .env(
            "VESPER_FAKE_RUST_TARGET_LIBDIR",
            &fixture.rust_target_libdir,
        )
        .env_remove("CI")
        .args(["android", "sample-apks"])
        .arg(&output_directory)
        .arg("arm64-v8a")
        .arg("--root")
        .arg(&root)
        .output()
        .expect("reject malformed Flutter sample APK");

    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("missing required lib/arm64-v8a/libapp.so")
    );
    assert_eq!(
        fs::read(output_directory.join("previous.apk"))
            .expect("read preserved Android sample output"),
        b"previous"
    );
    assert_eq!(
        fs::read_dir(
            output_directory
                .parent()
                .expect("Android sample output parent")
        )
        .expect("inspect Android sample staging cleanup")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".vesper-android-samples-stage-")
        })
        .count(),
        0
    );
}

#[cfg(unix)]
#[test]
fn android_aar_uses_fallback_cached_gradle_and_exact_core_tasks() {
    let (_directory, root) = android_cli_fixture();
    let gradle_log = root.join("android-gradle.log");
    write_android_cached_gradle(
        &root,
        "examples/android-compose-host",
        r#"#!/bin/sh
/bin/mkdir -p "$VESPER_ANDROID_HOST_JNI_LIBS/arm64-v8a"
printf 'fixture host\n' > "$VESPER_ANDROID_HOST_JNI_LIBS/arm64-v8a/libvesper_player_android.so"
{
  printf 'cwd=%s\n' "$PWD"
  printf 'gradle_user_home=%s\n' "$GRADLE_USER_HOME"
  printf 'vesper_cli=%s\n' "$VESPER_CLI"
  for argument in "$@"; do
    printf 'arg=%s\n' "$argument"
  done
} > "$ANDROID_GRADLE_LOG"
"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env_remove("CI")
        .env_remove("GRADLE_USER_HOME")
        .env("VESPER_ANDROID_INCLUDE_OPTIONAL_PLUGINS", "YES")
        .env("ANDROID_GRADLE_LOG", &gradle_log)
        .args([
            "android",
            "aar",
            "assembleFixture",
            "--include-optional-plugins=false",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("build Android AAR fixture");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    let canonical_root = root.canonicalize().expect("resolve Android CLI root");
    let project = canonical_root.join("lib/android");
    let log = fs::read_to_string(gradle_log).expect("read Android Gradle log");
    assert!(log.contains(&format!("cwd={}\n", canonical_root.display())));
    assert!(log.contains(&format!(
        "gradle_user_home={}\n",
        project.join(".gradle/gradle-user-home").display()
    )));
    assert!(log.contains("vesper_cli="));
    assert!(log.contains(&format!("arg=-p\narg={}\n", project.display())));
    assert!(log.ends_with(
        "arg=:vesper-player-kit:assembleFixture\n\
         arg=:vesper-player-kit-compose:assembleFixture\n\
         arg=:vesper-player-kit-compose-ui:assembleFixture\n"
    ));
    assert!(!canonical_root.join(".gradle").exists());
}

#[cfg(unix)]
#[test]
fn android_aar_uses_primary_cache_without_requiring_the_fallback_project() {
    let (_directory, root) = android_cli_fixture();
    fs::remove_dir_all(root.join("examples/android-compose-host"))
        .expect("remove Android fallback project");
    write_android_cached_gradle(
        &root,
        "lib/android",
        r#"#!/bin/sh
/bin/mkdir -p "$VESPER_ANDROID_HOST_JNI_LIBS/arm64-v8a"
printf 'fixture host\n' > "$VESPER_ANDROID_HOST_JNI_LIBS/arm64-v8a/libvesper_player_android.so"
"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env_remove("CI")
        .args([
            "android",
            "aar",
            "assembleFixture",
            "--include-optional-plugins=false",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("build Android AAR without fallback project");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[cfg(unix)]
#[test]
fn android_aar_rejects_a_missing_host_jni_stage_without_replacing_previous_output() {
    let (_directory, root) = android_cli_fixture();
    let host_output = root.join("lib/android/vesper-player-kit/src/main/jniLibs");
    fs::create_dir_all(&host_output).expect("create previous Android host JNI output");
    fs::write(host_output.join("previous.txt"), "previous host output\n")
        .expect("write previous Android host JNI output");
    write_android_cached_gradle(&root, "lib/android", "#!/bin/sh\nexit 0\n");

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env_remove("CI")
        .args([
            "android",
            "aar",
            "assembleFixture",
            "--include-optional-plugins=false",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("run Android AAR with missing staged host JNI");

    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("did not stage required host JNI library")
    );
    assert_eq!(
        fs::read_to_string(host_output.join("previous.txt"))
            .expect("read preserved Android host JNI output"),
        "previous host output\n"
    );
    assert_eq!(
        fs::read_dir(
            host_output
                .parent()
                .expect("Android host JNI output parent")
        )
        .expect("inspect Android host JNI staging cleanup")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".vesper-android-host-jni-stage-")
        })
        .count(),
        0
    );
}

#[cfg(unix)]
#[test]
fn android_aar_rejects_non_artifact_tasks_before_starting_gradle() {
    let (_directory, root) = android_cli_fixture();
    let marker = root.join("gradle-started");
    write_android_cached_gradle(
        &root,
        "lib/android",
        "#!/bin/sh\nprintf 'started\\n' > \"$ANDROID_GRADLE_MARKER\"\n",
    );

    for task in [
        "clean",
        "assembleDebugAndroidTest",
        "assembleReleaseUnitTest",
        "assembleDebugTestFixtures",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
            .env("ANDROID_GRADLE_MARKER", &marker)
            .env_remove("CI")
            .args([
                "android",
                "aar",
                task,
                "--include-optional-plugins=false",
                "--root",
            ])
            .arg(&root)
            .output()
            .expect("reject a non-artifact Android Gradle task");

        assert_eq!(output.status.code(), Some(2), "task: {task}");
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("AAR-producing Gradle task"));
    }
    assert!(!marker.exists());
}

#[cfg(unix)]
#[test]
fn android_aar_rejects_empty_optional_artifacts_without_replacing_previous_outputs() {
    let (_directory, root) = android_cli_fixture();
    let host_output = root.join("lib/android/vesper-player-kit/src/main/jniLibs");
    fs::create_dir_all(&host_output).expect("create previous Android host JNI output");
    fs::write(host_output.join("previous.txt"), "previous host output\n")
        .expect("write previous Android host JNI output");
    let outputs = prepare_android_optional_project_fixture(&root);
    for (index, output) in outputs.iter().enumerate() {
        fs::create_dir_all(output).expect("create previous optional Android output");
        fs::write(
            output.join("previous.txt"),
            format!("previous optional output {index}\n"),
        )
        .expect("write previous optional Android output");
    }

    let tools = root.join("empty optional Android tools");
    let path = prepare_android_optional_toolchain(&root, &tools);
    write_android_cached_gradle(
        &root,
        "lib/android",
        r#"#!/bin/sh
/bin/mkdir -p "$VESPER_ANDROID_HOST_JNI_LIBS/arm64-v8a"
printf 'replacement host\n' > "$VESPER_ANDROID_HOST_JNI_LIBS/arm64-v8a/libvesper_player_android.so"
"#,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", path)
        .env(
            "ANDROID_TEST_TARGET_LIBDIR",
            root.join("fake rust target/lib"),
        )
        .env("ANDROID_SDK_ROOT", root.join("Android SDK"))
        .env("ANDROID_NDK_ROOT", root.join("Android NDK"))
        .env("ANDROID_TEST_CARGO_NDK_EMPTY", "1")
        .env_remove("CI")
        .args([
            "android",
            "aar",
            "assembleFixture",
            "--include-optional-plugins=true",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("reject empty optional Android artifacts");

    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("did not produce"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(host_output.join("previous.txt"))
            .expect("read preserved Android host JNI output"),
        "previous host output\n"
    );
    for (index, output) in outputs.iter().enumerate() {
        assert_eq!(
            fs::read_to_string(output.join("previous.txt"))
                .expect("read preserved optional Android output"),
            format!("previous optional output {index}\n")
        );
    }
}

#[cfg(unix)]
#[test]
fn android_aar_ctrl_c_reaps_nested_jni_process_groups() {
    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let (_directory, root) = android_cli_fixture();
    let tools = root.join("nested Android tools");
    fs::create_dir_all(&tools).expect("create nested Android tools");
    let target_libdir = root.join("fake rust target/lib");
    fs::create_dir_all(&target_libdir).expect("create fake Rust target library directory");
    write_executable_script(
        &tools.join("rustc"),
        "#!/bin/sh\nprintf '%s\\n' \"$ANDROID_TEST_TARGET_LIBDIR\"\n",
    );
    write_executable_script(&tools.join("cargo"), "#!/bin/sh\nexit 0\n");
    write_executable_script(
        &tools.join("cargo-ndk"),
        r#"#!/bin/sh
printf '%s' "$$" > "$NESTED_ANDROID_CARGO_PID_FILE"
sh -c 'trap "" INT TERM; printf "%s" "$$" > "$1"; while :; do /bin/sleep 1; done' sh "$NESTED_ANDROID_DESCENDANT_PID_FILE" &
trap '' INT TERM
while :; do /bin/sleep 1; done
"#,
    );
    write_android_cached_gradle(
        &root,
        "lib/android",
        r#"#!/bin/sh
trap 'exit 130' INT TERM
"$VESPER_CLI" android jni debug arm64-v8a --root "$NESTED_ANDROID_ROOT" &
wait "$!"
"#,
    );
    let sdk = root.join("Android SDK");
    let ndk = root.join("Android NDK");
    fs::create_dir_all(&ndk).expect("create explicit Android NDK");
    let jni_libs = root.join("lib/android/vesper-player-kit/src/main/jniLibs");
    fs::create_dir_all(&jni_libs).expect("create previous Android JNI output");
    fs::write(jni_libs.join("previous.txt"), "previous\n")
        .expect("write previous Android JNI output");
    let cargo_pid_path = root.join("nested-cargo.pid");
    let descendant_pid_path = root.join("nested-cargo-descendant.pid");
    let system_bin = PathBuf::from("/bin");
    let system_usr_bin = PathBuf::from("/usr/bin");
    let path = std::env::join_paths([
        tools.as_path(),
        system_usr_bin.as_path(),
        system_bin.as_path(),
    ])
    .expect("compose nested Android PATH");

    let mut process = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", path)
        .env("ANDROID_TEST_TARGET_LIBDIR", &target_libdir)
        .env("ANDROID_SDK_ROOT", &sdk)
        .env("ANDROID_NDK_ROOT", &ndk)
        .env("NESTED_ANDROID_ROOT", &root)
        .env("NESTED_ANDROID_CARGO_PID_FILE", &cargo_pid_path)
        .env("NESTED_ANDROID_DESCENDANT_PID_FILE", &descendant_pid_path)
        .env_remove("CI")
        .args([
            "android",
            "aar",
            "assembleFixture",
            "--include-optional-plugins=false",
            "--root",
        ])
        .arg(&root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start nested Android AAR build");
    let parent_pid = i32::try_from(process.id()).expect("Vesper parent pid");
    let ready_deadline = Instant::now() + Duration::from_secs(15);
    while (!cargo_pid_path.is_file() || !descendant_pid_path.is_file())
        && Instant::now() < ready_deadline
    {
        if process
            .try_wait()
            .expect("poll nested Android AAR build")
            .is_some()
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !cargo_pid_path.is_file() || !descendant_pid_path.is_file() {
        let _ = kill(Pid::from_raw(parent_pid), Signal::SIGINT);
        let output = process
            .wait_with_output()
            .expect("wait after nested Android readiness failure");
        panic!(
            "nested Android JNI build was not ready; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let cargo_pid: i32 = fs::read_to_string(&cargo_pid_path)
        .expect("read nested Android Cargo pid")
        .parse()
        .expect("parse nested Android Cargo pid");
    let descendant_pid: i32 = fs::read_to_string(&descendant_pid_path)
        .expect("read nested Android descendant pid")
        .parse()
        .expect("parse nested Android descendant pid");

    kill(Pid::from_raw(parent_pid), Signal::SIGINT).expect("cancel nested Android AAR build");
    let output = process
        .wait_with_output()
        .expect("wait for cancelled nested Android AAR build");
    assert_eq!(output.status.code(), Some(6));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Android AAR Gradle build was cancelled")
    );
    assert_eq!(
        fs::read_to_string(jni_libs.join("previous.txt"))
            .expect("read preserved Android JNI output after cancellation"),
        "previous\n"
    );

    for pid in [cargo_pid, descendant_pid] {
        let reap_deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match kill(Pid::from_raw(pid), None) {
                Err(Errno::ESRCH) => break,
                _ if Instant::now() < reap_deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                result => panic!("nested Android process {pid} is still alive: {result:?}"),
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn vesper_wrapper_routes_android_root_forms_to_the_rust_cli() {
    use std::os::unix::fs::PermissionsExt;

    let (_directory, root) = android_cli_fixture();
    let gradle_log = root.join("wrapper-android-gradle.log");
    write_android_cached_gradle(
        &root,
        "lib/android",
        r#"#!/bin/sh
/bin/mkdir -p "$VESPER_ANDROID_HOST_JNI_LIBS/arm64-v8a"
printf 'fixture host\n' > "$VESPER_ANDROID_HOST_JNI_LIBS/arm64-v8a/libvesper_player_android.so"
{
  printf 'begin=gradle\n'
  printf 'cwd=%s\n' "$PWD"
  for argument in "$@"; do
    printf 'arg=%s\n' "$argument"
  done
  printf 'end=gradle\n'
} >> "$WRAPPER_ANDROID_GRADLE_LOG"
"#,
    );
    let wrapper = root.join("scripts/vesper");
    fs::create_dir_all(wrapper.parent().expect("Android wrapper parent"))
        .expect("create Android wrapper directory");
    fs::copy(workspace_root().join("scripts/vesper"), &wrapper)
        .expect("copy Vesper wrapper into Android fixture");
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700))
        .expect("make Android fixture wrapper executable");
    let outside_repository = tempfile::tempdir().expect("outside Android repository directory");

    let invocations = [
        vec![
            "android".to_owned(),
            "aar".to_owned(),
            "assembleFixture".to_owned(),
            "--include-optional-plugins=false".to_owned(),
        ],
        vec![
            "android".to_owned(),
            "aar".to_owned(),
            "assembleFixture".to_owned(),
            "--include-optional-plugins=false".to_owned(),
            "--root".to_owned(),
            root.display().to_string(),
        ],
        vec![
            "android".to_owned(),
            "--root".to_owned(),
            root.display().to_string(),
            "aar".to_owned(),
            "assembleFixture".to_owned(),
            "--include-optional-plugins=false".to_owned(),
        ],
        vec![
            "android".to_owned(),
            format!("--root={}", root.display()),
            "aar".to_owned(),
            "assembleFixture".to_owned(),
            "--include-optional-plugins=false".to_owned(),
        ],
    ];
    for arguments in invocations {
        let output = Command::new(&wrapper)
            .current_dir(outside_repository.path())
            .env("VESPER_CLI", env!("CARGO_BIN_EXE_vesper"))
            .env("WRAPPER_ANDROID_GRADLE_LOG", &gradle_log)
            .env_remove("CI")
            .args(arguments)
            .output()
            .expect("route Android AAR command through Vesper wrapper");
        assert_eq!(
            output.status.code(),
            Some(0),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }

    let help = Command::new(&wrapper)
        .current_dir(outside_repository.path())
        .env("VESPER_CLI", env!("CARGO_BIN_EXE_vesper"))
        .args(["android", "--help"])
        .output()
        .expect("route Android group help through Vesper wrapper");
    assert_eq!(help.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&help.stdout).contains("Usage: vesper android"));
    assert!(help.stderr.is_empty());

    for arguments in [["android", "--root"], ["android", "unknown-command"]] {
        let invalid = Command::new(&wrapper)
            .current_dir(outside_repository.path())
            .env("VESPER_CLI", env!("CARGO_BIN_EXE_vesper"))
            .args(arguments)
            .output()
            .expect("route malformed Android command through Vesper wrapper");
        assert_eq!(invalid.status.code(), Some(2));
        assert!(invalid.stdout.is_empty());
        assert!(!invalid.stderr.is_empty());
    }

    let canonical_root = root.canonicalize().expect("resolve Android wrapper root");
    let log = fs::read_to_string(gradle_log).expect("read Android wrapper Gradle log");
    assert_eq!(log.matches("begin=gradle\n").count(), 4);
    assert_eq!(
        log.matches(&format!("cwd={}\n", canonical_root.display()))
            .count(),
        4
    );
    assert_eq!(
        log.matches("arg=:vesper-player-kit:assembleFixture\n")
            .count(),
        4
    );
}

#[cfg(unix)]
#[test]
fn vesper_wrapper_maps_bootstrap_and_unexpected_cli_failures_to_stable_exit_codes() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().expect("temporary Vesper wrapper bootstrap fixture");
    let root = directory.path().join("wrapper fixture");
    let wrapper = root.join("scripts/vesper");
    fs::create_dir_all(wrapper.parent().expect("wrapper parent")).expect("create wrapper fixture");
    fs::copy(workspace_root().join("scripts/vesper"), &wrapper)
        .expect("copy Vesper wrapper into bootstrap fixture");
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700))
        .expect("make bootstrap wrapper executable");

    let missing = Command::new(&wrapper)
        .env("VESPER_CLI", root.join("missing-vesper"))
        .args(["android", "--help"])
        .output()
        .expect("run wrapper with missing configured CLI");
    assert_eq!(missing.status.code(), Some(4));
    assert!(missing.stdout.is_empty());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("is not executable"));

    let unexpected_cli = root.join("unexpected-vesper");
    write_executable_script(&unexpected_cli, "#!/bin/sh\nexit 23\n");
    let unexpected = Command::new(&wrapper)
        .env("VESPER_CLI", &unexpected_cli)
        .args(["android", "--help"])
        .output()
        .expect("run wrapper with unexpected configured CLI status");
    assert_eq!(unexpected.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&unexpected.stderr).contains("failed with status 23"));

    let panic_cli = root.join("panic-vesper");
    write_executable_script(&panic_cli, "#!/bin/sh\nexit 101\n");
    let panic = Command::new(&wrapper)
        .env("VESPER_CLI", &panic_cli)
        .args(["android", "--help"])
        .output()
        .expect("run wrapper with Rust panic status");
    assert_eq!(panic.status.code(), Some(6));
    assert!(panic.stdout.is_empty());

    let conformance_cli = root.join("conformance-vesper");
    write_executable_script(&conformance_cli, "#!/bin/sh\nexit 5\n");
    let conformance = Command::new(&wrapper)
        .env("VESPER_CLI", &conformance_cli)
        .args(["android", "--help"])
        .output()
        .expect("run wrapper with stable configured CLI status");
    assert_eq!(conformance.status.code(), Some(5));

    let bootstrap_tools = root.join("bootstrap tools without cargo");
    fs::create_dir_all(&bootstrap_tools).expect("create bootstrap tool directory");
    write_executable_script(
        &bootstrap_tools.join("dirname"),
        "#!/bin/sh\n/usr/bin/dirname \"$@\"\n",
    );
    let no_cargo = Command::new("/bin/bash")
        .arg(&wrapper)
        .args(["android", "--help"])
        .env_remove("VESPER_CLI")
        .env("PATH", &bootstrap_tools)
        .output()
        .expect("run wrapper without Cargo");
    assert_eq!(no_cargo.status.code(), Some(4));
    assert!(no_cargo.stdout.is_empty());
    assert!(String::from_utf8_lossy(&no_cargo.stderr).contains("cargo is unavailable"));
}

#[cfg(unix)]
#[test]
fn vesper_wrapper_forwards_top_level_help_without_reinterpreting_it() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().expect("temporary Vesper wrapper help fixture");
    let root = directory.path().join("wrapper fixture");
    let wrapper = root.join("scripts/vesper");
    fs::create_dir_all(wrapper.parent().expect("wrapper parent")).expect("create wrapper fixture");
    fs::copy(workspace_root().join("scripts/vesper"), &wrapper)
        .expect("copy Vesper wrapper into help fixture");
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700))
        .expect("make help wrapper executable");

    let direct = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .arg("--help")
        .output()
        .expect("run direct Vesper help");
    let wrapped = Command::new(&wrapper)
        .env("VESPER_CLI", env!("CARGO_BIN_EXE_vesper"))
        .arg("--help")
        .output()
        .expect("run wrapped Vesper help");

    assert_eq!(wrapped.status.code(), direct.status.code());
    assert_eq!(wrapped.stdout, direct.stdout);
    assert_eq!(wrapped.stderr, direct.stderr);
}

#[cfg(unix)]
#[test]
fn android_aar_optional_build_failure_preserves_all_previous_outputs() {
    let (_directory, root) = android_cli_fixture();
    let outputs = prepare_android_optional_project_fixture(&root);
    for (index, output) in outputs.iter().enumerate() {
        fs::create_dir_all(output).expect("create previous optional Android output");
        fs::write(
            output.join("previous.txt"),
            format!("previous optional output {index}\n"),
        )
        .expect("write previous optional Android output");
    }
    write_android_cached_gradle(
        &root,
        "examples/android-compose-host",
        "#!/bin/sh\nexit 99\n",
    );
    let tools = root.join("optional Android failure tools");
    let path = prepare_android_optional_toolchain(&root, &tools);
    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", path)
        .env(
            "ANDROID_TEST_TARGET_LIBDIR",
            root.join("fake rust target/lib"),
        )
        .env("ANDROID_SDK_ROOT", root.join("Android SDK"))
        .env("ANDROID_NDK_ROOT", root.join("Android NDK"))
        .env(
            "ANDROID_TEST_CARGO_NDK_FAIL_CRATE",
            "player-source-normalizer-ffmpeg",
        )
        .env("ANDROID_TEST_CARGO_NDK_STATUS", "23")
        .args([
            "android",
            "aar",
            "assembleFixture",
            "--include-optional-plugins",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("run failing optional Android AAR build");

    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Android SourceNormalizer FFmpeg plugin build exited unsuccessfully"),
        "stderr: {stderr}"
    );
    for (index, output) in outputs.iter().enumerate() {
        assert_eq!(
            fs::read_to_string(output.join("previous.txt"))
                .expect("read preserved optional Android output"),
            format!("previous optional output {index}\n")
        );
        let retained_stages = fs::read_dir(output.parent().expect("optional output parent"))
            .expect("inspect optional output parent")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".vesper-android-")
            })
            .count();
        assert_eq!(retained_stages, 0);
    }
}

#[cfg(unix)]
#[test]
fn android_aar_gradle_failure_preserves_all_previous_optional_outputs() {
    let (_directory, root) = android_cli_fixture();
    let outputs = prepare_android_optional_project_fixture(&root);
    let host_output = root.join("lib/android/vesper-player-kit/src/main/jniLibs");
    fs::create_dir_all(&host_output).expect("create previous Android host JNI output");
    fs::write(host_output.join("previous.txt"), "previous host output\n")
        .expect("write previous Android host JNI output");
    for (index, output) in outputs.iter().enumerate() {
        fs::create_dir_all(output).expect("create previous optional Android output");
        fs::write(
            output.join("previous.txt"),
            format!("previous optional output {index}\n"),
        )
        .expect("write previous optional Android output");
    }
    let nested_jni_completed = root.join("nested-jni-completed.txt");
    write_android_cached_gradle(
        &root,
        "examples/android-compose-host",
        r#"#!/bin/sh
"$VESPER_CLI" android jni release arm64-v8a --root "$ANDROID_NESTED_JNI_ROOT" >/dev/null || exit 91
host_jni="${VESPER_ANDROID_HOST_JNI_LIBS:-$ANDROID_NESTED_JNI_ROOT/lib/android/vesper-player-kit/src/main/jniLibs}"
test -f "$host_jni/arm64-v8a/libvesper_player_android.so" || exit 92
printf 'completed\n' > "$ANDROID_NESTED_JNI_COMPLETED"
exit 23
"#,
    );
    let tools = root.join("optional Android Gradle failure tools");
    let path = prepare_android_optional_toolchain(&root, &tools);

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", path)
        .env(
            "ANDROID_TEST_TARGET_LIBDIR",
            root.join("fake rust target/lib"),
        )
        .env("ANDROID_SDK_ROOT", root.join("Android SDK"))
        .env("ANDROID_NDK_ROOT", root.join("Android NDK"))
        .env("ANDROID_NESTED_JNI_ROOT", &root)
        .env("ANDROID_NESTED_JNI_COMPLETED", &nested_jni_completed)
        .env_remove("CI")
        .args([
            "android",
            "aar",
            "assembleFixture",
            "--include-optional-plugins",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("run optional Android AAR with failing Gradle");

    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Android AAR Gradle build exited unsuccessfully"),
        "stderr: {stderr}"
    );
    assert!(nested_jni_completed.is_file());
    assert_eq!(
        fs::read_to_string(host_output.join("previous.txt"))
            .expect("read host JNI output preserved after Gradle failure"),
        "previous host output\n"
    );
    assert_eq!(
        fs::read_dir(
            host_output
                .parent()
                .expect("Android host JNI output parent")
        )
        .expect("inspect host JNI staging cleanup after Gradle failure")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".vesper-android-host-jni-stage-")
        })
        .count(),
        0
    );
    for (index, output) in outputs.iter().enumerate() {
        assert_eq!(
            fs::read_to_string(output.join("previous.txt"))
                .expect("read optional output preserved after Gradle failure"),
            format!("previous optional output {index}\n")
        );
        let retained_stages = fs::read_dir(output.parent().expect("optional output parent"))
            .expect("inspect optional output parent")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".vesper-android-")
            })
            .count();
        assert_eq!(retained_stages, 0);
    }
}

#[cfg(unix)]
#[test]
fn android_aar_holds_the_jni_lock_until_staged_host_output_is_published() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let (_directory, root) = android_cli_fixture();
    let host_output = root.join("lib/android/vesper-player-kit/src/main/jniLibs");
    fs::create_dir_all(&host_output).expect("create previous Android host JNI output");
    fs::write(host_output.join("previous.txt"), "previous host output\n")
        .expect("write previous Android host JNI output");

    let tools = root.join("Android AAR JNI lock tools");
    fs::create_dir_all(&tools).expect("create Android AAR JNI lock tools");
    let target_libdir = root.join("fake rust target/lib");
    fs::create_dir_all(&target_libdir).expect("create fake Rust target library directory");
    write_executable_script(
        &tools.join("rustc"),
        "#!/bin/sh\nprintf '%s\\n' \"$ANDROID_TEST_TARGET_LIBDIR\"\n",
    );
    write_executable_script(&tools.join("cargo"), "#!/bin/sh\nexit 0\n");
    write_executable_script(
        &tools.join("cargo-ndk"),
        r#"#!/bin/sh
output=
profile=debug
while [ "$#" -gt 0 ]; do
  if [ "$1" = '-o' ]; then
    shift
    output="$1"
  elif [ "$1" = '--release' ]; then
    profile=release
  fi
  shift
done
test -n "$output" || exit 93
/bin/mkdir -p "$output/arm64-v8a"
printf '%s host\n' "$profile" > "$output/arm64-v8a/libvesper_player_android.so"
"#,
    );

    let ready = root.join("Android AAR JNI ready");
    let release = root.join("Android AAR JNI release");
    write_android_cached_gradle(
        &root,
        "lib/android",
        r#"#!/bin/sh
"$VESPER_CLI" android jni release arm64-v8a --root "$ANDROID_NESTED_JNI_ROOT" || exit 91
host_jni="${VESPER_ANDROID_HOST_JNI_LIBS:-$ANDROID_NESTED_JNI_ROOT/lib/android/vesper-player-kit/src/main/jniLibs}"
test -f "$host_jni/arm64-v8a/libvesper_player_android.so" || exit 92
printf 'ready\n' > "$ANDROID_AAR_JNI_READY"
while [ ! -f "$ANDROID_AAR_JNI_RELEASE" ]; do /bin/sleep 0.02; done
"#,
    );
    let sdk = root.join("Android SDK");
    let ndk = root.join("Android NDK");
    fs::create_dir_all(&sdk).expect("create Android SDK fixture");
    fs::create_dir_all(&ndk).expect("create Android NDK fixture");
    let path = std::env::join_paths([
        tools.as_path(),
        PathBuf::from("/usr/bin").as_path(),
        PathBuf::from("/bin").as_path(),
    ])
    .expect("compose Android AAR JNI lock PATH");

    let mut aar = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", &path)
        .env("ANDROID_TEST_TARGET_LIBDIR", &target_libdir)
        .env("ANDROID_SDK_ROOT", &sdk)
        .env("ANDROID_NDK_ROOT", &ndk)
        .env("ANDROID_NESTED_JNI_ROOT", &root)
        .env("ANDROID_AAR_JNI_READY", &ready)
        .env("ANDROID_AAR_JNI_RELEASE", &release)
        .env_remove("CI")
        .args([
            "android",
            "aar",
            "assembleFixture",
            "--include-optional-plugins=false",
            "--root",
        ])
        .arg(&root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start Android AAR JNI lock fixture");
    let ready_deadline = Instant::now() + Duration::from_secs(15);
    while !ready.is_file() && Instant::now() < ready_deadline {
        if aar
            .try_wait()
            .expect("poll Android AAR JNI lock fixture")
            .is_some()
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let direct = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", &path)
        .env("ANDROID_TEST_TARGET_LIBDIR", &target_libdir)
        .env("ANDROID_SDK_ROOT", &sdk)
        .env("ANDROID_NDK_ROOT", &ndk)
        .args(["android", "jni", "debug", "arm64-v8a", "--root"])
        .arg(&root)
        .output()
        .expect("attempt concurrent direct Android JNI build");
    fs::write(&release, "release\n").expect("release Android AAR Gradle fixture");
    let aar_output = aar
        .wait_with_output()
        .expect("wait for Android AAR JNI lock fixture");

    assert!(
        ready.is_file(),
        "stderr: {}",
        String::from_utf8_lossy(&aar_output.stderr)
    );
    assert_eq!(direct.status.code(), Some(4));
    assert!(direct.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&direct.stderr)
            .contains("another Android jni build is already active")
    );
    assert_eq!(
        aar_output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&aar_output.stderr)
    );
    assert_eq!(
        fs::read_to_string(host_output.join("arm64-v8a/libvesper_player_android.so"))
            .expect("read published Android host JNI output"),
        "release host\n"
    );
    assert!(!host_output.join("previous.txt").exists());
}

#[cfg(unix)]
#[test]
fn android_aar_gradle_cancellation_preserves_all_previous_optional_outputs() {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let (_directory, root) = android_cli_fixture();
    let outputs = prepare_android_optional_project_fixture(&root);
    for (index, output) in outputs.iter().enumerate() {
        fs::create_dir_all(output).expect("create previous optional Android output");
        fs::write(
            output.join("previous.txt"),
            format!("previous optional output {index}\n"),
        )
        .expect("write previous optional Android output");
    }
    let ready = root.join("optional-gradle.ready");
    write_android_cached_gradle(
        &root,
        "examples/android-compose-host",
        r#"#!/bin/sh
trap 'exit 130' INT TERM
printf 'ready\n' > "$ANDROID_OPTIONAL_GRADLE_READY"
while :; do /bin/sleep 1; done
"#,
    );
    let tools = root.join("optional Android cancellation tools");
    let path = prepare_android_optional_toolchain(&root, &tools);

    let mut process = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", path)
        .env(
            "ANDROID_TEST_TARGET_LIBDIR",
            root.join("fake rust target/lib"),
        )
        .env("ANDROID_SDK_ROOT", root.join("Android SDK"))
        .env("ANDROID_NDK_ROOT", root.join("Android NDK"))
        .env("ANDROID_OPTIONAL_GRADLE_READY", &ready)
        .env_remove("CI")
        .args([
            "android",
            "aar",
            "assembleFixture",
            "--include-optional-plugins",
            "--root",
        ])
        .arg(&root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start optional Android AAR cancellation fixture");
    let parent_pid = i32::try_from(process.id()).expect("optional Android Vesper pid");
    let ready_deadline = Instant::now() + Duration::from_secs(30);
    while !ready.is_file() && Instant::now() < ready_deadline {
        if process
            .try_wait()
            .expect("poll optional Android AAR cancellation fixture")
            .is_some()
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !ready.is_file() {
        let _ = kill(Pid::from_raw(parent_pid), Signal::SIGINT);
        let output = process
            .wait_with_output()
            .expect("wait after optional Android readiness failure");
        panic!(
            "optional Android Gradle build was not ready; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    kill(Pid::from_raw(parent_pid), Signal::SIGINT)
        .expect("cancel optional Android AAR Gradle build");
    let output = process
        .wait_with_output()
        .expect("wait for cancelled optional Android AAR Gradle build");
    assert_eq!(output.status.code(), Some(6));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Android AAR Gradle build was cancelled")
    );
    for (index, output) in outputs.iter().enumerate() {
        assert_eq!(
            fs::read_to_string(output.join("previous.txt"))
                .expect("read optional output preserved after Gradle cancellation"),
            format!("previous optional output {index}\n")
        );
    }
}

#[cfg(unix)]
#[test]
fn android_aar_optional_builds_stage_outputs_and_preserve_command_order() {
    let (_directory, root) = android_cli_fixture();
    let outputs = prepare_android_optional_project_fixture(&root);
    for output in &outputs {
        fs::create_dir_all(output).expect("create stale optional Android output");
        fs::write(output.join("stale.txt"), b"stale\n")
            .expect("write stale optional Android output");
    }
    let log_path = root.join("optional-android-build.log");
    let tools = root.join("optional Android tools");
    let path = prepare_android_optional_toolchain(&root, &tools);
    write_android_cached_gradle(
        &root,
        "examples/android-compose-host",
        r#"#!/bin/sh
/bin/mkdir -p "$VESPER_ANDROID_HOST_JNI_LIBS/arm64-v8a"
printf 'fixture host\n' > "$VESPER_ANDROID_HOST_JNI_LIBS/arm64-v8a/libvesper_player_android.so"
test -f "$VESPER_ANDROID_DECODER_JNI_LIBS/arm64-v8a/libvesper_decoder_mediacodec.so" || exit 81
test -f "$VESPER_ANDROID_SOURCE_NORMALIZER_JNI_LIBS/arm64-v8a/libvesper_source_normalizer_ffmpeg.so" || exit 82
test -f "$VESPER_ANDROID_SOURCE_NORMALIZER_ASSETS/vesper-source-normalizer-ffmpeg/profile-hash.txt" || exit 83
test -f "$VESPER_ANDROID_SOURCE_NORMALIZER_ASSETS/vesper-source-normalizer-ffmpeg/source-normalizer-profile.txt" || exit 84
test -f "$VESPER_ANDROID_SOURCE_NORMALIZER_ASSETS/vesper-source-normalizer-ffmpeg/arm64-v8a-vesper-ffmpeg-build-metadata.txt" || exit 85
test -f "$VESPER_ANDROID_FRAME_PROCESSOR_JNI_LIBS/arm64-v8a/libvesper_frame_processor_diagnostic.so" || exit 86
/bin/mkdir -p "$VESPER_ANDROID_EXTERNAL_RELAY_JNI_LIBS/arm64-v8a"
printf 'relay\n' > "$VESPER_ANDROID_EXTERNAL_RELAY_JNI_LIBS/arm64-v8a/libvesper_player_relay_ffmpeg.so"
/bin/mkdir -p "$VESPER_ANDROID_EXTERNAL_RELAY_ASSETS/vesper-relay-ffmpeg"
cat "$VESPER_ANDROID_SOURCE_NORMALIZER_ASSETS/vesper-source-normalizer-ffmpeg/profile-hash.txt" > "$VESPER_ANDROID_EXTERNAL_RELAY_ASSETS/vesper-relay-ffmpeg/profile-hash.txt"
{
  printf 'begin=gradle\n'
  printf 'cwd=%s\n' "$PWD"
  printf 'gradle_user_home=%s\n' "$GRADLE_USER_HOME"
  printf 'vesper_cli=%s\n' "$VESPER_CLI"
  printf 'host_jni=%s\n' "$VESPER_ANDROID_HOST_JNI_LIBS"
  printf 'decoder_jni=%s\n' "$VESPER_ANDROID_DECODER_JNI_LIBS"
  printf 'source_jni=%s\n' "$VESPER_ANDROID_SOURCE_NORMALIZER_JNI_LIBS"
  printf 'source_assets=%s\n' "$VESPER_ANDROID_SOURCE_NORMALIZER_ASSETS"
  printf 'frame_jni=%s\n' "$VESPER_ANDROID_FRAME_PROCESSOR_JNI_LIBS"
  printf 'runtime_jni=%s\n' "$VESPER_ANDROID_FFMPEG_RUNTIME_JNI_LIBS"
  printf 'runtime_assets=%s\n' "$VESPER_ANDROID_FFMPEG_RUNTIME_ASSETS"
  printf 'relay_jni=%s\n' "$VESPER_ANDROID_EXTERNAL_RELAY_JNI_LIBS"
  printf 'relay_assets=%s\n' "$VESPER_ANDROID_EXTERNAL_RELAY_ASSETS"
  for argument in "$@"; do
    printf 'arg=%s\n' "$argument"
  done
  printf 'end=gradle\n'
} >> "$ANDROID_OPTIONAL_LOG"
"#,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", path)
        .env("ANDROID_OPTIONAL_LOG", &log_path)
        .env(
            "ANDROID_TEST_TARGET_LIBDIR",
            root.join("fake rust target/lib"),
        )
        .env("ANDROID_SDK_ROOT", root.join("Android SDK"))
        .env("ANDROID_NDK_ROOT", root.join("Android NDK"))
        .env("VESPER_ANDROID_INCLUDE_OPTIONAL_PLUGINS", "0")
        .env_remove("CI")
        .env_remove("GRADLE_USER_HOME")
        .args([
            "android",
            "aar",
            "assembleFixture",
            "--include-optional-plugins=true",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("build optional Android AAR fixture");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    for output in &outputs {
        assert!(!output.join("stale.txt").exists());
    }
    assert_eq!(
        fs::read_to_string(outputs[0].join("arm64-v8a/libvesper_decoder_mediacodec.so"))
            .expect("read staged decoder output"),
        "player-decoder-mediacodec\n"
    );
    assert_eq!(
        fs::read_to_string(outputs[1].join("arm64-v8a/libvesper_source_normalizer_ffmpeg.so"))
            .expect("read staged source normalizer output"),
        "player-source-normalizer-ffmpeg\n"
    );
    let profile_hash = fs::read_to_string(outputs[2].join("profile-hash.txt"))
        .expect("read staged source normalizer metadata");
    assert!(!profile_hash.trim().is_empty());
    assert_eq!(
        fs::read_to_string(outputs[3].join("arm64-v8a/libvesper_frame_processor_diagnostic.so"))
            .expect("read staged frame processor output"),
        "player-frame-processor-diagnostic\n"
    );
    assert_eq!(
        fs::read_to_string(outputs[4].join("arm64-v8a/libavcodec.so"))
            .expect("read staged FFmpeg runtime output"),
        "fixture ffmpeg runtime avcodec\n"
    );
    assert_eq!(
        fs::read_to_string(outputs[5].join("profile-hash.txt"))
            .expect("read staged FFmpeg runtime metadata"),
        profile_hash
    );
    assert_eq!(
        fs::read_to_string(outputs[6].join("arm64-v8a/libvesper_player_relay_ffmpeg.so"))
            .expect("read staged relay output"),
        "relay\n"
    );
    assert_eq!(
        fs::read_to_string(outputs[7].join("profile-hash.txt"))
            .expect("read staged relay metadata"),
        profile_hash
    );

    let log = fs::read_to_string(log_path).expect("read optional Android build log");
    let canonical_root = root
        .canonicalize()
        .expect("resolve optional Android fixture root");
    let gradle_runs = log.matches("begin=gradle\n").count();
    let expected_gradle_home = format!(
        "gradle_user_home={}\n",
        canonical_root
            .join("lib/android/.gradle/gradle-user-home")
            .display()
    );
    assert!(gradle_runs > 0, "log:\n{log}");
    assert_eq!(log.matches(&expected_gradle_home).count(), gradle_runs);
    assert!(!canonical_root.join(".gradle").exists());
    let ffmpeg_native = log
        .find("ffmpeg-native version=8.1.2")
        .expect("native FFmpeg build log");
    let decoder = log
        .find("cargo-ndk crate=player-decoder-mediacodec")
        .expect("decoder plugin log");
    let source = log
        .find("cargo-ndk crate=player-source-normalizer-ffmpeg")
        .expect("source normalizer plugin log");
    let frame = log
        .find("cargo-ndk crate=player-frame-processor-diagnostic")
        .expect("frame processor plugin log");
    let relay = log
        .find("cargo-ndk crate=player-relay-ffmpeg-android")
        .expect("external playback relay log");
    let gradle = log.find("begin=gradle\n").expect("Gradle log");
    assert!(
        ffmpeg_native < decoder
            && decoder < source
            && source < frame
            && frame < relay
            && relay < gradle
    );
    assert!(log.contains("runtime_jni=") && log.contains("runtime_assets="));
    assert!(log.contains("relay_jni=") && log.contains("relay_assets="));
    assert!(log.contains("arg=:vesper-player-kit:assembleFixture\n"));
    assert!(log.contains("arg=:vesper-player-kit-external-playback:assembleFixture\n"));
    assert_eq!(
        fs::read_to_string(root.join(
            "lib/android/vesper-player-kit/src/main/jniLibs/arm64-v8a/libvesper_player_android.so",
        ),)
        .expect("read staged Android host JNI output"),
        "fixture host\n"
    );
}

#[cfg(unix)]
#[test]
fn android_stage_release_verifies_the_complete_optional_archive_set() {
    let (_directory, root) = android_cli_fixture();
    let output_directory = root.join("dist/release/android");
    let _artifacts = prepare_android_stage_release_fixture(&root);
    let frame_processor_manifest =
        fs::read_to_string(root.join("plugins/frame-processor-diagnostic/vesper-plugin.toml"))
            .expect("read Android FrameProcessor project manifest fixture");
    assert!(frame_processor_manifest.contains("[[artifacts]]"));
    let tools = root.join("Android stage release tools");
    let path = prepare_android_optional_toolchain(&root, &tools);
    write_android_cached_gradle(
        &root,
        "lib/android",
        r#"#!/bin/sh
set -eu
/bin/mkdir -p "$VESPER_ANDROID_HOST_JNI_LIBS/arm64-v8a"
printf 'fixture host\n' > "$VESPER_ANDROID_HOST_JNI_LIBS/arm64-v8a/libvesper_player_android.so"
exit "${ANDROID_STAGE_RELEASE_GRADLE_STATUS:-0}"
"#,
    );

    let result = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", path)
        .env(
            "ANDROID_TEST_TARGET_LIBDIR",
            root.join("fake rust target/lib"),
        )
        .env("ANDROID_SDK_ROOT", root.join("Android SDK"))
        .env("ANDROID_NDK_ROOT", root.join("Android NDK"))
        .env_remove("CI")
        .args(["android", "stage-release"])
        .arg(&output_directory)
        .args(["--include-optional-plugins", "--root"])
        .arg(&root)
        .output()
        .expect("stage the complete optional Android release");

    assert_eq!(
        result.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stderr.is_empty());
    let mut names = fs::read_dir(&output_directory)
        .expect("read complete Android release output")
        .map(|entry| {
            entry
                .expect("read Android release entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        vec![
            "VesperPlayerKit-android-arm64-v8a.aar",
            "VesperPlayerKitCompose-android-arm64-v8a.aar",
            "VesperPlayerKitComposeUi-android-arm64-v8a.aar",
            "VesperPlayerKitDecoderMediaCodec-android-arm64-v8a.aar",
            "VesperPlayerKitExternalPlayback-android-arm64-v8a.aar",
            "VesperPlayerKitFfmpegRuntime-android-arm64-v8a.aar",
            "VesperPlayerKitFrameProcessorDiagnostic-android-arm64-v8a.aar",
            "VesperPlayerKitSourceNormalizerFfmpeg-android-arm64-v8a.aar",
        ]
    );
}

#[cfg(unix)]
#[test]
fn android_stage_release_preserves_previous_outputs_when_gradle_fails() {
    let (_directory, root) = android_cli_fixture();
    let output_directory = root.join("dist/release/android");
    fs::create_dir_all(&output_directory).expect("create previous Android release output");
    fs::write(output_directory.join("previous.txt"), b"previous release\n")
        .expect("write previous Android release marker");
    let _artifacts = prepare_android_stage_release_fixture(&root);
    let tools = root.join("Android stage release rollback tools");
    let path = prepare_android_optional_toolchain(&root, &tools);
    write_android_cached_gradle(
        &root,
        "lib/android",
        r#"#!/bin/sh
set -eu
/bin/mkdir -p "$VESPER_ANDROID_HOST_JNI_LIBS/arm64-v8a"
printf 'replacement host\n' > "$VESPER_ANDROID_HOST_JNI_LIBS/arm64-v8a/libvesper_player_android.so"
exit 23
"#,
    );

    let result = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", path)
        .env(
            "ANDROID_TEST_TARGET_LIBDIR",
            root.join("fake rust target/lib"),
        )
        .env("ANDROID_SDK_ROOT", root.join("Android SDK"))
        .env("ANDROID_NDK_ROOT", root.join("Android NDK"))
        .env_remove("CI")
        .args(["android", "stage-release"])
        .arg(&output_directory)
        .args(["--include-optional-plugins", "--root"])
        .arg(&root)
        .output()
        .expect("run failing Android release staging");

    assert_eq!(result.status.code(), Some(5));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Android AAR Gradle build"));
    assert_eq!(
        fs::read(output_directory.join("previous.txt")).expect("read preserved Android marker"),
        b"previous release\n"
    );
    assert_eq!(
        fs::read_dir(&output_directory)
            .expect("inspect preserved Android release output")
            .count(),
        1
    );
}

#[cfg(unix)]
#[test]
fn mobile_no_remux_scans_aars_without_extracting_untrusted_entries() {
    let directory = tempfile::tempdir().expect("temporary mobile no-remux fixture");
    let root = directory.path();
    fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("write mobile manifest");
    prepare_android_project_fixture(root);
    write_android_cached_gradle(
        root,
        "lib/android",
        r#"#!/bin/sh
/bin/mkdir -p "$VESPER_ANDROID_HOST_JNI_LIBS/arm64-v8a"
printf 'fixture host\n' > "$VESPER_ANDROID_HOST_JNI_LIBS/arm64-v8a/libvesper_player_android.so"
"#,
    );
    let kit =
        root.join("lib/android/vesper-player-kit/build/outputs/aar/vesper-player-kit-release.aar");
    let compose = root.join(
        "lib/android/vesper-player-kit-compose/build/outputs/aar/vesper-player-kit-compose-release.aar",
    );
    write_test_zip(
        &kit,
        &[("jni/arm64-v8a/libvesper_player_android.so", b"host library")],
    );
    write_test_zip(&compose, &[("classes.jar", b"compose classes")]);

    let success = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["mobile", "verify-no-remux", "android", "--root"])
        .arg(root)
        .output()
        .expect("verify clean Android AARs");
    assert_eq!(
        success.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&success.stderr)
    );
    assert!(success.stderr.is_empty());
    let stdout = String::from_utf8(success.stdout).expect("UTF-8 Android verification output");
    assert_eq!(stdout.lines().count(), 2);
    let canonical_kit = fs::canonicalize(&kit).expect("canonical kit path");
    let canonical_compose = fs::canonicalize(&compose).expect("canonical compose path");
    assert!(stdout.contains(&format!(
        "Verified Android host artifact without FFmpeg payload: {} ({} bytes)",
        canonical_kit.display(),
        fs::metadata(&kit).expect("kit metadata").len()
    )));
    assert!(stdout.contains(&format!(
        "Verified Android host artifact without FFmpeg payload: {} ({} bytes)",
        canonical_compose.display(),
        fs::metadata(&compose).expect("compose metadata").len()
    )));

    write_test_zip(
        &compose,
        &[("jni/arm64-v8a/libavcodec.so.61", b"unexpected")],
    );
    let payload = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["mobile", "verify-no-remux", "android", "--root"])
        .arg(root)
        .output()
        .expect("reject FFmpeg Android payload");
    assert_eq!(payload.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&payload.stderr).contains("libavcodec.so.61"));

    write_test_zip(&compose, &[("../libavcodec.so", b"unsafe path")]);
    let traversal = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["mobile", "verify-no-remux", "android", "--root"])
        .arg(root)
        .output()
        .expect("reject Android AAR traversal");
    assert_eq!(traversal.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&traversal.stderr).contains("unsafe path"));
}

#[cfg(unix)]
#[test]
fn mobile_no_remux_ctrl_c_reaps_the_build_process_group() {
    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    use std::time::{Duration, Instant};

    let directory = tempfile::tempdir().expect("temporary mobile cancellation fixture");
    let root = directory.path();
    fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("write mobile manifest");
    prepare_android_project_fixture(root);
    write_android_cached_gradle(
        root,
        "lib/android",
        r#"#!/bin/sh
printf '%s' "$$" > "$MOBILE_BUILD_PID_FILE"
sh -c 'trap "" INT TERM; printf "%s" "$$" > "$1"; while :; do sleep 1; done' sh "$MOBILE_BUILD_DESCENDANT_PID_FILE" &
trap '' INT TERM
while :; do sleep 1; done
"#,
    );
    let build_pid_path = root.join("build.pid");
    let descendant_pid_path = root.join("descendant.pid");
    let mut process = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("MOBILE_BUILD_PID_FILE", &build_pid_path)
        .env("MOBILE_BUILD_DESCENDANT_PID_FILE", &descendant_pid_path)
        .args(["mobile", "verify-no-remux", "android", "--root"])
        .arg(root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("start mobile cancellation verification");
    let parent_pid = i32::try_from(process.id()).expect("vesper pid");
    let ready_deadline = Instant::now() + Duration::from_secs(15);
    while (!build_pid_path.is_file() || !descendant_pid_path.is_file())
        && Instant::now() < ready_deadline
    {
        if process.try_wait().expect("poll vesper process").is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !build_pid_path.is_file() || !descendant_pid_path.is_file() {
        let _ = kill(Pid::from_raw(parent_pid), Signal::SIGINT);
        let output = process
            .wait_with_output()
            .expect("wait after mobile build readiness failure");
        panic!(
            "mobile build process group was not ready; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let build_pid: i32 = fs::read_to_string(&build_pid_path)
        .expect("read build pid")
        .parse()
        .expect("parse build pid");
    let descendant_pid: i32 = fs::read_to_string(&descendant_pid_path)
        .expect("read build descendant pid")
        .parse()
        .expect("parse build descendant pid");

    kill(Pid::from_raw(parent_pid), Signal::SIGINT).expect("cancel mobile verification");
    let output = process
        .wait_with_output()
        .expect("wait for cancelled mobile verification");
    assert_eq!(output.status.code(), Some(6));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Android AAR Gradle build was cancelled")
    );

    for pid in [build_pid, descendant_pid] {
        let reap_deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match kill(Pid::from_raw(pid), None) {
                Err(Errno::ESRCH) => break,
                _ if Instant::now() < reap_deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                result => panic!("mobile build process {pid} is still alive: {result:?}"),
            }
        }
    }
}

#[test]
fn flutter_local_overrides_preserves_order_paths_and_optional_exclusion() {
    let directory = flutter_fixture(true);
    let root = flutter_fixture_root(&directory);
    let optional_output = root
        .join("lib/flutter")
        .join(FLUTTER_OPTIONAL_PACKAGE)
        .join("pubspec_overrides.yaml");
    fs::write(&optional_output, b"optional sentinel\n").expect("seed optional override");
    let first_output = root
        .join("lib/flutter")
        .join(FLUTTER_CORE_PACKAGES[0])
        .join("pubspec_overrides.yaml");
    fs::write(&first_output, b"stale\n").expect("seed first override");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(&first_output, fs::Permissions::from_mode(0o640))
            .expect("set first override permissions");
    }

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args([
            "flutter",
            "local-overrides",
            "--include-optional-plugins=false",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("write default Flutter overrides");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let mut expected_stdout = String::from("Wrote local Flutter pubspec_overrides.yaml files.\n");
    for package in FLUTTER_CORE_PACKAGES {
        expected_stdout.push_str(&format!("  lib/flutter/{package}/pubspec_overrides.yaml\n"));
        let actual = fs::read_to_string(
            root.join("lib/flutter")
                .join(package)
                .join("pubspec_overrides.yaml"),
        )
        .expect("read generated package override");
        assert_eq!(
            actual,
            expected_package_overrides(package, &FLUTTER_CORE_PACKAGES)
        );
    }
    expected_stdout.push_str("  examples/flutter-host/pubspec_overrides.yaml\n");
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 stdout"),
        expected_stdout
    );
    assert_eq!(
        fs::read_to_string(root.join("examples/flutter-host/pubspec_overrides.yaml"))
            .expect("read example override"),
        expected_example_overrides(&FLUTTER_CORE_PACKAGES)
    );
    assert_eq!(
        fs::read_to_string(optional_output).expect("read untouched optional override"),
        "optional sentinel\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        assert_eq!(
            fs::metadata(first_output)
                .expect("first override metadata")
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
    }
}

#[test]
fn flutter_local_overrides_includes_optional_package_from_cli_or_environment() {
    let directory = flutter_fixture(true);
    let root = flutter_fixture_root(&directory);
    let packages: Vec<_> = FLUTTER_CORE_PACKAGES
        .iter()
        .copied()
        .chain(std::iter::once(FLUTTER_OPTIONAL_PACKAGE))
        .collect();

    let cli_output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args([
            "flutter",
            "local-overrides",
            "--include-optional-plugins",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("write optional Flutter overrides from CLI");
    assert_eq!(cli_output.status.code(), Some(0));
    assert!(cli_output.stderr.is_empty());
    let optional_output = root
        .join("lib/flutter")
        .join(FLUTTER_OPTIONAL_PACKAGE)
        .join("pubspec_overrides.yaml");
    assert_eq!(
        fs::read_to_string(&optional_output).expect("read optional package override"),
        expected_package_overrides(FLUTTER_OPTIONAL_PACKAGE, &packages)
    );
    assert_eq!(
        fs::read_to_string(root.join("examples/flutter-host/pubspec_overrides.yaml"))
            .expect("read optional example override"),
        expected_example_overrides(&packages)
    );

    fs::remove_file(&optional_output).expect("remove optional override before environment run");
    let environment_output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("VESPER_FLUTTER_INCLUDE_OPTIONAL_PLUGINS", "YES")
        .args(["flutter", "local-overrides", "--root"])
        .arg(&root)
        .output()
        .expect("write optional Flutter overrides from environment");
    assert_eq!(environment_output.status.code(), Some(0));
    assert!(optional_output.is_file());
}

#[cfg(unix)]
#[test]
fn vesper_wrapper_resolves_flutter_root_from_its_own_location() {
    use std::os::unix::fs::PermissionsExt;

    let directory = flutter_fixture(true);
    let root = flutter_fixture_root(&directory);
    prepare_flutter_pub_fixture(&root);
    let wrapper = root.join("scripts/vesper");
    fs::create_dir_all(wrapper.parent().expect("wrapper parent"))
        .expect("create fixture scripts directory");
    fs::copy(workspace_root().join("scripts/vesper"), &wrapper)
        .expect("copy Vesper wrapper into fixture");
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700))
        .expect("make fixture wrapper executable");

    let outside_repository = tempfile::tempdir().expect("outside repository directory");
    let output = Command::new(&wrapper)
        .current_dir(outside_repository.path())
        .env("VESPER_CLI", env!("CARGO_BIN_EXE_vesper"))
        .args([
            "flutter",
            "local-overrides",
            "--include-optional-plugins=false",
        ])
        .output()
        .expect("run Vesper wrapper outside its repository");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert!(
        root.join("lib/flutter/vesper_player/pubspec_overrides.yaml")
            .is_file()
    );
    assert!(
        !outside_repository
            .path()
            .join("lib/flutter/vesper_player/pubspec_overrides.yaml")
            .exists()
    );

    let group_option_output = Command::new(&wrapper)
        .current_dir(outside_repository.path())
        .env("VESPER_CLI", env!("CARGO_BIN_EXE_vesper"))
        .args(["flutter", "--root"])
        .arg(&root)
        .args(["local-overrides", "--include-optional-plugins=false"])
        .output()
        .expect("route a Flutter command after its group-level root option");
    assert_eq!(
        group_option_output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&group_option_output.stderr)
    );
    assert!(group_option_output.stderr.is_empty());

    let stage = directory.path().join("wrapper Flutter pub stage");
    let stage_output = Command::new(&wrapper)
        .current_dir(outside_repository.path())
        .env("VESPER_CLI", env!("CARGO_BIN_EXE_vesper"))
        .args(["flutter", "--root"])
        .arg(&root)
        .args(["stage-pub"])
        .arg(&stage)
        .args(["0.4.0", "--include-optional-plugins=false"])
        .output()
        .expect("route Flutter pub staging through the Rust CLI");
    assert_eq!(
        stage_output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&stage_output.stderr)
    );
    assert!(stage.join(FLUTTER_CORE_PACKAGES[0]).is_dir());
}

#[test]
fn flutter_stage_pub_rewrites_release_metadata_and_excludes_local_artifacts() {
    let directory = flutter_fixture(true);
    let root = flutter_fixture_root(&directory);
    prepare_flutter_pub_fixture(&root);
    let first_package = root.join("lib/flutter").join(FLUTTER_CORE_PACKAGES[0]);
    fs::create_dir_all(first_package.join(".dart_tool")).expect("create excluded Dart directory");
    fs::write(first_package.join(".dart_tool/state"), "local\n")
        .expect("write excluded Dart state");
    fs::write(first_package.join("pubspec.lock"), "local lock\n").expect("write excluded pub lock");
    fs::create_dir_all(first_package.join("ios/Flutter/ephemeral"))
        .expect("create excluded Flutter ephemeral directory");
    fs::write(
        first_package.join("ios/Flutter/ephemeral/generated"),
        "local\n",
    )
    .expect("write excluded Flutter ephemeral file");
    fs::create_dir_all(first_package.join("ios/project.xcworkspace"))
        .expect("create excluded Xcode workspace");

    let stage = directory.path().join("Flutter pub stage - 发布");
    fs::create_dir_all(&stage).expect("seed existing Flutter pub stage");
    fs::write(stage.join("stale.txt"), "stale\n").expect("seed stale Flutter stage file");
    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["flutter", "stage-pub"])
        .arg(&stage)
        .args(["1.2.3-beta.1", "--include-optional-plugins=false", "--root"])
        .arg(&root)
        .output()
        .expect("stage Flutter pub packages");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stage output");
    let mut expected_stdout = format!("Staged Flutter pub packages into:\n  {}\n", stage.display());
    for package in FLUTTER_CORE_PACKAGES {
        expected_stdout.push_str(&format!("  {package}\n"));
    }
    expected_stdout.push_str(
        "Skipped optional Flutter plugin packages. Set VESPER_FLUTTER_INCLUDE_OPTIONAL_PLUGINS=1 to stage them.\n",
    );
    assert_eq!(stdout, expected_stdout);
    assert!(!stage.join("stale.txt").exists());
    assert!(!stage.join(FLUTTER_OPTIONAL_PACKAGE).exists());

    let staged_first = stage.join(FLUTTER_CORE_PACKAGES[0]);
    let pubspec =
        fs::read_to_string(staged_first.join("pubspec.yaml")).expect("read staged Flutter pubspec");
    assert!(pubspec.contains("version: 1.2.3-beta.1\n"));
    assert!(pubspec.contains("homepage: https://github.com/umbrella22/Vesper\n"));
    assert!(pubspec.contains("repository: https://github.com/umbrella22/Vesper\n"));
    assert!(pubspec.contains("issue_tracker: https://github.com/umbrella22/Vesper/issues\n"));
    assert!(pubspec.contains("  vesper_player_platform_interface: ^1.2.3-beta.1\n"));
    assert!(!pubspec.contains("publish_to:"));
    assert_eq!(
        fs::read_to_string(staged_first.join("LICENSE")).expect("read staged license"),
        "fixture license\n"
    );
    assert!(staged_first.join("lib/src/fixture.dart").is_file());
    assert!(!staged_first.join(".dart_tool").exists());
    assert!(!staged_first.join("pubspec.lock").exists());
    assert!(!staged_first.join("ios/Flutter/ephemeral").exists());
    assert!(!staged_first.join("ios/project.xcworkspace").exists());
}

#[test]
fn flutter_stage_pub_preflight_failure_preserves_existing_output() {
    let directory = flutter_fixture(true);
    let root = flutter_fixture_root(&directory);
    prepare_flutter_pub_fixture(&root);
    let leaked = root
        .join("lib/flutter")
        .join(FLUTTER_CORE_PACKAGES[0])
        .join("xcode-derived");
    fs::create_dir_all(&leaked).expect("create generated Xcode directory");
    fs::write(leaked.join("cache.bin"), "generated\n").expect("write generated Xcode file");
    let stage = directory.path().join("preserved Flutter stage");
    fs::create_dir_all(&stage).expect("create existing Flutter stage");
    fs::write(stage.join("sentinel.txt"), "preserved\n").expect("seed stage sentinel");

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["flutter", "stage-pub"])
        .arg(&stage)
        .args(["0.4.0", "--include-optional-plugins=false", "--root"])
        .arg(&root)
        .output()
        .expect("reject generated Flutter staging artifact");

    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("generated local artifact"));
    assert_eq!(
        fs::read_to_string(stage.join("sentinel.txt")).expect("read preserved stage sentinel"),
        "preserved\n"
    );
}

#[cfg(unix)]
#[test]
fn flutter_stage_pub_default_output_rejects_intermediate_symlink_without_writing() {
    use std::os::unix::fs::symlink;

    let directory = flutter_fixture(true);
    let root = flutter_fixture_root(&directory);
    prepare_flutter_pub_fixture(&root);
    let external = tempfile::tempdir().expect("external Flutter staging directory");
    symlink(external.path(), root.join("dist")).expect("link default output outside repository");

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args([
            "flutter",
            "stage-pub",
            "--include-optional-plugins=false",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("reject escaped default Flutter pub output");

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not a regular non-symlink directory")
    );
    assert!(
        fs::read_dir(external.path())
            .expect("inspect external Flutter staging directory")
            .next()
            .is_none(),
        "default staging must not create directories outside the repository"
    );
}

#[test]
fn flutter_stage_pub_rejects_destructive_output_overlap() {
    let directory = flutter_fixture(true);
    let root = flutter_fixture_root(&directory);
    prepare_flutter_pub_fixture(&root);

    let current_directory_output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .current_dir(&root)
        .args([
            "flutter",
            "stage-pub",
            ".",
            "0.4.0",
            "--include-optional-plugins=false",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("reject the current directory as Flutter pub output");
    assert_eq!(current_directory_output.status.code(), Some(4));
    assert!(current_directory_output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&current_directory_output.stderr).contains("repository root"));

    let source_output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["flutter", "stage-pub"])
        .arg(root.join("lib/flutter"))
        .args(["0.4.0", "--include-optional-plugins=false", "--root"])
        .arg(&root)
        .output()
        .expect("reject the Flutter source tree as pub output");
    assert_eq!(source_output.status.code(), Some(4));
    assert!(source_output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&source_output.stderr).contains("protected source"));

    assert!(root.join("Cargo.toml").is_file());
    assert!(
        root.join("lib/flutter")
            .join(FLUTTER_CORE_PACKAGES[0])
            .join("pubspec.yaml")
            .is_file()
    );
}

#[cfg(unix)]
#[test]
fn flutter_stage_pub_rejects_canonical_source_alias_overlap() {
    use std::os::unix::fs::symlink;

    let directory = flutter_fixture(true);
    let root = flutter_fixture_root(&directory);
    prepare_flutter_pub_fixture(&root);
    let real_lib = root.join("real-lib");
    fs::rename(root.join("lib"), &real_lib).expect("move Flutter source directory");
    symlink(&real_lib, root.join("lib")).expect("create contained Flutter source alias");

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["flutter", "stage-pub"])
        .arg(&real_lib)
        .args(["0.4.0", "--include-optional-plugins=false", "--root"])
        .arg(&root)
        .output()
        .expect("reject canonical Flutter source alias as pub output");

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("protected source"));
    assert!(
        real_lib
            .join("flutter")
            .join(FLUTTER_CORE_PACKAGES[0])
            .join("pubspec.yaml")
            .is_file()
    );
}

#[cfg(unix)]
#[test]
fn flutter_stage_pub_rejects_final_source_symlink_overlap() {
    use std::os::unix::fs::symlink;

    let directory = flutter_fixture(true);
    let root = flutter_fixture_root(&directory);
    prepare_flutter_pub_fixture(&root);
    let real_flutter = root.join("real-flutter");
    fs::rename(root.join("lib/flutter"), &real_flutter)
        .expect("move final Flutter source directory");
    symlink(&real_flutter, root.join("lib/flutter")).expect("create final Flutter source alias");

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["flutter", "stage-pub"])
        .arg(&real_flutter)
        .args(["0.4.0", "--include-optional-plugins=false", "--root"])
        .arg(&root)
        .output()
        .expect("reject final Flutter source alias as pub output");

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("protected source"));
    assert!(
        real_flutter
            .join(FLUTTER_CORE_PACKAGES[0])
            .join("pubspec.yaml")
            .is_file()
    );
}

#[test]
fn flutter_stage_pub_embeds_the_verified_optional_ios_package() {
    let directory = flutter_fixture(true);
    let root = flutter_fixture_root(&directory);
    prepare_flutter_pub_fixture(&root);
    prepare_optional_ios_pub_fixture(&root);
    let stage = directory.path().join("optional Flutter stage");

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["flutter", "stage-pub"])
        .arg(&stage)
        .args(["0.4.0", "--include-optional-plugins=true", "--root"])
        .arg(&root)
        .output()
        .expect("stage optional Flutter pub package");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let optional_ios = stage
        .join(FLUTTER_OPTIONAL_PACKAGE)
        .join("ios/VesperPlayerOptionalPlugins");
    assert!(optional_ios.join("Package.swift").is_file());
    assert!(
        optional_ios
            .join("Sources/FixtureProduct/Fixture.swift")
            .is_file()
    );
    assert!(
        optional_ios
            .join("Artifacts/VesperPlayerRemuxFfmpegPlugin.xcframework/Info.plist")
            .is_file()
    );
    assert!(!optional_ios.join(".build").exists());
    assert!(!optional_ios.join(".swiftpm").exists());
    assert!(
        String::from_utf8(output.stdout)
            .expect("UTF-8 optional stage output")
            .contains(&format!("  {FLUTTER_OPTIONAL_PACKAGE}\n"))
    );
}

#[cfg(unix)]
#[test]
fn flutter_pub_dry_run_and_publish_preserve_package_order_and_arguments() {
    let directory = flutter_fixture(true);
    let root = flutter_fixture_root(&directory);
    prepare_flutter_pub_fixture(&root);
    let tools = directory.path().join("fake Flutter tools");
    fs::create_dir_all(&tools).expect("create fake Flutter tools directory");
    let flutter = tools.join("flutter");
    write_executable_script(
        &flutter,
        r#"#!/bin/sh
{
  printf 'cwd=%s' "$PWD"
  for argument in "$@"; do
    printf '\t%s' "$argument"
  done
  printf '\n'
} >> "$FLUTTER_COMMAND_LOG"
"#,
    );
    let stage = directory.path().join("Flutter command stage - 发布");
    let command_log = directory.path().join("flutter-command.log");

    let dry_run = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", &tools)
        .env("FLUTTER_COMMAND_LOG", &command_log)
        .env("VESPER_FLUTTER_INCLUDE_OPTIONAL_PLUGINS", "YES")
        .args(["flutter", "pub-dry-run"])
        .arg(&stage)
        .args(["0.4.0", "--include-optional-plugins=false", "--root"])
        .arg(&root)
        .output()
        .expect("run Flutter pub dry-run with fixture executable");
    assert_eq!(
        dry_run.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&dry_run.stderr)
    );
    assert!(dry_run.stderr.is_empty());
    let dry_stdout = String::from_utf8(dry_run.stdout).expect("UTF-8 dry-run stdout");
    let canonical_stage = stage.canonicalize().expect("resolve Flutter command stage");
    let mut expected_log = String::new();
    for package in FLUTTER_CORE_PACKAGES {
        assert!(dry_stdout.contains(&format!(
            "::group::flutter pub publish --dry-run {package}\n::endgroup::\n"
        )));
        expected_log.push_str(&format!(
            "cwd={}\tpub\tget\n",
            canonical_stage.join(package).display()
        ));
        expected_log.push_str(&format!(
            "cwd={}\tpub\tpublish\t--dry-run\n",
            canonical_stage.join(package).display()
        ));
        assert_eq!(
            fs::read_to_string(stage.join(package).join("pubspec_overrides.yaml"))
                .expect("read dry-run package overrides"),
            expected_package_overrides(package, &FLUTTER_CORE_PACKAGES)
        );
    }
    assert_eq!(
        fs::read_to_string(&command_log).expect("read dry-run Flutter command log"),
        expected_log
    );
    assert!(!stage.join(FLUTTER_OPTIONAL_PACKAGE).exists());

    fs::write(&command_log, "").expect("clear Flutter command log");
    let publish = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", &tools)
        .env("FLUTTER_COMMAND_LOG", &command_log)
        .args(["flutter", "pub-publish"])
        .arg(&stage)
        .args(["0.4.0", "--include-optional-plugins=false", "--root"])
        .arg(&root)
        .output()
        .expect("run Flutter pub publish with fixture executable");
    assert_eq!(
        publish.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&publish.stderr)
    );
    assert!(publish.stderr.is_empty());
    let publish_stdout = String::from_utf8(publish.stdout).expect("UTF-8 publish stdout");
    let mut expected_publish_log = String::new();
    for package in FLUTTER_CORE_PACKAGES {
        assert!(publish_stdout.contains(&format!(
            "::group::flutter pub publish {package}\n::endgroup::\n"
        )));
        expected_publish_log.push_str(&format!(
            "cwd={}\tpub\tget\n",
            canonical_stage.join(package).display()
        ));
        expected_publish_log.push_str(&format!(
            "cwd={}\tpub\tpublish\t--force\n",
            canonical_stage.join(package).display()
        ));
        assert!(!stage.join(package).join("pubspec_overrides.yaml").exists());
    }
    assert_eq!(
        fs::read_to_string(command_log).expect("read publish Flutter command log"),
        expected_publish_log
    );
}

#[test]
fn flutter_local_overrides_preflights_every_pubspec_before_writing() {
    let directory = flutter_fixture(false);
    let root = flutter_fixture_root(&directory);
    let mut seeded = Vec::new();
    for package in FLUTTER_CORE_PACKAGES {
        let output = root
            .join("lib/flutter")
            .join(package)
            .join("pubspec_overrides.yaml");
        let contents = format!("sentinel for {package}\n");
        fs::write(&output, &contents).expect("seed package override");
        seeded.push((output, contents));
    }

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args([
            "flutter",
            "local-overrides",
            "--include-optional-plugins=false",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("reject missing example pubspec");

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Flutter example pubspec"));
    for (path, contents) in seeded {
        assert_eq!(
            fs::read_to_string(path).expect("read preserved output"),
            contents
        );
    }
}

#[cfg(unix)]
#[test]
fn flutter_local_overrides_rejects_symlinked_output_before_writing() {
    use std::os::unix::fs::symlink;

    let directory = flutter_fixture(true);
    let root = flutter_fixture_root(&directory);
    let first_output = root
        .join("lib/flutter")
        .join(FLUTTER_CORE_PACKAGES[0])
        .join("pubspec_overrides.yaml");
    fs::write(&first_output, b"first sentinel\n").expect("seed first override");
    let external = directory.path().join("external sentinel.yaml");
    fs::write(&external, b"external sentinel\n").expect("write external sentinel");
    let symlinked_output = root
        .join("lib/flutter")
        .join(FLUTTER_CORE_PACKAGES[1])
        .join("pubspec_overrides.yaml");
    symlink(&external, &symlinked_output).expect("create output symlink");

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args([
            "flutter",
            "local-overrides",
            "--include-optional-plugins=false",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("reject symlinked Flutter override");

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("non-symlink file"));
    assert_eq!(
        fs::read_to_string(first_output).expect("read preserved first output"),
        "first sentinel\n"
    );
    assert_eq!(
        fs::read_to_string(external).expect("read external sentinel"),
        "external sentinel\n"
    );
}

#[cfg(unix)]
#[test]
fn flutter_local_overrides_rejects_intermediate_symlinks_outside_the_root() {
    use std::os::unix::fs::symlink;

    let directory = flutter_fixture(true);
    let root = flutter_fixture_root(&directory);
    let external_lib = directory.path().join("external Flutter lib");
    fs::rename(root.join("lib"), &external_lib).expect("move Flutter packages outside root");
    let external_output = external_lib
        .join("flutter")
        .join(FLUTTER_CORE_PACKAGES[0])
        .join("pubspec_overrides.yaml");
    fs::write(&external_output, b"external sentinel\n").expect("seed external override");
    symlink(&external_lib, root.join("lib")).expect("link repository lib outside root");

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args([
            "flutter",
            "local-overrides",
            "--include-optional-plugins=false",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("reject intermediate Flutter symlink");

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("resolves outside repository root"));
    assert_eq!(
        fs::read_to_string(external_output).expect("read preserved external override"),
        "external sentinel\n"
    );
}

#[cfg(unix)]
#[test]
fn flutter_android_verification_uses_matching_local_gradle_and_exact_tasks() {
    let (_directory, root) = flutter_android_gradle_fixture("9.6.0");
    write_cached_gradle(&root, "9.6.0", "aaa", "#!/bin/sh\nexit 91\n");
    write_cached_gradle(
        &root,
        "9.6.0",
        "zzz",
        r#"#!/bin/sh
{
  printf 'cwd=%s\n' "$PWD"
  printf 'gradle_user_home=%s\n' "$GRADLE_USER_HOME"
  for argument in "$@"; do
    printf 'arg=%s\n' "$argument"
  done
} > "$FLUTTER_GRADLE_LOG"
"#,
    );
    write_cached_gradle(&root, "8.0.0", "zzz", "#!/bin/sh\nexit 92\n");
    let unusable = write_cached_gradle(&root, "9.6.0", "zzzz", "#!/bin/sh\nexit 93\n");
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(&unusable, fs::Permissions::from_mode(0o001))
            .expect("make newest cached Gradle unusable by its owner");
    }
    let log = root.join("local-gradle.log");

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env_remove("CI")
        .env_remove("GRADLE_USER_HOME")
        .env("VESPER_FLUTTER_INCLUDE_OPTIONAL_PLUGINS", "YES")
        .env("FLUTTER_GRADLE_LOG", &log)
        .args([
            "flutter",
            "verify-android-plugin",
            "--include-optional-plugins=false",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("run local Flutter Android verification");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    let canonical_root = fs::canonicalize(&root).expect("canonical Flutter Android fixture root");
    let project = canonical_root.join("examples/flutter-host/android");
    assert_eq!(
        fs::read_to_string(&log).expect("read local Gradle log"),
        format!(
            "cwd={}\ngradle_user_home={}\narg=-p\narg={}\n\
             arg=:vesper_player_android:compileDebugKotlin\n\
             arg=:vesper_player_external_playback:compileDebugKotlin\n",
            canonical_root.display(),
            project.join(".gradle/gradle-user-home").display(),
            project.display()
        )
    );
    assert!(!canonical_root.join(".gradle").exists());

    let custom_gradle_home = root.join("custom Gradle home");
    let optional_output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env_remove("CI")
        .env("GRADLE_USER_HOME", &custom_gradle_home)
        .env("FLUTTER_GRADLE_LOG", &log)
        .args([
            "flutter",
            "verify-android-plugin",
            "--include-optional-plugins",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("run optional Flutter Android verification");
    assert_eq!(optional_output.status.code(), Some(0));
    let optional_log = fs::read_to_string(&log).expect("read optional Gradle log");
    assert!(optional_log.contains(&format!(
        "gradle_user_home={}\n",
        custom_gradle_home.display()
    )));
    assert!(
        optional_log.ends_with("arg=:vesper_player_source_normalizer_ffmpeg:compileDebugKotlin\n")
    );
}

#[cfg(unix)]
#[test]
fn flutter_android_verification_ci_uses_path_gradle_symlink() {
    use std::os::unix::fs::symlink;

    let (_directory, root) = flutter_android_gradle_fixture("9.6.0");
    let tools = root.join("CI tools");
    fs::create_dir_all(&tools).expect("create CI tools directory");
    let real_gradle = tools.join("real-gradle");
    write_executable_script(
        &real_gradle,
        "#!/bin/sh\nprintf 'ci-gradle=%s\\n' \"$1\" > \"$FLUTTER_GRADLE_LOG\"\n",
    );
    symlink(&real_gradle, tools.join("gradle")).expect("create CI Gradle symlink");
    let log = root.join("ci-gradle.log");

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("CI", "true")
        .env("PATH", &tools)
        .env("FLUTTER_GRADLE_LOG", &log)
        .args([
            "flutter",
            "verify-android-plugin",
            "--include-optional-plugins=false",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("run CI Flutter Android verification");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(log).expect("read CI Gradle log"),
        "ci-gradle=-p\n"
    );
}

#[cfg(unix)]
#[test]
fn flutter_android_verification_never_invokes_uncached_gradlew() {
    let (_directory, root) = flutter_android_gradle_fixture("9.6.0");
    let gradlew = root.join("examples/flutter-host/android/gradlew");
    let sentinel = root.join("gradlew-invoked");
    write_executable_script(
        &gradlew,
        "#!/bin/sh\nprintf invoked > \"$GRADLEW_SENTINEL\"\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env_remove("CI")
        .env("GRADLEW_SENTINEL", &sentinel)
        .args([
            "flutter",
            "verify-android-plugin",
            "--include-optional-plugins=false",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("reject uncached local Gradle verification");

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 missing Gradle diagnostic");
    assert!(stderr.contains("No local cached Gradle distribution"));
    assert!(stderr.contains("Project wrapper version:\n  9.6.0"));
    assert!(stderr.contains("Project wrapper intentionally not invoked"));
    assert!(!sentinel.exists());
}

#[cfg(unix)]
#[test]
fn flutter_android_verification_rejects_malformed_wrapper_before_cache_scan() {
    let (_directory, root) = flutter_android_gradle_fixture("9.6.0");
    let sentinel = root.join("fallback-gradle-invoked");
    write_cached_gradle(
        &root,
        "9.6.0",
        "fallback",
        "#!/bin/sh\nprintf invoked > \"$FALLBACK_GRADLE_SENTINEL\"\n",
    );
    fs::write(
        root.join("examples/flutter-host/android/gradle/wrapper/gradle-wrapper.properties"),
        "distributionUrl=https\\://example.invalid/gradle-9\\..\\..\\outside-bin.zip\n",
    )
    .expect("write malformed Gradle wrapper properties");

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env_remove("CI")
        .env("FALLBACK_GRADLE_SENTINEL", &sentinel)
        .args([
            "flutter",
            "verify-android-plugin",
            "--include-optional-plugins=false",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("reject malformed Gradle wrapper version");

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported distributionUrl"));
    assert!(!sentinel.exists());
}

#[cfg(unix)]
#[test]
fn flutter_android_verification_ci_requires_path_provisioned_gradle() {
    let (_directory, root) = flutter_android_gradle_fixture("9.6.0");
    let empty_path = root.join("empty CI tools");
    fs::create_dir_all(&empty_path).expect("create empty CI tools directory");

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("CI", "true")
        .env("PATH", &empty_path)
        .args([
            "flutter",
            "verify-android-plugin",
            "--include-optional-plugins=false",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("reject missing CI Gradle");

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("CI=true but no CI-provisioned gradle executable was found in PATH")
    );
}

#[cfg(unix)]
#[test]
fn flutter_android_verification_rejects_symlinked_local_gradle() {
    use std::os::unix::fs::symlink;

    let (_directory, root) = flutter_android_gradle_fixture("9.6.0");
    let cached = write_cached_gradle(&root, "9.6.0", "symlink", "#!/bin/sh\nexit 90\n");
    fs::remove_file(&cached).expect("remove regular cached Gradle");
    let external = root.join("external-gradle");
    write_executable_script(&external, "#!/bin/sh\nexit 0\n");
    symlink(&external, &cached).expect("create cached Gradle symlink");

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env_remove("CI")
        .args([
            "flutter",
            "verify-android-plugin",
            "--include-optional-plugins=false",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("reject symlinked cached Gradle");

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("No local cached Gradle distribution")
    );
}

#[cfg(unix)]
#[test]
fn flutter_android_verification_rejects_symlinked_gradle_installation() {
    use std::os::unix::fs::symlink;

    let (directory, root) = flutter_android_gradle_fixture("9.6.0");
    let sentinel = root.join("external-gradle-invoked");
    let external_installation = directory.path().join("external Gradle installation");
    let external_gradle = external_installation.join("bin/gradle");
    fs::create_dir_all(external_gradle.parent().expect("external Gradle parent"))
        .expect("create external Gradle installation");
    write_executable_script(
        &external_gradle,
        "#!/bin/sh\nprintf invoked > \"$EXTERNAL_GRADLE_SENTINEL\"\n",
    );
    let cache_key = root
        .join("examples/flutter-host/android/.gradle/wrapper/dists/gradle-9.6.0-bin")
        .join("symlinked-installation");
    fs::create_dir_all(&cache_key).expect("create Gradle cache key");
    symlink(&external_installation, cache_key.join("gradle-9.6.0"))
        .expect("link Gradle installation outside cache");

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env_remove("CI")
        .env("EXTERNAL_GRADLE_SENTINEL", &sentinel)
        .args([
            "flutter",
            "verify-android-plugin",
            "--include-optional-plugins=false",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("reject symlinked Gradle installation");

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("No local cached Gradle distribution")
    );
    assert!(!sentinel.exists());
}

#[cfg(unix)]
#[test]
fn flutter_android_verification_maps_gradle_failure_to_conformance() {
    let (_directory, root) = flutter_android_gradle_fixture("9.6.0");
    write_cached_gradle(&root, "9.6.0", "failure", "#!/bin/sh\nexit 23\n");

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env_remove("CI")
        .args([
            "flutter",
            "verify-android-plugin",
            "--include-optional-plugins=false",
            "--root",
        ])
        .arg(&root)
        .output()
        .expect("run failing Flutter Android verification");

    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("exited unsuccessfully"));
}

#[cfg(unix)]
#[test]
fn flutter_android_verification_ctrl_c_reaps_the_gradle_process_group() {
    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    use std::time::{Duration, Instant};

    let (_directory, root) = flutter_android_gradle_fixture("9.6.0");
    write_cached_gradle(
        &root,
        "9.6.0",
        "cancel",
        r#"#!/bin/sh
printf '%s' "$$" > "$FLUTTER_GRADLE_PID_FILE"
sh -c 'trap "" INT TERM; printf "%s" "$$" > "$1"; while :; do sleep 1; done' sh "$FLUTTER_GRADLE_DESCENDANT_PID_FILE" &
trap '' INT TERM
while :; do sleep 1; done
"#,
    );
    let gradle_pid_path = root.join("gradle.pid");
    let descendant_pid_path = root.join("gradle-descendant.pid");
    let mut process = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env_remove("CI")
        .env("FLUTTER_GRADLE_PID_FILE", &gradle_pid_path)
        .env("FLUTTER_GRADLE_DESCENDANT_PID_FILE", &descendant_pid_path)
        .args([
            "flutter",
            "verify-android-plugin",
            "--include-optional-plugins=false",
            "--root",
        ])
        .arg(&root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("start cancellable Flutter Android verification");
    let parent_pid = i32::try_from(process.id()).expect("vesper pid");
    let ready_deadline = Instant::now() + Duration::from_secs(15);
    while (!gradle_pid_path.is_file() || !descendant_pid_path.is_file())
        && Instant::now() < ready_deadline
    {
        if process.try_wait().expect("poll vesper process").is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !gradle_pid_path.is_file() || !descendant_pid_path.is_file() {
        let _ = kill(Pid::from_raw(parent_pid), Signal::SIGINT);
        let output = process
            .wait_with_output()
            .expect("wait after Gradle readiness failure");
        panic!(
            "Gradle process group was not ready; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let gradle_pid: i32 = fs::read_to_string(&gradle_pid_path)
        .expect("read Gradle pid")
        .parse()
        .expect("parse Gradle pid");
    let descendant_pid: i32 = fs::read_to_string(&descendant_pid_path)
        .expect("read Gradle descendant pid")
        .parse()
        .expect("parse Gradle descendant pid");

    kill(Pid::from_raw(parent_pid), Signal::SIGINT).expect("cancel Flutter verification");
    let output = process
        .wait_with_output()
        .expect("wait for cancelled Flutter verification");
    assert_eq!(output.status.code(), Some(6));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Flutter Android plugin Gradle verification was cancelled")
    );

    for pid in [gradle_pid, descendant_pid] {
        let reap_deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match kill(Pid::from_raw(pid), None) {
                Err(Errno::ESRCH) => break,
                _ if Instant::now() < reap_deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                result => panic!("Flutter Gradle process {pid} is still alive: {result:?}"),
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn desktop_remux_verification_preserves_override_and_ci_skip_semantics() {
    let directory = tempfile::tempdir().expect("temporary desktop remux fixture");
    let root = directory.path();
    fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("write desktop manifest");
    let plugin = root.join("libvesper_remux_ffmpeg.dylib");
    fs::write(&plugin, b"fixture plugin").expect("write plugin override");
    let fake_cargo = root.join("fake-cargo");
    write_executable_script(&fake_cargo, "#!/bin/sh\nexit 0\n");

    let loader = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("CARGO", &fake_cargo)
        .env("VESPER_PLAYER_REMUX_FFMPEG_PLUGIN_PATH", &plugin)
        .args(["desktop", "verify-remux", "loader", "--root"])
        .arg(root)
        .output()
        .expect("run desktop remux loader verification");
    assert_eq!(
        loader.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&loader.stderr)
    );
    assert!(loader.stderr.is_empty());
    assert_eq!(
        loader.stdout,
        format!("Using player-remux-ffmpeg plugin: {}\n", plugin.display()).as_bytes()
    );

    let tools = root.join("tools");
    fs::create_dir_all(&tools).expect("create desktop tools");
    write_executable_script(&tools.join("ffmpeg"), "#!/bin/sh\nexit 0\n");
    write_executable_script(&tools.join("ffprobe"), "#!/bin/sh\nexit 0\n");
    let path = ffi_path_with_tools(&tools);
    let download = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", path)
        .env("CI", "true")
        .env("VESPER_PLAYER_REMUX_FFMPEG_PLUGIN_PATH", &plugin)
        .args(["desktop", "verify-remux", "download", "--root"])
        .arg(root)
        .output()
        .expect("run desktop remux download verification");
    assert_eq!(download.status.code(), Some(0));
    assert_eq!(
        download.stdout,
        format!("Using player-remux-ffmpeg plugin: {}\n", plugin.display()).as_bytes()
    );
    assert!(
        String::from_utf8_lossy(&download.stderr).contains(
            "Desktop remux fixture is missing in CI, skipping example remux verification"
        )
    );

    let decoder = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("CARGO", &fake_cargo)
        .env("VESPER_DECODER_FIXTURE_PLUGIN_PATH", &plugin)
        .args(["desktop", "verify-decoder-diagnostics", "loader", "--root"])
        .arg(root)
        .output()
        .expect("run decoder diagnostics loader verification");
    assert_eq!(decoder.status.code(), Some(0));
    assert!(decoder.stderr.is_empty());
    assert_eq!(
        decoder.stdout,
        format!(
            "Using decoder fixture plugin: {}\nFixture decoder codecs: fixture-video,H264,HEVC\n",
            plugin.display()
        )
        .as_bytes()
    );

    let d3d11 = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["desktop", "verify-decoder-d3d11", "loader", "--root"])
        .arg(root)
        .output()
        .expect("reject D3D11 verification off Windows");
    assert_eq!(d3d11.status.code(), Some(4));
    assert!(d3d11.stdout.is_empty());
    assert_eq!(
        d3d11.stderr,
        b"D3D11 decoder verification only runs on Windows.\n"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn desktop_ensure_ffmpeg_uses_an_existing_repository_install_without_shell_workers() {
    let directory = tempfile::tempdir().expect("temporary desktop FFmpeg fixture");
    let root = directory.path();
    fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("write desktop manifest");
    let install = root.join("third_party/ffmpeg/desktop");
    for component in [
        "avcodec",
        "avfilter",
        "avformat",
        "avutil",
        "swresample",
        "swscale",
    ] {
        let pkg_config = install.join(format!("lib/pkgconfig/lib{component}.pc"));
        fs::create_dir_all(pkg_config.parent().expect("pkg-config parent"))
            .expect("create desktop FFmpeg pkg-config directory");
        fs::write(&pkg_config, b"Version: 8.1.2\n").expect("write pkg-config metadata");
        fs::write(install.join(format!("lib/lib{component}.a")), b"archive\n")
            .expect("write desktop FFmpeg static archive");
    }

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["desktop", "ensure-ffmpeg", "--root"])
        .arg(root)
        .output()
        .expect("run desktop FFmpeg worker");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let canonical_install = install
        .canonicalize()
        .expect("canonical desktop FFmpeg install");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}\n", canonical_install.display())
    );
    assert!(!root.join("scripts/desktop/ensure-ffmpeg.sh").exists());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn desktop_ensure_ffmpeg_rejects_unsupported_hosts() {
    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["desktop", "ensure-ffmpeg", "--root"])
        .arg(workspace_root())
        .output()
        .expect("reject desktop FFmpeg provisioning off macOS");

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"The repository-local desktop FFmpeg fallback is only supported on macOS.\n"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn desktop_videotoolbox_verification_preserves_loader_and_basic_player_contracts() {
    let directory = tempfile::tempdir().expect("temporary VideoToolbox fixture");
    let root = directory.path();
    fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("write desktop manifest");
    let decoder = root.join("VideoToolbox 插件.dylib");
    let frame_processor = root.join("FrameProcessor 插件.dylib");
    let source = root.join("B-frame source.mp4");
    fs::write(&decoder, b"decoder").expect("write decoder override");
    fs::write(&frame_processor, b"frame processor").expect("write frame processor override");
    fs::write(&source, b"source").expect("write source override");

    let tools = root.join("tools");
    fs::create_dir_all(&tools).expect("create fake tools");
    let cargo_log = root.join("cargo.log");
    write_executable_script(
        &tools.join("cargo"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$FAKE_CARGO_LOG\"\nexit 0\n",
    );
    write_executable_script(&tools.join("ffprobe"), "#!/bin/sh\nprintf '1\\n'\n");
    let path = ffi_path_with_tools(&tools);

    let loader = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", &path)
        .env("CARGO", tools.join("cargo"))
        .env("FAKE_CARGO_LOG", &cargo_log)
        .env("VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH", &decoder)
        .env("VESPER_DECODER_VIDEOTOOLBOX_SOURCE", &source)
        .args(["desktop", "verify-decoder-videotoolbox", "loader", "--root"])
        .arg(root)
        .output()
        .expect("run VideoToolbox loader verification");
    assert_eq!(
        loader.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&loader.stderr)
    );
    assert!(loader.stderr.is_empty());
    assert_eq!(
        loader.stdout,
        format!(
            "Using VideoToolbox decoder plugin: {}\nUsing VideoToolbox smoke source: {}\n",
            decoder.display(),
            source.display()
        )
        .as_bytes()
    );
    let loader_commands = fs::read_to_string(&cargo_log).expect("read fake Cargo log");
    assert_eq!(loader_commands.lines().count(), 6);
    assert!(loader_commands.contains("native_dynamic_loader_opens_videotoolbox_decoder"));
    assert!(loader_commands.contains("macos_videotoolbox_decoder_flush_seek_and_eof_headless"));
    assert!(loader_commands.contains("source_normalizer_packet_source_drop_after_backpressure"));

    let basic_player = root.join("target/debug/basic-player");
    fs::create_dir_all(basic_player.parent().expect("basic-player parent"))
        .expect("create target directory");
    write_executable_script(
        &basic_player,
        r#"#!/bin/sh
test "$VESPER_PLUGIN_DEVELOPMENT_MODE" = "1" || exit 91
printf '%s\n' \
  'initialized desktop player' \
  'selected sdkManagedNativeFrame route' \
  'supports_external_video_surface=true' \
  'frame processor plugins: 1/1 supported' \
  'macOS frame processor debug summary deadline_misses=0 dropped_outputs=0' \
  'basic-player smoke script observed playback' \
  'basic-player smoke script showed overlay' \
  'basic-player smoke script paused playback' \
  'basic-player smoke script resumed playback' \
  'basic-player smoke script seeked to midpoint' \
  'basic-player smoke script changed rate' \
  'player playback ended'
"#,
    );
    let basic = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", path)
        .env("CARGO", tools.join("cargo"))
        .env_remove("CARGO_TARGET_DIR")
        .env("FAKE_CARGO_LOG", &cargo_log)
        .env("VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH", &decoder)
        .env(
            "VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH",
            &frame_processor,
        )
        .env("VESPER_DECODER_VIDEOTOOLBOX_SOURCE", &source)
        .env("VESPER_BASIC_PLAYER_SMOKE_TIMEOUT_SECONDS", "5")
        .args([
            "desktop",
            "verify-decoder-videotoolbox",
            "basic-player",
            "--root",
        ])
        .arg(root)
        .output()
        .expect("run VideoToolbox basic-player verification");
    assert_eq!(
        basic.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&basic.stderr)
    );
    assert!(basic.stderr.is_empty());
    let stdout = String::from_utf8(basic.stdout).expect("UTF-8 basic-player stdout");
    assert!(stdout.contains(&format!(
        "Using diagnostic frame processor plugin: {}\n",
        frame_processor.display()
    )));
    let log_path = stdout
        .lines()
        .find_map(|line| line.strip_prefix("Running basic-player VideoToolbox smoke; log: "))
        .expect("basic-player log path");
    assert!(stdout.contains(&format!(
        "basic-player VideoToolbox smoke passed; log: {log_path}\n"
    )));
    assert!(PathBuf::from(log_path).is_file());
    fs::remove_file(log_path).expect("remove basic-player smoke log");
}

#[cfg(target_os = "macos")]
#[test]
fn desktop_videotoolbox_ctrl_c_reaps_the_basic_player_process_group() {
    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    use std::time::{Duration, Instant};

    let directory = tempfile::tempdir().expect("temporary VideoToolbox cancellation fixture");
    let root = directory.path();
    fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("write desktop manifest");
    let decoder = root.join("decoder.dylib");
    let frame_processor = root.join("frame-processor.dylib");
    let source = root.join("source.mp4");
    fs::write(&decoder, b"decoder").expect("write decoder override");
    fs::write(&frame_processor, b"frame processor").expect("write frame processor override");
    fs::write(&source, b"source").expect("write source override");

    let tools = root.join("tools");
    fs::create_dir_all(&tools).expect("create fake tools");
    write_executable_script(&tools.join("cargo"), "#!/bin/sh\nexit 0\n");
    write_executable_script(&tools.join("ffprobe"), "#!/bin/sh\nprintf '1\\n'\n");
    let path = ffi_path_with_tools(&tools);
    let basic_player = root.join("target/debug/basic-player");
    fs::create_dir_all(basic_player.parent().expect("basic-player parent"))
        .expect("create target directory");
    write_executable_script(
        &basic_player,
        r#"#!/bin/sh
printf '%s' "$$" > "$VT_BASIC_PID_FILE"
sh -c 'trap "" INT TERM; printf "%s" "$$" > "$1"; while :; do sleep 1; done' sh "$VT_DESCENDANT_PID_FILE" &
trap '' INT TERM
while :; do sleep 1; done
"#,
    );
    let basic_pid_path = root.join("basic.pid");
    let descendant_pid_path = root.join("descendant.pid");
    let mut process = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("PATH", path)
        .env("CARGO", tools.join("cargo"))
        .env_remove("CARGO_TARGET_DIR")
        .env("VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH", &decoder)
        .env(
            "VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH",
            &frame_processor,
        )
        .env("VESPER_DECODER_VIDEOTOOLBOX_SOURCE", &source)
        .env("VESPER_BASIC_PLAYER_SMOKE_TIMEOUT_SECONDS", "30")
        .env("VT_BASIC_PID_FILE", &basic_pid_path)
        .env("VT_DESCENDANT_PID_FILE", &descendant_pid_path)
        .args([
            "desktop",
            "verify-decoder-videotoolbox",
            "basic-player",
            "--root",
        ])
        .arg(root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("start VideoToolbox cancellation verification");
    let parent_pid = i32::try_from(process.id()).expect("vesper pid");
    let ready_deadline = Instant::now() + Duration::from_secs(15);
    while (!basic_pid_path.is_file() || !descendant_pid_path.is_file())
        && Instant::now() < ready_deadline
    {
        if process.try_wait().expect("poll vesper process").is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !basic_pid_path.is_file() || !descendant_pid_path.is_file() {
        let _ = kill(Pid::from_raw(parent_pid), Signal::SIGINT);
        let output = process
            .wait_with_output()
            .expect("wait after basic-player readiness failure");
        panic!(
            "basic-player process group was not ready; stdout: {}; stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let basic_pid: i32 = fs::read_to_string(&basic_pid_path)
        .expect("read basic-player pid")
        .parse()
        .expect("parse basic-player pid");
    let descendant_pid: i32 = fs::read_to_string(&descendant_pid_path)
        .expect("read descendant pid")
        .parse()
        .expect("parse descendant pid");

    kill(Pid::from_raw(parent_pid), Signal::SIGINT).expect("cancel VideoToolbox command");
    let output = process
        .wait_with_output()
        .expect("wait for cancelled VideoToolbox command");
    assert_eq!(output.status.code(), Some(6));
    assert!(String::from_utf8_lossy(&output.stderr).contains("basic-player smoke was cancelled"));

    for pid in [basic_pid, descendant_pid] {
        let reap_deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match kill(Pid::from_raw(pid), None) {
                Err(Errno::ESRCH) => break,
                _ if Instant::now() < reap_deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                result => panic!("basic-player process {pid} is still alive: {result:?}"),
            }
        }
    }

    if let Some(log_path) = String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("Running basic-player VideoToolbox smoke; log: "))
    {
        fs::remove_file(log_path).expect("remove cancelled basic-player smoke log");
    }
}

#[test]
fn plugin_new_creates_native_and_wasm_projects_without_path_dependencies() {
    let parent = tempfile::tempdir().expect("temporary scaffold parent");
    let native = parent.path().join("Native 插件");
    let native_output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["plugin", "new"])
        .arg(&native)
        .args([
            "--plugin-id",
            "dev.vesper.native-example",
            "--publisher",
            "dev.vesper",
            "--license",
            "Apache-2.0",
            "--transport",
            "native",
        ])
        .args([
            "--capability",
            "post-download",
            "--capability",
            "event-hook",
            "--capability",
            "benchmark",
            "--capability",
            "decoder",
            "--capability",
            "frame-processor",
            "--capability",
            "source-normalizer-packet",
            "--capability",
            "source-normalizer-resource",
        ])
        .output()
        .expect("create native scaffold");
    assert_eq!(
        native_output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&native_output.stderr)
    );
    assert!(native_output.stderr.is_empty());
    let native_report: serde_json::Value =
        serde_json::from_slice(&native_output.stdout).expect("native scaffold JSON");
    assert_eq!(native_report["transport"], "native");
    assert_eq!(
        native_report["capabilities"].as_array().map(Vec::len),
        Some(7)
    );
    let native_cargo = fs::read_to_string(native.join("Cargo.toml")).expect("native Cargo.toml");
    let native_source = fs::read_to_string(native.join("src/lib.rs")).expect("native source");
    assert!(!native_cargo.contains("path ="));
    assert!(!native_source.contains("unsafe {"));
    assert!(!native_source.contains("extern \"C\""));

    let wasm = parent.path().join("WASM plugin");
    let wasm_output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["plugin", "new"])
        .arg(&wasm)
        .args([
            "--plugin-id",
            "dev.vesper.wasm-example",
            "--publisher",
            "dev.vesper",
            "--license",
            "Apache-2.0",
            "--transport",
            "wasm",
            "--capability",
            "event-hook",
            "--capability",
            "benchmark",
        ])
        .output()
        .expect("create WASM scaffold");
    assert_eq!(
        wasm_output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&wasm_output.stderr)
    );
    let wasm_cargo = fs::read_to_string(wasm.join("Cargo.toml")).expect("WASM Cargo.toml");
    let wasm_source = fs::read_to_string(wasm.join("src/lib.rs")).expect("WASM source");
    assert!(!wasm_cargo.contains("path ="));
    assert!(wasm_cargo.contains(&format!(
        "player-plugin-wasm = \"={}\"",
        env!("CARGO_PKG_VERSION")
    )));
    assert!(wasm_source.contains("#![deny(unsafe_code)]"));
    assert!(!wasm_source.contains("allow(unsafe_code)"));
    assert!(!wasm_source.contains("unsafe {"));
    assert!(!wasm_source.contains("unsafe fn"));
    assert!(!wasm_source.contains("unsafe extern"));
    assert!(!wasm_source.contains("extern \"C\""));
    assert_eq!(
        fs::read(wasm.join("wit/plugin.wit")).expect("scaffold WIT"),
        player_plugin_wasm_host::VESPER_PLUGIN_WIT.as_bytes()
    );
}

#[test]
fn plugin_new_refuses_overwrite_and_unsupported_wasm_capability() {
    let parent = tempfile::tempdir().expect("temporary scaffold parent");
    let existing = parent.path().join("existing");
    fs::create_dir(&existing).expect("create existing directory");
    fs::write(existing.join("sentinel"), b"keep").expect("write sentinel");
    let overwrite = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["plugin", "new"])
        .arg(&existing)
        .args([
            "--plugin-id",
            "dev.vesper.example",
            "--publisher",
            "dev.vesper",
            "--license",
            "Apache-2.0",
            "--transport",
            "native",
            "--capability",
            "event-hook",
        ])
        .output()
        .expect("refuse scaffold overwrite");
    assert_eq!(overwrite.status.code(), Some(3));
    assert!(overwrite.stdout.is_empty());
    assert_eq!(
        fs::read(existing.join("sentinel")).expect("sentinel"),
        b"keep"
    );

    let unsupported = parent.path().join("unsupported");
    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["plugin", "new"])
        .arg(&unsupported)
        .args([
            "--plugin-id",
            "dev.vesper.example",
            "--publisher",
            "dev.vesper",
            "--license",
            "Apache-2.0",
            "--transport",
            "wasm",
            "--capability",
            "decoder",
        ])
        .output()
        .expect("reject WASM decoder scaffold");
    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    assert!(!unsupported.exists());
}

#[cfg(unix)]
#[test]
fn plugin_build_stages_the_single_cargo_cdylib_atomically() {
    let directory = tempfile::tempdir().expect("temporary plugin build project");
    let manifest_path = directory.path().join("vesper-plugin.toml");
    let cargo_manifest = directory.path().join("Cargo.toml");
    let fake_cargo = directory.path().join("fake cargo.sh");
    let cargo_artifact = directory.path().join("Cargo 输出.wasm");
    let staged_artifact = directory.path().join("dist/fixture.wasm");
    fs::write(&manifest_path, wasm_build_manifest("dist/fixture.wasm"))
        .expect("write build manifest");
    fs::write(
        &cargo_manifest,
        "[package]\nname='fixture'\nversion='0.1.0'\n",
    )
    .expect("write Cargo manifest");
    fs::write(&cargo_artifact, b"new wasm component").expect("write Cargo artifact");
    fs::create_dir(directory.path().join("dist")).expect("create existing dist");
    fs::write(&staged_artifact, b"stale artifact").expect("write stale artifact");
    write_executable_script(
        &fake_cargo,
        r#"#!/bin/sh
printf '{"reason":"compiler-artifact","package_id":"fixture 0.1.0","target":{"name":"fixture","kind":["cdylib"],"crate_types":["cdylib"]},"filenames":["%s"]}\n' "$VESPER_FAKE_CARGO_ARTIFACT"
printf '{"reason":"build-finished","success":true}\n'
"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("CARGO", &fake_cargo)
        .env("VESPER_FAKE_CARGO_ARTIFACT", &cargo_artifact)
        .args(["plugin", "build"])
        .arg(&manifest_path)
        .args(["--profile", "dev"])
        .output()
        .expect("build plugin with fake Cargo");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        fs::read(&staged_artifact).expect("read staged artifact"),
        b"new wasm component"
    );
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("build report JSON");
    assert_eq!(report["transport"], "wasm");
    assert_eq!(report["target"], "wasm32-wasip2");
    assert_eq!(report["profile"], "dev");
    assert_eq!(report["cargoTargetName"], "fixture");
    assert_eq!(report["bytes"], 18);
}

#[cfg(unix)]
#[test]
fn plugin_build_supports_separate_descriptor_and_cargo_directories() {
    let directory = tempfile::tempdir().expect("temporary split plugin build project");
    let descriptor_directory = directory.path().join("descriptor 文档");
    let cargo_directory = directory.path().join("Rust crate 空间");
    fs::create_dir(&descriptor_directory).expect("create descriptor directory");
    fs::create_dir(&cargo_directory).expect("create Cargo directory");
    let manifest_path = descriptor_directory.join("vesper-plugin.toml");
    let cargo_manifest = cargo_directory.join("Cargo.toml");
    let fake_cargo = directory.path().join("fake cargo.sh");
    let cargo_log = directory.path().join("cargo invocation.log");
    let cargo_artifact = directory.path().join("Cargo output.wasm");
    let staged_artifact = descriptor_directory.join("dist/fixture.wasm");
    fs::write(&manifest_path, wasm_build_manifest("dist/fixture.wasm"))
        .expect("write build manifest");
    fs::write(
        &cargo_manifest,
        "[package]\nname='fixture'\nversion='0.1.0'\n",
    )
    .expect("write Cargo manifest");
    fs::write(&cargo_artifact, b"split wasm component").expect("write Cargo artifact");
    write_executable_script(
        &fake_cargo,
        r#"#!/bin/sh
printf '%s\n' "$PWD" > "$VESPER_FAKE_CARGO_LOG"
printf '%s\n' "$@" >> "$VESPER_FAKE_CARGO_LOG"
printf '{"reason":"compiler-artifact","package_id":"fixture 0.1.0","target":{"name":"fixture","kind":["cdylib"],"crate_types":["cdylib"]},"filenames":["%s"]}\n' "$VESPER_FAKE_CARGO_ARTIFACT"
printf '{"reason":"build-finished","success":true}\n'
"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .current_dir(directory.path())
        .env("CARGO", &fake_cargo)
        .env("VESPER_FAKE_CARGO_LOG", &cargo_log)
        .env("VESPER_FAKE_CARGO_ARTIFACT", &cargo_artifact)
        .args(["plugin", "build", "descriptor 文档/vesper-plugin.toml"])
        .args(["--cargo-manifest", "Rust crate 空间/Cargo.toml"])
        .output()
        .expect("build split-layout plugin with fake Cargo");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        fs::read(&staged_artifact).expect("read staged artifact"),
        b"split wasm component"
    );
    let invocation = fs::read_to_string(&cargo_log).expect("read Cargo invocation log");
    let invocation_lines = invocation.lines().collect::<Vec<_>>();
    assert_eq!(
        PathBuf::from(invocation_lines[0]),
        fs::canonicalize(&cargo_directory).expect("canonical Cargo directory")
    );
    let manifest_flag = invocation_lines
        .iter()
        .position(|line| *line == "--manifest-path")
        .expect("Cargo manifest flag");
    assert_eq!(
        PathBuf::from(invocation_lines[manifest_flag + 1]),
        fs::canonicalize(&cargo_manifest).expect("canonical Cargo manifest")
    );
}

#[cfg(unix)]
#[test]
fn plugin_build_failure_and_artifact_ambiguity_use_stable_exit_codes() {
    let directory = tempfile::tempdir().expect("temporary plugin build failures");
    let manifest_path = directory.path().join("vesper-plugin.toml");
    let cargo_manifest = directory.path().join("Cargo.toml");
    let fake_cargo = directory.path().join("fake-cargo.sh");
    let staged_artifact = directory.path().join("dist/fixture.wasm");
    fs::write(&manifest_path, wasm_build_manifest("dist/fixture.wasm"))
        .expect("write build manifest");
    fs::write(
        &cargo_manifest,
        "[package]\nname='fixture'\nversion='0.1.0'\n",
    )
    .expect("write Cargo manifest");
    fs::create_dir(directory.path().join("dist")).expect("create dist");
    fs::write(&staged_artifact, b"keep old artifact").expect("write old artifact");
    write_executable_script(
        &fake_cargo,
        "#!/bin/sh\nprintf 'synthetic compiler failure\\n' >&2\nexit 1\n",
    );
    let failed = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("CARGO", &fake_cargo)
        .args(["plugin", "build"])
        .arg(&manifest_path)
        .output()
        .expect("run failed Cargo build");
    assert_eq!(failed.status.code(), Some(5));
    assert!(failed.stdout.is_empty());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("synthetic compiler failure"));
    assert_eq!(
        fs::read(&staged_artifact).expect("read preserved artifact"),
        b"keep old artifact"
    );

    let first = directory.path().join("first.wasm");
    let second = directory.path().join("second.wasm");
    fs::write(&first, b"first").expect("write first artifact");
    fs::write(&second, b"second").expect("write second artifact");
    write_executable_script(
        &fake_cargo,
        r#"#!/bin/sh
printf '{"reason":"compiler-artifact","package_id":"first 0.1.0","target":{"name":"first","kind":["cdylib"],"crate_types":["cdylib"]},"filenames":["%s"]}\n' "$VESPER_FAKE_CARGO_ARTIFACT_A"
printf '{"reason":"compiler-artifact","package_id":"second 0.1.0","target":{"name":"second","kind":["cdylib"],"crate_types":["cdylib"]},"filenames":["%s"]}\n' "$VESPER_FAKE_CARGO_ARTIFACT_B"
printf '{"reason":"build-finished","success":true}\n'
"#,
    );
    let ambiguous = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("CARGO", &fake_cargo)
        .env("VESPER_FAKE_CARGO_ARTIFACT_A", &first)
        .env("VESPER_FAKE_CARGO_ARTIFACT_B", &second)
        .args(["plugin", "build"])
        .arg(&manifest_path)
        .output()
        .expect("run ambiguous Cargo build");
    assert_eq!(ambiguous.status.code(), Some(4));
    assert!(ambiguous.stdout.is_empty());
    assert!(String::from_utf8_lossy(&ambiguous.stderr).contains("emitted 2 matching"));
    assert_eq!(
        fs::read(&staged_artifact).expect("read artifact after ambiguity"),
        b"keep old artifact"
    );
}

#[cfg(unix)]
#[test]
fn plugin_build_ctrl_c_reaps_the_cargo_process_group() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let directory = tempfile::tempdir().expect("temporary cancellable plugin build");
    let manifest_path = directory.path().join("vesper-plugin.toml");
    let fake_cargo = directory.path().join("fake-cargo.sh");
    let cargo_pid_path = directory.path().join("cargo.pid");
    let descendant_pid_path = directory.path().join("descendant.pid");
    fs::write(&manifest_path, wasm_build_manifest("dist/fixture.wasm"))
        .expect("write build manifest");
    fs::write(
        directory.path().join("Cargo.toml"),
        "[package]\nname='fixture'\nversion='0.1.0'\n",
    )
    .expect("write Cargo manifest");
    write_executable_script(
        &fake_cargo,
        r#"#!/bin/sh
printf '%s' "$$" > "$VESPER_FAKE_CARGO_PID_PATH"
sleep 30 &
printf '%s' "$!" > "$VESPER_FAKE_DESCENDANT_PID_PATH"
wait
"#,
    );
    let process = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("CARGO", &fake_cargo)
        .env("VESPER_FAKE_CARGO_PID_PATH", &cargo_pid_path)
        .env("VESPER_FAKE_DESCENDANT_PID_PATH", &descendant_pid_path)
        .args(["plugin", "build"])
        .arg(&manifest_path)
        .args(["--timeout-ms", "30000"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cancellable build");
    let parent_pid = i32::try_from(process.id()).expect("parent pid fits i32");
    let ready_deadline = Instant::now() + Duration::from_secs(10);
    while (!cargo_pid_path.is_file() || !descendant_pid_path.is_file())
        && Instant::now() < ready_deadline
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    if !cargo_pid_path.is_file() || !descendant_pid_path.is_file() {
        let _ = kill(Pid::from_raw(parent_pid), Signal::SIGINT);
        let output = process
            .wait_with_output()
            .expect("wait after fake Cargo readiness timeout");
        panic!(
            "fake Cargo did not start; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let cargo_pid: i32 = fs::read_to_string(&cargo_pid_path)
        .expect("read Cargo pid")
        .parse()
        .expect("parse Cargo pid");
    let descendant_pid: i32 = fs::read_to_string(&descendant_pid_path)
        .expect("read descendant pid")
        .parse()
        .expect("parse descendant pid");
    kill(Pid::from_raw(parent_pid), Signal::SIGINT).expect("cancel vesper build");
    let output = process
        .wait_with_output()
        .expect("wait for cancelled build");
    assert_eq!(output.status.code(), Some(6));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cancelled"));
    let reap_deadline = Instant::now() + Duration::from_secs(2);
    for pid in [cargo_pid, descendant_pid] {
        loop {
            match kill(Pid::from_raw(pid), None) {
                Err(Errno::ESRCH) => break,
                _ if Instant::now() < reap_deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                result => panic!("Cargo build process {pid} is still alive: {result:?}"),
            }
        }
    }
}

#[test]
fn manifest_only_inspection_does_not_access_declared_artifacts() {
    let path = temporary_manifest(&format!(
        r#"{}

[[artifacts]]
transport = "native"
target = "aarch64-apple-darwin"
format = "dylib"
source = "this artifact must not exist.dylib"
path = "artifacts/aarch64-apple-darwin/missing.dylib"
architecture = "arm64"
capabilities = [{{ interface_id = "e9479dbc-42d2-575e-b39e-a24bc512fbc7", instance_id = "dev.vesper.fixture.post-download" }}]
"#,
        manifest("dev.vesper.fixture")
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["plugin", "inspect"])
        .arg(&path)
        .arg("--manifest-only")
        .output()
        .expect("inspect manifest only");
    let _ = fs::remove_file(path);

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("inspection report JSON");
    assert_eq!(report["outcome"], "passed");
    assert!(report.get("transport").is_none());
    assert!(report.get("runtimePluginId").is_none());
}

#[test]
fn incompatible_manifest_inspection_uses_exit_code_four() {
    let source = manifest("dev.vesper.fixture").replace("abi_major = 1", "abi_major = 2");
    let path = temporary_manifest(&source);
    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["plugin", "inspect"])
        .arg(&path)
        .arg("--manifest-only")
        .output()
        .expect("inspect incompatible manifest");
    let _ = fs::remove_file(path);

    assert_eq!(output.status.code(), Some(4));
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("compatibility report JSON");
    assert_eq!(report["outcome"], "compatibilityFailure");
    assert_eq!(report["compatible"], false);
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 stderr")
            .contains("compatibility failures")
    );
}

#[test]
fn invalid_native_artifact_is_a_worker_reported_conformance_failure() {
    let directory = tempfile::tempdir().expect("temporary plugin artifact");
    let manifest_path = directory.path().join("vesper-plugin.toml");
    let artifact_path = directory.path().join("not a library.dylib");
    fs::write(&manifest_path, manifest("dev.vesper.fixture")).expect("write manifest");
    fs::write(&artifact_path, b"not a native library").expect("write invalid artifact");

    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["plugin", "inspect"])
        .arg(&manifest_path)
        .arg("--artifact")
        .arg(&artifact_path)
        .args(["--transport", "native"])
        .output()
        .expect("inspect invalid native artifact");

    assert_eq!(output.status.code(), Some(5));
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("conformance report JSON");
    assert_eq!(report["outcome"], "conformanceFailure");
    assert_eq!(report["conformant"], false);
    assert!(report["workerOutput"]["stdoutBytes"].is_number());
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 stderr")
            .contains("conformance failures")
    );
}

#[test]
fn native_fixture_inspect_and_check_use_a_fresh_worker() {
    let Some(artifact) = native_fixture_artifact() else {
        return;
    };
    let manifest_path = temporary_manifest(&native_fixture_manifest());

    for operation in ["inspect", "check"] {
        let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
            .args(["plugin", operation])
            .arg(&manifest_path)
            .arg("--artifact")
            .arg(&artifact)
            .args(["--transport", "native"])
            .output()
            .expect("run native fixture command");
        assert_eq!(
            output.status.code(),
            Some(0),
            "{operation} stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let report: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("native fixture report JSON");
        assert_eq!(report["outcome"], "passed");
        assert_eq!(report["runtimePluginId"], "dev.vesper.plugin-fixture");
        assert_eq!(report["interfaces"].as_array().map(Vec::len), Some(3));
        assert!(report["workerOutput"]["stdoutBytes"].is_number());
    }
    let _ = fs::remove_file(manifest_path);
}

#[test]
fn native_worker_crash_timeout_and_invalid_response_use_exit_code_six() {
    let Some(artifact) = native_fixture_artifact() else {
        return;
    };
    let manifest_path = temporary_manifest(&native_fixture_manifest());

    let abort = run_native_fixture(&manifest_path, &artifact, "abort", 2_000);
    assert_eq!(abort.status.code(), Some(6));
    assert!(abort.stdout.is_empty());
    assert!(
        String::from_utf8(abort.stderr)
            .expect("UTF-8 abort stderr")
            .contains("exited unsuccessfully")
    );

    let timeout = run_native_fixture(&manifest_path, &artifact, "hang", 200);
    assert_eq!(timeout.status.code(), Some(6));
    assert!(timeout.stdout.is_empty());
    assert!(
        String::from_utf8(timeout.stderr)
            .expect("UTF-8 timeout stderr")
            .contains("deadline")
    );

    let invalid_response = run_native_fixture(&manifest_path, &artifact, "reserve-response", 2_000);
    assert_eq!(invalid_response.status.code(), Some(6));
    assert!(invalid_response.stdout.is_empty());
    assert!(
        String::from_utf8(invalid_response.stderr)
            .expect("UTF-8 invalid-response stderr")
            .contains("exited unsuccessfully")
    );

    let _ = fs::remove_file(manifest_path);
}

#[test]
fn native_worker_output_is_drained_and_retained_only_to_the_cap() {
    let Some(artifact) = native_fixture_artifact() else {
        return;
    };
    let manifest_path = temporary_manifest(&native_fixture_manifest());
    let output = run_native_fixture(&manifest_path, &artifact, "flood", 5_000);
    let _ = fs::remove_file(manifest_path);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("flood report JSON");
    assert!(
        report["workerOutput"]["stdoutBytes"]
            .as_u64()
            .is_some_and(|bytes| bytes >= 300 * 1024)
    );
    assert!(
        report["workerOutput"]["stderrBytes"]
            .as_u64()
            .is_some_and(|bytes| bytes >= 300 * 1024)
    );
    assert_eq!(report["workerOutput"]["stdoutTruncated"], true);
    assert_eq!(report["workerOutput"]["stderrTruncated"], true);
}

#[cfg(unix)]
#[test]
fn native_worker_cleans_up_descendants_after_success() {
    use std::time::{Duration, Instant};

    use nix::errno::Errno;
    use nix::sys::signal::kill;
    use nix::unistd::Pid;

    let Some(artifact) = native_fixture_artifact() else {
        return;
    };
    let directory = tempfile::tempdir().expect("temporary descendant state");
    let manifest_path = directory.path().join("vesper-plugin.toml");
    let pid_path = directory.path().join("descendant.pid");
    fs::write(&manifest_path, native_fixture_manifest()).expect("write fixture manifest");
    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("VESPER_PLUGIN_FIXTURE_WORKER_BEHAVIOR", "spawn-descendant")
        .env("VESPER_PLUGIN_FIXTURE_DESCENDANT_PID_PATH", &pid_path)
        .args(["plugin", "inspect"])
        .arg(&manifest_path)
        .arg("--artifact")
        .arg(&artifact)
        .args(["--transport", "native", "--timeout-ms", "5000"])
        .output()
        .expect("inspect fixture with descendant");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let pid: i32 = fs::read_to_string(&pid_path)
        .expect("read descendant pid")
        .parse()
        .expect("parse descendant pid");
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match kill(Pid::from_raw(pid), None) {
            Err(Errno::ESRCH) => break,
            _ if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            result => panic!("worker descendant {pid} is still alive: {result:?}"),
        }
    }
}

#[cfg(unix)]
#[test]
fn native_worker_ctrl_c_cancels_and_reaps_the_worker_group() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let Some(artifact) = native_fixture_artifact() else {
        return;
    };
    let directory = tempfile::tempdir().expect("temporary cancellation state");
    let manifest_path = directory.path().join("vesper-plugin.toml");
    let worker_pid_path = directory.path().join("worker.pid");
    fs::write(&manifest_path, native_fixture_manifest()).expect("write fixture manifest");
    let process = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("VESPER_PLUGIN_FIXTURE_WORKER_BEHAVIOR", "hang")
        .env("VESPER_PLUGIN_FIXTURE_READY_PID_PATH", &worker_pid_path)
        .args(["plugin", "inspect"])
        .arg(&manifest_path)
        .arg("--artifact")
        .arg(&artifact)
        .args(["--transport", "native", "--timeout-ms", "30000"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cancellable inspection");

    let parent_pid = i32::try_from(process.id()).expect("parent pid fits i32");
    let ready_deadline = Instant::now() + Duration::from_secs(10);
    while !worker_pid_path.is_file() && Instant::now() < ready_deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    if !worker_pid_path.is_file() {
        let _ = kill(Pid::from_raw(parent_pid), Signal::SIGINT);
        let output = process
            .wait_with_output()
            .expect("wait after fixture readiness timeout");
        panic!(
            "worker did not reach fixture; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let worker_pid: i32 = fs::read_to_string(&worker_pid_path)
        .expect("worker reached fixture")
        .parse()
        .expect("parse worker pid");
    kill(Pid::from_raw(parent_pid), Signal::SIGINT).expect("signal vesper parent");
    let output = process
        .wait_with_output()
        .expect("wait for cancelled vesper");

    assert_eq!(output.status.code(), Some(6));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 cancellation stderr")
            .contains("cancelled")
    );
    let reap_deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match kill(Pid::from_raw(worker_pid), None) {
            Err(Errno::ESRCH) => break,
            _ if Instant::now() < reap_deadline => std::thread::sleep(Duration::from_millis(20)),
            result => panic!("cancelled worker {worker_pid} is still alive: {result:?}"),
        }
    }
}

#[test]
fn output_option_atomically_replaces_an_existing_file() {
    let path = temporary_manifest(&manifest("dev.vesper.fixture"));
    let output_path = path.with_extension("sha256");
    fs::write(&output_path, b"stale\n").expect("write stale output");
    let output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["plugin", "descriptor"])
        .arg(&path)
        .args(["--hash-only", "--output"])
        .arg(&output_path)
        .output()
        .expect("run vesper");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    let generated = fs::read_to_string(&output_path).expect("read generated output");
    assert_eq!(generated.trim().len(), 64);
    assert_ne!(generated, "stale\n");

    let _ = fs::remove_file(path);
    let _ = fs::remove_file(output_path);
}

#[test]
fn key_package_verify_install_list_and_uninstall_form_a_trusted_workflow() {
    let directory = tempfile::tempdir().expect("temporary plugin project");
    let manifest_path = directory.path().join("vesper-plugin.toml");
    let artifact_path = directory.path().join("fixture plugin.dylib");
    let signing_key_path = directory.path().join("publisher-key.json");
    let trust_store_path = directory.path().join("trust-store.json");
    let package_path = directory.path().join("fixture.vesper-plugin");
    fs::write(&manifest_path, package_manifest()).expect("write package manifest");
    fs::write(&artifact_path, b"fixture native artifact").expect("write artifact");
    fs::write(directory.path().join("LICENSE"), b"Apache-2.0\n").expect("write license");
    fs::write(directory.path().join("NOTICE"), b"Fixture notice\n").expect("write notice");

    let key_output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["plugin", "key", "generate", "--publisher"])
        .arg("dev.vesper.publisher")
        .arg("--signing-key-output")
        .arg(&signing_key_path)
        .arg("--trust-store-output")
        .arg(&trust_store_path)
        .output()
        .expect("generate plugin signing key");
    assert_eq!(key_output.status.code(), Some(0));
    assert!(key_output.stderr.is_empty());
    assert!(signing_key_path.is_file());
    assert!(trust_store_path.is_file());
    let key_report: serde_json::Value =
        serde_json::from_slice(&key_output.stdout).expect("key report JSON");
    assert_eq!(key_report["publisher"], "dev.vesper.publisher");
    assert_eq!(key_report["keyId"].as_str().map(str::len), Some(64));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&signing_key_path)
                .expect("signing key metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    let package_output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["plugin", "package"])
        .arg(&manifest_path)
        .arg("--signing-key")
        .arg(&signing_key_path)
        .arg("--output")
        .arg(&package_path)
        .output()
        .expect("build signed plugin package");
    assert_eq!(package_output.status.code(), Some(0));
    assert!(package_output.stderr.is_empty());
    assert!(package_path.is_file());
    let package_report: serde_json::Value =
        serde_json::from_slice(&package_output.stdout).expect("package report JSON");
    assert_eq!(package_report["plugin_id"], "dev.vesper.fixture");
    assert_eq!(package_report["artifact_count"], 1);

    let verify_output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["plugin", "verify"])
        .arg(&package_path)
        .arg("--trust-store")
        .arg(&trust_store_path)
        .output()
        .expect("verify signed plugin package");
    assert_eq!(verify_output.status.code(), Some(0));
    assert!(verify_output.stderr.is_empty());
    let verification: serde_json::Value =
        serde_json::from_slice(&verify_output.stdout).expect("verification JSON");
    assert_eq!(verification["plugin_id"], "dev.vesper.fixture");
    assert_eq!(
        verification["package_sha256"],
        package_report["package_sha256"]
    );

    let install_root = directory.path().join("installed-plugins");
    let install_output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["plugin", "install"])
        .arg(&package_path)
        .arg("--trust-store")
        .arg(&trust_store_path)
        .arg("--root")
        .arg(&install_root)
        .output()
        .expect("install signed plugin package");
    assert_eq!(install_output.status.code(), Some(0));
    assert!(install_output.stderr.is_empty());
    let install_report: serde_json::Value =
        serde_json::from_slice(&install_output.stdout).expect("install report JSON");
    assert_eq!(install_report["plugin_id"], "dev.vesper.fixture");
    assert_eq!(install_report["version"], "1.2.3");
    assert_eq!(install_report["already_installed"], false);
    assert!(
        install_report["install_path"]
            .as_str()
            .map(PathBuf::from)
            .is_some_and(|path| path.is_dir())
    );

    let list_output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["plugin", "list", "--root"])
        .arg(&install_root)
        .output()
        .expect("list installed plugins");
    assert_eq!(list_output.status.code(), Some(0));
    assert!(list_output.stderr.is_empty());
    let installed: serde_json::Value =
        serde_json::from_slice(&list_output.stdout).expect("installed plugin list JSON");
    assert_eq!(installed.as_array().map(Vec::len), Some(1));
    assert_eq!(installed[0]["plugin_id"], "dev.vesper.fixture");
    assert_eq!(
        installed[0]["package_sha256"],
        package_report["package_sha256"]
    );

    let uninstall_output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args([
            "plugin",
            "uninstall",
            "--plugin-id",
            "dev.vesper.fixture",
            "--version",
            "1.2.3",
            "--root",
        ])
        .arg(&install_root)
        .output()
        .expect("uninstall plugin");
    assert_eq!(uninstall_output.status.code(), Some(0));
    assert!(uninstall_output.stderr.is_empty());
    let uninstall_report: serde_json::Value =
        serde_json::from_slice(&uninstall_output.stdout).expect("uninstall report JSON");
    assert_eq!(uninstall_report["removed"], true);

    let empty_list_output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["plugin", "list", "--root"])
        .arg(&install_root)
        .output()
        .expect("list plugins after uninstall");
    assert_eq!(empty_list_output.status.code(), Some(0));
    assert!(empty_list_output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&empty_list_output.stdout)
            .expect("empty installed plugin list JSON"),
        serde_json::json!([])
    );

    let descriptor_output = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["plugin", "descriptor"])
        .arg(&manifest_path)
        .output()
        .expect("emit descriptor from package project");
    assert_eq!(descriptor_output.status.code(), Some(0));
    let descriptor: serde_json::Value =
        serde_json::from_slice(&descriptor_output.stdout).expect("descriptor JSON");
    assert!(descriptor.get("artifacts").is_none());
    assert!(descriptor.get("package_files").is_none());
}

#[cfg(unix)]
#[test]
fn json_path_preflight_rejects_non_utf8_before_mutating_state() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let directory = tempfile::tempdir().expect("temporary CLI directory");
    let invalid_path = |name: &[u8]| directory.path().join(OsString::from_vec(name.to_vec()));
    let assert_preflight = |output: std::process::Output| {
        assert_eq!(output.status.code(), Some(3));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .expect("UTF-8 stderr")
                .contains("must be valid UTF-8 because it is included in JSON output")
        );
    };

    let invalid_package_output = invalid_path(b"package-\xff.vesper-plugin");
    assert_preflight(
        Command::new(env!("CARGO_BIN_EXE_vesper"))
            .args(["plugin", "package"])
            .arg(directory.path().join("missing.toml"))
            .arg("--signing-key")
            .arg(directory.path().join("missing-key.json"))
            .arg("--output")
            .arg(&invalid_package_output)
            .output()
            .expect("preflight package output"),
    );
    assert!(!invalid_package_output.exists());

    let invalid_package_input = invalid_path(b"package-input-\xff.vesper-plugin");
    assert_preflight(
        Command::new(env!("CARGO_BIN_EXE_vesper"))
            .args(["plugin", "verify"])
            .arg(&invalid_package_input)
            .arg("--trust-store")
            .arg(directory.path().join("missing-trust.json"))
            .output()
            .expect("preflight verify input"),
    );

    let invalid_install_root = invalid_path(b"install-\xff");
    assert_preflight(
        Command::new(env!("CARGO_BIN_EXE_vesper"))
            .args(["plugin", "install"])
            .arg(directory.path().join("missing.vesper-plugin"))
            .arg("--trust-store")
            .arg(directory.path().join("missing-trust.json"))
            .arg("--root")
            .arg(&invalid_install_root)
            .output()
            .expect("preflight install root"),
    );
    assert!(!invalid_install_root.exists());

    assert_preflight(
        Command::new(env!("CARGO_BIN_EXE_vesper"))
            .args(["plugin", "list", "--root"])
            .arg(invalid_path(b"list-\xff"))
            .output()
            .expect("preflight list root"),
    );

    let signing_key_path = directory.path().join("publisher-key.json");
    let invalid_trust_store_path = invalid_path(b"trust-\xff.json");
    assert_preflight(
        Command::new(env!("CARGO_BIN_EXE_vesper"))
            .args(["plugin", "key", "generate", "--publisher"])
            .arg("dev.vesper.publisher")
            .arg("--signing-key-output")
            .arg(&signing_key_path)
            .arg("--trust-store-output")
            .arg(&invalid_trust_store_path)
            .output()
            .expect("preflight trust store output"),
    );
    assert!(!signing_key_path.exists());
    assert!(!invalid_trust_store_path.exists());

    let invalid_signing_key_path = invalid_path(b"key-\xff.json");
    let trust_store_path = directory.path().join("trust-store.json");
    assert_preflight(
        Command::new(env!("CARGO_BIN_EXE_vesper"))
            .args(["plugin", "key", "generate", "--publisher"])
            .arg("dev.vesper.publisher")
            .arg("--signing-key-output")
            .arg(&invalid_signing_key_path)
            .arg("--trust-store-output")
            .arg(&trust_store_path)
            .output()
            .expect("preflight signing key output"),
    );
    assert!(!invalid_signing_key_path.exists());
    assert!(!trust_store_path.exists());
}
