use std::io;
use std::os::unix::process::CommandExt;
use std::process::Command;

use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};

/// Places a child in a new process group and makes terminal diagnostics safe.
///
/// Supervised CLI children run in the background relative to the caller's
/// terminal process group. Ignoring `SIGTTOU` before `exec` lets tools that
/// inspect or write inherited diagnostic streams finish instead of being
/// stopped by terminal job control. stdin remains the caller's responsibility;
/// non-interactive callers should set it to `Stdio::null()`.
pub fn configure_background_process_group(command: &mut Command) {
    command.process_group(0);

    // SAFETY: `pre_exec` runs after fork and before exec. The closure captures
    // no Rust state and only invokes the async-signal-safe `sigaction` syscall
    // with stack-owned values, so it does not allocate or touch shared locks.
    unsafe {
        command.pre_exec(|| {
            let action = SigAction::new(SigHandler::SigIgn, SaFlags::empty(), SigSet::empty());
            // `SIGTTOU` is intentionally ignored only in the child. The
            // parent's terminal signal disposition is never changed.
            sigaction(Signal::SIGTTOU, &action)
                .map(|_| ())
                .map_err(|error| io::Error::from_raw_os_error(error as i32))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::configure_background_process_group;
    use std::ffi::CString;
    use std::fs::File;
    use std::io::Read;
    use std::os::unix::ffi::OsStrExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use nix::libc;
    use nix::pty::{ForkptyResult, forkpty};
    use nix::sys::signal::{Signal, killpg};
    use nix::sys::termios::{LocalFlags, SetArg, tcgetattr, tcsetattr};
    use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};

    const PTY_CONTROLLER_ENV: &str = "VESPER_PROCESS_TEST_PTY_CONTROLLER";

    #[cfg(unix)]
    #[test]
    fn child_ignores_sigttou_before_exec() {
        let mut command = Command::new("sh");
        command.args(["-c", "kill -s TTOU $$"]);
        configure_background_process_group(&mut command);

        let mut child = command.spawn().expect("run SIGTTOU fixture");
        let status = wait_with_deadline(&mut child, Duration::from_secs(2))
            .unwrap_or_else(|| terminate_test_group(&mut child));

        assert!(status.success(), "child was stopped by SIGTTOU: {status}");
    }

    #[test]
    fn background_process_group_can_write_to_a_tostop_controlling_pty() {
        if std::env::var_os(PTY_CONTROLLER_ENV).is_some() {
            run_pty_controller();
            return;
        }

        let env_program = CString::new("/usr/bin/env").expect("static env path has no NUL");
        let controller_env = CString::new(format!("{PTY_CONTROLLER_ENV}=1"))
            .expect("controller environment has no NUL");
        let executable_path = std::env::current_exe().expect("locate test executable");
        let executable = CString::new(executable_path.as_os_str().as_bytes())
            .expect("test executable path has no NUL");
        let exact = CString::new("--exact").expect("static argument has no NUL");
        let test_name = CString::new(
            "unix_process::tests::background_process_group_can_write_to_a_tostop_controlling_pty",
        )
        .expect("static test name has no NUL");
        let nocapture = CString::new("--nocapture").expect("static argument has no NUL");
        let argv = [
            env_program.as_ptr(),
            controller_env.as_ptr(),
            executable.as_ptr(),
            exact.as_ptr(),
            test_name.as_ptr(),
            nocapture.as_ptr(),
            std::ptr::null(),
        ];

        // SAFETY: forkpty is called only to create an isolated controller. The
        // child immediately invokes async-signal-safe execv with pointers built
        // before fork, and calls _exit if exec fails. It does not allocate or
        // access Rust synchronization state after fork.
        let fork = unsafe { forkpty(None, None) }.expect("fork PTY controller");
        let (controller, master) = match fork {
            ForkptyResult::Child => {
                // SAFETY: every pointer in argv refers to a live, NUL-terminated
                // CString created before fork, and argv itself has a trailing null.
                // execv and _exit are async-signal-safe; _exit runs only when execv
                // fails and terminates the child without unwinding Rust state.
                unsafe {
                    libc::execv(env_program.as_ptr(), argv.as_ptr());
                    libc::_exit(127);
                }
            }
            ForkptyResult::Parent { child, master } => (child, master),
        };
        let output_reader = std::thread::spawn(move || {
            let master = File::from(master);
            let mut output = Vec::new();
            let result = master.take(64 * 1024).read_to_end(&mut output);
            if let Err(error) = result
                && error.raw_os_error() != Some(libc::EIO)
            {
                panic!("read PTY controller output: {error}");
            }
            output
        });
        let status = wait_for_pty_controller(controller, Duration::from_secs(5));
        let controller_output = output_reader.join().expect("join PTY output reader");
        assert!(
            matches!(status, WaitStatus::Exited(_, 0)),
            "PTY controller failed with {status:?}: {}",
            String::from_utf8_lossy(&controller_output)
        );
    }

    fn run_pty_controller() {
        let stdin = std::io::stdin();
        let mut termios = tcgetattr(&stdin).expect("read PTY terminal settings");
        termios.local_flags.insert(LocalFlags::TOSTOP);
        tcsetattr(&stdin, SetArg::TCSANOW, &termios).expect("enable TOSTOP on controlling PTY");

        let mut command = Command::new("sh");
        command
            .args(["-c", "printf 'background diagnostic\\n' >&2"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        configure_background_process_group(&mut command);

        let mut child = command.spawn().expect("start background PTY writer");
        let status = wait_with_deadline(&mut child, Duration::from_secs(2))
            .unwrap_or_else(|| terminate_test_group(&mut child));
        assert!(
            status.success(),
            "background PTY writer did not exit successfully: {status}"
        );
    }

    fn wait_for_pty_controller(controller: nix::unistd::Pid, timeout: Duration) -> WaitStatus {
        let deadline = Instant::now() + timeout;
        loop {
            match waitpid(
                controller,
                Some(WaitPidFlag::WNOHANG | WaitPidFlag::WUNTRACED),
            )
            .expect("poll PTY controller")
            {
                WaitStatus::StillAlive if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                WaitStatus::StillAlive => return terminate_pty_controller(controller),
                status @ (WaitStatus::Exited(_, _)
                | WaitStatus::Signaled(_, _, _)
                | WaitStatus::Stopped(_, _)) => return status,
                _ if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                _ => return terminate_pty_controller(controller),
            }
        }
    }

    fn terminate_pty_controller(controller: nix::unistd::Pid) -> WaitStatus {
        let _ = killpg(controller, Signal::SIGCONT);
        let _ = killpg(controller, Signal::SIGKILL);
        waitpid(controller, None).expect("reap PTY controller")
    }

    fn wait_with_deadline(
        child: &mut std::process::Child,
        timeout: Duration,
    ) -> Option<std::process::ExitStatus> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            match child.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(error) => panic!("failed to poll process test child: {error}"),
            }
        }
        None
    }

    fn terminate_test_group(child: &mut std::process::Child) -> std::process::ExitStatus {
        use nix::sys::signal::{Signal, killpg};
        use nix::unistd::Pid;

        let process_group = i32::try_from(child.id()).expect("test child pid fits process group");
        let _ = killpg(Pid::from_raw(process_group), Signal::SIGCONT);
        let _ = killpg(Pid::from_raw(process_group), Signal::SIGKILL);
        let _ = child.kill();
        child.wait().expect("reap timed-out process test child")
    }
}
