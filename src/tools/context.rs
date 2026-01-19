//! ToolContext - execution context for tools

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;

use crate::coordinator::CoordinatorHandle;

use super::ToolError;

/// Execution context for tools - scoped to a single loop
///
/// Each loop gets its own `ToolContext` that scopes all operations to
/// its git worktree. This provides sandboxing - tools cannot escape
/// the worktree unless explicitly disabled.
#[derive(Clone)]
pub struct ToolContext {
    /// Git worktree path - all file ops constrained here
    pub worktree: PathBuf,

    /// Loop execution ID (for coordination events)
    pub exec_id: String,

    /// Files read this iteration (for edit validation)
    read_files: Arc<Mutex<HashSet<PathBuf>>>,

    /// Whether sandbox mode is enabled (default: true)
    pub sandbox_enabled: bool,

    /// Optional coordinator handle for inter-loop communication
    pub coordinator: Option<CoordinatorHandle>,
}

impl ToolContext {
    /// Create a new tool context
    pub fn new(worktree: PathBuf, exec_id: String) -> Self {
        debug!(?worktree, %exec_id, "ToolContext::new: called");
        Self {
            worktree,
            exec_id,
            read_files: Arc::new(Mutex::new(HashSet::new())),
            sandbox_enabled: true,
            coordinator: None,
        }
    }

    /// Create a context with sandbox disabled (for testing)
    pub fn new_unsandboxed(worktree: PathBuf, exec_id: String) -> Self {
        debug!(?worktree, %exec_id, "ToolContext::new_unsandboxed: called");
        Self {
            worktree,
            exec_id,
            read_files: Arc::new(Mutex::new(HashSet::new())),
            sandbox_enabled: false,
            coordinator: None,
        }
    }

    /// Create a context with coordinator handle for inter-loop communication
    pub fn with_coordinator(worktree: PathBuf, exec_id: String, coordinator: CoordinatorHandle) -> Self {
        debug!(?worktree, %exec_id, "ToolContext::with_coordinator: called");
        Self {
            worktree,
            exec_id,
            read_files: Arc::new(Mutex::new(HashSet::new())),
            sandbox_enabled: true,
            coordinator: Some(coordinator),
        }
    }

    /// Track that a file was read (enables edit validation)
    pub async fn track_read(&self, path: &Path) {
        debug!(?path, "ToolContext::track_read: called");
        let mut read_files = self.read_files.lock().await;
        read_files.insert(self.normalize_path(path));
    }

    /// Check if a file was read (required before edit)
    pub async fn was_read(&self, path: &Path) -> bool {
        debug!(?path, "ToolContext::was_read: called");
        let read_files = self.read_files.lock().await;
        let result = read_files.contains(&self.normalize_path(path));
        debug!(?result, "ToolContext::was_read: returning");
        result
    }

    /// Clear read tracking (called at iteration start)
    pub async fn clear_reads(&self) {
        debug!("ToolContext::clear_reads: called");
        let mut read_files = self.read_files.lock().await;
        read_files.clear();
    }

    /// Normalize a path relative to worktree
    fn normalize_path(&self, path: &Path) -> PathBuf {
        debug!(?path, "ToolContext::normalize_path: called");
        if path.is_absolute() {
            debug!("ToolContext::normalize_path: path is absolute");
            path.to_path_buf()
        } else {
            debug!("ToolContext::normalize_path: path is relative, joining with worktree");
            self.worktree.join(path)
        }
    }

    /// Validate path is within worktree (sandbox enforcement)
    pub fn validate_path(&self, path: &Path) -> Result<PathBuf, ToolError> {
        debug!(?path, "ToolContext::validate_path: called");
        let normalized = self.normalize_path(path);

        if !self.sandbox_enabled {
            debug!("ToolContext::validate_path: sandbox disabled, returning normalized path");
            return Ok(normalized);
        }

        // For paths that don't exist yet (new files), check prefix
        // For existing paths, canonicalize to resolve symlinks
        let canonical = if normalized.exists() {
            debug!("ToolContext::validate_path: path exists, canonicalizing");
            normalized.canonicalize().unwrap_or_else(|_| normalized.clone())
        } else {
            debug!("ToolContext::validate_path: path does not exist");
            // For non-existent paths, normalize parent and check
            if let Some(parent) = normalized.parent() {
                if parent.exists() {
                    debug!("ToolContext::validate_path: parent exists, canonicalizing parent");
                    let canonical_parent = parent.canonicalize().unwrap_or_else(|_| parent.to_path_buf());
                    canonical_parent.join(normalized.file_name().unwrap_or_default())
                } else {
                    debug!("ToolContext::validate_path: parent does not exist");
                    normalized.clone()
                }
            } else {
                debug!("ToolContext::validate_path: no parent directory");
                normalized.clone()
            }
        };

        let worktree_canonical = self.worktree.canonicalize().unwrap_or_else(|_| self.worktree.clone());

        if canonical.starts_with(&worktree_canonical) {
            debug!("ToolContext::validate_path: path is within worktree");
            Ok(canonical)
        } else {
            debug!("ToolContext::validate_path: sandbox violation detected");
            Err(ToolError::SandboxViolation {
                path: path.to_path_buf(),
                worktree: self.worktree.clone(),
            })
        }
    }
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("worktree", &self.worktree)
            .field("exec_id", &self.exec_id)
            .field("sandbox_enabled", &self.sandbox_enabled)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_track_and_check_read() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test-exec".to_string());

        let file_path = Path::new("src/lib.rs");

        // Initially not read
        assert!(!ctx.was_read(file_path).await);

        // Track read
        ctx.track_read(file_path).await;

        // Now it's marked as read
        assert!(ctx.was_read(file_path).await);
    }

    #[tokio::test]
    async fn test_clear_reads() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test-exec".to_string());

        ctx.track_read(Path::new("a.rs")).await;
        ctx.track_read(Path::new("b.rs")).await;

        ctx.clear_reads().await;

        assert!(!ctx.was_read(Path::new("a.rs")).await);
        assert!(!ctx.was_read(Path::new("b.rs")).await);
    }

    #[tokio::test]
    async fn test_validate_path_within_worktree() {
        let temp = tempdir().unwrap();
        let worktree = temp.path().to_path_buf();

        // Create a file inside worktree
        let file_path = worktree.join("test.txt");
        fs::write(&file_path, "content").unwrap();

        let ctx = ToolContext::new(worktree, "test-exec".to_string());

        // Relative path should work
        let result = ctx.validate_path(Path::new("test.txt"));
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_path_outside_worktree() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test-exec".to_string());

        // Absolute path outside worktree should fail
        let result = ctx.validate_path(Path::new("/etc/passwd"));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ToolError::SandboxViolation { .. }));
    }

    #[tokio::test]
    async fn test_validate_path_with_sandbox_disabled() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new_unsandboxed(temp.path().to_path_buf(), "test-exec".to_string());

        // With sandbox disabled, any path should work
        let result = ctx.validate_path(Path::new("/etc/passwd"));
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_new_file_path() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test-exec".to_string());

        // Non-existent file within worktree should be allowed
        let result = ctx.validate_path(Path::new("new_file.txt"));
        assert!(result.is_ok());
    }
}
