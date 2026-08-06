use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

#[cfg(unix)]
use player_platform_process::configure_background_process_group;

const MAX_FFI_HEADER_BYTES: usize = 32 * 1024 * 1024;

#[cfg(unix)]
const PROCESS_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(10);
#[cfg(unix)]
const PROCESS_TERMINATION_GRACE: std::time::Duration = std::time::Duration::from_millis(500);
#[cfg(unix)]
const PROCESS_REAP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfiHeaderMode {
    Generate,
    Sync,
    Verify,
}

impl FfiHeaderMode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Generate => "generate",
            Self::Sync => "sync",
            Self::Verify => "verify",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfiErrorKind {
    Storage,
    Conformance,
    Worker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfiError {
    kind: FfiErrorKind,
    message: String,
}

impl FfiError {
    fn storage(message: impl Into<String>) -> Self {
        Self {
            kind: FfiErrorKind::Storage,
            message: message.into(),
        }
    }

    fn conformance(message: impl Into<String>) -> Self {
        Self {
            kind: FfiErrorKind::Conformance,
            message: message.into(),
        }
    }

    fn worker(message: impl Into<String>) -> Self {
        Self {
            kind: FfiErrorKind::Worker,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> FfiErrorKind {
        self.kind
    }
}

impl std::fmt::Display for FfiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for FfiError {}

struct HeaderPaths {
    crate_directory: PathBuf,
    config: PathBuf,
    lockfile: PathBuf,
    header: PathBuf,
}

impl HeaderPaths {
    fn new(root: &Path) -> Self {
        let crate_directory = root.join("crates/ffi/player-ffi");
        Self {
            config: crate_directory.join("cbindgen.toml"),
            crate_directory,
            lockfile: root.join("Cargo.lock"),
            header: root.join("include/player_ffi.h"),
        }
    }

    fn validate(&self) -> Result<(), FfiError> {
        require_directory(&self.crate_directory, "player-ffi crate")?;
        require_regular_file(&self.config, "cbindgen configuration")?;
        require_regular_file(&self.lockfile, "Cargo lockfile")?;
        let parent = self.header.parent().ok_or_else(|| {
            FfiError::storage(format!(
                "FFI header path '{}' has no parent directory",
                self.header.display()
            ))
        })?;
        require_directory(parent, "FFI include directory")
    }
}

pub fn run_header(
    root: &Path,
    mode: FfiHeaderMode,
    output: &mut dyn Write,
) -> Result<(), FfiError> {
    let paths = HeaderPaths::new(root);
    paths.validate()?;
    let temporary = tempfile::Builder::new()
        .prefix("player_ffi.")
        .tempdir()
        .map_err(|error| {
            FfiError::storage(format!("failed to create temporary FFI directory: {error}"))
        })?;
    let generated_header = temporary.path().join("player_ffi.h");
    generate_header(root, &paths, mode, &generated_header)?;
    let generated = read_bounded_regular_file(&generated_header, "generated FFI header")?;

    match mode {
        FfiHeaderMode::Generate => {
            atomic_replace_file(&generated_header, &paths.header)?;
            writeln!(output, "Generated {}", paths.header.display()).map_err(output_error)?;
            Ok(())
        }
        FfiHeaderMode::Sync => {
            if read_optional_bounded_regular_file(&paths.header, "checked-in FFI header")?
                .is_some_and(|current| current == generated)
            {
                writeln!(output, "include/player_ffi.h is up to date.").map_err(output_error)?;
                return Ok(());
            }
            atomic_replace_file(&generated_header, &paths.header)?;
            writeln!(output, "Synced {}", paths.header.display()).map_err(output_error)?;
            Ok(())
        }
        FfiHeaderMode::Verify => {
            let current = read_bounded_regular_file(&paths.header, "checked-in FFI header")?;
            if current == generated {
                writeln!(output, "include/player_ffi.h is up to date.").map_err(output_error)?;
                return Ok(());
            }
            write_unified_diff(
                output,
                &paths.header,
                &current,
                &generated_header,
                &generated,
            )?;
            Err(FfiError::conformance(
                "include/player_ffi.h is out of date. Run scripts/vesper ffi sync.",
            ))
        }
    }
}

pub fn run_c_host_smoke(
    root: &Path,
    build_only: bool,
    source: Option<&Path>,
    output: &mut dyn Write,
) -> Result<(), FfiError> {
    #[cfg(not(unix))]
    {
        let _ = (root, build_only, source, output);
        return Err(FfiError::conformance(
            "the C host smoke command is currently supported only on Unix hosts",
        ));
    }

    #[cfg(unix)]
    {
        let source = source
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.join("fixtures/media/tiny-h264-aac.m4v"));

        status_line(output, "[c-host] syncing generated FFI header")?;
        run_header(root, FfiHeaderMode::Sync, output)?;

        status_line(output, "[c-host] building player-ffi")?;
        let cargo = nonempty_environment_command("CARGO", "cargo");
        let mut cargo_command = Command::new(cargo);
        cargo_command
            .args(["build", "-p", "player-ffi"])
            .current_dir(root);
        require_success(&mut cargo_command, "Cargo player-ffi build", None)?;

        status_line(output, "[c-host] compiling examples/c-host/main.c")?;
        let compiler = nonempty_environment_command("CC", "cc");
        let binary = root.join("target/debug/c-host-smoke");
        let mut compiler_command = Command::new(compiler);
        compiler_command
            .arg("examples/c-host/main.c")
            .arg("-Iinclude")
            .arg("-Ltarget/debug")
            .arg("-Wl,-rpath,@executable_path")
            .arg("-lvesper_player_ffi")
            .arg("-o")
            .arg(&binary)
            .current_dir(root);
        require_success(&mut compiler_command, "C host compilation", None)?;
        require_regular_file(&binary, "C host smoke binary")?;

        if build_only {
            status_line(output, "[c-host] built target/debug/c-host-smoke")?;
            return Ok(());
        }

        status_line(
            output,
            &format!(
                "[c-host] running target/debug/c-host-smoke {}",
                source.display()
            ),
        )?;
        let mut smoke_command = Command::new(binary);
        smoke_command.arg(source).current_dir(root);
        require_success(&mut smoke_command, "C host smoke", None)
    }
}

fn generate_header(
    root: &Path,
    paths: &HeaderPaths,
    mode: FfiHeaderMode,
    output: &Path,
) -> Result<(), FfiError> {
    let cbindgen = nonempty_environment_command("CBINDGEN", "cbindgen");
    let mut command = Command::new(cbindgen);
    command
        .arg(&paths.crate_directory)
        .arg("--config")
        .arg(&paths.config)
        .args(["--crate", "player-ffi", "--lang", "c"])
        .arg("--lockfile")
        .arg(&paths.lockfile)
        .arg("--only-target-dependencies")
        .arg("--output")
        .arg(output)
        .current_dir(root);
    require_success(
        &mut command,
        "cbindgen",
        Some(format!(
            "cbindgen is required to {} include/player_ffi.h.\nInstall it with: cargo install cbindgen",
            mode.as_str()
        )),
    )?;
    require_regular_file(output, "generated FFI header")
}

fn nonempty_environment_command(variable: &str, default: &str) -> OsString {
    std::env::var_os(variable)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| OsString::from(default))
}

fn require_directory(path: &Path, label: &str) -> Result<(), FfiError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        FfiError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_dir() {
        return Err(FfiError::storage(format!(
            "{label} '{}' is not a directory",
            path.display()
        )));
    }
    Ok(())
}

fn require_regular_file(path: &Path, label: &str) -> Result<(), FfiError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        FfiError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_file() {
        return Err(FfiError::storage(format!(
            "{label} '{}' is not a regular non-symlink file",
            path.display()
        )));
    }
    Ok(())
}

fn read_optional_bounded_regular_file(
    path: &Path,
    label: &str,
) -> Result<Option<Vec<u8>>, FfiError> {
    match fs::symlink_metadata(path) {
        Ok(_) => read_bounded_regular_file(path, label).map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(FfiError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))),
    }
}

fn read_bounded_regular_file(path: &Path, label: &str) -> Result<Vec<u8>, FfiError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        FfiError::storage(format!(
            "failed to inspect {label} '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_file() {
        return Err(FfiError::storage(format!(
            "{label} '{}' is not a regular non-symlink file",
            path.display()
        )));
    }
    if metadata.len() > MAX_FFI_HEADER_BYTES as u64 {
        return Err(FfiError::storage(format!(
            "{label} '{}' exceeds {MAX_FFI_HEADER_BYTES} bytes",
            path.display()
        )));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)
        .and_then(|file| {
            file.take((MAX_FFI_HEADER_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
        })
        .map_err(|error| {
            FfiError::storage(format!(
                "failed to read {label} '{}': {error}",
                path.display()
            ))
        })?;
    if bytes.len() > MAX_FFI_HEADER_BYTES {
        return Err(FfiError::storage(format!(
            "{label} '{}' exceeds {MAX_FFI_HEADER_BYTES} bytes",
            path.display()
        )));
    }
    Ok(bytes)
}

fn atomic_replace_file(source: &Path, destination: &Path) -> Result<(), FfiError> {
    require_regular_file(source, "generated FFI header")?;
    let source_permissions = fs::metadata(source)
        .map_err(|error| {
            FfiError::storage(format!(
                "failed to inspect generated FFI header permissions '{}': {error}",
                source.display()
            ))
        })?
        .permissions();
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| {
            FfiError::storage(format!(
                "FFI header destination '{}' has no parent directory",
                destination.display()
            ))
        })?;
    require_directory(parent, "FFI include directory")?;
    let mut input = File::open(source).map_err(|error| {
        FfiError::storage(format!(
            "failed to open generated FFI header '{}': {error}",
            source.display()
        ))
    })?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|error| {
        FfiError::storage(format!(
            "failed to create staged FFI header in '{}': {error}",
            parent.display()
        ))
    })?;
    io::copy(&mut input, &mut temporary).map_err(|error| {
        FfiError::storage(format!(
            "failed to stage FFI header for '{}': {error}",
            destination.display()
        ))
    })?;
    temporary
        .as_file()
        .set_permissions(source_permissions)
        .map_err(|error| {
            FfiError::storage(format!(
                "failed to preserve generated FFI header permissions for '{}': {error}",
                destination.display()
            ))
        })?;
    temporary.as_file().sync_all().map_err(|error| {
        FfiError::storage(format!(
            "failed to sync staged FFI header for '{}': {error}",
            destination.display()
        ))
    })?;
    temporary.persist(destination).map_err(|error| {
        FfiError::storage(format!(
            "failed to atomically replace FFI header '{}': {}",
            destination.display(),
            error.error
        ))
    })?;
    Ok(())
}

fn write_unified_diff(
    output: &mut dyn Write,
    current_path: &Path,
    current: &[u8],
    generated_path: &Path,
    generated: &[u8],
) -> Result<(), FfiError> {
    let current = String::from_utf8_lossy(current);
    let generated = String::from_utf8_lossy(generated);
    let current_lines = current.lines().collect::<Vec<_>>();
    let generated_lines = generated.lines().collect::<Vec<_>>();
    let common_prefix = current_lines
        .iter()
        .zip(&generated_lines)
        .take_while(|(left, right)| left == right)
        .count();
    let maximum_suffix = current_lines
        .len()
        .saturating_sub(common_prefix)
        .min(generated_lines.len().saturating_sub(common_prefix));
    let common_suffix = current_lines
        .iter()
        .rev()
        .zip(generated_lines.iter().rev())
        .take(maximum_suffix)
        .take_while(|(left, right)| left == right)
        .count();
    let context_start = common_prefix.saturating_sub(3);
    let current_change_end = current_lines.len().saturating_sub(common_suffix);
    let generated_change_end = generated_lines.len().saturating_sub(common_suffix);
    let current_context_end = current_change_end
        .saturating_add(3)
        .min(current_lines.len());
    let generated_context_end = generated_change_end
        .saturating_add(3)
        .min(generated_lines.len());

    writeln!(output, "--- {}", current_path.display()).map_err(output_error)?;
    writeln!(output, "+++ {}", generated_path.display()).map_err(output_error)?;
    writeln!(
        output,
        "@@ -{},{} +{},{} @@",
        context_start + 1,
        current_context_end.saturating_sub(context_start),
        context_start + 1,
        generated_context_end.saturating_sub(context_start)
    )
    .map_err(output_error)?;
    for line in &current_lines[context_start..common_prefix] {
        writeln!(output, " {line}").map_err(output_error)?;
    }
    for line in &current_lines[common_prefix..current_change_end] {
        writeln!(output, "-{line}").map_err(output_error)?;
    }
    for line in &generated_lines[common_prefix..generated_change_end] {
        writeln!(output, "+{line}").map_err(output_error)?;
    }
    for line in &current_lines[current_change_end..current_context_end] {
        writeln!(output, " {line}").map_err(output_error)?;
    }
    Ok(())
}

#[cfg(unix)]
fn status_line(output: &mut dyn Write, line: &str) -> Result<(), FfiError> {
    writeln!(output, "{line}").map_err(output_error)?;
    output.flush().map_err(output_error)
}

fn output_error(error: io::Error) -> FfiError {
    FfiError::worker(format!("failed to write FFI command output: {error}"))
}

fn require_success(
    command: &mut Command,
    label: &str,
    missing_message: Option<String>,
) -> Result<(), FfiError> {
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let status = run_interruptible(command, label, missing_message)?;
    if status.success() {
        Ok(())
    } else {
        Err(FfiError::conformance(format!(
            "{label} exited unsuccessfully ({status})"
        )))
    }
}

#[cfg(unix)]
fn run_interruptible(
    command: &mut Command,
    label: &str,
    missing_message: Option<String>,
) -> Result<ExitStatus, FfiError> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    configure_background_process_group(command);
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound
            && let Some(message) = missing_message
        {
            return FfiError::storage(message);
        }
        FfiError::worker(format!("failed to spawn {label}: {error}"))
    })?;
    let process_group = i32::try_from(child.id()).map_err(|_| {
        let _ = child.kill();
        let _ = child.wait();
        FfiError::worker(format!(
            "{label} process id cannot be represented as a process group"
        ))
    })?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal_id =
        signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&cancelled)).map_err(
            |error| {
                let _ = terminate_and_reap(&mut child, process_group, false, label);
                FfiError::worker(format!(
                    "failed to install {label} cancellation handler: {error}"
                ))
            },
        )?;
    let signal_guard = SignalRegistration(signal_id);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                drop(signal_guard);
                cleanup_descendants(process_group);
                return Ok(status);
            }
            Ok(None) => {}
            Err(error) => {
                let _ = terminate_and_reap(&mut child, process_group, false, label);
                return Err(FfiError::worker(format!("failed to poll {label}: {error}")));
            }
        }
        if cancelled.load(Ordering::Acquire) {
            terminate_and_reap(&mut child, process_group, true, label)?;
            drop(signal_guard);
            return Err(FfiError::worker(format!("{label} was cancelled")));
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

#[cfg(not(unix))]
fn run_interruptible(
    command: &mut Command,
    label: &str,
    missing_message: Option<String>,
) -> Result<ExitStatus, FfiError> {
    command.status().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound
            && let Some(message) = missing_message
        {
            return FfiError::storage(message);
        }
        FfiError::worker(format!("failed to run {label}: {error}"))
    })
}

#[cfg(unix)]
fn terminate_and_reap(
    child: &mut std::process::Child,
    process_group: i32,
    cancelled: bool,
    label: &str,
) -> Result<(), FfiError> {
    use nix::sys::signal::Signal;
    use std::thread;
    use std::time::Instant;

    let initial_signal = if cancelled {
        Signal::SIGINT
    } else {
        Signal::SIGTERM
    };
    let initial_error = signal_process_group(process_group, initial_signal, label).err();
    if initial_error.is_some() {
        let _ = child.kill();
    }
    let grace_deadline = Instant::now() + PROCESS_TERMINATION_GRACE;
    while Instant::now() < grace_deadline {
        if child
            .try_wait()
            .map_err(|error| FfiError::worker(format!("failed to reap {label}: {error}")))?
            .is_some()
        {
            cleanup_descendants(process_group);
            return initial_error.map_or(Ok(()), Err);
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
    let kill_error = signal_process_group(process_group, Signal::SIGKILL, label).err();
    let _ = child.kill();
    let reap_deadline = Instant::now() + PROCESS_REAP_TIMEOUT;
    while Instant::now() < reap_deadline {
        if child
            .try_wait()
            .map_err(|error| FfiError::worker(format!("failed to reap {label}: {error}")))?
            .is_some()
        {
            return initial_error.or(kill_error).map_or(Ok(()), Err);
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
    Err(FfiError::worker(format!(
        "{label} could not be reaped after termination"
    )))
}

#[cfg(unix)]
fn signal_process_group(
    process_group: i32,
    signal: nix::sys::signal::Signal,
    label: &str,
) -> Result<(), FfiError> {
    use nix::errno::Errno;
    use nix::sys::signal::killpg;
    use nix::unistd::Pid;

    match killpg(Pid::from_raw(process_group), signal) {
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        Err(error) => Err(FfiError::worker(format!(
            "failed to signal {label} process group: {error}"
        ))),
    }
}

#[cfg(unix)]
fn cleanup_descendants(process_group: i32) {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;
    use std::thread;
    use std::time::Duration;

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
    fn unified_diff_keeps_context_and_changed_lines() {
        let mut output = Vec::new();
        write_unified_diff(
            &mut output,
            Path::new("current.h"),
            b"one\ntwo\nthree\n",
            Path::new("generated.h"),
            b"one\nchanged\nthree\n",
        )
        .expect("write diff");
        let output = String::from_utf8(output).expect("UTF-8 diff");
        assert!(output.contains("--- current.h\n+++ generated.h\n"));
        assert!(output.contains("-two\n+changed\n"));
    }
}
