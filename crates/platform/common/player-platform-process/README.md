# Vesper Player Process Supervision Primitives

`vesper-player-platform-process` contains small cross-platform process
containment helpers used by Vesper tooling and platform wrapper code. Use it
when a wrapper launches a worker process and must preserve terminal behavior or
reliably terminate the worker tree.

## Platform APIs

On Unix, `configure_background_process_group(&mut Command)` puts a child in a
new process group and ignores `SIGTTOU` in that child before `exec`. This lets a
supervised background child write inherited diagnostic streams without terminal
job control stopping it. The parent process signal disposition is unchanged.

On Windows, `WindowsJob::new_kill_on_close()` creates a Job Object with
`KILL_ON_JOB_CLOSE`. Assign a spawned child with `assign_child`, then call
`terminate` for an explicit shutdown or drop the job object to terminate all
assigned processes.

```rust
#[cfg(unix)]
{
    use std::process::Command;
    use player_platform_process::configure_background_process_group;

    let mut command = Command::new("worker");
    configure_background_process_group(&mut command);
    let child = command.spawn()?;
}
```

## Scope

This crate does not spawn workers, read process output, define retry policy, or
provide an asynchronous process supervisor. Callers remain responsible for
standard input, output draining, timeouts, cancellation, reaping children, and
their application-specific failure mapping.

The API follows the platform-specific process model, so dependent code should
use conditional compilation for Unix and Windows behavior.
