//! Git worktree management
//!
//! Each Ralph loop executes in its own git worktree on a feature branch,
//! enabling parallel work without file conflicts.

mod manager;

pub use manager::{WorktreeConfig, WorktreeError, WorktreeInfo, WorktreeManager};
