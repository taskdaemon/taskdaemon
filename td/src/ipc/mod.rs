//! Inter-Process Communication for daemon wake-up
//!
//! This module provides Unix Domain Socket-based IPC between the TUI/CLI and the daemon.
//! When the TUI changes execution state, it connects to the daemon's socket and sends
//! a message to wake it immediately (instead of waiting for the 60-second poll interval).

use std::path::PathBuf;

pub mod client;
pub mod messages;

pub use client::DaemonClient;
pub use messages::{DaemonMessage, DaemonResponse};

/// Get the socket path for daemon IPC
///
/// Uses the same base directory as other daemon files (PID file, version file).
pub fn get_socket_path() -> PathBuf {
    dirs::runtime_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("taskdaemon")
        .join("daemon.sock")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_path_ends_with_daemon_sock() {
        let path = get_socket_path();
        assert!(path.ends_with("taskdaemon/daemon.sock"));
    }
}
