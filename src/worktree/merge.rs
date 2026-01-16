//! Git merge operations for completed specs
//!
//! Handles merging completed worktree branches back to main.

use std::path::Path;

use eyre::{Result, bail};
use tokio::process::Command;
use tracing::{info, warn};

/// Result of a merge operation
#[derive(Debug, Clone)]
pub enum MergeResult {
    /// Merge succeeded
    Success,
    /// Merge had conflicts that need resolution
    Conflict { message: String },
    /// Push to remote failed
    PushFailed { message: String },
}

impl MergeResult {
    /// Check if the merge was successful
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success)
    }

    /// Check if there was a conflict
    pub fn is_conflict(&self) -> bool {
        matches!(self, Self::Conflict { .. })
    }

    /// Get error message if any
    pub fn error_message(&self) -> Option<&str> {
        match self {
            Self::Success => None,
            Self::Conflict { message } => Some(message),
            Self::PushFailed { message } => Some(message),
        }
    }
}

/// Merge a completed spec's worktree branch to main
///
/// This function:
/// 1. Auto-commits any uncommitted changes in the worktree
/// 2. Switches to main in the repo root
/// 3. Pulls latest main
/// 4. Merges the feature branch with --no-ff
/// 5. Pushes to remote
///
/// # Arguments
/// * `repo_root` - Path to the main repository
/// * `worktree_path` - Path to the worktree
/// * `exec_id` - Execution ID (used for branch name)
/// * `spec_title` - Title of the spec (used in commit message)
///
/// # Returns
/// * `Ok(MergeResult::Success)` if merge completed successfully
/// * `Ok(MergeResult::Conflict { .. })` if there were merge conflicts
/// * `Ok(MergeResult::PushFailed { .. })` if push to remote failed
/// * `Err(..)` for other git failures
pub async fn merge_to_main(
    repo_root: &Path,
    worktree_path: &Path,
    exec_id: &str,
    spec_title: &str,
) -> Result<MergeResult> {
    let branch_name = format!("taskdaemon/{}", exec_id);

    info!(
        exec_id = %exec_id,
        branch = %branch_name,
        spec = %spec_title,
        "Starting merge to main"
    );

    // 1. Ensure all changes are committed in worktree
    let status = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(worktree_path)
        .output()
        .await?;

    if !status.stdout.is_empty() {
        info!("Auto-committing uncommitted changes in worktree");

        // Stage all changes
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(worktree_path)
            .output()
            .await?;

        // Commit
        let commit_msg = format!("WIP: Auto-commit before merge for {}", spec_title);
        let commit_output = Command::new("git")
            .args(["commit", "-m", &commit_msg])
            .current_dir(worktree_path)
            .output()
            .await?;

        if !commit_output.status.success() {
            let stderr = String::from_utf8_lossy(&commit_output.stderr);
            warn!("Auto-commit failed: {}", stderr);
            // Continue anyway - might be nothing to commit
        }
    }

    // 2. Switch to main in repo root
    let checkout_output = Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo_root)
        .output()
        .await?;

    if !checkout_output.status.success() {
        let stderr = String::from_utf8_lossy(&checkout_output.stderr);
        bail!("Failed to checkout main: {}", stderr);
    }

    // 3. Pull latest main
    let pull_output = Command::new("git")
        .args(["pull", "--rebase"])
        .current_dir(repo_root)
        .output()
        .await?;

    if !pull_output.status.success() {
        let stderr = String::from_utf8_lossy(&pull_output.stderr);
        warn!("Pull failed (might be no remote): {}", stderr);
        // Continue anyway - might be a local-only repo
    }

    // 4. Merge the feature branch with no-ff
    let merge_msg = format!("Merge spec: {}", spec_title);
    let merge_output = Command::new("git")
        .args(["merge", "--no-ff", &branch_name, "-m", &merge_msg])
        .current_dir(repo_root)
        .output()
        .await?;

    if !merge_output.status.success() {
        let stderr = String::from_utf8_lossy(&merge_output.stderr);
        if stderr.contains("CONFLICT") {
            warn!("Merge conflict detected for {}", exec_id);
            return Ok(MergeResult::Conflict {
                message: stderr.to_string(),
            });
        }
        bail!("Merge failed: {}", stderr);
    }

    info!(
        exec_id = %exec_id,
        "Merge completed, pushing to remote"
    );

    // 5. Push to remote
    let push_output = Command::new("git")
        .args(["push", "origin", "main"])
        .current_dir(repo_root)
        .output()
        .await?;

    if !push_output.status.success() {
        let stderr = String::from_utf8_lossy(&push_output.stderr);
        warn!("Push failed: {}", stderr);
        return Ok(MergeResult::PushFailed {
            message: stderr.to_string(),
        });
    }

    info!(
        exec_id = %exec_id,
        branch = %branch_name,
        "Successfully merged and pushed to main"
    );

    Ok(MergeResult::Success)
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

        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "initial"])
            .current_dir(dir)
            .output()
            .await
            .unwrap();
    }

    #[test]
    fn test_merge_result_is_success() {
        assert!(MergeResult::Success.is_success());
        assert!(
            !MergeResult::Conflict {
                message: "conflict".into()
            }
            .is_success()
        );
        assert!(
            !MergeResult::PushFailed {
                message: "failed".into()
            }
            .is_success()
        );
    }

    #[test]
    fn test_merge_result_is_conflict() {
        assert!(!MergeResult::Success.is_conflict());
        assert!(
            MergeResult::Conflict {
                message: "conflict".into()
            }
            .is_conflict()
        );
    }

    #[test]
    fn test_merge_result_error_message() {
        assert!(MergeResult::Success.error_message().is_none());
        assert_eq!(
            MergeResult::Conflict { message: "test".into() }.error_message(),
            Some("test")
        );
        assert_eq!(
            MergeResult::PushFailed {
                message: "error".into()
            }
            .error_message(),
            Some("error")
        );
    }

    #[tokio::test]
    async fn test_merge_nonexistent_branch() {
        let repo_dir = tempdir().unwrap();
        let worktree_dir = tempdir().unwrap();

        setup_git_repo(repo_dir.path()).await;

        // Try to merge a branch that doesn't exist - should fail
        let result = merge_to_main(repo_dir.path(), worktree_dir.path(), "nonexistent", "Test Spec").await;

        assert!(result.is_err());
    }
}
