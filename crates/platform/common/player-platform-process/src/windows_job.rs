use std::io;
use std::mem::size_of;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::Child;

use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};

/// Owns a Windows Job Object that terminates every assigned process on close.
#[derive(Debug)]
pub struct WindowsJob {
    handle: OwnedHandle,
}

impl WindowsJob {
    /// Creates an unnamed Job Object with `KILL_ON_JOB_CLOSE` enabled.
    pub fn new_kill_on_close() -> io::Result<Self> {
        // SAFETY: Null security attributes and name request an unnamed Job Object. The returned
        // handle is checked before ownership is transferred to `OwnedHandle`.
        let raw_handle = unsafe {
            windows_sys::Win32::System::JobObjects::CreateJobObjectW(
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if raw_handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `CreateJobObjectW` returned a new, non-null owned handle.
        let handle = unsafe { OwnedHandle::from_raw_handle(raw_handle) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let information_size = u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
            .map_err(|_| io::Error::other("Windows Job Object information size overflowed"))?;
        // SAFETY: `limits` points to a correctly initialized structure for the requested
        // information class and remains alive for the duration of the call.
        let configured = unsafe {
            SetInformationJobObject(
                handle.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                std::ptr::addr_of!(limits).cast(),
                information_size,
            )
        };
        if configured == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { handle })
    }

    /// Assigns a spawned child before the child is released to execute untrusted work.
    pub fn assign_child(&self, child: &Child) -> io::Result<()> {
        // SAFETY: Both handles are live for this call. The caller owns the child and this Job
        // Object, and Windows validates whether the process can be assigned.
        let assigned =
            unsafe { AssignProcessToJobObject(self.handle.as_raw_handle(), child.as_raw_handle()) };
        if assigned == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// Terminates all processes currently associated with this Job Object.
    pub fn terminate(&self, exit_code: u32) -> io::Result<()> {
        // SAFETY: The Job Object handle is owned by `self` and remains live for this call.
        let terminated = unsafe { TerminateJobObject(self.handle.as_raw_handle(), exit_code) };
        if terminated == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}
