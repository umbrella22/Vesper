use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::subtitle::{SubtitleError, SubtitleScope};
use crate::{android, contract, external_process, gradle};

const ANDROID_ABI: &str = "arm64-v8a";
const ANDROID_APPLICATION_ID: &str = "io.github.ikaros.vesper.example.flutterhost";
const MAX_STEP_STDOUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_STEP_STDERR_BYTES: usize = 64 * 1024 * 1024;
const MAX_PREFLIGHT_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_EVIDENCE_JSON_BYTES: u64 = 1024 * 1024;
const MAX_INSTRUMENTATION_XML_BYTES: u64 = 8 * 1024 * 1024;
const MAX_EVIDENCE_FILES: usize = 4096;
const MAX_EVIDENCE_DIRECTORY_ENTRIES: usize = 16384;
const MAX_EVIDENCE_DEPTH: usize = 32;
const MAX_EVIDENCE_FILE_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct SubtitleRequest {
    pub(crate) scope: SubtitleScope,
    pub(crate) device_id: Option<String>,
    pub(crate) evidence_directory: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StepRecord {
    name: String,
    result: &'static str,
    duration_seconds: u64,
    log: String,
}

struct EvidenceRun {
    root: PathBuf,
    directory: PathBuf,
    logs: PathBuf,
    preflight: PathBuf,
    flutter: PathBuf,
    android: PathBuf,
    source_sha: String,
    source_dirty: bool,
    run_id: String,
    started_at: String,
    selected_device: Option<Value>,
    steps: Vec<StepRecord>,
}

pub(crate) fn verify(
    root: &Path,
    request: SubtitleRequest,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> Result<(), SubtitleError> {
    validate_request(&request)?;
    let _run_lock =
        android::AndroidBuildLock::acquire(root, "subtitle").map_err(map_android_error)?;
    let mut evidence = EvidenceRun::create(root, &request)?;
    let mut flutter_ready = false;
    let run_result = (|| {
        collect_toolchain(&mut evidence)?;
        if request.scope.includes_regression() {
            run_regression(&mut evidence, diagnostics, &mut flutter_ready)?;
        }
        if request.scope.includes_device() {
            let device = request.device_id.as_deref().ok_or_else(|| {
                SubtitleError::usage(
                    "--device is required for Android device subtitle verification",
                )
            })?;
            run_device(&mut evidence, diagnostics, &mut flutter_ready, device)?;
        }
        Ok(())
    })();
    let exit_code = run_result
        .as_ref()
        .err()
        .map_or(0, SubtitleError::exit_code);
    let finalize_result = evidence.finalize(&request, exit_code);
    writeln!(
        output,
        "Subtitle verification evidence: {}",
        evidence.directory.display()
    )
    .map_err(|error| SubtitleError::storage(format!("failed to write subtitle result: {error}")))?;
    output.flush().map_err(|error| {
        SubtitleError::storage(format!("failed to flush subtitle result: {error}"))
    })?;
    match (run_result, finalize_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Ok(()), Err(error)) => Err(error),
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(finalize_error)) => Err(error.with_suffix(finalize_error)),
    }
}

fn validate_request(request: &SubtitleRequest) -> Result<(), SubtitleError> {
    match (request.scope, request.device_id.as_deref()) {
        (SubtitleScope::Regression, Some(_)) => Err(SubtitleError::usage(
            "--device is not used by Android subtitle scope 'regression'",
        )),
        (SubtitleScope::Device | SubtitleScope::Complete, None | Some("")) => {
            Err(SubtitleError::usage(format!(
                "--device is required for Android subtitle scope '{}'",
                request.scope.as_str()
            )))
        }
        (_, Some(device)) if device.len() > 256 || device.chars().any(char::is_control) => {
            Err(SubtitleError::usage(
                "Android device identifier is empty, overlong, or contains control characters",
            ))
        }
        _ => Ok(()),
    }
}

impl EvidenceRun {
    fn create(root: &Path, request: &SubtitleRequest) -> Result<Self, SubtitleError> {
        let canonical_root = root.canonicalize().map_err(|error| {
            SubtitleError::storage(format!(
                "failed to resolve repository root '{}': {error}",
                root.display()
            ))
        })?;
        let git = require_command("git", "git is required for subtitle verification")?;
        let source_sha = capture_text(
            Command::new(&git)
                .arg("-C")
                .arg(&canonical_root)
                .args(["rev-parse", "HEAD"]),
            "subtitle source revision",
            MAX_PREFLIGHT_OUTPUT_BYTES,
        )?;
        let short_sha = capture_text(
            Command::new(&git).arg("-C").arg(&canonical_root).args([
                "rev-parse",
                "--short=12",
                "HEAD",
            ]),
            "subtitle short source revision",
            MAX_PREFLIGHT_OUTPUT_BYTES,
        )?;
        let now = UtcTimestamp::now()?;
        let run_id = format!("{}-{short_sha}", now.compact);
        let requested = request.evidence_directory.clone().unwrap_or_else(|| {
            canonical_root
                .join("devnotes/evidence/subtitle/android")
                .join(&run_id)
        });
        let directory = if requested.is_absolute() {
            requested
        } else {
            canonical_root.join(requested)
        };
        reject_existing_path(&directory, "subtitle evidence directory")?;
        let parent = directory.parent().ok_or_else(|| {
            SubtitleError::usage(format!(
                "subtitle evidence directory '{}' has no parent",
                directory.display()
            ))
        })?;
        fs::create_dir_all(parent).map_err(|error| {
            SubtitleError::storage(format!(
                "failed to create subtitle evidence parent '{}': {error}",
                parent.display()
            ))
        })?;
        fs::create_dir(&directory).map_err(|error| {
            SubtitleError::storage(format!(
                "failed to create subtitle evidence directory '{}': {error}",
                directory.display()
            ))
        })?;
        let directory = directory.canonicalize().map_err(|error| {
            SubtitleError::storage(format!(
                "failed to resolve subtitle evidence directory '{}': {error}",
                directory.display()
            ))
        })?;
        let logs = directory.join("logs");
        let preflight = directory.join("preflight");
        let flutter = directory.join("flutter");
        let android = directory.join("android");
        let xcresult = directory.join("xcresult");
        let xctest_attachments = directory.join("xctest-attachments");
        for child in [
            &logs,
            &preflight,
            &flutter,
            &android,
            &xcresult,
            &xctest_attachments,
        ] {
            fs::create_dir(child).map_err(|error| {
                SubtitleError::storage(format!(
                    "failed to create subtitle evidence directory '{}': {error}",
                    child.display()
                ))
            })?;
        }
        let status = capture_command(
            Command::new(&git)
                .arg("-C")
                .arg(&canonical_root)
                .args(["status", "--short"]),
            "subtitle source status",
            MAX_PREFLIGHT_OUTPUT_BYTES,
            MAX_PREFLIGHT_OUTPUT_BYTES,
        )?;
        if !status.status.success() {
            return Err(process_status_error(
                "subtitle source status",
                status.status,
            ));
        }
        write_file(&directory.join("source-status.txt"), &status.stdout)?;
        write_file(
            &directory.join("source-sha.txt"),
            format!("{source_sha}\n").as_bytes(),
        )?;
        Ok(Self {
            root: canonical_root,
            directory,
            logs,
            preflight,
            flutter,
            android,
            source_sha,
            source_dirty: !status.stdout.is_empty(),
            run_id,
            started_at: now.iso8601,
            selected_device: None,
            steps: Vec::new(),
        })
    }

    fn run_step(
        &mut self,
        name: &str,
        working_directory: &Path,
        command: &mut Command,
        diagnostics: &mut dyn Write,
    ) -> Result<Vec<u8>, SubtitleError> {
        let started = Instant::now();
        let log_path = self.logs.join(format!("{name}.log"));
        let relative_log = format!("logs/{name}.log");
        command.current_dir(working_directory);
        writeln!(diagnostics, "Running subtitle verification step: {name}")
            .map_err(diagnostic_error)?;
        diagnostics.flush().map_err(diagnostic_error)?;
        let header = format!(
            "Working directory: {}\nCommand: {}\n\n",
            working_directory.display(),
            display_command(command)
        );
        let result = external_process::run_interruptible_capture(
            command,
            &format!("subtitle verification step {name}"),
            MAX_STEP_STDOUT_BYTES,
            MAX_STEP_STDERR_BYTES,
        );
        let duration_seconds = started.elapsed().as_secs();
        match result {
            Ok(captured) => {
                let mut log = Vec::with_capacity(
                    header.len() + captured.stdout.len() + captured.stderr.len() + 32,
                );
                log.extend_from_slice(header.as_bytes());
                log.extend_from_slice(&captured.stdout);
                if !captured.stdout.ends_with(b"\n") && !captured.stdout.is_empty() {
                    log.push(b'\n');
                }
                log.extend_from_slice(&captured.stderr);
                write_file(&log_path, &log)?;
                let passed = captured.status.success();
                self.steps.push(StepRecord {
                    name: name.to_owned(),
                    result: if passed { "passed" } else { "failed" },
                    duration_seconds,
                    log: relative_log,
                });
                if passed {
                    Ok(captured.stdout)
                } else {
                    Err(process_status_error(name, captured.status))
                }
            }
            Err(error) => {
                let error = map_process_error(error);
                let mut log = header.into_bytes();
                log.extend_from_slice(format!("{error}\n").as_bytes());
                write_file(&log_path, &log)?;
                self.steps.push(StepRecord {
                    name: name.to_owned(),
                    result: "failed",
                    duration_seconds,
                    log: relative_log,
                });
                Err(error)
            }
        }
    }

    fn record_internal_step(
        &mut self,
        name: &str,
        result: Result<Vec<u8>, SubtitleError>,
        started: Instant,
    ) -> Result<Vec<u8>, SubtitleError> {
        let log_path = self.logs.join(format!("{name}.log"));
        let duration_seconds = started.elapsed().as_secs();
        match result {
            Ok(log) => {
                write_file(&log_path, &log)?;
                self.steps.push(StepRecord {
                    name: name.to_owned(),
                    result: "passed",
                    duration_seconds,
                    log: format!("logs/{name}.log"),
                });
                Ok(log)
            }
            Err(error) => {
                write_file(&log_path, format!("{error}\n").as_bytes())?;
                self.steps.push(StepRecord {
                    name: name.to_owned(),
                    result: "failed",
                    duration_seconds,
                    log: format!("logs/{name}.log"),
                });
                Err(error)
            }
        }
    }

    fn finalize(&self, request: &SubtitleRequest, exit_code: i32) -> Result<(), SubtitleError> {
        let finished_at = UtcTimestamp::now()?.iso8601;
        let result = if exit_code == 0 { "passed" } else { "failed" };
        let mut summary = String::from("# Vesper Subtitle Verification\n\n");
        for (label, value) in [
            ("Result", result.to_owned()),
            ("Platform", "android".to_owned()),
            ("Scope", request.scope.as_str().to_owned()),
            ("Run ID", self.run_id.clone()),
            ("Source SHA", self.source_sha.clone()),
            ("Started", self.started_at.clone()),
            ("Finished", finished_at.clone()),
            (
                "Device",
                request
                    .device_id
                    .clone()
                    .unwrap_or_else(|| "not requested".to_owned()),
            ),
            ("Evidence", self.directory.display().to_string()),
        ] {
            summary.push_str(&format!("- {label}: {value}\n"));
        }
        summary.push_str(
            "\n## Steps\n\n| Step | Result | Seconds | Log |\n| --- | --- | ---: | --- |\n",
        );
        for step in &self.steps {
            summary.push_str(&format!(
                "| `{}` | {} | {} | `{}` |\n",
                step.name, step.result, step.duration_seconds, step.log
            ));
        }
        write_file(&self.directory.join("summary.md"), summary.as_bytes())?;
        let mut steps = String::new();
        for step in &self.steps {
            steps.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                step.name, step.result, step.duration_seconds, step.log
            ));
        }
        write_file(&self.directory.join("steps.tsv"), steps.as_bytes())?;
        let manifest = json!({
            "schema": "vesper-subtitle-evidence-v1",
            "result": result,
            "exitCode": exit_code,
            "platform": "android",
            "scope": request.scope.as_str(),
            "runId": self.run_id,
            "sourceSha": self.source_sha,
            "sourceDirty": self.source_dirty,
            "startedAt": self.started_at,
            "finishedAt": finished_at,
            "deviceId": request.device_id,
            "simulatorId": Value::Null,
            "selectedDevice": self.selected_device,
            "steps": self.steps,
            "artifacts": {
                "summary": "summary.md",
                "toolchain": "toolchain.txt",
                "sourceStatus": "source-status.txt",
                "logs": "logs",
                "xcresults": "xcresult",
                "flutter": "flutter",
                "xctestAttachments": "xctest-attachments",
                "android": "android",
                "checksums": "SHA256SUMS"
            }
        });
        let mut bytes = serde_json::to_vec_pretty(&manifest).map_err(|error| {
            SubtitleError::worker(format!(
                "failed to serialize subtitle evidence manifest: {error}"
            ))
        })?;
        bytes.push(b'\n');
        write_file(&self.directory.join("manifest.json"), &bytes)?;
        write_evidence_checksums(&self.directory)
    }
}

fn collect_toolchain(evidence: &mut EvidenceRun) -> Result<(), SubtitleError> {
    let started = Instant::now();
    let result = (|| {
        let mut text = String::new();
        for (label, command_name, arguments) in [
            ("git", "git", vec!["--version"]),
            ("rustc", "rustc", vec!["--version"]),
            ("cargo", "cargo", vec!["--version"]),
            ("flutter", "flutter", vec!["--version"]),
        ] {
            let command = require_command(
                command_name,
                &format!("{command_name} is required for Android subtitle verification"),
            )?;
            let value = capture_text(
                Command::new(command).args(arguments),
                &format!("{label} version"),
                MAX_PREFLIGHT_OUTPUT_BYTES,
            )?;
            text.push_str(&format!("{label}:\n{value}\n\n"));
        }
        if let Some(adb) = resolve_command("adb") {
            let value = capture_text(
                Command::new(adb).arg("version"),
                "adb version",
                MAX_PREFLIGHT_OUTPUT_BYTES,
            )?;
            text.push_str(&format!("adb:\n{value}\n"));
        }
        write_file(&evidence.directory.join("toolchain.txt"), text.as_bytes())?;
        Ok(text.into_bytes())
    })();
    evidence.record_internal_step("toolchain", result, started)?;
    Ok(())
}

fn prepare_flutter_dependencies(
    evidence: &mut EvidenceRun,
    diagnostics: &mut dyn Write,
    ready: &mut bool,
) -> Result<PathBuf, SubtitleError> {
    let flutter = require_command("flutter", "Flutter is required for subtitle verification")?;
    if !*ready {
        let project = evidence.root.join("examples/flutter-host");
        let mut command = Command::new(&flutter);
        command.args(["pub", "get"]);
        evidence.run_step("flutter-host-pub-get", &project, &mut command, diagnostics)?;
        *ready = true;
    }
    Ok(flutter)
}

fn run_regression(
    evidence: &mut EvidenceRun,
    diagnostics: &mut dyn Write,
    flutter_ready: &mut bool,
) -> Result<(), SubtitleError> {
    let cargo = require_command("cargo", "cargo is required for subtitle verification")?;
    let flutter = prepare_flutter_dependencies(evidence, diagnostics, flutter_ready)?;
    let root = evidence.root.clone();

    let started = Instant::now();
    let contract_result = contract::verify(&root)
        .map(|verification| verification.output().as_bytes().to_vec())
        .map_err(|error| match error {
            contract::ContractError::Drift(message) => SubtitleError::conformance(message),
            contract::ContractError::Storage(message) => SubtitleError::storage(message),
        });
    evidence.record_internal_step("contract-verify", contract_result, started)?;

    let mut rust = Command::new(&cargo);
    rust.args([
        "test",
        "-p",
        "player-ffi",
        "-p",
        "player-platform-android",
        "-p",
        "player-jni-android",
    ]);
    evidence.run_step("android-rust-subtitle-tests", &root, &mut rust, diagnostics)?;

    let android_project = root.join("lib/android");
    let compose_fallback = root.join("examples/android-compose-host");
    let android_gradle =
        gradle::resolve(&android_project, Some(&compose_fallback)).map_err(map_gradle_error)?;
    let mut host = Command::new(android_gradle);
    host.arg("-p")
        .arg(&android_project)
        .arg("-Pvesper.player.android.abis=arm64-v8a")
        .arg("test")
        .env(
            "GRADLE_USER_HOME",
            android_project.join(".gradle/gradle-user-home"),
        );
    evidence.run_step("android-host-subtitle-tests", &root, &mut host, diagnostics)?;

    let flutter_android = root.join("examples/flutter-host/android");
    let flutter_gradle =
        gradle::resolve(&flutter_android, Some(&android_project)).map_err(map_gradle_error)?;
    let mut kotlin = Command::new(flutter_gradle);
    kotlin
        .arg("-p")
        .arg(&flutter_android)
        .arg("-Pvesper.player.android.abis=arm64-v8a")
        .arg(":vesper_player_android:testDebugUnitTest")
        .env(
            "GRADLE_USER_HOME",
            flutter_android.join(".gradle/gradle-user-home"),
        );
    evidence.run_step(
        "flutter-android-kotlin-tests",
        &root,
        &mut kotlin,
        diagnostics,
    )?;

    for (name, directory, tests) in [
        (
            "flutter-platform-subtitle-tests",
            "lib/flutter/vesper_player_platform_interface",
            vec![
                "test/subtitle_exception_test.dart",
                "test/subtitle_state_models_test.dart",
            ],
        ),
        (
            "flutter-controller-subtitle-tests",
            "lib/flutter/vesper_player",
            vec!["test/vesper_download_manager_test.dart"],
        ),
        (
            "flutter-android-channel-tests",
            "lib/flutter/vesper_player_android",
            vec!["test/method_channel_vesper_player_android_test.dart"],
        ),
        (
            "flutter-host-subtitle-evidence-test",
            "examples/flutter-host",
            vec!["test/subtitle_overlay_evidence_test.dart"],
        ),
    ] {
        let mut command = Command::new(&flutter);
        command.args(["test"]).args(tests);
        evidence.run_step(name, &root.join(directory), &mut command, diagnostics)?;
    }
    Ok(())
}

fn run_device(
    evidence: &mut EvidenceRun,
    diagnostics: &mut dyn Write,
    flutter_ready: &mut bool,
    device_id: &str,
) -> Result<(), SubtitleError> {
    let flutter = prepare_flutter_dependencies(evidence, diagnostics, flutter_ready)?;
    let adb = require_command(
        "adb",
        "adb is required for Android device subtitle verification",
    )?;
    let root = evidence.root.clone();
    let flutter_project = root.join("examples/flutter-host");

    let mut devices = Command::new(&flutter);
    devices.args(["devices", "--machine"]);
    let devices_json = evidence.run_step(
        "android-flutter-device-list",
        &flutter_project,
        &mut devices,
        diagnostics,
    )?;
    write_file(
        &evidence.preflight.join("flutter-devices-device.json"),
        &devices_json,
    )?;
    let selected = select_flutter_device(&devices_json, device_id)?;

    let mut state = Command::new(&adb);
    state.args(["-s", device_id, "get-state"]);
    let state_output = evidence.run_step("android-adb-state", &root, &mut state, diagnostics)?;
    if std::str::from_utf8(&state_output)
        .map(str::trim)
        .unwrap_or_default()
        != "device"
    {
        return Err(SubtitleError::compatibility(format!(
            "Android device '{device_id}' is not in adb device state"
        )));
    }

    let mut getprop = Command::new(&adb);
    getprop.args(["-s", device_id, "shell", "getprop"]);
    let properties_output =
        evidence.run_step("android-adb-properties", &root, &mut getprop, diagnostics)?;
    let properties = parse_android_properties(&properties_output)?;
    let selected = augment_selected_device(selected, &properties)?;
    let mut selected_bytes = serde_json::to_vec_pretty(&selected).map_err(|error| {
        SubtitleError::worker(format!(
            "failed to serialize selected Android device: {error}"
        ))
    })?;
    selected_bytes.push(b'\n');
    write_file(
        &evidence.preflight.join("selected-device.json"),
        &selected_bytes,
    )?;
    evidence.selected_device = Some(selected);

    let android_project = root.join("lib/android");
    let gradle = gradle::resolve(
        &android_project,
        Some(&root.join("examples/android-compose-host")),
    )
    .map_err(map_gradle_error)?;
    let instrumentation_results =
        android_project.join("vesper-player-kit/build/outputs/androidTest-results/connected/debug");
    let before = snapshot_instrumentation_files(&instrumentation_results)?;
    let mut instrumentation = Command::new(gradle);
    instrumentation
        .arg("-p")
        .arg(&android_project)
        .arg("-Pvesper.player.android.abis=arm64-v8a")
        .arg("-Pandroid.testInstrumentationRunnerArguments.class=io.github.ikaros.vesper.player.android.VesperSubtitleMedia3InstrumentationTest,io.github.ikaros.vesper.player.android.VesperSubtitleSelectionLifecycleInstrumentationTest")
        .arg(":vesper-player-kit:connectedDebugAndroidTest")
        .env("ANDROID_SERIAL", device_id)
        .env(
            "GRADLE_USER_HOME",
            android_project.join(".gradle/gradle-user-home"),
        );
    evidence.run_step(
        "android-device-subtitle-instrumentation",
        &root,
        &mut instrumentation,
        diagnostics,
    )?;
    let started = Instant::now();
    let verification = verify_instrumentation_results(
        &instrumentation_results,
        &evidence.android.join("instrumentation"),
        &before,
    );
    evidence.record_internal_step(
        "android-device-subtitle-instrumentation-evidence",
        verification,
        started,
    )?;

    run_flutter_device_positive(evidence, diagnostics, &flutter, &adb, device_id)
}

fn select_flutter_device(bytes: &[u8], device_id: &str) -> Result<Value, SubtitleError> {
    if bytes.len() > MAX_PREFLIGHT_OUTPUT_BYTES {
        return Err(SubtitleError::conformance(
            "Flutter device inventory exceeds the bounded preflight size",
        ));
    }
    let devices: Value = serde_json::from_slice(bytes).map_err(|error| {
        SubtitleError::conformance(format!("invalid Flutter device inventory JSON: {error}"))
    })?;
    let devices = devices.as_array().ok_or_else(|| {
        SubtitleError::conformance("Flutter device inventory must be a JSON array")
    })?;
    let device = devices
        .iter()
        .find(|device| device.get("id").and_then(Value::as_str) == Some(device_id))
        .ok_or_else(|| {
            SubtitleError::compatibility(format!(
                "Flutter does not expose the requested Android device: {device_id}"
            ))
        })?;
    let target = device
        .get("targetPlatform")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let supported = device
        .get("isSupported")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let emulator = device
        .get("emulator")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if !target.starts_with("android") || !supported || emulator {
        return Err(SubtitleError::compatibility(format!(
            "Flutter device '{device_id}' must be a supported physical Android device"
        )));
    }
    Ok(json!({
        "id": device_id,
        "name": device.get("name").cloned().unwrap_or(Value::Null),
        "targetPlatform": target,
        "emulator": false,
        "sdk": device.get("sdk").cloned().unwrap_or(Value::Null)
    }))
}

fn parse_android_properties(bytes: &[u8]) -> Result<BTreeMap<String, String>, SubtitleError> {
    if bytes.len() > MAX_PREFLIGHT_OUTPUT_BYTES {
        return Err(SubtitleError::conformance(
            "Android device properties exceed the bounded preflight size",
        ));
    }
    let source = std::str::from_utf8(bytes).map_err(|error| {
        SubtitleError::conformance(format!("Android device properties are not UTF-8: {error}"))
    })?;
    let mut properties = BTreeMap::new();
    for line in source.lines() {
        let Some(rest) = line.strip_prefix('[') else {
            continue;
        };
        let Some((name, value)) = rest.split_once("]: [") else {
            continue;
        };
        let Some(value) = value.strip_suffix(']') else {
            continue;
        };
        if properties
            .insert(name.to_owned(), value.to_owned())
            .is_some()
        {
            return Err(SubtitleError::conformance(format!(
                "Android device property is duplicated: {name}"
            )));
        }
    }
    Ok(properties)
}

fn augment_selected_device(
    mut selected: Value,
    properties: &BTreeMap<String, String>,
) -> Result<Value, SubtitleError> {
    let required = |name: &str| {
        properties
            .get(name)
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                SubtitleError::compatibility(format!("Android device property is missing: {name}"))
            })
    };
    let api_level = required("ro.build.version.sdk")?
        .parse::<u32>()
        .map_err(|error| {
            SubtitleError::compatibility(format!("Android device API level is invalid: {error}"))
        })?;
    if api_level < 26 {
        return Err(SubtitleError::compatibility(format!(
            "Android subtitle verification requires API 26 or newer; found API {api_level}"
        )));
    }
    let abi = required("ro.product.cpu.abi")?;
    if abi != ANDROID_ABI {
        return Err(SubtitleError::compatibility(format!(
            "Android subtitle verification requires {ANDROID_ABI}; found {abi}"
        )));
    }
    let object = selected.as_object_mut().ok_or_else(|| {
        SubtitleError::worker("selected Android device metadata is not a JSON object")
    })?;
    object.insert("apiLevel".to_owned(), json!(api_level));
    object.insert("abi".to_owned(), json!(abi));
    for (target, property) in [
        ("manufacturer", "ro.product.manufacturer"),
        ("model", "ro.product.model"),
        ("osVersion", "ro.build.version.release"),
    ] {
        object.insert(target.to_owned(), json!(required(property)?));
    }
    object.insert(
        "build".to_owned(),
        json!({
            "id": required("ro.build.id")?,
            "display": required("ro.build.display.id")?,
            "fingerprint": required("ro.build.fingerprint")?
        }),
    );
    Ok(selected)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InstrumentationFileSnapshot {
    size: u64,
    modified: Option<SystemTime>,
    sha256: String,
}

fn snapshot_instrumentation_files(
    directory: &Path,
) -> Result<BTreeMap<PathBuf, InstrumentationFileSnapshot>, SubtitleError> {
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => {
            return Err(SubtitleError::storage(format!(
                "failed to inspect Android instrumentation results '{}': {error}",
                directory.display()
            )));
        }
    };
    if !metadata.file_type().is_dir() {
        return Err(SubtitleError::conformance(format!(
            "Android instrumentation results '{}' is not a regular directory",
            directory.display()
        )));
    }
    let mut result = BTreeMap::new();
    for path in instrumentation_xml_paths(directory)? {
        result.insert(path.clone(), instrumentation_file_snapshot(&path)?);
    }
    Ok(result)
}

fn instrumentation_xml_paths(directory: &Path) -> Result<Vec<PathBuf>, SubtitleError> {
    let mut paths = Vec::new();
    let mut inspected_entries = 0_usize;
    for entry in fs::read_dir(directory).map_err(|error| {
        SubtitleError::storage(format!(
            "failed to read Android instrumentation results '{}': {error}",
            directory.display()
        ))
    })? {
        inspected_entries += 1;
        if inspected_entries > MAX_EVIDENCE_DIRECTORY_ENTRIES {
            return Err(SubtitleError::conformance(format!(
                "Android instrumentation results contain more than {MAX_EVIDENCE_DIRECTORY_ENTRIES} directory entries"
            )));
        }
        let entry = entry.map_err(|error| {
            SubtitleError::storage(format!(
                "failed to read instrumentation result entry: {error}"
            ))
        })?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("TEST-") || !name.ends_with(".xml") {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
            SubtitleError::storage(format!(
                "failed to inspect instrumentation result '{}': {error}",
                entry.path().display()
            ))
        })?;
        if !metadata.is_file() || metadata.len() > MAX_INSTRUMENTATION_XML_BYTES {
            return Err(SubtitleError::conformance(format!(
                "instrumentation result '{}' is not a bounded regular file",
                entry.path().display()
            )));
        }
        paths.push(entry.path());
        if paths.len() > 64 {
            return Err(SubtitleError::conformance(
                "Android instrumentation produced more than 64 XML result files",
            ));
        }
    }
    paths.sort();
    Ok(paths)
}

fn instrumentation_file_snapshot(
    path: &Path,
) -> Result<InstrumentationFileSnapshot, SubtitleError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        SubtitleError::storage(format!(
            "failed to inspect instrumentation result '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_INSTRUMENTATION_XML_BYTES {
        return Err(SubtitleError::conformance(format!(
            "instrumentation result '{}' is not a bounded regular file",
            path.display()
        )));
    }
    Ok(InstrumentationFileSnapshot {
        size: metadata.len(),
        modified: metadata.modified().ok(),
        sha256: sha256_file(path, MAX_INSTRUMENTATION_XML_BYTES)?,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstrumentationCase {
    name: String,
    class_name: String,
}

fn verify_instrumentation_results(
    results_directory: &Path,
    evidence_directory: &Path,
    before: &BTreeMap<PathBuf, InstrumentationFileSnapshot>,
) -> Result<Vec<u8>, SubtitleError> {
    let paths = instrumentation_xml_paths(results_directory)?;
    let fresh = paths
        .into_iter()
        .map(|path| {
            let snapshot = instrumentation_file_snapshot(&path)?;
            Ok::<_, SubtitleError>(if before.get(&path) == Some(&snapshot) {
                None
            } else {
                Some((path, snapshot))
            })
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if fresh.is_empty() {
        return Err(SubtitleError::conformance(format!(
            "Android instrumentation did not produce fresh XML under '{}'",
            results_directory.display()
        )));
    }
    let expected: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::from([
        (
            "io.github.ikaros.vesper.player.android.VesperSubtitleMedia3InstrumentationTest",
            BTreeSet::from([
                "localDashWebVttIsDiscoveredSelectedAndProducesCue",
                "nativeBindingsPreserveBridgeListenersAcrossReinitialize",
            ]),
        ),
        (
            "io.github.ikaros.vesper.player.android.VesperSubtitleSelectionLifecycleInstrumentationTest",
            BTreeSet::from([
                "newerSubtitleSelectionSupersedesPendingCommandOnTheDevice",
                "pendingSubtitleSelectionTimesOutAgainstTheDeviceClock",
                "sourceSwitchCancelsPendingSubtitleSelectionOnTheDevice",
            ]),
        ),
    ]);
    let mut actual: BTreeMap<String, Vec<InstrumentationCase>> = BTreeMap::new();
    fs::create_dir_all(evidence_directory).map_err(|error| {
        SubtitleError::storage(format!(
            "failed to create instrumentation evidence '{}': {error}",
            evidence_directory.display()
        ))
    })?;
    let mut xml_files = Vec::new();
    for (path, _) in fresh {
        let bytes = read_bounded_file(&path, MAX_INSTRUMENTATION_XML_BYTES)?;
        for case in parse_instrumentation_xml(&bytes, &path)? {
            actual
                .entry(case.class_name.clone())
                .or_default()
                .push(case);
        }
        let name = path.file_name().ok_or_else(|| {
            SubtitleError::conformance("instrumentation XML path has no file name")
        })?;
        let target = evidence_directory.join(name);
        fs::copy(&path, &target).map_err(|error| {
            SubtitleError::storage(format!(
                "failed to copy instrumentation evidence '{}' to '{}': {error}",
                path.display(),
                target.display()
            ))
        })?;
        xml_files.push(name.to_string_lossy().into_owned());
    }
    if actual.len() != expected.len() {
        return Err(SubtitleError::conformance(format!(
            "Android instrumentation suites were {:?}; expected {:?}",
            actual.keys().collect::<Vec<_>>(),
            expected.keys().collect::<Vec<_>>()
        )));
    }
    for (suite, names) in &expected {
        let cases = actual.get(*suite).ok_or_else(|| {
            SubtitleError::conformance(format!(
                "Android instrumentation is missing expected suite {suite}"
            ))
        })?;
        let actual_names = cases
            .iter()
            .map(|case| case.name.as_str())
            .collect::<BTreeSet<_>>();
        if &actual_names != names || cases.len() != names.len() {
            return Err(SubtitleError::conformance(format!(
                "Android instrumentation suite {suite} ran {actual_names:?}; expected {names:?}"
            )));
        }
    }
    xml_files.sort();
    let summary = json!({
        "result": "passed",
        "totalTests": actual.values().map(Vec::len).sum::<usize>(),
        "suites": actual,
        "xmlFiles": xml_files
    });
    let mut bytes = serde_json::to_vec_pretty(&summary).map_err(|error| {
        SubtitleError::worker(format!(
            "failed to serialize instrumentation evidence: {error}"
        ))
    })?;
    bytes.push(b'\n');
    write_file(&evidence_directory.join("summary.json"), &bytes)?;
    Ok(bytes)
}

fn parse_instrumentation_xml(
    bytes: &[u8],
    path: &Path,
) -> Result<Vec<InstrumentationCase>, SubtitleError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut cases = Vec::new();
    let mut active_case: Option<(InstrumentationCase, bool)> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) if event.name().as_ref() == b"testcase" => {
                if active_case.is_some() {
                    return Err(SubtitleError::conformance(format!(
                        "nested testcase in instrumentation XML '{}'",
                        path.display()
                    )));
                }
                active_case = Some((instrumentation_case(&reader, &event, path)?, false));
            }
            Ok(Event::Empty(event)) if event.name().as_ref() == b"testcase" => {
                cases.push(instrumentation_case(&reader, &event, path)?);
            }
            Ok(Event::Start(event))
                if matches!(event.name().as_ref(), b"failure" | b"error" | b"skipped") =>
            {
                if let Some((_, failed)) = active_case.as_mut() {
                    *failed = true;
                }
            }
            Ok(Event::End(event)) if event.name().as_ref() == b"testcase" => {
                let (case, failed) = active_case.take().ok_or_else(|| {
                    SubtitleError::conformance(format!(
                        "unmatched testcase end in instrumentation XML '{}'",
                        path.display()
                    ))
                })?;
                if failed {
                    return Err(SubtitleError::conformance(format!(
                        "Android instrumentation test did not pass: {}.{}",
                        case.class_name, case.name
                    )));
                }
                cases.push(case);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(SubtitleError::conformance(format!(
                    "invalid instrumentation XML '{}': {error}",
                    path.display()
                )));
            }
        }
    }
    if active_case.is_some() || cases.is_empty() {
        return Err(SubtitleError::conformance(format!(
            "instrumentation XML '{}' has incomplete or empty testcases",
            path.display()
        )));
    }
    Ok(cases)
}

fn instrumentation_case(
    reader: &Reader<&[u8]>,
    event: &quick_xml::events::BytesStart<'_>,
    path: &Path,
) -> Result<InstrumentationCase, SubtitleError> {
    let mut name = None;
    let mut class_name = None;
    for attribute in event.attributes() {
        let attribute = attribute.map_err(|error| {
            SubtitleError::conformance(format!(
                "invalid instrumentation XML attribute '{}': {error}",
                path.display()
            ))
        })?;
        let value = attribute
            .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
            .map_err(|error| {
                SubtitleError::conformance(format!(
                    "invalid instrumentation XML value '{}': {error}",
                    path.display()
                ))
            })?
            .into_owned();
        match attribute.key.as_ref() {
            b"name" => name = Some(value),
            b"classname" => class_name = Some(value),
            _ => {}
        }
    }
    Ok(InstrumentationCase {
        name: name.ok_or_else(|| {
            SubtitleError::conformance(format!(
                "instrumentation testcase has no name in '{}'",
                path.display()
            ))
        })?,
        class_name: class_name.ok_or_else(|| {
            SubtitleError::conformance(format!(
                "instrumentation testcase has no classname in '{}'",
                path.display()
            ))
        })?,
    })
}

fn run_flutter_device_positive(
    evidence: &mut EvidenceRun,
    diagnostics: &mut dyn Write,
    flutter: &Path,
    adb: &Path,
    device_id: &str,
) -> Result<(), SubtitleError> {
    let root = evidence.root.clone();
    let project = root.join("examples/flutter-host");
    let output_directory = evidence.flutter.join("device");
    fs::create_dir_all(&output_directory).map_err(|error| {
        SubtitleError::storage(format!(
            "failed to create Flutter subtitle evidence '{}': {error}",
            output_directory.display()
        ))
    })?;

    force_stop_flutter_host(
        evidence,
        diagnostics,
        adb,
        device_id,
        "flutter-device-subtitle-positive-cleanup-before",
    )?;
    let before = capture_adb_forwards(
        evidence,
        diagnostics,
        adb,
        device_id,
        "flutter-device-subtitle-positive-forwards-before",
    )?;

    let mut drive = Command::new(flutter);
    drive
        .args([
            "drive",
            "--no-keep-app-running",
            "--no-dds",
            "--driver=test_driver/subtitle_integration_test.dart",
            "--target=integration_test/subtitle_contract_test.dart",
            "--device-id",
            device_id,
        ])
        .env(
            "GRADLE_USER_HOME",
            project.join("android/.gradle/gradle-user-home"),
        )
        .env("VESPER_SUBTITLE_EVIDENCE_DIR", &output_directory)
        .env("VESPER_SUBTITLE_EVIDENCE_NAME", "subtitle-positive");
    let drive_result = evidence.run_step(
        "flutter-device-subtitle-positive",
        &project,
        &mut drive,
        diagnostics,
    );

    let force_stop_result = force_stop_flutter_host(
        evidence,
        diagnostics,
        adb,
        device_id,
        "flutter-device-subtitle-positive-cleanup-after",
    );
    let forwards_result = cleanup_flutter_adb_forwards(
        evidence,
        diagnostics,
        adb,
        device_id,
        &before,
        "flutter-device-subtitle-positive",
    );
    if let Err(error) = drive_result {
        return Err(match (force_stop_result, forwards_result) {
            (Ok(()), Ok(())) => error,
            (Err(cleanup), Ok(())) | (Ok(()), Err(cleanup)) => error.with_suffix(cleanup),
            (Err(first), Err(second)) => error.with_suffix(format!("{first}; {second}")),
        });
    }
    force_stop_result?;
    forwards_result?;

    let started = Instant::now();
    let verification = verify_flutter_positive_evidence(&output_directory);
    evidence.record_internal_step(
        "flutter-device-subtitle-positive-evidence",
        verification,
        started,
    )?;
    Ok(())
}

fn force_stop_flutter_host(
    evidence: &mut EvidenceRun,
    diagnostics: &mut dyn Write,
    adb: &Path,
    device_id: &str,
    step: &str,
) -> Result<(), SubtitleError> {
    let root = evidence.root.clone();
    let mut command = Command::new(adb);
    command.args([
        "-s",
        device_id,
        "shell",
        "am",
        "force-stop",
        ANDROID_APPLICATION_ID,
    ]);
    evidence.run_step(step, &root, &mut command, diagnostics)?;
    Ok(())
}

fn capture_adb_forwards(
    evidence: &mut EvidenceRun,
    diagnostics: &mut dyn Write,
    adb: &Path,
    device_id: &str,
    step: &str,
) -> Result<BTreeMap<String, String>, SubtitleError> {
    let root = evidence.root.clone();
    let mut command = Command::new(adb);
    command.args(["-s", device_id, "forward", "--list"]);
    let bytes = evidence.run_step(step, &root, &mut command, diagnostics)?;
    parse_adb_forwards(&bytes, device_id)
}

fn parse_adb_forwards(
    bytes: &[u8],
    device_id: &str,
) -> Result<BTreeMap<String, String>, SubtitleError> {
    if bytes.len() > MAX_PREFLIGHT_OUTPUT_BYTES {
        return Err(SubtitleError::conformance(
            "adb forward inventory exceeds the bounded preflight size",
        ));
    }
    let source = std::str::from_utf8(bytes).map_err(|error| {
        SubtitleError::conformance(format!("adb forward inventory is not UTF-8: {error}"))
    })?;
    let mut result = BTreeMap::new();
    for line in source.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.is_empty() || fields[0] != device_id {
            continue;
        }
        if fields.len() != 3 {
            return Err(SubtitleError::conformance(format!(
                "unexpected adb forward record: {line}"
            )));
        }
        if result
            .insert(fields[1].to_owned(), fields[2].to_owned())
            .is_some()
        {
            return Err(SubtitleError::conformance(format!(
                "duplicate adb local forward: {}",
                fields[1]
            )));
        }
    }
    Ok(result)
}

fn cleanup_flutter_adb_forwards(
    evidence: &mut EvidenceRun,
    diagnostics: &mut dyn Write,
    adb: &Path,
    device_id: &str,
    before: &BTreeMap<String, String>,
    drive_step: &str,
) -> Result<(), SubtitleError> {
    let after = capture_adb_forwards(
        evidence,
        diagnostics,
        adb,
        device_id,
        "flutter-device-subtitle-positive-forwards-after",
    )?;
    let log = read_bounded_file(
        &evidence.logs.join(format!("{drive_step}.log")),
        MAX_STEP_STDOUT_BYTES as u64 + MAX_STEP_STDERR_BYTES as u64,
    )?;
    let ports = flutter_vm_service_ports(&log)?;
    for (local, remote) in &after {
        if before.contains_key(local) {
            continue;
        }
        if !ports.contains(local) {
            continue;
        }
        validate_numeric_tcp_forward(local)?;
        validate_numeric_tcp_forward(remote)?;
        let root = evidence.root.clone();
        let mut remove = Command::new(adb);
        remove.args(["-s", device_id, "forward", "--remove", local]);
        evidence.run_step(
            &format!(
                "flutter-device-subtitle-positive-forward-remove-{}",
                &local[4..]
            ),
            &root,
            &mut remove,
            diagnostics,
        )?;
    }
    let final_state = capture_adb_forwards(
        evidence,
        diagnostics,
        adb,
        device_id,
        "flutter-device-subtitle-positive-forwards-final",
    )?;
    if &final_state != before {
        return Err(SubtitleError::conformance(format!(
            "ADB forwards changed during Flutter drive. Before={before:?} Final={final_state:?}"
        )));
    }
    Ok(())
}

fn flutter_vm_service_ports(bytes: &[u8]) -> Result<BTreeSet<String>, SubtitleError> {
    let source = std::str::from_utf8(bytes).map_err(|error| {
        SubtitleError::conformance(format!("Flutter drive log is not UTF-8: {error}"))
    })?;
    let marker = "VMServiceFlutterDriver: Connecting to Flutter application at http://127.0.0.1:";
    let mut ports = BTreeSet::new();
    for suffix in source.split(marker).skip(1) {
        let port = suffix
            .bytes()
            .take_while(u8::is_ascii_digit)
            .collect::<Vec<_>>();
        if port.is_empty() {
            continue;
        }
        let port = String::from_utf8(port).map_err(|error| {
            SubtitleError::worker(format!("failed to parse Flutter VM service port: {error}"))
        })?;
        ports.insert(format!("tcp:{port}"));
    }
    Ok(ports)
}

fn validate_numeric_tcp_forward(value: &str) -> Result<(), SubtitleError> {
    let port = value.strip_prefix("tcp:").ok_or_else(|| {
        SubtitleError::worker(format!(
            "refusing to remove unexpected adb forward endpoint: {value}"
        ))
    })?;
    if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(SubtitleError::worker(format!(
            "refusing to remove invalid adb forward endpoint: {value}"
        )));
    }
    Ok(())
}

fn verify_flutter_positive_evidence(directory: &Path) -> Result<Vec<u8>, SubtitleError> {
    let json_path = directory.join("subtitle-positive.json");
    let bytes = read_required_flutter_evidence_file(&json_path, MAX_EVIDENCE_JSON_BYTES)?;
    let payload: Value = serde_json::from_slice(&bytes).map_err(|error| {
        SubtitleError::conformance(format!(
            "invalid Flutter subtitle evidence '{}': {error}",
            json_path.display()
        ))
    })?;
    if payload.get("evidenceName").and_then(Value::as_str) != Some("subtitle-positive") {
        return Err(SubtitleError::conformance(
            "Flutter subtitle evidence has an unexpected evidenceName",
        ));
    }
    let snapshot = payload.get("snapshot").ok_or_else(|| {
        SubtitleError::conformance("Flutter subtitle evidence is missing snapshot")
    })?;
    let frame = snapshot
        .get("frame")
        .ok_or_else(|| SubtitleError::conformance("Flutter subtitle evidence is missing frame"))?;
    let positive_number = |value: Option<&Value>| {
        value
            .and_then(Value::as_f64)
            .is_some_and(|value| value > 0.0)
    };
    if snapshot.get("text").and_then(Value::as_str) != Some("Subtitle B")
        || snapshot.get("visible").and_then(Value::as_bool) != Some(true)
        || snapshot.get("hidden").and_then(Value::as_bool) != Some(false)
        || snapshot.get("windowAttached").and_then(Value::as_bool) != Some(true)
        || !positive_number(snapshot.get("alpha"))
        || !positive_number(frame.get("width"))
        || !positive_number(frame.get("height"))
    {
        return Err(SubtitleError::conformance(
            "Flutter subtitle evidence does not prove a visible attached Subtitle B overlay",
        ));
    }
    if payload.get("pngFile").and_then(Value::as_str) != Some("subtitle-positive.png") {
        return Err(SubtitleError::conformance(
            "Flutter subtitle evidence does not declare subtitle-positive.png",
        ));
    }
    let png_path = directory.join("subtitle-positive.png");
    let png = read_required_flutter_evidence_file(&png_path, 64 * 1024 * 1024)?;
    if !png.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(SubtitleError::conformance(format!(
            "invalid Flutter subtitle evidence PNG: {}",
            png_path.display()
        )));
    }
    Ok(b"Verified Flutter subtitle-positive JSON and PNG evidence.\n".to_vec())
}

fn reject_existing_path(path: &Path, label: &str) -> Result<(), SubtitleError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(SubtitleError::usage(format!(
            "{label} already exists: {}",
            path.display()
        ))),
        Err(error) => Err(SubtitleError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))),
    }
}

fn capture_text(
    command: &mut Command,
    label: &str,
    maximum_bytes: usize,
) -> Result<String, SubtitleError> {
    let output = capture_command(command, label, maximum_bytes, maximum_bytes)?;
    if !output.status.success() {
        return Err(process_status_error(label, output.status));
    }
    let text = String::from_utf8(output.stdout).map_err(|error| {
        SubtitleError::conformance(format!("{label} returned non-UTF-8 output: {error}"))
    })?;
    let text = text.trim();
    if text.is_empty() {
        return Err(SubtitleError::conformance(format!(
            "{label} returned empty output"
        )));
    }
    Ok(text.to_owned())
}

fn capture_command(
    command: &mut Command,
    label: &str,
    stdout_maximum_bytes: usize,
    stderr_maximum_bytes: usize,
) -> Result<external_process::BoundedProcessOutput, SubtitleError> {
    external_process::run_interruptible_capture(
        command,
        label,
        stdout_maximum_bytes,
        stderr_maximum_bytes,
    )
    .map_err(map_process_error)
}

fn map_process_error(error: external_process::ExternalProcessError) -> SubtitleError {
    match error.kind() {
        external_process::ExternalProcessErrorKind::Compatibility => {
            SubtitleError::compatibility(error.to_string())
        }
        external_process::ExternalProcessErrorKind::Cancelled
        | external_process::ExternalProcessErrorKind::Worker => {
            SubtitleError::worker(error.to_string())
        }
    }
}

fn process_status_error(label: &str, status: ExitStatus) -> SubtitleError {
    let message = format!("{label} failed with status {status}");
    match status.code() {
        None | Some(6) => SubtitleError::worker(message),
        Some(2) => SubtitleError::usage(message),
        Some(3) => SubtitleError::storage(message),
        Some(4) => SubtitleError::compatibility(message),
        Some(5) => SubtitleError::conformance(message),
        _ => SubtitleError::conformance(message),
    }
}

fn map_gradle_error(error: gradle::GradleError) -> SubtitleError {
    match error.kind() {
        gradle::GradleErrorKind::Storage => SubtitleError::storage(error.to_string()),
        gradle::GradleErrorKind::Compatibility => SubtitleError::compatibility(error.to_string()),
    }
}

fn map_android_error(error: android::AndroidError) -> SubtitleError {
    match error.kind() {
        android::AndroidErrorKind::Usage => SubtitleError::usage(error.to_string()),
        android::AndroidErrorKind::Storage => SubtitleError::storage(error.to_string()),
        android::AndroidErrorKind::Compatibility => SubtitleError::compatibility(error.to_string()),
        android::AndroidErrorKind::Conformance => SubtitleError::conformance(error.to_string()),
        android::AndroidErrorKind::Worker => SubtitleError::worker(error.to_string()),
    }
}

fn diagnostic_error(error: io::Error) -> SubtitleError {
    SubtitleError::storage(format!("failed to write subtitle diagnostics: {error}"))
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), SubtitleError> {
    let mut file = File::create(path).map_err(|error| {
        SubtitleError::storage(format!("failed to create '{}': {error}", path.display()))
    })?;
    file.write_all(bytes).map_err(|error| {
        SubtitleError::storage(format!("failed to write '{}': {error}", path.display()))
    })?;
    file.flush().map_err(|error| {
        SubtitleError::storage(format!("failed to flush '{}': {error}", path.display()))
    })
}

fn read_bounded_file(path: &Path, maximum_bytes: u64) -> Result<Vec<u8>, SubtitleError> {
    read_bounded_file_with_policy(path, maximum_bytes, false)
}

fn read_required_flutter_evidence_file(
    path: &Path,
    maximum_bytes: u64,
) -> Result<Vec<u8>, SubtitleError> {
    read_bounded_file_with_policy(path, maximum_bytes, true)
}

fn read_bounded_file_with_policy(
    path: &Path,
    maximum_bytes: u64,
    missing_is_conformance: bool,
) -> Result<Vec<u8>, SubtitleError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        if missing_is_conformance && error.kind() == io::ErrorKind::NotFound {
            SubtitleError::conformance(format!(
                "required Flutter subtitle evidence is missing: {}",
                path.display()
            ))
        } else {
            SubtitleError::storage(format!("failed to inspect '{}': {error}", path.display()))
        }
    })?;
    if !metadata.file_type().is_file() || metadata.len() > maximum_bytes {
        return Err(SubtitleError::conformance(format!(
            "'{}' is not a bounded regular non-symlink file",
            path.display()
        )));
    }
    let mut file = File::open(path).map_err(|error| {
        if missing_is_conformance && error.kind() == io::ErrorKind::NotFound {
            SubtitleError::conformance(format!(
                "required Flutter subtitle evidence is missing: {}",
                path.display()
            ))
        } else {
            SubtitleError::storage(format!("failed to open '{}': {error}", path.display()))
        }
    })?;
    let mut bytes = Vec::with_capacity(metadata.len().min(maximum_bytes) as usize);
    Read::by_ref(&mut file)
        .take(maximum_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            SubtitleError::storage(format!("failed to read '{}': {error}", path.display()))
        })?;
    if bytes.len() as u64 > maximum_bytes {
        return Err(SubtitleError::conformance(format!(
            "'{}' exceeds {maximum_bytes} bytes",
            path.display()
        )));
    }
    Ok(bytes)
}

fn sha256_file(path: &Path, maximum_bytes: u64) -> Result<String, SubtitleError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        SubtitleError::storage(format!("failed to inspect '{}': {error}", path.display()))
    })?;
    if !metadata.file_type().is_file() || metadata.len() > maximum_bytes {
        return Err(SubtitleError::conformance(format!(
            "'{}' is not a bounded regular non-symlink file",
            path.display()
        )));
    }
    let mut file = File::open(path).map_err(|error| {
        SubtitleError::storage(format!("failed to open '{}': {error}", path.display()))
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let count = file.read(&mut buffer).map_err(|error| {
            SubtitleError::storage(format!("failed to hash '{}': {error}", path.display()))
        })?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or_else(|| SubtitleError::conformance("subtitle evidence file size overflowed"))?;
        if total > maximum_bytes {
            return Err(SubtitleError::conformance(format!(
                "'{}' exceeds {maximum_bytes} bytes while hashing",
                path.display()
            )));
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn write_evidence_checksums(root: &Path) -> Result<(), SubtitleError> {
    let checksum_path = root.join("SHA256SUMS");
    let mut pending = VecDeque::from([(root.to_path_buf(), 0_usize)]);
    let mut files = Vec::new();
    let mut total_bytes = 0_u64;
    let mut inspected_entries = 0_usize;
    let mut inspected_directories = 0_usize;
    while let Some((directory, depth)) = pending.pop_front() {
        inspected_directories += 1;
        if inspected_directories > MAX_EVIDENCE_FILES {
            return Err(SubtitleError::conformance(format!(
                "subtitle evidence tree contains more than {MAX_EVIDENCE_FILES} directories"
            )));
        }
        if depth > MAX_EVIDENCE_DEPTH {
            return Err(SubtitleError::conformance(format!(
                "subtitle evidence tree exceeds depth {MAX_EVIDENCE_DEPTH}"
            )));
        }
        for entry in fs::read_dir(&directory).map_err(|error| {
            SubtitleError::storage(format!(
                "failed to inspect subtitle evidence '{}': {error}",
                directory.display()
            ))
        })? {
            inspected_entries += 1;
            if inspected_entries > MAX_EVIDENCE_DIRECTORY_ENTRIES {
                return Err(SubtitleError::conformance(format!(
                    "subtitle evidence tree contains more than {MAX_EVIDENCE_DIRECTORY_ENTRIES} directory entries"
                )));
            }
            let entry = entry.map_err(|error| {
                SubtitleError::storage(format!("failed to read subtitle evidence entry: {error}"))
            })?;
            let path = entry.path();
            if path == checksum_path {
                continue;
            }
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                SubtitleError::storage(format!(
                    "failed to inspect subtitle evidence '{}': {error}",
                    path.display()
                ))
            })?;
            if metadata.file_type().is_dir() {
                pending.push_back((path, depth + 1));
            } else if metadata.file_type().is_file() {
                if metadata.len() > MAX_EVIDENCE_FILE_BYTES {
                    return Err(SubtitleError::conformance(format!(
                        "subtitle evidence file '{}' exceeds {MAX_EVIDENCE_FILE_BYTES} bytes",
                        path.display()
                    )));
                }
                total_bytes = total_bytes.checked_add(metadata.len()).ok_or_else(|| {
                    SubtitleError::conformance("subtitle evidence size overflowed")
                })?;
                if total_bytes > MAX_EVIDENCE_FILE_BYTES * 2 {
                    return Err(SubtitleError::conformance(
                        "subtitle evidence tree exceeds the 2 GiB total size limit",
                    ));
                }
                files.push(path);
                if files.len() > MAX_EVIDENCE_FILES {
                    return Err(SubtitleError::conformance(format!(
                        "subtitle evidence tree contains more than {MAX_EVIDENCE_FILES} files"
                    )));
                }
            } else {
                return Err(SubtitleError::conformance(format!(
                    "subtitle evidence contains symlink or special file '{}'",
                    path.display()
                )));
            }
        }
    }
    let mut indexed = files
        .into_iter()
        .map(|path| evidence_relative_path(root, &path).map(|relative| (relative, path)))
        .collect::<Result<Vec<_>, _>>()?;
    indexed.sort_by(|left, right| left.0.cmp(&right.0));
    let mut output = String::new();
    for (relative, path) in indexed {
        let hash = sha256_file(&path, MAX_EVIDENCE_FILE_BYTES)?;
        output.push_str(&format!("{hash}  ./{relative}\n"));
    }
    write_file(&checksum_path, output.as_bytes())
}

fn evidence_relative_path(root: &Path, path: &Path) -> Result<String, SubtitleError> {
    let relative = path.strip_prefix(root).map_err(|error| {
        SubtitleError::worker(format!(
            "subtitle evidence path '{}' escaped '{}': {error}",
            path.display(),
            root.display()
        ))
    })?;
    let mut components = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(value) => {
                let value = value.to_str().ok_or_else(|| {
                    SubtitleError::conformance(format!(
                        "subtitle evidence path is not UTF-8: {}",
                        path.display()
                    ))
                })?;
                if value.contains('\n') || value.contains('\r') {
                    return Err(SubtitleError::conformance(format!(
                        "subtitle evidence path contains a line break: {}",
                        path.display()
                    )));
                }
                components.push(value);
            }
            _ => {
                return Err(SubtitleError::conformance(format!(
                    "subtitle evidence path is not canonical: {}",
                    path.display()
                )));
            }
        }
    }
    Ok(components.join("/"))
}

fn display_command(command: &Command) -> String {
    std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|value| format!("{:?}", value.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn require_command(name: &str, message: &str) -> Result<PathBuf, SubtitleError> {
    resolve_command(name).ok_or_else(|| SubtitleError::compatibility(message))
}

fn resolve_command(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    let current_directory = env::current_dir().ok()?;
    resolve_command_in_path(name, &path, &current_directory)
}

fn resolve_command_in_path(name: &str, path: &OsStr, current_directory: &Path) -> Option<PathBuf> {
    for directory in env::split_paths(&path) {
        let directory = if directory.as_os_str().is_empty() {
            current_directory.to_path_buf()
        } else if directory.is_absolute() {
            directory
        } else {
            current_directory.join(directory)
        };
        for candidate in command_candidates(&directory, name) {
            let Ok(metadata) = fs::metadata(&candidate) else {
                continue;
            };
            if metadata.is_file() && can_execute(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn command_candidates(directory: &Path, name: &str) -> Vec<PathBuf> {
    #[cfg(not(windows))]
    let candidates = vec![directory.join(name)];
    #[cfg(windows)]
    let candidates = [".exe", ".cmd", ".bat"]
        .into_iter()
        .map(|extension| directory.join(format!("{name}{extension}")))
        .collect();
    candidates
}

#[cfg(unix)]
fn can_execute(path: &Path) -> bool {
    use nix::unistd::{AccessFlags, access};

    access(path, AccessFlags::X_OK).is_ok()
}

#[cfg(not(unix))]
fn can_execute(_path: &Path) -> bool {
    true
}

struct UtcTimestamp {
    iso8601: String,
    compact: String,
}

impl UtcTimestamp {
    fn now() -> Result<Self, SubtitleError> {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| {
                SubtitleError::worker(format!("system clock predates Unix epoch: {error}"))
            })?
            .as_secs();
        let days = (seconds / 86_400) as i64;
        let seconds_of_day = seconds % 86_400;
        let (year, month, day) = civil_from_days(days);
        let hour = seconds_of_day / 3_600;
        let minute = (seconds_of_day % 3_600) / 60;
        let second = seconds_of_day % 60;
        Ok(Self {
            iso8601: format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"),
            compact: format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z"),
        })
    }
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i64, i64, i64) {
    let shifted = days_since_unix_epoch + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subtitle::SubtitleErrorKind;

    #[test]
    fn subtitle_scope_requires_a_physical_device_only_for_device_runs() {
        assert!(
            validate_request(&SubtitleRequest {
                scope: SubtitleScope::Regression,
                device_id: None,
                evidence_directory: None,
            })
            .is_ok()
        );
        assert_eq!(
            validate_request(&SubtitleRequest {
                scope: SubtitleScope::Regression,
                device_id: Some("device-serial".to_owned()),
                evidence_directory: None,
            })
            .expect_err("regression must reject a device id")
            .kind(),
            SubtitleErrorKind::Usage
        );
        assert_eq!(
            validate_request(&SubtitleRequest {
                scope: SubtitleScope::Device,
                device_id: None,
                evidence_directory: None,
            })
            .expect_err("device scope must require a device id")
            .kind(),
            SubtitleErrorKind::Usage
        );
    }

    #[test]
    fn missing_flutter_evidence_is_a_conformance_failure() {
        let directory = tempfile::tempdir().expect("create Flutter evidence fixture");
        let error = verify_flutter_positive_evidence(directory.path())
            .expect_err("missing Flutter evidence must fail");

        assert_eq!(error.kind(), SubtitleErrorKind::Conformance);
        assert_eq!(error.exit_code(), 5);
    }

    #[test]
    fn evidence_finalization_materializes_the_complete_v1_artifact_contract() {
        let directory = tempfile::tempdir().expect("create evidence fixture");
        let evidence_directory = directory.path().join("evidence");
        fs::create_dir(&evidence_directory).expect("create evidence directory");
        for child in [
            "logs",
            "preflight",
            "xcresult",
            "flutter",
            "xctest-attachments",
            "android",
        ] {
            fs::create_dir(evidence_directory.join(child)).expect("create artifact directory");
        }
        fs::write(evidence_directory.join("source-status.txt"), b"").expect("write source status");
        fs::write(evidence_directory.join("source-sha.txt"), b"fixture-sha\n")
            .expect("write source sha");
        fs::write(evidence_directory.join("toolchain.txt"), b"fixture\n").expect("write toolchain");
        let evidence = EvidenceRun {
            root: directory.path().to_path_buf(),
            directory: evidence_directory.clone(),
            logs: evidence_directory.join("logs"),
            preflight: evidence_directory.join("preflight"),
            flutter: evidence_directory.join("flutter"),
            android: evidence_directory.join("android"),
            source_sha: "fixture-sha".to_owned(),
            source_dirty: false,
            run_id: "fixture-run".to_owned(),
            started_at: "2026-08-05T00:00:00Z".to_owned(),
            selected_device: None,
            steps: vec![StepRecord {
                name: "fixture-step".to_owned(),
                result: "passed",
                duration_seconds: 1,
                log: "logs/fixture-step.log".to_owned(),
            }],
        };
        fs::write(evidence.logs.join("fixture-step.log"), b"passed\n").expect("write step log");

        evidence
            .finalize(
                &SubtitleRequest {
                    scope: SubtitleScope::Regression,
                    device_id: None,
                    evidence_directory: Some(evidence_directory.clone()),
                },
                0,
            )
            .expect("finalize evidence");

        let manifest: Value = serde_json::from_slice(
            &fs::read(evidence_directory.join("manifest.json")).expect("read manifest"),
        )
        .expect("parse manifest");
        assert_eq!(manifest["schema"], "vesper-subtitle-evidence-v1");
        let artifacts = manifest["artifacts"].as_object().expect("artifact map");
        for artifact in artifacts.values() {
            let relative = artifact.as_str().expect("artifact path");
            assert!(
                evidence_directory.join(relative).exists(),
                "missing declared artifact: {relative}"
            );
        }
        assert_eq!(
            fs::read_to_string(evidence_directory.join("steps.tsv")).expect("read step ledger"),
            "fixture-step\tpassed\t1\tlogs/fixture-step.log\n"
        );
        let checksums =
            fs::read_to_string(evidence_directory.join("SHA256SUMS")).expect("read checksums");
        assert!(checksums.contains("  ./manifest.json\n"));
        assert!(checksums.contains("  ./steps.tsv\n"));
    }

    #[cfg(unix)]
    #[test]
    fn command_resolution_accepts_executable_symlink_and_preserves_shim_name() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let directory = tempfile::tempdir().expect("create command-resolution fixture");
        let proxy = directory.path().join("tool-proxy");
        fs::write(&proxy, b"#!/bin/sh\nprintf '%s\\n' \"$0\"\n").expect("write executable proxy");
        let mut permissions = fs::metadata(&proxy)
            .expect("inspect executable proxy")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&proxy, permissions).expect("make proxy executable");
        let shim = directory.path().join("rustc");
        symlink("tool-proxy", &shim).expect("create relative executable shim");

        let resolved =
            resolve_command_in_path("rustc", directory.path().as_os_str(), Path::new("/"))
                .expect("resolve executable shim");
        assert_eq!(resolved, shim);
        let output = Command::new(&resolved)
            .output()
            .expect("run executable shim by its preserved name");
        assert!(output.status.success());
        assert_eq!(
            std::str::from_utf8(&output.stdout)
                .expect("shim output is UTF-8")
                .trim(),
            resolved.to_string_lossy()
        );
    }

    #[test]
    fn adb_forward_parser_is_device_scoped_and_rejects_duplicates() {
        let parsed = parse_adb_forwards(
            b"other-device tcp:1000 tcp:2000\nphysical tcp:1001 tcp:2001\n",
            "physical",
        )
        .expect("parse forwards for the selected device");
        assert_eq!(parsed.get("tcp:1001"), Some(&"tcp:2001".to_owned()));
        assert!(!parsed.contains_key("tcp:1000"));

        let duplicate = parse_adb_forwards(
            b"physical tcp:1001 tcp:2001\nphysical tcp:1001 tcp:2002\n",
            "physical",
        )
        .expect_err("duplicate local forwards must be rejected");
        assert_eq!(duplicate.kind(), SubtitleErrorKind::Conformance);
    }

    #[test]
    fn instrumentation_verifier_does_not_ignore_malformed_fresh_xml() {
        let directory = tempfile::tempdir().expect("create instrumentation fixture");
        let results = directory.path().join("results");
        let evidence = directory.path().join("evidence");
        fs::create_dir(&results).expect("create instrumentation results");
        fs::write(
            results.join("TEST-malformed.xml"),
            b"<testsuite><testcase name=\"broken\" classname=\"suite\">",
        )
        .expect("write malformed instrumentation XML");

        let error = verify_instrumentation_results(&results, &evidence, &BTreeMap::new())
            .expect_err("malformed fresh XML must fail closed");
        assert_eq!(error.kind(), SubtitleErrorKind::Conformance);
        assert!(
            error.to_string().contains("instrumentation XML"),
            "unexpected malformed XML error: {error}"
        );
    }

    #[test]
    fn instrumentation_verifier_rejects_an_unchanged_snapshot() {
        let directory = tempfile::tempdir().expect("create instrumentation fixture");
        let results = directory.path().join("results");
        let evidence = directory.path().join("evidence");
        fs::create_dir(&results).expect("create instrumentation results");
        let xml = b"<testsuite><testcase name=\"localDashWebVttIsDiscoveredSelectedAndProducesCue\" classname=\"io.github.ikaros.vesper.player.android.VesperSubtitleMedia3InstrumentationTest\"/></testsuite>";
        let path = results.join("TEST-suite.xml");
        fs::write(&path, xml).expect("write instrumentation XML");
        let before = snapshot_instrumentation_files(&results).expect("snapshot results");

        let error = verify_instrumentation_results(&results, &evidence, &before)
            .expect_err("unchanged XML must not count as fresh evidence");
        assert_eq!(error.kind(), SubtitleErrorKind::Conformance);
        assert!(error.to_string().contains("did not produce fresh XML"));
    }

    #[cfg(unix)]
    #[test]
    fn instrumentation_result_symlinks_are_rejected_before_parsing() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().expect("create instrumentation fixture");
        let results = directory.path().join("results");
        fs::create_dir(&results).expect("create instrumentation results");
        let source = directory.path().join("source.xml");
        fs::write(&source, b"<testsuite/>").expect("write source XML");
        symlink(&source, results.join("TEST-link.xml")).expect("create result symlink");

        let error = instrumentation_xml_paths(&results)
            .expect_err("instrumentation symlinks must be rejected");
        assert_eq!(error.kind(), SubtitleErrorKind::Conformance);
    }

    #[test]
    fn numeric_tcp_forward_validation_is_strict() {
        assert!(validate_numeric_tcp_forward("tcp:1234").is_ok());
        for value in ["tcp:", "tcp:12x", "local:1234", "tcp:-1"] {
            assert_eq!(
                validate_numeric_tcp_forward(value)
                    .expect_err("invalid forward endpoint")
                    .kind(),
                SubtitleErrorKind::Worker
            );
        }
    }
}
