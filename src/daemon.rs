//! Daemon process management
//!
//! Handles daemonization, PID file management, and process control.

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use eyre::{Context, Result};
use tracing::{debug, info, warn};

/// Current version from git describe (set at compile time)
pub const VERSION: &str = env!("GIT_DESCRIBE");

/// Default PID file location
fn default_pid_path() -> PathBuf {
    debug!("default_pid_path: called");
    let path = dirs::runtime_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("taskdaemon")
        .join("taskdaemon.pid");
    debug!(?path, "default_pid_path: returning path");
    path
}

/// Default version file location (alongside PID file)
fn default_version_path() -> PathBuf {
    debug!("default_version_path: called");
    let path = dirs::runtime_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("taskdaemon")
        .join("taskdaemon.version");
    debug!(?path, "default_version_path: returning path");
    path
}

/// Daemon process manager
#[derive(Debug)]
pub struct DaemonManager {
    /// Path to the PID file
    pid_file: PathBuf,
    /// Path to the version file
    version_file: PathBuf,
}

impl Default for DaemonManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DaemonManager {
    /// Create a new daemon manager with the default PID file location
    pub fn new() -> Self {
        debug!("DaemonManager::new: called");
        let mgr = Self {
            pid_file: default_pid_path(),
            version_file: default_version_path(),
        };
        debug!(?mgr.pid_file, ?mgr.version_file, "DaemonManager::new: created with default paths");
        mgr
    }

    /// Create a daemon manager with a custom PID file path
    pub fn with_pid_file(pid_file: PathBuf) -> Self {
        debug!(?pid_file, "DaemonManager::with_pid_file: called");
        let version_file = pid_file.with_extension("version");
        Self { pid_file, version_file }
    }

    /// Check if a daemon is running
    pub fn is_running(&self) -> bool {
        debug!("DaemonManager::is_running: called");
        let result = self.read_pid().is_some_and(is_process_running);
        debug!(result, "DaemonManager::is_running: returning");
        result
    }

    /// Get the running daemon's PID
    pub fn running_pid(&self) -> Option<u32> {
        debug!("DaemonManager::running_pid: called");
        let result = self.read_pid().filter(|&pid| is_process_running(pid));
        debug!(?result, "DaemonManager::running_pid: returning");
        result
    }

    /// Read the PID from the PID file
    fn read_pid(&self) -> Option<u32> {
        debug!(?self.pid_file, "DaemonManager::read_pid: called");
        if !self.pid_file.exists() {
            debug!("DaemonManager::read_pid: pid file does not exist");
            return None;
        }

        let mut file = fs::File::open(&self.pid_file).ok()?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).ok()?;

        let pid = contents.trim().parse().ok();
        debug!(?pid, "DaemonManager::read_pid: returning");
        pid
    }

    /// Write the PID to the PID file
    fn write_pid(&self, pid: u32) -> Result<()> {
        debug!(pid, ?self.pid_file, "DaemonManager::write_pid: called");
        // Ensure parent directory exists
        if let Some(parent) = self.pid_file.parent() {
            debug!(?parent, "DaemonManager::write_pid: creating parent directory");
            fs::create_dir_all(parent).context("Failed to create PID file directory")?;
        }

        let mut file = fs::File::create(&self.pid_file).context("Failed to create PID file")?;
        write!(file, "{}", pid).context("Failed to write PID")?;

        debug!(pid, path = ?self.pid_file, "Wrote PID file");
        Ok(())
    }

    /// Remove the PID file
    fn remove_pid_file(&self) -> Result<()> {
        debug!(?self.pid_file, "DaemonManager::remove_pid_file: called");
        if self.pid_file.exists() {
            debug!("DaemonManager::remove_pid_file: removing file");
            fs::remove_file(&self.pid_file).context("Failed to remove PID file")?;
            debug!(path = ?self.pid_file, "Removed PID file");
        } else {
            debug!("DaemonManager::remove_pid_file: file does not exist");
        }
        Ok(())
    }

    /// Write the version to the version file
    fn write_version(&self, version: &str) -> Result<()> {
        debug!(?self.version_file, version, "DaemonManager::write_version: called");
        // Ensure parent directory exists
        if let Some(parent) = self.version_file.parent() {
            fs::create_dir_all(parent).context("Failed to create version file directory")?;
        }

        let mut file = fs::File::create(&self.version_file).context("Failed to create version file")?;
        write!(file, "{}", version).context("Failed to write version")?;

        debug!(version, path = ?self.version_file, "Wrote version file");
        Ok(())
    }

    /// Read the version from the version file
    pub fn read_version(&self) -> Option<String> {
        debug!(?self.version_file, "DaemonManager::read_version: called");
        if !self.version_file.exists() {
            debug!("DaemonManager::read_version: version file does not exist");
            return None;
        }

        let mut file = fs::File::open(&self.version_file).ok()?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).ok()?;

        let version = contents.trim().to_string();
        debug!(?version, "DaemonManager::read_version: returning");
        Some(version)
    }

    /// Remove the version file
    fn remove_version_file(&self) -> Result<()> {
        debug!(?self.version_file, "DaemonManager::remove_version_file: called");
        if self.version_file.exists() {
            fs::remove_file(&self.version_file).context("Failed to remove version file")?;
            debug!(path = ?self.version_file, "Removed version file");
        }
        Ok(())
    }

    /// Check if the running daemon version matches the current CLI version
    pub fn version_matches(&self) -> bool {
        debug!("DaemonManager::version_matches: called");
        match self.read_version() {
            Some(daemon_version) => {
                let matches = daemon_version == VERSION;
                debug!(daemon_version, cli_version = VERSION, matches, "DaemonManager::version_matches: checked");
                matches
            }
            None => {
                debug!("DaemonManager::version_matches: no version file, assuming mismatch");
                false
            }
        }
    }

    /// Start the daemon
    ///
    /// This forks a new process and returns immediately.
    pub fn start(&self) -> Result<u32> {
        debug!("DaemonManager::start: called");
        if let Some(pid) = self.running_pid() {
            debug!(pid, "DaemonManager::start: daemon already running");
            return Err(eyre::eyre!("Daemon already running with PID {}", pid));
        }

        info!("Starting daemon...");
        debug!("DaemonManager::start: getting current executable");

        // Get the current executable path
        let exe = std::env::current_exe().context("Failed to get current executable")?;
        debug!(?exe, "DaemonManager::start: spawning daemon process");

        // Spawn the daemon process
        let child = Command::new(&exe)
            .arg("run-daemon")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to spawn daemon process")?;

        let pid = child.id();
        debug!(pid, "DaemonManager::start: daemon spawned");
        self.write_pid(pid)?;

        info!(pid, "Daemon started");
        Ok(pid)
    }

    /// Stop the daemon
    pub fn stop(&self) -> Result<()> {
        debug!("DaemonManager::stop: called");
        let pid = self.running_pid().ok_or_else(|| {
            debug!("DaemonManager::stop: daemon is not running");
            eyre::eyre!("Daemon is not running")
        })?;

        info!(pid, "Stopping daemon...");
        debug!(pid, "DaemonManager::stop: sending termination signal");

        // Send SIGTERM on Unix
        #[cfg(unix)]
        {
            use nix::sys::signal::{Signal, kill};
            use nix::unistd::Pid;

            debug!(pid, "DaemonManager::stop: sending SIGTERM");
            kill(Pid::from_raw(pid as i32), Signal::SIGTERM).context("Failed to send SIGTERM")?;
        }

        // On Windows, use taskkill
        #[cfg(windows)]
        {
            debug!(pid, "DaemonManager::stop: using taskkill");
            Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F"])
                .output()
                .context("Failed to kill process")?;
        }

        // Wait for process to exit (with timeout)
        debug!("DaemonManager::stop: waiting for process to exit");
        let mut attempts = 0;
        while is_process_running(pid) && attempts < 50 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            attempts += 1;
        }
        debug!(attempts, "DaemonManager::stop: waited for process");

        if is_process_running(pid) {
            debug!(pid, "DaemonManager::stop: process still running, sending SIGKILL");
            warn!(pid, "Daemon did not stop gracefully, sending SIGKILL");
            #[cfg(unix)]
            {
                use nix::sys::signal::{Signal, kill};
                use nix::unistd::Pid;
                let _ = kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
            }
        } else {
            debug!("DaemonManager::stop: process exited gracefully");
        }

        self.remove_pid_file()?;
        self.remove_version_file()?;
        info!(pid, "Daemon stopped");
        debug!("DaemonManager::stop: done");
        Ok(())
    }

    /// Register the current process as the daemon
    ///
    /// This should be called by the daemon process after forking.
    pub fn register_self(&self) -> Result<()> {
        debug!("DaemonManager::register_self: called");
        let pid = std::process::id();
        debug!(pid, version = VERSION, "DaemonManager::register_self: registering pid and version");
        self.write_pid(pid)?;
        self.write_version(VERSION)?;
        info!(pid, version = VERSION, "Daemon registered");
        Ok(())
    }

    /// Get the PID file path
    pub fn pid_file(&self) -> &PathBuf {
        debug!(?self.pid_file, "DaemonManager::pid_file: called");
        &self.pid_file
    }
}

/// Check if a process with the given PID is running
fn is_process_running(pid: u32) -> bool {
    debug!(pid, "is_process_running: called");
    #[cfg(unix)]
    {
        use nix::sys::signal::kill;
        use nix::unistd::Pid;
        // Sending signal 0 checks if the process exists without affecting it
        let result = kill(Pid::from_raw(pid as i32), None).is_ok();
        debug!(pid, result, "is_process_running: unix check");
        result
    }

    #[cfg(windows)]
    {
        use std::process::Command;
        let result = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .output()
            .map(|o| !o.stdout.is_empty() && !String::from_utf8_lossy(&o.stdout).contains("No tasks"))
            .unwrap_or(false);
        debug!(pid, result, "is_process_running: windows check");
        return result;
    }

    #[cfg(not(any(unix, windows)))]
    {
        // Assume running on unknown platforms
        debug!(pid, "is_process_running: unknown platform, assuming running");
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
        debug!("DaemonManager::status: called");
        let pid = self.running_pid();
        let status = DaemonStatus {
            running: pid.is_some(),
            pid,
            pid_file: self.pid_file.clone(),
        };
        debug!(?status, "DaemonManager::status: returning");
        status
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

    #[test]
    fn test_write_and_read_version() {
        let temp_dir = TempDir::new().unwrap();
        let pid_file = temp_dir.path().join("test.pid");

        let manager = DaemonManager::with_pid_file(pid_file);

        // Initially no version file
        assert_eq!(manager.read_version(), None);

        // Write version
        manager.write_version("v1.2.3").unwrap();
        assert_eq!(manager.read_version(), Some("v1.2.3".to_string()));

        // Overwrite version
        manager.write_version("v2.0.0").unwrap();
        assert_eq!(manager.read_version(), Some("v2.0.0".to_string()));

        // Remove version file
        manager.remove_version_file().unwrap();
        assert_eq!(manager.read_version(), None);
    }

    #[test]
    fn test_version_matches_when_same() {
        let temp_dir = TempDir::new().unwrap();
        let pid_file = temp_dir.path().join("test.pid");

        let manager = DaemonManager::with_pid_file(pid_file);

        // Write the current VERSION
        manager.write_version(VERSION).unwrap();

        // Should match
        assert!(manager.version_matches());
    }

    #[test]
    fn test_version_matches_when_different() {
        let temp_dir = TempDir::new().unwrap();
        let pid_file = temp_dir.path().join("test.pid");

        let manager = DaemonManager::with_pid_file(pid_file);

        // Write a different version
        manager.write_version("totally-different-version").unwrap();

        // Should not match
        assert!(!manager.version_matches());
    }

    #[test]
    fn test_version_matches_when_no_file() {
        let temp_dir = TempDir::new().unwrap();
        let pid_file = temp_dir.path().join("test.pid");

        let manager = DaemonManager::with_pid_file(pid_file);

        // No version file exists - should return false (mismatch)
        assert!(!manager.version_matches());
    }

    #[test]
    fn test_version_file_path_derived_from_pid_file() {
        let temp_dir = TempDir::new().unwrap();
        let pid_file = temp_dir.path().join("myapp.pid");

        let manager = DaemonManager::with_pid_file(pid_file.clone());

        // Version file should be alongside pid file with .version extension
        let expected_version_file = temp_dir.path().join("myapp.version");
        assert_eq!(manager.version_file, expected_version_file);
    }
}
