# Design Document: Iteration Artifact Storage

**Author:** Claude (with Scott)
**Date:** 2026-01-21
**Status:** Ready for Review
**Review Passes Completed:** 5/5

## Summary

TaskDaemon's TUI Output and Logs views are empty because per-iteration execution data is captured but immediately discarded after truncation. This document proposes a new `IterationLog` domain model and storage integration to persist full iteration history, enabling debugging, performance analysis, and richer TUI displays.

## Problem Statement

### Background

TaskDaemon executes "Ralph Wiggum" loops that iteratively run an LLM agent, apply changes, and validate with a command (typically `otto ci`). Each iteration produces valuable debugging information:
- Validation stdout/stderr
- Exit codes and duration
- Files changed
- LLM token usage
- Tool call history

Currently, this data flows through the system but is aggressively truncated before persistence.

### Current Data Flow

```
Subprocess output (stdout/stderr)
    ↓
ValidationResult (full capture - stdout, stderr, exit_code, duration_ms)
    ↓
IterationContext (full capture passed to progress strategy)
    ↓
SystemCapturedProgress::record()
    ↓ [TRUNCATES: 500 chars/iteration, 5 iterations max]
    ↓
LoopExecution.progress field (accumulated text)
    ↓
[DEAD END - original data discarded]
```

### What's Currently Stored in `LoopExecution`

| Field | Purpose | Limitation |
|-------|---------|------------|
| `progress` | Accumulated iteration summaries | Truncated to 500 chars × 5 = 2.5 KB total |
| `iteration` | Current iteration counter | Just a number, no history |
| `last_error` | Most recent error | Only latest; history lost |
| `context` | Template parameters (JSON) | Not iteration-specific |

### What's NOT Stored

- Individual iteration stdout/stderr (full, untruncated)
- Multiple error history across iterations
- Per-iteration metrics (duration, tokens, cost)
- Tool call sequences per iteration
- Explicit artifact file paths (plan.md, spec.md)
- Artifact validation status (draft/complete/failed)

### Problem

Users cannot debug failed loops because:
1. The `[o] Output` view has no data source
2. The `[L] Logs` view has no structured iteration records
3. Only 2.5 KB of truncated progress exists for potentially hundreds of KB of output
4. No way to query "show me iteration 3's stderr" or "list all failed iterations"

### Goals

- Persist full, untruncated iteration output for debugging
- Enable TUI views to display rich iteration history
- Track aggregate metrics (total tokens, total duration) on executions
- Store explicit artifact paths for quick navigation
- Support real-time streaming to TUI as iterations complete

### Non-Goals

- Storing raw LLM conversation transcripts (too large, different concern)
- Replacing the truncated `progress` field (still needed for prompt context)
- Historical migration (existing executions will simply have no iteration logs)
- Cost tracking (token counts enable this but cost calculation is out of scope)

## Proposed Solution

### Overview

Introduce a new `IterationLog` domain model that stores the complete record of each iteration's execution. This creates a one-to-many relationship: one `LoopExecution` has many `IterationLog` records.

### Architecture

```
┌─────────────────┐       ┌─────────────────┐
│  LoopExecution  │──1:N──│  IterationLog   │
│                 │       │                 │
│ - id            │       │ - id            │
│ - progress      │       │ - execution_id  │
│ - iteration     │       │ - iteration     │
│ - artifact_path │       │ - stdout (full) │
│ - total_tokens  │       │ - stderr (full) │
└─────────────────┘       │ - exit_code     │
         │                │ - duration_ms   │
         │                │ - tool_calls    │
         │                └─────────────────┘
         ↓
┌─────────────────┐
│   StateManager  │
│                 │
│ - Commands:     │
│   CreateIterLog │
│   ListIterLogs  │
│                 │
│ - Events:       │
│   IterLogCreate │
└─────────────────┘
         │
         ↓
┌─────────────────┐
│    TaskStore    │
│                 │
│ - SQLite index  │
│ - JSONL persist │
└─────────────────┘
```

### Data Model

#### New: `IterationLog`

```rust
/// Persistent record of a single loop iteration's execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationLog {
    /// Unique ID: {execution_id}-iter-{N}
    pub id: String,

    /// Parent LoopExecution ID (indexed for queries)
    pub execution_id: String,

    /// Iteration number (1-indexed for display; LoopExecution.iteration is 0-indexed at creation,
    /// incremented before each iteration runs)
    pub iteration: u32,

    /// Validation command that was run (e.g., "otto ci")
    pub validation_command: String,

    /// Exit code from validation (0 = success)
    pub exit_code: i32,

    /// Full validation stdout (NO truncation for storage)
    pub stdout: String,

    /// Full validation stderr (NO truncation for storage)
    pub stderr: String,

    /// Validation duration in milliseconds
    pub duration_ms: u64,

    /// Files changed during this iteration (from git status)
    pub files_changed: Vec<String>,

    /// LLM input tokens consumed in this iteration
    pub llm_input_tokens: Option<u64>,

    /// LLM output tokens generated in this iteration
    pub llm_output_tokens: Option<u64>,

    /// Summary of tool calls made during agentic loop
    pub tool_calls: Vec<ToolCallSummary>,

    /// Creation timestamp (milliseconds since Unix epoch)
    pub created_at: i64,

    /// Last update timestamp
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallSummary {
    /// Tool name (e.g., "Edit", "Bash", "Read")
    pub tool_name: String,
    /// First N chars of arguments for display
    pub arguments_summary: String,
    /// First N chars of result for display
    pub result_summary: String,
    /// Whether tool call resulted in error
    pub is_error: bool,
}
```

**Collection:** `iteration_logs` (stored in TaskStore as JSONL + SQLite indexes)

**Indexed Fields:**
- `execution_id` (String) — filter logs by parent execution
- `iteration` (Integer) — order and lookup specific iterations
- `exit_code` (Integer) — filter failed iterations (non-zero)

#### Enhanced: `LoopExecution` (Additional Fields)

```rust
pub struct LoopExecution {
    // ... existing fields unchanged ...

    // NEW: Explicit artifact tracking
    /// Path to primary artifact (e.g., ".taskdaemon/plans/{id}/plan.md")
    pub artifact_path: Option<String>,

    /// Artifact validation status: "draft" | "complete" | "failed"
    pub artifact_status: Option<String>,

    // NEW: Aggregate metrics across all iterations
    /// Total LLM input tokens consumed
    pub total_input_tokens: u64,

    /// Total LLM output tokens generated
    pub total_output_tokens: u64,

    /// Total validation execution time in milliseconds
    pub total_duration_ms: u64,
}
```

### API Design

#### New StateManager Commands

Commands follow the existing pattern: each command includes a `reply: tokio::sync::oneshot::Sender<StateResponse<T>>` for async responses.

```rust
pub enum StateCommand {
    // ... existing commands ...

    /// Create a new iteration log record
    CreateIterationLog {
        log: IterationLog,
        reply: oneshot::Sender<StateResponse<String>>,
    },

    /// List iteration logs for an execution
    /// Results are ordered by iteration number ascending (oldest first)
    ListIterationLogs {
        execution_id: String,
        reply: oneshot::Sender<StateResponse<Vec<IterationLog>>>,
    },

    /// Get a specific iteration log by ID
    GetIterationLog {
        id: String,
        reply: oneshot::Sender<StateResponse<Option<IterationLog>>>,
    },

    /// Delete iteration logs for an execution (for cleanup)
    DeleteIterationLogs {
        execution_id: String,
        reply: oneshot::Sender<StateResponse<usize>>,  // Returns count deleted
    },
}
```

Corresponding convenience methods on `StateManager`:

```rust
impl StateManager {
    pub async fn create_iteration_log(&self, log: IterationLog) -> StateResponse<String>;
    pub async fn list_iteration_logs(&self, execution_id: &str) -> StateResponse<Vec<IterationLog>>;
    pub async fn get_iteration_log(&self, id: &str) -> StateResponse<Option<IterationLog>>;
    pub async fn delete_iteration_logs(&self, execution_id: &str) -> StateResponse<usize>;
}
```

#### New StateManager Events

```rust
pub enum StateEvent {
    // ... existing events ...

    /// Emitted when a new iteration log is persisted
    IterationLogCreated {
        execution_id: String,
        iteration: u32,
        exit_code: i32,
    },
}
```

### Implementation Plan

#### Phase 1: IterationLog Domain Model

**Files:**
- `td/src/domain/iteration_log.rs` (new)
- `td/src/domain/mod.rs` (add export)

**Tasks:**
1. Create `IterationLog` struct with all fields
2. Implement `Record` trait:
   - `id()` → `&self.id`
   - `updated_at()` → `self.updated_at`
   - `collection_name()` → `"iteration_logs"`
   - `indexed_fields()` → execution_id, iteration, exit_code
3. Create `ToolCallSummary` struct

#### Phase 2: StateManager Integration

**Files:**
- `td/src/state/manager.rs`
- `td/src/state/messages.rs` (contains StateCommand, StateError, StateResponse)

**Tasks:**
1. Add `CreateIterationLog`, `ListIterationLogs`, `GetIterationLog`, `DeleteIterationLogs` to `StateCommand`
2. Implement command handlers delegating to Store
3. Add `StateEvent::IterationLogCreated` variant
4. Broadcast event after successful creation
5. Register `iteration_logs` collection for index rebuilding on startup (in `spawn()`)
6. **Cascade delete**: Modify `DeleteExecution` handler to first delete associated IterationLogs:
   ```rust
   StateCommand::DeleteExecution { id, reply } => {
       // Delete associated iteration logs first
       let _ = store.delete_by_index::<IterationLog>(
           "execution_id",
           IndexValue::String(id.clone())
       );
       // Then delete the execution
       let result = store.delete::<LoopExecution>(&id)...;
   }
   ```

#### Phase 3: Engine Integration

**Files:**
- `td/src/loop/engine.rs`

**Tasks:**
1. Add `tool_call_buffer: Vec<ToolCallSummary>` field to `LoopEngine`
2. Hook tool executor to call `record_tool_call()` after each tool completes
3. After `run_validation()` returns (success OR error), before progress truncation:
   - Capture tool call summaries from buffer
   - Create `IterationLog` with full stdout/stderr
   - Send `CreateIterationLog` command to StateManager
4. **Handle validation errors**: If `run_validation()` returns `Err(e)`:
   - Create IterationLog with `exit_code = -1`, `stdout = ""`, `stderr = e.to_string()`
   - Still record duration and tool calls
5. Track LLM token usage from API responses (plumb through from agentic loop)
6. Update `LoopExecution` aggregate fields after each iteration

**Key code change in `run_iteration()`:**

The engine needs to:
1. Access LLM token usage from the API response (already captured but not persisted)
2. Track tool calls made during the agentic loop
3. Create the IterationLog before truncating progress

```rust
// After validation completes, BEFORE progress.record()

// Token usage comes from the LLM response - need to accumulate across
// all API calls in this iteration. The agentic loop already returns this
// in the response struct; we just need to plumb it through.
let llm_usage = self.get_iteration_token_usage();

// Tool calls are tracked during the agentic loop. Add a Vec<ToolCallSummary>
// field to LoopEngine that gets populated on each tool call and cleared
// at iteration start.
let tool_summaries = std::mem::take(&mut self.tool_call_buffer);

let iteration_log = IterationLog {
    id: format!("{}-iter-{}", self.exec_id, self.iteration),
    execution_id: self.exec_id.clone(),
    iteration: self.iteration,
    validation_command: self.config.validation_command.clone(),
    exit_code: validation.exit_code,
    stdout: validation.stdout.clone(),  // Full, not truncated
    stderr: validation.stderr.clone(),  // Full, not truncated
    duration_ms: validation.duration_ms,
    files_changed: files_changed.clone(),
    llm_input_tokens: llm_usage.map(|u| u.input_tokens),
    llm_output_tokens: llm_usage.map(|u| u.output_tokens),
    tool_calls: tool_summaries,
    created_at: now_ms(),
    updated_at: now_ms(),
};

self.state_manager.create_iteration_log(iteration_log).await?;

// Then continue with existing truncated progress recording
self.progress.record(&iter_ctx);
```

**New helper method on LoopEngine:**

```rust
impl LoopEngine {
    /// Record a tool call for the current iteration (called from tool executor)
    fn record_tool_call(&mut self, name: &str, args: &str, result: &str, is_error: bool) {
        const SUMMARY_LEN: usize = 200;
        self.tool_call_buffer.push(ToolCallSummary {
            tool_name: name.to_string(),
            arguments_summary: args.chars().take(SUMMARY_LEN).collect(),
            result_summary: result.chars().take(SUMMARY_LEN).collect(),
            is_error,
        });
    }
}
```

#### Phase 4: Artifact Tracking

**Files:**
- `td/src/domain/execution.rs`
- `td/src/loop/engine.rs`

**Tasks:**
1. Add `artifact_path`, `artifact_status`, `total_input_tokens`, `total_output_tokens`, `total_duration_ms` fields to `LoopExecution`
2. Set `artifact_path` when creating execution based on loop type:
   - Plan loops → `.taskdaemon/plans/{id}/plan.md`
   - Spec loops → `.taskdaemon/specs/{id}/spec.md`
   - Phase loops → determined by parent spec
3. Update `artifact_status`:
   - "draft" on creation
   - "complete" when validation passes (exit_code == 0)
   - "failed" when max_iterations exceeded without success
4. Accumulate token counts and duration after each iteration

#### Phase 5: TUI Integration

**Files:**
- `td/src/tui/views.rs`
- `td/src/tui/state.rs`
- `td/src/tui/runner.rs`

**Tasks:**

**Output View (`[o]`):**
1. Query `ListIterationLogs { execution_id }` when view activates
2. Render iteration list with expandable sections:
   - Header: `## Iteration {N} — Exit {code} — {duration_ms}ms`
   - Collapsed: first 200 chars of stdout or "[no output]"
   - Expanded: full stdout, then stderr if present
3. Support scrolling and expansion toggle

**Logs View (`[L]`):**

Currently `state.logs` in the TUI is populated from a separate source. Replace this with IterationLog queries:

1. Query `list_iteration_logs(execution_id)` when view activates or execution changes
2. Store results in `AppState.iteration_logs: Vec<IterationLog>`
3. Chronological list (iteration order, since timestamps are creation order)
4. Color-code by exit code (green=0, red=non-zero, yellow for signals like -1)
5. Show: `[{iteration}] {validation_command} — {exit_code} — {duration_ms}ms — {files_changed.len()} files`
6. Expandable to show tool calls (collapsible list) and full stdout/stderr

**Real-time updates:**

The TUI runner already subscribes to `StateEvent` via `state_manager.subscribe_events()`.

1. Add handler for `StateEvent::IterationLogCreated { execution_id, iteration, exit_code }`
2. If the TUI is currently viewing that execution (check `app_state.selected_execution`):
   - Fetch the new IterationLog: `get_iteration_log(&format!("{}-iter-{}", execution_id, iteration))`
   - Append to `app_state.iteration_logs`
   - If Output or Logs view is active, trigger re-render
3. Auto-scroll to bottom when user has "follow mode" enabled (new toggle, default on for Running executions)

**Describe View enhancements:**
1. Display `artifact_path` with clickable path
2. Display `artifact_status` with color coding
3. Show aggregate metrics: total iterations, total tokens, total duration

#### Phase 6: Cleanup Command (Future/Optional)

**Files:**
- `td/src/commands/clean.rs` (new)

**Tasks:**
1. Add `td clean` subcommand
2. Options: `--keep-days N`, `--keep-last N`, `--dry-run`
3. Delete old `IterationLog` entries based on retention policy
4. Optionally cascade to delete orphaned `LoopExecution` entries
5. Clean up orphaned artifacts in `.taskdaemon/` directory

## Alternatives Considered

### Alternative 1: Store Output in Filesystem Only

**Description:** Write iteration output to files like `.taskdaemon/logs/{exec_id}/iter-{N}.log` instead of JSONL/SQLite.

**Pros:**
- Simple implementation
- Easy to inspect manually
- No JSONL size concerns

**Cons:**
- Lose query capability (can't filter by exit_code, list failed iterations)
- No indexing for fast lookups
- Inconsistent with existing Record/Store pattern
- Harder to clean up (orphaned files)

**Why not chosen:** TaskStore's hybrid model already handles this well. Querying and indexing are essential for TUI functionality.

### Alternative 2: Embed Iteration Logs in LoopExecution

**Description:** Add `iterations: Vec<IterationLog>` field directly to `LoopExecution` instead of separate collection.

**Pros:**
- Single record to fetch
- Atomic updates

**Cons:**
- Unbounded growth of single JSONL record
- Can't query iterations independently
- Poor performance for executions with many iterations
- Breaks normalization patterns

**Why not chosen:** Separate collection enables efficient queries and prevents unbounded record growth.

### Alternative 3: Streaming Log Files

**Description:** Append to a single log file per execution as iterations run, similar to traditional logging.

**Pros:**
- Very simple
- Good for streaming tails

**Cons:**
- No structured data (can't parse exit codes, durations)
- Harder to implement TUI iteration list
- No indexing

**Why not chosen:** Structured data is essential for the planned TUI features.

## Technical Considerations

### Dependencies

**Internal:**
- TaskStore (`ts/src/store.rs`) — no changes needed, just new collection
- StateManager — extended with new commands/events
- LoopEngine — creates IterationLog records
- TUI — queries and displays IterationLog data

**External:**
- None; builds entirely on existing infrastructure

### Performance

**Storage growth:**
- Typical validation output: 1-50 KB
- 10 iterations × 25 KB average = 250 KB per execution
- 100 executions = 25 MB (acceptable for local development tool)

**Query performance:**
- SQLite indexes on execution_id, iteration, exit_code
- `ListIterationLogs` is O(log n) index lookup + O(k) record fetch
- TUI queries single execution's logs; no full table scans

**Memory:**
- TUI loads iteration logs on-demand, not all at startup
- Large stdout/stderr only loaded when view expanded

### Security

No security implications. All data is local to the user's machine, same as existing execution data.

### Testing Strategy

**Unit tests:**
- `IterationLog` serialization/deserialization
- `Record` trait implementation
- Index field extraction

**Integration tests:**
- StateManager command handling for iteration logs
- LoopEngine creates IterationLog after validation
- Multiple iterations accumulate correctly

**TUI tests:**
- Output view renders iteration list
- Logs view filters by exit code
- Real-time updates append correctly

### Rollout Plan

1. **Phase 1-2:** Add domain model and StateManager support (invisible to users)
2. **Phase 3:** Engine integration — new executions start producing logs
3. **Phase 4:** Artifact tracking — executions show paths/status
4. **Phase 5:** TUI integration — views become functional
5. **Phase 6:** Cleanup command — users can manage storage

Backward compatible at each phase. Existing executions continue working; they just won't have iteration logs.

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Large outputs bloat storage | Medium | Low | Phase 6 cleanup command; future: compress or external files for >100KB |
| JSONL sync performance with many logs | Low | Medium | SQLite indexes; pagination for TUI; lazy loading |
| Tool call tracking adds complexity | Low | Low | Make tool_calls optional; can be empty initially |
| Breaking existing executions | Low | High | All new fields are Option or have sensible defaults; no migration needed |
| Orphaned IterationLogs on execution delete | Medium | Low | Delete cascade: when deleting LoopExecution, also delete its IterationLogs |
| Binary output in stdout/stderr | Low | Low | Already handled: `String::from_utf8_lossy` in ValidationResult |
| Validation timeout/error vs exit code | Low | Medium | Create IterationLog even on timeout with exit_code=-1, empty output, error in stderr |

## Edge Cases

### Orphaned IterationLogs

When a `LoopExecution` is deleted (via `StateManager::delete_execution`), its associated `IterationLog` records become orphaned. Options:

1. **Cascade delete** (recommended): Add to `delete_execution` handler:
   ```rust
   // In actor_loop, before deleting execution:
   store.delete_by_index::<IterationLog>("execution_id", IndexValue::String(id.clone()))?;
   ```

2. **Lazy cleanup**: Let Phase 6 cleanup command find and remove orphans.

**Decision:** Implement cascade delete in Phase 2 to prevent orphan accumulation.

### Validation Failures

If `run_validation()` returns `Err(...)` (timeout, spawn failure, etc.) rather than a result with an exit code:

- Still create an IterationLog
- Set `exit_code = -1` (or a sentinel like `-999`)
- Set `stdout = ""`, `stderr = error.to_string()`
- Set `duration_ms` to elapsed time until failure

This ensures every iteration attempt is logged, even failed ones.

### Very Large Outputs

Some validation commands (e.g., verbose test runs) can produce megabytes of output.

**Thresholds:**
- 0-100 KB: Store inline in JSONL (typical case)
- 100 KB - 1 MB: Store inline but warn in logs
- >1 MB: Future enhancement - store in external file, reference path in IterationLog

**Initial implementation:** No size limit. Monitor in practice and add overflow handling if needed.

### Paused/Resumed Executions

When an execution is paused and later resumed:
- `iteration` counter persists in `LoopExecution`
- New iterations continue from where they left off
- IterationLog records maintain correct sequence

No special handling needed; existing iteration tracking handles this.

### Index Rebuild on Startup

`StateManager::spawn()` calls `rebuild_indexes` for known collections. Add IterationLog:

```rust
let iter_log_count = store.rebuild_indexes::<IterationLog>()?;
info!(iter_log_count, "Rebuilt indexes for IterationLog records");
```

## Open Questions

- [x] Should we limit stdout/stderr size at storage time? **Decision: No limit initially; add optional overflow to external files if needed later**
- [ ] Should IterationLog include the rendered prompt template? (Large, but useful for debugging)
- [ ] Should we track cache hits for LLM calls? (Anthropic returns cache_read_input_tokens)
- [ ] What's the retention policy for cleanup? (Suggest: 30 days default, configurable)

## Implementation Summary

| Phase | Description | Files Changed | Priority |
|-------|-------------|---------------|----------|
| 1 | IterationLog domain model | `domain/iteration_log.rs` (new), `domain/mod.rs` | High |
| 2 | StateManager integration | `state/manager.rs`, `state/messages.rs` | High |
| 3 | Engine integration | `loop/engine.rs` | High |
| 4 | Artifact tracking | `domain/execution.rs`, `loop/engine.rs` | Medium |
| 5 | TUI integration | `tui/views.rs`, `tui/state.rs`, `tui/runner.rs` | High |
| 6 | Cleanup command | `commands/clean.rs` (new) | Low |

**Recommended implementation order:** Phase 1 → 2 → 3 → 5 → 4 → 6

Phases 1-3 provide the core functionality. Phase 5 makes it visible. Phase 4 adds nice-to-have metadata. Phase 6 is future work.

## References

- Otto artifact storage pattern: `~/repos/otto-rs/otto/src/executor/`
- TaskStore implementation: `taskdaemon/ts/src/store.rs`
- Current domain models: `taskdaemon/td/src/domain/`
- TUI views: `taskdaemon/td/src/tui/views.rs`
- SystemCapturedProgress (truncation logic): `taskdaemon/td/src/progress/system_captured.rs`
- ValidationResult struct: `taskdaemon/td/src/loop/validation.rs`
