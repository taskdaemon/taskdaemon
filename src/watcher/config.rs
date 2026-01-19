//! Watcher configuration

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::debug;

/// Configuration for the MainWatcher
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherConfig {
    /// Polling interval in seconds
    #[serde(default = "default_poll_interval_secs")]
    pub poll_interval_secs: u64,

    /// The main branch name to watch
    #[serde(default = "default_main_branch")]
    pub main_branch: String,

    /// The remote name
    #[serde(default = "default_remote")]
    pub remote: String,

    /// Whether to fetch from remote before checking
    #[serde(default = "default_fetch_enabled")]
    pub fetch_enabled: bool,

    /// Event type for alerts
    #[serde(default = "default_event_type")]
    pub event_type: String,
}

fn default_poll_interval_secs() -> u64 {
    debug!("default_poll_interval_secs: called");
    30
}

fn default_main_branch() -> String {
    debug!("default_main_branch: called");
    "main".to_string()
}

fn default_remote() -> String {
    debug!("default_remote: called");
    "origin".to_string()
}

fn default_fetch_enabled() -> bool {
    debug!("default_fetch_enabled: called");
    true
}

fn default_event_type() -> String {
    debug!("default_event_type: called");
    "main_updated".to_string()
}

impl Default for WatcherConfig {
    fn default() -> Self {
        debug!("WatcherConfig::default: called");
        Self {
            poll_interval_secs: 30,
            main_branch: "main".to_string(),
            remote: "origin".to_string(),
            fetch_enabled: true,
            event_type: "main_updated".to_string(),
        }
    }
}

impl WatcherConfig {
    /// Get the poll interval as a Duration
    pub fn poll_interval(&self) -> Duration {
        debug!(%self.poll_interval_secs, "WatcherConfig::poll_interval: called");
        Duration::from_secs(self.poll_interval_secs)
    }

    /// Get the full remote branch reference
    pub fn remote_branch(&self) -> String {
        debug!(%self.remote, %self.main_branch, "WatcherConfig::remote_branch: called");
        format!("{}/{}", self.remote, self.main_branch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = WatcherConfig::default();
        assert_eq!(config.poll_interval_secs, 30);
        assert_eq!(config.main_branch, "main");
        assert_eq!(config.remote, "origin");
        assert!(config.fetch_enabled);
        assert_eq!(config.event_type, "main_updated");
    }

    #[test]
    fn test_poll_interval_duration() {
        let config = WatcherConfig {
            poll_interval_secs: 60,
            ..Default::default()
        };
        assert_eq!(config.poll_interval(), Duration::from_secs(60));
    }

    #[test]
    fn test_remote_branch() {
        let config = WatcherConfig::default();
        assert_eq!(config.remote_branch(), "origin/main");
    }
}
