# Design Document: TaskDaemon - Extensible Ralph Wiggum Loop Orchestrator

**Author:** Scott A. Idler
**Date:** 2026-01-14
**Status:** Ready for Implementation
**Review Passes:** 5/5

## Summary

TaskDaemon is an extensible framework for orchestrating N concurrent autonomous agentic workflows using the Ralph Wiggum loop pattern. Each loop restarts iterations with fresh context windows (preventing context rot) while persisting state in files and git. Loops coordinate via Alert/Share/Query events to maintain consistency across parallel work streams.

**Core Innovation:**
- **Extensible loop types**: Users define custom loops (Plan refinement, Spec decomposition, implementation, review, etc.) stored as YAML configs or TaskStore records
- **Fresh context always**: Every iteration starts a new API conversation, state lives in files
- **Massive parallelism**: Tokio async tasks enable 50+ concurrent loops efficiently (~100MB vs 10GB for processes)
- **Proactive coordination**: Loops Alert on main branch updates, Share data, Query each other

**Architecture:** Thin CLI forks a background daemon running tokio multi-threaded runtime. Daemon spawns Ralph loops as lightweight async tasks (~2MB each).

## Problem Statement

### Background

**The Ralph Wiggum Technique** (Geoffrey Huntley):
```bash
while :; do
  cat prompt.md | claude-code
  if validation_passes; then break; fi
done
```

The key: **start a new session with fresh context** each iteration. State persists in files/git, not in memory. This prevents context rot - LLM performance degradation after ~100k tokens.

**Existing Limitations:**

1. **Single-task focus**: Current Ralph implementations run one task at a time
2. **Context rot in plugins**: Official Ralph Wiggum plugin doesn't restart sessions → degradation
3. **Process waste**: Gas Town spawns full OS processes per agent (~10GB for 50 agents)
4. **No workflow extensibility**: Hardcoded workflows, can't define new loop types
5. **No coordination**: Loops can't communicate (duplicated work, conflicts, drift from main)
6. **Manual orchestration**: Developers chain workflows manually

### Problem

**How do we build a system that:**
- Runs 50+ concurrent Ralph loops efficiently (not Gas Town's process waste)
- Maintains fresh context at every iteration (not plugin's context rot)
- Supports custom workflow types (all loop types defined via configuration)
- Coordinates parallel loops (Alert on main updates, Share data, Query each other)
- Persists state durably (survives crashes, enables resume)
- Validates progress concretely (tests pass, not "looks good?")

### Goals

1. **Extensible loop framework**: Define custom loop types via YAML/TaskStore/Rust
2. **Fresh context always**: Every iteration = new API conversation
3. **Efficient concurrency**: Tokio async tasks, not OS processes
4. **Durable state**: TaskStore (SQLite + JSONL + Git) persists all progress
5. **Coordination events**: Alert/Share/Query primitives for inter-loop communication
6. **Concrete validation**: Completion determined by artifacts (files, tests, exit codes)
7. **Language agnostic**: Template-based prompts adapt to Rust, Python, TypeScript
8. **Observable**: TUI shows real-time progress across all loops
9. **Recoverable**: Crash/restart picks up where it left off

### Non-Goals

1. **Not a single-session orchestrator**: Each loop restarts between iterations (intentional)
2. **Not deterministic replay**: LLM outputs vary, that's acceptable
3. **Not a visual workflow editor**: Workflows defined in YAML/code, not dragged in UI
4. **Not multi-model**: Anthropic API only (extensible later)
5. **Not distributed**: All loops run on one machine (for now)

## Proposed Solution

### Overview: Extensible Ralph Loop System

TaskDaemon provides a **framework** for defining custom Ralph loop types. Out of the box, we define four loop types for standard software development, but users can create additional types for code review, docs generation, refactoring, security audits, etc.

**All loop types are defined via configuration.** TaskDaemon ships with four default loop type configs for standard software development:

```
Level 1: Plan Refinement Loop
  │ Input:  User idea + conversation
  │ Output: .taskstore/plans/{plan-id}.md (validated Plan document)
  │ Validation: User-configured validator command
  │ Completion: Validator exits 0
  │ Iterations: Typically 5-10 (Rule of Five refinement)
  ↓
Level 2: Spec Decomposition Loop
  │ Input:  Completed Plan
  │ Output: .taskstore/specs/{spec-id}.md × N (Specs with dependencies)
  │ Validation: User-configured validator command
  │ Completion: Validator exits 0
  │ Iterations: Typically 3-7 (until full coverage)
  ↓
Level 3: Spec Implementation Loop (Outer) — MANY RUN IN PARALLEL
  │ Input:  Spec with N phases
  │ Output: Git commits in worktree (one per phase)
  │ Validation: User-configured validator command
  │ Completion: Validator exits 0
  │ Iterations: N phases (each spawns Level 4)
  │ Parallelism: 10 ready Specs = 10 concurrent outer loops
  ↓
Level 4: Phase Implementation Loop (Inner) — MANY RUN IN PARALLEL
  │ Input:  Phase description from Spec
  │ Output: Code + tests + docs in worktree
  │ Validation: User-configured validator command
  │ Completion: Validator exits 0
  │ Iterations: Typically 1-20 (until validator passes)
  │ Parallelism: 50 phases across 10 Specs = 50 concurrent inner loops
```

**Users can modify the defaults or define their own loop types:**
- **Code review loops**: Automated review with iterative fixes
- **Documentation loops**: Generate/update docs from code
- **Refactoring loops**: Systematic code improvements
- **Security audit loops**: Vulnerability scanning + fixes
- **Performance loops**: Profiling + optimization cycles

**Loop type configuration locations:**

| Location | Use Case |
|----------|----------|
| `~/.config/taskdaemon/loop-types/*.yaml` | User-defined loop types |
| `.taskdaemon/loop-types/*.yaml` | Project-specific loop types |
| Built-in defaults | Ship with TaskDaemon, can be overridden |

### Daemon Architecture

**Process Model:** Thin CLI → Fork → Background Daemon → Tokio Tasks

```
┌──────────────────────────────────────────────────────────┐
│  CLI Process (ephemeral)                                 │
│  $ taskdaemon start                                      │
│  $ taskdaemon tui                                        │
└────────────────┬─────────────────────────────────────────┘
                 │ fork()
                 ▼
┌──────────────────────────────────────────────────────────┐
│  Daemon Process (long-running, background)               │
│  PID: 12345                                              │
│                                                           │
│  ┌────────────────────────────────────────────────────┐ │
│  │  Tokio Multi-Threaded Runtime                      │ │
│  │  Workers: num_cpus (8 threads)                     │ │
│  │                                                     │ │
│  │  ┌──────────────────────────────────────────────┐ │ │
│  │  │  LoopManager                             │ │ │
│  │  │  ┌───────┐ ┌───────┐ ┌───────┐ ┌────────┐   │ │ │
│  │  │  │Loop 1 │ │Loop 2 │ │Loop N │ │ ...    │   │ │ │
│  │  │  │Plan   │ │Spec   │ │Phase  │ │(50 max)│   │ │ │
│  │  │  │iter 3 │ │iter 5 │ │iter 7 │ │        │   │ │ │
│  │  │  └───┬───┘ └───┬───┘ └───┬───┘ └───┬────┘   │ │ │
│  │  └──────┼─────────┼─────────┼─────────┼────────┘ │ │
│  │         │ NEW API │ NEW API │ NEW API │          │ │
│  │         ▼         ▼         ▼         ▼          │ │
│  │  ┌──────────────────────────────────────────────┐ │ │
│  │  │  LlmClient (trait) + AnthropicClient (impl)  │ │ │
│  │  │  - Each iteration = new completion request   │ │ │
│  │  │  - Fresh context window (no rot)             │ │ │
│  │  └──────────────────────────────────────────────┘ │ │
│  └────────────────────────────────────────────────────┘ │
│                         │                                │
│                         │ persist state                  │
│                         ▼                                │
│  ┌────────────────────────────────────────────────────┐ │
│  │  TaskStore (SQLite + JSONL + Git)                  │ │
│  │  .taskstore/                                       │ │
│  │  ├── plans.jsonl                                   │ │
│  │  ├── plans/{plan-id}.md                            │ │
│  │  ├── specs.jsonl                                   │ │
│  │  ├── specs/{spec-id}.md                            │ │
│  │  ├── loops.jsonl                             │ │
│  │  └── taskstore.db (SQLite cache)                  │ │
│  └────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────┘
```

**Why fork a daemon?**
- Daemon survives CLI exit (long-running loops continue)
- TUI can disconnect/reconnect without interrupting work
- Single daemon manages all loops (no per-loop processes)
- Crash recovery: daemon restarts, reads TaskStore, resumes loops

**Daemon safety:**
- PID file prevents multiple daemon instances (`.taskstore/taskdaemon.pid`)
- Graceful shutdown on SIGTERM (finish current iteration, persist state)
- Signal handling for SIGHUP (reload config without restart)

**Implementation:**

```rust
// CLI (main.rs)
fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Start => {
            if daemon_running()? {
                eprintln!("Daemon already running");
                return Ok(());
            }
            let daemon_pid = daemonize()?;
            println!("TaskDaemon started (PID: {})", daemon_pid);
        }
        Command::Tui => {
            connect_and_launch_tui()?;
        }
        Command::Stop => {
            signal_daemon_shutdown()?;
        }
    }
    Ok(())
}

// Daemon (daemon.rs)
#[tokio::main]
async fn daemon_main() -> Result<()> {
    // Note: #[tokio::main] already creates runtime, no need to build another
    let manager = LoopManager::new().await?;

    // Recover incomplete loops from TaskStore
    manager.recover_loops().await?;

    // Main event loop
    manager.run().await
}
```

### Key Architectural Principles

#### Principle 1: Fresh Context, Always

**Never reuse context windows.** Each iteration creates a new API conversation:

```rust
// NOT THIS (context rot):
conversation.send_message(prompt).await?;  // Builds up context

// THIS (fresh context):
let response = api_client
    .create_conversation()  // New conversation ID
    .send_message(prompt)
    .await?;
```

**Why:** After ~100k tokens, LLM output quality degrades. Fresh context windows maintain "smart" token region performance.

#### Principle 2: State in Files, Not Memory

**All state persists in observable artifacts:**

| State | Storage | Format |
|-------|---------|--------|
| Plans | `.taskstore/plans/{id}.md` | Markdown (git-tracked) |
| Specs | `.taskstore/specs/{id}.md` | Markdown (git-tracked) |
| Metadata | `.taskstore/*.jsonl` | JSONL (git-tracked) |
| Code | Git worktrees (`/tmp/taskdaemon/worktrees/`) | Standard files (ephemeral, not in main repo) |
| Progress | Completion markers | In-file comments or sentinel files |
| Loop state | `loops.jsonl` | Iteration count, status, errors (git-tracked) |

**Why:** State survives crashes, is human-readable, version-controlled, debuggable.

#### Principle 3: Concrete Validation

**Never trust LLM completion signals.** Validate with concrete checks:

```rust
// NOT THIS:
if response.contains("I'm done") { break; }

// THIS:
let status = Command::new(&loop_config.validator_command)
    .args(&loop_config.validator_args)
    .status().await?;
if status.success() { break; }
```

**Validation is user-configured.** Each loop type specifies a validator command. TaskDaemon simply runs it and checks the exit code:
- Exit 0 → validation passed → loop complete
- Non-zero → validation failed → loop continues iterating

The validator can be **anything the user wants**:
- `otto ci`
- `make validate`
- `./scripts/check.sh`
- `python validate.py`
- `npm test`
- `cargo test && cargo clippy`

TaskDaemon has no opinion on what the validator should be. It's entirely user-defined per loop type and/or per project.

#### Principle 4: Massive Parallelism via Tokio

**Each Ralph loop = one tokio task (lightweight async):**

```rust
// 50 concurrent loops running in parallel:
let mut handles = vec![];
for spec in ready_specs {
    let handle = tokio::spawn(async move {
        run_loop(LoopConfig {
            loop_type: "phase".to_string(),
            parent: Some(spec.id),
            context: spec.into_context(),
        }).await
    });
    handles.push(handle);
}

// Memory: ~100MB for 50 loops
// vs. Gas Town: ~10GB for 50 processes
```

**Critical:** Many implementation loops run **simultaneously**, not sequentially:
- 10 Specs ready (dependencies met) → 10 outer loops spawn immediately
- Each outer loop has 5 phases → phases run sequentially within a Spec, but phases across different Specs run in parallel
- With 10 Specs each working on a phase concurrently → 10-20 inner loops running at any moment
- Total: ~30 tokio tasks active (10 outer + 10-20 inner), memory ~60-120MB

**Why:** Efficient shared memory, built-in coordination via channels, scales to 100+ loops.

#### Principle 5: Coordination via Events

**Loops coordinate without shared state** using three event types:

##### 1. Alert (Broadcast)

**Purpose:** Notify all loops of important system events

**Primary use case:** Main branch updated → all loops must rebase

```rust
// MainWatcher detects commit to main
coordinator.alert(Alert::MainBranchUpdated {
    commit_sha: "abc123".to_string(),
    timestamp: now_ms(),
}).await?;

// All running loops receive alert
async fn handle_alert(alert: Alert, worktree: &Path) -> Result<()> {
    match alert {
        Alert::MainBranchUpdated { commit_sha, .. } => {
            tracing::info!("Main updated to {}, rebasing...", commit_sha);

            // Pause work
            set_loop_state(LoopState::Rebasing).await?;

            // Rebase worktree
            let rebase_result = Command::new("git")
                .args(["rebase", "main"])
                .current_dir(worktree)
                .status()
                .await?;

            if !rebase_result.success() {
                // Rebase conflict: abort and alert operator
                Command::new("git")
                    .args(["rebase", "--abort"])
                    .current_dir(worktree)
                    .status()
                    .await?;

                tracing::error!("Rebase conflict in worktree {:?}, manual intervention needed", worktree);
                set_loop_state(LoopState::Blocked {
                    reason: "Rebase conflict with main".to_string()
                }).await?;
                return Err(eyre!("Rebase conflict requires manual resolution"));
            }

            // Resume work
            set_loop_state(LoopState::Running).await?;
            tracing::info!("Rebase complete, resuming");
        }
    }
}
```

**Why:** Prevents drift between feature branches and main, reduces merge conflicts.

##### 2. Share (Peer-to-Peer Data)

**Purpose:** One loop sends data to another loop(s)

**Use cases:**
- Loop A generates API schema → Share with Loop B (client code)
- Loop A runs benchmarks → Share results with Loop B (optimization)

```rust
// Loop A shares data
coordinator.share(Share {
    from: exec_id_a.clone(),
    to: vec![exec_id_b.clone()],
    data_type: "api_schema".to_string(),
    data: serde_json::to_value(&api_schema)?,
}).await?;

// Loop B receives share
async fn handle_share(share: Share) -> Result<()> {
    match share.data_type.as_str() {
        "api_schema" => {
            let schema: ApiSchema = serde_json::from_value(share.data)?;
            tracing::info!("Received API schema from {}", share.from);
            // Use schema in implementation
        }
        _ => {}
    }
}
```

##### 3. Query (Request/Reply)

**Purpose:** One loop asks another a question, waits for reply (with timeout)

**Use cases:**
- Loop A: "What's the endpoint URL?" → Loop B replies
- Loop A: "Did you implement feature X?" → Loop B replies

```rust
// Loop A queries Loop B
let reply = coordinator.query(Query {
    from: exec_id_a.clone(),
    to: exec_id_b.clone(),
    question: "What's the base URL for the API?".to_string(),
    timeout_ms: 30_000,
}).await?;

tracing::info!("Loop B replied: {}", reply);

// Loop B receives query
async fn handle_query(query: Query) -> Result<String> {
    match query.question.as_str() {
        q if q.contains("base URL") => {
            Ok("https://api.example.com".to_string())
        }
        _ => Ok("I don't know".to_string())
    }
}
```

**See:** [coordinator-design.md](./coordinator-design.md) for full protocol specification

## Core Concepts

This section defines the key abstractions that make TaskDaemon work.

### 1. Loop Type Definition

A **loop type** is a reusable workflow template that defines:
- **Inputs**: What state does the loop read?
- **Outputs**: What artifacts does it produce?
- **Validation**: How do we know it's progressing?
- **Completion**: When is the loop done?
- **Prompt template**: How do we build the fresh prompt each iteration?

**Example (YAML format):**

```yaml
# ~/.config/taskdaemon/loop-types/phase-implementation.yaml
loop_type:
  name: "phase-implementation"
  description: "Implement a single phase with code, tests, and docs"

  inputs:
    - type: "spec_phase"
      description: "Phase description from Spec markdown"
    - type: "worktree"
      description: "Git worktree path"
    - type: "language_config"
      description: "Language-specific settings (validation cmd, etc.)"

  outputs:
    - path: "worktree/*"
      description: "Code and test files"
    - path: "worktree/.phase-complete"
      description: "Sentinel file marking completion"

  validation:
    command: "{{language-config.validation-cmd}}"
    working-dir: "{{worktree}}"
    success-exit-code: 0

  completion:
    condition: "validation.exit-code == 0"

  prompt-template: |
    You are implementing {{phase-name}} (Phase {{phase-number}}/{{total-phases}}).

    ## Spec Context
    {{spec-content}}

    ## Current State
    {{git-status}}
    {{git-diff}}

    {{#if previous-errors}}
    ## Previous Validation Errors
    {{previous-errors}}
    {{/if}}

    ## Task
    1. Implement phase requirements
    2. Write tests
    3. Run: {{validation-command}}
    4. When CI passes, you're done

    Working directory: {{worktree}}

  max-iterations: 100
  iteration-timeout-ms: 300000  # 5 minutes
```

**See:** [loop-type-definition.md](./loop-type-definition.md) *(future doc)*

### 2. Loop Execution Engine

The core loop pattern (same for all loop types):

```rust
async fn run_loop(
    level: LoopLevel,
    llm: Arc<dyn LlmClient>,
    coordinator: Arc<Coordinator>,
    store_tx: mpsc::Sender<StoreMessage>,
) -> Result<()> {
    let mut iteration = 0;
    let loop_def = load_loop_definition(&level)?;

    loop {
        iteration += 1;
        tracing::info!("Ralph loop {} iteration {}", level.name(), iteration);

        // 1. Read current state from files/git
        let state = read_state_from_files(&level).await?;

        // 2. Check for coordination events (Alert/Share/Query)
        // Note: Use try_recv() or poll pattern, not blocking await
        while let Some(event) = coordinator.poll_events_non_blocking() {
            handle_coordination_event(event, &level).await?;
        }

        // 3. Generate fresh prompt from template
        let prompt = render_prompt_template(&loop_def.prompt_template, &state)?;

        // 4. Call LLM (fresh context - no conversation state!)
        let request = CompletionRequest {
            system_prompt: loop_def.system_prompt.clone(),
            messages: vec![Message::user(&prompt)],
            tools: loop_def.tools.clone(),
        };
        let response = match llm.complete(request).await {
            Ok(resp) => resp,
            Err(e) if e.is_rate_limit() => {
                // Exponential backoff on rate limit
                tracing::warn!("Rate limited, backing off...");
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue; // Retry same iteration
            }
            Err(e) => return Err(e), // Other errors fail the loop
        };

        // 5. Execute tool calls / actions
        execute_actions(&response, &level).await?;

        // 6. Persist loop state
        persist_loop_iteration(&level, iteration, &store_tx).await?;

        // 7. Run validation
        let validation_passed = run_validation(&loop_def.validation, &level).await?;

        // 8. Check completion
        if validation_passed && check_completion_condition(&loop_def, &level).await? {
            tracing::info!("Loop {} complete after {} iterations", level.name(), iteration);
            break;
        }

        if iteration >= loop_def.max_iterations {
            tracing::error!("Loop {} exceeded max iterations", level.name());
            return Err(eyre!("Max iterations exceeded"));
        }

        // 9. Brief pause before next iteration
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Context dies here. Next iteration reads fresh state.
    }

    Ok(())
}
```

**Key Points:**
- Same execution logic for all loop types (extensible)
- State read from files (not carried in memory)
- Coordination events checked each iteration
- Fresh API conversation every time
- Validation runs after every action
- Completion determined by concrete checks

### 3. Completion Markers

Each loop type specifies a **validator command**. Completion = validator exits 0.

```rust
async fn check_completion(loop_type: &LoopType, ctx: &LoopContext) -> Result<bool> {
    // Run the user-configured validator command
    let status = Command::new("sh")
        .args(["-c", &loop_type.validator])
        .current_dir(&ctx.worktree)
        .status()
        .await?;

    Ok(status.success())
}
```

**The validator is user-defined.** TaskDaemon doesn't care what it is:

```yaml
# Loop type definition example
name: rust-phase
validator: "otto ci"  # or "make check" or "cargo test" or "./validate.sh"
```

That's it. Exit 0 = done. Non-zero = keep iterating.

### 4. TaskStore Integration

TaskStore provides durable state with SQLite+JSONL+Git pattern:

```rust
// Define domain types
#[derive(Serialize, Deserialize, Clone)]
struct Plan {
    id: String,
    title: String,
    status: PlanStatus,  // Draft, Ready, InProgress, Complete
    file: String,        // "add-feature.md"
    created_at: i64,
    updated_at: i64,
}

impl Record for Plan {
    fn id(&self) -> &str { &self.id }
    fn updated_at(&self) -> i64 { self.updated_at }
    fn collection_name() -> &'static str { "plans" }
    fn indexed_fields(&self) -> HashMap<String, IndexValue> {
        let mut fields = HashMap::new();
        fields.insert("status".to_string(),
                     IndexValue::String(self.status.to_string()));
        fields
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct Spec {
    id: String,
    parent: String,           // ID of parent (typically a Plan)
    title: String,
    status: SpecStatus,
    deps: Vec<String>,        // IDs that must complete before this can start
    file: String,
    phases: Vec<Phase>,
    created_at: i64,
    updated_at: i64,
}

impl Record for Spec { /* ... */ }

#[derive(Serialize, Deserialize, Clone)]
struct LoopExecution {
    id: String,
    loop_type: String,        // "plan", "spec", "phase", "ralph"
    parent: Option<String>,   // ID of parent record (up the tree)
    deps: Vec<String>,        // IDs that must complete before this can start
    worktree: Option<String>,
    status: LoopStatus,       // Running, Complete, Failed, Paused
    iteration: u32,
    progress: String,         // Accumulated progress from previous iterations
    context: serde_json::Value, // Template context
    created_at: i64,
    updated_at: i64,
}

impl Record for LoopExecution { /* ... */ }

// Store operations
let mut store = Store::open(".taskstore")?;

// Query ready specs
let ready_specs: Vec<Spec> = store.list(&[
    Filter {
        field: "status",
        op: Eq,
        value: String("ready")
    }
])?;

// Persist loop state
store.update(loop_exec)?;
```

**Storage layout:**

```
.taskstore/
├── plans.jsonl                 # Plan metadata
├── plans/
│   ├── add-oauth.md
│   └── refactor-api.md
├── specs.jsonl                 # Spec metadata
├── specs/
│   ├── oauth-db-schema.md
│   ├── oauth-endpoints.md
│   └── oauth-tests.md
├── loops.jsonl           # Loop execution state
├── events.jsonl   # Alert/Share/Query messages
└── taskstore.db                # SQLite query cache
```

**See:** [TaskStore README](https://github.com/saidler/taskstore)

### 5. Dependency Resolution

Specs can depend on other Specs. The LoopManager resolves dependencies before spawning:

```rust
// Dependency validation (before spawning any loops)
fn validate_dependency_graph(specs: &[Spec]) -> Result<()> {
    // Build adjacency list
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    for spec in specs {
        graph.insert(spec.id.clone(), spec.dependencies.clone());
    }

    // Detect cycles using DFS
    let mut visited = HashSet::new();
    let mut rec_stack = HashSet::new();

    for spec in specs {
        if has_cycle(&spec.id, &graph, &mut visited, &mut rec_stack) {
            return Err(eyre!("Circular dependency detected in Spec {}", spec.id));
        }
    }

    Ok(())
}

// Scheduling (only spawn loops for specs with satisfied dependencies)
fn get_ready_specs(store: &Store) -> Result<Vec<Spec>> {
    let all_specs: Vec<Spec> = store.list(&[
        Filter {
            field: "status",
            op: Eq,
            value: String("pending")
        }
    ])?;

    let ready = all_specs.into_iter()
        .filter(|spec| {
            // All dependencies must be complete
            spec.dependencies.iter().all(|dep_id| {
                store.get::<Spec>(dep_id)
                    .ok()
                    .flatten()
                    .map(|d| d.status == SpecStatus::Complete)
                    .unwrap_or(false)
            })
        })
        .collect();

    Ok(ready)
}
```

**Dependency graph example:**

```
spec-001 (OAuth DB schema)
  ├─ no dependencies → spawn immediately

spec-002 (OAuth endpoints)
  ├─ depends on: spec-001
  └─ wait until spec-001 complete → then spawn

spec-003 (OAuth tests)
  ├─ depends on: spec-002
  └─ wait until spec-002 complete → then spawn
```

**Scheduler behavior:**
- Polls TaskStore every 10 seconds
- Checks for Specs with `status=pending` and all dependencies complete
- Spawns Level 3 (Spec Implementation Loop) for each ready Spec
- Updates Spec `status=running` when loop starts
- When loop completes, marks Spec `status=complete` and wakes dependent Specs

### 6. LlmClient Abstraction

The `LlmClient` trait abstracts LLM interactions, enabling testability and future provider flexibility while keeping the implementation focused on Anthropic.

**Trait definition:**

```rust
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Send a completion request, get a response (stateless - no conversation)
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse>;
}

pub struct CompletionRequest {
    pub system_prompt: Option<String>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
}

pub struct CompletionResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub stop_reason: StopReason,
    pub usage: TokenUsage,
}

pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
}
```

**Anthropic implementation:**

```rust
pub struct AnthropicClient {
    model: String,
    api_key: String,
    base_url: String,
    http: reqwest::Client,
    max_tokens: u32,
}

impl AnthropicClient {
    pub fn from_config(config: &LlmConfig) -> Result<Self> {
        let api_key = std::env::var(&config.api_key_env)
            .context("API key environment variable not set")?;

        Ok(Self {
            model: config.model.clone(),
            api_key,
            base_url: config.base_url.clone(),
            http: reqwest::Client::new(),
            max_tokens: config.max_tokens,
        })
    }
}

impl LlmClient for AnthropicClient {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        // Anthropic-specific API call
        // Handles rate limiting internally with exponential backoff
    }
}
```

**Configuration:**

```yaml
# ~/.config/taskdaemon/config.yaml
llm:
  provider: anthropic
  model: claude-opus-4-5-20251101
  api_key_env: ANTHROPIC_API_KEY
  base_url: https://api.anthropic.com
  max_tokens: 8192
  timeout_ms: 300000
```

**Design rationale:**
- **Trait for testability** - mock `LlmClient` in tests without hitting real API
- **Stateless by design** - no conversation management, each call is independent (fresh context)
- **Configuration over hardcoding** - model, API key source, base URL all configurable
- **Anthropic-focused** - no premature abstraction for providers we don't need
- **Rate limiting internal** - `AnthropicClient` handles backoff, callers don't need to know

**What's NOT abstracted** (Anthropic-specific, kept in implementation):
- Prompt caching (`cache_control` blocks)
- Beta features (computer use, etc.)
- Anthropic-specific error types

## Architecture

### System Components

```
┌─────────────────────────────────────────────────────────────┐
│                   TaskDaemon Daemon                         │
│                                                              │
│  ┌──────────────────────────────────────────────────────┐  │
│  │              LoopManager                         │  │
│  │  - Spawns loops as tokio tasks                       │  │
│  │  - Tracks running loops (HashMap<id, JoinHandle>)    │  │
│  │  - Recovers incomplete loops on startup              │  │
│  │  - Enforces concurrency limits (Semaphore)           │  │
│  │  - Resolves Spec dependencies (topological sort)     │  │
│  │  - Schedules ready Specs when dependencies met       │  │
│  └───────────────────┬──────────────────────────────────┘  │
│                      │ spawns                               │
│  ┌───────────────────▼──────────────────────────────────┐  │
│  │  Ralph Loops (tokio tasks, many running in parallel) │  │
│  │  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐        │  │
│  │  │ Plan   │ │ Spec   │ │ Outer  │ │ Inner  │  ...   │  │
│  │  │ Loop   │ │ Decomp │ │ Impl   │ │ Impl   │        │  │
│  │  │ iter 3 │ │ iter 5 │ │ iter 2 │ │ iter 7 │        │  │
│  │  └────┬───┘ └────┬───┘ └────┬───┘ └────┬───┘        │  │
│  └───────┼──────────┼──────────┼──────────┼────────────┘  │
│          │          │          │          │                │
│  ┌───────▼──────────▼──────────▼──────────▼────────────┐  │
│  │            Coordinator (tokio task)                   │  │
│  │  - Routes Alert/Share/Query messages                 │  │
│  │  - Maintains registry: exec_id → channel             │  │
│  │  - Persists events to TaskStore                      │  │
│  │  - Implements MainWatcher (detects main updates)     │  │
│  └───────────────────┬───────────────────────────────────┘  │
│                      │ uses                                 │
│  ┌───────────────────▼───────────────────────────────────┐ │
│  │    LlmClient (trait) + AnthropicClient (impl)         │ │
│  │  - Stateless completion requests (fresh context)      │ │
│  │  - Configurable: model, API key, base URL             │ │
│  │  - Handles rate limiting with backoff                 │ │
│  └───────────────────┬───────────────────────────────────┘ │
│                      │ persists                             │
│  ┌───────────────────▼───────────────────────────────────┐ │
│  │    StateManager (tokio task, owns Store)              │ │
│  │  - Processes StoreMessages (actor pattern)            │ │
│  │  - Queries TaskStore (SQLite)                         │ │
│  │  - Writes to JSONL files                              │ │
│  └───────────────────┬───────────────────────────────────┘ │
│                      │                                      │
│  ┌───────────────────▼───────────────────────────────────┐ │
│  │    TaskStore (SQLite + JSONL + Git)                   │ │
│  │  .taskstore/                                          │ │
│  │  ├── plans.jsonl / plans/*.md                         │ │
│  │  ├── specs.jsonl / specs/*.md                         │ │
│  │  ├── loops.jsonl                                │ │
│  │  ├── events.jsonl                        │ │
│  │  └── taskstore.db                                     │ │
│  └────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

### Data Flow: End-to-End Example

**User Request → Complete Implementation:**

```
1. User: "I want to add OAuth authentication"
   $ taskdaemon new-plan

   ↓ Spawn Level 1: Plan Refinement Loop
   ├─ Iteration 1: Agent gathers requirements via Q&A
   ├─ Iteration 2: Generate initial Plan draft
   ├─ Iterations 3-7: Review passes (Rule of Five)
   └─ Complete: .taskstore/plans/add-oauth.md created

2. User marks Plan ready:
   $ taskdaemon plan mark-ready add-oauth

   ↓ Spawn Level 2: Spec Decomposition Loop
   ├─ Iteration 1: Analyze Plan, generate 3 Specs
   ├─ Iteration 2: Add missing coverage (validator found gap)
   └─ Complete: 3 Specs created with dependencies
       - spec-001.md: OAuth database schema (no deps)
       - spec-002.md: OAuth endpoints (depends on spec-001)
       - spec-003.md: OAuth tests (depends on spec-002)

3. Scheduler detects ready Spec (spec-001 has no deps):

   ↓ Spawn Level 3: Spec Implementation Loop (Outer)
   │
   ├─ Phase 1: Spawn Level 4 (Inner)
   │   ├─ Iteration 1: Write migration SQL
   │   ├─ Iteration 2: Fix syntax error (CI failed)
   │   └─ Iteration 3: CI passes → Phase 1 complete, git commit
   │
   ├─ Phase 2: Spawn Level 4 (Inner) — RUNS IN PARALLEL with others
   │   ├─ Iteration 1: Write schema structs
   │   ├─ Iteration 2: Fix imports (CI failed)
   │   └─ Iteration 3: CI passes → Phase 2 complete, git commit
   │
   └─ All phases done + acceptance tests pass → Spec complete
      Merge to main → ALERT all loops

4. Alert: Main branch updated

   ↓ All 49 running loops receive Alert
   ├─ Each loop pauses current work
   ├─ Rebases worktree against new main
   └─ Resumes work with fresh context (next iteration)

5. spec-001 complete → spec-002 now ready (dependency met)

   ↓ Spawn another Level 3 loop for spec-002 (runs in parallel)
   ↓ Now 2 outer loops running concurrently
   ↓ Each has 5 phases → 10 inner loops running concurrently

6. All Specs complete → Plan marked complete
```

**Throughout execution:**
- Each iteration = fresh context window
- State persists in `.taskstore/` files
- Git tracks all changes
- TUI shows real-time progress (optional)
- Crashes/restarts recover from TaskStore

## Implementation Approach

### Phase 1: Core Ralph Loop Engine
**Goal:** Single Ralph loop working end-to-end

**Tasks:**
1. Implement `run_loop()` core function
2. Define `LlmClient` trait and implement `AnthropicClient`
3. Implement `check_completion()` for 4 standard loop types
4. Load loop type definitions from config files
5. Test with manual Level 4 (Phase Implementation) loop

**Deliverable:** Can run one inner loop manually, sees fresh context per iteration

**See:** [implementation-phase-1.md](./implementation-phase-1.md) *(future doc)*

### Phase 2: TaskStore Integration
**Goal:** Durable state persistence

**Tasks:**
1. Define domain types (Plan, Spec, LoopExecution)
2. Implement Record trait for each type
3. Build StateManager with actor pattern (message passing)
4. Persist loop iterations to TaskStore
5. Implement crash recovery (read incomplete loops, restart)

**Deliverable:** Loops survive crashes and resume from last state

**See:** [implementation-phase-2.md](./implementation-phase-2.md) *(future doc)*

### Phase 3: Coordination Protocol
**Goal:** Inter-loop communication (Alert/Share/Query)

**Tasks:**
1. Implement Coordinator with event routing
2. Add MainWatcher (detect main branch commits)
3. Implement Alert broadcast (rebase on main update)
4. Implement Share (p2p data exchange)
5. Implement Query (request/reply with timeout)

**Deliverable:** Loops coordinate, rebase proactively on main updates

**See:** [implementation-phase-3.md](./implementation-phase-3.md) *(future doc)*

### Phase 4: Multi-Loop Orchestration
**Goal:** Parallel execution of multiple loops with dependency resolution

**Tasks:**
1. Implement LoopManager with scheduler
2. Add dependency graph validation (topological sort, cycle detection)
3. Implement polling scheduler (check for ready Specs every 10s)
4. Spawn multiple Level 3 loops concurrently (10+)
5. Test dependency chain: Spec A → Spec B → Spec C
6. Add semaphore for concurrency limits

**Deliverable:** Can run 50+ loops in parallel efficiently, respecting dependencies

**See:** [implementation-phase-4.md](./implementation-phase-4.md) *(future doc)*

### Phase 5: Full Pipeline
**Goal:** All 4 standard loop types integrated

**Tasks:**
1. Implement Level 1 (Plan Refinement)
2. Implement Level 2 (Spec Decomposition)
3. Wire up Level 1 → 2 → 3 → 4 cascade
4. Test full user idea → implementation flow

**Deliverable:** End-to-end workflow functional

**See:** [implementation-phase-5.md](./implementation-phase-5.md) *(future doc)*

### Phase 6: Advanced Loop Features
**Goal:** Enhanced loop capabilities and tooling

**Tasks:**
1. Hot-reload loop type configs without daemon restart
2. Loop type inheritance (extend existing types)
3. Conditional validation (different validators per phase)
4. Loop analytics and performance reporting
5. Loop type testing framework

**Deliverable:** Production-hardened loop system with advanced customization

**See:** [implementation-phase-6.md](./implementation-phase-6.md) *(future doc)*

### Phase 7: TUI & Polish
**Goal:** Observable, controllable system

**Tasks:**
1. Build ratatui TUI showing all loops
2. Real-time event streaming
3. Control operations (pause, stop, query)
4. Add metrics and logging (tracing + structured logs)
5. Performance tuning for 50+ loops

**Deliverable:** Production-ready system

**See:** [implementation-phase-7.md](./implementation-phase-7.md) *(future doc)*

## Quality Assurance

### Testing Strategy

**Unit Tests:**
- Loop type definition parsing (YAML validation)
- Completion marker detection logic
- Prompt template rendering (Handlebars)
- TaskStore Record trait implementations
- Coordinator message routing

**Integration Tests:**
- Single loop end-to-end (spawn → iterate → complete)
- Crash recovery (kill daemon mid-loop, restart, verify resume)
- Alert broadcast (main update → all loops rebase)
- Share/Query between two loops
- Dependency resolution (Spec A blocks on Spec B)

**System Tests:**
- 10 concurrent Specs running end-to-end
- 50 concurrent Phase loops with CI validation
- Memory usage under load (verify ~100MB for 50 loops)
- API rate limiting behavior (verify backoff + queue)
- Full pipeline: user idea → Plan → Specs → implementation

**Manual Tests:**
- TUI responsiveness with 50 active loops
- Daemon daemonization (survives terminal close)
- Hot-reload config changes
- Git worktree isolation (no cross-contamination)

## Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| **Loop restart strategy** | New API conversation each iteration | Prevents context rot, maintains LLM performance |
| **Loop extensibility** | YAML configs → Rust types | Users can define custom loops without forking codebase |
| **Concurrency model** | Tokio async tasks | Efficient (~100MB for 50 loops vs 10GB processes) |
| **State persistence** | TaskStore (SQLite + JSONL + Git) | Durable, observable, version-controlled |
| **Coordination** | Alert/Share/Query events | Loops coordinate without shared memory |
| **Completion detection** | File-based markers + validation | Concrete, testable, not LLM promises |
| **Prompt generation** | Template-based (Handlebars) | Reusable, maintainable, language-agnostic |
| **Validation approach** | Concrete artifacts (exit codes, files) | Deterministic, no ambiguity |
| **Architecture pattern** | Actor model (message passing) | Avoids deadlocks, clear ownership |
| **Process model** | CLI forks daemon (tokio runtime) | Long-running, survives CLI exit, efficient |
| **Dependency scheduling** | Topological sort, poll-based scheduler | Prevents deadlocks, enables parallel work streams |
| **Error recovery** | Persist every iteration, restart from TaskStore | Crash-safe, no work lost |
| **LLM abstraction** | `LlmClient` trait + `AnthropicClient` impl | Testable, configurable, not hardcoded |

## Critical Differences from Existing Approaches

| Aspect | Gas Town | Ralph Plugin | TaskDaemon |
|--------|----------|--------------|------------|
| **Context Management** | Shared across processes | Accumulates in session | Fresh each iteration |
| **Concurrency** | OS processes (~200MB each) | Single session | Tokio tasks (~2MB each) |
| **State Persistence** | Beads (complex) | In-memory | TaskStore (SQLite+JSONL) |
| **Workflow Extensibility** | Hardcoded tmux scripts | N/A | YAML config files |
| **Coordination** | Manual (tmux panes) | N/A | Alert/Share/Query events |
| **Completion Detection** | Manual checkpoints | Promise markers | File-based + validation |
| **Memory for 50 loops** | ~10GB | ~500MB (then degrades) | ~100MB |
| **Context Rot** | No (new processes) | Yes (same session) | No (fresh conversations) |
| **Parallelism** | Manual (spawn scripts) | No | Automatic (tokio spawns) |

## Documents from Previous Design

Several docs from the previous AWL-based design contain useful concepts:

**Still Applicable (moved to docs/):**
- [coordinator-design.md](./coordinator-design.md) - Alert/Share/Query protocol fully applies
- [stop-using-the-ralph-loop-plugin-summary.md](./stop-using-the-ralph-loop-plugin-summary.md) - Context rot background
- [execution-model-design.md](./execution-model-design.md) - Git worktree management, crash recovery, state transitions (ignore AWL iteration pattern sections)
- [tui-design.md](./tui-design.md) - TUI architecture, navigation model, real-time updates (just update terminology: PRD→Plan, TS→Spec)

**Partially Applicable (in docs/old/):**
- `developer-guide.md` - Validation patterns (section 3), TaskStore integration (section 2), actor model (section 9), naming conventions (section 1) apply

**Superseded (in docs/old/):**
- `awl-schema-design.md` - AWL replaced by extensible Ralph loops with YAML loop types
- `taskdaemon-design.md` - Old design, superseded by this document

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| API rate limiting blocks all loops | Medium | High | Implement exponential backoff, queue management, and semaphore limits (start with 50 concurrent) |
| Context window overflow with large state | Low | Medium | Truncate old git history, summarize large files in prompts, limit state reading to recent changes |
| Circular dependencies in Spec graph | Medium | High | Validate dependency graph before spawning loops, detect cycles with topological sort |
| Main branch rebase conflicts | High | Medium | Abort rebase on conflict, mark loop as Blocked, require manual resolution before resuming |
| Loop stuck in infinite iteration | Low | Medium | Max iteration limits (1000 default), supervisor monitoring, timeout per iteration (5 min default) |
| TaskStore corruption | Low | High | JSONL is source of truth (can rebuild SQLite), git tracks all changes, regular backups |
| Anthropic API downtime | Low | High | Persist loop state on every iteration, resume when API returns, alert operator |
| Memory exhaustion with 100+ loops | Low | Medium | Monitor memory usage, enforce max concurrent loops (configurable), prioritize critical Specs |
| Prompt injection via user-controlled Spec content | Medium | Medium | Sanitize Spec content before template rendering, validate markdown structure, limit prompt size |
| Git worktree cleanup failure (disk full) | Low | Medium | Monitor disk usage, implement cleanup retries, alert if worktrees accumulate |
| Concurrent writes to same JSONL file | Low | High | TaskStore uses file locking, StateManager serializes writes via actor pattern |

## Open Questions

- [x] Should loop type definitions support Turing-complete logic, or keep simple (validation, templates)? **Decision:** Keep simple. Complex logic belongs in LLM prompts, not loop definitions.
- [x] Store loop definitions in TaskStore or YAML files? **Decision:** YAML files (parsed into Rust types at runtime)
- [x] Optimal semaphore limit for concurrent API calls? **Decision:** 10 concurrent API calls. Conservative start, tune based on rate limit feedback.
- [x] Should we implement rate limiting per loop or global? **Decision:** Global. Let Anthropic throttle us, handle 429s with exponential backoff.
- [ ] How to handle Specs that block on external dependencies? **Recommend:** Manual unblock command
- [ ] Should Plan refinement loop support user interaction mid-loop? **Recommend:** Yes, via channel (Phase 5+)
- [x] Maximum reasonable iterations before declaring loop stuck? **Decision:** 100 default (configurable per loop type). 1000 is too high for most cases.
- [ ] Should we add a "supervisor loop" that monitors and restarts failed loops? **Recommend:** Phase 8 enhancement
- [ ] How to handle authentication/API key rotation without daemon restart? **Recommend:** Hot-reload config file
- [ ] Should loops prioritize by Plan importance or FIFO? **Recommend:** Add priority field to Plans (Phase 6+)
- [x] How to accumulate progress across iterations? **Decision:** ProgressStrategy trait with SystemCapturedProgress default. See [progress-strategy.md](./progress-strategy.md)

## References

### Inspiration
- **Ralph Wiggum Technique:** Geoffrey Huntley's bash loop pattern with fresh sessions
- **Gas Town:** Steve Yegge's multi-agent orchestration (tmux + processes)
- **TaskStore:** Beads-inspired SQLite+JSONL+Git pattern

### Related Documents
- [taskdaemon.yml](../taskdaemon.yml) - Full example config with all builtin loop definitions (plan, spec, phase, ralph)
- [TaskStore](../taskstore/) - Generic persistent state library (sibling directory)
- [Ralph Plugin Criticism](./stop-using-the-ralph-loop-plugin/summary.md) - Why plugins suffer context rot
- [Coordinator Protocol](./coordinator-design.md) - Alert/Share/Query event system
- [Execution Model](./execution-model-design.md) - Git worktree management and crash recovery
- [TUI Design](./tui-design.md) - Terminal interface architecture
- [Implementation Details](./implementation-details.md) - Loop schema, domain types, ID format, template variables
- [Config Schema](./config-schema.md) - Configuration hierarchy and full schema
- [Progress Strategy](./progress-strategy.md) - Cross-iteration state accumulation (ProgressStrategy trait)
- [Rule of Five](./rule-of-five.md) - Structured Plan refinement methodology

### Implementation Specifications
- [LLM Client](./llm-client.md) - LlmClient trait, AnthropicClient implementation, streaming
- [Tools](./tools.md) - Tool system with worktree-scoped execution
- [Scheduler](./scheduler.md) - Priority queue + rate limiting for API calls
- [Loop Engine](./loop-engine.md) - Iteration execution flow, agentic tool loop, validation
- [Loop Manager](./loop-manager.md) - Orchestrator, dependency resolution, recovery, shutdown
- [Domain Types](./domain-types.md) - Plan, Spec, LoopExecution types and Record trait

### Future Detail Documents
- `deployment-guide.md` - Running TaskDaemon in production

---

## Review Log

### Status: Complete (5/5 passes)

**Pass 1 (Completeness):** Added Risks & Mitigations, Testing Strategy, expanded future docs
**Pass 2 (Correctness):** Fixed tokio runtime creation, completion logic, parallelism explanation, polling pattern, worktree tracking
**Pass 3 (Edge Cases):** Added daemon safety, rate limit handling, rebase conflict handling, 3 more risks (prompt injection, disk full, concurrent writes)
**Pass 4 (Architecture):** Added dependency resolution section with code examples, updated LoopManager responsibilities, expanded Phase 4 tasks
**Pass 5 (Clarity):** Document is implementation-ready with concrete examples throughout

---

**Document Goal:** Provide clear, actionable design for extensible fractal Ralph loop orchestrator that enables custom workflow types, massive parallelism, fresh context at every iteration, and concrete validation. Users can define new loop types for their specific needs without forking the codebase.
