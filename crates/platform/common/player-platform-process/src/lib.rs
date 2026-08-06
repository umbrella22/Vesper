//! Process-containment primitives owned by platform wrapper code.

#![warn(clippy::undocumented_unsafe_blocks)]

#[cfg(unix)]
mod unix_process;

#[cfg(windows)]
mod windows_job;

#[cfg(unix)]
pub use unix_process::configure_background_process_group;

#[cfg(windows)]
pub use windows_job::WindowsJob;
