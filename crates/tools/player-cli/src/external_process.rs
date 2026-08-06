use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::JoinHandle;

use std::sync::Arc;
#[cfg(any(unix, windows))]
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(test)]
use std::sync::{Mutex, MutexGuard};
#[cfg(any(unix, windows))]
use std::time::{Duration, Instant};

#[cfg(unix)]
use player_platform_process::configure_background_process_group;

#[cfg(any(unix, windows))]
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);
#[cfg(any(unix, windows))]
const PROCESS_TERMINATION_GRACE: Duration = Duration::from_millis(500);
#[cfg(any(unix, windows))]
const PROCESS_REAP_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct ExternalProcessError {
    kind: ExternalProcessErrorKind,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExternalProcessErrorKind {
    Compatibility,
    Worker,
    Cancelled,
}

#[derive(Debug)]
pub(crate) struct BoundedProcessOutput {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

#[derive(Debug)]
enum CaptureReadError {
    Overflow {
        stream: &'static str,
        maximum_bytes: usize,
    },
    Read {
        stream: &'static str,
        source: io::Error,
    },
}

struct CaptureReaders {
    stdout: JoinHandle<Result<Vec<u8>, CaptureReadError>>,
    stderr: JoinHandle<Result<Vec<u8>, CaptureReadError>>,
    failed: Arc<AtomicBool>,
}

impl ExternalProcessError {
    fn compatibility(message: impl Into<String>) -> Self {
        Self {
            kind: ExternalProcessErrorKind::Compatibility,
            message: message.into(),
        }
    }

    fn worker(message: impl Into<String>) -> Self {
        Self {
            kind: ExternalProcessErrorKind::Worker,
            message: message.into(),
        }
    }

    fn cancelled(message: impl Into<String>) -> Self {
        Self {
            kind: ExternalProcessErrorKind::Cancelled,
            message: message.into(),
        }
    }

    pub(crate) const fn kind(&self) -> ExternalProcessErrorKind {
        self.kind
    }
}

#[cfg(any(unix, windows))]
fn cancellation_state(label: &str) -> Result<&'static CancellationState, ExternalProcessError> {
    static CANCELLATION: OnceLock<Result<CancellationState, String>> = OnceLock::new();

    match CANCELLATION.get_or_init(|| {
        let cancelled = Arc::new(AtomicBool::new(false));
        let use_default = Arc::new(AtomicBool::new(true));
        signal_hook::flag::register_conditional_default(
            signal_hook::consts::SIGINT,
            Arc::clone(&use_default),
        )
        .and_then(|_| {
            signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&cancelled))
        })
        .map(|_| CancellationState {
            cancelled,
            use_default,
        })
        .map_err(|error| error.to_string())
    }) {
        Ok(state) => Ok(state),
        Err(error) => Err(ExternalProcessError::compatibility(format!(
            "failed to install {label} cancellation handler: {error}"
        ))),
    }
}

#[cfg(any(unix, windows))]
struct CancellationState {
    cancelled: Arc<AtomicBool>,
    use_default: Arc<AtomicBool>,
}

#[cfg(any(unix, windows))]
struct CancellationScope {
    state: &'static CancellationState,
    finished: bool,
    #[cfg(test)]
    test_guard: Option<MutexGuard<'static, ()>>,
}

#[cfg(test)]
static CANCELLATION_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(any(unix, windows))]
impl CancellationScope {
    fn start(label: &str) -> Result<Self, ExternalProcessError> {
        // The production signal handler intentionally permits only one active
        // supervised operation. Unit tests exercise several independent
        // modules in parallel, so serialize their scopes without changing the
        // fail-fast production behavior.
        #[cfg(test)]
        let test_guard = Some(
            CANCELLATION_TEST_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        let state = cancellation_state(label)?;
        state
            .use_default
            .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| {
                ExternalProcessError::worker(format!(
                    "cannot start {label} while another supervised process is active"
                ))
            })?;
        Ok(Self {
            state,
            finished: false,
            #[cfg(test)]
            test_guard,
        })
    }

    fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire)
    }

    fn finish(mut self) -> bool {
        self.state.use_default.store(true, Ordering::SeqCst);
        let cancelled = self.state.cancelled.swap(false, Ordering::SeqCst);
        self.finished = true;
        #[cfg(test)]
        self.test_guard.take();
        cancelled
    }
}

#[cfg(any(unix, windows))]
impl Drop for CancellationScope {
    fn drop(&mut self) {
        if !self.finished {
            self.state.use_default.store(true, Ordering::SeqCst);
            self.state.cancelled.store(false, Ordering::SeqCst);
        }
        #[cfg(test)]
        self.test_guard.take();
    }
}

#[cfg(any(unix, windows))]
pub(crate) struct InterruptDeferral {
    scope: CancellationScope,
}

#[cfg(any(unix, windows))]
impl InterruptDeferral {
    pub(crate) fn start(label: &str) -> Result<Self, ExternalProcessError> {
        CancellationScope::start(label).map(|scope| Self { scope })
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.scope.is_cancelled()
    }

    pub(crate) fn finish(self) -> bool {
        self.scope.finish()
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) struct InterruptDeferral;

#[cfg(not(any(unix, windows)))]
impl InterruptDeferral {
    pub(crate) fn start(_label: &str) -> Result<Self, ExternalProcessError> {
        Ok(Self)
    }

    pub(crate) const fn is_cancelled(&self) -> bool {
        false
    }

    pub(crate) const fn finish(self) -> bool {
        false
    }
}

impl CaptureReaders {
    fn start(
        child: &mut Child,
        stdout_maximum_bytes: usize,
        stderr_maximum_bytes: usize,
    ) -> Result<Self, ExternalProcessError> {
        let stdout = child.stdout.take().ok_or_else(|| {
            ExternalProcessError::worker("bounded process stdout pipe is unavailable")
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            ExternalProcessError::worker("bounded process stderr pipe is unavailable")
        })?;
        let failed = Arc::new(AtomicBool::new(false));
        let stdout_reader =
            spawn_capture_reader(stdout, "stdout", stdout_maximum_bytes, Arc::clone(&failed))?;
        let stderr_reader =
            spawn_capture_reader(stderr, "stderr", stderr_maximum_bytes, Arc::clone(&failed))?;
        Ok(Self {
            stdout: stdout_reader,
            stderr: stderr_reader,
            failed,
        })
    }

    fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    fn finish(self, label: &str) -> Result<(Vec<u8>, Vec<u8>), ExternalProcessError> {
        let stdout = join_capture_reader(self.stdout, label)?;
        let stderr = join_capture_reader(self.stderr, label)?;
        Ok((stdout, stderr))
    }
}

fn spawn_capture_reader(
    mut stream: impl Read + Send + 'static,
    stream_name: &'static str,
    maximum_bytes: usize,
    failed: Arc<AtomicBool>,
) -> Result<JoinHandle<Result<Vec<u8>, CaptureReadError>>, ExternalProcessError> {
    std::thread::Builder::new()
        .name(format!("vesper-{stream_name}-capture"))
        .spawn(move || {
            let mut output = Vec::with_capacity(maximum_bytes.min(64 * 1024));
            let mut buffer = [0_u8; 8192];
            loop {
                let count = match stream.read(&mut buffer) {
                    Ok(count) => count,
                    Err(source) => {
                        failed.store(true, Ordering::Release);
                        return Err(CaptureReadError::Read {
                            stream: stream_name,
                            source,
                        });
                    }
                };
                if count == 0 {
                    return Ok(output);
                }
                if count > maximum_bytes.saturating_sub(output.len()) {
                    failed.store(true, Ordering::Release);
                    return Err(CaptureReadError::Overflow {
                        stream: stream_name,
                        maximum_bytes,
                    });
                }
                output.extend_from_slice(&buffer[..count]);
            }
        })
        .map_err(|error| {
            ExternalProcessError::worker(format!(
                "failed to start bounded {stream_name} capture: {error}"
            ))
        })
}

fn join_capture_reader(
    reader: JoinHandle<Result<Vec<u8>, CaptureReadError>>,
    label: &str,
) -> Result<Vec<u8>, ExternalProcessError> {
    match reader.join() {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(CaptureReadError::Overflow {
            stream,
            maximum_bytes,
        })) => Err(ExternalProcessError::worker(format!(
            "{label} {stream} exceeds {maximum_bytes} bytes"
        ))),
        Ok(Err(CaptureReadError::Read { stream, source })) => Err(ExternalProcessError::worker(
            format!("failed to read {label} {stream}: {source}"),
        )),
        Err(_) => Err(ExternalProcessError::worker(format!(
            "{label} output capture thread panicked"
        ))),
    }
}

#[cfg(any(unix, windows))]
fn supervise_captured_child(
    mut child: Child,
    cancellation: &CancellationScope,
    readers: CaptureReaders,
    label: &str,
    deadline: Option<Instant>,
    mut terminate: impl FnMut(&mut Child, bool) -> Result<(), ExternalProcessError>,
    mut cleanup_after_exit: impl FnMut(),
) -> Result<BoundedProcessOutput, ExternalProcessError> {
    loop {
        if cancellation.is_cancelled() {
            let termination = terminate(&mut child, true);
            let _ = readers.finish(label);
            termination?;
            return Err(ExternalProcessError::cancelled(format!(
                "{label} was cancelled"
            )));
        }
        if readers.failed() {
            let termination = terminate(&mut child, false);
            let capture = readers.finish(label);
            termination?;
            return match capture {
                Err(error) => Err(error),
                Ok(_) => Err(ExternalProcessError::worker(format!(
                    "{label} output capture failed"
                ))),
            };
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                cleanup_after_exit();
                let capture = readers.finish(label);
                if cancellation.is_cancelled() {
                    return Err(ExternalProcessError::cancelled(format!(
                        "{label} was cancelled"
                    )));
                }
                let (stdout, stderr) = capture?;
                if status.code().is_none() {
                    return Err(ExternalProcessError::worker(format!(
                        "{label} terminated without an exit code"
                    )));
                }
                return Ok(BoundedProcessOutput {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) if deadline.is_some_and(|deadline| Instant::now() >= deadline) => {
                let termination = terminate(&mut child, false);
                let _ = readers.finish(label);
                termination?;
                return Err(ExternalProcessError::worker(format!(
                    "{label} exceeded its execution deadline"
                )));
            }
            Ok(None) => std::thread::sleep(PROCESS_POLL_INTERVAL),
            Err(error) => {
                let _ = terminate(&mut child, false);
                let _ = readers.finish(label);
                return Err(ExternalProcessError::worker(format!(
                    "failed to poll {label}: {error}"
                )));
            }
        }
    }
}

#[cfg(unix)]
pub(crate) fn run_interruptible_capture(
    command: &mut Command,
    label: &str,
    stdout_maximum_bytes: usize,
    stderr_maximum_bytes: usize,
) -> Result<BoundedProcessOutput, ExternalProcessError> {
    let cancellation = CancellationScope::start(label)?;
    let result = run_interruptible_capture_in_scope(
        command,
        label,
        stdout_maximum_bytes,
        stderr_maximum_bytes,
        &cancellation,
        PROCESS_TERMINATION_GRACE,
        None,
    );
    finish_capture_scope(result, cancellation, label)
}

#[cfg(unix)]
pub(crate) fn run_interruptible_capture_with_timeout(
    command: &mut Command,
    label: &str,
    stdout_maximum_bytes: usize,
    stderr_maximum_bytes: usize,
    timeout: Duration,
) -> Result<BoundedProcessOutput, ExternalProcessError> {
    let cancellation = CancellationScope::start(label)?;
    let result = run_interruptible_capture_in_scope(
        command,
        label,
        stdout_maximum_bytes,
        stderr_maximum_bytes,
        &cancellation,
        PROCESS_TERMINATION_GRACE,
        Some(Instant::now() + timeout),
    );
    finish_capture_scope(result, cancellation, label)
}

#[cfg(unix)]
pub(crate) fn run_interruptible_capture_in_deferral(
    command: &mut Command,
    label: &str,
    stdout_maximum_bytes: usize,
    stderr_maximum_bytes: usize,
    cancellation: &InterruptDeferral,
) -> Result<BoundedProcessOutput, ExternalProcessError> {
    run_interruptible_capture_in_scope(
        command,
        label,
        stdout_maximum_bytes,
        stderr_maximum_bytes,
        &cancellation.scope,
        PROCESS_TERMINATION_GRACE,
        None,
    )
}

#[cfg(unix)]
fn run_interruptible_capture_in_scope(
    command: &mut Command,
    label: &str,
    stdout_maximum_bytes: usize,
    stderr_maximum_bytes: usize,
    cancellation: &CancellationScope,
    termination_grace: Duration,
    deadline: Option<Instant>,
) -> Result<BoundedProcessOutput, ExternalProcessError> {
    configure_background_process_group(command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if cancellation.is_cancelled() {
        return Err(ExternalProcessError::cancelled(format!(
            "{label} was cancelled"
        )));
    }
    let mut child = command.spawn().map_err(|error| {
        ExternalProcessError::compatibility(format!("failed to spawn {label}: {error}"))
    })?;
    let process_group = i32::try_from(child.id()).map_err(|_| {
        let _ = child.kill();
        let _ = child.wait();
        ExternalProcessError::worker(format!(
            "{label} process id cannot be represented as a process group"
        ))
    })?;
    let readers =
        match CaptureReaders::start(&mut child, stdout_maximum_bytes, stderr_maximum_bytes) {
            Ok(readers) => readers,
            Err(error) => {
                let _ = terminate_and_reap_with_grace(
                    &mut child,
                    process_group,
                    false,
                    label,
                    termination_grace,
                );
                return Err(error);
            }
        };
    supervise_captured_child(
        child,
        cancellation,
        readers,
        label,
        deadline,
        |child, cancelled| {
            terminate_and_reap_with_grace(child, process_group, cancelled, label, termination_grace)
        },
        || cleanup_descendants(process_group),
    )
}

#[cfg(any(unix, windows))]
fn finish_capture_scope(
    result: Result<BoundedProcessOutput, ExternalProcessError>,
    cancellation: CancellationScope,
    label: &str,
) -> Result<BoundedProcessOutput, ExternalProcessError> {
    let cancelled = cancellation.finish();
    match (result, cancelled) {
        (Ok(_), true) => Err(ExternalProcessError::cancelled(format!(
            "{label} was cancelled"
        ))),
        (result, _) => result,
    }
}

#[cfg(windows)]
pub(crate) fn run_interruptible_capture(
    command: &mut Command,
    label: &str,
    stdout_maximum_bytes: usize,
    stderr_maximum_bytes: usize,
) -> Result<BoundedProcessOutput, ExternalProcessError> {
    use std::os::windows::process::CommandExt;

    command
        .creation_flags(CREATE_NEW_PROCESS_GROUP)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let cancellation = CancellationScope::start(label)?;
    if cancellation.is_cancelled() {
        cancellation.finish();
        return Err(ExternalProcessError::cancelled(format!(
            "{label} was cancelled"
        )));
    }
    let mut child = command.spawn().map_err(|error| {
        ExternalProcessError::compatibility(format!("failed to spawn {label}: {error}"))
    })?;
    let readers =
        match CaptureReaders::start(&mut child, stdout_maximum_bytes, stderr_maximum_bytes) {
            Ok(readers) => readers,
            Err(error) => {
                let _ = terminate_windows_tree(&mut child, label);
                cancellation.finish();
                return Err(error);
            }
        };
    let result = supervise_captured_child(
        child,
        &cancellation,
        readers,
        label,
        None,
        |child, _cancelled| terminate_windows_tree(child, label),
        || {},
    );
    finish_capture_scope(result, cancellation, label)
}

#[cfg(windows)]
pub(crate) fn run_interruptible_capture_with_timeout(
    command: &mut Command,
    label: &str,
    stdout_maximum_bytes: usize,
    stderr_maximum_bytes: usize,
    timeout: Duration,
) -> Result<BoundedProcessOutput, ExternalProcessError> {
    use std::os::windows::process::CommandExt;

    command
        .creation_flags(CREATE_NEW_PROCESS_GROUP)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let cancellation = CancellationScope::start(label)?;
    if cancellation.is_cancelled() {
        cancellation.finish();
        return Err(ExternalProcessError::cancelled(format!(
            "{label} was cancelled"
        )));
    }
    let mut child = command.spawn().map_err(|error| {
        ExternalProcessError::compatibility(format!("failed to spawn {label}: {error}"))
    })?;
    let readers =
        match CaptureReaders::start(&mut child, stdout_maximum_bytes, stderr_maximum_bytes) {
            Ok(readers) => readers,
            Err(error) => {
                let _ = terminate_windows_tree(&mut child, label);
                cancellation.finish();
                return Err(error);
            }
        };
    let result = supervise_captured_child(
        child,
        &cancellation,
        readers,
        label,
        Some(Instant::now() + timeout),
        |child, _cancelled| terminate_windows_tree(child, label),
        || {},
    );
    finish_capture_scope(result, cancellation, label)
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn run_interruptible_capture(
    command: &mut Command,
    label: &str,
    stdout_maximum_bytes: usize,
    stderr_maximum_bytes: usize,
) -> Result<BoundedProcessOutput, ExternalProcessError> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        ExternalProcessError::compatibility(format!("failed to spawn {label}: {error}"))
    })?;
    let readers = CaptureReaders::start(&mut child, stdout_maximum_bytes, stderr_maximum_bytes)?;
    loop {
        if readers.failed() {
            let _ = child.kill();
            let _ = child.wait();
            return match readers.finish(label) {
                Err(error) => Err(error),
                Ok(_) => Err(ExternalProcessError::worker(format!(
                    "{label} output capture failed"
                ))),
            };
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                let (stdout, stderr) = readers.finish(label)?;
                return Ok(BoundedProcessOutput {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = readers.finish(label);
                return Err(ExternalProcessError::worker(format!(
                    "failed to poll {label}: {error}"
                )));
            }
        }
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn run_interruptible_capture_with_timeout(
    command: &mut Command,
    label: &str,
    stdout_maximum_bytes: usize,
    stderr_maximum_bytes: usize,
    _timeout: std::time::Duration,
) -> Result<BoundedProcessOutput, ExternalProcessError> {
    run_interruptible_capture(command, label, stdout_maximum_bytes, stderr_maximum_bytes)
}

#[cfg(unix)]
pub(crate) fn run_interruptible(
    command: &mut Command,
    label: &str,
) -> Result<ExitStatus, ExternalProcessError> {
    run_interruptible_after_registration(command, label, || {})
}

#[cfg(unix)]
pub(crate) fn run_interruptible_in_deferral(
    command: &mut Command,
    label: &str,
    cancellation: &InterruptDeferral,
) -> Result<ExitStatus, ExternalProcessError> {
    run_interruptible_in_scope(command, label, &cancellation.scope)
}

#[cfg(windows)]
pub(crate) fn run_interruptible_in_deferral(
    command: &mut Command,
    label: &str,
    cancellation: &InterruptDeferral,
) -> Result<ExitStatus, ExternalProcessError> {
    run_interruptible_windows_in_scope(command, label, &cancellation.scope)
}

#[cfg(unix)]
pub(crate) fn run_inherited_process_group(
    command: &mut Command,
    label: &str,
) -> Result<ExitStatus, ExternalProcessError> {
    command.status().map_err(|error| {
        ExternalProcessError::compatibility(format!("failed to run {label}: {error}"))
    })
}

#[cfg(not(unix))]
pub(crate) fn run_inherited_process_group(
    command: &mut Command,
    label: &str,
) -> Result<ExitStatus, ExternalProcessError> {
    command.status().map_err(|error| {
        ExternalProcessError::compatibility(format!("failed to run {label}: {error}"))
    })
}

#[cfg(unix)]
fn run_interruptible_after_registration(
    command: &mut Command,
    label: &str,
    after_registration: impl FnOnce(),
) -> Result<ExitStatus, ExternalProcessError> {
    let cancellation = CancellationScope::start(label)?;
    after_registration();
    let result = run_interruptible_in_scope(command, label, &cancellation);
    let cancelled = cancellation.finish();
    match (result, cancelled) {
        (Ok(_), true) => Err(ExternalProcessError::cancelled(format!(
            "{label} was cancelled"
        ))),
        (result, _) => result,
    }
}

#[cfg(unix)]
fn run_interruptible_in_scope(
    command: &mut Command,
    label: &str,
    cancellation: &CancellationScope,
) -> Result<ExitStatus, ExternalProcessError> {
    // A supervised child runs outside the CLI's foreground process group, so
    // inheriting terminal input can suspend it with SIGTTIN. External commands
    // used by the CLI are non-interactive and must observe EOF instead.
    configure_background_process_group(command);
    command.stdin(Stdio::null());
    if cancellation.is_cancelled() {
        return Err(ExternalProcessError::cancelled(format!(
            "{label} was cancelled"
        )));
    }

    let mut child = command.spawn().map_err(|error| {
        ExternalProcessError::compatibility(format!("failed to spawn {label}: {error}"))
    })?;
    let process_group = i32::try_from(child.id()).map_err(|_| {
        let _ = child.kill();
        let _ = child.wait();
        ExternalProcessError::worker(format!(
            "{label} process id cannot be represented as a process group"
        ))
    })?;
    loop {
        if cancellation.is_cancelled() {
            let termination = terminate_and_reap(&mut child, process_group, true, label);
            termination?;
            return Err(ExternalProcessError::cancelled(format!(
                "{label} was cancelled"
            )));
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                cleanup_descendants(process_group);
                if cancellation.is_cancelled() {
                    return Err(ExternalProcessError::cancelled(format!(
                        "{label} was cancelled"
                    )));
                }
                return Ok(status);
            }
            Ok(None) => {}
            Err(error) => {
                abort_process(&mut child, process_group, label);
                return Err(ExternalProcessError::worker(format!(
                    "failed to poll {label}: {error}"
                )));
            }
        }
        std::thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

#[cfg(windows)]
fn run_interruptible_windows_in_scope(
    command: &mut Command,
    label: &str,
    cancellation: &CancellationScope,
) -> Result<ExitStatus, ExternalProcessError> {
    use std::os::windows::process::CommandExt;

    command.creation_flags(CREATE_NEW_PROCESS_GROUP);
    if cancellation.is_cancelled() {
        return Err(ExternalProcessError::cancelled(format!(
            "{label} was cancelled"
        )));
    }

    let mut child = command.spawn().map_err(|error| {
        ExternalProcessError::compatibility(format!("failed to spawn {label}: {error}"))
    })?;
    loop {
        if cancellation.is_cancelled() {
            let termination = terminate_windows_tree(&mut child, label);
            termination?;
            return Err(ExternalProcessError::cancelled(format!(
                "{label} was cancelled"
            )));
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if cancellation.is_cancelled() {
                    return Err(ExternalProcessError::cancelled(format!(
                        "{label} was cancelled"
                    )));
                }
                return Ok(status);
            }
            Ok(None) => std::thread::sleep(PROCESS_POLL_INTERVAL),
            Err(error) => {
                let _ = terminate_windows_tree(&mut child, label);
                return Err(ExternalProcessError::worker(format!(
                    "failed to poll {label}: {error}"
                )));
            }
        }
    }
}

#[cfg(windows)]
pub(crate) fn run_interruptible(
    command: &mut Command,
    label: &str,
) -> Result<ExitStatus, ExternalProcessError> {
    let cancellation = CancellationScope::start(label)?;
    let result = run_interruptible_windows_in_scope(command, label, &cancellation);
    let cancelled = cancellation.finish();
    match (result, cancelled) {
        (Ok(_), true) => Err(ExternalProcessError::cancelled(format!(
            "{label} was cancelled"
        ))),
        (result, _) => result,
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn run_interruptible(
    command: &mut Command,
    label: &str,
) -> Result<ExitStatus, ExternalProcessError> {
    command.status().map_err(|error| {
        ExternalProcessError::compatibility(format!("failed to run {label}: {error}"))
    })
}

#[cfg(windows)]
fn terminate_windows_tree(
    child: &mut std::process::Child,
    label: &str,
) -> Result<(), ExternalProcessError> {
    use std::time::Instant;

    if child
        .try_wait()
        .map_err(|error| ExternalProcessError::worker(format!("failed to poll {label}: {error}")))?
        .is_some()
    {
        return Ok(());
    }
    let taskkill_error = run_windows_taskkill(child.id(), label);
    let direct_kill_error = taskkill_error.as_ref().and_then(|_| {
        child.kill().err().map(|error| {
            ExternalProcessError::worker(format!("failed to terminate {label}: {error}"))
        })
    });
    let reap_deadline = Instant::now() + PROCESS_REAP_TIMEOUT + PROCESS_TERMINATION_GRACE;
    while Instant::now() < reap_deadline {
        if child
            .try_wait()
            .map_err(|error| {
                ExternalProcessError::worker(format!("failed to reap {label}: {error}"))
            })?
            .is_some()
        {
            return taskkill_error.or(direct_kill_error).map_or(Ok(()), Err);
        }
        std::thread::sleep(PROCESS_POLL_INTERVAL);
    }
    Err(ExternalProcessError::worker(format!(
        "{label} could not be reaped after termination"
    )))
}

#[cfg(windows)]
fn run_windows_taskkill(process_id: u32, label: &str) -> Option<ExternalProcessError> {
    use std::process::Stdio;
    use std::time::Instant;

    let Some(taskkill_path) = std::env::var_os("SystemRoot")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .map(|root| root.join("System32/taskkill.exe"))
    else {
        return Some(ExternalProcessError::worker(format!(
            "failed to locate taskkill while terminating {label}: SystemRoot is unavailable"
        )));
    };
    let process_id = process_id.to_string();
    let mut taskkill = match Command::new(&taskkill_path)
        .args(["/PID", &process_id, "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(taskkill) => taskkill,
        Err(error) => {
            return Some(ExternalProcessError::worker(format!(
                "failed to run taskkill for {label} using '{}': {error}",
                taskkill_path.display()
            )));
        }
    };
    let taskkill_deadline = Instant::now() + PROCESS_REAP_TIMEOUT;
    loop {
        match taskkill.try_wait() {
            Ok(Some(status)) if status.success() => return None,
            Ok(Some(status)) => {
                return Some(ExternalProcessError::worker(format!(
                    "failed to terminate {label} process tree ({status})"
                )));
            }
            Ok(None) if Instant::now() < taskkill_deadline => {
                std::thread::sleep(PROCESS_POLL_INTERVAL);
            }
            Ok(None) => {
                let _ = taskkill.kill();
                let _ = taskkill.wait();
                return Some(ExternalProcessError::worker(format!(
                    "taskkill timed out while terminating {label}"
                )));
            }
            Err(error) => {
                let _ = taskkill.kill();
                let _ = taskkill.wait();
                return Some(ExternalProcessError::worker(format!(
                    "failed to poll taskkill for {label}: {error}"
                )));
            }
        }
    }
}

#[cfg(unix)]
fn terminate_and_reap(
    child: &mut std::process::Child,
    process_group: i32,
    cancelled: bool,
    label: &str,
) -> Result<(), ExternalProcessError> {
    terminate_and_reap_with_grace(
        child,
        process_group,
        cancelled,
        label,
        PROCESS_TERMINATION_GRACE,
    )
}

#[cfg(unix)]
fn terminate_and_reap_with_grace(
    child: &mut std::process::Child,
    process_group: i32,
    cancelled: bool,
    label: &str,
    termination_grace: Duration,
) -> Result<(), ExternalProcessError> {
    use nix::sys::signal::Signal;
    use std::time::Instant;

    if child
        .try_wait()
        .map_err(|error| ExternalProcessError::worker(format!("failed to poll {label}: {error}")))?
        .is_some()
    {
        cleanup_descendants(process_group);
        return Ok(());
    }
    let initial_signal = if cancelled {
        Signal::SIGINT
    } else {
        Signal::SIGTERM
    };
    let initial_error = signal_process_group(process_group, initial_signal, label).err();
    if initial_error.is_some() {
        let _ = child.kill();
    }
    let grace_deadline = Instant::now() + termination_grace;
    while Instant::now() < grace_deadline {
        if child
            .try_wait()
            .map_err(|error| {
                ExternalProcessError::worker(format!("failed to reap {label}: {error}"))
            })?
            .is_some()
        {
            cleanup_descendants(process_group);
            return initial_error.map_or(Ok(()), Err);
        }
        std::thread::sleep(PROCESS_POLL_INTERVAL);
    }
    let kill_error = signal_process_group(process_group, Signal::SIGKILL, label).err();
    let _ = child.kill();
    let reap_deadline = Instant::now() + PROCESS_REAP_TIMEOUT;
    while Instant::now() < reap_deadline {
        if child
            .try_wait()
            .map_err(|error| {
                ExternalProcessError::worker(format!("failed to reap {label}: {error}"))
            })?
            .is_some()
        {
            return initial_error.or(kill_error).map_or(Ok(()), Err);
        }
        std::thread::sleep(PROCESS_POLL_INTERVAL);
    }
    Err(ExternalProcessError::worker(format!(
        "{label} could not be reaped after termination"
    )))
}

#[cfg(unix)]
fn signal_process_group(
    process_group: i32,
    signal: nix::sys::signal::Signal,
    label: &str,
) -> Result<(), ExternalProcessError> {
    use nix::errno::Errno;
    use nix::sys::signal::killpg;
    use nix::unistd::Pid;

    match killpg(Pid::from_raw(process_group), signal) {
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        Err(error) => Err(ExternalProcessError::worker(format!(
            "failed to signal {label} process group: {error}"
        ))),
    }
}

#[cfg(unix)]
fn cleanup_descendants(process_group: i32) {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;

    let process_group = Pid::from_raw(process_group);
    if killpg(process_group, Signal::SIGTERM).is_ok() {
        std::thread::sleep(Duration::from_millis(20));
        let _ = killpg(process_group, Signal::SIGKILL);
    }
}

#[cfg(unix)]
fn abort_process(child: &mut std::process::Child, process_group: i32, label: &str) {
    let _ = terminate_and_reap(child, process_group, false, label);
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static PROCESS_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn process_test_guard() -> MutexGuard<'static, ()> {
        PROCESS_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn bounded_capture_collects_stdout_and_stderr_concurrently() {
        let _guard = process_test_guard();
        let mut command = Command::new("sh");
        command.args(["-c", "printf stdout-value; printf stderr-value >&2"]);

        let output = run_interruptible_capture(&mut command, "capture fixture", 64, 64)
            .expect("capture fixture output");

        assert!(output.status.success());
        assert_eq!(output.stdout, b"stdout-value");
        assert_eq!(output.stderr, b"stderr-value");
    }

    #[test]
    fn supervised_process_does_not_consume_configured_stdin() {
        let _guard = process_test_guard();
        let input = tempfile::tempfile().expect("temporary process input");
        let mut command = Command::new("sh");
        command
            .args(["-c", "if IFS= read -r _line; then exit 42; else exit 0; fi"])
            .stdin(Stdio::from(input));

        let status = run_interruptible(&mut command, "non-interactive stdin fixture")
            .expect("supervised process status");

        assert!(status.success());
    }

    #[test]
    fn bounded_capture_stops_a_process_that_exceeds_a_stream_limit() {
        let _guard = process_test_guard();
        for (script, expected) in [
            (
                "while :; do printf 0123456789abcdef0123456789abcdef; done",
                "stdout exceeds 128 bytes",
            ),
            (
                "while :; do printf 0123456789abcdef0123456789abcdef >&2; done",
                "stderr exceeds 64 bytes",
            ),
        ] {
            let mut command = Command::new("sh");
            command.args(["-c", script]);
            let error = run_interruptible_capture(&mut command, "overflow fixture", 128, 64)
                .expect_err("oversized output must stop the process");

            assert_eq!(error.kind(), ExternalProcessErrorKind::Worker);
            assert!(
                error.to_string().contains(expected),
                "unexpected capture error: {error}"
            );
        }
    }

    #[test]
    fn bounded_capture_timeout_reaps_term_ignoring_descendants() {
        let _guard = process_test_guard();
        use nix::errno::Errno;
        use nix::sys::signal::kill;
        use nix::unistd::Pid;

        let directory = tempfile::tempdir().expect("temporary timeout process state");
        let descendant_pid_path = directory.path().join("descendant.pid");
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(
                "sh -c 'trap \"\" TERM; while :; do sleep 1; done' & \
                 printf '%s' \"$!\" > \"$EXTERNAL_PROCESS_DESCENDANT_PID\"; \
                 while :; do sleep 1; done",
            )
            .env("EXTERNAL_PROCESS_DESCENDANT_PID", &descendant_pid_path);

        let started = Instant::now();
        let error = run_interruptible_capture_with_timeout(
            &mut command,
            "timeout fixture",
            64,
            64,
            Duration::from_millis(250),
        )
        .expect_err("deadline must stop the process group");

        assert_eq!(error.kind(), ExternalProcessErrorKind::Worker);
        assert!(
            error
                .to_string()
                .contains("exceeded its execution deadline")
        );
        assert!(started.elapsed() < Duration::from_secs(3));
        let descendant_pid = std::fs::read_to_string(&descendant_pid_path)
            .expect("read timeout descendant pid")
            .parse::<i32>()
            .expect("parse timeout descendant pid");
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match kill(Pid::from_raw(descendant_pid), None) {
                Err(Errno::ESRCH) => break,
                Ok(()) if Instant::now() < deadline => {
                    std::thread::sleep(PROCESS_POLL_INTERVAL);
                }
                result => {
                    panic!("timeout descendant {descendant_pid} survived cleanup: {result:?}")
                }
            }
        }
    }

    #[test]
    fn cancellation_is_installed_before_the_external_process_spawns() {
        let _guard = process_test_guard();
        use nix::sys::signal::{Signal, raise};

        let directory = tempfile::tempdir().expect("temporary process state");
        let sentinel = directory.path().join("spawned");
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("printf spawned > \"$EXTERNAL_PROCESS_SENTINEL\"")
            .env("EXTERNAL_PROCESS_SENTINEL", &sentinel);

        let error = run_interruptible_after_registration(&mut command, "fixture process", || {
            raise(Signal::SIGINT).expect("raise cancellation signal before spawn");
        })
        .expect_err("pre-spawn cancellation must stop the command");

        assert_eq!(error.kind(), ExternalProcessErrorKind::Cancelled);
        assert!(error.to_string().contains("fixture process was cancelled"));
        assert!(!sentinel.exists());

        let mut after_cancellation = Command::new("sh");
        after_cancellation.args(["-c", ":"]);
        let status =
            run_interruptible(&mut after_cancellation, "post-cancellation fixture process")
                .expect("consumed cancellation must not affect later processes");
        assert!(status.success());
    }

    #[test]
    fn deferral_keeps_cancellation_active_between_external_processes() {
        let _guard = process_test_guard();
        use nix::sys::signal::{Signal, raise};

        let cancellation = InterruptDeferral::start("multi-phase fixture")
            .expect("install multi-phase cancellation");
        let mut completed = Command::new("sh");
        completed.args(["-c", ":"]);
        let status =
            run_interruptible_in_deferral(&mut completed, "completed phase", &cancellation)
                .expect("first phase must complete");
        assert!(status.success());

        raise(Signal::SIGINT).expect("cancel between external processes");
        let directory = tempfile::tempdir().expect("temporary process state");
        let sentinel = directory.path().join("spawned");
        let mut blocked = Command::new("sh");
        blocked
            .arg("-c")
            .arg("printf spawned > \"$EXTERNAL_PROCESS_SENTINEL\"")
            .env("EXTERNAL_PROCESS_SENTINEL", &sentinel);
        let error = run_interruptible_in_deferral(&mut blocked, "blocked phase", &cancellation)
            .expect_err("cancellation between phases must prevent the next spawn");

        assert_eq!(error.kind(), ExternalProcessErrorKind::Cancelled);
        assert!(error.to_string().contains("blocked phase was cancelled"));
        assert!(!sentinel.exists());
        assert!(cancellation.finish());
    }

    #[test]
    fn inactive_supervisor_restores_default_sigint_behavior() {
        let _guard = process_test_guard();
        use nix::sys::signal::Signal;
        use std::os::unix::process::ExitStatusExt;

        const CHILD_ENV: &str = "VESPER_EXTERNAL_PROCESS_INACTIVE_SIGINT_FIXTURE";
        if std::env::var_os(CHILD_ENV).is_some() {
            let mut completed = Command::new("sh");
            completed.args(["-c", ":"]);
            let status = run_interruptible(&mut completed, "completed fixture process")
                .expect("supervised fixture process must complete");
            assert!(status.success());
            nix::sys::signal::raise(Signal::SIGINT).expect("raise inactive SIGINT");
            panic!("inactive SIGINT must terminate the fixture process");
        }

        let status = Command::new(std::env::current_exe().expect("locate test executable"))
            .args([
                "--exact",
                "external_process::tests::inactive_supervisor_restores_default_sigint_behavior",
                "--nocapture",
            ])
            .env(CHILD_ENV, "1")
            .status()
            .expect("run inactive SIGINT fixture process");
        assert_eq!(status.signal(), Some(Signal::SIGINT as i32));
    }
}
