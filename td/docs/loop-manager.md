# Loop Manager Specification

**Author:** Scott A. Idler
**Date:** 2026-01-15
**Status:** Implementation Spec

---

## Summary

The Loop Manager is the top-level orchestrator in the TaskDaemon daemon. It spawns loops as tokio tasks, tracks their lifecycle, resolves dependencies, monitors for main branch updates, and handles crash recovery.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                            LoopManager                                   │
│                                                                          │
│  ┌────────────────────────────────────────────────────────────────────┐ │
│  │ Task Registry: HashMap<ExecId, JoinHandle<Result<()>>>             │ │
│  └────────────────────────────────────────────────────────────────────┘ │
│                                                                          │
│  ┌────────────────┐  ┌────────────────┐  ┌──────────────────────────┐  │
│  │ Scheduler Task │  │ MainWatcher    │  │ Recovery Task            │  │
│  │ (10s interval) │  │ (30s interval) │  │ (startup only)           │  │
│  └───────┬────────┘  └───────┬────────┘  └──────────────────────────┘  │
│          │                   │                                          │
│          ▼                   ▼                                          │
│  ┌────────────────────────────────────────────────────────────────────┐ │
│  │ Coordinator (shared Arc)                                           │ │
│  │ - Routes Alert/Query/Share                                         │ │
│  │ - Receives MainWatcher alerts                                      │ │
│  └────────────────────────────────────────────────────────────────────┘ │
│                                                                          │
│  ┌────────────────────────────────────────────────────────────────────┐ │
│  │ StateManager (owns TaskStore, actor pattern)                       │ │
│  └────────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Core Responsibilities

### 1. Task Registry

Tracks all running loops:

```rust
struct LoopManager {
    /// Running loop tasks
    tasks: HashMap<String, JoinHandle<Result<()>>>,

    /// Concurrency limit
    semaphore: Arc<Semaphore>,  // Default: 50 loops

    /// Shared components
    coordinator: Arc<Coordinator>,
    scheduler: Arc<Scheduler>,
    llm: Arc<dyn LlmClient>,
    store_tx: mpsc::Sender<StoreMessage>,
}
```

### 2. Dependency Resolution

Before spawning a loop, verify all dependencies are complete:

| Step | Action |
|------|--------|
| 1 | Query TaskStore for record's `deps` field |
| 2 | Check each dep's status == Complete |
| 3 | If any dep not complete: skip, check again next cycle |
| 4 | If all deps complete: spawn loop |

**Cycle detection** runs once at Plan/Spec creation time (not at spawn time). Uses DFS to detect strongly connected components > 1 node.

### 3. Scheduling Loop

Runs every 10 seconds:

```
Poll TaskStore for:
  - Specs with status=pending AND all deps complete
  - LoopExecutions with status=pending AND all deps complete

For each ready record:
  - Acquire semaphore permit
  - Create worktree (if needed)
  - Spawn LoopEngine as tokio task
  - Register in task registry
  - Update status=running
```

### 4. MainWatcher

Polls git every 30 seconds:

```
current_sha = git rev-parse main
if current_sha != last_known_sha:
    coordinator.alert("main_updated", { commit_sha: current_sha })
    last_known_sha = current_sha
```

All running loops receive this alert and rebase their worktrees.

### 5. Crash Recovery

On daemon startup:

| Step | Action |
|------|--------|
| 1 | Scan TaskStore for LoopExecutions with status in [Running, Rebasing, Paused] |
| 2 | For each: check if worktree still exists |
| 3 | If worktree exists: resume loop from current iteration |
| 4 | If worktree missing: recreate worktree, restart loop |
| 5 | Register recovered loops in task registry |

### 6. Graceful Shutdown

On SIGTERM:

| Step | Action |
|------|--------|
| 1 | Stop accepting new loops |
| 2 | Send Stop to all running loops via Coordinator |
| 3 | Wait for in-progress iterations to complete (with timeout) |
| 4 | Persist final state to TaskStore |
| 5 | Clean up worktrees (optional, configurable) |
| 6 | Exit |

---

## Spawning a Loop

```rust
async fn spawn_loop(&mut self, exec: LoopExecution) -> Result<()> {
    // 1. Acquire concurrency permit
    let permit = self.semaphore.clone().acquire_owned().await?;

    // 2. Create/verify worktree
    let worktree = self.ensure_worktree(&exec).await?;

    // 3. Load loop config
    let config = self.load_loop_config(&exec.loop_type)?;

    // 4. Create coordinator channel for this loop
    let (coord_tx, coord_rx) = mpsc::channel(32);
    self.coordinator.register(&exec.id, coord_tx).await;

    // 5. Build and spawn engine
    let engine = LoopEngine::new(
        exec.id.clone(),
        config,
        self.llm.clone(),
        self.scheduler.clone(),
        self.coordinator.clone(),
        coord_rx,
        self.store_tx.clone(),
        worktree,
    );

    let handle = tokio::spawn(async move {
        let result = engine.run().await;
        drop(permit);  // Release semaphore on completion
        result
    });

    // 6. Register in task registry
    self.tasks.insert(exec.id, handle);

    Ok(())
}
```

---

## Worktree Management

| Operation | Command |
|-----------|---------|
| Create | `git worktree add /tmp/taskdaemon/worktrees/{exec-id} -b {branch-name}` |
| Rebase | `git rebase main` (in worktree) |
| Cleanup | `git worktree remove {path}` |

**Branch naming:** `taskdaemon/{exec-id}` (e.g., `taskdaemon/019432-spec-oauth-endpoints`)

**Worktree location:** `/tmp/taskdaemon/worktrees/` (configurable)

---

## Task Lifecycle

```
                    ┌─────────┐
                    │ pending │
                    └────┬────┘
                         │ deps satisfied
                         ▼
                    ┌─────────┐
        ┌───────────│ running │───────────┐
        │           └────┬────┘           │
        │ main_updated   │                │ stop requested
        ▼                │                ▼
   ┌──────────┐          │           ┌─────────┐
   │ rebasing │──────────┤           │ stopped │
   └──────────┘          │           └─────────┘
        │ conflict       │
        ▼                │
   ┌─────────┐           │
   │ blocked │           │
   └─────────┘           │
                         │ validation passes
                         ▼
                   ┌──────────┐
                   │ complete │
                   └──────────┘
```

---

## Concurrency Limits

| Resource | Default | Config Key |
|----------|---------|------------|
| Max concurrent loops | 50 | `concurrency.max-loops` |
| Max concurrent API calls | 10 | `concurrency.max-api-calls` |
| Max worktrees | 50 | `concurrency.max-worktrees` |

The loop semaphore (50) is separate from the scheduler semaphore (10). A loop can be "running" (doing file I/O, waiting for tools) without using an API slot.

---

## Error Handling

| Scenario | Response |
|----------|----------|
| Loop task panics | Log error, remove from registry, mark status=Failed |
| Worktree creation fails | Log error, skip this loop, retry next cycle |
| TaskStore unavailable | Retry with backoff, alert operator after 3 failures |
| All API slots exhausted | Loops queue in scheduler, no action needed |

---

## Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Polling interval | 10s for scheduler, 30s for MainWatcher | Balance responsiveness vs overhead |
| Recovery strategy | Resume from last iteration | Don't lose progress; iteration is idempotent |
| Worktree location | `/tmp/` | Ephemeral; doesn't pollute main repo |
| Shutdown timeout | 60s | Allow current iteration to finish cleanly |

---

## References

- [TaskDaemon Design](./taskdaemon-design.md) - Overall architecture
- [Loop Engine](./loop-engine.md) - What gets spawned
- [Coordinator Design](./coordinator-design.md) - Alert routing
- [Scheduler](./scheduler.md) - API rate limiting
