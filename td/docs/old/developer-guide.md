# TaskDaemon Developer Guide

**Date:** 2026-01-13
**Status:** Active

## Overview

This guide provides practical implementation details for building TaskDaemon, including AWL naming conventions, validation patterns, and orchestration workflows.

## 1. Naming Conventions

### Rust Module Names

**Rule:** Use short, single-word module names to avoid underscores entirely.

```
taskdaemon/src/
├── lib.rs
├── looper.rs     # mod looper; (not loop_manager)
├── coordinator.rs  # mod coordinator;
├── awl.rs        # mod awl; (not awl_evaluator)
├── worktree.rs   # mod worktree;
├── api.rs        # mod api;
└── bin/
    └── taskdaemon.rs
```

### AWL (YAML) Naming

**Rule:** Hyphens in YAML keys (serde converts to snake_case in Rust)

```yaml
# AWL files use kebab-case:
- action: prompt-agent        # NOT prompt_agent
  event-type: "main-updated"  # NOT event_type
  phase-name: "Phase 1"       # NOT phase_name
  store-in: "api_url"         # NOT store_in
  working-dir: "{worktree}"   # NOT working_dir
  max-turns: 10               # NOT max_turns
```

**Rust structs use snake_case with serde rename:**

```rust
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]  // Converts YAML hyphens to Rust underscores
struct Action {
    #[serde(rename = "action")]
    action_type: String,           // Rust field: snake_case
    event_type: Option<String>,
    phase_name: Option<String>,
    store_in: Option<String>,
    working_dir: Option<String>,
    max_turns: Option<u32>,
}
```

**Why kebab-case in AWL?**
- YAML convention (more readable than underscores)
- Consistent with other YAML languages (GitHub Actions, Docker Compose)
- Serde handles conversion automatically

## 2. Repository Structure

TaskDaemon is structured as a library + binary:

```
taskdaemon/
├── Cargo.toml              # Depends on: taskstore = { git = "https://github.com/saidler/taskstore" }
├── src/
│   ├── lib.rs              # Main library (pub use)
│   ├── looper.rs           # Loop manager
│   ├── coordinator.rs      # Coordinator
│   ├── awl.rs              # AWL evaluator
│   ├── worktree.rs         # Git worktree management
│   ├── api.rs              # Anthropic API client
│   ├── tui.rs              # TUI
│   ├── cli.rs              # CLI argument parsing
│   ├── config.rs           # Config loading
│   └── bin/
│       └── taskdaemon.rs   # Thin CLI (calls lib)
```

### Using TaskStore from TaskDaemon

```rust
// taskdaemon/src/lib.rs
use taskstore::{Store, Record, Filter, FilterOp, IndexValue, now_ms};

// TaskDaemon defines its own domain types
use crate::models::{Prd, TaskSpec, Execution, Dependency};

pub struct LoopManager {
    store_tx: mpsc::Sender<StoreMessage>,  // Message passing to state manager
}

// State manager owns the Store
tokio::spawn(async move {
    let mut store = Store::open(".taskstore")?;
    loop {
        match rx.recv().await {
            StoreMessage::GetPrd(id, reply_tx) => {
                // Use generic API with type annotation
                let prd: Option<Prd> = store.get(&id)?;
                reply_tx.send(prd).await?;
            }
            // ... other operations
        }
    }
});
```

### Implementing the Record Trait

TaskStore is generic and requires types to implement the `Record` trait:

```rust
// taskdaemon/src/models/prd.rs
use taskstore::{Record, IndexValue};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prd {
    pub id: String,
    pub title: String,
    pub status: PrdStatus,
    pub file: String,           // Path to markdown file
    pub created_at: i64,
    pub updated_at: i64,
}

impl Record for Prd {
    fn id(&self) -> &str {
        &self.id
    }

    fn updated_at(&self) -> i64 {
        self.updated_at
    }

    fn collection_name() -> &'static str {
        "prds"  // Stored in .taskstore/prds.jsonl
    }

    fn indexed_fields(&self) -> HashMap<String, IndexValue> {
        let mut fields = HashMap::new();
        fields.insert("status".to_string(),
                     IndexValue::String(self.status.to_string()));
        fields
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PrdStatus {
    Draft,
    Ready,
    InProgress,
    Complete,
    Failed,
    Cancelled,
}

impl ToString for PrdStatus {
    fn to_string(&self) -> String {
        match self {
            Self::Draft => "draft".to_string(),
            Self::Ready => "ready".to_string(),
            Self::InProgress => "in_progress".to_string(),
            Self::Complete => "complete".to_string(),
            Self::Failed => "failed".to_string(),
            Self::Cancelled => "cancelled".to_string(),
        }
    }
}
```

Similarly, implement `Record` for `TaskSpec`, `Execution`, and `Dependency` types.

## 3. Validation Loop Pattern

**Core Principle:** Every agentic LLM interaction must have concrete artifacts to validate success.

### The Pattern

```
1. Agent does work → Produces artifact
2. Validate artifact (concrete, deterministic)
   ├─► Pass → Continue to next step
   └─► Fail → Feed error to agent, iterate
3. Agent sees validation results
4. Only when validation passes → Mark complete
```

### Validation Hierarchy

#### Level 1: PRD Generation
**Validation:** grep for required sections, word count, spell check

```bash
# Check required sections exist
grep -q "## Summary" prd.md || exit 1
grep -q "## Goals" prd.md || exit 1
grep -q "## Proposed Solution" prd.md || exit 1

# Check word count (minimum 500 words)
word_count=$(wc -w < prd.md)
[ "$word_count" -ge 500 ] || exit 1
```

#### Level 2: TS Decomposition
**Validation:** JSON schema, coverage check, dependency validation

```python
# validate-ts-coverage.py
def validate_ts_coverage(prd, task_specs):
    # Check all PRD phases covered
    prd_phases = extract_phases(prd)
    ts_phases = [ts.phase_name for ts in task_specs]

    missing = set(prd_phases) - set(ts_phases)
    if missing:
        raise ValidationError(f"Missing phases: {missing}")

    # Check no circular dependencies
    if has_cycle(task_specs):
        raise ValidationError("Circular dependency detected")

    return True
```

#### Level 3: Code Implementation
**Validation:** cargo check → cargo test → cargo clippy → otto ci

**AWL workflow:**
```yaml
# Validation loop (nested)
- action: loop
  name: "Validation"
  foreach:
    items: "range(1, 10)"
    until: "{validation.exit_code == 0}"
    steps:
      - action: shell
        command: "cargo check && cargo test && cargo clippy"
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
```

#### Level 4: Integration
**Validation:** release build, integration tests, acceptance criteria

### Key Principles

1. **Never trust LLM "done" signals alone** - Always validate with concrete checks
2. **Make validation outputs visible** - Feed errors back to agent
3. **Validation must be deterministic** - Not "looks good?" but "exits 0?"
4. **Fail fast, iterate quickly** - Order: syntax → tests → lints → CI
5. **Timeout per iteration** - Each loop has timeout, but phase can take N iterations

### Example: Full Stack

```
User Request
    ↓
PRD Generation (validation: required sections)
    ↓
TS Decomposition (validation: JSON schema, coverage)
    ↓
Phase 1: Write code
    Iteration 1: cargo check FAILS (missing import)
    Iteration 2: cargo check PASSES, cargo test FAILS
    Iteration 3: cargo check PASSES, cargo test PASSES, clippy FAILS
    Iteration 4: ALL PASS → Phase complete
    ↓
Phase 2: (same pattern)
    ↓
Integration Validation (build, tests, acceptance)
    ↓
PRD COMPLETE
```

## 4. PRD Completion Cascade

When an execution completes, TaskDaemon orchestrates a cascade:

**Execution → TS → PRD**

```rust
// In taskdaemon/src/looper.rs
pub async fn handle_loop_completion(
    exec_id: &str,
    store_tx: &mpsc::Sender<StoreMessage>,
) -> Result<()> {
    // 1. Mark execution complete
    let (reply_tx, reply_rx) = oneshot::channel();
    store_tx.send(StoreMessage::CompleteExecution(exec_id.to_string(), reply_tx)).await?;
    reply_rx.await??;

    // Store handles the cascade:
    // - Marks execution complete
    // - Marks TS complete
    // - Checks if all TSs for PRD are complete
    // - If yes, marks PRD complete

    Ok(())
}
```

**TaskDaemon implements the cascade logic** using TaskStore's generic API:

```rust
// In taskdaemon/src/cascade.rs
use taskstore::{Store, Filter, FilterOp, IndexValue, now_ms};
use crate::models::{Execution, TaskSpec, Prd, ExecStatus, TaskSpecStatus, PrdStatus};

pub fn complete_execution(store: &mut Store, exec_id: &str) -> Result<()> {
    // 1. Get and mark execution complete
    let mut exec: Execution = store.get(exec_id)?.unwrap();
    exec.status = ExecStatus::Complete;
    exec.completed_at = Some(now_ms());
    store.update(exec.clone())?;

    // 2. Get and mark TS complete
    let mut ts: TaskSpec = store.get(&exec.ts_id)?.unwrap();
    ts.status = TaskSpecStatus::Complete;
    ts.updated_at = now_ms();
    store.update(ts.clone())?;

    // 3. Check if all TSs for this PRD are complete
    let filters = vec![Filter {
        field: "prd_id".to_string(),
        op: FilterOp::Eq,
        value: IndexValue::String(ts.prd_id.clone()),
    }];
    let all_ts: Vec<TaskSpec> = store.list(&filters)?;

    if all_ts.iter().all(|ts| ts.status == TaskSpecStatus::Complete) {
        // 4. Mark PRD complete
        let mut prd: Prd = store.get(&ts.prd_id)?.unwrap();
        prd.status = PrdStatus::Complete;
        prd.updated_at = now_ms();
        store.update(prd)?;
    }

    Ok(())
}
```

**Key Points:**
- TaskStore provides generic CRUD operations
- TaskDaemon implements domain logic (cascade semantics)
- Use `Filter` to query related records

## 5. Validation Scripts

Create validation tools for each level:

```
taskdaemon/validation/
├── validate-prd.py           # Check PRD completeness
├── validate-ts-coverage.py   # Check TS covers PRD
├── validate-acceptance.py    # Check acceptance criteria
└── common.py                 # Shared validation logic
```

### validate-prd.py

```python
#!/usr/bin/env python3
import sys
import json

def validate_prd(prd_path):
    with open(prd_path) as f:
        content = f.read()

    required_sections = [
        "## Summary",
        "## Problem Statement",
        "## Goals",
        "## Proposed Solution",
    ]

    errors = []
    for section in required_sections:
        if section not in content:
            errors.append(f"Missing section: {section}")

    word_count = len(content.split())
    if word_count < 500:
        errors.append(f"Word count too low: {word_count} (minimum: 500)")

    if errors:
        print(json.dumps({"valid": False, "errors": errors}))
        sys.exit(1)
    else:
        print(json.dumps({"valid": True}))
        sys.exit(0)

if __name__ == "__main__":
    validate_prd(sys.argv[1])
```

**Usage in AWL:**

```yaml
- action: shell
  command: "python validation/validate-prd.py {worktree}/prds/{prd_file}"
  capture: validation

- action: conditional
  if: "{validation.exit_code != 0}"
  then:
    - action: prompt-agent
      prompt: |
        PRD validation failed:
        {validation.stdout}

        Fix the issues and regenerate the PRD.
```

### validate-ts-coverage.py

```python
#!/usr/bin/env python3
import json
import sys

def validate_coverage(prd_path, ts_dir):
    with open(prd_path) as f:
        prd = f.read()

    # Extract phases from PRD
    prd_phases = extract_phases_from_prd(prd)

    # Load all TS files
    ts_files = glob.glob(f"{ts_dir}/*.md")
    ts_phases = [extract_phase_name(f) for f in ts_files]

    # Check coverage
    missing = set(prd_phases) - set(ts_phases)

    if missing:
        print(json.dumps({
            "valid": False,
            "errors": [f"Missing task specs for: {list(missing)}"]
        }))
        sys.exit(1)

    # Check circular dependencies
    if has_circular_deps(ts_files):
        print(json.dumps({
            "valid": False,
            "errors": ["Circular dependency detected"]
        }))
        sys.exit(1)

    print(json.dumps({"valid": True}))
    sys.exit(0)
```

### Common Patterns

All validation scripts:
- Exit 0 on success, non-zero on failure
- Print structured JSON for parsing
- Provide clear error messages
- Are idempotent (safe to run multiple times)

## 6. Data Flow

```
1. User request
   └─► PRD generation loop (prd-generation.awl.yaml)
       └─► Rule of Five (5 review passes)
           └─► Create PRD (status: draft)
           └─► User marks ready → (status: ready)

2. User starts PRD
   └─► TS decomposition loop (ts-decomposition.awl.yaml)
       └─► Generate N task specs
           └─► Create TSs (status: pending, workflow_name set)
       └─► PRD status = in_progress

3. Scheduler finds ready TS
   └─► Spawn execution loop (TS.workflow_name)
       └─► Mark TS status = running

4. Execution complete
   └─► Mark execution = complete
       └─► Mark TS = complete
           └─► Check all TSs for PRD
               └─► If all complete → Mark PRD = complete
```

## 7. AWL Workflow Examples

### Example: prd-generation.awl.yaml

```yaml
workflow:
  name: "PRD Generation"
  version: "1.0"
  description: "Generate PRD using Rule of Five"

  variables:
    review_passes: 5

  before:
    # Gather requirements from user
    - action: prompt-agent
      model: "opus-4.5"
      prompt: |
        Gather requirements for a new PRD:
        1. Feature description
        2. Target users
        3. Success criteria
        4. Constraints
      max-turns: 20
      capture: requirements

  context:
    # Read current PRD draft
    - action: read-file
      path: "/tmp/prd-draft.md"
      format: text
      bind: current_prd

  foreach:
    items: "range(1, {review_passes} + 1)"
    steps:
      - action: prompt-agent
        model: "opus-4.5"
        prompt: |
          Review Pass {item}/5: {review_focus}

          Current PRD:
          {current_prd}

          Focus areas:
          - Pass 1: Completeness
          - Pass 2: Correctness
          - Pass 3: Edge Cases
          - Pass 4: Architecture
          - Pass 5: Clarity
        capture: review_result

      - action: write-file
        path: "/tmp/prd-draft.md"
        content: "{review_result.text}"

  after:
    # Validate final PRD
    - action: shell
      command: "python validation/validate-prd.py /tmp/prd-draft.md"
      capture: validation

    - action: conditional
      if: "{validation.exit_code == 0}"
      then:
        # Save to taskstore
        - action: create-prd
          file: "/tmp/prd-draft.md"
          bind: prd_id
```

### Example: rust-development.awl.yaml

See `awl-schema-design.md` for full example.

## 8. Implementation Priority

### Phase 1: Core Infrastructure
1. Set up taskstore dependency
2. Implement AWL parser and evaluator
3. Create git worktree manager
4. Build Anthropic API client

### Phase 2: Loop Execution
5. Implement loop spawner
6. Add validation loop pattern
7. Integrate with coordinator
8. Test with single loop

### Phase 3: Multi-Loop Orchestration
9. Implement dependency resolution
10. Add proactive rebase
11. Test with 5 concurrent loops
12. Stress test with 50 loops

### Phase 4: PRD Lifecycle
13. Implement PRD generation workflow
14. Implement TS decomposition workflow
15. Add draft→ready→in_progress flow
16. Test full pipeline

### Phase 5: TUI
17. Build ratatui interface (see `tui-design.md`)
18. Integrate with event streams
19. Add control operations
20. Polish UX

## 9. Actor Pattern for State Management

**Prefer message passing over shared mutexes:**

```rust
// State manager task (owns Store)
tokio::spawn(async move {
    let mut store = Store::open(".taskstore")?;
    loop {
        match rx.recv().await {
            StoreMessage::CreateExecution(exec, reply_tx) => {
                let result = store.create_execution(exec);
                reply_tx.send(result).await;
            }
            StoreMessage::UpdateExecution(id, exec, reply_tx) => {
                let result = store.update_execution(&id, exec);
                reply_tx.send(result).await;
            }
            StoreMessage::CompleteExecution(id, reply_tx) => {
                let result = store.complete_execution(&id);
                reply_tx.send(result).await;
            }
            // ... other operations
        }
    }
});

// Loop tasks send messages
let (reply_tx, reply_rx) = oneshot::channel();
store_tx.send(StoreMessage::UpdateExecution(exec_id, exec, reply_tx)).await?;
reply_rx.await??;
```

**Benefits:**
- No deadlocks (no lock contention)
- Single owner of Store (clear ownership)
- Easy to add logging/metrics (centralized)
- Testable (mock message channel)

## 10. Testing Strategy

### Unit Tests
- AWL parser (valid/invalid syntax)
- Validation loop logic
- Worktree creation/cleanup
- API client (mocked responses)

### Integration Tests
- Single loop execution end-to-end
- Multi-loop coordination
- PRD → TS → Loop pipeline
- Crash recovery

### Manual Testing
- Run full PRD generation workflow
- Test with real Anthropic API
- Verify proactive rebase
- Stress test with 50 loops

## 11. References

- [TaskDaemon Design](./taskdaemon-design.md) - Main architecture
- [AWL Schema](./awl-schema-design.md) - Workflow language spec
- [Coordinator Design](./coordinator-design.md) - Inter-loop messaging
- [Execution Model](./execution-model-design.md) - Loop lifecycle
- [TUI Design](./tui-design.md) - Terminal interface
- [TaskStore](https://github.com/saidler/taskstore) - Generic storage library with SQLite+JSONL+Git pattern
