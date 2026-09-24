//! Spawning agent processes so that killing one takes its entire tree with it.
//!
//! Windows: each child is assigned to a Job Object with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so the tree also dies if the daemon
//! crashes. Unix: each child leads its own process group and we `killpg` it.

use std::io;
use tokio::process::{Child, Command};

/// A kill-the-whole-tree handle, implemented per platform.
pub trait ProcessGroup: Send + Sync {
    fn kill_tree(&self) -> io::Result<()>;
}

pub struct SupervisedChild {
    pub child: Child,
    group: Box<dyn ProcessGroup>,
}

impl Drop for SupervisedChild {
    fn drop(&mut self) {
        // Also covers cancellation/timeouts before an explicit async shutdown.
        let _ = self.group.kill_tree();
    }
}

impl SupervisedChild {
    /// Spawn `cmd` in a fresh process group / job.
    pub fn spawn(mut cmd: Command) -> io::Result<Self> {
        cmd.kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);
        let child = cmd.spawn()?;
        let group = platform::group_for(&child)?;
        Ok(Self { child, group })
    }

    pub fn id(&self) -> Option<u32> {
        self.child.id()
    }

    /// Kill the process and every descendant, then reap it.
    pub async fn kill_tree(&mut self) -> io::Result<()> {
        self.group.kill_tree()?;
        let _ = self.child.wait().await;
        Ok(())
    }
}

#[cfg(windows)]
mod platform {
    use super::ProcessGroup;
    use std::io;
    use tokio::process::Child;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
        TerminateJobObject,
    };

    struct Job(HANDLE);

    // SAFETY: a job handle is a kernel object handle usable from any thread.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Drop for Job {
        fn drop(&mut self) {
            // Closing the last handle kills the tree (KILL_ON_JOB_CLOSE).
            unsafe { CloseHandle(self.0) };
        }
    }

    impl ProcessGroup for Job {
        fn kill_tree(&self) -> io::Result<()> {
            if unsafe { TerminateJobObject(self.0, 1) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    pub fn group_for(child: &Child) -> io::Result<Box<dyn ProcessGroup>> {
        let process = child.raw_handle().ok_or_else(|| io::Error::other("child already exited"))? as HANDLE;
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = Job(job);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            if AssignProcessToJobObject(job.0, process) == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Box::new(job))
        }
    }
}

#[cfg(unix)]
mod platform {
    use super::ProcessGroup;
    use std::io;
    use tokio::process::Child;

    struct Pgid(libc::pid_t);

    impl ProcessGroup for Pgid {
        fn kill_tree(&self) -> io::Result<()> {
            if unsafe { libc::killpg(self.0, libc::SIGKILL) } != 0 {
                let err = io::Error::last_os_error();
                if err.raw_os_error() != Some(libc::ESRCH) {
                    return Err(err);
                }
            }
            Ok(())
        }
    }

    pub fn group_for(child: &Child) -> io::Result<Box<dyn ProcessGroup>> {
        let pid = child.id().ok_or_else(|| io::Error::other("child already exited"))?;
        Ok(Box::new(Pgid(pid as libc::pid_t)))
    }
}

/// Background helpers must not flash terminal windows on Windows.
pub fn hide_console(command: &mut tokio::process::Command) {
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    #[cfg(not(windows))]
    let _ = command;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn long_running() -> Command {
        if cfg!(windows) {
            let mut c = Command::new("powershell");
            c.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", "sleep 30 & sleep 30"]);
            c
        }
    }

    #[tokio::test]
    async fn kill_tree_terminates_quickly() {
        let mut child = SupervisedChild::spawn(long_running()).unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        tokio::time::timeout(Duration::from_secs(5), child.kill_tree())
            .await
            .expect("kill_tree should not hang")
            .unwrap();
        assert!(child.child.try_wait().unwrap().is_some());
    }
}
