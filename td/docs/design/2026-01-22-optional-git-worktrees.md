# Design Document: Optional Git Worktrees

**Author:** Claude (with Scott)
**Date:** 2026-01-22
**Status:** Ready for Review
**Review Passes Completed:** 5/5

## Summary

TaskDaemon currently **requires** being started from within a git repository. This is because concurrent loop executions use git worktrees for file isolation—each loop gets its own branch so edits don't collide.

This design makes git **optional**:
- Daemon starts in any directory
- Single loops run without git (SimpleMode)
- When the user requests a second concurrent loop, we prompt: "Initialize git for isolation?"
- Optionally, we remove `.git` after all work completes (temporary git)

## Problem Statement

### Background

TaskDaemon uses git worktrees to isolate concurrent loop executions. When multiple loops run simultaneously, each gets its own worktree branch so changes don't conflict. This works well for code-centric workflows but creates friction for:

- Non-code tasks (research, writing, analysis)
- Single-loop execution (no isolation needed)
- Quick one-off tasks in arbitrary directories
- Users unfamiliar with git

Currently, the daemon fails at startup if not in a git repository:

```rust
if !repo_root.join(".git").exists() {
    return Err(eyre::eyre!(
        "Not a git repository: {}. TaskDaemon requires a git repo.",
        repo_root.display()
    ));
}
```

### Problem

The hard git requirement blocks valid use cases and creates poor UX:

1. User runs `td daemon start` in `~/Documents` → immediate failure
2. User wants to run a simple summarization task → must create git repo first
3. User doesn't know git → blocked entirely

### Goals

- Daemon starts successfully in any directory
- Single-loop execution works without git
- Concurrent loops still get proper isolation
- Just-in-time prompting when git is needed but missing
- Option for temporary git that cleans up after work completes

### Non-Goals

- Replacing git worktrees with a different isolation mechanism
- Supporting concurrent file modifications without isolation
- Auto-detecting optimal mode (user explicitly chooses)

## Proposed Solution

### Overview

Introduce two execution modes:

| Mode | Git Required | Concurrent Loops | Use Case |
|------|--------------|------------------|----------|
| **Simple** | No | No (sequential only) | Non-code tasks, quick work |
| **Isolated** | Yes | Yes (worktree per loop) | Code changes, parallel work |

The daemon auto-detects mode based on git availability, but prompts when the user's request requires a mode upgrade.

### Architecture

**Startup Flow:**

```
td daemon start
       │
       ▼
┌──────────────────┐     ┌──────────────────┐
│  .git exists?    │─Yes─▶│  IsolatedMode    │
│                  │      │  (full features) │
└────────┬─────────┘      └──────────────────┘
         │No
         ▼
┌──────────────────┐
│   SimpleMode     │
│ (single loop,    │
│  no worktrees)   │
└──────────────────┘
```

**Concurrent Loop Request Flow (SimpleMode):**

```
User activates 2nd loop
         │
         ▼
┌──────────────────────────────────────────┐
│  Already a loop running?                  │
│  Yes → Return GitRequired error           │
└──────────────────────────────────────────┘
         │
         ▼
┌──────────────────────────────────────────┐
│  TUI shows prompt:                        │
│  ┌────────────────────────────────────┐  │
│  │ Concurrent loops need git.         │  │
│  │                                    │  │
│  │ [Init git (temp)] [Init git]       │  │
│  │ [Run sequentially] [Cancel]        │  │
│  └────────────────────────────────────┘  │
└──────────────────────────────────────────┘
         │
         ▼ (user chooses "Init git (temp)")
┌──────────────────────────────────────────┐
│  1. git init                              │
│  2. Upgrade mode to Isolated              │
│  3. Save cleanup tracker to disk          │
│  4. Retry loop activation                 │
└──────────────────────────────────────────┘
```

**Cleanup Flow:**

```
Last running loop completes
         │
         ▼
┌──────────────────────────────────────────┐
│  Cleanup tracker exists?                  │
│  No → Done                                │
│  Yes, temporary=true → Prompt for cleanup │
└──────────────────────────────────────────┘
         │
         ▼
┌──────────────────────────────────────────┐
│  TUI shows prompt:                        │
│  "Remove .git? [Yes] [No, keep it]"       │
└──────────────────────────────────────────┘
         │
         ▼ (user confirms)
┌──────────────────────────────────────────┐
│  rm -rf .git                              │
│  Delete cleanup tracker                   │
│  Downgrade to SimpleMode                  │
└──────────────────────────────────────────┘
```

### Data Model

**New fields in LoopManagerConfig:**

```rust
pub struct LoopManagerConfig {
    // Existing fields...

    /// Current execution mode (detected at startup, can upgrade)
    pub mode: ExecutionMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// No git, single loop at a time, runs in current directory
    Simple,
    /// Git available, concurrent loops with worktree isolation
    Isolated,
}
```

**New struct for tracking temporary git:**

```rust
/// Tracks git directories initialized by the daemon for cleanup
pub struct GitCleanupTracker {
    /// Directory where we ran `git init`
    pub path: PathBuf,
    /// If true, prompt for removal when all work completes
    pub temporary: bool,
}
```

This is persisted to `{store_path}/.git_cleanup_tracker.json` so it survives daemon restarts.

**SimpleMode execution:**

In SimpleMode, loops run directly in the current working directory without worktrees:

```rust
// In spawn_loop():
let working_dir = if self.config.mode == ExecutionMode::Simple {
    self.config.repo_root.clone()  // Just use cwd
} else {
    self.worktree_manager.create(&exec.id).await?.path
};
```

**Concurrency detection:**

"Concurrent loop requested" is detected in `spawn_loop()` when:
1. Current mode is `Simple`
2. There's already a loop with status `Running`
3. User is trying to start/activate another loop

### API Design

**New IPC messages for prompting:**

```rust
/// Messages requiring user response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DaemonPrompt {
    /// Git required for concurrent execution
    GitRequired {
        reason: String,
        options: Vec<GitInitOption>,
    },
    /// Confirm cleanup of temporary git
    ConfirmGitCleanup {
        path: PathBuf,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GitInitOption {
    InitTemporary,  // Init git, cleanup when done
    InitPermanent,  // Init git, keep it
    RunSequential,  // Don't init, run one at a time
    Cancel,         // Abort the operation
}

/// User's response to a prompt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PromptResponse {
    GitInit { temporary: bool },
    RunSequential,
    Cancel,
    ConfirmCleanup { confirmed: bool },
}
```

**Prompt delivery mechanism:**

The daemon cannot push prompts to the TUI (IPC is TUI→daemon). Instead:

1. When concurrent execution is blocked, daemon returns an error with structured data:
   ```rust
   pub enum SpawnError {
       // Existing variants...

       /// Concurrent execution requires git, user must choose
       GitRequired {
           reason: String,
           options: Vec<GitInitOption>,
       },
   }
   ```

2. The TUI (via StateManager) receives this error when calling `activate_draft()` or `start_draft()`

3. TUI displays modal dialog with options

4. User's choice triggers a new call:
   ```rust
   // New StateManager method
   pub async fn upgrade_to_isolated_mode(&self, init_option: GitInitOption) -> Result<()>
   ```

5. After upgrade, TUI retries the original operation

**Alternative: Event-based prompting**

Use the existing `StateEvent` broadcast channel:
```rust
pub enum StateEvent {
    // Existing variants...

    /// Daemon needs user decision
    PromptRequired {
        prompt_id: String,
        prompt: DaemonPrompt,
    },
}
```

TUI subscribes to events, shows prompt, responds via new IPC message.

### Implementation Plan

**Phase 1: Remove Startup Requirement**
- Remove git check from `run_daemon()`
- Add `ExecutionMode` detection based on `.git` presence
- Make `MainWatcher` optional (only start in Isolated mode)
- Store mode in daemon state

**Phase 2: SimpleMode Execution**
- Modify `spawn_loop()` to skip worktree creation in SimpleMode
- Run loop engine directly in cwd
- Block concurrent execution in SimpleMode (queue instead)

**Phase 3: Mode Upgrade Prompting**
- Add `DaemonPrompt` and `PromptResponse` IPC messages
- Detect when concurrent loop requested in SimpleMode
- Send prompt to TUI, wait for response
- Implement `git init` on confirmation

**Phase 4: Temporary Git Cleanup**
- Persist `GitCleanupTracker` to disk
- "All work complete" = no executions with status Running, Pending, or Paused
- On completion, emit `StateEvent::PromptRequired` for cleanup confirmation
- TUI shows prompt, user confirms, daemon removes `.git`
- If daemon restarts, check tracker file and resume cleanup flow

**Phase 5: Testing & Edge Cases**
- Test SimpleMode execution
- Test mode upgrade flow
- Test cleanup flow
- Test crash recovery (don't lose `.git` unexpectedly)

## Alternatives Considered

### Alternative 1: Always Use Temp Directories (No Git)
- **Description:** Replace worktrees with plain temp directories, copy files in/out
- **Pros:** No git dependency at all
- **Cons:** Loses git history, merge conflicts harder to resolve, file copying is slow for large repos
- **Why not chosen:** Git worktrees are genuinely better for code isolation

### Alternative 2: Require Git, Improve Error Message
- **Description:** Keep current behavior, just explain better
- **Pros:** Simpler, no new code paths
- **Cons:** Still blocks valid non-code use cases
- **Why not chosen:** Doesn't solve the core UX problem

### Alternative 3: Auto-Init Git Without Prompting
- **Description:** If no git and concurrent needed, just `git init` automatically
- **Pros:** Seamless UX
- **Cons:** Surprising side effect, user might not want git in that directory
- **Why not chosen:** Violates principle of least surprise

### Alternative 4: Separate "Light Mode" Binary
- **Description:** Ship `td-lite` that never uses git
- **Pros:** Clear separation of concerns
- **Cons:** Maintenance burden, user confusion about which to use
- **Why not chosen:** Overcomplicates the product

## Technical Considerations

### Dependencies

- No new external dependencies
- Internal: IPC module (already exists), config module

### Performance

- SimpleMode is actually faster (no worktree creation overhead)
- Mode detection is O(1) - just check for `.git` directory
- No performance regression for existing Isolated mode users

### Security

- `git init` creates a `.git` directory - user must consent
- Cleanup removes `.git` - requires confirmation to prevent data loss
- No new attack surfaces

### Testing Strategy

| Test | Description |
|------|-------------|
| `test_daemon_starts_without_git` | Daemon starts in non-git directory |
| `test_simple_mode_single_loop` | Single loop executes in SimpleMode |
| `test_simple_mode_blocks_concurrent` | Second loop queues in SimpleMode |
| `test_mode_upgrade_prompt` | Prompt appears when concurrent requested |
| `test_git_init_temporary` | Git initialized and tracked for cleanup |
| `test_git_init_permanent` | Git initialized without cleanup flag |
| `test_cleanup_on_completion` | `.git` removed after all loops done |
| `test_cleanup_prompt` | User must confirm cleanup |
| `test_crash_preserves_git` | Unexpected exit doesn't delete `.git` |

### Rollout Plan

1. Feature-flag the changes (`--experimental-simple-mode`)
2. Test with internal users
3. Remove flag, make it default behavior
4. Update documentation

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| User accidentally deletes wanted `.git` | Low | High | Require explicit confirmation; check for uncommitted changes first |
| Crash during git init leaves partial state | Low | Medium | Git init is atomic, worst case is empty `.git` |
| SimpleMode loops modify same files | Medium | Medium | Queue concurrent requests, don't run in parallel |
| User confused about modes | Medium | Low | Clear status display in TUI showing current mode |
| MainWatcher errors in SimpleMode | Low | Low | Simply don't start MainWatcher in SimpleMode |
| User expects parallelism but gets sequential | Medium | Medium | Prompt explicitly states "run one at a time"; TUI shows queue |
| Cleanup tracker file gets corrupted | Low | Low | JSON parse error → delete tracker, assume no cleanup needed |
| Git init in directory with existing content | Low | Medium | Not a problem—git init works fine, we don't auto-add/commit |

## Edge Cases

### User deletes `.git` while daemon is in Isolated mode
- **Detection:** Next worktree operation fails with git error
- **Handling:** Catch error, log warning, downgrade to SimpleMode, emit event to TUI
- **Recovery:** User can re-init git or continue in SimpleMode

### Daemon crashes with temporary git flag set
- **Detection:** On restart, check for cleanup tracker file
- **Handling:** If tracker exists and temporary=true, check if any loops are active
- **Recovery:** If no active loops, prompt for cleanup; otherwise resume normally

### User runs `td run` (batch mode, no TUI)
- **Detection:** No TUI connected to receive prompts
- **Handling:** For batch mode, fail with clear error message explaining the situation
- **Alternative:** Add `--init-git` flag to `td run` for explicit opt-in

### Two users run daemon in same directory
- **Detection:** Second daemon fails to start (PID file exists)
- **Handling:** Existing behavior, no change needed

### Worktree creation fails after git init (disk full, permissions)
- **Detection:** WorktreeManager.create() returns error
- **Handling:** Return error to user, keep git initialized (don't auto-cleanup on partial failure)
- **Recovery:** User fixes issue, retries

### User chooses "Run sequentially" then starts many loops
- **Behavior:** Loops queue up, run one at a time
- **Concern:** Could be confusing if user expects parallelism
- **Mitigation:** TUI shows clear indicator "Sequential mode - 3 loops queued"

## Open Questions

- [ ] Should SimpleMode loops still create a temp directory copy for safety, even without git?
  - **Leaning no:** Adds complexity, most non-code tasks don't need it
- [ ] What's the UX for `td run --init-git`? Auto-cleanup after run completes?
  - **Suggestion:** `td run --init-git=temp` (cleanup) vs `td run --init-git` (keep)
- [ ] Should we warn if user has uncommitted changes when we're about to remove `.git`?
  - **Leaning yes:** `git status` check before cleanup, warn if dirty

## Appendix: Mode Indicator in TUI

The TUI status bar should show current mode:

```
┌─ TaskDaemon ──────────────────────────────────────────────┐
│ Status: Running │ Mode: Simple (sequential) │ Loops: 1   │
└──────────────────────────────────────────────────────────-┘
```

Or in Isolated mode:

```
┌─ TaskDaemon ──────────────────────────────────────────────┐
│ Status: Running │ Mode: Isolated (git) │ Loops: 3        │
└──────────────────────────────────────────────────────────-┘
```

This helps users understand why behavior differs between directories.

## References

- Current git validation: `td/src/main.rs:816-824`
- WorktreeManager: `td/src/worktree/manager.rs`
- MainWatcher: `td/src/watcher/main_watcher.rs`
- IPC module: `td/src/ipc/`
- LoopManagerConfig: `td/src/loop/manager.rs:28-47`
