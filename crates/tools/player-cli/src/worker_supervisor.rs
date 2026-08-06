#[cfg(any(unix, windows))]
use std::io::{Read, Write};
#[cfg(any(unix, windows))]
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::sync::Arc;
#[cfg(unix)]
use std::sync::atomic::AtomicBool;
#[cfg(any(unix, windows))]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(any(unix, windows))]
use std::sync::mpsc::{self, Receiver};
#[cfg(any(unix, windows))]
use std::thread;
use std::time::Duration;
#[cfg(any(unix, windows))]
use std::time::Instant;

#[cfg(unix)]
use player_platform_process::configure_background_process_group;

use crate::cli_error::{CliError, CliResult};
use crate::plugin_inspection::PluginInspectionReport;
#[cfg(any(unix, windows))]
use crate::plugin_inspection::PluginWorkerOutputSummary;
use crate::worker_protocol::PluginWorkerRequest;
#[cfg(any(unix, windows))]
use crate::worker_protocol::{
    PLUGIN_WORKER_START_GATE, read_worker_response, write_worker_request,
};

#[cfg(any(unix, windows))]
const MAX_CAPTURED_WORKER_STREAM_BYTES: usize = 256 * 1024;
#[cfg(any(unix, windows))]
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(10);
#[cfg(any(unix, windows))]
const WORKER_TERMINATION_GRACE: Duration = Duration::from_millis(500);
#[cfg(any(unix, windows))]
const WORKER_REAP_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(any(unix, windows))]
const WORKER_DRAIN_GRACE: Duration = Duration::from_millis(250);
#[cfg(any(unix, windows))]
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(any(unix, windows))]
#[derive(Debug)]
struct DrainCapture {
    _prefix: Vec<u8>,
    total_bytes: u64,
    truncated: bool,
    read_error: Option<String>,
}

#[cfg(unix)]
pub fn supervise_native_worker(
    mut request: PluginWorkerRequest,
    timeout: Duration,
) -> CliResult<PluginInspectionReport> {
    request.request_id = next_request_id();
    request.validate().map_err(CliError::worker)?;
    let directory = tempfile::tempdir()
        .map_err(|error| CliError::worker(format!("failed to create worker directory: {error}")))?;
    let request_path = directory.path().join("request.json");
    let response_path = directory.path().join("response.json");
    write_worker_request(&request_path, &request).map_err(CliError::worker)?;

    let executable = std::env::current_exe().map_err(|error| {
        CliError::worker(format!("failed to locate the vesper executable: {error}"))
    })?;
    let mut command = Command::new(executable);
    command
        .arg("__plugin-worker")
        .arg("--request")
        .arg(&request_path)
        .arg("--response")
        .arg(&response_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_background_process_group(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| CliError::worker(format!("failed to spawn plugin worker: {error}")))?;
    let process_group = match i32::try_from(child.id()) {
        Ok(process_group) => process_group,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CliError::worker(
                "plugin worker process id cannot be represented as a process group",
            ));
        }
    };
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            return abort_worker_setup(
                &mut child,
                process_group,
                "plugin worker stdout pipe is missing",
            );
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            return abort_worker_setup(
                &mut child,
                process_group,
                "plugin worker stderr pipe is missing",
            );
        }
    };
    let stdout_capture = start_drain(stdout);
    let stderr_capture = start_drain(stderr);

    let cancelled = Arc::new(AtomicBool::new(false));
    let signal_id =
        match signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&cancelled)) {
            Ok(signal_id) => signal_id,
            Err(error) => {
                return abort_worker_setup(
                    &mut child,
                    process_group,
                    format!("failed to install worker cancellation handler: {error}"),
                );
            }
        };
    let signal_guard = SignalRegistration(signal_id);

    let mut stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => {
            return abort_worker_setup(
                &mut child,
                process_group,
                "plugin worker stdin gate is missing",
            );
        }
    };
    if let Err(error) = stdin
        .write_all(PLUGIN_WORKER_START_GATE)
        .and_then(|()| stdin.flush())
    {
        return abort_worker_setup(
            &mut child,
            process_group,
            format!("failed to release plugin worker: {error}"),
        );
    }
    drop(stdin);

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let poll_error = CliError::worker(format!("failed to poll plugin worker: {error}"));
                let _ = terminate_and_reap(&mut child, process_group, false);
                return Err(poll_error);
            }
        }
        if cancelled.load(Ordering::Acquire) {
            terminate_and_reap(&mut child, process_group, true)?;
            drop(signal_guard);
            return Err(CliError::worker("plugin worker was cancelled"));
        }
        if started.elapsed() >= timeout {
            terminate_and_reap(&mut child, process_group, false)?;
            drop(signal_guard);
            return Err(CliError::worker(format!(
                "plugin worker exceeded its {} ms deadline",
                timeout.as_millis()
            )));
        }
        thread::sleep(WORKER_POLL_INTERVAL);
    };
    drop(signal_guard);

    cleanup_descendants(process_group);
    let capture_deadline = Instant::now() + WORKER_DRAIN_GRACE;
    let stdout = receive_capture(stdout_capture, capture_deadline, "stdout")?;
    let stderr = receive_capture(stderr_capture, capture_deadline, "stderr")?;
    if let Some(error) = stdout.read_error.as_ref().or(stderr.read_error.as_ref()) {
        return Err(CliError::worker(format!(
            "failed to drain plugin worker output: {error}"
        )));
    }
    if !status.success() {
        return Err(CliError::worker(format!(
            "plugin worker exited unsuccessfully ({status}); captured {} stdout bytes and {} stderr bytes",
            stdout.total_bytes, stderr.total_bytes
        )));
    }

    let response = read_worker_response(&response_path).map_err(CliError::worker)?;
    response
        .validate_for_request(request.request_id)
        .map_err(CliError::worker)?;
    Ok(response
        .report
        .with_worker_output(PluginWorkerOutputSummary {
            stdout_bytes: stdout.total_bytes,
            stderr_bytes: stderr.total_bytes,
            stdout_truncated: stdout.truncated,
            stderr_truncated: stderr.truncated,
        }))
}

#[cfg(windows)]
pub fn supervise_native_worker(
    mut request: PluginWorkerRequest,
    timeout: Duration,
) -> CliResult<PluginInspectionReport> {
    use std::os::windows::process::CommandExt;

    use player_platform_process::WindowsJob;

    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    request.request_id = next_request_id();
    request.validate().map_err(CliError::worker)?;
    let directory = tempfile::tempdir()
        .map_err(|error| CliError::worker(format!("failed to create worker directory: {error}")))?;
    let request_path = directory.path().join("request.json");
    let response_path = directory.path().join("response.json");
    write_worker_request(&request_path, &request).map_err(CliError::worker)?;

    let executable = std::env::current_exe().map_err(|error| {
        CliError::worker(format!("failed to locate the vesper executable: {error}"))
    })?;
    let mut command = Command::new(executable);
    command
        .arg("__plugin-worker")
        .arg("--request")
        .arg(&request_path)
        .arg("--response")
        .arg(&response_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NEW_PROCESS_GROUP);
    let mut child = command
        .spawn()
        .map_err(|error| CliError::worker(format!("failed to spawn plugin worker: {error}")))?;
    let job = match WindowsJob::new_kill_on_close() {
        Ok(job) => job,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CliError::worker(format!(
                "failed to create plugin worker Job Object: {error}"
            )));
        }
    };
    if let Err(error) = job.assign_child(&child) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(CliError::worker(format!(
            "failed to contain plugin worker in its Job Object: {error}"
        )));
    }

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            return abort_windows_worker_setup(
                &mut child,
                &job,
                "plugin worker stdout pipe is missing",
            );
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            return abort_windows_worker_setup(
                &mut child,
                &job,
                "plugin worker stderr pipe is missing",
            );
        }
    };
    let stdout_capture = start_drain(stdout);
    let stderr_capture = start_drain(stderr);
    let cancellation = match crate::external_process::InterruptDeferral::start("plugin worker") {
        Ok(cancellation) => cancellation,
        Err(error) => {
            return abort_windows_worker_setup(&mut child, &job, error.to_string());
        }
    };

    let mut stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => {
            let _ = cancellation.finish();
            return abort_windows_worker_setup(
                &mut child,
                &job,
                "plugin worker stdin gate is missing",
            );
        }
    };
    if let Err(error) = stdin
        .write_all(PLUGIN_WORKER_START_GATE)
        .and_then(|()| stdin.flush())
    {
        let _ = cancellation.finish();
        return abort_windows_worker_setup(
            &mut child,
            &job,
            format!("failed to release plugin worker: {error}"),
        );
    }
    drop(stdin);

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let poll_error = CliError::worker(format!("failed to poll plugin worker: {error}"));
                let _ = terminate_windows_worker(&mut child, &job);
                let _ = cancellation.finish();
                return Err(poll_error);
            }
        }
        if cancellation.is_cancelled() {
            terminate_windows_worker(&mut child, &job)?;
            let _ = cancellation.finish();
            return Err(CliError::worker("plugin worker was cancelled"));
        }
        if started.elapsed() >= timeout {
            terminate_windows_worker(&mut child, &job)?;
            let _ = cancellation.finish();
            return Err(CliError::worker(format!(
                "plugin worker exceeded its {} ms deadline",
                timeout.as_millis()
            )));
        }
        thread::sleep(WORKER_POLL_INTERVAL);
    };
    let cancelled = cancellation.finish();

    // Closing a KILL_ON_JOB_CLOSE Job Object after the root exits terminates any descendants that
    // outlived it before their inherited stdout or stderr handles can stall output draining.
    drop(job);
    let capture_deadline = Instant::now() + WORKER_DRAIN_GRACE;
    let stdout = receive_capture(stdout_capture, capture_deadline, "stdout")?;
    let stderr = receive_capture(stderr_capture, capture_deadline, "stderr")?;
    if cancelled {
        return Err(CliError::worker("plugin worker was cancelled"));
    }
    if let Some(error) = stdout.read_error.as_ref().or(stderr.read_error.as_ref()) {
        return Err(CliError::worker(format!(
            "failed to drain plugin worker output: {error}"
        )));
    }
    if !status.success() {
        return Err(CliError::worker(format!(
            "plugin worker exited unsuccessfully ({status}); captured {} stdout bytes and {} stderr bytes",
            stdout.total_bytes, stderr.total_bytes
        )));
    }

    let response = read_worker_response(&response_path).map_err(CliError::worker)?;
    response
        .validate_for_request(request.request_id)
        .map_err(CliError::worker)?;
    Ok(response
        .report
        .with_worker_output(PluginWorkerOutputSummary {
            stdout_bytes: stdout.total_bytes,
            stderr_bytes: stderr.total_bytes,
            stdout_truncated: stdout.truncated,
            stderr_truncated: stderr.truncated,
        }))
}

#[cfg(not(any(unix, windows)))]
pub fn supervise_native_worker(
    _request: PluginWorkerRequest,
    _timeout: Duration,
) -> CliResult<PluginInspectionReport> {
    Err(CliError::worker(
        "native plugin worker containment is not implemented for this platform",
    ))
}

#[cfg(any(unix, windows))]
fn next_request_id() -> u64 {
    let sequence = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    (u64::from(std::process::id()) << 32) | sequence.max(1)
}

#[cfg(any(unix, windows))]
fn start_drain<R>(mut reader: R) -> Receiver<DrainCapture>
where
    R: Read + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut prefix = Vec::with_capacity(MAX_CAPTURED_WORKER_STREAM_BYTES);
        let mut total_bytes = 0_u64;
        let mut read_error = None;
        let mut buffer = [0_u8; 8 * 1024];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    total_bytes = total_bytes.saturating_add(count as u64);
                    let remaining = MAX_CAPTURED_WORKER_STREAM_BYTES.saturating_sub(prefix.len());
                    prefix.extend_from_slice(&buffer[..count.min(remaining)]);
                }
                Err(error) => {
                    read_error = Some(error.to_string());
                    break;
                }
            }
        }
        let _ = sender.send(DrainCapture {
            _prefix: prefix,
            total_bytes,
            truncated: total_bytes > MAX_CAPTURED_WORKER_STREAM_BYTES as u64,
            read_error,
        });
    });
    receiver
}

#[cfg(any(unix, windows))]
fn receive_capture(
    receiver: Receiver<DrainCapture>,
    deadline: Instant,
    stream: &str,
) -> CliResult<DrainCapture> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    receiver.recv_timeout(remaining).map_err(|error| {
        CliError::worker(format!(
            "plugin worker {stream} did not finish draining within {} ms: {error}",
            WORKER_DRAIN_GRACE.as_millis()
        ))
    })
}

#[cfg(windows)]
fn abort_windows_worker_setup<T>(
    child: &mut std::process::Child,
    job: &player_platform_process::WindowsJob,
    message: impl Into<String>,
) -> CliResult<T> {
    let message = message.into();
    match terminate_windows_worker(child, job) {
        Ok(()) => Err(CliError::worker(message)),
        Err(cleanup_error) => Err(CliError::worker(format!(
            "{message}; worker cleanup also failed: {cleanup_error}"
        ))),
    }
}

#[cfg(windows)]
fn terminate_windows_worker(
    child: &mut std::process::Child,
    job: &player_platform_process::WindowsJob,
) -> CliResult<()> {
    if child
        .try_wait()
        .map_err(|error| CliError::worker(format!("failed to poll plugin worker: {error}")))?
        .is_some()
    {
        return Ok(());
    }

    let termination_error = job.terminate(1).err();
    if termination_error.is_some() {
        let _ = child.kill();
    }
    let deadline = Instant::now() + WORKER_TERMINATION_GRACE + WORKER_REAP_TIMEOUT;
    while Instant::now() < deadline {
        if child
            .try_wait()
            .map_err(|error| CliError::worker(format!("failed to reap plugin worker: {error}")))?
            .is_some()
        {
            return match termination_error {
                Some(error) => Err(CliError::worker(format!(
                    "failed to terminate plugin worker Job Object: {error}"
                ))),
                None => Ok(()),
            };
        }
        thread::sleep(WORKER_POLL_INTERVAL);
    }
    Err(CliError::worker(
        "plugin worker could not be reaped after Job Object termination",
    ))
}

#[cfg(unix)]
fn abort_worker_setup<T>(
    child: &mut std::process::Child,
    process_group: i32,
    message: impl Into<String>,
) -> CliResult<T> {
    let message = message.into();
    match terminate_and_reap(child, process_group, false) {
        Ok(()) => Err(CliError::worker(message)),
        Err(cleanup_error) => Err(CliError::worker(format!(
            "{message}; worker cleanup also failed: {cleanup_error}"
        ))),
    }
}

#[cfg(unix)]
fn terminate_and_reap(
    child: &mut std::process::Child,
    process_group: i32,
    cancelled: bool,
) -> CliResult<()> {
    use nix::sys::signal::Signal;

    let initial_signal = if cancelled {
        Signal::SIGINT
    } else {
        Signal::SIGTERM
    };
    let initial_signal_error = signal_process_group(process_group, initial_signal).err();
    if initial_signal_error.is_some() {
        let _ = child.kill();
    }
    let grace_deadline = Instant::now() + WORKER_TERMINATION_GRACE;
    while Instant::now() < grace_deadline {
        if child
            .try_wait()
            .map_err(|error| CliError::worker(format!("failed to reap plugin worker: {error}")))?
            .is_some()
        {
            cleanup_descendants(process_group);
            return match initial_signal_error {
                Some(error) => Err(error),
                None => Ok(()),
            };
        }
        thread::sleep(WORKER_POLL_INTERVAL);
    }

    let kill_error = signal_process_group(process_group, Signal::SIGKILL).err();
    let _ = child.kill();
    let reap_deadline = Instant::now() + WORKER_REAP_TIMEOUT;
    while Instant::now() < reap_deadline {
        if child
            .try_wait()
            .map_err(|error| CliError::worker(format!("failed to reap plugin worker: {error}")))?
            .is_some()
        {
            return match initial_signal_error.or(kill_error) {
                Some(error) => Err(error),
                None => Ok(()),
            };
        }
        thread::sleep(WORKER_POLL_INTERVAL);
    }
    Err(CliError::worker(
        "plugin worker could not be reaped after termination",
    ))
}

#[cfg(unix)]
fn signal_process_group(process_group: i32, signal: nix::sys::signal::Signal) -> CliResult<()> {
    use nix::errno::Errno;
    use nix::sys::signal::killpg;
    use nix::unistd::Pid;

    match killpg(Pid::from_raw(process_group), signal) {
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        Err(error) => Err(CliError::worker(format!(
            "failed to signal plugin worker process group: {error}"
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
