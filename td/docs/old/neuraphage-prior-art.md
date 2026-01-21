# Prior Art: What to Steal from Neuraphage

**Author:** Scott A. Idler
**Date:** 2026-01-15
**Status:** Reference Document

---

## Summary

Neuraphage (`~/repos/neuraphage/neuraphage/`) is an earlier implementation of a multi-task AI orchestrator. While TaskDaemon has a different architecture (Ralph loops vs persistent conversations), many Neuraphage components are directly reusable.

**Verdict:** Steal liberally. The code is production-grade with comprehensive tests.

---

## What to Steal

### 1. LlmClient Trait + AnthropicClient

**Source:** `src/agentic/llm.rs`, `src/agentic/anthropic.rs`

**Why steal:** Complete, tested implementation with:
- Streaming support (SSE parsing)
- Retry with exponential backoff
- Cost calculation per request
- Tool call parsing

**Port directly:**

```rust
// From neuraphage/src/agentic/llm.rs

/// A streaming chunk from the LLM.
#[derive(Debug, Clone)]
pub enum StreamChunk {
    TextDelta(String),
    ToolUseStart { id: String, name: String },
    ToolUseDelta { id: String, json_delta: String },
    ToolUseEnd { id: String },
    MessageDone { stop_reason: String, input_tokens: u64, output_tokens: u64 },
    Error(String),
}

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(&self, model: &str, messages: &[Message], tools: &[Tool]) -> Result<LlmResponse>;

    async fn stream(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[Tool],
        chunk_tx: mpsc::Sender<StreamChunk>,
    ) -> Result<LlmResponse>;
}
```

**Modifications needed:**
- Add `system_prompt: &str` parameter (TaskDaemon uses Handlebars templates)
- Consider adding prompt caching headers for Anthropic

---

### 2. Tool System

**Source:** `src/agentic/tools/mod.rs`, individual tool files

**Why steal:** Complete tool definitions with JSON schemas, context tracking, sandboxing hooks.

**Available tools (steal all):**
| Tool | File | Description |
|------|------|-------------|
| `read_file` | `filesystem.rs` | Read file with line numbers |
| `write_file` | `filesystem.rs` | Write file (requires prior read) |
| `list_directory` | `filesystem.rs` | List directory contents |
| `glob` | `filesystem.rs` | Glob pattern matching |
| `grep` | `grep.rs` | Content search with ripgrep |
| `edit` | `edit.rs` | String replacement edit |
| `run_command` | `bash.rs` | Execute shell commands |
| `ask_user` | `user.rs` | Prompt user for input |
| `web_fetch` | `web.rs` | Fetch URL content |
| `web_search` | `search.rs` | Web search |
| `spawn_task` | `task.rs` | Spawn sub-task |
| `todo_write` | `todo.rs` | Update todo list |
| `complete_task` | `control.rs` | Mark task complete |
| `fail_task` | `control.rs` | Mark task failed |

**Key pattern to steal - ToolContext:**

```rust
// From neuraphage/src/agentic/tools/mod.rs

pub struct ToolContext {
    pub working_dir: PathBuf,
    pub read_files: Arc<Mutex<HashSet<PathBuf>>>,  // Track reads for edit validation
    pub sandbox_enabled: bool,
}

impl ToolContext {
    pub async fn track_read(&self, path: &Path) {
        let mut read_files = self.read_files.lock().await;
        read_files.insert(path.to_path_buf());
    }

    pub async fn was_read(&self, path: &Path) -> bool {
        let read_files = self.read_files.lock().await;
        read_files.contains(path)
    }
}
```

**For TaskDaemon:** Each loop gets its own `ToolContext` scoped to its worktree.

---

### 3. KnowledgeStore

**Source:** `src/coordination/knowledge.rs`

**Why steal:** Cross-task learning extraction and injection - exactly what TaskDaemon needs for the Syncer persona concept.

**Key types:**

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KnowledgeKind {
    Learning,        // Pattern or technique
    Decision,        // Decision with rationale
    Fact,           // Discovered fact about codebase
    Preference,     // Convention
    ErrorResolution, // Error and its fix
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Knowledge {
    pub id: String,
    pub kind: KnowledgeKind,
    pub title: String,
    pub content: String,
    pub source_task: Option<TaskId>,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub relevance: f32,
    pub use_count: u32,  // Tracks usage for ranking
}
```

**Key method - query_relevant:**

```rust
/// Get relevant knowledge for a task context
pub async fn query_relevant(
    &self,
    tags: &[String],
    kinds: &[KnowledgeKind],
    limit: usize
) -> Vec<Knowledge> {
    // Scores: relevance + 0.1 * log(use_count + 1)
    // Relevance is primary, use_count is tiebreaker
}
```

**For TaskDaemon:**
- Store in TaskStore (new collection: `knowledge.jsonl`)
- Inject relevant knowledge into loop prompts via `{{knowledge}}` template variable
- Extract knowledge after successful Spec completion

---

### 4. EventBus

**Source:** `src/coordination/events.rs`

**Why steal:** Richer than TaskDaemon's current Alert/Share/Query model. Adds:
- Durability tracking (which events to persist)
- Filtered subscriptions
- Event history with query
- Statistics

**Key pattern - durable vs ephemeral events:**

```rust
impl EventKind {
    /// Returns true if this event should be persisted
    pub fn is_durable(&self) -> bool {
        match self {
            // Always persist
            EventKind::TaskStarted => true,
            EventKind::TaskCompleted => true,
            EventKind::TaskFailed => true,
            EventKind::MainUpdated => true,
            EventKind::RebaseCompleted => true,
            EventKind::RebaseConflict => true,

            // Ephemeral (don't persist)
            EventKind::FileModified => false,      // High volume
            EventKind::RateLimitReached => false,  // Operational
            _ => false,
        }
    }
}
```

**For TaskDaemon:** Add `is_durable()` to `TuiEvent` enum. Persist durable events to `events.jsonl` for debugging/audit.

---

### 5. Scheduler

**Source:** `src/coordination/scheduler.rs`

**Why steal:** Priority queue + rate limiting in one component. Cleaner than TaskDaemon's separate semaphore approach.

**Key types:**

```rust
pub struct SchedulerConfig {
    pub max_concurrent: usize,           // Max running tasks
    pub max_requests_per_minute: u32,    // Global rate limit
    pub rate_window: Duration,           // Sliding window
}

pub enum ScheduleResult {
    Ready,                              // Can run now
    Queued { position: usize },         // Waiting for slot
    RateLimited { retry_after: Duration }, // Hit rate limit
    Rejected { reason: String },        // Cannot schedule
}
```

**Key method - schedule:**

```rust
pub async fn schedule(&self, task: &Task) -> Result<ScheduleResult> {
    // 1. Check if already running
    // 2. Check if already queued
    // 3. Check rate limit
    // 4. Check concurrent limit
    // 5. Return Ready or queue
}
```

**For TaskDaemon:** Replace the separate `api_semaphore` with this Scheduler. Gives better visibility into queue state for TUI.

---

### 6. ExecutionStateStore (Recovery)

**Source:** `src/recovery.rs`

**Why steal:** Simple JSON-file-per-task recovery. Simpler than TaskDaemon's current TaskStore-based approach for crash recovery.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedExecutionState {
    pub task_id: String,
    pub iteration: u32,
    pub tokens_used: u64,
    pub cost: f64,
    pub phase: String,
    pub working_dir: PathBuf,
    pub worktree_path: Option<PathBuf>,
    pub checkpoint_at: DateTime<Utc>,
    pub started_reason: String,
}

impl ExecutionStateStore {
    pub async fn save(&self, task_id: &str, state: &PersistedExecutionState) -> Result<()>;
    pub async fn load(&self, task_id: &str) -> Result<Option<PersistedExecutionState>>;
    pub async fn remove(&self, task_id: &str) -> Result<()>;
    pub async fn list_all(&self) -> Result<Vec<PersistedExecutionState>>;
}
```

**For TaskDaemon:** Consider hybrid approach:
- Use TaskStore for permanent records (`LoopExecution`)
- Use ExecutionStateStore pattern for hot recovery state (checkpoints)

---

## Mapping: Neuraphage → TaskDaemon

| Neuraphage Concept | TaskDaemon Equivalent | Notes |
|--------------------|----------------------|-------|
| Task | LoopExecution | Running loop instance |
| Task priority | Spec/Plan priority | Inherit from parent |
| Conversation | Progress | Fresh context per iteration |
| Knowledge | Share events | Cross-loop learning |
| EventBus | Coordinator | Similar pub-sub model |
| Scheduler | Loop semaphore | Upgrade to Scheduler |
| Personas | Not needed | Ralph loops are simpler |
| Watcher | Max iterations | Loop self-terminates |
| Syncer | Share/Query | Built into coordinator |

---

## Files to Copy Directly

These files can be copied with minimal modification:

| Source | Destination | Changes |
|--------|-------------|---------|
| `src/agentic/llm.rs` | `src/llm.rs` | Remove neuraphage-specific imports |
| `src/agentic/anthropic.rs` | `src/anthropic.rs` | Update imports |
| `src/agentic/tools/*.rs` | `src/tools/*.rs` | Update imports, add worktree context |
| `src/coordination/knowledge.rs` | `src/knowledge.rs` | Integrate with TaskStore |
| `src/recovery.rs` | `src/recovery.rs` | Rename task_id to exec_id |

---

## Files to Adapt

These need significant modification:

| Source | Why Adapt |
|--------|-----------|
| `src/coordination/events.rs` | Merge with existing TuiEvent, add durability |
| `src/coordination/scheduler.rs` | Different scheduling model (loops vs tasks) |
| `src/daemon.rs` | TaskDaemon has different architecture |

---

## Implementation Order

1. **LlmClient + AnthropicClient** - Foundation for API calls
2. **Tool system** - Enable loop execution
3. **Scheduler** - Replace semaphore
4. **KnowledgeStore** - Enable cross-loop learning (Phase 5+)
5. **Event durability** - Audit trail (Phase 6+)

---

## Code Quality Notes

Neuraphage code is:
- Well-tested (see `#[cfg(test)]` modules)
- Uses `async_trait` for async traits
- Uses `tokio::sync::Mutex` (not `std::sync::Mutex`)
- Uses `eyre` for error handling (compatible with TaskDaemon)
- Has comprehensive doc comments

**Trust level:** High. Code has been run in production.

---

## Dependencies to Add

When porting, add these to `Cargo.toml`:

```toml
# For LLM client
reqwest = { version = "0.12", features = ["json", "stream"] }
reqwest-eventsource = "0.7"
futures = "0.3"

# For tools
glob = "0.3"
grep-regex = "0.1"  # Or use ripgrep crate

# For scheduler
# (no additional deps, uses tokio primitives)
```

---

## References

- [Neuraphage Design](../../neuraphage/docs/neuraphage-design.md)
- [Neuraphage Agentic Loop](../../neuraphage/docs/neuraphage-agentic-loop.md)
- [TaskDaemon Design](./taskdaemon-design.md)
- [Progress Strategy](./progress-strategy.md)
