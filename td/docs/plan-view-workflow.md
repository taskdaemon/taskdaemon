# Design Document: Plan View Workflow

**Author:** Claude (via create-design-doc skill)
**Date:** 2026-01-17
**Status:** In Review
**Review Passes:** 5/5

## Summary

Implement an interactive Plan view workflow in the TaskDaemon TUI where users converse with an AI agent to iteratively gather and refine requirements, then generate a structured plan document using the Rule of Five methodology. Plans are persisted as draft executions with unique hash-slug identifiers and appear in the Executions view for tracking and execution.

## Problem Statement

### Background

TaskDaemon TUI currently has a Plan mode toggle (Tab key switches between Chat and Plan modes) but both modes share identical behavior - same system prompt, same conversation handling, same tools. The Plan mode was scaffolded but not differentiated.

The existing Plan loop type (`src/loop/builtin_types/plan.yml`) implements Rule of Five methodology for creating plan documents, but there's no interactive way for users to gather requirements before triggering plan generation.

### Problem

Users need a guided, conversational way to develop their requirements before committing to plan creation. Currently, they must either:
1. Write all requirements upfront in a single message, or
2. Use Chat mode which doesn't guide them toward structured planning

This leads to incomplete requirements, missed edge cases, and plans that need significant revision.

### Goals

- Provide a conversational flow in Plan mode that guides users through requirements gathering
- Generate structured plan documents using Rule of Five methodology
- Persist plans as draft executions with unique identifiers (hash-slug format)
- Display draft plans in the Executions view for visibility and management
- Allow users to review and approve drafts before execution begins

### Non-Goals

- Real-time collaborative editing of plans (single user only)
- Version control integration for plan documents (git tracking is separate)
- Plan templates or pre-defined plan structures (Rule of Five is the methodology)
- Automatic plan execution without user approval

## User Flow

### Complete Walkthrough

```
1. User launches TUI (default: REPL view in Chat mode)
   ┌─────────────────────────────────────────┐
   │ TaskDaemon          Chat|Plan     1/0/0 │
   │─────────────────────────────────────────│
   │ Welcome to TaskDaemon Chat              │
   │ Type a message and press Enter...       │
   │                                         │
   │ > _                                     │
   └─────────────────────────────────────────┘

2. User presses Tab to switch to Plan mode
   ┌─────────────────────────────────────────┐
   │ TaskDaemon          Chat|Plan     1/0/0 │
   │─────────────────────────────────────────│
   │ Welcome to TaskDaemon Plan              │
   │ Describe your goal and press Enter...   │
   │                                         │
   │ > _                                     │
   └─────────────────────────────────────────┘

3. User describes what they want to build
   > I want to add OAuth authentication to our app

4. AI asks clarifying questions (guided by plan system prompt)
   AI: What OAuth providers should be supported?
   > Google and GitHub initially

   AI: Should users be able to link multiple providers?
   > Yes, they should be able to link both

   AI: What happens to existing username/password users?
   > They keep their accounts, OAuth is an additional option

5. User types /create when ready
   > /create

6. System summarizes conversation and creates draft
   ┌─────────────────────────────────────────┐
   │ Created draft plan: Add OAuth (019bc8)  │
   │ View in Executions with `2` key         │
   └─────────────────────────────────────────┘

7. User presses 2 to go to Executions view
   ┌─────────────────────────────────────────┐
   │ Executions                              │
   │─────────────────────────────────────────│
   │ ◌ 019bc8-add-oauth    plan   draft     │
   │                                         │
   │ [n]ew [s]tart [d]escribe [x]cancel     │
   └─────────────────────────────────────────┘

8. User reviews plan.md (externally or via describe)
   $ cat .taskdaemon/plans/019bc8-loop-plan-add-oauth/plan.md

9. User presses 's' to start the draft
   Status changes: draft → pending → running

10. Plan loop executes Rule of Five passes
    Plan is refined through 5 iterations
```

## Proposed Solution

### Overview

Extend the existing Plan mode with:
1. A dedicated system prompt that guides requirements gathering
2. A `/create` command to trigger plan generation from the conversation
3. A new `Draft` status for executions that holds plans pending approval
4. UI affordances to view, approve, and start draft plans

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         TUI Layer                                │
├─────────────────────────────────────────────────────────────────┤
│  Plan Mode                                                       │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐      │
│  │ Requirements │───▶│   /create    │───▶│ Draft Plan   │      │
│  │ Conversation │    │   Command    │    │ in Executions│      │
│  └──────────────┘    └──────────────┘    └──────────────┘      │
│         │                   │                    │               │
│         ▼                   ▼                    ▼               │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐      │
│  │ Plan System  │    │  Summarize   │    │  [s] Start   │      │
│  │   Prompt     │    │ Conversation │    │    Draft     │      │
│  └──────────────┘    └──────────────┘    └──────────────┘      │
├─────────────────────────────────────────────────────────────────┤
│                       Domain Layer                               │
├─────────────────────────────────────────────────────────────────┤
│  ┌──────────────────────────────────────────────────────────┐   │
│  │ LoopExecution                                             │   │
│  │ - status: Draft | Pending | Running | Complete | ...      │   │
│  │ - context: { user-request, conversation-summary, ... }    │   │
│  │ - id: {hash}-loop-plan-{slug}                             │   │
│  └──────────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────┤
│                      Storage Layer                               │
├─────────────────────────────────────────────────────────────────┤
│  .taskdaemon/plans/{id}/plan.md    │    StateManager (JSONL)    │
└─────────────────────────────────────────────────────────────────┘
```

### Data Model

**LoopExecutionStatus** (extended):
```rust
pub enum LoopExecutionStatus {
    Draft,      // NEW: Plan created, awaiting user approval
    Pending,    // Ready to run, waiting for scheduler
    Running,    // Actively executing
    Paused,     // User paused
    Rebasing,   // Handling git rebase
    Blocked,    // Conflict or blocker
    Complete,   // Successfully finished
    Failed,     // Unrecoverable error
    Stopped,    // User cancelled
}
```

**Plan Context** (stored in `LoopExecution.context`):
```json
{
  "user-request": "Summarized requirements from conversation",
  "conversation-summary": "Key decisions and clarifications",
  "review-pass": 1,
  "original-messages": ["first msg", "second msg", "..."]
}
```

**Plan File Location**:
```
.taskdaemon/plans/{id}/plan.md

# ID format from generate_id("loop", "{loop_type}-{description}"):
# {6-char-hex}-loop-{loop_type}-{slug}
Example: .taskdaemon/plans/019bc8-loop-plan-add-oauth/plan.md
```

### Plan System Prompt

The Plan mode uses a dedicated system prompt that guides requirements gathering:

```
You are a senior software architect helping gather requirements for a technical plan.

Your role is to:
1. Ask clarifying questions about the user's goals
2. Identify missing details (scope, constraints, dependencies)
3. Suggest considerations they may have missed
4. Summarize the requirements when asked

Guidelines:
- Keep responses concise and focused
- Ask one or two questions at a time
- Acknowledge good answers before moving on
- When requirements seem complete, suggest using /create

Do NOT generate the full plan during this conversation.
Focus on gathering comprehensive requirements first.

Working directory: {worktree}
```

### Conversation Summarization Prompt

When `/create` is invoked, the conversation is summarized:

```
Analyze this conversation and extract a structured requirements summary.

Output format:
## Goal
[One sentence describing what the user wants to build/change]

## Requirements
- [Requirement 1]
- [Requirement 2]
...

## Constraints
- [Constraint 1]
- [Constraint 2]
...

## Key Decisions
- [Decision made during conversation]
...

Be comprehensive but concise. Include all requirements discussed.
```

### Initial Plan File Template

When a draft is created, the initial `plan.md` contains:

```markdown
# Plan: {title}

**Status:** Draft
**Created:** {timestamp}
**ID:** {execution_id}

## Summary

{summarized_goal}

## Requirements

{extracted_requirements}

## Constraints

{extracted_constraints}

---

*This is a draft plan. Review and edit as needed, then use `s` in the Executions view to start execution.*
```

### API Design

**New State Fields** (`AppState`):
```rust
/// Request to create a plan from current conversation
pub pending_plan_create: Option<PlanCreateRequest>,
```

**PlanCreateRequest struct**:
```rust
#[derive(Debug, Clone)]
pub struct PlanCreateRequest {
    /// The conversation messages to summarize
    pub messages: Vec<ReplMessage>,
}
```

**New Pending Action**:
```rust
pub enum PendingAction {
    // ... existing variants
    StartDraft(String),  // Execution ID to start
}
```

**New StateManager Method**:
```rust
impl StateManager {
    pub async fn start_draft(&self, id: &str) -> StateResponse<()>;
}
```

### Implementation Plan

**Phase 1: Domain Model Extension**
- Add `Draft` status to `LoopExecutionStatus` enum
- Add `is_draft()` and `mark_ready()` helper methods
- Update Display and serialization

**Phase 2: Plan-Specific System Prompt**
- Create `build_plan_system_prompt()` function
- Modify `start_repl_request()` to select prompt based on mode
- Store separate `plan_system_prompt` in TuiRunner

**Phase 3: Plan Creation Flow**
- Add `/create` command handling in `app.rs`
- Implement `create_plan_draft()` in `runner.rs`
- Create conversation summarization via LLM
- Generate and persist plan file
- Create LoopExecution record with Draft status

**Phase 4: UI Updates**
- Add draft status styling (gray color, `◌` icon)
- Add `[s]` keybind for starting drafts
- Update Executions view keybind help

**Phase 5: StateManager Integration**
- Add `start_draft()` method
- Handle `PendingAction::StartDraft` in runner tick loop

### Post-Create Behavior

After `/create` completes:
1. Conversation history is preserved (user can continue discussing or start new topic)
2. Success message shows: "Created draft plan: {title} ({hash})"
3. User is prompted: "View in Executions with `2` key, or continue chatting"
4. Draft appears immediately in Executions view (next refresh cycle)

The conversation is NOT cleared automatically because:
- User may want to create multiple related plans
- User may want to refine and create another version
- Explicit `/clear` command available if user wants fresh start

## Alternatives Considered

### Alternative 1: Unified Draft Status for All Loop Types

**Description:** Add `Draft` status to `LoopExecutionStatus` and allow any loop type (Plan, Spec, Phase, Ralph) to be created in draft mode.

**Pros:**
- Consistent model across all loop types
- Users can review any generated artifact before execution
- Spec decomposition could also be reviewed before running phases
- Single status enum, simpler than separate lifecycles

**Cons:**
- Draft may not make sense for all loop types (e.g., Ralph is auto-generated)
- Adds a step to workflows that were previously automatic

**Assessment:** This is the recommended approach. The `Draft` status is added to the shared `LoopExecutionStatus` enum, and the TUI can choose which loop types surface the draft review step. For Plan, draft review is mandatory (via `/create`). For Spec/Phase/Ralph, they can be auto-promoted to Pending by the coordinator, or optionally held for review based on configuration.

### Alternative 2: Separate Document Lifecycle

**Description:** Introduce a separate `DocumentStatus` (Draft → Ready → Superseded) distinct from `LoopExecutionStatus`.

**Pros:**
- Clean separation of document state vs execution state
- Could track document revisions independently
- More precise modeling of reality

**Cons:**
- Two status fields to track and synchronize
- More complex state machine
- Overkill for current requirements

**Why not chosen:** Added complexity without immediate benefit. Can be introduced later if document versioning becomes important.

### Alternative 3: Inline Plan Generation (No Draft Stage)

**Description:** Generate and immediately start executing plans without a draft review stage.

**Pros:**
- Simpler implementation (no new status)
- Faster path to execution

**Cons:**
- No opportunity to review before execution
- Mistakes require cancelling and recreating
- Doesn't match user mental model of "draft then approve"

**Why not chosen:** Users expect to review plans before committing resources to execution.

### Alternative 4: Separate Plan Mode Conversation History

**Description:** Maintain completely separate conversation history for Plan mode vs Chat mode.

**Pros:**
- Clean separation of concerns
- Plan context never polluted by chat

**Cons:**
- More complex state management
- Users might want to reference chat context in planning
- Memory overhead of duplicate histories

**Why not chosen:** Added complexity without clear benefit. Users can use `/clear` if they want fresh context.

### Alternative 5: Plan Templates

**Description:** Offer pre-defined plan templates for common scenarios (feature, bugfix, refactor).

**Pros:**
- Faster plan creation for common cases
- More consistent plan structure

**Cons:**
- Template maintenance burden
- May not fit all use cases
- Rule of Five already provides structure

**Why not chosen:** Rule of Five methodology already provides sufficient structure. Templates can be added later if needed.

## Technical Considerations

### Dependencies

**Internal:**
- `LoopExecution` domain type
- `StateManager` for persistence
- `LlmClient` for conversation summarization
- Existing REPL infrastructure

**External:**
- None new (uses existing Anthropic API via LlmClient)

### Architectural Fit

**Scheduler Compatibility:**
The scheduler in `state/manager.rs` uses `status_filter` when listing executions. It specifically queries for `Pending` status to find work. Adding `Draft` status is safe because:
- Scheduler filters explicitly for `Pending`
- `Draft` executions won't appear in scheduler's work queue
- Only explicit user action (`s` key / `start_draft()`) transitions Draft → Pending

**Integration with Plan Loop Type:**
The existing `plan.yml` loop type defines the Rule of Five execution. This design creates the *initial draft* that becomes the input when the Plan loop runs:
1. User conversation → Draft plan.md (this feature)
2. User approves draft → Status becomes Pending
3. Scheduler picks up → Plan loop runs with Rule of Five passes
4. Plan loop refines the draft through 5 review iterations

**Conversation History Strategy:**
Both Chat and Plan modes share `repl_conversation` in the TuiRunner. This is intentional:
- Allows context to flow between modes
- Avoids complexity of dual conversation tracking
- `/clear` provides explicit reset when needed

**File Storage Pattern:**
Plan files follow the existing pattern where execution artifacts live in `.taskdaemon/`:
```
.taskdaemon/
├── plans/
│   └── {execution-id}/
│       └── plan.md
├── state/
│   └── loop_executions.jsonl
└── ...
```

### Performance

- Conversation summarization adds one LLM call (~2-5 seconds)
- Plan file I/O is negligible
- No impact on TUI responsiveness (async operations)

### Security

- Plan files stored locally in project directory
- No new network exposure
- Conversation content stays local (only sent to configured LLM provider)
- No secrets expected in plan documents

### Testing Strategy

**Unit Tests:**
- `LoopExecutionStatus::Draft` serialization roundtrip
- `is_draft()` and `mark_ready()` state transitions
- ID generation with plan type

**Integration Tests:**
- Plan creation flow end-to-end
- Draft to Pending transition via StateManager
- Plan file creation and content

**Manual TUI Tests:**
- Tab toggle shows mode change
- Plan mode welcome message differs
- `/create` generates draft
- Draft appears in Executions
- `s` key starts draft

### Rollout Plan

1. Implement and test locally
2. Add to feature branch
3. Manual testing of full workflow
4. Merge to main
5. No feature flag needed (extends existing UI)

## Edge Cases and Error Handling

### `/create` with Empty Conversation
- **Behavior:** Show error "No conversation to create plan from. Describe your requirements first."
- **Implementation:** Check `repl_history.is_empty()` before proceeding

### `/create` Called in Chat Mode
- **Behavior:** Show error "Switch to Plan mode first (Tab key)"
- **Implementation:** Check `repl_mode == ReplMode::Plan`

### LLM Summarization Fails
- **Behavior:** Show error, do not create draft, preserve conversation
- **Implementation:** Wrap LLM call in error handling, display: "Failed to summarize conversation: {error}. Try again or use /clear to start over."

### StateManager Unavailable
- **Behavior:** Show error "Cannot create plan: no state manager connected"
- **Implementation:** Check `state_manager.is_some()` before attempting creation

### Plan Directory Already Exists
- **Behavior:** This shouldn't happen (UUIDs are unique), but handle gracefully
- **Implementation:** Use `create_dir_all` which is idempotent; if file exists, append timestamp to filename

### Plan File Deleted Externally
- **Behavior:** Execution record remains, but plan.md missing
- **Implementation:** When starting draft, check file exists. If missing, show error: "Plan file not found. Delete this draft and recreate."

### Multiple Rapid `/create` Calls
- **Behavior:** Prevent duplicate creation while one is in progress
- **Implementation:** Set `pending_plan_create` atomically; ignore subsequent calls while Some

### User Starts Draft That's Already Running
- **Behavior:** No-op with message "This execution is already running"
- **Implementation:** `start_draft()` checks current status before transitioning

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| LLM summarization produces poor requirements | Medium | Medium | Include full conversation in context; user can edit plan.md |
| Draft status breaks existing scheduler | Low | High | Scheduler already filters by Pending status; Draft is ignored |
| Plan files left orphaned if execution deleted | Low | Low | Add cleanup in delete_execution; files are small |
| Users confused by two-stage process | Medium | Low | Clear UI messaging; `/help` explains workflow |
| LLM API failure during creation | Low | Medium | Graceful error with retry guidance; conversation preserved |
| Concurrent `/create` calls | Low | Low | Atomic pending state prevents duplicates |

## Open Questions

- [x] Should switching modes clear conversation? **Decision: No, preserve context**
- [x] Can users have multiple drafts? **Decision: Yes, independent executions**
- [x] Should `/create` work without any conversation? **Decision: No, require at least one exchange. Show error prompting user to describe requirements first.**
- [x] Should draft plans be editable in-TUI or only via external editor? **Decision: External editor only for v1. In-TUI editing can be added later if needed.**

## References

- Existing Plan loop type: `src/loop/builtin_types/plan.yml`
- Plan system prompt: `prompts/plan-system.pmt`
- Rule of Five methodology: `docs/rule-of-five.md`
- TUI design doc: `docs/tui-design.md`
