# Design Document: TaskDaemon - Multi-Loop Agentic Orchestrator

**Author:** Scott Aidler
**Date:** 2026-01-13
**Status:** Complete
**Review Passes:** 5/5 (Complete)

## Summary

TaskDaemon is a Rust-based orchestrator for concurrent agentic workflows. Unlike Claude Code (single task per terminal), TaskDaemon manages N parallel "Ralph loops" in a single process using tokio async/await, with each loop running in an isolated git worktree. AWL (Agentic Workflow Language) defines reusable workflow templates. TaskStore provides durable state via the SQLite+JSONL+Git pattern.

## Documentation Structure

This design is split across multiple documents:

| Document | Description |
|----------|-------------|
| **[Overview](./taskdaemon-design.md)** | Architecture, positioning, key decisions (this document) |
| **[AWL Schema](./awl-schema-design.md)** | Workflow language format, primitives, examples |
| **[TaskStore](https://github.com/saidler/taskstore)** | Generic storage library (SQLite, JSONL, Git integration) |
| **[Coordinator Protocol](./coordinator-design.md)** | Notify/Query/Share inter-loop messaging |
| **[Execution Model](./execution-model-design.md)** | Loop lifecycle, spawn/pause/resume, crash recovery |
| **[Developer Guide](./developer-guide.md)** | Implementation details, naming conventions, validation patterns |

## Table of Contents

1. [Problem Statement](#problem-statement)
2. [Proposed Solution](#proposed-solution)
3. [Architecture Overview](#architecture-overview)
4. [Key Concepts](#key-concepts)
5. [Technology Stack](#technology-stack)
6. [Design Decisions](#design-decisions)
7. [Implementation Plan](#implementation-plan)
8. [Alternatives Considered](#alternatives-considered)
9. [Technical Considerations](#technical-considerations)
10. [Risks and Mitigations](#risks-and-mitigations)
11. [Open Questions](#open-questions)
12. [References](#references)

## Problem Statement

### Background

**Ralph Wiggum** pioneered iterative autonomous development: feed the same prompt repeatedly, let the agent work, persist progress in files/git, loop until complete. This pattern is powerful but limited to **one task at a time**.

**Gas Town** (Steve Yegge) demonstrated multi-agent orchestration using tmux + 20-30 separate Claude Code processes. This achieves parallelism but is chaotic:
- Process-per-task overhead
- tmux UI is hard to manage
- No structured coordination
- No clean state management

**Claude Code** is excellent for single-task work but:
- One task per terminal session
- Context window exhaustion
- No parallel execution
- No task persistence beyond conversation history

### Problem

We need a system that:
1. Runs **N concurrent agentic loops** (like Ralph, but parallel)
2. Uses **async/await** not processes (efficient, shared memory)
3. Provides **structured coordination** (loops can notify/query each other)
4. Offers **clean UX** (ratatui TUI, not tmux chaos)
5. Persists **durable state** (survive crashes, resume work)
6. Isolates **parallel work** (git worktrees prevent conflicts)

### Goals

1. **Multi-loop execution:** Run 5, 10, 50 loops concurrently in one process
2. **Workflow templates:** Define reusable patterns (Rust dev, Python dev, PRD generation)
3. **Durable state:** SQLite+JSONL+Git pattern for persistence
4. **Git isolation:** One worktree per loop, automatic rebase coordination
5. **Structured coordination:** Notify (broadcast), Query (request), Share (p2p)
6. **Ratatui TUI:** Dashboard showing PRDs → TS → Loops
7. **Direct API calls:** Call Anthropic API (not shelling out to claude-code)

### Non-Goals

1. Multi-repo coordination (single repo per PRD for now)
2. Process isolation (tokio tasks, not OS processes)
3. Automatic conflict resolution (escalate to user)
4. Web UI (terminal TUI only)
5. Support for non-Anthropic models (Anthropic-first, extensible later)

## Proposed Solution

### Overview

TaskDaemon orchestrates parallel agentic workflows defined in AWL (Agentic Workflow Language):

```
User creates PRD → Decompose to TS (if needed) → Spawn N Loops → Coordinate
                                                           ↓
                                              Each loop: isolated worktree
                                                         tokio task
                                                         API calls
                                                         validation
```

**Three-tier architecture:**

```
┌─────────────────────────────────────────────────────────────────────┐
│                        USER INTERFACE                                │
│  - Ratatui TUI (dashboard, controls)                                 │
│  - CLI (taskdaemon prd create, taskdaemon start, etc.)               │
└────────────────────────────────┬────────────────────────────────────┘
                                 │
┌────────────────────────────────▼────────────────────────────────────┐
│                      TASKDAEMON (Orchestrator)                       │
│  - Loop executor (tokio async)                                       │
│  - Coordinator (Notify/Query/Share)                                  │
│  - Git worktree manager                                              │
│  - AWL template loader                                               │
│  - API client (Anthropic)                                            │
└────────────────────────────────┬────────────────────────────────────┘
                                 │
┌────────────────────────────────▼────────────────────────────────────┐
│                      TASKSTORE (Datastore)                           │
│  - SQLite (query cache, fast lookups)                                │
│  - JSONL (git-tracked source of truth)                               │
│  - Merge driver (conflict-free collaboration)                        │
└─────────────────────────────────────────────────────────────────────┘
```

**Data flow:**

```
┌──────────────────────────────────────────────────────────────────────┐
│                        End-to-End Data Flow                           │
│                                                                       │
│  1. User request                                                      │
│     └─► PRD generation loop (AWL: prd-generation.awl.yaml)           │
│         └─► PRD saved to taskstore (prds.jsonl)                      │
│                                                                       │
│  2. TS decomposition                                                  │
│     └─► Load PRD from taskstore                                      │
│         └─► Decomposition loop (AWL: ts-decompose.awl.yaml)          │
│             └─► TS records + dependency graph saved to taskstore     │
│                                                                       │
│  3. Execution scheduling                                              │
│     └─► Query taskstore for ready TS (no unmet dependencies)         │
│         └─► For each ready TS:                                        │
│             ├─► Create git worktree                                   │
│             ├─► Load workflow template (e.g., rust-dev.awl.yaml)     │
│             ├─► Spawn tokio task (loop executor)                     │
│             └─► Record execution in taskstore                        │
│                                                                       │
│  4. Loop execution (per tokio task)                                   │
│     ├─► Execute AWL before steps                                      │
│     ├─► For each phase in TS:                                         │
│     │   ├─► Execute AWL context steps                                 │
│     │   ├─► Prompt Anthropic API                                      │
│     │   ├─► Validation loop (retry until pass)                        │
│     │   └─► Update execution state in taskstore                       │
│     └─► Execute AWL after steps                                       │
│         └─► Mark TS complete, trigger dependent TS                    │
│                                                                       │
│  5. Coordination events                                               │
│     ├─► MainWatcher detects commit to main                            │
│     │   └─► Notify all loops → pause, rebase, resume                  │
│     ├─► User sends Query to loop                                      │
│     │   └─► Loop responds via channel                                 │
│     └─► Loop A completes                                              │
│         └─► Scheduler spawns Loop B (was blocked on A)                │
└──────────────────────────────────────────────────────────────────────┘
```

### Architecture

**Repository structure:**

```
taskstore/              # Generic storage library (external dependency)
    Public API Contract:
    - Store::open(path) -> Result<Store>
    - Store::create<T: Record>(record) -> Result<String>
    - Store::get<T: Record>(id) -> Result<Option<T>>
    - Store::update<T: Record>(record) -> Result<()>
    - Store::delete<T: Record>(id) -> Result<()>
    - Store::list<T: Record>(filters: &[Filter]) -> Result<Vec<T>>
    - Store::sync() -> Result<()>
    - Record trait (implement for domain types)
    - Filter/FilterOp/IndexValue (for queries)
    - now_ms() helper function

taskdaemon/             # Orchestrator application
├── src/
│   ├── main.rs         # CLI (clap)
│   ├── models/         # Domain types implementing Record trait
│   │   ├── prd.rs      # PRD type + Record impl
│   │   ├── task_spec.rs # TaskSpec type + Record impl
│   │   ├── execution.rs # Execution type + Record impl
│   │   └── dependency.rs # Dependency type + Record impl
│   ├── executor.rs     # Loop execution (tokio)
│   ├── coordinator.rs  # Notify/Query/Share
│   ├── worktree.rs     # Git worktree management
│   ├── awl.rs          # AWL template loading
│   ├── api.rs          # Anthropic API client
│   └── tui.rs          # Ratatui dashboard
├── docs/
│   ├── taskdaemon-design.md
│   ├── awl-schema-design.md
│   ├── coordinator-design.md
│   └── execution-model-design.md
└── Cargo.toml
    [dependencies]
    taskstore = { git = "https://github.com/saidler/taskstore" }
```

**Config structure:**

```
~/.config/taskdaemon/
├── workflows/                    # AWL templates
│   ├── prd-generation.awl.yaml
│   ├── rust-development.awl.yaml
│   ├── python-development.awl.yaml
│   └── typescript-development.awl.yaml
├── taskdaemon.yaml               # Config (API key, settings)
└── .taskstore/                   # Persistent state (managed by TaskStore)
    ├── prds.jsonl                # PRD records (collection_name = "prds")
    ├── task_specs.jsonl          # TaskSpec records (collection_name = "task_specs")
    ├── executions.jsonl          # Execution records (collection_name = "executions")
    ├── dependencies.jsonl        # Dependency records (collection_name = "dependencies")
    ├── taskstore.db              # SQLite cache (auto-managed)
    ├── .gitignore                # Auto-generated by TaskStore
    └── .version                  # Schema version file
```

## Key Concepts

### 1. PRD (Product Requirements Document)

Top-level work unit. Created via:
- User describes feature
- Agent chats to gather context
- Rule of Five generates PRD

**PRD contains:**
- Title, description
- One or more phases
- Success criteria
- Auto-install flag

### 2. TS (Task Spec)

Decomposed chunk of a PRD. Small enough to complete in one loop within context window.

**TS contains:**
- Reference to parent PRD
- Description, requirements
- Dependencies on other TS
- Status (open, in_progress, blocked, closed)

**Example decomposition:**
```
PRD: "Add OAuth authentication"
  ├─ TS-1: Database schema                [no dependencies, can start immediately]
  ├─ TS-2: OAuth endpoints                [blocked until TS-1 completes]
  └─ TS-3: Tests                          [blocked until TS-2 completes]

Dependency semantics:
- TS-2 "blocks on" TS-1 means: Don't start TS-2's loop until TS-1's loop completes
- TS-1 → TS-2 → TS-3 forms a chain (sequential execution)
- If TS-1 and TS-2 have no dependency, they run in parallel
```

**Dependency graph:**
```
TS-1 (schema)
  ↓ blocks
TS-2 (endpoints) ←┐
  ↓ blocks        │ related (no blocking)
TS-3 (tests)  ←───┘

Execution timeline:
t0: Spawn loop for TS-1
t1: TS-1 completes
t1: Spawn loop for TS-2 (was blocked, now ready)
t2: TS-2 completes
t2: Spawn loop for TS-3 (was blocked, now ready)
```

### 3. Loop (Execution Instance)

Running instance of an AWL workflow operating on ONE TS OR a PRD directly.

**Important distinction:**
- If PRD is small: Can skip TS decomposition, run PRD directly
- If PRD is large: Decompose to TS first, one loop per TS

**Loop has:**
- Reference to TS (or PRD if no decomposition)
- Git worktree path
- Workflow template (e.g., rust-development.awl.yaml)
- Phase counter (within AWL workflow phases)
- Iteration counter (validation retries)
- Status (running, paused, completed, failed)
- Variables/context (JSON blob from before/context steps)

### 4. AWL (Agentic Workflow Language)

YAML-based templates defining workflow structure.

**Core construct: Loop**

Simplified example (full spec in [AWL Schema](./awl-schema-design.md)):

```yaml
workflow:
  name: "Rust Development"
  version: "1.0"

  variables:
    validation_cmd: "cargo check && cargo test && cargo clippy"

  # One-time setup
  before:
    - action: read-file
      path: "{worktree}/Cargo.toml"
      bind: cargo_toml

    - action: shell
      command: "cargo check"
      working-dir: "{worktree}"

  # Fresh data each iteration
  context:
    - action: read-ts
      bind: ts_data

  # Iterate over TS phases
  foreach:
    items: "{ts_data.phases}"
    steps:
      # Implementation step
      - action: prompt-agent
        model: "opus-4.5"
        prompt: |
          Implement phase {item.number}: {item.name}

          Requirements: {item.requirements}
          Success criteria: {item.success_criteria}

          Work in: {worktree}

      # Validation loop (nested)
      - action: loop
        name: "Validation"
        foreach:
          items: "range(1, 10)"
          until: "{validation.exit_code == 0}"
          steps:
            - action: shell
              command: "{validation_cmd}"
              working-dir: "{worktree}"
              capture: validation

            - action: conditional
              if: "{validation.exit_code != 0}"
              then:
                - action: prompt-agent
                  model: "opus-4.5"
                  prompt: |
                    Fix these errors:
                    {validation.stderr}

      # Commit phase
      - action: shell
        command: |
          git add .
          git commit -m "feat: {item.name}"
        working-dir: "{worktree}"

  # One-time teardown
  after:
    - action: shell
      command: "cargo build --release"
      working-dir: "{worktree}"

    - action: notify
      event: "loop_completed"
      data:
        loop-id: "{execution_id}"
        ts-id: "{ts_data.id}"
```

**Note:** This is simplified. Full AWL specification includes error handling, timeouts, retry policies, etc.

**Coordination primitives:**
- **Notify:** Broadcast event to all/subset of loops
- **Query:** Request info from a loop
- **Share:** Send data to specific loop(s)
- **Stop:** Halt a loop

### 5. Git Worktrees

Each loop runs in isolated worktree:
```
repo/
├── .git/
├── main branch working dir/
└── .worktrees/
    ├── exec-abc123/  # Loop 1
    ├── exec-def456/  # Loop 2
    └── exec-ghi789/  # Loop 3
```

**Benefits:**
- No file conflicts
- Clean parallel work
- Easy to review per-loop changes
- Automatic branch management

### 6. Proactive Rebase

When someone merges to main:
```
1. Main updated (new commit)
2. Notify all running loops
3. Each loop pauses, rebases, resumes
4. Small frequent rebases > large conflicts
```

## Technology Stack

**Language:** Rust
**Async runtime:** tokio
**CLI:** clap
**Error handling:** eyre
**TUI:** ratatui
**Database:** SQLite (rusqlite)
**Serialization:** serde (YAML, JSON, JSONL)
**Git:** Call git CLI (no libgit2 for now)
**API:** Anthropic API via reqwest

## Design Decisions

| # | Decision | Choice | Rationale |
|---|----------|--------|-----------|
| 1 | Architecture | Two repos: taskstore (lib) + taskdaemon (bin) | Clean separation, taskstore reusable |
| 2 | Concurrency | tokio async/await, not processes | Lower overhead, shared memory, perfect for I/O-bound LLM calls |
| 3 | Storage pattern | SQLite+JSONL+Git (beads/engram pattern) | Proven approach, git-native, fast queries |
| 4 | Merge driver | Include (don't repeat engram's mistake) | Critical for git-backed workflow |
| 5 | Workflow language | YAML (AWL format) | Human-readable, version-controllable |
| 6 | Config format | YAML for humans, JSONL for machines | Clear distinction, appropriate tools |
| 7 | Git coordination | Worktrees + proactive rebase | Isolation + sync, learned from Neuraphage |
| 8 | API calls | Direct (not shelling to claude-code) | Full control, better error handling |
| 9 | TUI library | ratatui | Modern, actively maintained |
| 10 | Naming | taskdaemon (not neuraphage) | Fresh start, clear purpose |

## Implementation Plan

### Phase 1: TaskStore Foundation
**Goal:** Build the datastore library

1. Create taskstore repo
2. SQLite schema (PRDs, TS, Executions, Dependencies)
3. JSONL persistence (append-only)
4. Basic CRUD operations
5. Merge driver for JSONL conflicts
6. Unit tests

**Deliverable:** taskstore crate, can store/query PRDs/TS

### Phase 2: TaskDaemon CLI Skeleton
**Goal:** Basic CLI and config loading

1. Create taskdaemon repo
2. Clap CLI structure (subcommands: init, prd, ts, loop, status)
3. Config loading (~/.config/taskdaemon/taskdaemon.yaml)
4. AWL template loading
5. taskstore integration (git dep)

**Deliverable:** `taskdaemon init` works, loads config

### Phase 3: Single Loop Execution
**Goal:** Run one loop end-to-end

1. Executor module (single-threaded first)
2. API client (Anthropic)
3. Simple AWL interpreter (just Loop construct)
4. Git worktree creation
5. Prompt engineering (implement → validate → iterate)
6. Record execution state

**Deliverable:** Can run one TS through Rust dev workflow

### Phase 4: Multi-Loop with Tokio
**Goal:** Concurrent execution

1. Convert executor to tokio async
2. Spawn N loops concurrently
3. Shared state management (use message passing, not Arc<Mutex>)
   - Each loop gets read-only snapshot of TS
   - Updates sent via channel to state manager
   - Avoids deadlocks, cleaner than shared mutex
4. Task scheduler (respect TS dependencies)

**Deliverable:** Run 5 TS in parallel

**Architecture note:** Prefer actor pattern over shared mutex:
```rust
// State manager task (single owner)
tokio::spawn(async move {
    loop {
        match rx.recv().await {
            StateUpdate::LoopStarted(id) => store.update(...),
            StateUpdate::PhaseComplete(id, phase) => store.update(...),
            // ...
        }
    }
});

// Loop tasks send updates via channel
tx.send(StateUpdate::PhaseComplete(exec_id, 2)).await?;
```

### Phase 5: Coordinator (Notify/Query/Share)
**Goal:** Inter-loop messaging

1. Channel-based message passing
2. Notify broadcast mechanism
3. Query request/response
4. Share point-to-point
5. Proactive rebase on main update

**Deliverable:** Loops can coordinate

### Phase 6: Ratatui TUI
**Goal:** Visual dashboard

1. TUI layout (PRDs → TS → Loops)
2. Status display (phase, iteration, logs)
3. User controls (start, stop, query)
4. Event streaming from loops

**Deliverable:** `taskdaemon tui` shows live status

### Phase 7: PRD Generation & TS Decomposition
**Goal:** Full pipeline

1. PRD generation AWL workflow (Rule of Five)
2. TS decomposition AWL workflow
3. Dependency graph creation
4. Queue management

**Deliverable:** User request → PRD → TS → Loops

## Alternatives Considered

### Alternative 1: Extend Neuraphage

**Description:** Build on existing Neuraphage codebase

**Pros:**
- Already has tokio, git worktrees, TUI
- Some concepts transferable

**Cons:**
- Coupled to old architecture
- engram missing git integration
- Want fresh start with clean slate

**Why not chosen:** Rebuild gives opportunity to get layering right from the start

### Alternative 2: Shell out to Claude Code

**Description:** Use claude-code binary, orchestrate via shell

**Pros:**
- Leverage existing tool
- Less code to write

**Cons:**
- Process overhead
- Less control over API calls
- Harder to coordinate
- Can't easily inspect/modify behavior

**Why not chosen:** Direct API calls give full control

### Alternative 3: Use Existing Beads

**Description:** Use beads (Go) as datastore

**Pros:**
- Mature, battle-tested
- Full git integration

**Cons:**
- Go, not Rust
- Overcomplicated for our needs
- Designed for Gas Town model

**Why not chosen:** Build taskstore purpose-built for our use case

## User Interaction Model

### Creating Work

```bash
# User describes feature
$ taskdaemon prd create

# Agent chats, gathers context
Agent: "What authentication method? OAuth, SAML, or custom?"
User: "OAuth with Google and GitHub providers"
Agent: "Should this support 2FA?"
User: "Yes"

# Agent generates PRD (Rule of Five)
# PRD saved to taskstore

$ taskdaemon prd list
PRD-abc123: Add OAuth authentication [queued]

# Optional: decompose to TS
$ taskdaemon ts decompose PRD-abc123
Created TS-001: Database schema
Created TS-002: OAuth endpoints (depends on TS-001)
Created TS-003: Tests (depends on TS-002)

# Start execution
$ taskdaemon start PRD-abc123
Spawned 1 loop (TS-001 ready)
TS-002, TS-003 blocked on dependencies

# Monitor
$ taskdaemon status
PRD-abc123: Add OAuth [in_progress]
  TS-001: Database schema [running] (phase 2/3, iteration 5)
  TS-002: OAuth endpoints [blocked]
  TS-003: Tests [blocked]

# Or use TUI
$ taskdaemon tui
[Interactive dashboard]
```

### Interacting with Running Loops

**Query a loop:**
```bash
$ taskdaemon loop query exec-abc123 "What files have you modified?"
Loop exec-abc123 response:
  src/db/schema.rs
  migrations/001_add_users_table.sql
```

**Send message to loop:**
```bash
$ taskdaemon loop share exec-abc123 "FYI: auth API changed, check docs/api.md"
Message sent to exec-abc123
```

**Pause/resume:**
```bash
$ taskdaemon loop pause exec-abc123
$ taskdaemon loop resume exec-abc123
```

**Stop loop:**
```bash
$ taskdaemon loop stop exec-abc123
Loop stopped, changes preserved in worktree
```

### TUI Controls

```
┌─────────────────────────────────────────────────────────────────┐
│ TaskDaemon - PRDs: 3  TS: 8  Loops: 5                           │
├─────────────────────────────────────────────────────────────────┤
│ PRD-abc123: Add OAuth [in_progress] ▼                           │
│   ├─ TS-001: Database schema [running]                          │
│   │   └─ exec-abc123: rust-dev.awl (phase 2/3, iter 5)          │
│   │      Status: Running validation                             │
│   │      Files: src/db/schema.rs, migrations/...                │
│   │      [p] pause [q] query [s] stop                           │
│   ├─ TS-002: OAuth endpoints [blocked on TS-001]                │
│   └─ TS-003: Tests [blocked on TS-002]                          │
│                                                                  │
│ PRD-def456: Refactor API [queued]                               │
│                                                                  │
│ Logs:                                                            │
│ [10:32:15] exec-abc123: cargo test passed                       │
│ [10:32:18] exec-abc123: Starting phase 3                        │
│ [10:32:20] MainWatcher: main updated, notifying 5 loops         │
│ [10:32:21] exec-abc123: Pausing for rebase                      │
└─────────────────────────────────────────────────────────────────┘
Keys: [↑↓] navigate [Enter] expand [q] quit [n] new PRD
```

## Cost Model

### API Usage

**Anthropic Opus 4.5 pricing (example):**
- Input: $15 / 1M tokens
- Output: $75 / 1M tokens

**Typical loop:**
- Phase implementation: 50K input, 5K output → $1.13
- Validation iterations (3x): 30K input, 2K output → $0.90
- Total per phase: ~$2

**Full TS (3 phases):** ~$6
**PRD with 5 TS:** ~$30
**Daily usage (10 PRDs):** ~$300

**Cost controls:**
- Max concurrent loops (default: 10)
- Token budget per loop
- Warn when approaching limits
- Support multiple API keys (rotate)

### Disk Usage

**Per loop:**
- Worktree: ~repo size (typically 10-100 MB)
- Execution state: ~1 KB (JSONL)
- SQLite: ~10 KB per loop

**10 concurrent loops:** ~1 GB disk

**Cleanup:**
- Remove worktree on loop completion
- Archive old JSONL (keep last 90 days)
- Vacuum SQLite periodically

## Observability

### Logging

**Structured logs (JSON):**
```json
{
  "timestamp": "2026-01-13T10:32:15Z",
  "level": "info",
  "component": "executor",
  "loop_id": "exec-abc123",
  "event": "phase_completed",
  "phase": 2,
  "duration_ms": 45000
}
```

**Log levels:**
- DEBUG: Detailed execution flow
- INFO: Phase changes, API calls
- WARN: Retries, slow operations
- ERROR: Failures, exceptions

**Log destinations:**
- stdout (interactive mode)
- ~/.config/taskdaemon/logs/ (daemon mode)
- TUI (live streaming)

### Metrics

**Track:**
- Loops active/completed/failed
- API calls per hour
- Token usage
- Phase duration (p50, p95, p99)
- Rebase frequency
- Error rate

**Export:**
- Prometheus metrics (future)
- JSON stats file
- TUI dashboard

### Debugging

**Inspection commands:**
```bash
# View loop state
$ taskdaemon loop inspect exec-abc123

# Dump execution history
$ taskdaemon loop history exec-abc123

# Replay conversation
$ taskdaemon loop replay exec-abc123

# Check worktree status
$ taskdaemon worktree status
```

## Technical Considerations

### Dependencies

**Internal:**
- taskstore (datastore library)

**External:**
- tokio (async runtime)
- clap (CLI)
- eyre (errors)
- ratatui (TUI)
- rusqlite (SQLite)
- serde, serde_yaml, serde_json (serialization)
- reqwest (HTTP client)
- chrono (timestamps)

### Performance

**Concurrency:**
- 10 loops: ~few MB RAM overhead (tokio tasks are lightweight)
- 50 loops: Still single-digit MB overhead
- Bottleneck: LLM API rate limits, not CPU/memory

**Storage:**
- SQLite queries: <10ms typical
- JSONL append: <1ms
- Git operations: 50-500ms (worktree creation, rebase)

**API calls:**
- Anthropic API: 1-30s per call
- This is the critical path (I/O bound)

### Security

**API keys:**
- Store in ~/.config/taskdaemon/taskdaemon.yaml
- File permissions 600
- Never log or expose

**Git operations:**
- Run in user's context
- No privilege escalation
- Sandboxing future enhancement

**User input:**
- Sanitize before passing to LLM
- Validate AWL templates before execution

### Testing Strategy

**Unit tests:**
- taskstore CRUD operations
- AWL parsing
- Coordinator message routing

**Integration tests:**
- Single loop execution
- Multi-loop coordination
- Git worktree isolation

**Manual testing:**
- Real PRD → TS → Loop workflow
- TUI interaction
- Rebase coordination

### Rollout Plan

**Phase 1:** Private use (author only)
**Phase 2:** Share with trusted users
**Phase 3:** Open source (GitHub)
**Phase 4:** Documentation, examples

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Tokio task crashes affect others | Medium | High | Catch panics, restart crashed task, isolate failures |
| Git merge conflicts | High | Medium | Proactive rebase, escalate to user, pause loop |
| API rate limits | High | High | Exponential backoff, queue requests, multiple API keys, fallback to Haiku |
| Context window exhaustion | Medium | Medium | TS sizing heuristics, monitor tokens, truncate if needed |
| SQLite corruption | Low | High | JSONL is source of truth, rebuild DB from JSONL |
| Worktree disk usage | Medium | Low | Cleanup completed loops, warn at 80%, fail at 95% |
| Loop diverges/hallucinates | Medium | High | Validation must pass, user review before merge, stop after N failures |
| Main branch deleted/force-pushed | Low | High | Detect via git, pause all loops, alert user |
| Network failure mid-API call | High | Medium | Retry with backoff, persist request state, resume |
| User deletes worktree manually | Low | Medium | Detect on next operation, mark loop failed, cleanup state |
| Two loops modify same file | Medium | High | Git merge conflict caught during rebase, pause both, alert user |
| Circular TS dependencies | Low | High | Detect during dependency graph build, reject with error |

## Open Questions

- [ ] How to size TS to fit in context window?
- [ ] What's the UX for manually intervening in a loop?
- [ ] Should we support models other than Anthropic?
- [ ] How to handle loops that diverge (hallucinate, go off track)?
- [ ] What's the right default for max concurrent loops?
- [ ] Should AWL support conditionals/branches or just linear loops?
- [ ] How to visualize dependency graph in TUI?
- [ ] Should we support remote execution (cloud VMs)?

## References

**Inspirations:**
- Ralph Wiggum: Iterative autonomous loops
- Gas Town: Multi-agent orchestration
- Beads/Engram: SQLite+JSONL+Git pattern
- Neuraphage: Tokio async, git worktrees, proactive rebase

**Research:**
- [Ralph Wiggum](~/.config/pais/research/tech/ralph-wiggum/2026-01-12.md)
- [Gas Town](~/.config/pais/tech/researcher/steve-yegge-gas-town-2026-01-13.md)
- [Engram vs Beads](~/.config/pais/research/tech/engram-vs-beads/2026-01-12-comparison.md)
- [Accidental Minimalism](~/.config/pais/research/tech/engram-vs-beads/2026-01-12-accidental-minimalism.md)

**Neuraphage docs:**
- [Neuraphage Design](~/repos/neuraphage/neuraphage/docs/neuraphage-design.md)
- [Git Worktree Integration](~/repos/neuraphage/neuraphage/docs/git-worktree-integration-design.md)
- [Proactive Rebase](~/repos/neuraphage/neuraphage/docs/proactive-rebase-design.md)

---

## Review Log

### Review Pass 1: Completeness (2026-01-13)

**Sections checked:**
- ✓ Summary, Problem Statement, Solution, Architecture
- ✓ Alternatives (3), Technical Considerations, Risks, Open Questions
- ✓ Implementation Plan (7 phases), References

**Gaps identified and filled:**

1. **User Interaction Model** - Added complete section showing:
   - How users create PRDs (interactive chat)
   - CLI commands for managing loops
   - TUI mockup with keyboard controls
   - In-flight loop interaction (query, share, pause/resume)

2. **Cost Model** - Added analysis of:
   - API usage costs per phase/TS/PRD
   - Daily usage estimates
   - Cost controls (concurrent limit, token budget)
   - Disk usage per loop
   - Cleanup strategies

3. **Observability** - Added coverage of:
   - Structured JSON logging
   - Log levels and destinations
   - Metrics tracking (loops, API, tokens, duration)
   - Debugging commands (inspect, history, replay)

**Assessment:** Document now complete with all major sections filled.

### Review Pass 2: Correctness (2026-01-13)

**Technical accuracy check:**

**Issues found and corrected:**

1. **Loop can run on PRD or TS**
   - Original: Implied loop always runs on TS
   - Corrected: Clarified loops can run on PRD directly if no decomposition
   - Added: Distinction between small PRDs (no TS) and large PRDs (decompose)

2. **AWL example too abstract**
   - Original: Showed comments instead of actual actions
   - Corrected: Provided concrete YAML with actual action types
   - Added: Variables, conditional, nested loops, bind syntax
   - Added: Reference to full spec in linked document

3. **Dependency semantics unclear**
   - Original: Said "depends on" without explaining execution model
   - Corrected: Clarified "blocks on" semantics
   - Added: Execution timeline showing when loops spawn
   - Added: Diagram showing dependency types (blocks vs related)

4. **Shared state concurrency model wrong**
   - Original: Suggested Arc<Mutex<Store>> (can deadlock)
   - Corrected: Message passing via channels (actor pattern)
   - Added: Code example showing state manager task
   - Rationale: Avoids deadlocks, cleaner architecture

**Assessment:** All major logical errors fixed. Design is now technically sound.

### Review Pass 3: Edge Cases (2026-01-13)

**Failure modes identified and mitigated:**

Expanded risks table from 6 to 12 risks:
- Added: Loop divergence/hallucination (validation + user review)
- Added: Main branch deleted/force-pushed (detect + pause)
- Added: Network failure mid-API call (retry + persist state)
- Added: Manual worktree deletion (detect + cleanup)
- Added: Two loops modify same file (conflict during rebase)
- Added: Circular TS dependencies (detect during build)

Enhanced existing mitigations with specific strategies.

**Assessment:** Major edge cases covered with concrete mitigation strategies.

### Review Pass 4: Architecture (2026-01-13)

**System-level design improvements:**

1. **Added end-to-end data flow diagram**
   - Shows complete pipeline from user request to execution
   - Clarifies PRD → TS → Loop relationship
   - Documents coordination event flow
   - Makes system behavior explicit

2. **Verified architectural decisions**
   - Two-repo structure (taskstore + taskdaemon) is sound
   - Message passing for concurrency (not shared mutex) is correct
   - SQLite+JSONL+Git pattern well-established (beads/engram proven)
   - Git worktrees provide clean isolation

3. **Scalability analysis**
   - 10-50 loops feasible in one process
   - Bottleneck is API rate limits, not system resources
   - State management via channels scales well

**Assessment:** Architecture is sound, scales appropriately, clear separation of concerns.

### Review Pass 5: Clarity (2026-01-13)

**Implementability check:**

1. **Can someone build this?** YES
   - Complete architecture with data flow
   - Technology stack specified
   - Implementation plan (7 phases)
   - Example AWL workflow
   - Concrete CLI commands

2. **Are concepts clearly defined?** YES
   - 6 key concepts explained with examples
   - Terminology consistent throughout
   - Distinctions clear (PRD vs TS vs Loop)

3. **Is the scope clear?** YES
   - Goals explicitly listed
   - Non-goals explicitly stated
   - Alternatives considered with rationale

4. **Can this be reviewed?** YES
   - Links to 4 deep-dive documents
   - References to inspiration sources
   - Open questions acknowledged

**Final assessment:** Document is implementation-ready. Converged after 5 passes.

---

## Document Status: COMPLETE ✓

This top-level design document has undergone full Rule of Five review. The following linked documents provide deep-dives:

- [AWL Schema Design](./awl-schema-design.md) - Workflow language specification
- [TaskStore](https://github.com/saidler/taskstore) - Generic storage library with SQLite+JSONL+Git pattern
- [Coordinator Protocol](./coordinator-design.md) - Inter-loop messaging
- [Execution Model](./execution-model-design.md) - Loop lifecycle, crash recovery
- [Developer Guide](./developer-guide.md) - Implementation details, naming conventions, validation patterns
