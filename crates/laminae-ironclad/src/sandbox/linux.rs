//! Linux sandbox provider using kernel namespaces, `prctl`, and `rlimit`.
//!
//! Works **without root** on modern kernels (>= 3.8) that allow unprivileged
//! user namespaces. Falls back gracefully when individual features are
//! unavailable.

use anyhow::Result;
use tokio::process::Command;

use super::{apply_common, NetworkPolicy, SandboxProfile, SandboxProvider};

/// Sandbox provider for Linux.
///
/// Applies the following isolation layers via `pre_exec` hooks (before `execve`):
///
/// 1. **`PR_SET_NO_NEW_PRIVS`** — prevents the child from gaining privileges
///    through setuid/setgid binaries or capabilities.
/// 2. **Network namespace (`unshare(CLONE_NEWNET)`)** — creates an isolated
///    network stack when the policy is [`NetworkPolicy::None`]. Requires
///    unprivileged user namespaces; skipped if unavailable.
/// 3. **Resource limits (`setrlimit`)** — caps file size, CPU time, address
///    space, and number of open files.
/// 4. **Environment scrubbing** — removes secret-bearing environment variables.
pub struct LinuxSandboxProvider;

impl SandboxProvider for LinuxSandboxProvider {
    fn sandboxed_command(
        &self,
        binary: &str,
        args: &[&str],
        profile: &SandboxProfile,
    ) -> Result<Command> {
        let network_policy = profile.network_policy.clone();

        let mut cmd = Command::new(binary);
        for arg in args {
            cmd.arg(arg);
        }

        // Safety: The closures passed to `pre_exec` run between fork and exec
        // in the child process. We only call async-signal-safe functions (libc
        // wrappers) so this is safe.
        unsafe {
            cmd.pre_exec(move || {
                apply_prctl()?;
                apply_network_isolation(&network_policy);
                apply_rlimits();
                Ok(())
            });
        }

        apply_common(&mut cmd, profile);
        Ok(cmd)
    }

    fn is_available(&self) -> bool {
        // PR_SET_NO_NEW_PRIVS is available on Linux >= 3.5 — effectively all
        // modern systems.
        true
    }

    fn name(&self) -> &'static str {
        "linux-ns"
    }
}

// ── Pre-exec helpers (run in the child, between fork and exec) ──────────

/// Prevent the child from ever gaining new privileges.
fn apply_prctl() -> std::io::Result<()> {
    // PR_SET_NO_NEW_PRIVS = 38
    let ret = unsafe { libc::prctl(38, 1, 0, 0, 0) };
    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// If the network policy is [`NetworkPolicy::None`], create a new network
/// namespace so the child has no network interfaces at all.
///
/// For [`NetworkPolicy::LocalhostOnly`] or [`NetworkPolicy::Restricted`] we
/// rely on the combination of `PR_SET_NO_NEW_PRIVS` and rlimits — full
/// network filtering would require eBPF/seccomp-bpf which is out of scope
/// for the initial implementation.
fn apply_network_isolation(policy: &NetworkPolicy) {
    if *policy == NetworkPolicy::None {
        // CLONE_NEWUSER | CLONE_NEWNET — creating a net namespace requires a
        // user namespace on unprivileged processes.
        let flags = libc::CLONE_NEWUSER | libc::CLONE_NEWNET;
        let ret = unsafe { libc::unshare(flags) };
        if ret != 0 {
            // Unprivileged user namespaces may be disabled
            // (sysctl kernel.unprivileged_userns_clone=0). Fall back silently.
            tracing::warn!(
                "[IRONCLAD] unshare(CLONE_NEWUSER|CLONE_NEWNET) failed — \
                 network namespace isolation unavailable on this system"
            );
        }
    }
}

/// Apply conservative resource limits.
fn apply_rlimits() {
    // Max file size: 256 MB
    set_rlimit(libc::RLIMIT_FSIZE, 256 * 1024 * 1024);
    // CPU time: 600 seconds (10 minutes)
    set_rlimit(libc::RLIMIT_CPU, 600);
    // Address space: 4 GB
    set_rlimit(libc::RLIMIT_AS, 4 * 1024 * 1024 * 1024);
    // Open file descriptors: 256
    set_rlimit(libc::RLIMIT_NOFILE, 256);
    // Max processes (prevent fork bombs): 64
    set_rlimit(libc::RLIMIT_NPROC, 64);
}

fn set_rlimit(resource: libc::__rlimit_resource_t, limit: u64) {
    let rlim = libc::rlimit {
        rlim_cur: limit,
        rlim_max: limit,
    };
    unsafe {
        libc::setrlimit(resource, &rlim);
    }
}
