//! Daemon process management
//!
//! Handles daemonization, PID file management, and process control.

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use eyre::{Context, Result};
use tracing::{debug, info, warn};

/// Default PID file location
fn default_pid_path() -> PathBuf {
    dirs::runtime_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("taskdaemon")
        .join("taskdaemon.pid")
}

/// Daemon process manager
#[derive(Debug)]
pub struct DaemonManager {
    /// Path to the PID file
    pid_file: PathBuf,
}

impl Default for DaemonManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DaemonManager {
    /// Create a new daemon manager with the default PID file location
    pub fn new() -> Self {
        Self {
            pid_file: default_pid_path(),
        }
    }

    /// Create a daemon manager with a custom PID file path
    pub fn with_pid_file(pid_file: PathBuf) -> Self {
        Self { pid_file }
    }

    /// Check if a daemon is running
    pub fn is_running(&self) -> bool {
        self.read_pid().is_some_and(is_process_running)
    }

    /// Get the running daemon's PID
    pub fn running_pid(&self) -> Option<u32> {
        self.read_pid().filter(|&pid| is_process_running(pid))
    }

    /// Read the PID from the PID file
    fn read_pid(&self) -> Option<u32> {
        if !self.pid_file.exists() {
            return None;
        }

        let mut file = fs::File::open(&self.pid_file).ok()?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).ok()?;

        contents.trim().parse().ok()
    }

    /// Write the PID to the PID file
    fn write_pid(&self, pid: u32) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = self.pid_file.parent() {
            fs::create_dir_all(parent).context("Failed to create PID file directory")?;
        }

        let mut file = fs::File::create(&self.pid_file).context("Failed to create PID file")?;
        write!(file, "{}", pid).context("Failed to write PID")?;

        debug!(pid, path = ?self.pid_file, "Wrote PID file");
        Ok(())
    }

    /// Remove the PID file
    fn remove_pid_file(&self) -> Result<()> {
        if self.pid_file.exists() {
            fs::remove_file(&self.pid_file).context("Failed to remove PID file")?;
            debug!(path = ?self.pid_file, "Removed PID file");
        }
        Ok(())
    }

    /// Start the daemon
    ///
    /// This forks a new process and returns immediately.
    pub fn start(&self) -> Result<u32> {
        if let Some(pid) = self.running_pid() {
            return Err(eyre::eyre!("Daemon already running with PID {}", pid));
        }

        info!("Starting daemon...");

        // Get the current executable path
        let exe = std::env::current_exe().context("Failed to get current executable")?;

        // Spawn the daemon process
        let child = Command::new(&exe)
            .arg("run-daemon")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to spawn daemon process")?;

        let pid = child.id();
        self.write_pid(pid)?;

        info!(pid, "Daemon started");
        Ok(pid)
    }

    /// Stop the daemon
    pub fn stop(&self) -> Result<()> {
        let pid = self.running_pid().ok_or_else(|| eyre::eyre!("Daemon is not running"))?;

        info!(pid, "Stopping daemon...");

        // Send SIGTERM on Unix
        #[cfg(unix)]
        {
            use nix::sys::signal::{Signal, kill};
            use nix::unistd::Pid;

            kill(Pid::from_raw(pid as i32), Signal::SIGTERM).context("Failed to send SIGTERM")?;
        }

        // On Windows, use taskkill
        #[cfg(windows)]
        {
            Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F"])
                .output()
                .context("Failed to kill process")?;
        }

        // Wait for process to exit (with timeout)
        let mut attempts = 0;
        while is_process_running(pid) && attempts < 50 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            attempts += 1;
        }

        if is_process_running(pid) {
            warn!(pid, "Daemon did not stop gracefully, sending SIGKILL");
            #[cfg(unix)]
            {
                use nix::sys::signal::{Signal, kill};
                use nix::unistd::Pid;
                let _ = kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
            }
        }

        self.remove_pid_file()?;
        info!(pid, "Daemon stopped");
        Ok(())
    }

    /// Register the current process as the daemon
    ///
    /// This should be called by the daemon process after forking.
    pub fn register_self(&self) -> Result<()> {
        let pid = std::process::id();
        self.write_pid(pid)?;
        info!(pid, "Daemon registered");
        Ok(())
    }

    /// Get the PID file path
    pub fn pid_file(&self) -> &PathBuf {
        &self.pid_file
    }
}

/// Check if a process with the given PID is running
fn is_process_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        use nix::sys::signal::kill;
        use nix::unistd::Pid;
        // Sending signal 0 checks if the process exists without affecting it
        kill(Pid::from_raw(pid as i32), None).is_ok()
    }

    #[cfg(windows)]
    {
        use std::process::Command;
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .output()
            .map(|o| !o.stdout.is_empty() && !String::from_utf8_lossy(&o.stdout).contains("No tasks"))
            .unwrap_or(false)
    }

    #[cfg(not(any(unix, windows)))]
    {
        // Assume running on unknown platforms
        let _ = pid;
        true
    }
}

/// Daemon status information
#[derive(Debug)]
pub struct DaemonStatus {
    /// Whether the daemon is running
    pub running: bool,
    /// Process ID (if running)
    pub pid: Option<u32>,
    /// PID file path
    pub pid_file: PathBuf,
}

impl DaemonManager {
    /// Get the daemon status
    pub fn status(&self) -> DaemonStatus {
        let pid = self.running_pid();
        DaemonStatus {
            running: pid.is_some(),
            pid,
            pid_file: self.pid_file.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_daemon_manager_new() {
        let manager = DaemonManager::new();
        // Just verify it doesn't panic - pid_file may or may not exist
        let _ = manager.pid_file();
    }

    #[test]
    fn test_daemon_manager_with_custom_pid() {
        let temp_dir = TempDir::new().unwrap();
        let pid_file = temp_dir.path().join("test.pid");

        let manager = DaemonManager::with_pid_file(pid_file.clone());
        assert_eq!(manager.pid_file(), &pid_file);
    }

    #[test]
    fn test_is_not_running_when_no_pid_file() {
        let temp_dir = TempDir::new().unwrap();
        let pid_file = temp_dir.path().join("nonexistent.pid");

        let manager = DaemonManager::with_pid_file(pid_file);
        assert!(!manager.is_running());
    }

    #[test]
    fn test_write_and_read_pid() {
        let temp_dir = TempDir::new().unwrap();
        let pid_file = temp_dir.path().join("test.pid");

        let manager = DaemonManager::with_pid_file(pid_file);

        manager.write_pid(12345).unwrap();
        assert_eq!(manager.read_pid(), Some(12345));

        manager.remove_pid_file().unwrap();
        assert_eq!(manager.read_pid(), None);
    }

    #[test]
    fn test_status() {
        let temp_dir = TempDir::new().unwrap();
        let pid_file = temp_dir.path().join("test.pid");

        let manager = DaemonManager::with_pid_file(pid_file.clone());
        let status = manager.status();

        assert!(!status.running);
        assert!(status.pid.is_none());
        assert_eq!(status.pid_file, pid_file);
    }
}
