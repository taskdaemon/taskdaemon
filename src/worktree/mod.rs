//! Git worktree management
//!
//! Each Ralph loop executes in its own git worktree on a feature branch,
//! enabling parallel work without file conflicts.

mod manager;
mod merge;

pub use manager::{WorktreeConfig, WorktreeError, WorktreeInfo, WorktreeManager};
pub use merge::{MergeResult, merge_to_main};
