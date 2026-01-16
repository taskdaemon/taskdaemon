//! Watcher module for git main branch monitoring
//!
//! The MainWatcher polls the main branch periodically and alerts
//! all running loops when an update is detected, triggering rebase.

mod config;
mod main_watcher;

pub use config::WatcherConfig;
pub use main_watcher::MainWatcher;
