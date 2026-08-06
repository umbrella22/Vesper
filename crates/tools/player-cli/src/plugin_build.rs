use std::collections::HashSet;
use std::fs::{self, File};
#[cfg(any(unix, test))]
use std::io::BufReader;
#[cfg(unix)]
use std::io::Read;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::sync::Arc;
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(unix)]
use std::sync::mpsc::{self, Receiver};
#[cfg(unix)]
use std::thread;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

use clap::ValueEnum;
use player_cli::{
    PluginArtifactFormat, PluginArtifactSource, PluginArtifactTransport, PluginProjectManifest,
};
#[cfg(unix)]
use player_platform_process::configure_background_process_group;
use serde::{Deserialize, Serialize};

const MAX_CARGO_JSON_LINE_BYTES: usize = 4 * 1024 * 1024;
const MAX_CARGO_ARTIFACT_CANDIDATES: usize = 128;
#[cfg(unix)]
const BUILD_POLL_INTERVAL: Duration = Duration::from_millis(10);
#[cfg(unix)]
const BUILD_TERMINATION_GRACE: Duration = Duration::from_millis(500);
#[cfg(unix)]
const BUILD_REAP_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(unix)]
const BUILD_OUTPUT_DRAIN_GRACE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginBuildProfile {
    Dev,
    Release,
}

impl PluginBuildProfile {
    #[cfg(unix)]
    const fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Release => "release",
        }
    }
}

#[derive(Debug)]
pub struct PluginArtifactSelector {
    pub transport: Option<PluginArtifactTransport>,
    pub target: Option<String>,
    pub architecture: Option<String>,
}

#[derive(Debug)]
pub struct PluginBuildRequest {
    pub plugin_id: String,
    #[cfg_attr(not(unix), allow(dead_code))]
    pub cargo_manifest: PathBuf,
    #[cfg_attr(not(unix), allow(dead_code))]
    pub working_directory: PathBuf,
    pub artifact: PluginArtifactSource,
    pub destination: PathBuf,
    pub profile: PluginBuildProfile,
    #[cfg_attr(not(unix), allow(dead_code))]
    pub package: Option<String>,
    #[cfg_attr(not(unix), allow(dead_code))]
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginBuildReport {
    pub schema_version: u32,
    pub plugin_id: String,
    pub transport: PluginArtifactTransport,
    pub target: String,
    pub architecture: String,
    pub profile: PluginBuildProfile,
    pub package_id: String,
    pub cargo_target_name: String,
    pub cargo_artifact: PathBuf,
    pub output: PathBuf,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginBuildError {
    Storage(String),
    Compatibility(String),
    Conformance(String),
    Worker(String),
}

impl std::fmt::Display for PluginBuildError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(message)
            | Self::Compatibility(message)
            | Self::Conformance(message)
            | Self::Worker(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for PluginBuildError {}

pub fn select_plugin_artifact(
    project: &PluginProjectManifest,
    selector: &PluginArtifactSelector,
) -> Result<PluginArtifactSource, PluginBuildError> {
    let matches = project
        .artifacts()
        .iter()
        .filter(|artifact| {
            selector
                .transport
                .is_none_or(|transport| artifact.transport == transport)
                && selector
                    .target
                    .as_deref()
                    .is_none_or(|target| artifact.target == target)
                && selector
                    .architecture
                    .as_deref()
                    .is_none_or(|architecture| artifact.architecture == architecture)
        })
        .cloned()
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [artifact] => match (artifact.transport, artifact.format) {
            (PluginArtifactTransport::Native, PluginArtifactFormat::Dylib)
            | (PluginArtifactTransport::Wasm, PluginArtifactFormat::WasmComponent) => {
                Ok(artifact.clone())
            }
            _ => Err(PluginBuildError::Compatibility(format!(
                "vesper plugin build supports Cargo dylib and wasm-component artifacts; selected '{}:{}' uses format '{}'",
                artifact.transport.as_str(),
                artifact.target,
                artifact.format.as_str()
            ))),
        },
        [] => Err(PluginBuildError::Compatibility(
            "no manifest artifact matches the requested transport, target, and architecture"
                .to_owned(),
        )),
        _ => Err(PluginBuildError::Compatibility(format!(
            "artifact selector is ambiguous and matches {} manifest entries; specify --transport, --target, and --architecture",
            matches.len()
        ))),
    }
}

pub fn build_plugin_artifact(
    request: PluginBuildRequest,
) -> Result<PluginBuildReport, PluginBuildError> {
    let candidates = run_cargo_build(&request)?;
    let expected_extension = expected_artifact_extension(&request.artifact)?;
    let mut matches = candidates
        .into_iter()
        .filter(|candidate| {
            candidate
                .path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case(expected_extension))
        })
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| left.path.cmp(&right.path));
    let candidate = match matches.as_slice() {
        [candidate] => candidate,
        [] => {
            return Err(PluginBuildError::Conformance(format!(
                "Cargo completed successfully but did not emit one '{}' cdylib artifact for target '{}'",
                expected_extension, request.artifact.target
            )));
        }
        _ => {
            return Err(PluginBuildError::Compatibility(format!(
                "Cargo emitted {} matching cdylib artifacts for target '{}'; use --package to select one package",
                matches.len(),
                request.artifact.target
            )));
        }
    };
    let bytes = atomic_copy_artifact(&candidate.path, &request.destination)?;
    Ok(PluginBuildReport {
        schema_version: 1,
        plugin_id: request.plugin_id,
        transport: request.artifact.transport,
        target: request.artifact.target,
        architecture: request.artifact.architecture,
        profile: request.profile,
        package_id: candidate.package_id.clone(),
        cargo_target_name: candidate.target_name.clone(),
        cargo_artifact: candidate.path.clone(),
        output: request.destination,
        bytes,
    })
}

fn expected_artifact_extension(
    artifact: &PluginArtifactSource,
) -> Result<&'static str, PluginBuildError> {
    match (artifact.transport, artifact.format) {
        (PluginArtifactTransport::Wasm, PluginArtifactFormat::WasmComponent) => Ok("wasm"),
        (PluginArtifactTransport::Native, PluginArtifactFormat::Dylib) => {
            if artifact.target.contains("windows") {
                Ok("dll")
            } else if artifact.target.contains("apple")
                || artifact.target.contains("darwin")
                || artifact.target.contains("ios")
            {
                Ok("dylib")
            } else {
                Ok("so")
            }
        }
        _ => Err(PluginBuildError::Compatibility(format!(
            "artifact format '{}' cannot be built by Cargo",
            artifact.format.as_str()
        ))),
    }
}

#[derive(Debug)]
struct CargoArtifactCandidate {
    package_id: String,
    target_name: String,
    path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct CargoMessage {
    reason: String,
    #[serde(default)]
    package_id: Option<String>,
    #[serde(default)]
    target: Option<CargoTarget>,
    #[serde(default)]
    filenames: Vec<PathBuf>,
    #[serde(default)]
    message: Option<CargoDiagnostic>,
}

#[derive(Debug, Deserialize)]
struct CargoTarget {
    name: String,
    #[serde(default)]
    kind: Vec<String>,
    #[serde(default)]
    crate_types: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CargoDiagnostic {
    #[serde(default)]
    rendered: Option<String>,
}

#[cfg(unix)]
fn run_cargo_build(
    request: &PluginBuildRequest,
) -> Result<Vec<CargoArtifactCandidate>, PluginBuildError> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .arg("build")
        .arg("--manifest-path")
        .arg(&request.cargo_manifest)
        .arg("--target")
        .arg(&request.artifact.target)
        .arg("--profile")
        .arg(request.profile.as_str())
        .arg("--message-format=json-render-diagnostics")
        .current_dir(&request.working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    configure_background_process_group(&mut command);
    if let Some(package) = request.package.as_deref() {
        command.arg("--package").arg(package);
    }
    let mut child = command.spawn().map_err(|error| {
        PluginBuildError::Worker(format!("failed to spawn Cargo build: {error}"))
    })?;
    let process_group = i32::try_from(child.id()).map_err(|_| {
        let _ = child.kill();
        let _ = child.wait();
        PluginBuildError::Worker(
            "Cargo build process id cannot be represented as a process group".to_owned(),
        )
    })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        abort_build_setup(&mut child, process_group);
        PluginBuildError::Worker("Cargo build stdout pipe is missing".to_owned())
    })?;
    let output = start_cargo_output_parser(stdout);
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal_id =
        signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&cancelled)).map_err(
            |error| {
                abort_build_setup(&mut child, process_group);
                PluginBuildError::Worker(format!(
                    "failed to install Cargo build cancellation handler: {error}"
                ))
            },
        )?;
    let signal_guard = SignalRegistration(signal_id);
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let _ = terminate_and_reap(&mut child, process_group, false);
                return Err(PluginBuildError::Worker(format!(
                    "failed to poll Cargo build: {error}"
                )));
            }
        }
        if cancelled.load(Ordering::Acquire) {
            terminate_and_reap(&mut child, process_group, true)?;
            drop(signal_guard);
            return Err(PluginBuildError::Worker(
                "Cargo build was cancelled".to_owned(),
            ));
        }
        if started.elapsed() >= request.timeout {
            terminate_and_reap(&mut child, process_group, false)?;
            drop(signal_guard);
            return Err(PluginBuildError::Worker(format!(
                "Cargo build exceeded its {} ms deadline",
                request.timeout.as_millis()
            )));
        }
        thread::sleep(BUILD_POLL_INTERVAL);
    };
    drop(signal_guard);
    cleanup_descendants(process_group);
    let candidates = receive_cargo_output(output)?;
    if !status.success() {
        return Err(PluginBuildError::Conformance(format!(
            "Cargo build exited unsuccessfully ({status})"
        )));
    }
    Ok(candidates)
}

#[cfg(not(unix))]
fn run_cargo_build(
    _request: &PluginBuildRequest,
) -> Result<Vec<CargoArtifactCandidate>, PluginBuildError> {
    Err(PluginBuildError::Worker(
        "Cargo build process containment is not implemented for this platform".to_owned(),
    ))
}

#[cfg(unix)]
fn start_cargo_output_parser<R>(reader: R) -> Receiver<Result<Vec<CargoArtifactCandidate>, String>>
where
    R: Read + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let result = parse_cargo_output(BufReader::new(reader));
        let _ = sender.send(result);
    });
    receiver
}

fn parse_cargo_output<R>(mut reader: R) -> Result<Vec<CargoArtifactCandidate>, String>
where
    R: BufRead,
{
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    loop {
        let Some(line) = read_bounded_line(&mut reader, MAX_CARGO_JSON_LINE_BYTES)? else {
            break;
        };
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let message: CargoMessage = serde_json::from_slice(&line)
            .map_err(|error| format!("Cargo emitted invalid JSON output: {error}"))?;
        if message.reason == "compiler-message"
            && let Some(rendered) = message.message.and_then(|message| message.rendered)
        {
            let mut stderr = io::stderr().lock();
            stderr
                .write_all(rendered.as_bytes())
                .map_err(|error| format!("failed to relay Cargo diagnostic: {error}"))?;
            if !rendered.ends_with('\n') {
                stderr
                    .write_all(b"\n")
                    .map_err(|error| format!("failed to terminate Cargo diagnostic: {error}"))?;
            }
        }
        if message.reason != "compiler-artifact" {
            continue;
        }
        let Some(target) = message.target else {
            return Err("Cargo compiler-artifact message is missing its target".to_owned());
        };
        if !target
            .crate_types
            .iter()
            .chain(target.kind.iter())
            .any(|kind| kind == "cdylib")
        {
            continue;
        }
        let package_id = message.package_id.ok_or_else(|| {
            "Cargo compiler-artifact message is missing its package_id".to_owned()
        })?;
        for path in message.filenames {
            if seen.insert(path.clone()) {
                if candidates.len() >= MAX_CARGO_ARTIFACT_CANDIDATES {
                    return Err(format!(
                        "Cargo emitted more than {MAX_CARGO_ARTIFACT_CANDIDATES} cdylib artifact candidates"
                    ));
                }
                candidates.push(CargoArtifactCandidate {
                    package_id: package_id.clone(),
                    target_name: target.name.clone(),
                    path,
                });
            }
        }
    }
    Ok(candidates)
}

fn read_bounded_line<R>(reader: &mut R, maximum_bytes: usize) -> Result<Option<Vec<u8>>, String>
where
    R: BufRead,
{
    let mut line = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .map_err(|error| format!("failed to read Cargo JSON output: {error}"))?;
        if available.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Ok(Some(line))
            };
        }
        let consumed = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |index| index + 1);
        if line.len().saturating_add(consumed) > maximum_bytes {
            return Err(format!(
                "Cargo JSON output line exceeds {maximum_bytes} bytes"
            ));
        }
        line.extend_from_slice(&available[..consumed]);
        let ended = available[consumed - 1] == b'\n';
        reader.consume(consumed);
        if ended {
            return Ok(Some(line));
        }
    }
}

#[cfg(unix)]
fn receive_cargo_output(
    receiver: Receiver<Result<Vec<CargoArtifactCandidate>, String>>,
) -> Result<Vec<CargoArtifactCandidate>, PluginBuildError> {
    receiver
        .recv_timeout(BUILD_OUTPUT_DRAIN_GRACE)
        .map_err(|error| {
            PluginBuildError::Worker(format!(
                "Cargo JSON output did not finish draining within {} ms: {error}",
                BUILD_OUTPUT_DRAIN_GRACE.as_millis()
            ))
        })?
        .map_err(PluginBuildError::Worker)
}

fn atomic_copy_artifact(source: &Path, destination: &Path) -> Result<u64, PluginBuildError> {
    let metadata = fs::symlink_metadata(source).map_err(|error| {
        PluginBuildError::Conformance(format!(
            "failed to inspect Cargo artifact '{}': {error}",
            source.display()
        ))
    })?;
    if !metadata.file_type().is_file() {
        return Err(PluginBuildError::Conformance(format!(
            "Cargo artifact '{}' is not a regular non-symlink file",
            source.display()
        )));
    }
    if source == destination {
        return Ok(metadata.len());
    }
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| {
        PluginBuildError::Storage(format!(
            "failed to create artifact output directory '{}': {error}",
            parent.display()
        ))
    })?;
    let mut input = File::open(source).map_err(|error| {
        PluginBuildError::Conformance(format!(
            "failed to open Cargo artifact '{}': {error}",
            source.display()
        ))
    })?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|error| {
        PluginBuildError::Storage(format!(
            "failed to create artifact staging file in '{}': {error}",
            parent.display()
        ))
    })?;
    let bytes = io::copy(&mut input, &mut temporary).map_err(|error| {
        PluginBuildError::Storage(format!(
            "failed to stage Cargo artifact for '{}': {error}",
            destination.display()
        ))
    })?;
    temporary.as_file().sync_all().map_err(|error| {
        PluginBuildError::Storage(format!(
            "failed to sync staged Cargo artifact for '{}': {error}",
            destination.display()
        ))
    })?;
    temporary.persist(destination).map_err(|error| {
        PluginBuildError::Storage(format!(
            "failed to atomically replace artifact '{}': {}",
            destination.display(),
            error.error
        ))
    })?;
    Ok(bytes)
}

#[cfg(unix)]
fn abort_build_setup(child: &mut std::process::Child, process_group: i32) {
    let _ = terminate_and_reap(child, process_group, false);
}

#[cfg(unix)]
fn terminate_and_reap(
    child: &mut std::process::Child,
    process_group: i32,
    cancelled: bool,
) -> Result<(), PluginBuildError> {
    use nix::sys::signal::Signal;

    let initial_signal = if cancelled {
        Signal::SIGINT
    } else {
        Signal::SIGTERM
    };
    let initial_error = signal_process_group(process_group, initial_signal).err();
    if initial_error.is_some() {
        let _ = child.kill();
    }
    let grace_deadline = Instant::now() + BUILD_TERMINATION_GRACE;
    while Instant::now() < grace_deadline {
        if child
            .try_wait()
            .map_err(|error| {
                PluginBuildError::Worker(format!("failed to reap Cargo build: {error}"))
            })?
            .is_some()
        {
            cleanup_descendants(process_group);
            return initial_error.map_or(Ok(()), Err);
        }
        thread::sleep(BUILD_POLL_INTERVAL);
    }
    let kill_error = signal_process_group(process_group, Signal::SIGKILL).err();
    let _ = child.kill();
    let reap_deadline = Instant::now() + BUILD_REAP_TIMEOUT;
    while Instant::now() < reap_deadline {
        if child
            .try_wait()
            .map_err(|error| {
                PluginBuildError::Worker(format!("failed to reap Cargo build: {error}"))
            })?
            .is_some()
        {
            return initial_error.or(kill_error).map_or(Ok(()), Err);
        }
        thread::sleep(BUILD_POLL_INTERVAL);
    }
    Err(PluginBuildError::Worker(
        "Cargo build could not be reaped after termination".to_owned(),
    ))
}

#[cfg(unix)]
fn signal_process_group(
    process_group: i32,
    signal: nix::sys::signal::Signal,
) -> Result<(), PluginBuildError> {
    use nix::errno::Errno;
    use nix::sys::signal::killpg;
    use nix::unistd::Pid;

    match killpg(Pid::from_raw(process_group), signal) {
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        Err(error) => Err(PluginBuildError::Worker(format!(
            "failed to signal Cargo build process group: {error}"
        ))),
    }
}

#[cfg(unix)]
fn cleanup_descendants(process_group: i32) {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;

    let process_group = Pid::from_raw(process_group);
    let _ = killpg(process_group, Signal::SIGTERM);
    thread::sleep(Duration::from_millis(20));
    let _ = killpg(process_group, Signal::SIGKILL);
}

#[cfg(unix)]
struct SignalRegistration(signal_hook::SigId);

#[cfg(unix)]
impl Drop for SignalRegistration {
    fn drop(&mut self) {
        signal_hook::low_level::unregister(self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_line_reader_rejects_an_unterminated_oversized_line() {
        let mut reader = BufReader::new(&b"12345"[..]);
        let error = read_bounded_line(&mut reader, 4).expect_err("oversized line");
        assert!(error.contains("exceeds 4 bytes"));
    }

    #[test]
    fn cargo_parser_keeps_only_unique_cdylib_filenames() {
        let source = br#"{"reason":"compiler-artifact","package_id":"fixture 0.1.0","target":{"name":"fixture","kind":["cdylib"],"crate_types":["cdylib"]},"filenames":["/tmp/fixture.wasm","/tmp/fixture.wasm"]}
{"reason":"build-finished","success":true}
"#;
        let candidates = parse_cargo_output(BufReader::new(&source[..])).expect("Cargo JSON");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].path, Path::new("/tmp/fixture.wasm"));
    }
}
