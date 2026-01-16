# Loop Engine Specification

**Author:** Scott A. Idler
**Date:** 2026-01-15
**Status:** Implementation Spec

---

## Summary

The Loop Engine executes Ralph Wiggum iterations: prompt → LLM → tools → repeat until validation passes. Each iteration starts with a fresh LLM context window. State persists in files and git, not memory.

---

## Iteration Flow

```
┌─────────────────────────────────────────────────────────────────────┐
│                        One Iteration                                 │
│                                                                      │
│  1. Read State ──> 2. Build Prompt ──> 3. Scheduler.wait_for_slot() │
│                                                 │                    │
│                                                 ▼                    │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │ 4. Agentic Tool Loop (within iteration)                      │   │
│  │                                                               │   │
│  │    LLM Call ──> Tool Calls? ──┬── No (EndTurn) ──> Exit loop │   │
│  │        ▲                      │                               │   │
│  │        │                      ▼ Yes                           │   │
│  │        └──── Tool Results <── Execute Tools                   │   │
│  │                                                               │   │
│  │    Guards: max_turns_per_iteration, MaxTokens handling        │   │
│  └──────────────────────────────────────────────────────────────┘   │
│                                                 │                    │
│                                                 ▼                    │
│  5. Scheduler.complete() ──> 6. Run Validation ──> 7. Record Progress│
│                                                 │                    │
│                                                 ▼                    │
│  8. If exit_code == 0: Complete | Else: Next iteration              │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Core Types

```rust
/// Loop configuration (from YAML)
pub struct LoopConfig {
    pub loop_type: String,
    pub prompt_template: String,
    pub validation_command: String,
    pub success_exit_code: i32,        // Usually 0
    pub max_iterations: u32,           // Default: 100
    pub max_turns_per_iteration: u32,  // Default: 50
    pub iteration_timeout_ms: u64,     // Default: 300_000 (5 min)
    pub tools: Vec<String>,
}

/// Iteration outcomes
pub enum IterationResult {
    Complete { iterations: u32 },
    Continue { validation_output: String, exit_code: i32 },
    RateLimited { retry_after: Duration },
    Interrupted { reason: InterruptReason },
    Error { message: String, recoverable: bool },
}

/// Loop status (persisted in LoopExecution)
pub enum LoopStatus {
    Running, Paused, Rebasing, Blocked, Complete, Failed, Stopped,
}
```

---

## The Agentic Tool Loop

Within each iteration, the engine runs an inner loop until `StopReason::EndTurn`:

| Stop Reason | Action |
|-------------|--------|
| `EndTurn` | Exit agentic loop, proceed to validation |
| `ToolUse` | Execute tools, append results, call LLM again |
| `MaxTokens` | Append "continue from where you left off", call LLM again |

**Guards:**
- `max_turns_per_iteration` (default 50) prevents runaway tool loops
- Each turn = one LLM API call within the iteration

---

## Integration Points

| Component | When | Method |
|-----------|------|--------|
| **Scheduler** | Before LLM call | `wait_for_slot(exec_id, priority)` |
| **Scheduler** | After LLM call | `complete(exec_id)` |
| **ProgressStrategy** | After validation | `record(IterationContext)` |
| **ProgressStrategy** | Building prompt | `get_progress()` → `{{progress}}` |
| **ToolContext** | Tool execution | Scopes all ops to worktree |
| **Coordinator** | Between iterations | Poll for Alert/Query/Share/Stop |
| **TaskStore** | State changes | Persist LoopExecution via StateManager |

---

## Coordination Event Handling

Events are polled (non-blocking) between iterations:

| Event | Response |
|-------|----------|
| `main_updated` | Set status=Rebasing, run `git rebase main`, resume or block on conflict |
| `Stop` | Set status=Stopped, exit cleanly |
| `Query` | Reply via coordinator, continue |
| `Share` | Store in context for prompt injection |

---

## Validation

```rust
// Pseudocode - not full impl
async fn run_validation(&self) -> ValidationResult {
    timeout(self.config.iteration_timeout_ms,
        Command::new("sh").arg("-c").arg(&self.config.validation_command)
            .current_dir(&self.worktree)
    ).await
}
```

Validation is **user-defined**. TaskDaemon just runs the command and checks exit code.

---

## State Reading

Each iteration reads fresh state:

| Source | Template Variable |
|--------|-------------------|
| `git status --porcelain` | `{{git-status}}` |
| `git diff HEAD` | `{{git-diff}}` |
| `git log --oneline -10` | `{{git-log}}` |
| ProgressStrategy | `{{progress}}` |

---

## Error Handling

| Error Type | Response |
|------------|----------|
| Rate limit (429) | Extract retry-after, sleep, retry same iteration |
| Timeout | Mark recoverable, retry with backoff |
| Network error | Mark recoverable, retry with backoff |
| Validation timeout | Record in progress, continue iterating |
| Rebase conflict | Set status=Blocked, require manual intervention |
| Max iterations | Set status=Failed, exit with error |

---

## Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Agentic loop within iteration | Continue until EndTurn | Matches Claude's tool-use pattern; one "thought" per iteration |
| Scheduler integration | Wrap only LLM calls | Tools run locally, don't need rate limiting |
| Non-blocking coord polling | `try_recv()` between iterations | Don't block iteration on coordination |
| Fresh context per iteration | No message history carried | Core Ralph Wiggum principle - prevents context rot |

---

## References

- [TaskDaemon Design](./taskdaemon-design.md) - Architecture context
- [Progress Strategy](./progress-strategy.md) - Cross-iteration state
- [Scheduler](./scheduler.md) - Rate limiting
- [Tools](./tools.md) - Tool execution
- [Coordinator Design](./coordinator-design.md) - Inter-loop communication
