#![cfg(all(target_os = "macos", vesper_source_checkout))]

use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const RELEASE_FIXTURE_ENV: &str = "VESPER_IOS_OPTIONAL_RELEASE_FIXTURE";
const CORE_RELEASE_FIXTURE_ENV: &str = "VESPER_IOS_CORE_RELEASE_FIXTURE";
const COMPLIANCE_ARCHIVE: &str = "VesperPlayerOptionalPlugins-FFmpeg-Compliance.zip";
const COMPLIANCE_ROOT: &str = "VesperPlayerOptionalPlugins-FFmpeg-Compliance";
const CORE_RELEASE_ARCHIVES: [&str; 3] = [
    "VesperPlayerKit-ios-arm64.framework.zip",
    "VesperPlayerKit-ios-simulator-arm64.framework.zip",
    "VesperPlayerKit.xcframework.zip",
];
const FFMPEG_FRAMEWORKS: [&str; 5] = [
    "VesperFFmpegAVCodec",
    "VesperFFmpegAVFormat",
    "VesperFFmpegAVUtil",
    "VesperPlayerRemuxFfmpegPlugin",
    "VesperPlayerSourceNormalizerFfmpegPlugin",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn run_checked(command: &mut Command, label: &str) -> Output {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("failed to run {label}: {error}"));
    assert!(
        output.status.success(),
        "{label} failed with {}:\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn copy_release_fixture(source: &Path, destination: &Path) {
    run_checked(
        Command::new("ditto").arg(source).arg(destination),
        "release fixture copy",
    );
}

fn copy_core_release_fixture(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create core release fixture directory");
    for archive in CORE_RELEASE_ARCHIVES {
        fs::copy(source.join(archive), destination.join(archive))
            .unwrap_or_else(|error| panic!("copy core release archive {archive}: {error}"));
    }
}

fn extract_zip(archive: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create ZIP extraction directory");
    run_checked(
        Command::new("ditto")
            .args([OsStr::new("-x"), OsStr::new("-k")])
            .arg(archive)
            .arg(destination),
        "ZIP extraction",
    );
}

fn repack_zip(root: &Path, archive: &Path) {
    fs::remove_file(archive).expect("remove original ZIP before repacking");
    run_checked(
        Command::new("ditto")
            .args([
                OsStr::new("-c"),
                OsStr::new("-k"),
                OsStr::new("--sequesterRsrc"),
                OsStr::new("--keepParent"),
            ])
            .arg(root)
            .arg(archive),
        "ZIP repack",
    );
}

fn repack_core_zip(root: &Path, archive: &Path) {
    fs::remove_file(archive).expect("remove original core ZIP before repacking");
    let parent = root.parent().expect("core framework parent directory");
    let framework = root.file_name().expect("core framework directory name");
    run_checked(
        Command::new("/usr/bin/zip")
            .current_dir(parent)
            .args([OsStr::new("-q"), OsStr::new("-r"), OsStr::new("-X")])
            .arg(archive)
            .arg(framework)
            .args([
                OsStr::new("-x"),
                OsStr::new("*/._*"),
                OsStr::new("__MACOSX/*"),
            ]),
        "core ZIP repack",
    );
}

fn remove_appledouble_files(root: &Path) {
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).expect("enumerate core mutation tree") {
            let entry = entry.expect("read core mutation entry");
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == "__MACOSX" {
                fs::remove_dir_all(path).expect("remove AppleDouble directory");
            } else if name.starts_with("._") {
                fs::remove_file(path).expect("remove AppleDouble sidecar");
            } else if entry
                .file_type()
                .expect("inspect core mutation entry")
                .is_dir()
            {
                pending.push(path);
            }
        }
    }
}

fn verify_release(release: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "--root"])
        .arg(workspace_root())
        .arg("verify-optional-plugins-release")
        .arg(release)
        .output()
        .expect("run optional iOS release verifier")
}

fn verify_core_release(root: &Path, release: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vesper"))
        .args(["ios", "--root"])
        .arg(root)
        .arg("verify-release")
        .arg(release)
        .args(["--scope", "core"])
        .output()
        .expect("run core iOS release verifier")
}

fn expect_verification_failure(release: &Path, expected: &str) {
    let output = verify_release(release);
    assert_eq!(
        output.status.code(),
        Some(5),
        "malformed release returned an unexpected status:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "failure output must stay on stderr"
    );
    let diagnostics = String::from_utf8(output.stderr).expect("UTF-8 verifier diagnostics");
    assert!(
        diagnostics.contains(expected),
        "expected diagnostic {expected:?}, got:\n{diagnostics}"
    );
}

fn replace_once(path: &Path, from: &str, to: &str) {
    let source = fs::read_to_string(path).expect("read mutation target");
    assert!(
        source.contains(from),
        "missing mutation source in {}",
        path.display()
    );
    fs::write(path, source.replacen(from, to, 1)).expect("write mutated fixture");
}

fn mutate_compliance(release: &Path, relative: &str, mutation: impl FnOnce(&Path)) {
    let archive = release.join(COMPLIANCE_ARCHIVE);
    let extraction = release.join("compliance-extract");
    extract_zip(&archive, &extraction);
    let root = extraction.join(COMPLIANCE_ROOT);
    mutation(&root.join(relative));
    repack_zip(&root, &archive);
    fs::remove_dir_all(extraction).expect("remove compliance extraction directory");
}

fn mutate_framework(release: &Path, framework: &str, label: &str, mutation: impl FnOnce(&Path)) {
    let archive = release.join(format!("{framework}.xcframework.zip"));
    let extraction = release.join(label);
    extract_zip(&archive, &extraction);
    let root = extraction.join(format!("{framework}.xcframework"));
    mutation(&root);
    repack_zip(&root, &archive);
    fs::remove_dir_all(extraction).expect("remove XCFramework extraction directory");
}

fn source_version(release: &Path) -> String {
    let prefix = "VesperPlayerOptionalPlugins-FFmpeg-";
    let suffix = "-source.tar.xz";
    let versions = fs::read_dir(release)
        .expect("read release fixture")
        .map(|entry| entry.expect("read release entry"))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter_map(|name| {
            name.strip_prefix(prefix)
                .and_then(|value| value.strip_suffix(suffix))
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    assert_eq!(versions.len(), 1, "fixture must contain one source archive");
    versions.into_iter().next().expect("one source version")
}

fn mutate_core_framework(
    release: &Path,
    archive_name: &str,
    label: &str,
    mutation: impl FnOnce(&Path),
) {
    let archive = release.join(archive_name);
    let extraction = release.join(label);
    extract_zip(&archive, &extraction);
    let root = extraction.join("VesperPlayerKit.framework");
    mutation(&root);
    remove_appledouble_files(&root);
    run_checked(
        Command::new("/usr/bin/xattr").args(["-c", "-r"]).arg(&root),
        "clear core mutation extended attributes",
    );
    repack_core_zip(&root, &archive);
    fs::remove_dir_all(extraction).expect("remove core framework extraction directory");
}

#[test]
#[ignore = "requires a staged core iOS release fixture"]
fn ios_core_release_real_fixture_rejects_policy_drift() {
    let source = std::env::var_os(CORE_RELEASE_FIXTURE_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            panic!("{CORE_RELEASE_FIXTURE_ENV} must name a staged release directory")
        });
    assert!(
        source.is_dir(),
        "missing staged release: {}",
        source.display()
    );
    let temporary = tempfile::tempdir().expect("create core release regression fixture");
    let root = temporary.path().join("repository");
    let generated = root.join("lib/ios/VesperPlayerKit/Sources");
    fs::create_dir_all(&generated).expect("create core release metadata directory");
    fs::write(root.join("Cargo.toml"), "[workspace]\n")
        .expect("write core release repository manifest");
    fs::copy(
        workspace_root().join("lib/ios/VesperPlayerKit/Sources/Generated-Info.plist"),
        generated.join("Generated-Info.plist"),
    )
    .expect("copy core release metadata");
    let fixture = temporary.path().join("core release");
    copy_core_release_fixture(&source, &fixture);
    let valid = verify_core_release(&root, &fixture);
    assert_eq!(
        valid.status.code(),
        Some(0),
        "valid core release failed:\n{}",
        String::from_utf8_lossy(&valid.stderr)
    );
    assert!(valid.stderr.is_empty());
    assert_eq!(
        String::from_utf8(valid.stdout).expect("UTF-8 core release report"),
        format!(
            "Verified VesperPlayerKit iOS release assets (core):\n  {}\n",
            fixture.display()
        )
    );

    let leaked_interface = temporary.path().join("leaked interface");
    copy_release_fixture(&fixture, &leaked_interface);
    mutate_core_framework(
        &leaked_interface,
        "VesperPlayerKit-ios-arm64.framework.zip",
        "leaked-interface-extract",
        |framework| {
            let path = framework
                .join("Modules/VesperPlayerKit.swiftmodule/arm64-apple-ios.swiftinterface");
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(path)
                .expect("open textual interface");
            writeln!(file, "// vesper_private_fixture").expect("append private declaration");
        },
    );
    expect_core_failure(
        &root,
        &leaked_interface,
        "textual interface leaks private bridge declarations",
    );

    let wrong_identity = temporary.path().join("wrong identity");
    copy_release_fixture(&fixture, &wrong_identity);
    mutate_core_framework(
        &wrong_identity,
        "VesperPlayerKit-ios-simulator-arm64.framework.zip",
        "wrong-identity-extract",
        |framework| {
            run_checked(
                Command::new("/usr/libexec/PlistBuddy")
                    .args([
                        "-c",
                        "Set :CFBundleIdentifier io.github.ikaros.vesper.invalid",
                    ])
                    .arg(framework.join("Info.plist")),
                "core framework identity mutation",
            );
        },
    );
    expect_core_failure(
        &root,
        &wrong_identity,
        "Unexpected VesperPlayerKit CFBundleIdentifier",
    );

    let binary_marker = temporary.path().join("binary marker");
    copy_release_fixture(&fixture, &binary_marker);
    mutate_core_framework(
        &binary_marker,
        "VesperPlayerKit-ios-arm64.framework.zip",
        "binary-marker-extract",
        |framework| {
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(framework.join("VesperPlayerKit"))
                .expect("open core framework binary");
            file.write_all(b"fixtures/contracts\0")
                .expect("append fixture marker");
        },
    );
    expect_core_failure(
        &root,
        &binary_marker,
        "Release binary contains test fixture markers",
    );
}

fn expect_core_failure(root: &Path, release: &Path, expected_diagnostic: &str) {
    let output = verify_core_release(root, release);
    assert_eq!(
        output.status.code(),
        Some(5),
        "malformed core release returned an unexpected status:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(expected_diagnostic),
        "expected diagnostic {expected_diagnostic:?}, got:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "requires a staged optional iOS release fixture"]
fn ios_optional_release_real_fixture_rejects_policy_drift() {
    let source = std::env::var_os(RELEASE_FIXTURE_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{RELEASE_FIXTURE_ENV} must name a staged release directory"));
    assert!(
        source.is_dir(),
        "missing staged release: {}",
        source.display()
    );
    let temporary = tempfile::tempdir().expect("create regression fixture root");

    let valid = verify_release(&source);
    assert_eq!(
        valid.status.code(),
        Some(0),
        "valid release failed:\n{}",
        String::from_utf8_lossy(&valid.stderr)
    );
    assert!(valid.stderr.is_empty());

    let missing_lipo = Command::new(env!("CARGO_BIN_EXE_vesper"))
        .env("LIPO", temporary.path().join("missing-lipo"))
        .args(["ios", "--root"])
        .arg(workspace_root())
        .arg("verify-optional-plugins-release")
        .arg(&source)
        .output()
        .expect("run optional verifier without lipo");
    assert_eq!(missing_lipo.status.code(), Some(4));
    assert!(missing_lipo.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&missing_lipo.stderr)
            .contains("failed to spawn framework architecture inspection")
    );

    let stale_asset = temporary.path().join("stale-asset");
    copy_release_fixture(&source, &stale_asset);
    fs::write(
        stale_asset.join("VesperPlayerRetiredPlugin.xcframework.zip"),
        [],
    )
    .expect("write stale release asset");
    expect_verification_failure(&stale_asset, "Unexpected top-level iOS release asset");

    let unlocked_source = temporary.path().join("unlocked-source");
    copy_release_fixture(&source, &unlocked_source);
    let locked_version = source_version(&unlocked_source);
    fs::rename(
        unlocked_source.join(format!(
            "VesperPlayerOptionalPlugins-FFmpeg-{locked_version}-source.tar.xz"
        )),
        unlocked_source.join("VesperPlayerOptionalPlugins-FFmpeg-8.1.999-source.tar.xz"),
    )
    .expect("rename source archive to an unlocked compatible version");
    expect_verification_failure(&unlocked_source, "does not match locked release version");

    let altered_source = temporary.path().join("altered-source");
    copy_release_fixture(&source, &altered_source);
    let version = source_version(&altered_source);
    let source_archive = altered_source.join(format!(
        "VesperPlayerOptionalPlugins-FFmpeg-{version}-source.tar.xz"
    ));
    let mut permissions = fs::metadata(&source_archive)
        .expect("inspect copied source archive")
        .permissions();
    permissions.set_mode(permissions.mode() | 0o200);
    fs::set_permissions(&source_archive, permissions)
        .expect("make copied source archive writable for mutation");
    let mut source_file = fs::OpenOptions::new()
        .append(true)
        .open(&source_archive)
        .expect("open source archive for mutation");
    source_file
        .write_all(b"altered")
        .expect("alter source archive checksum");
    expect_verification_failure(&altered_source, "expected locked release SHA-256");

    let empty_notice = temporary.path().join("empty-notice");
    copy_release_fixture(&source, &empty_notice);
    mutate_compliance(&empty_notice, "NOTICE.txt", |path| {
        fs::write(path, []).expect("empty compliance notice");
    });
    expect_verification_failure(
        &empty_notice,
        "Missing or empty compliance bundle entry: NOTICE.txt",
    );

    let altered_license = temporary.path().join("altered-license");
    copy_release_fixture(&source, &altered_license);
    mutate_compliance(&altered_license, "COPYING.LGPLv2.1", |path| {
        fs::write(path, b"corrupted release fixture\n").expect("alter compliance license");
    });
    expect_verification_failure(
        &altered_license,
        "The LGPL-2.1 license copy does not match its release source",
    );

    let extra_slice = temporary.path().join("extra-slice");
    copy_release_fixture(&source, &extra_slice);
    mutate_framework(
        &extra_slice,
        "VesperFFmpegAVCodec",
        "extra-slice-extract",
        |root| {
            copy_release_fixture(&root.join("ios-arm64"), &root.join("ios-arm64-maccatalyst"));
        },
    );
    expect_verification_failure(
        &extra_slice,
        "payload does not match the canonical release layout",
    );

    let altered_manifest = temporary.path().join("altered-manifest");
    copy_release_fixture(&source, &altered_manifest);
    mutate_framework(
        &altered_manifest,
        "VesperFFmpegAVCodec",
        "altered-manifest-extract",
        |root| {
            let plist = root.join("Info.plist");
            let simulator_index = (0..16)
                .find(|index| {
                    let output = Command::new("/usr/libexec/PlistBuddy")
                        .args([
                            "-c",
                            &format!("Print :AvailableLibraries:{index}:SupportedPlatformVariant"),
                        ])
                        .arg(&plist)
                        .output()
                        .expect("read XCFramework platform variant");
                    output.status.success()
                        && String::from_utf8_lossy(&output.stdout).trim() == "simulator"
                })
                .expect("locate simulator library in XCFramework manifest");
            run_checked(
                Command::new("/usr/libexec/PlistBuddy")
                    .args([
                        "-c",
                        &format!(
                            "Set :AvailableLibraries:{simulator_index}:SupportedPlatformVariant maccatalyst"
                        ),
                    ])
                    .arg(&plist),
                "XCFramework manifest mutation",
            );
        },
    );
    expect_verification_failure(
        &altered_manifest,
        "Unexpected XCFramework SupportedPlatformVariant",
    );

    let missing_path = temporary.path().join("missing-manifest-path");
    copy_release_fixture(&source, &missing_path);
    mutate_framework(
        &missing_path,
        "VesperPlayerDecoderVideoToolboxPlugin",
        "missing-path-extract",
        |root| {
            fs::rename(
                root.join("ios-arm64/VesperPlayerDecoderVideoToolboxPlugin.framework"),
                root.join("ios-arm64/Rogue.framework"),
            )
            .expect("rename declared framework path");
        },
    );
    expect_verification_failure(
        &missing_path,
        "payload does not match the canonical release layout",
    );

    let undeclared_payload = temporary.path().join("undeclared-payload");
    copy_release_fixture(&source, &undeclared_payload);
    mutate_framework(
        &undeclared_payload,
        "VesperPlayerDecoderVideoToolboxPlugin",
        "undeclared-payload-extract",
        |root| {
            let unexpected = root.join("ios-arm64/Unexpected.framework");
            fs::create_dir_all(&unexpected).expect("create undeclared framework");
            fs::write(unexpected.join("Unexpected"), []).expect("write undeclared payload");
        },
    );
    expect_verification_failure(
        &undeclared_payload,
        "payload does not match the canonical release layout",
    );

    let invalid_target = temporary.path().join("invalid-target");
    copy_release_fixture(&source, &invalid_target);
    for framework in FFMPEG_FRAMEWORKS {
        mutate_framework(
            &invalid_target,
            framework,
            &format!("{framework}-target-extract"),
            |root| {
                let metadata = root.join(format!(
                    "ios-arm64/{framework}.framework/ios-arm64-vesper-ffmpeg-build-metadata.txt"
                ));
                replace_once(
                    &metadata,
                    "target=ios-arm64",
                    "target=invalid-device-target",
                );
            },
        );
    }
    mutate_compliance(
        &invalid_target,
        "build-metadata/ios-arm64-vesper-ffmpeg-build-metadata.txt",
        |path| replace_once(path, "target=ios-arm64", "target=invalid-device-target"),
    );
    expect_verification_failure(
        &invalid_target,
        "Unexpected FFmpeg metadata value for 'target'",
    );

    let mismatched_slice = temporary.path().join("mismatched-slice-metadata");
    copy_release_fixture(&source, &mismatched_slice);
    let version = source_version(&mismatched_slice);
    mutate_framework(
        &mismatched_slice,
        "VesperFFmpegAVCodec",
        "mismatched-slice-extract",
        |root| {
            let metadata = root.join(
                "ios-arm64-simulator/VesperFFmpegAVCodec.framework/ios-simulator-arm64-vesper-ffmpeg-build-metadata.txt",
            );
            replace_once(
                &metadata,
                &format!("ffmpeg_version={version}"),
                "ffmpeg_version=9.9.9",
            );
        },
    );
    expect_verification_failure(
        &mismatched_slice,
        "Unexpected FFmpeg metadata value for 'ffmpeg_version'",
    );

    let duplicate_metadata = temporary.path().join("duplicate-metadata");
    copy_release_fixture(&source, &duplicate_metadata);
    mutate_compliance(&duplicate_metadata, "SOURCE.txt", |path| {
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(path)
            .expect("open compliance source record");
        writeln!(file, "ffmpeg_version=9.9.9").expect("append duplicate metadata key");
    });
    expect_verification_failure(
        &duplicate_metadata,
        "Duplicate FFmpeg metadata key 'ffmpeg_version'",
    );
}
