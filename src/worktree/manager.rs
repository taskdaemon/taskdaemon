//! Worktree manager for creating, rebasing, and cleaning up git worktrees

use eyre::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tracing::{debug, info, warn};

/// Error types for worktree operations
#[derive(Debug, thiserror::Error)]
pub enum WorktreeError {
    #[error("Failed to create worktree: {0}")]
    CreateFailed(String),

    #[error("Failed to remove worktree: {0}")]
    RemoveFailed(String),

    #[error("Rebase conflict in worktree: {0}")]
    RebaseConflict(String),

    #[error("Worktree not found: {0}")]
    NotFound(String),

    #[error("Worktree corrupted: {0}")]
    Corrupted(String),

    #[error("Disk space error: {0}")]
    DiskSpace(String),

    #[error("Git command failed: {0}")]
    GitError(String),
}

/// Configuration for worktree manager
#[derive(Debug, Clone)]
pub struct WorktreeConfig {
    /// Base directory for worktrees (default: /tmp/taskdaemon/worktrees)
    pub base_dir: PathBuf,

    /// Path to the main repository
    pub repo_root: PathBuf,

    /// Minimum disk space in GB before refusing to create worktrees
    pub min_disk_space_gb: u64,

    /// Branch prefix for worktree branches
    pub branch_prefix: String,
}

impl Default for WorktreeConfig {
    fn default() -> Self {
        debug!("WorktreeConfig::default: called");
        Self {
            base_dir: PathBuf::from("/tmp/taskdaemon/worktrees"),
            repo_root: PathBuf::from("."),
            min_disk_space_gb: 5,
            branch_prefix: "taskdaemon".to_string(),
        }
    }
}

impl WorktreeConfig {
    /// Create config with specified repo root
    pub fn with_repo(repo_root: impl Into<PathBuf>) -> Self {
        let repo_root = repo_root.into();
        debug!(?repo_root, "WorktreeConfig::with_repo: called");
        Self {
            repo_root,
            ..Default::default()
        }
    }
}

/// Information about a created worktree
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    /// Execution ID
    pub exec_id: String,

    /// Path to the worktree
    pub path: PathBuf,

    /// Branch name
    pub branch: String,
}

/// Manager for git worktrees
pub struct WorktreeManager {
    config: WorktreeConfig,
}

impl WorktreeManager {
    /// Create a new worktree manager
    pub fn new(config: WorktreeConfig) -> Self {
        debug!(?config, "WorktreeManager::new: called");
        Self { config }
    }

    /// Create a new worktree for a loop execution
    pub async fn create(&self, exec_id: &str) -> Result<WorktreeInfo, WorktreeError> {
        debug!(%exec_id, "WorktreeManager::create: called");

        // Check disk space first
        self.ensure_disk_space().await?;

        // Ensure base directory exists
        if let Err(e) = tokio::fs::create_dir_all(&self.config.base_dir).await {
            debug!("WorktreeManager::create: failed to create base dir");
            return Err(WorktreeError::CreateFailed(format!("Failed to create base dir: {}", e)));
        }
        debug!("WorktreeManager::create: base directory exists");

        let worktree_path = self.config.base_dir.join(exec_id);
        let branch_name = format!("{}/{}", self.config.branch_prefix, exec_id);

        // Create the worktree
        let output = Command::new("git")
            .args([
                "worktree",
                "add",
                worktree_path.to_str().unwrap(),
                "-b",
                &branch_name,
                "HEAD",
            ])
            .current_dir(&self.config.repo_root)
            .output()
            .await
            .map_err(|e| WorktreeError::GitError(e.to_string()))?;

        if !output.status.success() {
            debug!("WorktreeManager::create: git worktree add failed");
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(WorktreeError::CreateFailed(stderr.to_string()));
        }
        debug!("WorktreeManager::create: git worktree add succeeded");

        info!("Created worktree at {:?} on branch {}", worktree_path, branch_name);

        Ok(WorktreeInfo {
            exec_id: exec_id.to_string(),
            path: worktree_path,
            branch: branch_name,
        })
    }

    /// Remove a worktree
    pub async fn remove(&self, exec_id: &str) -> Result<(), WorktreeError> {
        debug!(%exec_id, "WorktreeManager::remove: called");
        let worktree_path = self.config.base_dir.join(exec_id);

        if !worktree_path.exists() {
            debug!("WorktreeManager::remove: worktree does not exist, skipping");
            warn!("Worktree {:?} does not exist, skipping removal", worktree_path);
            return Ok(());
        }
        debug!("WorktreeManager::remove: worktree exists, proceeding with removal");

        // Remove the worktree
        let output = Command::new("git")
            .args(["worktree", "remove", worktree_path.to_str().unwrap(), "--force"])
            .current_dir(&self.config.repo_root)
            .output()
            .await
            .map_err(|e| WorktreeError::GitError(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Don't fail if already removed
            if !stderr.contains("is not a working tree") {
                debug!("WorktreeManager::remove: git worktree remove failed");
                return Err(WorktreeError::RemoveFailed(stderr.to_string()));
            }
            debug!("WorktreeManager::remove: worktree already removed");
        } else {
            debug!("WorktreeManager::remove: git worktree remove succeeded");
        }

        // Delete the branch
        let branch_name = format!("{}/{}", self.config.branch_prefix, exec_id);
        let _ = Command::new("git")
            .args(["branch", "-D", &branch_name])
            .current_dir(&self.config.repo_root)
            .output()
            .await;
        debug!("WorktreeManager::remove: branch deletion attempted");

        info!("Removed worktree for {}", exec_id);

        Ok(())
    }

    /// Rebase a worktree against main branch
    pub async fn rebase(&self, exec_id: &str) -> Result<(), WorktreeError> {
        debug!(%exec_id, "WorktreeManager::rebase: called");
        let worktree_path = self.config.base_dir.join(exec_id);

        if !worktree_path.exists() {
            debug!("WorktreeManager::rebase: worktree not found");
            return Err(WorktreeError::NotFound(exec_id.to_string()));
        }
        debug!("WorktreeManager::rebase: worktree exists");

        // First, commit any uncommitted changes
        self.auto_commit(&worktree_path, "WIP: before rebase").await?;

        // Attempt rebase
        let output = Command::new("git")
            .args(["rebase", "main"])
            .current_dir(&worktree_path)
            .output()
            .await
            .map_err(|e| WorktreeError::GitError(e.to_string()))?;

        if !output.status.success() {
            debug!("WorktreeManager::rebase: rebase failed, aborting");
            // Abort the rebase
            let _ = Command::new("git")
                .args(["rebase", "--abort"])
                .current_dir(&worktree_path)
                .output()
                .await;

            return Err(WorktreeError::RebaseConflict(exec_id.to_string()));
        }
        debug!("WorktreeManager::rebase: rebase succeeded");

        info!("Successfully rebased worktree for {}", exec_id);

        Ok(())
    }

    /// Auto-commit any uncommitted changes in a worktree
    async fn auto_commit(&self, worktree_path: &Path, message: &str) -> Result<(), WorktreeError> {
        debug!(?worktree_path, %message, "WorktreeManager::auto_commit: called");

        // Check if there are uncommitted changes
        let status_output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(worktree_path)
            .output()
            .await
            .map_err(|e| WorktreeError::GitError(e.to_string()))?;

        if status_output.stdout.is_empty() {
            debug!("WorktreeManager::auto_commit: no uncommitted changes");
            return Ok(());
        }
        debug!("WorktreeManager::auto_commit: uncommitted changes found");

        // Stage all changes
        let _ = Command::new("git")
            .args(["add", "-A"])
            .current_dir(worktree_path)
            .output()
            .await;
        debug!("WorktreeManager::auto_commit: staged all changes");

        // Commit
        let _ = Command::new("git")
            .args(["commit", "-m", message, "--allow-empty"])
            .current_dir(worktree_path)
            .output()
            .await;
        debug!("WorktreeManager::auto_commit: committed changes");

        Ok(())
    }

    /// Validate a worktree is healthy
    pub async fn validate(&self, exec_id: &str) -> Result<(), WorktreeError> {
        debug!(%exec_id, "WorktreeManager::validate: called");
        let worktree_path = self.config.base_dir.join(exec_id);

        if !worktree_path.exists() {
            debug!("WorktreeManager::validate: worktree not found");
            return Err(WorktreeError::NotFound(exec_id.to_string()));
        }
        debug!("WorktreeManager::validate: worktree exists");

        let output = Command::new("git")
            .args(["status"])
            .current_dir(&worktree_path)
            .output()
            .await
            .map_err(|e| WorktreeError::GitError(e.to_string()))?;

        if !output.status.success() {
            debug!("WorktreeManager::validate: worktree corrupted");
            return Err(WorktreeError::Corrupted(exec_id.to_string()));
        }
        debug!("WorktreeManager::validate: worktree healthy");

        Ok(())
    }

    /// List all worktrees
    pub async fn list(&self) -> Result<Vec<WorktreeInfo>> {
        debug!("WorktreeManager::list: called");
        let mut worktrees = Vec::new();

        if !self.config.base_dir.exists() {
            debug!("WorktreeManager::list: base dir does not exist");
            return Ok(worktrees);
        }
        debug!("WorktreeManager::list: base dir exists");

        let mut entries = tokio::fs::read_dir(&self.config.base_dir)
            .await
            .context("Failed to read worktrees directory")?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                debug!(?path, "WorktreeManager::list: found worktree directory");
                let exec_id = path.file_name().unwrap().to_str().unwrap().to_string();
                let branch_name = format!("{}/{}", self.config.branch_prefix, exec_id);
                worktrees.push(WorktreeInfo {
                    exec_id,
                    path,
                    branch: branch_name,
                });
            } else {
                debug!(?path, "WorktreeManager::list: skipping non-directory entry");
            }
        }

        debug!(count = worktrees.len(), "WorktreeManager::list: returning worktrees");
        Ok(worktrees)
    }

    /// Get worktree path for an execution
    pub fn worktree_path(&self, exec_id: &str) -> PathBuf {
        debug!(%exec_id, "WorktreeManager::worktree_path: called");
        self.config.base_dir.join(exec_id)
    }

    /// Check if a worktree exists
    pub fn exists(&self, exec_id: &str) -> bool {
        debug!(%exec_id, "WorktreeManager::exists: called");
        let exists = self.worktree_path(exec_id).exists();
        debug!(%exists, "WorktreeManager::exists: result");
        exists
    }

    /// Ensure sufficient disk space before creating worktrees
    async fn ensure_disk_space(&self) -> Result<(), WorktreeError> {
        debug!("WorktreeManager::ensure_disk_space: called");
        let available_gb = self.check_disk_space().await?;

        if available_gb < self.config.min_disk_space_gb {
            debug!(
                available_gb,
                min = self.config.min_disk_space_gb,
                "WorktreeManager::ensure_disk_space: insufficient disk space"
            );
            return Err(WorktreeError::DiskSpace(format!(
                "Only {}GB available, need {}GB minimum",
                available_gb, self.config.min_disk_space_gb
            )));
        }
        debug!(
            available_gb,
            "WorktreeManager::ensure_disk_space: sufficient disk space"
        );

        Ok(())
    }

    /// Check available disk space in GB
    async fn check_disk_space(&self) -> Result<u64, WorktreeError> {
        debug!("WorktreeManager::check_disk_space: called");
        let output = Command::new("df")
            .args(["-BG", self.config.base_dir.to_str().unwrap_or("/tmp")])
            .output()
            .await
            .map_err(|e| WorktreeError::DiskSpace(format!("Failed to check disk space: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Parse df output - format: "Filesystem     1G-blocks  Used Available Use% Mounted on"
        // Second line contains values
        for line in stdout.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                // Available column is index 3
                let available = parts[3].trim_end_matches('G');
                if let Ok(gb) = available.parse::<u64>() {
                    debug!(gb, "WorktreeManager::check_disk_space: parsed available space");
                    return Ok(gb);
                }
            }
        }

        // Default to safe value if parsing fails
        debug!("WorktreeManager::check_disk_space: parsing failed, returning default");
        Ok(100)
    }

    /// Clean up orphaned worktrees (worktrees without corresponding execution records)
    pub async fn cleanup_orphaned(&self, active_exec_ids: &[String]) -> Result<usize> {
        debug!(?active_exec_ids, "WorktreeManager::cleanup_orphaned: called");
        let worktrees = self.list().await?;
        let mut cleaned = 0;

        for wt in worktrees {
            if !active_exec_ids.contains(&wt.exec_id) {
                debug!(exec_id = %wt.exec_id, "WorktreeManager::cleanup_orphaned: worktree is orphaned");
                info!("Cleaning up orphaned worktree: {}", wt.exec_id);
                if let Err(e) = self.remove(&wt.exec_id).await {
                    debug!(exec_id = %wt.exec_id, "WorktreeManager::cleanup_orphaned: removal failed");
                    warn!("Failed to remove orphaned worktree {}: {}", wt.exec_id, e);
                } else {
                    debug!(exec_id = %wt.exec_id, "WorktreeManager::cleanup_orphaned: removal succeeded");
                    cleaned += 1;
                }
            } else {
                debug!(exec_id = %wt.exec_id, "WorktreeManager::cleanup_orphaned: worktree is active");
            }
        }

        debug!(cleaned, "WorktreeManager::cleanup_orphaned: completed");
        Ok(cleaned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    async fn setup_git_repo(dir: &Path) {
        Command::new("git")
            .args(["init"])
            .current_dir(dir)
            .output()
            .await
            .unwrap();

        // Configure git
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir)
            .output()
            .await
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir)
            .output()
            .await
            .unwrap();

        // Create initial commit
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "initial"])
            .current_dir(dir)
            .output()
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_worktree_create_and_remove() {
        let repo_dir = tempdir().unwrap();
        let worktree_dir = tempdir().unwrap();

        setup_git_repo(repo_dir.path()).await;

        let config = WorktreeConfig {
            base_dir: worktree_dir.path().to_path_buf(),
            repo_root: repo_dir.path().to_path_buf(),
            min_disk_space_gb: 1,
            branch_prefix: "test".to_string(),
        };

        let manager = WorktreeManager::new(config);

        // Create worktree
        let info = manager.create("exec-123").await.unwrap();
        assert!(info.path.exists());
        assert_eq!(info.exec_id, "exec-123");
        assert_eq!(info.branch, "test/exec-123");

        // Validate it
        manager.validate("exec-123").await.unwrap();

        // Remove worktree
        manager.remove("exec-123").await.unwrap();
        assert!(!info.path.exists());
    }

    #[tokio::test]
    async fn test_worktree_list() {
        let repo_dir = tempdir().unwrap();
        let worktree_dir = tempdir().unwrap();

        setup_git_repo(repo_dir.path()).await;

        let config = WorktreeConfig {
            base_dir: worktree_dir.path().to_path_buf(),
            repo_root: repo_dir.path().to_path_buf(),
            min_disk_space_gb: 1,
            branch_prefix: "test".to_string(),
        };

        let manager = WorktreeManager::new(config);

        // Create two worktrees
        manager.create("exec-1").await.unwrap();
        manager.create("exec-2").await.unwrap();

        let list = manager.list().await.unwrap();
        assert_eq!(list.len(), 2);

        // Cleanup
        manager.remove("exec-1").await.unwrap();
        manager.remove("exec-2").await.unwrap();
    }

    #[tokio::test]
    async fn test_worktree_not_found() {
        let repo_dir = tempdir().unwrap();
        let worktree_dir = tempdir().unwrap();

        let config = WorktreeConfig {
            base_dir: worktree_dir.path().to_path_buf(),
            repo_root: repo_dir.path().to_path_buf(),
            min_disk_space_gb: 1,
            branch_prefix: "test".to_string(),
        };

        let manager = WorktreeManager::new(config);

        let result = manager.validate("nonexistent").await;
        assert!(matches!(result, Err(WorktreeError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_worktree_exists() {
        let repo_dir = tempdir().unwrap();
        let worktree_dir = tempdir().unwrap();

        setup_git_repo(repo_dir.path()).await;

        let config = WorktreeConfig {
            base_dir: worktree_dir.path().to_path_buf(),
            repo_root: repo_dir.path().to_path_buf(),
            min_disk_space_gb: 1,
            branch_prefix: "test".to_string(),
        };

        let manager = WorktreeManager::new(config);

        assert!(!manager.exists("exec-123"));

        manager.create("exec-123").await.unwrap();
        assert!(manager.exists("exec-123"));

        manager.remove("exec-123").await.unwrap();
        assert!(!manager.exists("exec-123"));
    }

    #[tokio::test]
    async fn test_worktree_path() {
        let worktree_dir = tempdir().unwrap();

        let config = WorktreeConfig {
            base_dir: worktree_dir.path().to_path_buf(),
            ..Default::default()
        };

        let manager = WorktreeManager::new(config);

        let path = manager.worktree_path("exec-123");
        assert_eq!(path, worktree_dir.path().join("exec-123"));
    }

    #[tokio::test]
    async fn test_cleanup_orphaned() {
        let repo_dir = tempdir().unwrap();
        let worktree_dir = tempdir().unwrap();

        setup_git_repo(repo_dir.path()).await;

        let config = WorktreeConfig {
            base_dir: worktree_dir.path().to_path_buf(),
            repo_root: repo_dir.path().to_path_buf(),
            min_disk_space_gb: 1,
            branch_prefix: "test".to_string(),
        };

        let manager = WorktreeManager::new(config);

        // Create three worktrees
        manager.create("exec-1").await.unwrap();
        manager.create("exec-2").await.unwrap();
        manager.create("exec-3").await.unwrap();

        // Only exec-2 is "active"
        let active = vec!["exec-2".to_string()];
        let cleaned = manager.cleanup_orphaned(&active).await.unwrap();

        assert_eq!(cleaned, 2);
        assert!(!manager.exists("exec-1"));
        assert!(manager.exists("exec-2"));
        assert!(!manager.exists("exec-3"));

        // Cleanup remaining
        manager.remove("exec-2").await.unwrap();
    }
}
