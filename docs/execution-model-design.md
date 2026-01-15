# Design Document: Execution Model - Git Worktree Management and Crash Recovery

**Author:** Scott A. Idler
**Date:** 2026-01-14
**Status:** Active
**Review Passes:** Updated for Ralph Loops

## Summary

This document defines how TaskDaemon manages git worktrees for isolated loop execution and recovers from crashes. Each Ralph loop executes in its own git worktree on a feature branch, enabling parallel work without file conflicts. When loops complete or crash, worktrees are cleaned up. State persists in TaskStore, enabling full recovery after daemon restart.

## Git Worktree Management

### Worktree Creation

When a Spec becomes ready (dependencies satisfied), TaskDaemon spawns a Level 3 (Spec Implementation) loop:

```rust
async fn spawn_spec_loop(spec: &Spec) -> Result<(String, PathBuf)> {
    let exec_id = uuid::Uuid::now_v7().to_string();
    let branch_name = format!("feature/{}-{}", spec.id, exec_id);
    let worktree_path = PathBuf::from(format!("/tmp/taskdaemon/worktrees/{}", exec_id));

    // Create git worktree
    let status = Command::new("git")
        .args(["worktree", "add", worktree_path.to_str().unwrap(), "-b", &branch_name, "main"])
        .current_dir(&repo_root)
        .status()
        .await?;

    if !status.success() {
        return Err(eyre!("Failed to create worktree for {}", exec_id));
    }

    tracing::info!("Created worktree at {:?} on branch {}", worktree_path, branch_name);

    // Create execution record
    let exec = LoopExecution {
        id: exec_id.clone(),
        loop_type: "spec-implementation".to_string(),
        spec_id: Some(spec.id.clone()),
        plan_id: Some(spec.plan_id.clone()),
        worktree: Some(worktree_path.to_str().unwrap().to_string()),
        status: LoopStatus::Running,
        iteration_count: 0,
        started_at: now_ms(),
        updated_at: now_ms(),
        last_error: None,
    };

    store.create(exec)?;

    Ok((exec_id, worktree_path))
}
```

**Worktree naming:**
- Path: `/tmp/taskdaemon/worktrees/{exec_id}`
- Branch: `feature/{spec_id}-{exec_id}`
- Example: `feature/spec-001-exec-abc123`

**Why separate worktrees:**
- Parallel execution: 10 Specs work simultaneously without conflicts
- Isolation: Each loop modifies only its worktree
- Clean state: Fresh checkout from main, no leftover changes
- Easy cleanup: Remove worktree directory when done

### Worktree Cleanup

Worktrees are cleaned up when loops complete or fail:

```rust
async fn cleanup_worktree(exec_id: &str, spec_id: &str, worktree: &Path) -> Result<()> {
    tracing::info!("Cleaning up worktree for {}", exec_id);

    // Remove worktree
    let status = Command::new("git")
        .args(["worktree", "remove", worktree.to_str().unwrap(), "--force"])
        .current_dir(&repo_root)
        .status()
        .await?;

    if !status.success() {
        tracing::warn!("Failed to remove worktree {:?}, will retry later", worktree);
        // Don't fail - cleanup task will retry
    }

    // Delete branch if not merged (matches creation pattern: feature/{spec_id}-{exec_id})
    let branch_name = format!("feature/{}-{}", spec_id, exec_id);
    Command::new("git")
        .args(["branch", "-D", &branch_name])
        .current_dir(&repo_root)
        .status()
        .await?;

    Ok(())
}
```

**Cleanup triggers:**
- Loop completes successfully → merge to main, cleanup worktree
- Loop fails → cleanup worktree, mark Spec as failed
- Loop stopped by user → cleanup worktree
- Daemon shutdown → cleanup all worktrees (graceful)

**Background cleanup task:**
```rust
// Runs every 5 minutes, cleans orphaned worktrees
async fn cleanup_orphaned_worktrees(store: &Store) -> Result<()> {
    let worktrees_dir = Path::new("/tmp/taskdaemon/worktrees");
    if !worktrees_dir.exists() {
        return Ok(());
    }

    for entry in std::fs::read_dir(worktrees_dir)? {
        let entry = entry?;
        let path = entry.path();
        let exec_id = path.file_name().unwrap().to_str().unwrap();

        // Check if execution record exists and is not running
        let exec: Option<LoopExecution> = store.get(exec_id)?;
        match exec {
            None => {
                // Orphaned: no record, cleanup
                cleanup_worktree(exec_id, &path).await?;
            }
            Some(e) if e.status != LoopStatus::Running => {
                // Loop finished but worktree remains, cleanup
                cleanup_worktree(exec_id, &path).await?;
            }
            _ => {
                // Still running, leave it
            }
        }
    }

    Ok(())
}
```

### Disk Space Management

**Monitoring:**
```rust
async fn check_disk_space() -> Result<u64> {
    let output = Command::new("df")
        .args(["-BG", "/tmp"])
        .output()
        .await?;

    // Parse available GB from df output
    let available_gb = parse_df_output(&output.stdout)?;

    if available_gb < 10 {
        tracing::warn!("Low disk space: {}GB available", available_gb);
    }

    Ok(available_gb)
}
```

**Quota enforcement:**
```rust
// Before creating worktree, check if we have space
async fn ensure_disk_space() -> Result<()> {
    let available = check_disk_space().await?;
    if available < 5 {
        // Trigger aggressive cleanup
        cleanup_all_completed_worktrees().await?;
    }

    let available = check_disk_space().await?;
    if available < 5 {
        return Err(eyre!("Insufficient disk space: {}GB", available));
    }

    Ok(())
}
```

## Loop Lifecycle States

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LoopStatus {
    Pending,    // Not yet started
    Running,    // Actively iterating
    Paused,     // Paused (main branch update, rate limit)
    Rebasing,   // Rebasing worktree against main
    Blocked,    // Blocked on unresolvable issue (rebase conflict)
    Complete,   // Successfully finished
    Failed,     // Failed (max iterations, unrecoverable error)
    Stopped,    // User-requested stop
}
```

**State transitions:**
```
Pending → Running        (loop spawned, worktree created)
Running → Paused         (rate limit hit, user pause)
Running → Rebasing       (main branch updated, Alert received)
Rebasing → Running       (rebase successful)
Rebasing → Blocked       (rebase conflict, needs manual resolution)
Running → Complete       (all phases done, CI passes)
Running → Failed         (max iterations exceeded, API error)
Running → Stopped        (user requested stop)
Paused → Running         (rate limit cleared, user resume)
Paused → Stopped         (user requested stop while paused)
Blocked → Stopped        (user gives up on conflict)
```

### State Persistence

After every iteration, loop state is persisted:

```rust
async fn persist_loop_state(
    exec_id: &str,
    iteration: u32,
    status: LoopStatus,
    error: Option<String>,
    store_tx: &mpsc::Sender<StoreMessage>,
) -> Result<()> {
    let (reply_tx, reply_rx) = oneshot::channel();

    store_tx.send(StoreMessage::UpdateLoop {
        exec_id: exec_id.to_string(),
        iteration_count: iteration,
        status,
        last_error: error,
        updated_at: now_ms(),
        reply: reply_tx,
    }).await?;

    reply_rx.await??;
    Ok(())
}
```

**Persisted data:**
```jsonl
{"id":"exec-abc123","loop_type":"spec-implementation","spec_id":"spec-001","plan_id":"plan-001","worktree":"/tmp/taskdaemon/worktrees/exec-abc123","status":"Running","iteration_count":7,"started_at":1705276800000,"updated_at":1705277200000,"last_error":null}
```

## Crash Recovery

When TaskDaemon restarts, it recovers incomplete loops:

```rust
async fn recover_loops(manager: &mut LoopManager, store: &Store) -> Result<()> {
    tracing::info!("Recovering incomplete loops...");

    // Find all loops that were running when daemon crashed
    let incomplete: Vec<LoopExecution> = store.list(&[
        Filter {
            field: "status",
            op: FilterOp::Eq,
            value: IndexValue::String("Running".to_string()),
        }
    ])?;

    tracing::info!("Found {} incomplete loops", incomplete.len());

    for exec in incomplete {
        // Verify worktree still exists
        let worktree_path = exec.worktree.as_ref().unwrap();
        if !Path::new(worktree_path).exists() {
            tracing::warn!("Worktree {:?} missing, marking {} as failed", worktree_path, exec.id);
            mark_loop_failed(&exec.id, "Worktree missing after restart", store).await?;
            continue;
        }

        // Verify git repo is clean
        let status_output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(worktree_path)
            .output()
            .await?;

        if !status_output.stdout.is_empty() {
            tracing::warn!("Worktree {} has uncommitted changes, committing before resume", exec.id);
            // Auto-commit any uncommitted changes
            Command::new("git")
                .args(["add", "-A"])
                .current_dir(worktree_path)
                .status()
                .await?;
            Command::new("git")
                .args(["commit", "-m", "Auto-commit before crash recovery"])
                .current_dir(worktree_path)
                .status()
                .await?;
        }

        // Resume loop
        tracing::info!("Resuming loop {}", exec.id);
        let loop_level = LoopLevel::SpecOuter {
            spec_id: exec.spec_id.unwrap(),
            worktree: PathBuf::from(worktree_path),
        };
        manager.spawn_loop(loop_level).await?;
    }

    Ok(())
}
```

**Recovery guarantees:**
- All uncommitted work in worktree is preserved (auto-commit before resume)
- Iteration count continues from last persisted value
- Fresh context window on resume (Ralph pattern)
- Failed worktrees (missing/corrupted) marked as failed, not resumed

## Proactive Rebase

When main branch is updated, all running loops must rebase:

```rust
// MainWatcher detects push to main
async fn handle_main_update(
    commit_sha: String,
    coordinator: &Coordinator,
) -> Result<()> {
    tracing::info!("Main branch updated to {}, alerting all loops", commit_sha);

    coordinator.alert(Alert::MainBranchUpdated {
        commit_sha: commit_sha.clone(),
        timestamp: now_ms(),
    }).await?;

    Ok(())
}

// Loops receive alert and rebase
async fn handle_rebase_alert(
    alert: Alert,
    worktree: &Path,
    exec_id: &str,
    store_tx: &mpsc::Sender<StoreMessage>,
) -> Result<()> {
    match alert {
        Alert::MainBranchUpdated { commit_sha, .. } => {
            tracing::info!("Rebasing {} against main@{}", exec_id, commit_sha);

            // Update status to Rebasing
            persist_loop_state(exec_id, 0, LoopStatus::Rebasing, None, store_tx).await?;

            // Commit any uncommitted changes
            Command::new("git")
                .args(["add", "-A"])
                .current_dir(worktree)
                .status()
                .await?;
            Command::new("git")
                .args(["commit", "-m", "WIP: before rebase", "--allow-empty"])
                .current_dir(worktree)
                .status()
                .await?;

            // Rebase
            let rebase_result = Command::new("git")
                .args(["rebase", "main"])
                .current_dir(worktree)
                .status()
                .await?;

            if !rebase_result.success() {
                // Conflict: abort and mark blocked
                Command::new("git")
                    .args(["rebase", "--abort"])
                    .current_dir(worktree)
                    .status()
                    .await?;

                tracing::error!("Rebase conflict in {}, manual intervention needed", exec_id);
                persist_loop_state(
                    exec_id,
                    0,
                    LoopStatus::Blocked,
                    Some("Rebase conflict with main".to_string()),
                    store_tx,
                ).await?;

                return Err(eyre!("Rebase conflict requires manual resolution"));
            }

            // Success: resume
            tracing::info!("Rebase complete for {}, resuming", exec_id);
            persist_loop_state(exec_id, 0, LoopStatus::Running, None, store_tx).await?;

            Ok(())
        }
    }
}
```

**Rebase strategy:**
- Commit any uncommitted work (WIP commit)
- Attempt rebase against main
- On conflict: abort, mark Blocked, alert operator
- On success: resume loop with next iteration

## Worktree Merge to Main

When a loop completes successfully:

```rust
async fn merge_to_main(
    exec_id: &str,
    spec: &Spec,
    worktree: &Path,
) -> Result<()> {
    let branch_name = format!("feature/{}-{}", spec.id, exec_id);

    // Switch to main
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(&repo_root)
        .status()
        .await?;

    // Pull latest
    Command::new("git")
        .args(["pull", "origin", "main"])
        .current_dir(&repo_root)
        .status()
        .await?;

    // Merge feature branch
    let merge_result = Command::new("git")
        .args(["merge", "--no-ff", &branch_name, "-m",
               &format!("Merge {}: {}", spec.id, spec.title)])
        .current_dir(&repo_root)
        .status()
        .await?;

    if !merge_result.success() {
        return Err(eyre!("Failed to merge {} to main", branch_name));
    }

    tracing::info!("Merged {} to main", branch_name);

    // Alert all other loops (main updated)
    coordinator.alert(Alert::MainBranchUpdated {
        commit_sha: get_main_sha().await?,
        timestamp: now_ms(),
    }).await?;

    // Cleanup worktree
    cleanup_worktree(exec_id, worktree).await?;

    Ok(())
}
```

## Performance Characteristics

**Expected metrics:**
- **Worktree creation time:** <1s
- **Worktree cleanup time:** <500ms
- **Disk per worktree:** 50-100MB (depends on repo size)
- **Max concurrent worktrees:** 50 (configurable)
- **Rebase time:** 1-5s (no conflicts), manual (conflicts)

## Edge Cases

### Disk Full During Creation

```rust
match spawn_spec_loop(&spec).await {
    Err(e) if e.to_string().contains("No space left") => {
        tracing::error!("Disk full, cannot create worktree");
        // Trigger aggressive cleanup
        cleanup_all_completed_worktrees().await?;
        // Retry once
        spawn_spec_loop(&spec).await?
    }
    Err(e) => return Err(e),
    Ok(result) => result,
}
```

### Worktree Corruption

```rust
// After creating worktree, validate it
async fn validate_worktree(path: &Path) -> Result<()> {
    let status = Command::new("git")
        .args(["status"])
        .current_dir(path)
        .status()
        .await?;

    if !status.success() {
        return Err(eyre!("Worktree at {:?} is corrupted", path));
    }

    Ok(())
}
```

### Cleanup Failure

```rust
// If cleanup fails, don't block - log and continue
async fn cleanup_worktree_safe(exec_id: &str, worktree: &Path) -> Result<()> {
    match cleanup_worktree(exec_id, worktree).await {
        Ok(_) => Ok(()),
        Err(e) => {
            tracing::warn!("Cleanup failed for {}: {}, will retry later", exec_id, e);
            // Background task will retry
            Ok(())
        }
    }
}
```

## References

- [Main Design](./taskdaemon-design.md) - Overall Ralph loop architecture
- [Coordinator Protocol](./coordinator-design.md) - Alert/Share/Query events
- [Implementation Details](./implementation-details.md) - Loop schema, domain types
- [Config Schema](./config-schema.md) - Configuration hierarchy
- Git worktree docs: `man git-worktree`
