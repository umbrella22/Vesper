use std::collections::{BTreeMap, VecDeque};
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::external_process;
use crate::subtitle::{SubtitleError, SubtitleErrorKind, SubtitleScope};

const MAX_STEP_STDOUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_STEP_STDERR_BYTES: usize = 64 * 1024 * 1024;
const MAX_PREFLIGHT_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_EVIDENCE_FILES: usize = 4096;
const MAX_EVIDENCE_DIRECTORY_ENTRIES: usize = 16_384;
const MAX_EVIDENCE_DEPTH: usize = 32;
const MAX_EVIDENCE_FILE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_JSON_BYTES: u64 = 8 * 1024 * 1024;
const MAX_PNG_BYTES: u64 = 64 * 1024 * 1024;
const MAX_XCRESULT_NODES: usize = 100_000;
const MAX_XCRESULT_DEPTH: usize = 64;
const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(2 * 60);
const PROJECT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const BUILD_TIMEOUT: Duration = Duration::from_secs(45 * 60);
const TEST_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const FLUTTER_DRIVE_TIMEOUT: Duration = Duration::from_secs(20 * 60);

#[derive(Debug)]
pub(crate) struct IosSubtitleRequest {
    pub(crate) scope: SubtitleScope,
    pub(crate) device_id: Option<String>,
    pub(crate) simulator_id: Option<String>,
    pub(crate) evidence_directory: Option<PathBuf>,
    pub(crate) development_team: Option<String>,
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
    xcresult: PathBuf,
    flutter: PathBuf,
    attachments: PathBuf,
    _android: PathBuf,
    _temporary: tempfile::TempDir,
    source_sha: String,
    source_dirty: bool,
    run_id: String,
    started_at: String,
    selected_device: Option<Value>,
    selected_simulator_id: Option<String>,
    steps: Vec<StepRecord>,
    projects_generated: bool,
    flutter_dependencies_ready: bool,
}

pub(crate) fn verify(
    root: &Path,
    request: IosSubtitleRequest,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> Result<(), SubtitleError> {
    validate_request(&request)?;
    ensure_supported_host()?;
    let mut evidence = EvidenceRun::create(root, &request)?;
    let run_result = (|| {
        collect_toolchain(&mut evidence)?;
        if request.scope.includes_regression() {
            run_regression(&mut evidence, &request, diagnostics)?;
        }
        if request.scope.includes_device() {
            run_device(&mut evidence, &request, diagnostics)?;
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

fn ensure_supported_host() -> Result<(), SubtitleError> {
    if cfg!(target_os = "macos") {
        Ok(())
    } else {
        Err(SubtitleError::compatibility(
            "iOS subtitle verification requires macOS",
        ))
    }
}

fn validate_request(request: &IosSubtitleRequest) -> Result<(), SubtitleError> {
    match (request.scope, request.device_id.as_deref()) {
        (SubtitleScope::Regression, Some(_)) => {
            return Err(SubtitleError::usage(
                "--device is not used by iOS subtitle scope 'regression'",
            ));
        }
        (SubtitleScope::Device | SubtitleScope::Complete, None) => {
            return Err(SubtitleError::usage(format!(
                "--device is required for iOS subtitle scope '{}'",
                request.scope.as_str()
            )));
        }
        _ => {}
    }
    if matches!(request.scope, SubtitleScope::Device) && request.simulator_id.is_some() {
        return Err(SubtitleError::usage(
            "--simulator is not used by iOS subtitle scope 'device'",
        ));
    }
    if let Some(device) = request.device_id.as_deref() {
        validate_opaque_identifier(device, "iOS device")?;
    }
    if let Some(simulator) = request.simulator_id.as_deref() {
        validate_opaque_identifier(simulator, "iOS Simulator")?;
    }
    if request.scope.includes_device() {
        let team = request.development_team.as_deref().ok_or_else(|| {
            SubtitleError::usage(
                "--development-team or VESPER_IOS_DEVELOPMENT_TEAM is required for physical iOS subtitle verification",
            )
        })?;
        validate_opaque_identifier(team, "iOS development team")?;
    }
    Ok(())
}

fn validate_opaque_identifier(value: &str, label: &str) -> Result<(), SubtitleError> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(SubtitleError::usage(format!(
            "{label} identifier must be non-empty, at most 256 bytes, trimmed, and contain no control characters"
        )));
    }
    Ok(())
}

impl EvidenceRun {
    fn create(root: &Path, request: &IosSubtitleRequest) -> Result<Self, SubtitleError> {
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
            PREFLIGHT_TIMEOUT,
        )?;
        let short_sha = capture_text(
            Command::new(&git).arg("-C").arg(&canonical_root).args([
                "rev-parse",
                "--short=12",
                "HEAD",
            ]),
            "subtitle short source revision",
            MAX_PREFLIGHT_OUTPUT_BYTES,
            PREFLIGHT_TIMEOUT,
        )?;
        let now = UtcTimestamp::now()?;
        let run_id = format!("{}-{short_sha}", now.compact);
        let requested = request.evidence_directory.clone().unwrap_or_else(|| {
            canonical_root
                .join("devnotes/evidence/subtitle/ios")
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
        let xcresult = directory.join("xcresult");
        let flutter = directory.join("flutter");
        let attachments = directory.join("xctest-attachments");
        let android = directory.join("android");
        for child in [
            &logs,
            &preflight,
            &xcresult,
            &flutter,
            &attachments,
            &android,
        ] {
            fs::create_dir(child).map_err(|error| {
                SubtitleError::storage(format!(
                    "failed to create subtitle evidence directory '{}': {error}",
                    child.display()
                ))
            })?;
        }
        write_file(&directory.join("steps.tsv"), b"")?;
        let status = capture_command(
            Command::new(&git)
                .arg("-C")
                .arg(&canonical_root)
                .args(["status", "--short"]),
            "subtitle source status",
            MAX_PREFLIGHT_OUTPUT_BYTES,
            MAX_PREFLIGHT_OUTPUT_BYTES,
            PREFLIGHT_TIMEOUT,
        )?;
        if !status.status.success() {
            return Err(process_status_error(
                "subtitle source status",
                status.status,
                SubtitleErrorKind::Conformance,
            ));
        }
        write_file(&directory.join("source-status.txt"), &status.stdout)?;
        write_file(
            &directory.join("source-sha.txt"),
            format!("{source_sha}\n").as_bytes(),
        )?;
        let temporary = tempfile::Builder::new()
            .prefix("vesper-subtitle-ios-")
            .tempdir()
            .map_err(|error| {
                SubtitleError::storage(format!(
                    "failed to create iOS subtitle temporary directory: {error}"
                ))
            })?;
        Ok(Self {
            root: canonical_root,
            directory,
            logs,
            preflight,
            xcresult,
            flutter,
            attachments,
            _android: android,
            _temporary: temporary,
            source_sha,
            source_dirty: !status.stdout.is_empty(),
            run_id,
            started_at: now.iso8601,
            selected_device: None,
            selected_simulator_id: None,
            steps: Vec::new(),
            projects_generated: false,
            flutter_dependencies_ready: false,
        })
    }

    fn run_step(
        &mut self,
        name: &str,
        working_directory: &Path,
        command: &mut Command,
        timeout: Duration,
        failure_kind: SubtitleErrorKind,
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
            "Working directory: {}\nCommand: {}\nTimeout seconds: {}\n\n",
            working_directory.display(),
            display_command(command),
            timeout.as_secs()
        );
        let result = external_process::run_interruptible_capture_with_timeout(
            command,
            &format!("subtitle verification step {name}"),
            MAX_STEP_STDOUT_BYTES,
            MAX_STEP_STDERR_BYTES,
            timeout,
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
                    Err(process_status_error(name, captured.status, failure_kind))
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

    fn finalize(&self, request: &IosSubtitleRequest, exit_code: i32) -> Result<(), SubtitleError> {
        let finished_at = UtcTimestamp::now()?.iso8601;
        let result = if exit_code == 0 { "passed" } else { "failed" };
        let mut summary = String::from("# Vesper Subtitle Verification\n\n");
        for (label, value) in [
            ("Result", result.to_owned()),
            ("Platform", "ios".to_owned()),
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
            (
                "Simulator",
                self.selected_simulator_id
                    .clone()
                    .or_else(|| request.simulator_id.clone())
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
        let simulator_id = self
            .selected_simulator_id
            .as_ref()
            .or(request.simulator_id.as_ref());
        let manifest = json!({
            "schema": "vesper-subtitle-evidence-v1",
            "result": result,
            "exitCode": exit_code,
            "platform": "ios",
            "scope": request.scope.as_str(),
            "runId": self.run_id,
            "sourceSha": self.source_sha,
            "sourceDirty": self.source_dirty,
            "startedAt": self.started_at,
            "finishedAt": finished_at,
            "deviceId": request.device_id,
            "simulatorId": simulator_id,
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
    let result = collect_toolchain_output();
    let bytes = evidence.record_internal_step("toolchain", result, started)?;
    write_file(&evidence.directory.join("toolchain.txt"), &bytes)
}

fn collect_toolchain_output() -> Result<Vec<u8>, SubtitleError> {
    let mut output = Vec::new();
    for (label, program, arguments) in [
        ("uname", "uname", vec!["-a"]),
        ("git", "git", vec!["--version"]),
        ("rustc", "rustc", vec!["--version"]),
        ("cargo", "cargo", vec!["--version"]),
        ("flutter", "flutter", vec!["--version"]),
        ("xcodebuild", "xcodebuild", vec!["-version"]),
        ("swiftc", "swiftc", vec!["--version"]),
        ("devicectl", "xcrun", vec!["devicectl", "--version"]),
    ] {
        let executable = require_command(
            program,
            &format!("{program} is required for iOS subtitle verification"),
        )?;
        let captured = capture_command(
            Command::new(executable).args(arguments),
            &format!("{label} toolchain probe"),
            MAX_PREFLIGHT_OUTPUT_BYTES,
            MAX_PREFLIGHT_OUTPUT_BYTES,
            PREFLIGHT_TIMEOUT,
        )?;
        if !captured.status.success() {
            return Err(process_status_error(
                &format!("{label} toolchain probe"),
                captured.status,
                SubtitleErrorKind::Compatibility,
            ));
        }
        output.extend_from_slice(format!("{label}:\n").as_bytes());
        output.extend_from_slice(&captured.stdout);
        if !captured.stderr.is_empty() {
            output.extend_from_slice(&captured.stderr);
        }
        if !output.ends_with(b"\n") {
            output.push(b'\n');
        }
        output.push(b'\n');
    }
    Ok(output)
}

fn run_regression(
    evidence: &mut EvidenceRun,
    request: &IosSubtitleRequest,
    diagnostics: &mut dyn Write,
) -> Result<(), SubtitleError> {
    let root = evidence.root.clone();
    let current_cli = current_cli()?;

    evidence.run_step(
        "contract-verify",
        &root,
        Command::new(&current_cli)
            .args(["contract", "--root"])
            .arg(&root)
            .arg("verify"),
        TEST_TIMEOUT,
        SubtitleErrorKind::Conformance,
        diagnostics,
    )?;
    evidence.run_step(
        "ios-rust-subtitle-tests",
        &root,
        Command::new(require_command(
            "cargo",
            "cargo is required for iOS subtitle verification",
        )?)
        .args([
            "test",
            "-p",
            "player-ffi",
            "-p",
            "player-ffi-ios",
            "-p",
            "player-platform-ios",
        ]),
        TEST_TIMEOUT,
        SubtitleErrorKind::Conformance,
        diagnostics,
    )?;
    evidence.run_step(
        "ios-simulator-ffi",
        &root,
        Command::new(&current_cli)
            .args(["ios", "--root"])
            .arg(&root)
            .args(["ffi", "debug", "--platform", "simulator"]),
        BUILD_TIMEOUT,
        SubtitleErrorKind::Conformance,
        diagnostics,
    )?;

    prepare_ios_projects(evidence, diagnostics)?;
    prepare_flutter_dependencies(evidence, diagnostics)?;
    prepare_ios_simulator(evidence, request.simulator_id.as_deref(), diagnostics)?;
    let simulator = evidence.selected_simulator_id.clone().ok_or_else(|| {
        SubtitleError::worker("iOS subtitle regression did not select a Simulator")
    })?;
    let result_bundle = evidence.xcresult.join("ios-simulator.xcresult");
    let derived_data = evidence._temporary.path().join("ios-simulator-derived");
    let destination = format!("platform=iOS Simulator,id={simulator}");
    evidence.run_step(
        "ios-simulator-xctest",
        &root,
        Command::new(require_command(
            "xcodebuild",
            "xcodebuild is required for iOS subtitle verification",
        )?)
        .args([
            "test",
            "-project",
            "lib/ios/VesperPlayerKit/VesperPlayerKit.xcodeproj",
            "-scheme",
            "VesperPlayerKit",
            "-configuration",
            "Debug",
            "-destination",
        ])
        .arg(destination)
        .arg("-resultBundlePath")
        .arg(&result_bundle)
        .arg("-derivedDataPath")
        .arg(&derived_data)
        .args([
            "CODE_SIGNING_ALLOWED=NO",
            "CODE_SIGNING_REQUIRED=NO",
            "-only-testing:VesperPlayerKitTests/VesperNativeSubtitleStateTests",
            "-only-testing:VesperPlayerKitTests/VesperSubtitleOverlayRendererTests",
        ]),
        TEST_TIMEOUT,
        SubtitleErrorKind::Conformance,
        diagnostics,
    )?;
    verify_xcresult(
        evidence,
        "ios-simulator",
        &result_bundle,
        &[
            ("VesperNativeSubtitleStateTests", 40),
            ("VesperSubtitleOverlayRendererTests", 10),
        ],
        diagnostics,
    )?;

    run_flutter_tests(evidence, diagnostics)?;
    run_flutter_integration(
        evidence,
        &simulator,
        "simulator",
        "subtitle-positive",
        "integration_test/subtitle_contract_test.dart",
        None,
        diagnostics,
    )?;
    run_flutter_integration(
        evidence,
        &simulator,
        "simulator",
        "subtitle-lifecycle",
        "integration_test/subtitle_lifecycle_test.dart",
        None,
        diagnostics,
    )
}

fn current_cli() -> Result<PathBuf, SubtitleError> {
    env::current_exe().map_err(|error| {
        SubtitleError::storage(format!("failed to resolve the current Vesper CLI: {error}"))
    })
}

fn prepare_ios_projects(
    evidence: &mut EvidenceRun,
    diagnostics: &mut dyn Write,
) -> Result<(), SubtitleError> {
    if evidence.projects_generated {
        return Ok(());
    }
    let xcodegen = require_command(
        "xcodegen",
        "xcodegen is required for iOS subtitle verification",
    )?;
    let player_kit = evidence.root.join("lib/ios/VesperPlayerKit");
    evidence.run_step(
        "ios-player-kit-xcodegen",
        &player_kit,
        Command::new(&xcodegen).arg("generate"),
        PROJECT_TIMEOUT,
        SubtitleErrorKind::Conformance,
        diagnostics,
    )?;
    let host = evidence.root.join("examples/ios-swift-host");
    evidence.run_step(
        "ios-host-xcodegen",
        &host,
        Command::new(&xcodegen).arg("generate"),
        PROJECT_TIMEOUT,
        SubtitleErrorKind::Conformance,
        diagnostics,
    )?;
    evidence.projects_generated = true;
    Ok(())
}

fn prepare_flutter_dependencies(
    evidence: &mut EvidenceRun,
    diagnostics: &mut dyn Write,
) -> Result<(), SubtitleError> {
    if evidence.flutter_dependencies_ready {
        return Ok(());
    }
    let flutter = require_command(
        "flutter",
        "Flutter is required for iOS subtitle verification",
    )?;
    let host = evidence.root.join("examples/flutter-host");
    evidence.run_step(
        "flutter-host-pub-get",
        &host,
        Command::new(flutter).args(["pub", "get"]),
        PROJECT_TIMEOUT,
        SubtitleErrorKind::Conformance,
        diagnostics,
    )?;
    evidence.flutter_dependencies_ready = true;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct SimctlInventory {
    devices: BTreeMap<String, Vec<SimctlDevice>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SimctlDevice {
    udid: String,
    name: String,
    state: String,
    #[serde(default = "default_true")]
    is_available: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SimulatorSelection {
    id: String,
    name: String,
    state: String,
    is_available: bool,
    runtime_identifier: String,
    os_version: String,
    #[serde(skip)]
    version: [u64; 3],
}

const fn default_true() -> bool {
    true
}

fn prepare_ios_simulator(
    evidence: &mut EvidenceRun,
    requested: Option<&str>,
    diagnostics: &mut dyn Write,
) -> Result<(), SubtitleError> {
    let root = evidence.root.clone();
    let xcrun = require_command("xcrun", "xcrun is required for iOS subtitle verification")?;
    let inventory = evidence.run_step(
        "ios-simctl-devices",
        &root,
        Command::new(&xcrun).args(["simctl", "list", "devices", "available", "--json"]),
        PREFLIGHT_TIMEOUT,
        SubtitleErrorKind::Compatibility,
        diagnostics,
    )?;
    write_file(&evidence.preflight.join("simctl-devices.json"), &inventory)?;
    let started = Instant::now();
    let selected_result = select_simulator(&inventory, requested);
    evidence.record_internal_step(
        "ios-simulator-select",
        selected_result
            .as_ref()
            .map(|selected| {
                format!(
                    "selected={} name={} state={} os={}\n",
                    selected.id, selected.name, selected.state, selected.os_version
                )
                .into_bytes()
            })
            .map_err(|error| SubtitleError::conformance(error.to_string())),
        started,
    )?;
    let selected = selected_result?;
    write_json(
        &evidence.preflight.join("selected-simulator.json"),
        &selected,
    )?;
    evidence.selected_simulator_id = Some(selected.id.clone());
    if selected.state != "Booted" {
        evidence.run_step(
            "ios-simulator-boot",
            &root,
            Command::new(&xcrun).args(["simctl", "boot", &selected.id]),
            PROJECT_TIMEOUT,
            SubtitleErrorKind::Compatibility,
            diagnostics,
        )?;
    }
    evidence.run_step(
        "ios-simulator-bootstatus",
        &root,
        Command::new(&xcrun).args(["simctl", "bootstatus", &selected.id, "-b"]),
        PROJECT_TIMEOUT,
        SubtitleErrorKind::Compatibility,
        diagnostics,
    )?;

    let flutter = require_command(
        "flutter",
        "Flutter is required for iOS subtitle verification",
    )?;
    let flutter_host = root.join("examples/flutter-host");
    let devices = evidence.run_step(
        "ios-flutter-simulator-devices",
        &flutter_host,
        Command::new(flutter).args(["devices", "--machine"]),
        PREFLIGHT_TIMEOUT,
        SubtitleErrorKind::Compatibility,
        diagnostics,
    )?;
    write_file(
        &evidence.preflight.join("flutter-devices-simulator.json"),
        &devices,
    )?;
    let started = Instant::now();
    let match_result = verify_flutter_device(&devices, &selected.id, true, "Simulator");
    evidence.record_internal_step(
        "ios-flutter-simulator-match",
        match_result.map(|()| format!("matched={}\n", selected.id).into_bytes()),
        started,
    )?;

    let destinations = evidence.run_step(
        "ios-xcode-simulator-destinations",
        &root,
        Command::new(require_command(
            "xcodebuild",
            "xcodebuild is required for iOS subtitle verification",
        )?)
        .args([
            "-project",
            "examples/ios-swift-host/VesperPlayerHostDemo.xcodeproj",
            "-scheme",
            "VesperPlayerHostDemo",
            "-showdestinations",
        ]),
        PREFLIGHT_TIMEOUT,
        SubtitleErrorKind::Compatibility,
        diagnostics,
    )?;
    write_file(
        &evidence.preflight.join("xcode-destinations-simulator.txt"),
        &destinations,
    )?;
    let started = Instant::now();
    let destination_result = verify_xcode_destination(&destinations, &selected.id);
    evidence.record_internal_step(
        "ios-xcode-simulator-destination-match",
        destination_result.map(|()| format!("matched={}\n", selected.id).into_bytes()),
        started,
    )?;
    Ok(())
}

fn select_simulator(
    bytes: &[u8],
    requested: Option<&str>,
) -> Result<SimulatorSelection, SubtitleError> {
    if bytes.len() as u64 > MAX_JSON_BYTES {
        return Err(SubtitleError::conformance(
            "simctl device inventory exceeds the JSON size limit",
        ));
    }
    let inventory: SimctlInventory = serde_json::from_slice(bytes).map_err(|error| {
        SubtitleError::conformance(format!("failed to parse simctl device inventory: {error}"))
    })?;
    let mut candidates = Vec::new();
    for (runtime, devices) in inventory.devices {
        let Some(version) = parse_ios_runtime_version(&runtime) else {
            continue;
        };
        if version < [17, 0, 0] {
            continue;
        }
        for device in devices {
            if !device.is_available || !device.name.starts_with("iPhone") {
                continue;
            }
            let os_version = if version[2] == 0 {
                format!("{}.{}", version[0], version[1])
            } else {
                format!("{}.{}.{}", version[0], version[1], version[2])
            };
            candidates.push(SimulatorSelection {
                id: device.udid,
                name: device.name,
                state: device.state,
                is_available: device.is_available,
                runtime_identifier: runtime.clone(),
                os_version,
                version,
            });
        }
    }
    let selected = match requested {
        Some(id) => candidates.into_iter().find(|candidate| candidate.id == id),
        None => candidates.into_iter().max_by_key(|candidate| {
            (
                candidate.state == "Booted",
                candidate.version,
                candidate.id.clone(),
            )
        }),
    };
    selected.ok_or_else(|| {
        SubtitleError::compatibility(
            "no matching available iPhone Simulator with iOS 17 or newer was found",
        )
    })
}

fn parse_ios_runtime_version(identifier: &str) -> Option<[u64; 3]> {
    let (_, version) = identifier.rsplit_once(".SimRuntime.iOS-")?;
    let mut values = [0_u64; 3];
    let mut count = 0_usize;
    for component in version.split('-') {
        if count >= values.len() || component.is_empty() {
            return None;
        }
        values[count] = component.parse().ok()?;
        count += 1;
    }
    (count > 0).then_some(values)
}

fn verify_flutter_device(
    bytes: &[u8],
    id: &str,
    emulator: bool,
    label: &str,
) -> Result<(), SubtitleError> {
    if bytes.len() as u64 > MAX_JSON_BYTES {
        return Err(SubtitleError::conformance(
            "Flutter device inventory exceeds the JSON size limit",
        ));
    }
    let devices: Value = serde_json::from_slice(bytes).map_err(|error| {
        SubtitleError::conformance(format!("failed to parse Flutter device inventory: {error}"))
    })?;
    let devices = devices.as_array().ok_or_else(|| {
        SubtitleError::conformance("Flutter device inventory must be a JSON array")
    })?;
    let matched = devices.iter().any(|device| {
        device.get("id").and_then(Value::as_str) == Some(id)
            && device.get("emulator").and_then(Value::as_bool) == Some(emulator)
            && device
                .get("targetPlatform")
                .and_then(Value::as_str)
                .is_some_and(|platform| platform.starts_with("ios"))
            && device.get("isSupported").and_then(Value::as_bool) == Some(true)
    });
    if matched {
        Ok(())
    } else {
        Err(SubtitleError::compatibility(format!(
            "Flutter does not expose the requested iOS {label}: {id}"
        )))
    }
}

fn verify_xcode_destination(bytes: &[u8], id: &str) -> Result<(), SubtitleError> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        SubtitleError::conformance(format!("Xcode destination output is not UTF-8: {error}"))
    })?;
    let matched = text.split("id:").skip(1).any(|suffix| {
        let candidate = suffix
            .split(|character: char| {
                character == ',' || character == '}' || character.is_whitespace()
            })
            .next()
            .unwrap_or_default();
        candidate == id
    });
    if matched {
        Ok(())
    } else {
        Err(SubtitleError::compatibility(format!(
            "Xcode does not expose the requested iOS destination: {id}"
        )))
    }
}

fn verify_xcresult(
    evidence: &mut EvidenceRun,
    prefix: &str,
    result_bundle: &Path,
    expected_suites: &[(&str, usize)],
    diagnostics: &mut dyn Write,
) -> Result<(), SubtitleError> {
    validate_xcresult_bundle(result_bundle)?;
    let root = evidence.root.clone();
    let xcrun = require_command("xcrun", "xcrun is required for XCResult verification")?;
    let summary = evidence.run_step(
        &format!("{prefix}-xcresult-summary"),
        &root,
        Command::new(&xcrun)
            .args(["xcresulttool", "get", "test-results", "summary", "--path"])
            .arg(result_bundle),
        PROJECT_TIMEOUT,
        SubtitleErrorKind::Conformance,
        diagnostics,
    )?;
    let tests = evidence.run_step(
        &format!("{prefix}-xcresult-tests"),
        &root,
        Command::new(&xcrun)
            .args(["xcresulttool", "get", "test-results", "tests", "--path"])
            .arg(result_bundle),
        PROJECT_TIMEOUT,
        SubtitleErrorKind::Conformance,
        diagnostics,
    )?;
    let summary_path = evidence.xcresult.join(format!("{prefix}-summary.json"));
    let tests_path = evidence.xcresult.join(format!("{prefix}-tests.json"));
    write_file(&summary_path, &summary)?;
    write_file(&tests_path, &tests)?;
    let started = Instant::now();
    let result = verify_xcresult_payloads(&summary, &tests, expected_suites);
    evidence.record_internal_step(
        &format!("{prefix}-xcresult-evidence"),
        result.map(|counts| {
            counts
                .into_iter()
                .map(|(suite, count)| format!("{suite}={count}\n"))
                .collect::<String>()
                .into_bytes()
        }),
        started,
    )?;
    Ok(())
}

fn validate_xcresult_bundle(path: &Path) -> Result<(), SubtitleError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        SubtitleError::conformance(format!(
            "missing XCResult bundle '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_dir() {
        return Err(SubtitleError::conformance(format!(
            "XCResult bundle is not a regular non-symlink directory: {}",
            path.display()
        )));
    }
    let info = path.join("Info.plist");
    let bytes = read_bounded_file(&info, 1024 * 1024)?;
    if bytes.is_empty() {
        return Err(SubtitleError::conformance(format!(
            "XCResult bundle has an empty Info.plist: {}",
            path.display()
        )));
    }
    Ok(())
}

fn verify_xcresult_payloads(
    summary: &[u8],
    tests: &[u8],
    expected_suites: &[(&str, usize)],
) -> Result<Vec<(String, usize)>, SubtitleError> {
    if summary.len() as u64 > MAX_JSON_BYTES || tests.len() as u64 > MAX_JSON_BYTES {
        return Err(SubtitleError::conformance(
            "XCResult JSON exceeds the size limit",
        ));
    }
    let summary: Value = serde_json::from_slice(summary).map_err(|error| {
        SubtitleError::conformance(format!("failed to parse XCResult summary: {error}"))
    })?;
    if summary.get("result").and_then(Value::as_str) != Some("Passed")
        || summary
            .get("failedTests")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            != 0
        || summary
            .get("totalTestCount")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            == 0
    {
        return Err(SubtitleError::conformance("XCResult did not pass"));
    }
    let tests: Value = serde_json::from_slice(tests).map_err(|error| {
        SubtitleError::conformance(format!("failed to parse XCResult test tree: {error}"))
    })?;
    let roots = tests
        .get("testNodes")
        .and_then(Value::as_array)
        .ok_or_else(|| SubtitleError::conformance("XCResult test tree is missing testNodes"))?;
    let mut nodes = Vec::new();
    let mut pending = roots
        .iter()
        .rev()
        .map(|node| (node, 0_usize))
        .collect::<Vec<_>>();
    while let Some((node, depth)) = pending.pop() {
        if depth > MAX_XCRESULT_DEPTH {
            return Err(SubtitleError::conformance(
                "XCResult test tree exceeds the depth limit",
            ));
        }
        nodes.push(node);
        if nodes.len() > MAX_XCRESULT_NODES {
            return Err(SubtitleError::conformance(
                "XCResult test tree exceeds the node limit",
            ));
        }
        let children = node
            .get("children")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        pending.extend(children.iter().rev().map(|child| (child, depth + 1)));
    }
    let mut counts = Vec::new();
    for (suite_name, minimum_count) in expected_suites {
        let suites = nodes
            .iter()
            .copied()
            .filter(|node| {
                node.get("nodeType").and_then(Value::as_str) == Some("Test Suite")
                    && node.get("name").and_then(Value::as_str) == Some(*suite_name)
            })
            .collect::<Vec<_>>();
        if suites.len() != 1 {
            return Err(SubtitleError::conformance(format!(
                "XCResult expected exactly one test suite named {suite_name}; found {}",
                suites.len()
            )));
        }
        let suite = suites[0];
        let mut suite_nodes = Vec::new();
        let mut stack = vec![(suite, 0_usize)];
        while let Some((node, depth)) = stack.pop() {
            if depth > MAX_XCRESULT_DEPTH || suite_nodes.len() > MAX_XCRESULT_NODES {
                return Err(SubtitleError::conformance(format!(
                    "XCResult suite exceeds traversal limits: {suite_name}"
                )));
            }
            suite_nodes.push(node);
            let children = node
                .get("children")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            stack.extend(children.iter().rev().map(|child| (child, depth + 1)));
        }
        let cases = suite_nodes
            .iter()
            .copied()
            .filter(|node| node.get("nodeType").and_then(Value::as_str) == Some("Test Case"))
            .collect::<Vec<_>>();
        if cases.len() < *minimum_count {
            return Err(SubtitleError::conformance(format!(
                "XCResult suite executed {} tests; expected at least {minimum_count}: {suite_name}",
                cases.len()
            )));
        }
        if suite.get("result").and_then(Value::as_str) != Some("Passed")
            || cases
                .iter()
                .any(|case| case.get("result").and_then(Value::as_str) != Some("Passed"))
        {
            return Err(SubtitleError::conformance(format!(
                "XCResult suite did not pass: {suite_name}"
            )));
        }
        counts.push(((*suite_name).to_owned(), cases.len()));
    }
    Ok(counts)
}

fn run_flutter_tests(
    evidence: &mut EvidenceRun,
    diagnostics: &mut dyn Write,
) -> Result<(), SubtitleError> {
    let flutter = require_command(
        "flutter",
        "Flutter is required for iOS subtitle verification",
    )?;
    for (name, directory, arguments) in [
        (
            "flutter-platform-subtitle-tests",
            "lib/flutter/vesper_player_platform_interface",
            vec![
                "test",
                "test/subtitle_exception_test.dart",
                "test/subtitle_state_models_test.dart",
            ],
        ),
        (
            "flutter-controller-subtitle-tests",
            "lib/flutter/vesper_player",
            vec!["test", "test/vesper_download_manager_test.dart"],
        ),
        (
            "flutter-ios-channel-tests",
            "lib/flutter/vesper_player_ios",
            vec!["test", "test/method_channel_vesper_player_ios_test.dart"],
        ),
        (
            "flutter-host-subtitle-evidence-test",
            "examples/flutter-host",
            vec!["test", "test/subtitle_overlay_evidence_test.dart"],
        ),
    ] {
        let working_directory = evidence.root.join(directory);
        evidence.run_step(
            name,
            &working_directory,
            Command::new(&flutter).args(arguments),
            TEST_TIMEOUT,
            SubtitleErrorKind::Conformance,
            diagnostics,
        )?;
    }
    Ok(())
}

fn run_flutter_integration(
    evidence: &mut EvidenceRun,
    target_device: &str,
    target_kind: &str,
    evidence_name: &str,
    test_target: &str,
    development_team: Option<&str>,
    diagnostics: &mut dyn Write,
) -> Result<(), SubtitleError> {
    let output_directory = evidence.flutter.join(target_kind);
    fs::create_dir_all(&output_directory).map_err(|error| {
        SubtitleError::storage(format!(
            "failed to create Flutter subtitle evidence directory '{}': {error}",
            output_directory.display()
        ))
    })?;
    let flutter = require_command(
        "flutter",
        "Flutter is required for iOS subtitle verification",
    )?;
    let host = evidence.root.join("examples/flutter-host");
    let mut command = Command::new(flutter);
    command
        .env("VESPER_SUBTITLE_EVIDENCE_DIR", &output_directory)
        .env("VESPER_SUBTITLE_EVIDENCE_NAME", evidence_name)
        .args([
            "drive",
            "--driver=test_driver/subtitle_integration_test.dart",
        ])
        .arg(format!("--target={test_target}"))
        .args(["--device-id", target_device]);
    if target_kind == "device" {
        command.args(["--no-keep-app-running", "--device-connection", "attached"]);
        if let Some(team) = development_team {
            command.env("DEVELOPMENT_TEAM", team);
        }
    }
    let drive_result = if target_kind == "device" {
        let temporary = evidence._temporary.path().to_path_buf();
        let started = Instant::now();
        let cleanup = cleanup_ios_flutter_host(
            target_device,
            &temporary,
            &format!("{evidence_name}-before"),
        );
        evidence.record_internal_step(
            &format!("flutter-{target_kind}-{evidence_name}-cleanup-before"),
            cleanup,
            started,
        )?;
        let drive = evidence.run_step(
            &format!("flutter-{target_kind}-{evidence_name}"),
            &host,
            &mut command,
            FLUTTER_DRIVE_TIMEOUT,
            SubtitleErrorKind::Conformance,
            diagnostics,
        );
        let started = Instant::now();
        let cleanup =
            cleanup_ios_flutter_host(target_device, &temporary, &format!("{evidence_name}-after"));
        let cleanup = evidence.record_internal_step(
            &format!("flutter-{target_kind}-{evidence_name}-cleanup-after"),
            cleanup,
            started,
        );
        match (drive, cleanup) {
            (Ok(bytes), Ok(_)) => Ok(bytes),
            (Err(error), Ok(_)) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Err(error), Err(cleanup_error)) => Err(error.with_suffix(cleanup_error)),
        }
    } else {
        evidence.run_step(
            &format!("flutter-{target_kind}-{evidence_name}"),
            &host,
            &mut command,
            FLUTTER_DRIVE_TIMEOUT,
            SubtitleErrorKind::Conformance,
            diagnostics,
        )
    };
    drive_result?;
    let started = Instant::now();
    let result = verify_flutter_evidence(&output_directory, evidence_name);
    evidence.record_internal_step(
        &format!("flutter-{target_kind}-{evidence_name}-evidence"),
        result.map(|()| format!("verified={evidence_name}\n").into_bytes()),
        started,
    )?;
    Ok(())
}

fn cleanup_ios_flutter_host(
    device_id: &str,
    temporary: &Path,
    evidence_name: &str,
) -> Result<Vec<u8>, SubtitleError> {
    const BUNDLE_ID: &str = "io.github.umbrella22.vesper.example.flutterhost";
    let xcrun = require_command("xcrun", "xcrun is required for iOS process cleanup")?;
    let apps_path = temporary.join(format!("ios-flutter-apps-{evidence_name}.json"));
    let processes_path = temporary.join(format!("ios-flutter-processes-{evidence_name}.json"));
    run_json_output_command(
        Command::new(&xcrun)
            .args([
                "devicectl",
                "device",
                "info",
                "apps",
                "--device",
                device_id,
                "--json-output",
            ])
            .arg(&apps_path)
            .arg("--quiet"),
        "iOS Flutter host app inventory",
        PREFLIGHT_TIMEOUT,
    )?;
    run_json_output_command(
        Command::new(&xcrun)
            .args([
                "devicectl",
                "device",
                "info",
                "processes",
                "--device",
                device_id,
                "--json-output",
            ])
            .arg(&processes_path)
            .arg("--quiet"),
        "iOS Flutter host process inventory",
        PREFLIGHT_TIMEOUT,
    )?;
    let apps = read_required_evidence(&apps_path, MAX_JSON_BYTES, "iOS app inventory")?;
    let processes =
        read_required_evidence(&processes_path, MAX_JSON_BYTES, "iOS process inventory")?;
    let Some((executable, pids)) = flutter_host_processes(&apps, &processes, BUNDLE_ID)? else {
        return Ok(b"No running iOS Flutter host process requires cleanup.\n".to_vec());
    };
    let mut log = Vec::new();
    for pid in pids {
        let current_path = temporary.join(format!(
            "ios-flutter-processes-{evidence_name}-{pid}-current.json"
        ));
        run_json_output_command(
            Command::new(&xcrun)
                .args([
                    "devicectl",
                    "device",
                    "info",
                    "processes",
                    "--device",
                    device_id,
                    "--json-output",
                ])
                .arg(&current_path)
                .arg("--quiet"),
            "current iOS Flutter host process inventory",
            PREFLIGHT_TIMEOUT,
        )?;
        let current = read_required_evidence(
            &current_path,
            MAX_JSON_BYTES,
            "current iOS process inventory",
        )?;
        match process_executable_for_pid(&current, pid)? {
            None => {
                log.extend_from_slice(
                    format!("iOS Flutter host process {pid} exited before cleanup.\n").as_bytes(),
                );
                continue;
            }
            Some(current_executable) if current_executable != executable => {
                return Err(SubtitleError::conformance(format!(
                    "refusing to terminate reused PID {pid}: {current_executable}"
                )));
            }
            Some(_) => {}
        }
        let captured = capture_command(
            Command::new(&xcrun).args([
                "devicectl",
                "device",
                "process",
                "terminate",
                "--device",
                device_id,
                "--pid",
                &pid.to_string(),
                "--timeout",
                "15",
            ]),
            "iOS Flutter host process termination",
            MAX_PREFLIGHT_OUTPUT_BYTES,
            MAX_PREFLIGHT_OUTPUT_BYTES,
            PREFLIGHT_TIMEOUT,
        )?;
        if !captured.status.success() {
            return Err(process_status_error(
                "iOS Flutter host process termination",
                captured.status,
                SubtitleErrorKind::Worker,
            ));
        }
        log.extend_from_slice(
            format!("Terminated iOS Flutter host process: bundle={BUNDLE_ID} pid={pid}\n")
                .as_bytes(),
        );
    }
    Ok(log)
}

fn run_json_output_command(
    command: &mut Command,
    label: &str,
    timeout: Duration,
) -> Result<(), SubtitleError> {
    let captured = capture_command(
        command,
        label,
        MAX_PREFLIGHT_OUTPUT_BYTES,
        MAX_PREFLIGHT_OUTPUT_BYTES,
        timeout,
    )?;
    if captured.status.success() {
        Ok(())
    } else {
        Err(process_status_error(
            label,
            captured.status,
            SubtitleErrorKind::Worker,
        ))
    }
}

fn flutter_host_processes(
    apps: &[u8],
    processes: &[u8],
    bundle_id: &str,
) -> Result<Option<(String, Vec<u64>)>, SubtitleError> {
    let apps: Value = serde_json::from_slice(apps).map_err(|error| {
        SubtitleError::conformance(format!("failed to parse iOS app inventory: {error}"))
    })?;
    let processes: Value = serde_json::from_slice(processes).map_err(|error| {
        SubtitleError::conformance(format!("failed to parse iOS process inventory: {error}"))
    })?;
    let matches = apps
        .pointer("/result/apps")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter(|app| app.get("bundleIdentifier").and_then(Value::as_str) == Some(bundle_id))
        .collect::<Vec<_>>();
    if matches.len() > 1 {
        return Err(SubtitleError::conformance(format!(
            "multiple installed apps reported bundle identifier {bundle_id}"
        )));
    }
    let Some(app) = matches.first() else {
        return Ok(None);
    };
    let app_url = app.get("url").and_then(Value::as_str).ok_or_else(|| {
        SubtitleError::conformance(format!("installed app {bundle_id} is missing its URL"))
    })?;
    const APP_PREFIX: &str = "file:///private/var/containers/Bundle/Application/";
    if !app_url.starts_with(APP_PREFIX) || !app_url.ends_with(".app/") {
        return Err(SubtitleError::conformance(format!(
            "refusing to terminate a process for unexpected app URL: {app_url}"
        )));
    }
    let app_directory = app_url
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .ok_or_else(|| SubtitleError::conformance("installed app URL has no bundle name"))?;
    let executable_name = app_directory.strip_suffix(".app").ok_or_else(|| {
        SubtitleError::conformance("installed app URL has an invalid bundle suffix")
    })?;
    let executable = format!("{app_url}{executable_name}");
    let mut pids = Vec::new();
    for process in processes
        .pointer("/result/runningProcesses")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        if process.get("executable").and_then(Value::as_str) != Some(executable.as_str()) {
            continue;
        }
        let pid = process
            .get("processIdentifier")
            .and_then(Value::as_u64)
            .filter(|pid| *pid > 0)
            .ok_or_else(|| {
                SubtitleError::conformance(format!("invalid process identifier for {bundle_id}"))
            })?;
        pids.push(pid);
    }
    Ok(Some((executable, pids)))
}

fn process_executable_for_pid(
    processes: &[u8],
    expected_pid: u64,
) -> Result<Option<String>, SubtitleError> {
    let payload: Value = serde_json::from_slice(processes).map_err(|error| {
        SubtitleError::conformance(format!(
            "failed to parse current iOS process inventory: {error}"
        ))
    })?;
    let process = payload
        .pointer("/result/runningProcesses")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .find(|process| {
            process.get("processIdentifier").and_then(Value::as_u64) == Some(expected_pid)
        });
    process
        .map(|process| {
            process
                .get("executable")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| {
                    SubtitleError::conformance(format!(
                        "iOS process {expected_pid} is missing its executable"
                    ))
                })
        })
        .transpose()
}

fn verify_flutter_evidence(directory: &Path, evidence_name: &str) -> Result<(), SubtitleError> {
    let json_path = directory.join(format!("{evidence_name}.json"));
    let bytes =
        read_required_evidence(&json_path, MAX_JSON_BYTES, "Flutter subtitle evidence JSON")?;
    let payload: Value = serde_json::from_slice(&bytes).map_err(|error| {
        SubtitleError::conformance(format!(
            "failed to parse Flutter subtitle evidence '{}': {error}",
            json_path.display()
        ))
    })?;
    if payload.get("evidenceName").and_then(Value::as_str) != Some(evidence_name) {
        return Err(SubtitleError::conformance(format!(
            "unexpected Flutter subtitle evidence name in '{}'",
            json_path.display()
        )));
    }
    match evidence_name {
        "subtitle-positive" => {
            verify_visible_subtitle_snapshot(&payload, "Flutter subtitle evidence")?;
            let expected_png = format!("{evidence_name}.png");
            if payload.get("pngFile").and_then(Value::as_str) != Some(&expected_png) {
                return Err(SubtitleError::conformance(
                    "Flutter subtitle evidence did not declare the expected PNG",
                ));
            }
            verify_png(
                &directory.join(expected_png),
                "Flutter subtitle evidence PNG",
            )
        }
        "subtitle-lifecycle" => {
            let scenarios = payload
                .get("scenarios")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    SubtitleError::conformance(
                        "Flutter subtitle lifecycle evidence is missing scenarios",
                    )
                })?;
            for (name, code) in [
                ("timeout", "subtitle_selection_timeout"),
                ("sourceChange", "subtitle_source_changed"),
                ("supersede", "subtitle_selection_superseded"),
            ] {
                let error = scenarios
                    .get(name)
                    .and_then(|scenario| scenario.get("error"))
                    .ok_or_else(|| {
                        SubtitleError::conformance(format!(
                            "Flutter subtitle lifecycle evidence is missing {name}.error"
                        ))
                    })?;
                if error.get("code").and_then(Value::as_str) != Some(code)
                    || error.get("commandId").and_then(Value::as_u64).unwrap_or(0) == 0
                    || error.get("sourceEpoch").and_then(Value::as_i64).is_none()
                {
                    return Err(SubtitleError::conformance(format!(
                        "invalid Flutter subtitle lifecycle evidence for {name}"
                    )));
                }
            }
            Ok(())
        }
        _ => Err(SubtitleError::worker(format!(
            "unsupported Flutter subtitle evidence name: {evidence_name}"
        ))),
    }
}

fn verify_visible_subtitle_snapshot(payload: &Value, label: &str) -> Result<(), SubtitleError> {
    let snapshot = payload.get("snapshot").unwrap_or(payload);
    let frame = snapshot
        .get("frame")
        .ok_or_else(|| SubtitleError::conformance(format!("{label} is missing its frame")))?;
    let alpha = snapshot.get("alpha").and_then(Value::as_f64).unwrap_or(0.0);
    let width = frame.get("width").and_then(Value::as_f64).unwrap_or(0.0);
    let height = frame.get("height").and_then(Value::as_f64).unwrap_or(0.0);
    if snapshot.get("text").and_then(Value::as_str) != Some("Subtitle B")
        || snapshot.get("visible").and_then(Value::as_bool) != Some(true)
        || snapshot.get("hidden").and_then(Value::as_bool) != Some(false)
        || snapshot.get("windowAttached").and_then(Value::as_bool) != Some(true)
        || !alpha.is_finite()
        || alpha <= 0.0
        || !width.is_finite()
        || width <= 0.0
        || !height.is_finite()
        || height <= 0.0
    {
        return Err(SubtitleError::conformance(format!(
            "{label} is not visibly attached with Subtitle B"
        )));
    }
    Ok(())
}

fn verify_png(path: &Path, label: &str) -> Result<(), SubtitleError> {
    let bytes = read_required_evidence(path, MAX_PNG_BYTES, label)?;
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(SubtitleError::conformance(format!(
            "invalid {label}: {}",
            path.display()
        )));
    }
    Ok(())
}

fn read_required_evidence(
    path: &Path,
    maximum_bytes: u64,
    label: &str,
) -> Result<Vec<u8>, SubtitleError> {
    match read_bounded_file(path, maximum_bytes) {
        Ok(bytes) if !bytes.is_empty() => Ok(bytes),
        Ok(_) => Err(SubtitleError::conformance(format!(
            "{label} is empty: {}",
            path.display()
        ))),
        Err(error) if error.kind() == SubtitleErrorKind::Storage => Err(
            SubtitleError::conformance(format!("missing {label}: {}; {error}", path.display())),
        ),
        Err(error) => Err(error),
    }
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), SubtitleError> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| {
        SubtitleError::worker(format!("failed to serialize '{}': {error}", path.display()))
    })?;
    bytes.push(b'\n');
    write_file(path, &bytes)
}

fn run_device(
    evidence: &mut EvidenceRun,
    request: &IosSubtitleRequest,
    diagnostics: &mut dyn Write,
) -> Result<(), SubtitleError> {
    let root = evidence.root.clone();
    let device = request.device_id.as_deref().ok_or_else(|| {
        SubtitleError::usage("--device is required for iOS device subtitle verification")
    })?;
    let team = request.development_team.as_deref().ok_or_else(|| {
        SubtitleError::usage("--development-team is required for iOS device subtitle verification")
    })?;
    prepare_ios_projects(evidence, diagnostics)?;
    prepare_flutter_dependencies(evidence, diagnostics)?;
    prepare_ios_device(evidence, device, team, diagnostics)?;

    let current_cli = current_cli()?;
    evidence.run_step(
        "ios-device-ffi",
        &root,
        Command::new(&current_cli)
            .args(["ios", "--root"])
            .arg(&root)
            .args(["ffi", "debug", "--platform", "device"]),
        BUILD_TIMEOUT,
        SubtitleErrorKind::Conformance,
        diagnostics,
    )?;
    let result_bundle = evidence.xcresult.join("ios-device.xcresult");
    let derived_data = evidence._temporary.path().join("ios-device-derived");
    let destination = format!("platform=iOS,id={device}");
    evidence.run_step(
        "ios-device-xctest",
        &root,
        Command::new(require_command(
            "xcodebuild",
            "xcodebuild is required for iOS subtitle verification",
        )?)
        .args([
            "test",
            "-project",
            "examples/ios-swift-host/VesperPlayerHostDemo.xcodeproj",
            "-scheme",
            "VesperPlayerHostDemo",
            "-configuration",
            "Debug",
            "-destination",
        ])
        .arg(destination)
        .arg("-resultBundlePath")
        .arg(&result_bundle)
        .arg("-derivedDataPath")
        .arg(&derived_data)
        .arg(format!("DEVELOPMENT_TEAM={team}"))
        .args([
            "CODE_SIGN_STYLE=Automatic",
            "-allowProvisioningUpdates",
            "-only-testing:VesperPlayerHostDemoTests/VesperSubtitleDeviceAcceptanceTests",
        ])
        .env("DEVELOPMENT_TEAM", team),
        TEST_TIMEOUT,
        SubtitleErrorKind::Conformance,
        diagnostics,
    )?;
    verify_xcresult(
        evidence,
        "ios-device",
        &result_bundle,
        &[("VesperSubtitleDeviceAcceptanceTests", 3)],
        diagnostics,
    )?;

    let exported = evidence.attachments.join("ios-device");
    reject_existing_path(&exported, "XCTest attachment export directory")?;
    let xcrun = require_command("xcrun", "xcrun is required for XCTest attachments")?;
    evidence.run_step(
        "ios-device-xctest-attachments",
        &root,
        Command::new(&xcrun)
            .args(["xcresulttool", "export", "attachments", "--path"])
            .arg(&result_bundle)
            .args(["--output-path"])
            .arg(&exported),
        PROJECT_TIMEOUT,
        SubtitleErrorKind::Conformance,
        diagnostics,
    )?;
    let started = Instant::now();
    let attachment_result = verify_ios_device_attachments(&exported);
    evidence.record_internal_step(
        "ios-device-xctest-attachment-evidence",
        attachment_result.map(|()| b"verified=subtitle-overlay\n".to_vec()),
        started,
    )?;

    run_flutter_integration(
        evidence,
        device,
        "device",
        "subtitle-positive",
        "integration_test/subtitle_contract_test.dart",
        Some(team),
        diagnostics,
    )?;
    run_flutter_integration(
        evidence,
        device,
        "device",
        "subtitle-lifecycle",
        "integration_test/subtitle_lifecycle_test.dart",
        Some(team),
        diagnostics,
    )
}

fn prepare_ios_device(
    evidence: &mut EvidenceRun,
    device_id: &str,
    development_team: &str,
    diagnostics: &mut dyn Write,
) -> Result<(), SubtitleError> {
    let root = evidence.root.clone();
    let temporary = evidence._temporary.path().to_path_buf();
    let xcrun = require_command("xcrun", "xcrun is required for iOS device verification")?;
    let inventory_path = temporary.join("devicectl-devices.json");
    evidence.run_step(
        "ios-devicectl-devices",
        &root,
        Command::new(&xcrun)
            .args([
                "devicectl",
                "list",
                "devices",
                "--timeout",
                "30",
                "--json-output",
            ])
            .arg(&inventory_path),
        PREFLIGHT_TIMEOUT,
        SubtitleErrorKind::Compatibility,
        diagnostics,
    )?;
    let inventory =
        read_required_evidence(&inventory_path, MAX_JSON_BYTES, "CoreDevice inventory")?;
    let started = Instant::now();
    let selected = select_core_device(&inventory, device_id)?;
    write_json(&evidence.preflight.join("selected-device.json"), &selected)?;
    evidence.selected_device = Some(selected.clone());
    evidence.record_internal_step(
        "ios-device-coredevice-match",
        Ok(serde_json::to_vec_pretty(&selected).map_err(|error| {
            SubtitleError::worker(format!("failed to serialize selected device: {error}"))
        })?),
        started,
    )?;

    let flutter = require_command(
        "flutter",
        "Flutter is required for iOS device subtitle verification",
    )?;
    let host = root.join("examples/flutter-host");
    let devices = evidence.run_step(
        "ios-flutter-device-list",
        &host,
        Command::new(&flutter).args(["devices", "--machine"]),
        PREFLIGHT_TIMEOUT,
        SubtitleErrorKind::Compatibility,
        diagnostics,
    )?;
    write_file(
        &evidence.preflight.join("flutter-devices-device.json"),
        &devices,
    )?;
    let started = Instant::now();
    let match_result = verify_flutter_device(&devices, device_id, false, "device");
    evidence.record_internal_step(
        "ios-flutter-device-match",
        match_result.map(|()| format!("matched={device_id}\n").into_bytes()),
        started,
    )?;

    let destinations = evidence.run_step(
        "ios-xcode-device-destinations",
        &root,
        Command::new(require_command(
            "xcodebuild",
            "xcodebuild is required for iOS device verification",
        )?)
        .args([
            "-project",
            "examples/ios-swift-host/VesperPlayerHostDemo.xcodeproj",
            "-scheme",
            "VesperPlayerHostDemo",
            "-showdestinations",
        ]),
        PREFLIGHT_TIMEOUT,
        SubtitleErrorKind::Compatibility,
        diagnostics,
    )?;
    write_file(
        &evidence.preflight.join("xcode-destinations-device.txt"),
        &destinations,
    )?;
    let started = Instant::now();
    let destination_result = verify_xcode_destination(&destinations, device_id);
    evidence.record_internal_step(
        "ios-xcode-device-destination-match",
        destination_result.map(|()| format!("matched={device_id}\n").into_bytes()),
        started,
    )?;

    let security = require_command(
        "security",
        "security is required for physical iOS subtitle verification",
    )?;
    let identities = evidence.run_step(
        "ios-codesigning-identities",
        &root,
        Command::new(&security).args(["find-identity", "-v", "-p", "codesigning"]),
        PREFLIGHT_TIMEOUT,
        SubtitleErrorKind::Compatibility,
        diagnostics,
    )?;
    write_file(
        &evidence.preflight.join("codesigning-identities.txt"),
        &identities,
    )?;
    let certificates = evidence.run_step(
        "ios-codesigning-certificates",
        &root,
        Command::new(&security).args(["find-certificate", "-a", "-p"]),
        PREFLIGHT_TIMEOUT,
        SubtitleErrorKind::Compatibility,
        diagnostics,
    )?;
    write_file(
        &evidence.preflight.join("codesigning-certificates.pem"),
        &certificates,
    )?;
    let started = Instant::now();
    let certificate = verify_signing_certificate(&identities, &certificates, development_team)?;
    write_file(
        &evidence.preflight.join("selected-signing-certificate.txt"),
        certificate.as_bytes(),
    )?;
    evidence.record_internal_step(
        "ios-signing-certificate",
        Ok(certificate.into_bytes()),
        started,
    )?;
    Ok(())
}

fn select_core_device(bytes: &[u8], requested_id: &str) -> Result<Value, SubtitleError> {
    let payload: Value = serde_json::from_slice(bytes).map_err(|error| {
        SubtitleError::conformance(format!("failed to parse CoreDevice inventory: {error}"))
    })?;
    let devices = payload
        .pointer("/result/devices")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            SubtitleError::conformance("CoreDevice inventory is missing result.devices")
        })?;
    let device = devices
        .iter()
        .find(|entry| {
            entry
                .pointer("/hardwareProperties/udid")
                .and_then(Value::as_str)
                == Some(requested_id)
        })
        .ok_or_else(|| {
            SubtitleError::compatibility(format!(
                "CoreDevice does not expose the requested iOS device: {requested_id}"
            ))
        })?;
    let pairing = device
        .pointer("/connectionProperties/pairingState")
        .and_then(Value::as_str)
        .ok_or_else(|| SubtitleError::conformance("CoreDevice device is missing pairingState"))?;
    let tunnel_state = device
        .pointer("/connectionProperties/tunnelState")
        .and_then(Value::as_str)
        .unwrap_or("");
    let developer_mode = device
        .pointer("/deviceProperties/developerModeStatus")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            SubtitleError::conformance("CoreDevice device is missing developerModeStatus")
        })?;
    let boot_state = device
        .pointer("/deviceProperties/bootState")
        .and_then(Value::as_str);
    let ddi_available = device
        .pointer("/deviceProperties/ddiServicesAvailable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let connect_capability = device
        .get("capabilities")
        .and_then(Value::as_array)
        .is_some_and(|capabilities| {
            capabilities.iter().any(|capability| {
                capability.get("featureIdentifier").and_then(Value::as_str)
                    == Some("com.apple.coredevice.feature.connectdevice")
            })
        });
    if pairing != "paired" {
        return Err(SubtitleError::compatibility(
            "the requested iOS device is not paired",
        ));
    }
    if developer_mode != "enabled" {
        return Err(SubtitleError::compatibility(
            "Developer Mode is not enabled on the requested iOS device",
        ));
    }
    let legacy_ready = boot_state == Some("booted") && tunnel_state == "connected" && ddi_available;
    let capability_ready = connect_capability && boot_state.is_none_or(|state| state == "booted");
    if !(legacy_ready || capability_ready) {
        return Err(SubtitleError::compatibility(
            "the requested iOS device is not booted and available",
        ));
    }
    let selected = json!({
        "id": requested_id,
        "name": device.pointer("/deviceProperties/name"),
        "model": device.pointer("/hardwareProperties/marketingName"),
        "productType": device.pointer("/hardwareProperties/productType"),
        "osVersion": device.pointer("/deviceProperties/osVersionNumber"),
        "osBuild": device.pointer("/deviceProperties/osBuildUpdate"),
        "releaseType": device.pointer("/deviceProperties/releaseType"),
        "pairingState": pairing,
        "developerMode": developer_mode,
        "bootState": boot_state,
        "transport": device.pointer("/connectionProperties/transportType"),
        "tunnelState": tunnel_state,
        "ddiServicesAvailable": ddi_available
    });
    Ok(selected)
}

fn verify_signing_certificate(
    identities: &[u8],
    certificates: &[u8],
    team: &str,
) -> Result<String, SubtitleError> {
    let identity_hashes = std::str::from_utf8(identities)
        .map_err(|error| {
            SubtitleError::conformance(format!("code-signing identities are not UTF-8: {error}"))
        })?
        .split_whitespace()
        .filter(|token| {
            token.len() == 40 && token.chars().all(|character| character.is_ascii_hexdigit())
        })
        .map(str::to_ascii_uppercase)
        .collect::<std::collections::BTreeSet<_>>();
    if identity_hashes.is_empty() {
        return Err(SubtitleError::compatibility(
            "no valid code-signing identity was reported by the security tool",
        ));
    }
    let mut remaining = certificates;
    let mut matches = Vec::new();
    loop {
        remaining = remaining
            .iter()
            .position(|byte| !byte.is_ascii_whitespace())
            .map_or(&[], |index| &remaining[index..]);
        if remaining.is_empty() {
            break;
        }
        let (rest, pem) = x509_parser::pem::parse_x509_pem(remaining).map_err(|error| {
            SubtitleError::conformance(format!("failed to parse keychain certificate PEM: {error}"))
        })?;
        if rest.len() >= remaining.len() {
            return Err(SubtitleError::conformance(
                "keychain certificate parser made no progress",
            ));
        }
        remaining = rest;
        if pem.label != "CERTIFICATE" {
            continue;
        }
        let (_, certificate) =
            x509_parser::parse_x509_certificate(&pem.contents).map_err(|error| {
                SubtitleError::conformance(format!("failed to parse X.509 certificate: {error}"))
            })?;
        let common_name = certificate
            .subject()
            .iter_common_name()
            .find_map(|attribute| attribute.as_str().ok())
            .unwrap_or_default();
        let organizational_unit = certificate
            .subject()
            .iter_organizational_unit()
            .find_map(|attribute| attribute.as_str().ok())
            .unwrap_or_default();
        let development_identity = common_name.starts_with("Apple Development:")
            || common_name.starts_with("iPhone Developer:");
        let mut sha1 = sha1::Sha1::new();
        sha1.update(&pem.contents);
        let sha1 = hex::encode_upper(sha1.finalize());
        if development_identity
            && organizational_unit == team
            && certificate.validity().is_valid()
            && identity_hashes.contains(&sha1)
        {
            let mut sha256 = Sha256::new();
            sha256.update(&pem.contents);
            matches.push(format!(
                "teamId={team}\ncommonName={common_name}\nnotBefore={}\nnotAfter={}\nsha1={sha1}\nsha256={}\n",
                certificate.validity().not_before.timestamp(),
                certificate.validity().not_after.timestamp(),
                hex::encode(sha256.finalize())
            ));
        }
    }
    matches.sort();
    matches.into_iter().next().ok_or_else(|| {
        SubtitleError::compatibility(format!(
            "no currently valid Apple development signing identity was found for Team ID {team}"
        ))
    })
}

fn verify_ios_device_attachments(directory: &Path) -> Result<(), SubtitleError> {
    let metadata = fs::symlink_metadata(directory).map_err(|error| {
        SubtitleError::conformance(format!(
            "missing XCTest attachment export directory '{}': {error}",
            directory.display()
        ))
    })?;
    if !metadata.file_type().is_dir() {
        return Err(SubtitleError::conformance(format!(
            "XCTest attachment export is not a regular non-symlink directory: {}",
            directory.display()
        )));
    }
    let manifest_path = directory.join("manifest.json");
    let manifest_bytes =
        read_required_evidence(&manifest_path, MAX_JSON_BYTES, "XCTest attachment manifest")?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes).map_err(|error| {
        SubtitleError::conformance(format!(
            "failed to parse XCTest attachment manifest '{}': {error}",
            manifest_path.display()
        ))
    })?;
    let entries = manifest.as_array().ok_or_else(|| {
        SubtitleError::conformance("XCTest attachment manifest must be a JSON array")
    })?;
    if entries.len() > MAX_XCRESULT_NODES {
        return Err(SubtitleError::conformance(
            "XCTest attachment manifest exceeds the record limit",
        ));
    }
    let mut snapshots = Vec::new();
    let mut images = Vec::new();
    let mut record_count = 0_usize;
    for entry in entries {
        let attachments = entry
            .get("attachments")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        for attachment in attachments {
            record_count += 1;
            if record_count > MAX_XCRESULT_NODES {
                return Err(SubtitleError::conformance(
                    "XCTest attachment manifest exceeds the attachment limit",
                ));
            }
            let suggested = attachment
                .get("suggestedHumanReadableName")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let exported = attachment
                .get("exportedFileName")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    SubtitleError::conformance(
                        "XCTest attachment record is missing exportedFileName",
                    )
                })?;
            if suggested.starts_with("subtitle-overlay-snapshot_") {
                snapshots.push(exported.to_owned());
            } else if suggested.starts_with("subtitle-overlay_") && suggested.ends_with(".png") {
                images.push(exported.to_owned());
            }
        }
    }
    if snapshots.len() != 1 || images.len() != 1 {
        return Err(SubtitleError::conformance(format!(
            "XCTest attachments require exactly one subtitle snapshot and PNG; found {} snapshots and {} PNGs",
            snapshots.len(),
            images.len()
        )));
    }
    let snapshot_path = resolve_attachment_file(directory, &snapshots[0])?;
    let image_path = resolve_attachment_file(directory, &images[0])?;
    let snapshot_bytes = read_required_evidence(
        &snapshot_path,
        MAX_JSON_BYTES,
        "XCTest subtitle snapshot attachment",
    )?;
    let snapshot: Value = serde_json::from_slice(&snapshot_bytes).map_err(|error| {
        SubtitleError::conformance(format!(
            "failed to parse XCTest subtitle snapshot '{}': {error}",
            snapshot_path.display()
        ))
    })?;
    verify_visible_subtitle_snapshot(&snapshot, "XCTest subtitle attachment")?;
    verify_png(&image_path, "XCTest subtitle overlay PNG")
}

fn resolve_attachment_file(
    directory: &Path,
    exported_name: &str,
) -> Result<PathBuf, SubtitleError> {
    if exported_name.is_empty()
        || exported_name.len() > 255
        || exported_name.chars().any(char::is_control)
    {
        return Err(SubtitleError::conformance(
            "XCTest attachment exportedFileName is empty, overlong, or contains control characters",
        ));
    }
    let mut components = Path::new(exported_name).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(SubtitleError::conformance(format!(
            "XCTest attachment exportedFileName must be a basename: {exported_name}"
        )));
    }
    let path = directory.join(exported_name);
    let metadata = fs::symlink_metadata(&path).map_err(|error| {
        SubtitleError::conformance(format!(
            "missing exported XCTest attachment '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_file() {
        return Err(SubtitleError::conformance(format!(
            "exported XCTest attachment is not a regular non-symlink file: {}",
            path.display()
        )));
    }
    let canonical_directory = directory.canonicalize().map_err(|error| {
        SubtitleError::storage(format!(
            "failed to resolve XCTest attachment directory '{}': {error}",
            directory.display()
        ))
    })?;
    let canonical_path = path.canonicalize().map_err(|error| {
        SubtitleError::storage(format!(
            "failed to resolve XCTest attachment '{}': {error}",
            path.display()
        ))
    })?;
    if canonical_path.parent() != Some(canonical_directory.as_path()) {
        return Err(SubtitleError::conformance(format!(
            "XCTest attachment escaped its export directory: {}",
            path.display()
        )));
    }
    Ok(path)
}

fn reject_existing_path(path: &Path, label: &str) -> Result<(), SubtitleError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(SubtitleError::usage(format!(
            "{label} already exists: {}",
            path.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
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
    timeout: Duration,
) -> Result<String, SubtitleError> {
    let captured = capture_command(command, label, maximum_bytes, maximum_bytes, timeout)?;
    if !captured.status.success() {
        return Err(process_status_error(
            label,
            captured.status,
            SubtitleErrorKind::Conformance,
        ));
    }
    let text = String::from_utf8(captured.stdout).map_err(|error| {
        SubtitleError::conformance(format!("{label} output is not UTF-8: {error}"))
    })?;
    let value = text.trim();
    if value.is_empty() {
        return Err(SubtitleError::conformance(format!(
            "{label} produced empty output"
        )));
    }
    Ok(value.to_owned())
}

fn capture_command(
    command: &mut Command,
    label: &str,
    stdout_maximum_bytes: usize,
    stderr_maximum_bytes: usize,
    timeout: Duration,
) -> Result<external_process::BoundedProcessOutput, SubtitleError> {
    external_process::run_interruptible_capture_with_timeout(
        command,
        label,
        stdout_maximum_bytes,
        stderr_maximum_bytes,
        timeout,
    )
    .map_err(map_process_error)
}

fn map_process_error(error: external_process::ExternalProcessError) -> SubtitleError {
    match error.kind() {
        external_process::ExternalProcessErrorKind::Compatibility => {
            SubtitleError::compatibility(error.to_string())
        }
        external_process::ExternalProcessErrorKind::Worker
        | external_process::ExternalProcessErrorKind::Cancelled => {
            SubtitleError::worker(error.to_string())
        }
    }
}

fn process_status_error(
    label: &str,
    status: ExitStatus,
    failure_kind: SubtitleErrorKind,
) -> SubtitleError {
    let message = if status.code().is_none() {
        format!("{label} crashed ({status})")
    } else {
        format!("{label} exited unsuccessfully ({status})")
    };
    if status.code().is_none() {
        return SubtitleError::worker(message);
    }
    match failure_kind {
        SubtitleErrorKind::Usage => SubtitleError::usage(message),
        SubtitleErrorKind::Storage => SubtitleError::storage(message),
        SubtitleErrorKind::Compatibility => SubtitleError::compatibility(message),
        SubtitleErrorKind::Conformance => SubtitleError::conformance(message),
        SubtitleErrorKind::Worker => SubtitleError::worker(message),
    }
}

fn diagnostic_error(error: io::Error) -> SubtitleError {
    SubtitleError::worker(format!("failed to write subtitle diagnostics: {error}"))
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
    })?;
    file.sync_all().map_err(|error| {
        SubtitleError::storage(format!(
            "failed to synchronize '{}': {error}",
            path.display()
        ))
    })
}

fn read_bounded_file(path: &Path, maximum_bytes: u64) -> Result<Vec<u8>, SubtitleError> {
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
                    "subtitle evidence contains symlink or special file '{}':",
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

    #[test]
    fn simulator_selection_prefers_a_booted_supported_iphone_and_honors_exact_ids() {
        let inventory = br#"{
          "devices": {
            "com.apple.CoreSimulator.SimRuntime.iOS-16-4": [
              {"udid":"ios16","name":"iPhone 14","state":"Booted","isAvailable":true}
            ],
            "com.apple.CoreSimulator.SimRuntime.iOS-17-5": [
              {"udid":"booted17","name":"iPhone 15","state":"Booted","isAvailable":true}
            ],
            "com.apple.CoreSimulator.SimRuntime.iOS-18-2": [
              {"udid":"shutdown18","name":"iPhone 16","state":"Shutdown","isAvailable":true},
              {"udid":"ipad18","name":"iPad Pro","state":"Booted","isAvailable":true}
            ]
          }
        }"#;
        let automatic = select_simulator(inventory, None).expect("select automatic Simulator");
        assert_eq!(automatic.id, "booted17");
        let requested =
            select_simulator(inventory, Some("shutdown18")).expect("select requested Simulator");
        assert_eq!(requested.id, "shutdown18");
        assert_eq!(requested.os_version, "18.2");
        assert_eq!(
            select_simulator(inventory, Some("ios16"))
                .expect_err("iOS 16 must remain unsupported")
                .kind(),
            SubtitleErrorKind::Compatibility
        );
    }

    #[test]
    fn xcode_destination_matching_rejects_identifier_prefix_collisions() {
        let destinations = b"{ platform:iOS Simulator, id:device-1234, name:iPhone 16 }\n";
        assert!(verify_xcode_destination(destinations, "device-1234").is_ok());
        assert_eq!(
            verify_xcode_destination(destinations, "device-123")
                .expect_err("prefix collision must be rejected")
                .kind(),
            SubtitleErrorKind::Compatibility
        );
    }

    #[test]
    fn xcresult_verification_rejects_duplicate_suites_and_insufficient_cases() {
        let summary = br#"{"result":"Passed","failedTests":0,"totalTestCount":2}"#;
        let suite = json!({
            "nodeType": "Test Suite",
            "name": "Suite",
            "result": "Passed",
            "children": [
                {"nodeType":"Test Case","name":"one","result":"Passed"}
            ]
        });
        let duplicate = serde_json::to_vec(&json!({
            "testNodes": [suite.clone(), suite.clone()]
        }))
        .expect("serialize duplicate suite fixture");
        let error = verify_xcresult_payloads(summary, &duplicate, &[("Suite", 1)])
            .expect_err("duplicate suite must fail");
        assert!(error.to_string().contains("exactly one"));

        let one_case = serde_json::to_vec(&json!({"testNodes": [suite]}))
            .expect("serialize one-case suite fixture");
        let error = verify_xcresult_payloads(summary, &one_case, &[("Suite", 2)])
            .expect_err("suite minimum must be enforced");
        assert!(error.to_string().contains("expected at least 2"));
    }

    #[test]
    fn flutter_positive_evidence_requires_numeric_visible_geometry_and_png_signature() {
        let directory = tempfile::tempdir().expect("create Flutter evidence fixture");
        fs::write(
            directory.path().join("subtitle-positive.json"),
            serde_json::to_vec(&json!({
                "evidenceName": "subtitle-positive",
                "snapshot": {
                    "text": "Subtitle B",
                    "visible": true,
                    "hidden": false,
                    "windowAttached": true,
                    "alpha": "1.0",
                    "frame": {"width": 100.0, "height": 20.0}
                },
                "pngFile": "subtitle-positive.png"
            }))
            .expect("serialize invalid Flutter evidence"),
        )
        .expect("write invalid Flutter evidence");
        fs::write(
            directory.path().join("subtitle-positive.png"),
            b"\x89PNG\r\n\x1a\nfixture",
        )
        .expect("write PNG fixture");
        let error = verify_flutter_evidence(directory.path(), "subtitle-positive")
            .expect_err("string alpha must be rejected");
        assert!(error.to_string().contains("visibly attached"));

        let valid = json!({
            "evidenceName": "subtitle-positive",
            "snapshot": {
                "text": "Subtitle B",
                "visible": true,
                "hidden": false,
                "windowAttached": true,
                "alpha": 1.0,
                "frame": {"width": 100.0, "height": 20.0}
            },
            "pngFile": "subtitle-positive.png"
        });
        fs::write(
            directory.path().join("subtitle-positive.json"),
            serde_json::to_vec(&valid).expect("serialize valid Flutter evidence"),
        )
        .expect("write valid Flutter evidence");
        verify_flutter_evidence(directory.path(), "subtitle-positive")
            .expect("accept valid Flutter evidence");
    }

    #[test]
    fn ios_subtitle_request_contract_rejects_invalid_scope_combinations() {
        assert_eq!(
            validate_request(&IosSubtitleRequest {
                scope: SubtitleScope::Regression,
                device_id: Some("device".to_owned()),
                simulator_id: None,
                evidence_directory: None,
                development_team: None,
            })
            .expect_err("regression scope must reject a physical device")
            .kind(),
            SubtitleErrorKind::Usage
        );
        assert_eq!(
            validate_request(&IosSubtitleRequest {
                scope: SubtitleScope::Device,
                device_id: Some("device".to_owned()),
                simulator_id: Some("simulator".to_owned()),
                evidence_directory: None,
                development_team: Some("TEAM123456".to_owned()),
            })
            .expect_err("device scope must reject a Simulator")
            .kind(),
            SubtitleErrorKind::Usage
        );
        assert_eq!(
            validate_request(&IosSubtitleRequest {
                scope: SubtitleScope::Device,
                device_id: Some("device".to_owned()),
                simulator_id: None,
                evidence_directory: None,
                development_team: None,
            })
            .expect_err("device scope must require a development team")
            .kind(),
            SubtitleErrorKind::Usage
        );
    }

    #[test]
    fn core_device_selection_requires_pairing_developer_mode_and_readiness() {
        let fixture = |pairing: &str, developer_mode: &str, boot: &str| {
            serde_json::to_vec(&json!({
                "result": {"devices": [{
                    "hardwareProperties": {"udid":"device", "marketingName":"iPhone", "productType":"iPhone17,1"},
                    "connectionProperties": {"pairingState":pairing, "tunnelState":"connected", "transportType":"wired"},
                    "deviceProperties": {
                        "developerModeStatus":developer_mode,
                        "bootState":boot,
                        "ddiServicesAvailable":true,
                        "name":"Phone",
                        "osVersionNumber":"18.2"
                    },
                    "capabilities": []
                }]}
            }))
            .expect("serialize CoreDevice fixture")
        };
        assert!(select_core_device(&fixture("paired", "enabled", "booted"), "device").is_ok());
        let capability_only = serde_json::to_vec(&json!({
            "result": {"devices": [{
                "hardwareProperties": {
                    "udid":"device",
                    "marketingName":"iPhone",
                    "productType":"iPhone17,1"
                },
                "connectionProperties": {
                    "pairingState":"paired",
                    "tunnelState":"disconnected",
                    "transportType":"localNetwork"
                },
                "deviceProperties": {
                    "developerModeStatus":"enabled",
                    "ddiServicesAvailable":false,
                    "name":"Phone",
                    "osVersionNumber":"27.0"
                },
                "capabilities": [{
                    "featureIdentifier":"com.apple.coredevice.feature.connectdevice"
                }]
            }]}
        }))
        .expect("serialize modern CoreDevice fixture");
        let selected = select_core_device(&capability_only, "device")
            .expect("connect capability replaces removed boot state");
        assert!(selected["bootState"].is_null());
        for (pairing, mode, boot, message) in [
            ("unpaired", "enabled", "booted", "not paired"),
            ("paired", "disabled", "booted", "Developer Mode"),
            ("paired", "enabled", "shutdown", "not booted"),
        ] {
            let error = select_core_device(&fixture(pairing, mode, boot), "device")
                .expect_err("invalid CoreDevice readiness must fail");
            assert_eq!(error.kind(), SubtitleErrorKind::Compatibility);
            assert!(error.to_string().contains(message));
        }
    }

    #[test]
    fn attachment_manifest_rejects_path_traversal() {
        let directory = tempfile::tempdir().expect("create attachment fixture");
        fs::write(
            directory.path().join("manifest.json"),
            serde_json::to_vec(&json!([{
                "attachments": [
                    {
                        "suggestedHumanReadableName":"subtitle-overlay-snapshot_fixture",
                        "exportedFileName":"../snapshot.json"
                    },
                    {
                        "suggestedHumanReadableName":"subtitle-overlay_fixture.png",
                        "exportedFileName":"image.png"
                    }
                ]
            }]))
            .expect("serialize attachment manifest"),
        )
        .expect("write attachment manifest");
        fs::write(directory.path().join("image.png"), b"\x89PNG\r\n\x1a\n")
            .expect("write image fixture");
        let error = verify_ios_device_attachments(directory.path())
            .expect_err("attachment traversal must fail");
        assert!(error.to_string().contains("basename"));
    }

    #[cfg(unix)]
    #[test]
    fn attachment_manifest_rejects_symlinked_payloads() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().expect("create attachment fixture");
        let outside = directory.path().join("outside.json");
        fs::write(
            &outside,
            serde_json::to_vec(&json!({
                "text":"Subtitle B",
                "visible":true,
                "hidden":false,
                "windowAttached":true,
                "alpha":1.0,
                "frame":{"width":10.0,"height":10.0}
            }))
            .expect("serialize snapshot fixture"),
        )
        .expect("write outside snapshot");
        symlink(&outside, directory.path().join("snapshot.json"))
            .expect("create attachment symlink");
        fs::write(directory.path().join("image.png"), b"\x89PNG\r\n\x1a\n")
            .expect("write image fixture");
        fs::write(
            directory.path().join("manifest.json"),
            serde_json::to_vec(&json!([{
                "attachments": [
                    {
                        "suggestedHumanReadableName":"subtitle-overlay-snapshot_fixture",
                        "exportedFileName":"snapshot.json"
                    },
                    {
                        "suggestedHumanReadableName":"subtitle-overlay_fixture.png",
                        "exportedFileName":"image.png"
                    }
                ]
            }]))
            .expect("serialize attachment manifest"),
        )
        .expect("write attachment manifest");
        let error = verify_ios_device_attachments(directory.path())
            .expect_err("attachment symlink must fail");
        assert!(error.to_string().contains("non-symlink"));
    }

    #[test]
    fn flutter_cleanup_matches_exact_executable_and_detects_pid_reuse() {
        let app_url = "file:///private/var/containers/Bundle/Application/UUID/Runner.app/";
        let executable = format!("{app_url}Runner");
        let apps = serde_json::to_vec(&json!({
            "result":{"apps":[{"bundleIdentifier":"io.github.umbrella22.vesper.example.flutterhost","url":app_url}]}
        }))
        .expect("serialize app inventory");
        let processes = serde_json::to_vec(&json!({
            "result":{"runningProcesses":[{"processIdentifier":42,"executable":executable}]}
        }))
        .expect("serialize process inventory");
        let selected = flutter_host_processes(
            &apps,
            &processes,
            "io.github.umbrella22.vesper.example.flutterhost",
        )
        .expect("match Flutter host process")
        .expect("installed Flutter host");
        assert_eq!(selected.1, vec![42]);

        let reused = serde_json::to_vec(&json!({
            "result":{"runningProcesses":[{"processIdentifier":42,"executable":"/other/process"}]}
        }))
        .expect("serialize reused PID fixture");
        assert_eq!(
            process_executable_for_pid(&reused, 42)
                .expect("parse reused PID")
                .as_deref(),
            Some("/other/process")
        );
    }
}
