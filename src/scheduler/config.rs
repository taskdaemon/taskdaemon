//! Scheduler configuration

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::domain::Priority;

/// Scheduler configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    /// Max concurrent API calls
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,

    /// Max requests per rate window
    #[serde(default = "default_max_requests_per_window")]
    pub max_requests_per_window: u32,

    /// Rate limit window duration in seconds
    #[serde(default = "default_rate_window_secs")]
    pub rate_window_secs: u64,

    /// Default priority for new requests
    #[serde(default)]
    pub default_priority: Priority,
}

fn default_max_concurrent() -> usize {
    10
}

fn default_max_requests_per_window() -> u32 {
    50
}

fn default_rate_window_secs() -> u64 {
    60
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 10,
            max_requests_per_window: 50,
            rate_window_secs: 60,
            default_priority: Priority::Normal,
        }
    }
}

impl SchedulerConfig {
    /// Get the rate window as a Duration
    pub fn rate_window(&self) -> Duration {
        Duration::from_secs(self.rate_window_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SchedulerConfig::default();
        assert_eq!(config.max_concurrent, 10);
        assert_eq!(config.max_requests_per_window, 50);
        assert_eq!(config.rate_window_secs, 60);
        assert_eq!(config.default_priority, Priority::Normal);
    }

    #[test]
    fn test_rate_window_duration() {
        let config = SchedulerConfig {
            rate_window_secs: 120,
            ..Default::default()
        };
        assert_eq!(config.rate_window(), Duration::from_secs(120));
    }
}
