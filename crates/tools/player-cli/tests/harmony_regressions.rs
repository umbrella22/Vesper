#![cfg(all(unix, vesper_source_checkout))]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

struct Fixture {
    directory: tempfile::TempDir,
    root: PathBuf,
    tools: PathBuf,
    bin: PathBuf,
    std: PathBuf,
}

fn script(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().expect("script parent")).expect("create parent");
    fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("write tool");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("executable tool");
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("fixture");
        let root = directory.path().join("checkout with spaces");
        let tools = directory.path().join("command line tools");
        let bin = directory.path().join("bin");
        let std = directory.path().join("rust std");
        for path in [&root, &tools, &bin, &std] {
            fs::create_dir_all(path).expect("directory");
        }
        fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").expect("manifest");
        fs::write(std.join("libstd-fixture.rlib"), []).expect("std marker");
        script(
            &bin.join("rustc"),
            r#"
case "$1 $2" in
    '--version ') echo 'rustc 1.99.1 (fixture)' ;;
    '--print cfg') printf '%s\n' 'target_arch="aarch64"' 'target_os="linux"' 'target_env="ohos"' ;;
    '--print target-libdir') printf '%s\n' "$VESPER_TEST_STD" ;;
    *) exit 1 ;;
esac"#,
        );
        let native = tools.join("sdk/default/openharmony/native");
        fs::create_dir_all(native.join("sysroot/usr/include/napi")).expect("headers");
        fs::write(native.join("sysroot/usr/include/napi/native_api.h"), []).expect("header");
        fs::create_dir_all(native.join("sysroot/usr/lib/aarch64-linux-ohos")).expect("libraries");
        fs::write(native.join("oh-uni-package.json"), json!({
            "apiVersion": "26", "platformVersion": "26.0.0", "releaseType": "Release", "version": "26.0.0.fixture"
        }).to_string()).expect("identity");
        for path in [
            "sdk/default/openharmony/native/llvm/bin/clang",
            "sdk/default/openharmony/native/llvm/bin/clang++",
            "sdk/default/openharmony/native/llvm/bin/llvm-readelf",
            "sdk/default/openharmony/native/llvm/bin/llvm-nm",
            "tool/node/bin/node",
            "bin/ohpm",
            "bin/hvigorw",
            "sdk/default/openharmony/toolchains/hdc",
        ] {
            script(&tools.join(path), "echo 'fixture version 1'");
        }
        Self {
            directory,
            root,
            tools,
            bin,
            std,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_vesper"));
        command
            .args(["harmony", "--root"])
            .arg(&self.root)
            .arg("--command-line-tools")
            .arg(&self.tools)
            .env("PATH", &self.bin)
            .env("VESPER_TEST_STD", &self.std)
            .env_remove("VESPER_HARMONY_COMMAND_LINE_TOOLS");
        command
    }

    fn cargo_tree(&self, extra: &str) {
        let tree = format!(
            "player-model v0.9.1\nplayer-runtime v0.9.1\nplayer-download v0.9.1\nplayer-preload v0.9.1\nplayer-playlist v0.9.1\nplayer-platform-mobile v0.9.1\n{extra}"
        );
        fs::write(self.directory.path().join("tree.txt"), tree).expect("tree fixture");
        script(
            &self.bin.join("cargo"),
            r#"
printf '%s\n' "$@" > "$VESPER_TEST_ARGS"
/bin/cat "$VESPER_TEST_TREE"
"#,
        );
    }

    fn audit(&self) -> Output {
        self.command()
            .args(["audit-dependencies", "--offline"])
            .env("VESPER_TEST_ARGS", self.directory.path().join("args.txt"))
            .env("VESPER_TEST_TREE", self.directory.path().join("tree.txt"))
            .output()
            .expect("audit")
    }
}

fn report(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("report {error}: {output:?}"))
}

#[test]
fn harmony_help_exposes_formal_workflows_without_retired_prototype_commands() {
    let fixture = Fixture::new();
    let output = fixture.command().arg("--help").output().expect("help");
    assert!(output.status.success(), "{output:?}");
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");
    for command in [
        "kit-har",
        "kit-host-contracts",
        "kit-consumer",
        "kit-verify",
        "emulator-gate",
    ] {
        assert!(help.contains(command), "missing {command}: {help}");
    }
    for command in [
        "native-smoke",
        "har-smoke",
        "host-bridge-smoke",
        "consumer-smoke",
    ] {
        assert!(!help.contains(command), "retired {command}: {help}");
    }
}

#[test]
fn doctor_reports_host_prerequisites_and_keeps_device_gates_unverified() {
    let fixture = Fixture::new();
    script(
        &fixture.tools.join("bin/hvigorw"),
        "/bin/mkdir .hvigor\necho 'fixture version 1'",
    );
    let output = fixture.command().arg("doctor").output().expect("doctor");
    assert!(output.status.success(), "{output:?}");
    assert!(!fixture.root.join(".hvigor").exists());
    let report = report(&output);
    assert_eq!(report["status"], "passed");
    assert_eq!(report["scope"], "harmony-host-prerequisites");
    assert_eq!(report["target"], "aarch64-unknown-linux-ohos");
    assert!(
        report["unverified"]
            .as_array()
            .expect("gates")
            .iter()
            .any(|value| value
                .as_str()
                .is_some_and(|v| v.contains("physical-device")))
    );
}

#[test]
fn doctor_does_not_mistake_a_printable_target_libdir_for_installed_std() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.std.join("libstd-fixture.rlib")).expect("remove std");
    let output = fixture.command().arg("doctor").output().expect("doctor");
    assert_eq!(output.status.code(), Some(4));
    let report = report(&output);
    assert_eq!(report["status"], "blocked");
    assert!(
        report["checks"]
            .as_array()
            .expect("checks")
            .iter()
            .any(|value| value["id"] == "rust_target_std" && value["status"] == "blocked")
    );
}

#[test]
fn doctor_rejects_beta_sdks_and_failing_tools() {
    let fixture = Fixture::new();
    fs::write(fixture.tools.join("sdk/default/openharmony/native/oh-uni-package.json"),
        r#"{"apiVersion":"26","platformVersion":"26.0.1","releaseType":"Beta","version":"fixture"}"#).expect("beta identity");
    script(
        &fixture.tools.join("bin/hvigorw"),
        "echo 'fixture failure' >&2; exit 1",
    );
    let output = fixture.command().arg("doctor").output().expect("doctor");
    assert_eq!(output.status.code(), Some(4));
    let report = report(&output);
    for id in ["sdk_release", "hvigor"] {
        assert!(
            report["checks"]
                .as_array()
                .expect("checks")
                .iter()
                .any(|value| value["id"] == id && value["status"] == "blocked")
        );
    }
}

#[test]
fn dependency_audit_uses_ohos_normal_and_build_edges_with_a_locked_graph() {
    let fixture = Fixture::new();
    fixture.cargo_tree("libloading v0.9.0\nlibc v0.2.189\n");
    let output = fixture.audit();
    assert!(output.status.success(), "{output:?}");
    let report = report(&output);
    assert_eq!(report["status"], "passed");
    assert_eq!(report["forbiddenPackages"], json!([]));
    assert!(
        report["packages"]
            .as_array()
            .expect("packages")
            .contains(&json!("libloading v0.9.0"))
    );
    let args = fs::read_to_string(fixture.directory.path().join("args.txt")).expect("args");
    for argument in [
        "--locked",
        "--offline",
        "aarch64-unknown-linux-ohos",
        "normal,build",
    ] {
        assert!(args.lines().any(|line| line == argument));
    }
}

#[test]
fn dependency_audit_rejects_desktop_and_ffmpeg_contamination() {
    let fixture = Fixture::new();
    fixture.cargo_tree("player-ffi v0.9.1\nffmpeg-sys-next v9.0.0\nplayer-platform-linux v0.9.1\n");
    let output = fixture.audit();
    assert_eq!(output.status.code(), Some(5));
    let report = report(&output);
    assert_eq!(report["status"], "failed");
    assert_eq!(
        report["forbiddenPackages"]
            .as_array()
            .expect("forbidden")
            .len(),
        3
    );
}

#[test]
fn dependency_audit_rejects_incomplete_or_malformed_cargo_output() {
    let fixture = Fixture::new();
    fixture.cargo_tree("");
    for tree in ["player-platform-mobile v0.9.1\n", "not cargo output\n"] {
        fs::write(fixture.directory.path().join("tree.txt"), tree).expect("replace tree");
        let output = fixture.audit();
        assert_eq!(output.status.code(), Some(5), "{output:?}");
    }
}

#[test]
fn cross_check_failure_is_reported_as_failure() {
    let fixture = Fixture::new();
    script(
        &fixture.bin.join("cargo"),
        "echo 'fixture compile failure' >&2; exit 101",
    );
    let output = fixture
        .command()
        .args(["check-shared", "--offline"])
        .output()
        .expect("check");
    assert_eq!(output.status.code(), Some(6));
    let report = report(&output);
    assert_eq!(report["status"], "failed");
    assert!(
        report["detail"]
            .as_str()
            .expect("failure")
            .contains("fixture compile failure")
    );
}

#[test]
fn kit_consumer_forwards_paths_and_feature_flags_as_separate_arguments() {
    let fixture = Fixture::new();
    let source = fixture.root.join("scripts/harmony/build-consumer.py");
    fs::create_dir_all(source.parent().expect("script parent")).expect("scripts");
    fs::write(&source, "# fixture\n").expect("verifier marker");
    script(
        &fixture.bin.join("python3"),
        "printf '%s\\n' \"$@\" > \"$VESPER_TEST_ARGS\"\necho '{}'",
    );
    let args_path = fixture.directory.path().join("consumer args.txt");
    let har = fixture
        .directory
        .path()
        .join("candidate with spaces/core.har");
    let output = fixture
        .command()
        .arg("kit-consumer")
        .arg("--har")
        .arg(&har)
        .args(["--ui", "--example", "--no-internet"])
        .env("VESPER_TEST_ARGS", &args_path)
        .output()
        .expect("consumer");
    assert!(output.status.success(), "{output:?}");
    let args = fs::read_to_string(args_path).expect("recorded arguments");
    let expected = [
        source
            .canonicalize()
            .expect("canonical verifier")
            .display()
            .to_string(),
        "--tools".into(),
        fixture.tools.display().to_string(),
        "--har".into(),
        har.display().to_string(),
        "--ui".into(),
        "--example".into(),
        "--no-internet".into(),
    ];
    assert_eq!(args.lines().collect::<Vec<_>>(), expected);
}
