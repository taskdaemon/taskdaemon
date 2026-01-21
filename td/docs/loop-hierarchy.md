# Design Document: Loop Hierarchy and Loops Pane

**Author:** Scott A. Idler
**Date:** 2026-01-18
**Status:** Draft
**Review Passes:** 5/5

## Summary

This document defines TaskDaemon's four-level loop hierarchy (Plan → Spec → Phase → Ralph) and specifies the new "Loops" pane that displays this hierarchy as a tree view. The design unifies the currently broken "Records" pane and the flat "Executions" pane into a single, coherent tree-based view that shows the full decomposition of work from high-level plans down to individual implementation loops.

## Problem Statement

### Background

TaskDaemon orchestrates concurrent AI coding loops that work on complex software projects. Work is decomposed hierarchically:

1. **Plans** - High-level design documents (created via Rule of Five)
2. **Specs** - Atomic work specifications derived from Plans
3. **Phases** - Implementation phases derived from Specs
4. **Ralph loops** - Actual code implementation in git worktrees

The codebase has two domain types that were intended to support this:
- `Loop` (in `src/domain/record.rs`) - A template/planning record with embedded phases
- `LoopExecution` (in `src/domain/execution.rs`) - A runtime execution record with `parent` field

The current TUI has two panes:
- **Executions pane** - Shows flat list of `LoopExecution` records
- **Records pane** - Intended to show `Loop` records, but these are never created

### Problem

The current implementation has several issues:

1. **Records pane is broken** - The `Loop` record type is never populated during normal operation; only `LoopExecution` records are created. The Records pane queries `list_loops()` which returns empty.
2. **Executions pane is flat** - No hierarchy visible; the `parent` field exists in `LoopExecution` but is ignored during rendering
3. **Mental model mismatch** - Users think in terms of "my Plan spawned these Specs which spawned these Phases" but the UI shows an unrelated flat list
4. **Two separate types unnecessary** - The `Loop` (record) vs `LoopExecution` distinction adds complexity; a single `LoopExecution` with `parent` hierarchy suffices
5. **No way to see cascade** - Can't visualize how validation flows upward
6. **Naming confusion** - "Executions" vs "Records" distinction is unclear to users

### Goals

1. **Unified tree view** - Single "Loops" pane showing Plan → Spec → Phase → Ralph hierarchy
2. **Visual hierarchy** - Tree-style indentation with expand/collapse like `tree` command output
3. **Validation cascade visibility** - See completion status propagate from leaves to roots
4. **Replace both panes** - Eliminate broken Records pane, replace flat Executions pane
5. **Status at every level** - Show Draft/Active/Complete/Failed for each node
6. **Progress tracking** - Show child completion counts (e.g., "3/5 phases complete")

### Non-Goals

1. **Horizontal dependencies** - This design doesn't visualize Spec-to-Spec or Phase-to-Phase dependencies (DAG view is a future feature)
2. **Historical view** - Only shows current/recent loops, not full history
3. **Multi-repo view** - Single repository at a time
4. **Real-time streaming output** - Tree shows status, not live command output (use Loop Focus view for that)
5. **Editing loops** - Tree is read-only; editing happens via commands or other views

## Proposed Solution

### Overview

Rename "Executions" to "Loops" and display all loop executions in a tree structure based on parent-child relationships. Remove the broken "Records" pane entirely.

### How It Works (User Perspective)

1. **User creates a Plan** in the Plan pane (Chat mode with `/plan` or dedicated Plan view)
2. **Plan appears in Loops pane** as `◌ Plan: my-feature (draft)` with `[-]` children
3. **User activates the Plan** (marks it ready/active)
4. **System automatically spawns Specs** → Specs appear as children in tree
5. **Specs automatically spawn Phases** → Phases appear as grandchildren
6. **Phases automatically spawn Ralphs** → Ralphs appear as great-grandchildren
7. **Ralphs iterate** until validation passes (or max iterations)
8. **Completion cascades upward** → Ralph ✓ → Phase ✓ → Spec ✓ → Plan ✓

The user watches the tree update in real-time as work progresses.

### The Four Loop Levels

#### Level 1: Plan Loop

**Purpose:** Transform a user's idea into a structured design document using the Rule of Five methodology (5 review passes).

| Property | Value |
|----------|-------|
| Input | User's feature description |
| Output | `plan.md` design document |
| Parent | None (root) |
| Max iterations | 25 |
| Validation | `otto ci` |

**Lifecycle:**
1. **Draft** - User creates/iterates on plan (not yet approved)
2. **Active** - User approves; triggers 1-2 Spec loop spawns
3. **Complete** - All child Specs complete validation
4. **Failed** - One or more Specs failed permanently

#### Level 2: Spec Loop

**Purpose:** Decompose a Plan into atomic, implementable specifications.

| Property | Value |
|----------|-------|
| Input | Parent `plan.md` content |
| Output | `spec.md` atomic specification |
| Parent | Plan ID |
| Max iterations | 50 |
| Validation | `otto ci` |

**Cardinality:** 1-2 Specs per Plan (focused scope)

**Lifecycle:**
1. **Created** - Spawned when parent Plan activates
2. **Active** - Immediately active; triggers 3-7 Phase spawns
3. **Complete** - All child Phases complete validation
4. **Failed** - One or more Phases failed permanently

#### Level 3: Phase Loop

**Purpose:** Break down a Spec into ordered implementation phases.

| Property | Value |
|----------|-------|
| Input | Parent `spec.md` content |
| Output | `phase.md` implementation phase |
| Parent | Spec ID |
| Max iterations | 50 |
| Validation | `otto ci` |

**Cardinality:** 3-7 Phases per Spec

**Lifecycle:**
1. **Created** - Spawned when parent Spec activates
2. **Active** - Immediately active; triggers 1-3 Ralph spawns
3. **Complete** - All child Ralph loops complete validation
4. **Failed** - Ralph loops exceeded max iterations

#### Level 4: Ralph Loop

**Purpose:** Execute actual code implementation in an isolated git worktree.

| Property | Value |
|----------|-------|
| Input | Parent `phase.md`, git worktree |
| Output | Committed code on feature branch |
| Parent | Phase ID |
| Max iterations | 100 |
| Validation | `otto ci` |

**Cardinality:** 1-3 Ralphs per Phase (allows retries)

**Lifecycle:**
1. **Created** - Spawned when parent Phase activates
2. **Running** - Writing code, running validation
3. **Complete** - Validation passes
4. **Failed** - Max iterations without passing

### Validation Cascade

Validation flows **upward** through the hierarchy:

```
Ralph completes (otto ci passes)
    ↓
Phase checks: All Ralph children complete?
    YES → Phase completes → notify Spec
    ↓
Spec checks: All Phase children complete?
    YES → Spec completes → notify Plan
    ↓
Plan checks: All Spec children complete?
    YES → Plan completes ✓
```

### Spawning Behavior

**Plan Activation (Draft → Active):**
```
1. Plan.status = Active
2. Analyze plan.md → identify 1-2 spec_templates
3. FOR EACH spec_template:
     CREATE Spec loop with parent = plan.id
     Spec.status = Active (immediate)
4. Each Spec triggers Phase spawning
```

**Spec Activation:**
```
1. Spec.status = Active
2. Analyze spec.md → identify 3-7 phase_templates
3. FOR EACH phase_template:
     CREATE Phase loop with parent = spec.id
     Phase.status = Active (immediate)
4. Each Phase triggers Ralph spawning
```

**Phase Activation:**
```
1. Phase.status = Active
2. CREATE Ralph loop with parent = phase.id
3. Ralph.status = Running
4. Ralph begins iterating in git worktree
```

### Tree View Design

The Loops pane renders a tree with expand/collapse:

```
┌─────────────────────────────────────────────────────────────────┐
│ Loops                                                    :loops │
├─────────────────────────────────────────────────────────────────┤
│ ▼ ⚙ Plan: add-oauth-authentication                    [1/2]    │
│   ▼ ✓ Spec: oauth-core-implementation                 [5/5]    │
│     │ ✓ Phase 1: database-schema                      [1/1]    │
│     │   └── ✓ Ralph 019abc (3 iters)                           │
│     │ ✓ Phase 2: jwt-token-service                    [1/1]    │
│     │   └── ✓ Ralph 019abd (5 iters)                           │
│     │ ✓ Phase 3: oauth-endpoints                      [2/2]    │
│     │   ├── ✗ Ralph 019abe (10 iters, failed)                  │
│     │   └── ✓ Ralph 019abf (7 iters)                           │
│     │ ✓ Phase 4: session-management                   [1/1]    │
│     │   └── ✓ Ralph 019ac0 (4 iters)                           │
│     └ ✓ Phase 5: integration-tests                    [1/1]    │
│         └── ✓ Ralph 019ac1 (6 iters)                           │
│   ▼ ⚙ Spec: oauth-provider-integrations               [2/3]    │
│       ✓ Phase 1: google-oauth                         [1/1]    │
│       │ └── ✓ Ralph 019ac2 (5 iters)                           │
│       ✓ Phase 2: github-oauth                         [1/1]    │
│       │ └── ✓ Ralph 019ac3 (4 iters)                           │
│       ⚙ Phase 3: provider-selection-ui                [0/1]    │
│         └── ⚙ Ralph 019ac4 (iter 6/100)                        │
│                                                                 │
│ ▶ ◌ Plan: database-migration-tool (draft)             [-]      │
├─────────────────────────────────────────────────────────────────┤
│ [Enter] expand/collapse  [d] describe  [p] pause  [s] stop     │
└─────────────────────────────────────────────────────────────────┘
```

**Status indicators:**
| Icon | Status | Meaning |
|------|--------|---------|
| `◌` | Draft | Plan created, awaiting user approval |
| `⚙` | Active/Running | Actively executing |
| `✓` | Complete | Validation passed |
| `✗` | Failed | Max iterations or error |
| `◑` | Paused | User paused |
| `⊘` | Stopped | User stopped |

**Progress format by level:**
| Level | Format | Example |
|-------|--------|---------|
| Plan | `[completed_specs/total_specs]` | `[1/2]` |
| Spec | `[completed_phases/total_phases]` | `[5/5]` |
| Phase | `[completed_ralphs/total_ralphs]` | `[1/1]` |
| Ralph | `(iter N/max)` or `(N iters)` | `(iter 6/100)` or `(5 iters)` |

**Tree characters:**
- `▼` - Expanded node (has visible children)
- `▶` - Collapsed node (children hidden)
- `│` - Vertical connector
- `├──` - Branch to sibling
- `└──` - Branch to last child

### Data Model

**LoopExecution fields used for hierarchy:**

```rust
pub struct LoopExecution {
    pub id: String,
    pub loop_type: String,      // "plan", "spec", "phase", "ralph"
    pub parent: Option<String>, // ID of parent loop
    pub status: LoopExecutionStatus,
    pub iteration: u32,
    // ...
}
```

**Building the tree:**
1. Query all LoopExecutions
2. Group by `loop_type` and `parent`
3. Build tree: Plans (parent=None) → Specs (parent=plan_id) → Phases (parent=spec_id) → Ralphs (parent=phase_id)
4. Calculate child counts at each level
5. Render with indentation

### Keyboard Navigation

| Key | Action |
|-----|--------|
| `↑`/`k` | Move selection up |
| `↓`/`j` | Move selection down |
| `Enter` | Toggle expand/collapse |
| `→`/`l` | Expand node |
| `←`/`h` | Collapse node |
| `d` | Describe selected loop (show details) |
| `p` | Pause selected loop |
| `r` | Resume selected loop |
| `s` | Stop selected loop |
| `Tab` | Cycle to next pane |
| `:loops` | Jump to Loops pane |

### Implementation Plan

**Phase 1: Data Layer**
- Add `children_count` computation to LoopExecution queries
- Create tree-building function from flat execution list
- Add parent-chain traversal for expand/collapse state

**Phase 2: TUI Rendering**
- Create `render_loops_tree()` function in `views.rs`
- Implement tree line rendering with Unicode box characters
- Add expand/collapse state to AppState

**Phase 3: Navigation**
- Implement tree-aware cursor movement
- Add expand/collapse keyboard handlers
- Wire up action keys (pause/resume/stop)

**Phase 4: Integration**
- Replace Executions pane with Loops pane
- Remove Records pane code
- Update `:loops` command routing
- Update Tab cycling order

## Alternatives Considered

### Alternative 1: Keep Two Separate Panes

**Description:** Fix Records pane, keep Executions pane separate

**Pros:**
- Less code change
- Preserves existing mental model (if there was one)

**Cons:**
- Users don't understand the Records/Executions split
- Duplicates information
- Records pane serves no clear purpose

**Why not chosen:** The split causes confusion; unified tree is more intuitive.

### Alternative 2: DAG Visualization

**Description:** Show full dependency graph including horizontal Spec-to-Spec and Phase-to-Phase dependencies

**Pros:**
- More complete picture of dependencies
- Handles complex multi-Spec Plans better

**Cons:**
- Significantly more complex to render
- ASCII DAGs are hard to read
- Most Plans have simple 1-2 Spec structure
- Overkill for typical usage

**Why not chosen:** Tree covers 90% of cases; DAG can be added later as optional view.

### Alternative 3: Flat List with Grouping

**Description:** Keep flat list but add collapsible group headers

**Pros:**
- Simpler rendering
- Familiar pattern (like file browsers)

**Cons:**
- Doesn't show full hierarchy (only one level of grouping)
- Can't see Plan → Spec → Phase → Ralph chain

**Why not chosen:** Doesn't satisfy requirement to see full 4-level hierarchy.

## Technical Considerations

### Dependencies

**Internal:**
- `src/domain/execution.rs` - LoopExecution type with `parent` field
- `src/tui/views.rs` - Current rendering code
- `src/tui/state.rs` - AppState with selection state
- `src/tui/runner.rs` - Data refresh logic
- `src/loop/cascade.rs` - Cascade logic (may need updates)
- `src/state/manager.rs` - StateManager queries

**External:**
- `ratatui` - TUI framework (already used)
- No new dependencies needed

### Architectural Changes

**Domain Model Simplification:**

Currently there are two types:
- `Loop` (record.rs) - Planning/template record with phases
- `LoopExecution` (execution.rs) - Runtime execution record

The design uses only `LoopExecution` for the tree. The `Loop` type may be deprecated or repurposed. Key change:

| Current | New |
|---------|-----|
| `Loop` created for planning, `LoopExecution` for runtime | Just `LoopExecution` with `status: Draft` for planning |
| `list_loops()` returns `Loop` records | `list_executions()` returns all, filter by `loop_type` |
| Cascade uses both types | Cascade uses only `LoopExecution` |

**View Enum Changes:**

```rust
// Before
pub enum View {
    Repl,
    Executions,
    Records { type_filter, parent_filter },
    Logs { target_id },
    Describe { target_id, target_type },
}

// After
pub enum View {
    Repl,
    Loops,  // Renamed from Executions
    // Records removed
    Logs { target_id },
    Describe { target_id, target_type },
}
```

**TopLevelPane Changes:**

```rust
// Before
pub enum TopLevelPane { Chat, Plan, Executions, Records }

// After
pub enum TopLevelPane { Chat, Plan, Loops }
// Tab cycle: Chat → Plan → Loops → Chat
```

**State Manager Changes:**

Need new method for efficient tree building:

```rust
impl StateManager {
    /// List all executions grouped by parent for tree building
    pub async fn list_executions_by_parent(&self) -> Result<HashMap<Option<String>, Vec<LoopExecution>>> {
        let all = self.list_executions(None, None).await?;
        let mut grouped: HashMap<Option<String>, Vec<LoopExecution>> = HashMap::new();
        for exec in all {
            grouped.entry(exec.parent.clone()).or_default().push(exec);
        }
        Ok(grouped)
    }
}
```

**Index Requirements:**

The `parent` field in `LoopExecution` is already indexed (see `indexed_fields()` in execution.rs:281-283). No schema changes needed.

### Performance

**Tree building:**
- O(n) to build tree from flat list (single pass with HashMap)
- O(n) to render visible nodes
- Typical case: <100 loops = negligible

**Memory:**
- Tree node struct: ~200 bytes per node
- 100 loops = ~20KB additional memory
- No concern

**Refresh:**
- Tree rebuilt on each data refresh (every 500ms)
- Could optimize with incremental updates if needed (not expected to be necessary)

### Testing Strategy

**Unit tests:**
- Tree building from flat list
- Child count calculation
- Expand/collapse state management
- Tree cursor navigation

**Integration tests:**
- Create Plan → verify tree shows Plan
- Activate Plan → verify Specs appear as children
- Complete Ralph → verify status cascades upward

**Manual testing:**
- Verify tree renders correctly in various terminal sizes
- Test expand/collapse UX
- Verify keyboard navigation feels natural

### Rollout Plan

1. Implement tree rendering behind feature flag
2. Internal testing on real workloads
3. Remove feature flag, replace Executions pane
4. Remove Records pane code
5. Update documentation

## Edge Cases and Error Handling

### Draft Plans (No Children)

Plans in Draft state have no children yet. Display with `[-]` for child count:

```
▶ ◌ Plan: database-migration-tool (draft)             [-]
```

When user activates the Plan, children appear and count updates.

### Orphaned Executions

If a `LoopExecution` has a `parent` ID that doesn't exist (data corruption, manual deletion):

1. Log warning: "Orphaned execution {id} has invalid parent {parent_id}"
2. Display at root level with warning indicator
3. Allow user to delete via `D` key

### Multiple Ralphs Per Phase (Retries)

A Phase may spawn multiple Ralph loops when the first fails:

```
⚙ Phase 3: oauth-endpoints                      [2/2]
  ├── ✗ Ralph 019abe (10 iters, failed)
  └── ✓ Ralph 019abf (7 iters)              ← retry succeeded
```

**Retry logic:**
- When Ralph fails (max iterations or error), Phase checks retry count
- If retries < max_retries (default 3), spawn new Ralph
- If all retries exhausted, Phase marks as Failed
- All Ralph children remain visible (history preserved)

### Circular Parent References

Detect cycles during tree building:

```rust
fn build_tree(executions: &[LoopExecution]) -> Tree {
    let mut visited = HashSet::new();
    for exec in executions {
        if let Some(parent) = &exec.parent {
            if visited.contains(parent) {
                warn!("Circular reference detected: {} -> {}", exec.id, parent);
                // Treat as orphan (root level)
            }
        }
        visited.insert(&exec.id);
    }
    // ...
}
```

### Large Trees

If a Plan somehow has >20 Specs or a Spec has >20 Phases:

1. Render first 20 with "..." indicator
2. Show total count: `[showing 20 of 47]`
3. Allow scrolling within collapsed parent to see all children
4. This shouldn't happen in normal use (typical: 1-2 Specs, 3-7 Phases)

### Race Conditions During Refresh

Tree state may change between data fetch and render:

1. Snapshot executions list at start of refresh
2. Build tree from snapshot
3. Render from snapshot
4. Next refresh cycle picks up changes
5. No mid-render updates (atomic swap of state)

### Empty State

When no loops exist:

```
┌─────────────────────────────────────────────────────────────────┐
│ Loops                                                    :loops │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│                      No loops yet                               │
│                                                                 │
│        Use the Plan pane (Tab) to create a new Plan             │
│                                                                 │
├─────────────────────────────────────────────────────────────────┤
│ [Tab] Go to Plan pane                                           │
└─────────────────────────────────────────────────────────────────┘
```

Note: Plan creation happens in the Plan pane, not the Loops pane. The Loops pane is read-only and shows execution state.

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Tree rendering breaks on narrow terminals | Medium | Medium | Set minimum width, truncate titles gracefully |
| Large trees (100+ nodes) slow to render | Low | Low | Profile if needed; typical usage is <50 nodes |
| Users miss flat list view | Low | Low | Can add `:flat-loops` command if requested |
| Expand/collapse state lost on refresh | Medium | Medium | Persist expand state in AppState, restore after refresh |
| Deep nesting hard to read | Low | Medium | Max depth is 4 levels; indent is reasonable |
| Orphaned executions from data corruption | Low | Medium | Detect and display at root with warning; allow deletion |
| Circular parent references | Very Low | High | Detect during tree build; treat as orphan |
| Retry storms (Ralph keeps failing and respawning) | Low | Medium | Max retries per Phase (default 3); exponential backoff |

## Open Questions

- [ ] Should collapsed Plans show aggregated status (e.g., "3/5 complete" across all descendants)?
- [ ] Should we auto-expand nodes that have running children?
- [ ] What's the default expand state for new Plans? (Currently: collapsed for Draft, expanded for Active)
- [ ] Should Tab cycling include Loops pane or separate tree navigation mode?

## References

- [TUI Design](./tui-design.md) - Overall TUI architecture
- [Execution Model](./execution-model-design.md) - Git worktree management
- [Loop Engine](./loop-engine.md) - Core loop execution mechanics
- [Rule of Five](./rule-of-five.md) - Plan review methodology
