//! Windows sandbox provider using Job Objects for resource constraints.
//!
//! On Windows, there is no direct equivalent to macOS Seatbelt or Linux
//! namespaces for filesystem/network isolation.  This provider uses Windows
//! Job Objects to enforce:
//!
//! - **Memory limits** (via `JOB_OBJECT_LIMIT_PROCESS_MEMORY`)
//! - **Process count limits** (via `JOB_OBJECT_LIMIT_ACTIVE_PROCESS`)
//! - **Environment variable scrubbing**
//!
//! ## Limitations
//!
//! - **No filesystem isolation**: Job Objects cannot restrict filesystem
//!   access.  A Windows Sandbox (Pro+ only) or AppContainer would be
//!   needed for that.
//! - **No network isolation**: Job Objects do not support network
//!   filtering.  Windows Filtering Platform (WFP) or AppContainers
//!   would be required.
//! - **Job Object assignment**: The Job Object must be assigned *after*
//!   the process is spawned.  Callers should invoke
//!   `assign_job_object_to_pid` with the child's PID immediately after
//!   `cmd.spawn()`.

use anyhow::Result;
use tokio::process::Command;

use super::{apply_common, SandboxProfile, SandboxProvider};

/// Default memory limit per process: 4 GB.
const DEFAULT_MEMORY_LIMIT_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Default maximum active child processes inside the job.
const DEFAULT_MAX_ACTIVE_PROCESSES: u32 = 64;

/// Windows sandbox provider using Job Objects.
///
/// Applies resource limits and environment scrubbing. Filesystem and network
/// restrictions are **not enforced** -- see the module-level docs for details.
pub struct WindowsSandboxProvider;

impl SandboxProvider for WindowsSandboxProvider {
    fn name(&self) -> &'static str {
        "windows-job"
    }

    fn is_available(&self) -> bool {
        // Job Objects are available on all supported Windows versions (Vista+).
        true
    }

    fn sandboxed_command(
        &self,
        binary: &str,
        args: &[&str],
        profile: &SandboxProfile,
    ) -> Result<Command> {
        let mut cmd = Command::new(binary);
        cmd.args(args);
        apply_common(&mut cmd, profile);

        // Restrict working directory to the project root.
        cmd.current_dir(&profile.project_dir);

        tracing::info!(
            "[IRONCLAD] Windows sandbox: process will run with env scrubbing, \
             workdir restriction, and Job Object resource limits (memory: {} MB, \
             max processes: {}).  Note: filesystem and network isolation are NOT \
             enforced on Windows -- see laminae-ironclad docs.",
            DEFAULT_MEMORY_LIMIT_BYTES / (1024 * 1024),
            DEFAULT_MAX_ACTIVE_PROCESSES,
        );

        Ok(cmd)
    }
}

/// Assign a Windows Job Object with resource limits to an existing process.
///
/// Call this immediately after spawning the child process.  The Job Object
/// enforces a per-process memory cap and a maximum number of active child
/// processes.
///
/// # Safety / Platform
///
/// This function uses the Windows API (`CreateJobObjectW`,
/// `SetInformationJobObject`, `AssignProcessToJobObject`).  It is a no-op
/// on non-Windows targets.
#[cfg(target_os = "windows")]
pub fn assign_job_object_to_pid(pid: u32) -> Result<()> {
    use std::ptr;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_BASIC_LIMIT_INFORMATION,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
        JOB_OBJECT_LIMIT_PROCESS_MEMORY,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_ALL_ACCESS};

    unsafe {
        let job: HANDLE = CreateJobObjectW(ptr::null(), ptr::null());
        if job.is_null() || job == INVALID_HANDLE_VALUE {
            anyhow::bail!("Failed to create Job Object");
        }

        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_PROCESS_MEMORY | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
        info.ProcessMemoryLimit = DEFAULT_MEMORY_LIMIT_BYTES as usize;
        info.BasicLimitInformation.ActiveProcessLimit = DEFAULT_MAX_ACTIVE_PROCESSES;

        let ok = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if ok == 0 {
            CloseHandle(job);
            anyhow::bail!("Failed to set Job Object limits");
        }

        let process: HANDLE = OpenProcess(PROCESS_ALL_ACCESS, 0, pid);
        if process.is_null() || process == INVALID_HANDLE_VALUE {
            CloseHandle(job);
            anyhow::bail!("Failed to open process {pid} for Job Object assignment");
        }

        let ok = AssignProcessToJobObject(job, process);
        CloseHandle(process);
        if ok == 0 {
            CloseHandle(job);
            anyhow::bail!("Failed to assign process {pid} to Job Object");
        }

        // Intentionally leak the Job Object handle so it stays alive for the
        // duration of the child process.  When the child (and all its
        // descendants inside the job) exit, Windows cleans up the object.

        tracing::debug!(
            "[IRONCLAD] Assigned Job Object to PID {pid}: \
             memory_limit={}MB, max_processes={}",
            DEFAULT_MEMORY_LIMIT_BYTES / (1024 * 1024),
            DEFAULT_MAX_ACTIVE_PROCESSES,
        );

        Ok(())
    }
}
