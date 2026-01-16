//! Coordinator configuration

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Coordinator configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorConfig {
    /// Default query timeout in seconds
    #[serde(default = "default_query_timeout_secs")]
    pub query_timeout_secs: u64,

    /// Max messages per second per loop (rate limiting)
    #[serde(default = "default_rate_limit")]
    pub rate_limit_per_sec: usize,

    /// Max payload size in bytes (1MB default)
    #[serde(default = "default_max_payload_size")]
    pub max_payload_size: usize,

    /// Channel buffer size for coordinator requests
    #[serde(default = "default_channel_buffer")]
    pub channel_buffer: usize,

    /// Channel buffer size for loop messages
    #[serde(default = "default_loop_channel_buffer")]
    pub loop_channel_buffer: usize,
}

fn default_query_timeout_secs() -> u64 {
    30
}

fn default_rate_limit() -> usize {
    100
}

fn default_max_payload_size() -> usize {
    1024 * 1024 // 1MB
}

fn default_channel_buffer() -> usize {
    1000
}

fn default_loop_channel_buffer() -> usize {
    100
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            query_timeout_secs: 30,
            rate_limit_per_sec: 100,
            max_payload_size: 1024 * 1024,
            channel_buffer: 1000,
            loop_channel_buffer: 100,
        }
    }
}

impl CoordinatorConfig {
    /// Get the default query timeout as a Duration
    pub fn query_timeout(&self) -> Duration {
        Duration::from_secs(self.query_timeout_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = CoordinatorConfig::default();
        assert_eq!(config.query_timeout_secs, 30);
        assert_eq!(config.rate_limit_per_sec, 100);
        assert_eq!(config.max_payload_size, 1024 * 1024);
        assert_eq!(config.channel_buffer, 1000);
        assert_eq!(config.loop_channel_buffer, 100);
    }

    #[test]
    fn test_query_timeout_duration() {
        let config = CoordinatorConfig {
            query_timeout_secs: 60,
            ..Default::default()
        };
        assert_eq!(config.query_timeout(), Duration::from_secs(60));
    }
}
