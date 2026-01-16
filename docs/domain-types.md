# Domain Types Specification

**Author:** Scott A. Idler
**Date:** 2026-01-15
**Status:** Implementation Spec

---

## Summary

TaskDaemon uses three core domain types stored in TaskStore: Plan, Spec, and LoopExecution. All implement the `Record` trait for JSONL persistence with SQLite indexing.

---

## Type Hierarchy

```
Plan
 └── Spec (parent = Plan.id)
      └── LoopExecution (parent = Spec.id, loop_type = "phase")

LoopExecution can also be standalone (loop_type = "ralph", no parent)
```

**Two relationship concepts:**
- `parent`: Structural hierarchy (who created me)
- `deps`: Execution dependencies (what must finish before I start)

These are orthogonal. A Spec has a parent Plan, but may also depend on sibling Specs.

---

## Plan

A Plan is the top-level work unit, created from user input via the Plan Refinement Loop.

```rust
pub struct Plan {
    pub id: String,              // "019430-plan-add-oauth"
    pub title: String,           // "Add OAuth Authentication"
    pub status: PlanStatus,
    pub file: String,            // Absolute path to markdown
    pub priority: Priority,      // For scheduler
    pub created_at: i64,         // Unix ms
    pub updated_at: i64,
}

pub enum PlanStatus {
    Draft,       // Being refined
    Ready,       // User approved, ready for Spec decomposition
    InProgress,  // Specs being generated/implemented
    Complete,    // All Specs complete
    Failed,      // Unrecoverable error
    Cancelled,   // User cancelled
}
```

| Field | Constraints |
|-------|-------------|
| `id` | 6-char hex prefix + slug, globally unique |
| `title` | Max 256 chars |
| `file` | Must be absolute path, file must exist |
| `priority` | Inherited by child Specs |

---

## Spec

A Spec is an atomic unit of work decomposed from a Plan. Contains phases that are implemented sequentially.

```rust
pub struct Spec {
    pub id: String,              // "019431-spec-oauth-endpoints"
    pub parent: String,          // Plan.id
    pub title: String,
    pub status: SpecStatus,
    pub deps: Vec<String>,       // Spec IDs that must complete first
    pub file: String,            // Absolute path to markdown
    pub phases: Vec<Phase>,
    pub priority: Priority,      // Inherited from Plan or overridden
    pub created_at: i64,
    pub updated_at: i64,
}

pub struct Phase {
    pub name: String,            // "Phase 1: Create endpoint stubs"
    pub description: String,
    pub status: PhaseStatus,
}

pub enum SpecStatus {
    Pending,     // Waiting for deps
    Blocked,     // Dep failed or manual intervention needed
    Running,     // Being implemented
    Complete,    // All phases done, validation passed
    Failed,
}

pub enum PhaseStatus {
    Pending,
    Running,
    Complete,
    Failed,
}
```

| Field | Constraints |
|-------|-------------|
| `parent` | Must reference existing Plan |
| `deps` | Must reference sibling Specs (same parent), no cycles |
| `phases` | At least 1 phase, phases run sequentially |

**Dependency rules:**
- Deps are checked before spawning implementation loop
- If any dep has status != Complete, Spec stays Pending
- Circular deps detected at creation time (rejected)

---

## LoopExecution

Tracks the runtime state of any loop (plan, spec, phase, or ralph).

```rust
pub struct LoopExecution {
    pub id: String,              // "019432-loop-phase-oauth-ep-p1"
    pub loop_type: String,       // "plan" | "spec" | "phase" | "ralph"
    pub parent: Option<String>,  // Spec.id for phase loops, Plan.id for spec loops
    pub deps: Vec<String>,       // LoopExecution IDs (rare, usually empty)
    pub status: LoopStatus,
    pub worktree: Option<String>,// Absolute path, None for plan/spec loops
    pub iteration: u32,          // Current iteration (1-indexed)
    pub progress: String,        // Accumulated progress text
    pub context: Value,          // Template context (JSON)
    pub created_at: i64,
    pub updated_at: i64,
}

pub enum LoopStatus {
    Pending,     // Waiting to start
    Running,     // Actively iterating
    Paused,      // User paused
    Rebasing,    // Handling main branch update
    Blocked,     // Rebase conflict or other blocker
    Complete,    // Validation passed
    Failed,      // Max iterations or unrecoverable error
    Stopped,     // User/coordinator requested stop
}
```

| Field | Constraints |
|-------|-------------|
| `loop_type` | Must match a configured loop type |
| `worktree` | Required for phase/ralph loops, None for plan/spec |
| `iteration` | Starts at 1, increments each iteration |
| `progress` | Managed by ProgressStrategy, may be large |
| `context` | Arbitrary JSON, used for prompt template variables |

---

## Record Trait

All domain types implement `Record` for TaskStore persistence:

```rust
pub trait Record: Serialize + DeserializeOwned {
    fn id(&self) -> &str;
    fn updated_at(&self) -> i64;
    fn collection_name() -> &'static str;
    fn indexed_fields(&self) -> HashMap<String, IndexValue>;
}
```

| Type | Collection | Indexed Fields |
|------|------------|----------------|
| Plan | `plans` | `status`, `priority` |
| Spec | `specs` | `status`, `parent`, `priority` |
| LoopExecution | `loop_executions` | `status`, `loop_type`, `parent` |

---

## ID Format

All IDs use: `{6-char-hex}-{type}-{slug}`

```
019430-plan-add-oauth
019431-spec-oauth-db-schema
019431-spec-oauth-endpoints
019432-loop-phase-oauth-ep-p1
```

- **Hex prefix**: First 6 chars of UUIDv7 (sortable by time)
- **Type**: plan, spec, loop
- **Slug**: Lowercased title, spaces → hyphens

**Resolution**: Users can reference by hex prefix alone (`019431`), full ID, or slug substring. Ambiguous references error with candidates list.

---

## Storage Layout

```
.taskstore/
├── plans.jsonl                    # Plan records
├── plans/
│   └── add-oauth.md               # Plan content
├── specs.jsonl                    # Spec records
├── specs/
│   ├── oauth-db-schema.md
│   └── oauth-endpoints.md
├── loop_executions.jsonl          # LoopExecution records
└── taskstore.db                   # SQLite index cache
```

**JSONL**: Source of truth, git-tracked
**SQLite**: Query cache, rebuilt from JSONL on startup

---

## Worktree Management

| Operation | Details |
|-----------|---------|
| **Location** | `/tmp/taskdaemon/worktrees/{exec-id}/` |
| **Branch** | `taskdaemon/{exec-id}` |
| **Create** | `git worktree add {path} -b {branch}` |
| **Rebase** | `git rebase main` (on main_updated alert) |
| **Cleanup** | `git worktree remove {path}` (on complete/failed) |

**Lifecycle:**
1. Created when LoopExecution spawns (for phase/ralph types)
2. Persists across daemon restarts (recovery resumes in existing worktree)
3. Removed when loop completes or fails (configurable retention)

**Conflict handling:**
- If rebase fails: abort rebase, set status=Blocked
- Blocked loops require manual resolution
- After manual fix: user runs `taskdaemon resume {exec-id}`

---

## Status Transitions

### Plan
```
Draft → Ready (user approval)
Ready → InProgress (spec loop spawned)
InProgress → Complete (all specs complete)
InProgress → Failed (unrecoverable)
* → Cancelled (user action)
```

### Spec
```
Pending → Running (deps satisfied, loop spawned)
Pending → Blocked (dep failed)
Running → Complete (all phases pass)
Running → Failed (max iterations, error)
Running → Blocked (rebase conflict)
```

### LoopExecution
```
Pending → Running (spawned by manager)
Running → Paused (user action)
Running → Rebasing (main_updated)
Running → Complete (validation passes)
Running → Failed (max iterations, error)
Running → Stopped (stop request)
Rebasing → Running (rebase success)
Rebasing → Blocked (rebase conflict)
Paused → Running (resume)
```

---

## References

- [TaskDaemon Design](./taskdaemon-design.md) - Architecture context
- [Implementation Details](./implementation-details.md) - ID format, template variables
- [Loop Manager](./loop-manager.md) - Worktree lifecycle
- [Progress Strategy](./progress-strategy.md) - Progress field management
