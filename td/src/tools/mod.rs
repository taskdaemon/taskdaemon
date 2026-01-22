//! Tool system for Ralph loops
//!
//! Tools provide file system access, command execution, and coordination
//! capabilities to Ralph loops. Each loop gets a `ToolContext` scoped to
//! its git worktree - tools cannot escape the worktree sandbox.

mod context;
mod error;
mod executor;
mod traits;

pub mod builtin;

pub use context::{ExploreConfig, ExploreSpawner, ExploreSpawnerRef, Thoroughness, ToolContext};
pub use error::ToolError;
pub use executor::{ToolExecutor, ToolProfile};
pub use traits::{Tool, ToolResult};
