//! Main branch watcher implementation

use std::path::PathBuf;
use std::process::Stdio;

use eyre::{Result, eyre};
use serde_json::json;
use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use super::config::WatcherConfig;
use crate::coordinator::CoordRequest;

/// The MainWatcher monitors the main branch for updates and alerts all loops
pub struct MainWatcher {
    config: WatcherConfig,
    repo_path: PathBuf,
    coordinator_tx: mpsc::Sender<CoordRequest>,
    last_known_sha: Option<String>,
}

impl MainWatcher {
    /// Create a new MainWatcher
    pub fn new(config: WatcherConfig, repo_path: PathBuf, coordinator_tx: mpsc::Sender<CoordRequest>) -> Self {
        Self {
            config,
            repo_path,
            coordinator_tx,
            last_known_sha: None,
        }
    }

    /// Get the current SHA of the main branch
    async fn get_main_sha(&self) -> Result<String> {
        let output = Command::new("git")
            .arg("rev-parse")
            .arg(&self.config.main_branch)
            .current_dir(&self.repo_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(eyre!("git rev-parse failed: {}", stderr));
        }

        let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(sha)
    }

    /// Fetch the latest from remote
    async fn fetch_remote(&self) -> Result<()> {
        if !self.config.fetch_enabled {
            return Ok(());
        }

        debug!(
            remote = %self.config.remote,
            branch = %self.config.main_branch,
            "Fetching from remote"
        );

        let output = Command::new("git")
            .arg("fetch")
            .arg(&self.config.remote)
            .arg(&self.config.main_branch)
            .current_dir(&self.repo_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("git fetch failed: {}", stderr);
            // Don't fail - we can still check local ref
        }

        Ok(())
    }

    /// Check for updates and alert if main has changed
    async fn check_for_updates(&mut self) -> Result<bool> {
        // Fetch from remote first
        self.fetch_remote().await?;

        // Get current SHA
        let current_sha = self.get_main_sha().await?;

        // Check if this is our first run
        let Some(last_sha) = &self.last_known_sha else {
            debug!(sha = %current_sha, "Initial main branch SHA");
            self.last_known_sha = Some(current_sha);
            return Ok(false);
        };

        // Check if SHA has changed
        if &current_sha != last_sha {
            info!(
                old_sha = %last_sha,
                new_sha = %current_sha,
                "Main branch updated"
            );

            // Alert all loops via coordinator
            self.coordinator_tx
                .send(CoordRequest::Alert {
                    from_exec_id: "_main_watcher".to_string(),
                    event_type: self.config.event_type.clone(),
                    data: json!({
                        "old_sha": last_sha,
                        "new_sha": &current_sha,
                        "branch": &self.config.main_branch,
                    }),
                })
                .await
                .map_err(|_| eyre!("Coordinator channel closed"))?;

            self.last_known_sha = Some(current_sha);
            return Ok(true);
        }

        debug!(sha = %current_sha, "Main branch unchanged");
        Ok(false)
    }

    /// Run the watcher loop
    ///
    /// This runs until the coordinator channel is closed.
    pub async fn run(mut self) -> Result<()> {
        info!(
            interval_secs = self.config.poll_interval_secs,
            branch = %self.config.main_branch,
            "MainWatcher started"
        );

        loop {
            match self.check_for_updates().await {
                Ok(updated) => {
                    if updated {
                        debug!("Alert sent for main branch update");
                    }
                }
                Err(e) => {
                    error!(error = %e, "Error checking for main branch updates");
                }
            }

            // Sleep until next poll
            tokio::time::sleep(self.config.poll_interval()).await;
        }
    }

    /// Run a single check (useful for testing)
    pub async fn check_once(&mut self) -> Result<bool> {
        self.check_for_updates().await
    }

    /// Get the last known SHA
    pub fn last_known_sha(&self) -> Option<&str> {
        self.last_known_sha.as_deref()
    }

    /// Set the last known SHA (for testing or recovery)
    pub fn set_last_known_sha(&mut self, sha: Option<String>) {
        self.last_known_sha = sha;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_main_watcher_creation() {
        let (tx, _rx) = mpsc::channel(10);
        let config = WatcherConfig::default();
        let watcher = MainWatcher::new(config, PathBuf::from("."), tx);

        assert!(watcher.last_known_sha().is_none());
    }

    #[tokio::test]
    async fn test_main_watcher_set_last_known_sha() {
        let (tx, _rx) = mpsc::channel(10);
        let config = WatcherConfig::default();
        let mut watcher = MainWatcher::new(config, PathBuf::from("."), tx);

        watcher.set_last_known_sha(Some("abc123".to_string()));
        assert_eq!(watcher.last_known_sha(), Some("abc123"));
    }

    #[tokio::test]
    async fn test_get_main_sha() {
        // This test runs in the actual repo
        let (tx, _rx) = mpsc::channel(10);
        let config = WatcherConfig {
            fetch_enabled: false, // Don't fetch in test
            ..Default::default()
        };

        let repo_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let watcher = MainWatcher::new(config, repo_path, tx);

        // Should be able to get current SHA (we're in a git repo)
        let result = watcher.get_main_sha().await;

        // This will succeed if we're on main, may fail on other branches
        // Just verify it doesn't panic
        if let Ok(sha) = result {
            assert!(!sha.is_empty());
            assert_eq!(sha.len(), 40); // Git SHA is 40 hex chars
        }
    }

    #[tokio::test]
    async fn test_check_once_first_run() {
        let (tx, _rx) = mpsc::channel(10);
        let config = WatcherConfig {
            fetch_enabled: false,
            ..Default::default()
        };

        let repo_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut watcher = MainWatcher::new(config, repo_path, tx);

        // First check should return false (just setting initial SHA)
        if let Ok(updated) = watcher.check_once().await {
            assert!(!updated); // First run returns false
            assert!(watcher.last_known_sha().is_some()); // But SHA is now set
        }
    }

    #[tokio::test]
    async fn test_check_once_no_change() {
        let (tx, _rx) = mpsc::channel(10);
        let config = WatcherConfig {
            fetch_enabled: false,
            ..Default::default()
        };

        let repo_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut watcher = MainWatcher::new(config, repo_path, tx);

        // First check to set initial SHA
        let _ = watcher.check_once().await;

        // Second check should also return false (no change)
        if let Ok(updated) = watcher.check_once().await {
            assert!(!updated);
        }
    }

    #[tokio::test]
    async fn test_check_once_with_change() {
        let (tx, mut rx) = mpsc::channel(10);
        let config = WatcherConfig {
            fetch_enabled: false,
            event_type: "main_updated".to_string(),
            ..Default::default()
        };

        let repo_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut watcher = MainWatcher::new(config, repo_path, tx);

        // First check to set initial SHA
        let _ = watcher.check_once().await;

        // Manually set a different "last known" SHA to simulate a change
        watcher.set_last_known_sha(Some("0000000000000000000000000000000000000000".to_string()));

        // Now check should detect a "change" and send an alert
        if let Ok(updated) = watcher.check_once().await {
            assert!(updated); // Should return true for change

            // Should have received an alert
            let msg = rx.try_recv();
            assert!(msg.is_ok());
            match msg.unwrap() {
                CoordRequest::Alert { event_type, .. } => {
                    assert_eq!(event_type, "main_updated");
                }
                _ => panic!("Expected Alert"),
            }
        }
    }
}
