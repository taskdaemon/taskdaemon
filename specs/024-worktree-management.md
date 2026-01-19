# Spec: Worktree Management

**ID:** 024-worktree-management
**Status:** Draft
**Dependencies:** [012-main-watcher]

## Summary

Implement worktree management to provide isolated git working directories for each loop execution. This ensures loops don't interfere with each other and can work on different branches simultaneously.

## Acceptance Criteria

1. **Worktree Lifecycle**
   - Create worktrees on demand
   - Clean up after completion
   - Handle concurrent access
   - Manage disk space

2. **Git Operations**
   - Branch creation and checkout
   - Commit operations
   - Push/pull support
   - Conflict handling

3. **Isolation**
   - Separate working directories
   - Independent git state
   - No cross-contamination
   - Resource limits

4. **Management**
   - Worktree registry
   - Health monitoring
   - Garbage collection
   - Recovery mechanisms

## Implementation Phases

### Phase 1: Core Worktree Operations
- Create/delete worktrees
- Basic git operations
- Path management
- Registry implementation

### Phase 2: Lifecycle Management
- Automatic cleanup
- Resource tracking
- Health checks
- Recovery logic

### Phase 3: Advanced Git Features
- Branch strategies
- Merge operations
- Rebase support
- Conflict resolution

### Phase 4: Optimization
- Worktree pooling
- Space management
- Performance tuning
- Monitoring integration

## Technical Details

### Module Structure
```
src/git/worktree/
├── mod.rs
├── manager.rs     # Worktree manager
├── operations.rs  # Git operations
├── registry.rs    # Worktree registry
├── cleanup.rs     # Cleanup logic
├── pool.rs        # Worktree pooling
└── monitor.rs     # Health monitoring
```

### Core Types
```rust
pub struct WorktreeManager {
    base_path: PathBuf,
    repo_path: PathBuf,
    registry: Arc<RwLock<WorktreeRegistry>>,
    cleanup_policy: CleanupPolicy,
    pool: Option<WorktreePool>,
}

pub struct Worktree {
    pub id: Uuid,
    pub path: PathBuf,
    pub branch: String,
    pub loop_id: LoopId,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub state: WorktreeState,
}

pub enum WorktreeState {
    Active,
    Idle,
    Cleaning,
    Error(String),
}

pub struct WorktreeRegistry {
    worktrees: HashMap<Uuid, Worktree>,
    by_loop: HashMap<LoopId, Uuid>,
    by_branch: HashMap<String, Vec<Uuid>>,
}

pub struct CleanupPolicy {
    pub max_age: Duration,
    pub max_idle: Duration,
    pub max_worktrees: usize,
    pub cleanup_interval: Duration,
}
```

### Worktree Operations
```rust
impl WorktreeManager {
    pub async fn create_worktree(
        &self,
        loop_id: LoopId,
        branch_name: String,
    ) -> Result<Worktree, WorktreeError> {
        // Check if pool has available worktree
        if let Some(pool) = &self.pool {
            if let Some(worktree) = pool.checkout().await? {
                return self.prepare_pooled_worktree(worktree, loop_id, branch_name).await;
            }
        }

        // Create new worktree
        let worktree_id = Uuid::new_v4();
        let worktree_path = self.base_path.join(worktree_id.to_string());

        // Git worktree add
        let repo = Repository::open(&self.repo_path)?;
        repo.worktree(
            &worktree_id.to_string(),
            &worktree_path,
            Some(&mut WorktreeAddOptions::new()),
        )?;

        // Checkout branch
        self.checkout_branch(&worktree_path, &branch_name).await?;

        // Register worktree
        let worktree = Worktree {
            id: worktree_id,
            path: worktree_path,
            branch: branch_name,
            loop_id,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            state: WorktreeState::Active,
        };

        self.registry.write().await.register(worktree.clone())?;

        Ok(worktree)
    }

    pub async fn release_worktree(&self, worktree_id: Uuid) -> Result<(), WorktreeError> {
        let mut registry = self.registry.write().await;

        if let Some(mut worktree) = registry.get_mut(worktree_id) {
            worktree.state = WorktreeState::Idle;
            worktree.last_accessed = Utc::now();

            // Return to pool if configured
            if let Some(pool) = &self.pool {
                if pool.can_reuse(&worktree) {
                    pool.return_worktree(worktree.clone()).await?;
                    return Ok(());
                }
            }

            // Otherwise schedule for cleanup
            self.schedule_cleanup(worktree_id).await;
        }

        Ok(())
    }
}
```

### Git Operations
```rust
impl WorktreeManager {
    pub async fn commit(
        &self,
        worktree_id: Uuid,
        message: &str,
        files: Vec<PathBuf>,
    ) -> Result<Oid, GitError> {
        let worktree = self.get_worktree(worktree_id).await?;
        let repo = Repository::open(&worktree.path)?;

        // Stage files
        let mut index = repo.index()?;
        for file in files {
            index.add_path(&file)?;
        }
        index.write()?;

        // Create commit
        let sig = self.get_signature()?;
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        let parent_commit = repo.head()?.peel_to_commit()?;

        let commit_id = repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            message,
            &tree,
            &[&parent_commit],
        )?;

        // Update registry
        self.update_last_accessed(worktree_id).await;

        Ok(commit_id)
    }

    pub async fn push(
        &self,
        worktree_id: Uuid,
        remote_name: &str,
    ) -> Result<(), GitError> {
        let worktree = self.get_worktree(worktree_id).await?;
        let repo = Repository::open(&worktree.path)?;

        // Get remote
        let mut remote = repo.find_remote(remote_name)?;

        // Push options
        let mut push_options = PushOptions::new();
        push_options.remote_callbacks(self.create_callbacks());

        // Push current branch
        let refspec = format!("refs/heads/{}", worktree.branch);
        remote.push(&[&refspec], Some(&mut push_options))?;

        Ok(())
    }

    pub async fn pull_rebase(
        &self,
        worktree_id: Uuid,
        remote_name: &str,
    ) -> Result<RebaseResult, GitError> {
        let worktree = self.get_worktree(worktree_id).await?;
        let repo = Repository::open(&worktree.path)?;

        // Fetch latest
        self.fetch(&repo, remote_name).await?;

        // Rebase
        let annotated_commit = self.find_annotated_commit(&repo, &worktree.branch)?;
        let mut rebase_options = RebaseOptions::new();

        let mut rebase = repo.rebase(
            None,
            Some(&annotated_commit),
            None,
            Some(&mut rebase_options),
        )?;

        // Process rebase
        let mut result = RebaseResult::default();
        while let Some(operation) = rebase.next() {
            match operation {
                Ok(rebase_operation) => {
                    rebase.commit(None, &self.get_signature()?, None)?;
                    result.commits_rebased += 1;
                }
                Err(e) => {
                    result.conflicts.push(self.analyze_conflict(&repo)?);
                }
            }
        }

        if result.conflicts.is_empty() {
            rebase.finish(None)?;
            result.success = true;
        } else {
            rebase.abort()?;
        }

        Ok(result)
    }
}
```

### Cleanup System
```rust
pub struct WorktreeCleanup {
    manager: Arc<WorktreeManager>,
    policy: CleanupPolicy,
}

impl WorktreeCleanup {
    pub async fn run_cleanup_cycle(&self) -> Result<CleanupStats, Error> {
        let mut stats = CleanupStats::default();
        let mut registry = self.manager.registry.write().await;

        // Find worktrees to clean
        let candidates = self.find_cleanup_candidates(&registry);

        for worktree_id in candidates {
            match self.cleanup_worktree(worktree_id).await {
                Ok(()) => {
                    stats.cleaned += 1;
                    registry.remove(worktree_id);
                }
                Err(e) => {
                    stats.errors += 1;
                    tracing::error!("Failed to cleanup worktree {}: {}", worktree_id, e);
                }
            }
        }

        // Run garbage collection if needed
        if stats.cleaned > 0 {
            self.run_git_gc().await?;
        }

        Ok(stats)
    }

    fn find_cleanup_candidates(&self, registry: &WorktreeRegistry) -> Vec<Uuid> {
        let now = Utc::now();
        let mut candidates = Vec::new();

        for (id, worktree) in registry.iter() {
            // Check if idle too long
            if worktree.state == WorktreeState::Idle {
                let idle_duration = now - worktree.last_accessed;
                if idle_duration > self.policy.max_idle {
                    candidates.push(*id);
                    continue;
                }
            }

            // Check if too old
            let age = now - worktree.created_at;
            if age > self.policy.max_age {
                candidates.push(*id);
            }
        }

        // Enforce max worktrees limit
        if registry.len() > self.policy.max_worktrees {
            let mut all_worktrees: Vec<_> = registry.iter().collect();
            all_worktrees.sort_by_key(|(_, w)| w.last_accessed);

            let excess = registry.len() - self.policy.max_worktrees;
            for (id, _) in all_worktrees.iter().take(excess) {
                if !candidates.contains(id) {
                    candidates.push(**id);
                }
            }
        }

        candidates
    }

    async fn cleanup_worktree(&self, worktree_id: Uuid) -> Result<(), Error> {
        // Get worktree info
        let worktree = self.manager.get_worktree(worktree_id).await?;

        // Remove git worktree
        let repo = Repository::open(&self.manager.repo_path)?;
        repo.find_worktree(&worktree_id.to_string())?
            .prune(Some(&mut WorktreePruneOptions::new().flags(
                WorktreePruneFlags::VALID | WorktreePruneFlags::LOCKED
            )))?;

        // Remove directory
        if worktree.path.exists() {
            tokio::fs::remove_dir_all(&worktree.path).await?;
        }

        Ok(())
    }
}
```

### Worktree Pool
```rust
pub struct WorktreePool {
    available: Arc<Mutex<Vec<Worktree>>>,
    max_size: usize,
    reset_strategy: ResetStrategy,
}

impl WorktreePool {
    pub async fn checkout(&self) -> Option<Worktree> {
        self.available.lock().await.pop()
    }

    pub async fn return_worktree(&self, mut worktree: Worktree) -> Result<(), Error> {
        // Reset worktree to clean state
        self.reset_worktree(&mut worktree).await?;

        // Return to pool if under limit
        let mut available = self.available.lock().await;
        if available.len() < self.max_size {
            worktree.state = WorktreeState::Idle;
            available.push(worktree);
        }

        Ok(())
    }

    async fn reset_worktree(&self, worktree: &mut Worktree) -> Result<(), Error> {
        let repo = Repository::open(&worktree.path)?;

        match self.reset_strategy {
            ResetStrategy::Hard => {
                // Hard reset to HEAD
                let head = repo.head()?.peel_to_commit()?;
                repo.reset(&head.as_object(), ResetType::Hard, None)?;
            }
            ResetStrategy::Clean => {
                // Clean untracked files
                // Implementation depends on git2 clean support
            }
        }

        Ok(())
    }
}
```

## Notes

- Worktrees should be cleaned up promptly to avoid disk space issues
- Consider implementing quotas per loop type or user
- Monitor for orphaned worktrees that might accumulate
- Provide tools for manual worktree management and debugging