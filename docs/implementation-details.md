# Implementation Details

**Author:** Scott A. Idler
**Date:** 2026-01-15
**Status:** Draft - Pending Review

This document captures implementation specifics that supplement the main design docs.

---

## 1. Loop Schema

A Loop is the core Ralph Wiggum construct. Every loop type must implement this interface.

### Required Fields

| Field | Type | Purpose |
|-------|------|---------|
| *(key)* | string | The YAML key IS the loop name (e.g., "plan", "spec", "phase", "ralph") |
| `prompt-template` | string | Handlebars template to generate fresh prompt each iteration |
| `validation-command` | string | Shell command to run after each iteration |
| `success-exit-code` | int | What exit code means "done" (usually 0) |
| `max-iterations` | int | Safety limit to prevent infinite loops |

### Runtime State Fields

These are managed by the system, not user-configured:

| Field | Type | Purpose |
|-------|------|---------|
| `iteration` | int | Current iteration number (starts at 1) |
| `progress` | string | Accumulates text describing previous iterations. Empty on first iteration. |

### Optional Fields

| Field | Type | Purpose |
|-------|------|---------|
| `description` | string | Human-readable explanation |
| `iteration-timeout-ms` | int | Max time per iteration |
| `inputs` | list | What state/files this loop reads |
| `outputs` | list | What artifacts it produces |
| `system-prompt` | string | System prompt for the LLM |

### Minimal Viable Loop Definition

```yaml
# The key IS the name (no separate 'name' field needed)
ralph:
  prompt-template: |
    {{task-description}}
    {{current-state}}
  validation-command: "otto ci"
  success-exit-code: 0
  max-iterations: 100
```

---

## 2. Config Schema

Full configuration spec for `~/.config/taskdaemon/config.yaml`.

*(To be expanded in separate doc: config-schema.md)*

---

## 3. StateManager Actor Messages

StateManager owns TaskStore and processes messages via actor pattern.

### Recommendation: Keep it Generic

```rust
enum StoreMessage {
    // Generic CRUD (works for any Record type)
    Create { collection: String, data: serde_json::Value, reply: oneshot::Sender<Result<String>> },
    Get { collection: String, id: String, reply: oneshot::Sender<Result<Option<Value>>> },
    Update { collection: String, data: serde_json::Value, reply: oneshot::Sender<Result<()>> },
    Delete { collection: String, id: String, reply: oneshot::Sender<Result<()>> },
    List { collection: String, filters: Vec<Filter>, reply: oneshot::Sender<Result<Vec<Value>>> },

    // Sync operations
    Sync { reply: oneshot::Sender<Result<()>> },
    RebuildIndexes { collection: String, reply: oneshot::Sender<Result<usize>> },
}
```

### Rationale

- TaskStore is already generic (works with any `Record` type)
- Don't need type-specific messages like `CreatePlan`, `UpdateSpec`
- The collection name + JSON value is enough
- Keeps the actor simple, lets TaskStore's generics do the work

### Alternative

Type-safe messages per domain type. More boilerplate but compile-time guarantees. Not recommended since TaskStore already handles the typing.

---

## 4. Handlebars Template Variables

The prompt template injects dynamic state each iteration. The LLM needs context about what it's working on, current state, and previous failures.

### Common Variables (All Loops)

| Variable | Description |
|----------|-------------|
| `iteration` | Current iteration number |
| `max-iterations` | Safety limit |
| `previous-errors` | Validation output from last failed iteration |
| `git-status` | Output of `git status --porcelain` |
| `git-diff` | Recent changes |

### Plan Loop Variables

| Variable | Description |
|----------|-------------|
| `user-request` | Original user input |
| `current-plan` | Contents of plan markdown so far |
| `review-pass` | Which Rule of Five pass we're on (1-5) |

### Spec Loop Variables

| Variable | Description |
|----------|-------------|
| `plan-content` | Full plan markdown |
| `existing-specs` | List of specs already created |
| `coverage-gaps` | What the plan covers that specs don't yet |

### Phase Loop Variables

| Variable | Description |
|----------|-------------|
| `spec-content` | Full spec markdown |
| `phase-name` | Current phase being implemented |
| `phase-number` | e.g., "3 of 5" |
| `completed-phases` | What's already done |

### Ralph Loop Variables (Generic)

| Variable | Description |
|----------|-------------|
| `task-description` | What to do |
| `current-state` | File contents, context |
| `working-directory` | Where we are |

---

## 5. Large JSONL File Handling

### Builtin Monitor

TaskDaemon includes a JSONL size monitor that checks file sizes before loading:

| Threshold | Action |
|-----------|--------|
| 50 MB | Warning logged |
| 200 MB | Error raised, operation blocked |

This fires BEFORE attempting to load into memory, preventing OOM.

```rust
const JSONL_WARN_THRESHOLD: u64 = 50 * 1024 * 1024;   // 50 MB
const JSONL_ERROR_THRESHOLD: u64 = 200 * 1024 * 1024; // 200 MB

fn check_jsonl_size(path: &Path) -> Result<()> {
    let size = std::fs::metadata(path)?.len();
    if size > JSONL_ERROR_THRESHOLD {
        return Err(eyre!("JSONL file {} exceeds 200MB limit ({}MB). Run compaction.",
            path.display(), size / 1024 / 1024));
    }
    if size > JSONL_WARN_THRESHOLD {
        tracing::warn!("JSONL file {} is large ({}MB). Consider compaction.",
            path.display(), size / 1024 / 1024);
    }
    Ok(())
}
```

### Expected Usage

This is unlikely to be a problem in practice. A typical project will have:
- Dozens of Plans (not thousands)
- Hundreds of Specs (not millions)
- Thousands of LoopExecutions over time (manageable)

At ~1KB per record average:
- 50 MB ≈ 50,000 records (warning)
- 200 MB ≈ 200,000 records (error)

### If Threshold Hit

Solutions include:
- Periodic compaction (dedupe old versions, remove tombstones)
- Archiving completed work to separate files
- Streaming JSONL reader (future enhancement)

---

## 6. Loop Types vs Domain Types

### Distinction

**Loop Types** are configurations that define HOW a loop runs:
- Plan, Spec, Phase, Ralph
- Defined in YAML files
- Implement the Loop interface (Section 1)

**Domain Types** are DATA stored in TaskStore:
- Plan, Spec, LoopExecution
- Stored in JSONL files
- Have field constraints defined below

When a Phase loop runs, it creates a **LoopExecution** record with `loop-type: "phase"`.

### Four Out-of-Box Loop Types

| Loop Type | Input | Output | Purpose |
|-----------|-------|--------|---------|
| **plan** | User idea | Plan markdown | Create/refine a Plan document |
| **spec** | Plan | N Spec markdowns | Decompose Plan into atomic Specs |
| **phase** | Spec phase | Code/tests in worktree | Implement one phase of a Spec |
| **ralph** | Task description | Varies | Generic loop for arbitrary tasks |

These are configurations in `~/.config/taskdaemon/loops/` or `.taskdaemon/loops/`. They all implement the Loop interface defined in Section 1.

---

### Domain Type: Plan

| Field | Type | Constraints |
|-------|------|-------------|
| `id` | string | Required, 6-char hex + slug |
| `parent` | string | Optional, ID of parent (typically null for top-level Plans) |
| `deps` | list | IDs that must complete before this can start (typically empty) |
| `title` | string | Required, max 256 chars |
| `status` | enum | draft, ready, in-progress, complete, failed, cancelled |
| `file` | string | Required, absolute path |
| `created-at` | int | Required, unix ms |
| `updated-at` | int | Required, unix ms |

---

### Domain Type: Spec

| Field | Type | Constraints |
|-------|------|-------------|
| `id` | string | Required, 6-char hex + slug |
| `parent` | string | Required, ID of parent (typically a Plan) |
| `title` | string | Required |
| `status` | enum | pending, blocked, running, complete, failed |
| `deps` | list | IDs that must complete before this can start |
| `file` | string | Required |
| `phases` | list | Phase objects with name, description |
| `created-at` | int | Required, unix ms |
| `updated-at` | int | Required, unix ms |

**Dependency Rules:**
- `parent` links to parent record (structural hierarchy)
- `deps` links to sibling records that must complete first (execution dependency)
- If `deps` is present, this record cannot start until ALL deps are `complete`
- Circular dependencies are not allowed (validated at creation)

**Example:**
```json
{
  "id": "019431-spec-oauth-endpoints",
  "parent": "019430-plan-add-oauth",
  "title": "OAuth API Endpoints",
  "status": "blocked",
  "deps": ["019431-spec-oauth-db-schema"],
  "file": "/home/user/project/.taskstore/specs/oauth-endpoints.md",
  "phases": [
    { "name": "Phase 1", "description": "Create endpoint stubs" },
    { "name": "Phase 2", "description": "Implement token validation" }
  ]
}
```

This Spec is `blocked` until `019431-spec-oauth-db-schema` is `complete`.

---

### Domain Type: LoopExecution

| Field | Type | Constraints |
|-------|------|-------------|
| `id` | string | Required, 6-char hex + slug |
| `loop-type` | string | Required, must match a defined loop name |
| `parent` | string | Optional, ID of parent record (up the tree) |
| `deps` | list | IDs that must complete before this can start |
| `status` | enum | pending, running, paused, rebasing, blocked, complete, failed, stopped |
| `worktree` | string | Optional, absolute path to git worktree |
| `iteration` | int | Current iteration, >= 1 |
| `progress` | string | Accumulated progress text from previous iterations |
| `context` | object | Template context (spawned values, NOT runtime values) |
| `created-at` | int | Required, unix ms |
| `updated-at` | int | Required, unix ms |

**Hierarchy:**
- `parent` = structural (who spawned me, points UP the tree)
- `deps` = execution dependencies (what must finish before I start)

These are orthogonal. A record can have a parent AND deps.

---

---

## 7. ID Format and Resolution

### ID Structure

All IDs use a human-readable format with a 6-char hex prefix from UUIDv7:

```
019432-spec-oauth-endpoints
^^^^^^ ^^^^^^^^^^^^^^^^^^^^
hex    slug (derived from title)
```

- **Hex prefix:** First 6 chars of UUIDv7 timestamp (~16ms precision, sorts chronologically)
- **Slug:** Lowercased title, spaces/special chars replaced with hyphens

### ID Resolution

Users and systems can reference records using partial matches:

| Method | Example | Matches |
|--------|---------|---------|
| Hex only | `019432` | Exact match on hex prefix |
| Full ID | `019432-spec-oauth-endpoints` | Exact match |
| Slug prefix | `spec-oauth` | Any ID where slug starts with `spec-oauth` |
| Slug substring | `endpoints` | Any ID where slug contains `endpoints` |

**Resolution rules:**
- If exactly one match: resolves to that record
- If zero matches: error "not found"
- If multiple matches: error "ambiguous reference" with list of candidates

**Examples:**
```
> taskdaemon show oauth-db
Found: 019432-spec-oauth-db-schema

> taskdaemon show oauth
Error: Ambiguous reference "oauth" matches:
  - 019432-spec-oauth-db-schema
  - 019433-spec-oauth-endpoints
  - 019434-spec-oauth-tests

> taskdaemon show 019433
Found: 019433-spec-oauth-endpoints
```

---

## 8. File Path Handling

**Input:** User can provide relative or absolute paths
**Storage:** Always stored as absolute paths

```rust
fn normalize_path(input: &str, base: &Path) -> PathBuf {
    let path = Path::new(input);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path).canonicalize().unwrap()
    }
}
```

**Example:**
```
User provides: specs/oauth-endpoints.md
Base directory: /home/user/project/.taskstore
Stored as:     /home/user/project/.taskstore/specs/oauth-endpoints.md
```

---

## References

- [Main Design](./taskdaemon-design.md)
- [Coordinator Protocol](./coordinator-design.md)
- [Execution Model](./execution-model-design.md)
- [TUI Design](./tui-design.md)
