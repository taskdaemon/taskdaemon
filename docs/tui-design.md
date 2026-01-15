# Design Document: TaskDaemon TUI (Terminal User Interface)

**Author:** Scott A. Idler
**Date:** 2026-01-14
**Status:** Active
**Review Passes:** 5/5

## Summary

TaskDaemon TUI is a ratatui-based terminal interface for monitoring and controlling concurrent Ralph loops. Inspired by k9s's multi-level navigation model, it provides hierarchical views (Plans → Specs → Loops) with drill-down navigation, real-time status updates, and comprehensive keyboard controls for managing the full workflow lifecycle from draft Plan creation to execution completion.

## Problem Statement

### Background

TaskDaemon orchestrates N concurrent Ralph loops working on complex software projects. Each loop:
- Executes in an isolated git worktree
- Follows a loop definition (YAML config)
- Makes iterative progress with fresh context windows
- Coordinates with other loops via Alert/Share/Query events
- Persists state to TaskStore (SQLite + JSONL)

The system manages three levels of work:
1. **Plans** - High-level features (formerly PRDs)
2. **Specs** - Atomic work units with dependencies (formerly Task Specs)
3. **Executions (Loops)** - Running instances of Ralph loop workflows

Without a proper TUI, users must:
- Poll CLI commands for status (`taskdaemon status`)
- Parse JSON/JSONL files manually
- Lack real-time visibility into loop progress
- Miss critical events (failures, rebase notifications)
- Struggle to debug failing loops
- Cannot quickly navigate between abstraction levels

### Problem

**How do we design a TUI that:**
- Provides hierarchical navigation matching mental models (Plan → Spec → Loop)
- Shows real-time updates without overwhelming users
- Enables control operations (pause, stop, query)
- Supports debugging (view logs, files, API calls)
- Scales to 50+ concurrent loops
- Maintains responsiveness with frequent state changes
- Follows familiar patterns (k9s, htop, etc.)

### Goals

1. **Multi-level navigation** - Users can drill down from Plans → Specs → individual loops
2. **Real-time monitoring** - Live updates as loops execute, without manual refresh
3. **Plan lifecycle management** - Visual distinction between draft/ready/in-progress/complete states
4. **Loop observability** - View command output, logs, file changes, API calls
5. **Control operations** - Pause, resume, stop, query loops directly from TUI
6. **k9s-inspired UX** - Familiar command-mode navigation (`:plans`, `:loops`)
7. **Multiple display modes** - Grid view (multiple loops) and focus view (single loop)
8. **Performance** - 60 FPS rendering, <100ms input latency even with 50 loops

### Non-Goals

1. **Web UI** - Terminal-only (no browser-based interface)
2. **Graphical workflow editor** - No drag-and-drop DAG editing
3. **Embedded text editor** - No in-place editing of Plan/Spec markdown
4. **Historical analysis** - No time-series charts or trend analysis (future feature)
5. **Multi-repo view** - Single repo per TUI instance
6. **Collaborative features** - No multi-user cursors or shared view state

## Proposed Solution

### Overview

TaskDaemon TUI is a full-screen terminal application built with ratatui that provides:

1. **Four primary views:**
   - **Plan View** (default) - List of all Plans with status filters
   - **Spec View** - Specs for a selected Plan, showing dependencies
   - **Loop View** - Active execution loops (grid or focus mode)
   - **Logs View** - Aggregated logs across all loops

2. **Navigation model:**
   - k9s-style command mode (`:plans`, `:specs`, `:loops`)
   - Drill-down with `Enter` (Plan → Spec → Loop)
   - Breadcrumb trail with `Esc` to go back
   - Vim-style movement keys (`j`/`k`, `↑`/`↓`)

3. **Real-time updates:**
   - Subscribe to TaskStore events via channels
   - Receive Coordinator notifications (iteration changes, completions)
   - Incremental UI updates (only redraw changed regions)

4. **Plan lifecycle integration:**
   - Draft Plans stored as `.md` files, not auto-decomposed
   - User marks Plan "ready" → triggers Spec decomposition
   - Visual status indicators (draft○, ready●, in_progress⚙, complete✓)

5. **Markdown storage model:**
   - JSONL stores metadata (id, status, timestamps)
   - `.md` files store human-readable content
   - Both committed to git

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    TaskDaemon Process                        │
│                                                              │
│  ┌────────────────┐                 ┌───────────────────┐  │
│  │   TUI Task     │◄────events─────│  State Manager    │  │
│  │  (ratatui)     │                 │  (owns Store)     │  │
│  └────────┬───────┘                 └───────────────────┘  │
│           │                                   ▲              │
│           │ controls                          │ updates      │
│           ▼                                   │              │
│  ┌────────────────────────────────────────────┴──────────┐ │
│  │              Loop Executor                             │ │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐            │ │
│  │  │ Loop A   │  │ Loop B   │  │ Loop C   │            │ │
│  │  └──────────┘  └──────────┘  └──────────┘            │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
│  ┌──────────────────────────────────────────────────────┐  │
│  │              TaskStore (SQLite + JSONL)              │  │
│  │  - plans.jsonl          - plans/add-oauth.md         │  │
│  │  - specs.jsonl          - specs/oauth-db.md          │  │
│  │  - executions.jsonl                                   │  │
│  └──────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘

Data Flow:
1. TUI reads initial state from Store
2. TUI subscribes to event channel
3. Loops/Coordinator publish events
4. TUI receives events, updates display
5. User input triggers control messages
6. Control messages sent to Loop Executor
7. Executor updates state, publishes events
```

**Component responsibilities:**

- **TUI Task:** Renders UI, handles keyboard input, manages view state
- **State Manager:** Owns TaskStore, processes queries, publishes events
- **Loop Executor:** Spawns/controls loops, sends progress events
- **Event Channel:** mpsc channel for real-time updates (bounded, capacity 1000)

### Data Model

#### View Hierarchy State

```rust
#[derive(Debug, Clone)]
pub enum View {
    Plans,
    Specs(String),           // Plan ID
    LoopsGrid,
    LoopFocus(String),       // Execution ID
    Logs,
}

pub struct AppState {
    // Navigation
    current_view: View,
    view_history: Vec<View>,  // Breadcrumb trail

    // Selection state per view
    plans_selected: usize,
    specs_selected: usize,
    loops_selected: usize,

    // Data (cached from TaskStore)
    plans: Vec<PlanSummary>,
    specs: HashMap<String, Vec<SpecSummary>>,  // plan_id → Spec list
    executions: Vec<ExecutionSummary>,

    // Filtering
    plan_status_filter: Option<PlanStatus>,
    spec_status_filter: Option<SpecStatus>,
    search_query: Option<String>,

    // Command mode
    command_mode: bool,
    command_buffer: String,

    // Real-time event buffer
    recent_events: VecDeque<TuiEvent>,  // Last 100 events
}
```

#### Plan Summary (for display)

```rust
#[derive(Debug, Clone)]
pub struct PlanSummary {
    pub id: String,
    pub title: String,
    pub status: PlanStatus,
    pub spec_count: usize,         // Number of specs
    pub spec_complete: usize,      // Completed specs
    pub file: String,              // Markdown filename
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanStatus {
    Draft,        // User is iterating, not ready
    Ready,        // Approved, Specs not yet created
    InProgress,   // At least one Spec running
    Complete,     // All Specs complete
    Failed,       // At least one Spec failed
    Cancelled,    // User cancelled
}
```

#### Spec Summary

```rust
#[derive(Debug, Clone)]
pub struct SpecSummary {
    pub id: String,
    pub title: String,
    pub status: SpecStatus,
    pub dependencies: Vec<String>,  // Spec IDs
    pub assigned_to: Option<String>,  // Execution ID if running
    pub file: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecStatus {
    Pending,
    Blocked,      // Dependencies not met
    Running,
    Complete,
    Failed,
}
```

#### Execution Summary

```rust
#[derive(Debug, Clone)]
pub struct ExecutionSummary {
    pub id: String,
    pub spec_id: String,
    pub spec_title: String,         // Cached for display
    pub status: ExecStatus,
    pub loop_type: String,          // e.g., "spec-implementation"
    pub iteration_count: u32,
    pub started_at: i64,
    pub updated_at: i64,
    pub error_message: Option<String>,

    // For loop focus view
    pub recent_output: String,    // Last 1000 chars of command output
    pub modified_files: Vec<String>,
}
```

#### TUI Events (Real-time updates)

```rust
#[derive(Debug, Clone)]
pub enum TuiEvent {
    // Plan lifecycle
    PlanCreated { plan_id: String, title: String },
    PlanStatusChanged { plan_id: String, old_status: PlanStatus, new_status: PlanStatus },

    // Spec events
    SpecsCreated { plan_id: String, count: usize },
    SpecStatusChanged { spec_id: String, status: SpecStatus },

    // Execution events
    LoopStarted { exec_id: String, spec_id: String },
    LoopIterationChanged { exec_id: String, iteration: u32 },
    LoopOutput { exec_id: String, output: String },
    LoopCompleted { exec_id: String, success: bool },
    LoopPaused { exec_id: String, reason: String },
    LoopResumed { exec_id: String },

    // Coordinator events
    MainBranchUpdated { commit_sha: String },
    RebaseTriggered { exec_ids: Vec<String> },

    // User actions
    CommandExecuted { command: String, result: String },
}
```

### View Specifications

#### 1. Plan View (Default)

**Layout:**

```
┌─────────────────────────────────────────────────────────────────┐
│ TaskDaemon v0.1.0 :: Plans                              :plans  │
│ Showing: 6 of 8 Plans  [3 ready, 2 draft, 1 in-progress]       │
├─────────────────────────────────────────────────────────────────┤
│ STATUS    │ NAME                          │ SPECS   │ UPDATED   │
├───────────┼───────────────────────────────┼─────────┼───────────┤
│ ready  ●  │ add-oauth-authentication      │ 3/3     │ 2h ago    │
│ ready  ●  │ refactor-api-layer           │ 5/5     │ 4h ago    │
│ draft  ○  │ database-migration-tool      │ -       │ 1d ago    │
│ draft  ○  │ implement-caching-layer      │ -       │ 3d ago    │
│ in_prog ⚙ │ websocket-support            │ 4/7 ✓2  │ updating..│
│ ready  ●  │ upgrade-dependencies         │ 2/2     │ 1w ago    │
├───────────┴───────────────────────────────┴─────────┴───────────┤
│ [Enter] drill down  [d] describe  [r] ready  [c] cancel [/] find│
│ [Space] toggle status  [n] new Plan                             │
├─────────────────────────────────────────────────────────────────┤
│ RECENT EVENTS                                                    │
│ 10:32:15 Loop exec-abc123 completed iteration 7                 │
│ 10:32:20 Main branch updated, rebasing 5 loops                  │
│ 10:32:21 Loop exec-abc123 paused for rebase                     │
└─────────────────────────────────────────────────────────────────┘
```

**Status indicators:**
- `draft ○` - Gray, Plan not yet approved
- `ready ●` - Green, awaiting execution
- `in_prog ⚙` - Yellow/blue, active work
- `complete ✓` - Green, all done
- `failed ✗` - Red, at least one failure
- `cancelled ⊗` - Gray, user cancelled

**Specs column format:**
- `3/3` - 3 Specs created, all pending
- `4/7 ✓2` - 7 Specs total, 4 created, 2 complete
- `-` - Not yet decomposed

**Keyboard shortcuts:**
- `↑`/`k`, `↓`/`j` - Navigate list
- `Enter` - Drill down to Spec view for selected Plan
- `d` - Show Plan markdown in side panel
- `r` - Mark selected draft Plan as "ready" (triggers Spec decomposition)
- `c` - Cancel Plan (prompts for confirmation)
- `Space` - Quick toggle status (draft ↔ ready, or pause/resume in-progress)
- `n` - Create new Plan (launches interactive agent)
- `/` - Search/filter mode
- `:` - Command mode

#### 2. Spec View (Drill-down from Plan)

**Layout:**

```
┌─────────────────────────────────────────────────────────────────┐
│ TaskDaemon :: Plan: add-oauth-authentication            :specs  │
│ [Esc] back to Plans  │  File: plans/add-oauth-authentication.md │
├─────────────────────────────────────────────────────────────────┤
│ STATUS    │ SPEC                         │ DEPS  │ PROGRESS    │
├───────────┼──────────────────────────────┼───────┼─────────────┤
│ complete ✓│ oauth-database-schema        │ -     │ 100%        │
│           │   File: specs/oauth-database-schema.md              │
│           │   Completed: 2h ago                                 │
├───────────┼──────────────────────────────┼───────┼─────────────┤
│ running ⚙ │ oauth-endpoints              │ S-1   │ Iter 5      │
│           │   File: specs/oauth-endpoints.md                    │
│           │   Loop: exec-abc123  Type: spec-implementation      │
│           │   Status: Running validation                        │
├───────────┼──────────────────────────────┼───────┼─────────────┤
│ blocked 🔒│ oauth-tests                  │ S-2   │ 0%          │
│           │   File: specs/oauth-tests.md                        │
│           │   Waiting for: oauth-endpoints                      │
├───────────┴──────────────────────────────┴───────┴─────────────┤
│ [Enter] view loop  [d] describe Spec  [l] view logs  [g] graph │
├─────────────────────────────────────────────────────────────────┤
│ DEPENDENCY GRAPH                                                │
│ oauth-database-schema (✓)                                       │
│   └─► oauth-endpoints (⚙)                                       │
│        └─► oauth-tests (🔒)                                     │
└─────────────────────────────────────────────────────────────────┘
```

**Features:**
- Expandable rows showing more detail
- Dependency graph visualization (ASCII art)
- Color-coded status (green=complete, yellow=running, gray=blocked)
- Direct link to markdown files

**Keyboard shortcuts:**
- `Esc` - Back to Plan view
- `Enter` - View loop for running Spec (jumps to Loop Focus)
- `d` - Show Spec markdown content in panel
- `l` - Show logs for this Spec's loop
- `g` - Toggle dependency graph view
- `e` - Edit Spec markdown (opens $EDITOR)

#### 3. Loop View - Grid Mode

**Layout (2x2 grid):**

```
┌─────────────────────────────────────────────────────────────────┐
│ TaskDaemon :: Loops [5 running, 2 paused]              :loops  │
├─────────────────────────────────────────────────────────────────┤
│ exec-abc123 [oauth-endpoints]   │ exec-def456 [api-refactor]   │
│ Type: spec-impl  Iter: 5        │ Type: spec-impl  Iter: 12    │
│ ⚙ Running validation            │ ⚙ Implementing core logic    │
│                                  │                               │
│ $ cargo test                     │ $ cargo check                │
│ running 8 tests                  │ Checking lib v0.1.0          │
│ test auth::test_jwt ... ok       │ Compiling...                 │
│ test auth::test_session ... FAIL │ Finished test [unoptimized]  │
│   assertion failed at src/...    │                              │
│                                  │                               │
│ Files: 3 modified                │ Files: 5 modified            │
├──────────────────────────────────┼──────────────────────────────┤
│ exec-ghi789 [ws-support]        │ exec-jkl012 [cache-layer]    │
│ Type: spec-impl  Iter: 3        │ Type: spec-impl  Iter: 1     │
│ ⚙ Fixing test failures          │ ⚙ Writing initial impl       │
│                                  │                               │
│ $ cargo test --test ws_tests    │ $ cargo build                │
│ running 5 tests                  │ Compiling cache v0.1.0       │
│ test ws::connect ... ok          │ Finished dev [unoptimized]   │
│ test ws::disconnect ... ok       │                              │
│ test ws::send_message ... ok     │                              │
│ test ws::recv_message ... ok     │                              │
│ test ws::reconnect ... ok        │                              │
│                                  │                               │
│ Files: 2 modified                │ Files: 8 modified            │
├─────────────────────────────────────────────────────────────────┤
│ [Enter] focus  [p] pause  [s] stop  [q] query  [1-9] jump     │
└─────────────────────────────────────────────────────────────────┘
```

**Grid configurations:**
- 1x1: Single loop (same as focus mode)
- 2x1: Two loops side-by-side
- 2x2: Four loops (default)
- 3x2: Six loops (for ultra-wide terminals)

**Auto-scroll:** Each pane auto-scrolls to show latest output (last 10 lines)

**Color coding:**
- Green border: Tests passing
- Red border: Tests failing
- Yellow border: Working (no test results yet)
- Blue border: Paused

**Keyboard shortcuts:**
- `Enter` - Focus on selected loop (full screen)
- `Tab` - Cycle between panes
- `1-9` - Jump directly to loop N
- `p` - Pause selected loop
- `s` - Stop selected loop (prompts for confirmation)
- `q` - Query loop (prompts for question)
- `[`/`]` - Change grid layout (fewer/more panes)

#### 4. Loop View - Focus Mode

**Layout:**

```
┌─────────────────────────────────────────────────────────────────┐
│ TaskDaemon :: Loop exec-abc123                                  │
│ Spec: oauth-endpoints  │  Type: spec-impl  │  Iter: 5  │  ⚙    │
│ [Esc] back to grid   │  Tabs: [O]utput [L]ogs [F]iles [A]pi [S]│
├─────────────────────────────────────────────────────────────────┤
│ CURRENT ACTIVITY                                                │
│ Running validation (iteration 5)                                │
│                                                                  │
│ COMMAND OUTPUT (last 50 lines)                                  │
│ $ cargo test                                                     │
│   Compiling oauth-service v0.1.0                                │
│   Finished test [unoptimized + debuginfo] target(s) in 2.34s   │
│   Running unittests src/lib.rs                                  │
│                                                                  │
│ running 8 tests                                                  │
│ test auth::test_jwt_generation ... ok                           │
│ test auth::test_jwt_validation ... ok                           │
│ test auth::test_session_create ... ok                           │
│ test auth::test_session_destroy ... ok                          │
│ test auth::test_invalid_token ... FAILED                        │
│                                                                  │
│ failures:                                                        │
│ ---- auth::test_invalid_token stdout ----                       │
│ thread 'auth::test_invalid_token' panicked at 'assertion failed:│
│ `(left == right)`                                               │
│   left: `Err(InvalidToken)`,                                    │
│  right: `Err(ExpiredToken)`', src/auth.rs:89:5                  │
│                                                                  │
│ test result: FAILED. 7 passed; 1 failed; 0 ignored; 0 measured │
│                                                                  │
│ NEXT ACTION                                                      │
│ ► Sending error to agent for fix attempt...                     │
│   Prompt tokens: 12,345  Output tokens: 1,234                   │
│   Model: claude-sonnet-4-5                                      │
│   Timeout: 30s                                                   │
│                                                                  │
├─────────────────────────────────────────────────────────────────┤
│ FILES MODIFIED (6)                                              │
│ M  src/auth.rs (+45, -12)                                       │
│ M  src/lib.rs (+3, -1)                                          │
│ A  tests/auth_test.rs (+89)                                     │
│ M  Cargo.toml (+2)                                              │
│ M  src/session.rs (+23, -8)                                     │
│ M  src/token.rs (+67, -34)                                      │
├─────────────────────────────────────────────────────────────────┤
│ [p] pause  [s] stop  [q] query loop  [r] rebase now            │
└─────────────────────────────────────────────────────────────────┘
```

**Tab views:**
- `[O]utput` (default) - Live command output
- `[L]ogs` - Structured logs (tracing events)
- `[F]iles` - Git diff of modified files
- `[A]pi` - Recent API calls (prompts/responses/tokens)
- `[S]tate` - Current loop state variables

**Output tab:**
- Auto-scroll to bottom
- Last 50 lines visible
- Syntax highlighting for test output
- Progress indicators for long-running commands

**Logs tab:**
```
┌─────────────────────────────────────────────────────────────────┐
│ LOGS (Filtered: exec-abc123, Level: INFO+)                     │
├──────────┬─────────────────────────────────────────────────────┤
│ 10:30:15 │ INFO  Starting iteration 5                          │
│ 10:30:16 │ DEBUG API call: prompt-agent (12,345 tokens)        │
│ 10:31:42 │ INFO  Agent completed implementation                │
│ 10:31:43 │ INFO  Running validation: cargo check               │
│ 10:31:45 │ INFO  Validation passed: cargo check                │
│ 10:31:45 │ INFO  Running validation: cargo test                │
│ 10:31:58 │ ERROR Test failed: test_invalid_token               │
│ 10:31:58 │ WARN  Retry iteration 5: Sending error to agent     │
└──────────┴─────────────────────────────────────────────────────┘
```

**Files tab (git diff):**
```
┌─────────────────────────────────────────────────────────────────┐
│ FILES: src/auth.rs (+45, -12)                                   │
├─────────────────────────────────────────────────────────────────┤
│   35 │ pub fn validate_token(token: &str) -> Result<Claims> { │
│   36 │     let secret = env::var("JWT_SECRET")?;               │
│ - 37 │     decode::<Claims>(token, secret.as_ref(), ...)       │
│ + 37 │     let validation = Validation::default();             │
│ + 38 │     let token_data = decode::<Claims>(                  │
│ + 39 │         token,                                           │
│ + 40 │         &DecodingKey::from_secret(secret.as_ref()),     │
│ + 41 │         &validation,                                     │
│ + 42 │     )?;                                                  │
│ + 43 │     Ok(token_data.claims)                               │
│   44 │ }                                                        │
└─────────────────────────────────────────────────────────────────┘
```

**API tab:**
```
┌─────────────────────────────────────────────────────────────────┐
│ API CALLS (last 5)                                              │
├──────────┬──────────────────────────────────────────────────────┤
│ 10:30:16 │ Model: sonnet-4-5  Duration: 86s                    │
│          │ Tokens: 12,345 in / 1,234 out  Cost: $0.21          │
│          │ Prompt: "Implement OAuth endpoints..."              │
│          │ Response: "I'll implement the OAuth endpoints..."   │
│          │ [v] View full prompt/response                       │
├──────────┼──────────────────────────────────────────────────────┤
│ 10:31:58 │ Model: sonnet-4-5  Duration: 42s                    │
│          │ Tokens: 8,901 in / 892 out  Cost: $0.15             │
│          │ Prompt: "Fix test failure: test_invalid_token..."   │
│          │ Response: "The test is failing because..."          │
│          │ [v] View full prompt/response                       │
└──────────┴──────────────────────────────────────────────────────┘
```

**State tab (loop state variables):**
```
┌─────────────────────────────────────────────────────────────────┐
│ LOOP STATE VARIABLES                                            │
├────────────────────────┬────────────────────────────────────────┤
│ worktree               │ /tmp/taskdaemon/worktrees/exec-abc123  │
│ execution_id           │ exec-abc123                             │
│ spec_id                │ spec-550e8400                           │
│ loop_type              │ spec-implementation                     │
│ iteration_count        │ 5                                       │
│ validation_cmd         │ cargo check && cargo test && clippy     │
│ cargo_toml             │ { package: { name: "oauth-service", ...│
│ git_status             │ M src/auth.rs\nM src/lib.rs\n...       │
│ check_output           │ Finished dev [unoptimized + debuginfo] │
└────────────────────────┴────────────────────────────────────────┘
```

**Keyboard shortcuts:**
- `Esc` - Back to grid view
- `o`, `l`, `f`, `a`, `s` - Switch tabs
- `p` - Pause loop
- `s` - Stop loop
- `q` - Query loop (prompts for question)
- `r` - Force rebase now (don't wait for auto-rebase)
- `↑`/`↓` or scroll wheel - Scroll content
- `g`/`G` - Jump to top/bottom

#### 5. Logs View (Aggregated)

**Layout:**

```
┌─────────────────────────────────────────────────────────────────┐
│ TaskDaemon :: Logs (All loops)                         :logs   │
│ Filter: [All] [ERROR] [WARN] [INFO] [DEBUG]   Search: _        │
├──────────┬──────────┬─────────────────────────────────────────┬┤
│ TIME     │ LOOP     │ LEVEL │ MESSAGE                         ││
├──────────┼──────────┼───────┼─────────────────────────────────┼┤
│ 10:30:15 │ abc123   │ INFO  │ Starting iteration 5            ││
│ 10:30:16 │ abc123   │ DEBUG │ API call: 12,345 tokens         ││
│ 10:31:42 │ abc123   │ INFO  │ Agent completed                 ││
│ 10:31:45 │ abc123   │ INFO  │ Validation: cargo check OK      ││
│ 10:31:58 │ abc123   │ ERROR │ Test failed: test_invalid_token ││
│ 10:32:01 │ def456   │ INFO  │ Starting iteration 1            ││
│ 10:32:03 │ def456   │ DEBUG │ Reading state variables         ││
│ 10:32:15 │ abc123   │ INFO  │ Iteration 5 completed           ││
│ 10:32:20 │ MainW... │ WARN  │ Main branch updated, rebasing   ││
│ 10:32:21 │ abc123   │ WARN  │ Paused for rebase               ││
│ 10:32:21 │ def456   │ WARN  │ Paused for rebase               ││
│ 10:32:22 │ ghi789   │ WARN  │ Paused for rebase               ││
│ 10:32:30 │ abc123   │ INFO  │ Rebase complete, resuming       ││
├─────────────────────────────────────────────────────────────────┤
│ [f] filter level  [/] search  [c] clear  [e] export            │
└─────────────────────────────────────────────────────────────────┘
```

**Features:**
- Real-time log streaming
- Color-coded by level (ERROR=red, WARN=yellow, INFO=white, DEBUG=gray)
- Click on loop ID to jump to that loop's focus view
- Filter by level (checkbox UI)
- Search/highlight text
- Export to file

**Keyboard shortcuts:**
- `f` - Toggle filter level checkboxes
- `/` - Search mode
- `c` - Clear logs (prompts for confirmation)
- `e` - Export logs to file (prompts for filename)
- `↑`/`↓` - Scroll
- `g`/`G` - Jump to top/bottom
- `Enter` on log line - Jump to source loop

### Command Mode (k9s-style)

Press `:` to enter command mode. A command bar appears at the bottom:

```
┌─────────────────────────────────────────────────────────────────┐
│ [Current view content]                                          │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
:plans_
```

**Available commands:**
- `:plans` - Jump to Plan view
- `:specs` - Jump to Spec view (requires Plan context or prompts for selection)
- `:loops` - Jump to Loop grid view
- `:logs` - Jump to Logs view
- `:help` or `:?` - Show help screen
- `:quit` or `:q` - Exit TUI
- `:exec <exec-id>` - Jump to specific loop focus view
- `:plan <plan-name>` - Jump to specific Plan (fuzzy search)
- `:search <query>` - Search across all views
- `:filter status=ready` - Apply filter
- `:export logs <file>` - Export logs to file

**Autocomplete:**
- Tab completion for commands
- Fuzzy search for IDs/names
- Recent command history (↑/↓)

### Plan Lifecycle & Markdown Integration

#### Storage Model

```
.taskstore/
├── plans.jsonl              # Metadata only
├── specs.jsonl              # Metadata only
├── plans/                   # Human-readable content
│   ├── add-oauth-authentication.md
│   └── refactor-api-layer.md
└── specs/                   # Human-readable content
    ├── oauth-database-schema.md
    └── oauth-endpoints.md
```

**plans.jsonl entry:**
```jsonl
{"id":"plan-550e8400","title":"Add OAuth Authentication","status":"draft","created_at":1704067200000,"updated_at":1704067200000,"file":"add-oauth-authentication.md","review_passes":5}
```

**plans/add-oauth-authentication.md:**
```markdown
# Plan: Add OAuth Authentication

**Status:** Draft
**Created:** 2026-01-14
**Review Passes:** 5/5

## Summary
Implement JWT-based authentication...

## Goals
- Support Google and GitHub OAuth
- Session management
...
```

#### Plan Status Flow

```
                    ┌─────────┐
                    │  Draft  │
                    └────┬────┘
                         │ User marks "ready"
                         ▼
                    ┌─────────┐
                    │  Ready  │──────┐
                    └────┬────┘      │ User cancels
                         │           ▼
                         │      ┌───────────┐
      Spawn first loop   │      │ Cancelled │
                         ▼      └───────────┘
                 ┌────────────┐
                 │ InProgress │
                 └─────┬──────┘
                       │
          ┌────────────┼────────────┐
          │            │            │
   All Specs complete  Some Specs fail  User cancels
          │            │            │
          ▼            ▼            ▼
     ┌─────────┐  ┌────────┐  ┌───────────┐
     │Complete │  │ Failed │  │ Cancelled │
     └─────────┘  └────────┘  └───────────┘
```

**Transitions:**
- `draft → ready`: User presses `r` in TUI or runs `taskdaemon plan ready <id>`
  - Triggers Spec decomposition (async agent workflow)
  - Creates N spec files in `specs/`
  - Updates Plan status to "ready"
  - Does NOT immediately spawn loops (waits for explicit start)

- `ready → in_progress`: User runs `taskdaemon start <plan-id>` or presses `s` in TUI
  - Spawns loops for all ready Specs (respects dependencies)

- `in_progress → complete`: Last Spec completes successfully
  - Automatic cascade (Execution → Spec → Plan)

- `in_progress → failed`: At least one Spec fails permanently
  - User must decide: retry, cancel, or fix manually

- `* → cancelled`: User cancels Plan
  - Stops all running loops
  - Preserves worktrees and progress (doesn't delete)

#### Creating a Plan (TUI Flow)

1. User presses `n` in Plan view
2. TUI prompts: "Plan Title: _"
3. User types title, presses Enter
4. TUI spawns agent interaction (Rule of Five Plan generation)
5. Agent chats with user (in side panel or separate dialog)
6. Agent generates Plan markdown (5 review passes)
7. Plan saved to `plans/<title>.md` with status "draft"
8. TUI returns to Plan view, new draft visible

**Alternative:** User can create `.md` file manually in `plans/`, then run `taskdaemon plan import <file>`

#### Marking Plan Ready (TUI Flow)

1. User selects draft Plan, presses `r`
2. TUI prompts: "Mark 'add-oauth-authentication' as ready? This will decompose to Specs. [y/N]"
3. User confirms
4. TUI shows spinner: "Decomposing Plan to Specs..."
5. Background agent workflow:
   - Reads Plan markdown
   - Generates N Specs with dependency graph
   - Writes Spec markdown files
   - Updates `specs.jsonl` with metadata
6. TUI updates Plan status to "ready"
7. TUI shows toast: "Created 3 Specs"

**User can now drill down with Enter to see Spec list**

#### Starting Execution (TUI Flow)

1. User selects "ready" Plan, presses `s`
2. TUI prompts: "Start execution for 'add-oauth-authentication'? This will spawn 3 loops. [y/N]"
3. User confirms
4. TaskDaemon scheduler:
   - Queries ready Specs (no unmet dependencies)
   - Creates git worktree for each
   - Spawns loop task for each
   - Records executions in `executions.jsonl`
5. TUI updates Plan status to "in_progress"
6. TUI shows loops in grid view

### Real-Time Event Streaming

#### Event Flow

```
┌──────────────┐         ┌────────────────┐         ┌──────────┐
│  Loop Task   │─events─►│ Event Channel  │─events─►│   TUI    │
└──────────────┘         │  (bounded)     │         └──────────┘
                         └────────────────┘
┌──────────────┐                │
│ Coordinator  │────────────────┘
└──────────────┘
┌──────────────┐
│State Manager │────────────────┘
└──────────────┘
```

**Event publishing:**
```rust
// In loop task
event_tx.send(TuiEvent::LoopIterationChanged {
    exec_id: self.id.clone(),
    iteration: 5,
}).await?;

// In coordinator
event_tx.send(TuiEvent::MainBranchUpdated {
    commit_sha: "abc123...".to_string(),
}).await?;
```

**Event consumption (TUI):**
```rust
// TUI main loop
loop {
    tokio::select! {
        // Handle keyboard input
        Some(key) = input_rx.recv() => {
            self.handle_key(key)?;
        }

        // Handle events from TaskDaemon
        Some(event) = event_rx.recv() => {
            self.handle_event(event)?;
            self.dirty = true;  // Mark for redraw
        }

        // Periodic refresh (fallback)
        _ = tokio::time::sleep(Duration::from_millis(100)) => {
            if self.dirty {
                self.render()?;
                self.dirty = false;
            }
        }
    }
}
```

**Incremental updates:**
- Don't rebuild entire state on every event
- Update only affected items
- Use dirty flags per view region
- Batch multiple events before redraw

#### Event Types & Updates

| Event | View | Update Action |
|-------|------|---------------|
| PlanStatusChanged | Plan View | Update status icon, color |
| SpecsCreated | Plan View | Update Spec count column |
| LoopStarted | Plan View, Spec View | Show loop ID, update status |
| LoopIterationChanged | Loop Grid/Focus | Update iteration display |
| LoopOutput | Loop Focus | Append to output buffer |
| LoopCompleted | All views | Update status, cascade to Spec/Plan |
| MainBranchUpdated | Status bar | Show notification, trigger rebase |

### Performance & Responsiveness

#### Target Metrics

- **Frame rate:** 60 FPS (16ms per frame)
- **Input latency:** <100ms from keypress to screen update
- **Event latency:** <500ms from loop event to TUI display
- **Memory:** <50MB for TUI state (even with 50 loops)
- **Startup time:** <1s to initial render

#### Optimization Strategies

**1. Incremental rendering:**
```rust
// Don't redraw entire screen
// Only redraw changed widgets
if self.plans_dirty {
    terminal.draw(|f| self.render_plan_view(f))?;
    self.plans_dirty = false;
}
```

**2. Output buffering:**
- Keep only last 1000 lines per loop
- Circular buffer, oldest lines dropped
- Don't store all output in memory

**3. Event batching:**
- Collect events for 50ms
- Process batch, then redraw once
- Avoid redrawing 10 times per second

**4. Lazy data loading:**
- Don't load all Spec markdown until user drills down
- Cache parsed markdown, invalidate on file change
- Use Arc<String> for shared data (don't clone)

**5. Background tasks:**
- File I/O in separate tokio task
- Don't block rendering on SQLite queries
- Show "loading..." spinner for slow operations

#### Handling High-Frequency Updates

**Problem:** 50 loops, each emitting 1 event/second = 50 events/sec

**Solution:**
- Event channel with capacity 1000 (bounded)
- TUI processes up to 100 events per frame
- If overwhelmed, show warning: "Event queue full, some updates may be delayed"

**Rate limiting per loop:**
- Max 10 output events/second per loop
- Debounce rapid iteration changes (wait 500ms before emitting)

### Implementation Plan

#### Phase 1: Core TUI Framework
- Set up ratatui app structure
- Implement view state machine (View enum)
- Add keyboard input handling
- Create basic Plan view (static data)
- Add navigation (Esc, Enter, :commands)

**Deliverable:** TUI skeleton with navigation, no real data

#### Phase 2: TaskStore Integration
- Connect to TaskStore (read-only queries)
- Load Plan/Spec/Execution summaries
- Display real data in Plan view
- Implement search/filter
- Add pagination for long lists

**Deliverable:** TUI shows real TaskStore data

#### Phase 3: Spec & Loop Views
- Implement Spec view with dependency graph
- Implement Loop grid view (2x2)
- Implement Loop focus view with tabs
- Add drill-down navigation (Plan → Spec → Loop)
- Test with mock loop data

**Deliverable:** All views functional with static loops

#### Phase 4: Real-Time Events
- Set up event channel from TaskDaemon
- Subscribe to loop/coordinator events
- Implement incremental updates
- Add event log panel
- Test with 10 concurrent loops

**Deliverable:** TUI updates in real-time

#### Phase 5: Plan Lifecycle & Controls
- Implement Plan creation flow (interactive agent)
- Add "mark ready" action (triggers decomposition)
- Add start/pause/stop controls
- Implement query loop functionality
- Add confirmation dialogs

**Deliverable:** Full control surface

#### Phase 6: Polish & Performance
- Optimize rendering (dirty flags, batching)
- Add syntax highlighting for output
- Implement command-mode autocomplete
- Add help screen
- Stress test with 50 loops

**Deliverable:** Production-ready TUI

#### Phase 7: Advanced Features
- Export functionality (logs, reports)
- Custom themes (light/dark)
- Configurable layouts
- Mouse support (optional)

**Deliverable:** Enhanced UX features

## Alternatives Considered

### Alternative 1: Web UI (Browser-based)

**Description:** Build React/Vue web app instead of terminal TUI

**Pros:**
- Richer UI (charts, graphs, drag-and-drop)
- Better for remote monitoring
- Mouse-friendly
- Easier to share screenshots

**Cons:**
- Requires web server, adds complexity
- Not native to terminal workflow
- Slower to launch (browser startup)
- Security concerns (expose local daemon to network?)
- Users want terminal-native tool

**Why not chosen:** TaskDaemon is a dev tool, devs live in terminals. TUI is more appropriate.

### Alternative 2: Simple CLI (No TUI)

**Description:** Stick with `taskdaemon status` commands, no interactive UI

**Pros:**
- Simplest implementation
- No rendering complexity
- Works over SSH without special setup

**Cons:**
- No real-time updates (must poll)
- Poor UX for monitoring 50 loops
- No quick navigation
- Hard to debug failing loops

**Why not chosen:** Monitoring concurrent loops requires real-time dashboard

### Alternative 3: tmux-based UI

**Description:** Use tmux panes, one per loop

**Pros:**
- Leverages existing tool
- No custom rendering
- Familiar to tmux users

**Cons:**
- Chaotic with 50+ loops
- No hierarchical navigation
- Hard to control programmatically
- Limited layout options
- Poor discoverability

**Why not chosen:** We want structured, navigable UI, not chaos

### Alternative 4: Separate Monitoring Tool (Not in daemon)

**Description:** TaskDaemon is headless, separate `taskdaemon-tui` binary

**Pros:**
- Separation of concerns
- Daemon can run without TUI
- Multiple TUI instances possible

**Cons:**
- More complex (need IPC or API)
- Adds latency for events
- Two binaries to manage

**Why not chosen:** TUI is integral to UX, keep it in main binary

## Technical Considerations

### Dependencies

**Rust crates:**
- `ratatui` - TUI framework
- `crossterm` - Terminal backend (keyboard, mouse, rendering)
- `tokio` - Async runtime (event loop)
- `tui-textarea` - Multi-line text input (for queries)
- `fuzzy-matcher` - Fuzzy search for command mode
- `syntect` - Syntax highlighting (optional)

**Internal dependencies:**
- `taskstore` - Read Plan/Spec/Execution data
- `taskdaemon` (coordinator, executor) - Subscribe to events

### Terminal Compatibility

**Supported terminals:**
- xterm-256color
- tmux (with 256 colors)
- iTerm2
- Windows Terminal
- Alacritty
- kitty

**Minimum requirements:**
- 80x24 character display (warn if smaller)
- 256 colors (graceful fallback to 16 colors)
- UTF-8 support (for symbols: ●, ⚙, ✓)

**Features:**
- Mouse support (optional, off by default)
- Clipboard integration (for copying IDs)
- Bracketed paste mode

### State Management

**Single source of truth:** TaskStore (SQLite + JSONL)

**TUI state:**
- Cached summaries (lightweight)
- TTL: 5 seconds (refresh after expiry)
- Invalidate on events

**Synchronization:**
- Events push updates to TUI
- TUI never writes directly to TaskStore
- All mutations go through TaskDaemon API (message passing)

### Testing Strategy

**Unit tests:**
- View state machine transitions
- Keyboard input handling
- Event processing logic
- Command parsing

**Integration tests:**
- Mock TaskStore with sample data
- Mock event channel
- Test navigation flows
- Verify correct data displayed

**Manual testing:**
- Stress test with 50 loops
- Test on different terminals
- Verify resize handling
- Check color rendering

**Snapshot tests (optional):**
- Capture rendered output as text
- Compare against golden snapshots
- Detect unintended visual regressions

### Accessibility

**Features:**
- All operations via keyboard (no mouse required)
- Screen reader friendly (semantic text, no ASCII art dependencies)
- High contrast mode option
- Configurable key bindings

**Color blindness:**
- Don't rely solely on color for status
- Use symbols: ✓, ✗, ⚙, 🔒
- Allow custom color schemes

### Rollout Plan

**Phase 1: Alpha (internal use)**
- Basic Plan/Spec/Loop views
- Manual testing by author

**Phase 2: Beta (early users)**
- Full feature set
- Gather feedback on UX
- Iterate on layouts

**Phase 3: Release (public)**
- Polish, documentation
- Screencasts/demos
- Announce on socials

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Ratatui rendering glitches | Medium | Medium | Test on multiple terminals, use stable crossterm backend |
| Event queue overflow (50 loops) | Medium | High | Bounded channel (capacity 1000), rate limiting, warn user if queue > 1000 |
| TUI becomes unresponsive | Low | High | Profile with tokio-console, ensure non-blocking I/O, add timeouts |
| Terminal resize breaks layout | Medium | Low | Handle resize events, reflow content, test edge cases |
| UTF-8 symbols don't render | Low | Low | Fallback to ASCII (e.g., ✓ → [X], ⚙ → [...]) |
| Color schemes clash with terminal | Medium | Low | Respect terminal color scheme, provide custom themes |
| Mouse support breaks SSH | Low | Low | Mouse off by default, enable with flag |
| Command mode conflicts with user's shell | Low | Low | Document keybindings, allow rebinding in config |
| Logs overwhelm display (high verbosity) | Medium | Medium | Filter by level, pagination, export to file |
| User gets lost in navigation | Medium | Low | Always show breadcrumb trail, help screen, status bar hints |
| Concurrent TUI instances conflict | Low | Low | Lock file (.taskdaemon.lock), warn if already running |
| Long Plan names overflow columns | Medium | Low | Truncate with ellipsis, show full on hover/describe |

## Open Questions

- [ ] Should we support multiple Plans selected at once (bulk operations)?
- [ ] How to visualize circular dependencies (if detected)?
- [ ] Should loop grid support custom layouts (user-defined pane arrangement)?
- [ ] Do we need a "paused" view showing only paused loops?
- [ ] Should we add a timeline view (Gantt chart of loop executions)?
- [ ] How to handle very long command output (>10,000 lines)?
- [ ] Should describe (d) show markdown in side panel or full screen?
- [ ] Do we need keyboard shortcuts for "jump to next error"?
- [ ] Should we support exporting TUI state as JSON (for scripting)?
- [ ] How to handle terminal size < 80x24 (ultra-minimal mode)?

## References

**Inspiration:**
- k9s: https://k9scli.io/ (Kubernetes TUI)
- htop: https://htop.dev/ (process monitor)
- lazygit: https://github.com/jesseduffield/lazygit (git TUI)
- bottom: https://github.com/ClementTsang/bottom (system monitor)

**Technical:**
- ratatui: https://ratatui.rs/
- crossterm: https://docs.rs/crossterm/

**Related Docs:**
- [Main Design](./taskdaemon-design.md) - Overall architecture
- [Execution Model](./execution-model-design.md) - Git worktree management
- [Coordinator Protocol](./coordinator-design.md) - Alert/Share/Query events
- [Implementation Details](./implementation-details.md) - Loop schema, domain types
- [Config Schema](./config-schema.md) - Configuration hierarchy
