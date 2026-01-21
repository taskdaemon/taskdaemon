# Design Document: /plan Command with Rule of Five

**Author:** Claude
**Date:** 2026-01-17
**Status:** Implemented
**Review Passes:** 5/5

## Summary

Fix the `/plan` command to properly persist plans to TaskStore (fixing current bug where drafts don't appear in Executions), implement Rule of Five iterative refinement for quality output, and introduce prompt template files (`.pmt`) for maintainable prompt engineering.

## Problem Statement

### Background

TaskDaemon has a Plan pane in the TUI where users can have conversations about requirements. The `/create` command is supposed to generate a plan from this conversation and persist it to TaskStore as a draft execution.

### Problem

1. **Bug: Drafts don't appear in Executions view** - When `/create` is used, the plan.md file may be created at `.taskdaemon/plans/{exec_id}/plan.md`, but the draft execution doesn't show up in the Executions or Records views. This defeats the purpose of the feature.

2. **Quality: Single-pass generation** - The current implementation does one LLM call to summarize the conversation. This produces mediocre output that often includes technical specifications and code samples that should be deferred to the Spec loop.

3. **Maintainability: Embedded prompts** - Prompts are embedded in Rust code, making them hard to iterate on without recompiling.

### Goals

- Fix the bug so draft plans appear in Executions view
- Implement Rule of Five methodology for iterative refinement
- Output PRD-level requirements only (no technical specs, no code)
- Create prompt template system with `.pmt` files
- Clear separation: Plan = WHAT, Spec = HOW

### Non-Goals

- Changing the Spec or Phase loop implementations
- Implementing plan execution (that's the scheduler's job)
- Auto-starting plans after creation (user should review first)

## Proposed Solution

### Overview

1. **Debug and fix** the draft visibility bug
2. **Replace `/create` with `/plan`** command that uses Rule of Five
3. **Create `prompts/` directory** with `.pmt` template files
4. **Iterate 3-5 times** on the requirements extraction

### Architecture

**Module Structure:**
```
src/
├── prompts/           # NEW: Prompt template handling
│   ├── mod.rs         # PromptLoader, PromptContext
│   └── embedded.rs    # Fallback embedded prompts
├── tui/
│   └── runner.rs      # Uses PromptLoader for /plan
└── ...

prompts/               # NEW: Template files (repo root)
├── requirements.pmt   # Plan extraction template
└── README.md
```

**Prompt Loading Chain:**
1. Check `.taskdaemon/prompts/{name}.pmt` (user override)
2. Check `prompts/{name}.pmt` (repo default)
3. Fall back to embedded default in code

**Flow Diagram:**
```
User Conversation
       │
       ▼
┌──────────────────┐
│  /plan command   │
└────────┬─────────┘
         │
         ▼
┌──────────────────┐     ┌─────────────────────┐
│ Pass 1:          │────▶│ PromptLoader        │
│ Extract Reqs     │     │   requirements.pmt  │
└────────┬─────────┘     └─────────────────────┘
         │
         ▼
┌──────────────────┐
│ Pass 2-5:        │
│ Review & Refine  │
│ (until converge) │
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│ Create Draft     │
│ LoopExecution    │
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│ Write plan.md    │
│ TaskStore persist│
│ Show in Execs    │
└──────────────────┘
```

**Integration with Loop Hierarchy:**
```
Plan (created by /plan)     ← THIS DESIGN
  └── Spec (created by plan loop when started)
        └── Phase (created by spec loop)
              └── Ralph (created by phase loop)
```

### Bug Investigation

The draft creation code in `runner.rs:843-850` calls `state_manager.create_execution()`. Potential causes:

1. **StateManager not connected** - `state_manager` is `Option<StateManager>`, might be `None`
2. **Store path mismatch** - TUI and daemon using different paths
3. **Refresh timing** - `DATA_REFRESH_INTERVAL` might miss the update
4. **Error swallowed** - Error might occur but not be visible to user

**Fix approach:**
- Add `info!` logging at each step of draft creation
- Verify `state_manager.is_some()` before attempting creation
- Force immediate refresh after creation (already done at line 903)
- Add draft count to status bar for visibility

### Data Model

The `LoopExecution` record for a draft plan:

```rust
LoopExecution {
    id: "{hash}-plan-{slug}",      // e.g., "a1b2c3-plan-user-auth"
    loop_type: "plan",
    title: Some("User Authentication"),
    status: LoopExecutionStatus::Draft,  // Key: Draft status
    context: {
        "user-request": "...",           // Extracted requirements
        "conversation-summary": "...",   // Original conversation
        "review-pass": 0,                // Which pass created this
    },
    parent: None,                        // Plans are top-level
    created_at: timestamp,
    iteration: 0,
}
```

The `plan.md` file at `.taskdaemon/plans/{exec_id}/plan.md`:
- Markdown format
- Contains the refined requirements
- User can edit before starting execution

### API Design

**PromptLoader** - loads and renders `.pmt` templates:

```rust
pub struct PromptLoader {
    templates_dir: PathBuf,
}

impl PromptLoader {
    /// Load from prompts/ directory
    pub fn new(prompts_dir: PathBuf) -> Result<Self>;

    /// Render a template with context
    pub fn render(&self, template_name: &str, context: &PromptContext) -> Result<String>;
}

pub struct PromptContext {
    pub conversation: String,
    pub pass_number: u8,
    pub previous_output: Option<String>,
    pub focus_area: FocusArea,
}

pub enum FocusArea {
    Completeness,
    Correctness,
    EdgeCases,
    Scope,
    Clarity,
}
```

**Command changes:**
- `/create` - Deprecated, aliased to `/plan` with warning
- `/plan` - New command, uses Rule of Five

### Prompt Template System

Create `prompts/` directory at project root:

```
prompts/
├── requirements.pmt      # Main requirements extraction
└── README.md            # Documentation for prompt authors
```

**Template format** (`.pmt`):
- Plain text with Handlebars-style placeholders
- `{{conversation}}` - The conversation history
- `{{pass_number}}` - Current review pass (1-5)
- `{{previous_output}}` - Output from previous pass
- `{{focus_area}}` - Focus for this pass (completeness, correctness, etc.)

**Example `requirements.pmt`:**
```
{{#if is_first_pass}}
Analyze this conversation and extract a structured requirements document.

IMPORTANT: Output PRD-level requirements ONLY.
- DO NOT include technical implementation details
- DO NOT include code samples or examples
- DO NOT include architecture decisions
- Those belong in the Spec phase, not the Plan phase

CONVERSATION:
{{conversation}}

Output format:
## Goal
[One sentence: what the user wants to accomplish]

## Requirements
- [Functional requirement 1]
- [Functional requirement 2]
...

## Constraints
- [Constraint 1]
...

## Success Criteria
- [How we know it's done]
...

## Non-Goals
- [Explicitly out of scope]
...

## Open Questions
- [Unresolved questions from conversation]
...
{{else}}
Review Pass {{pass_number}}: {{focus_area}}

Review the following requirements document and improve it.
Focus specifically on: {{focus_area}}

Questions to consider:
{{#if focus_completeness}}
- Are all requirements from the conversation captured?
- Are there gaps or missing sections?
- Are success criteria measurable?
{{/if}}
{{#if focus_correctness}}
- Are there logical errors or contradictions?
- Do requirements conflict with each other?
- Are constraints realistic?
{{/if}}
{{#if focus_edge_cases}}
- What could go wrong?
- Are failure modes addressed?
- Are there security considerations?
{{/if}}
{{#if focus_scope}}
- Is scope clearly bounded?
- Are non-goals explicit?
- Is this achievable as a single plan?
{{/if}}
{{#if focus_clarity}}
- Could someone implement from this?
- Are requirements unambiguous?
- Is the language clear and specific?
{{/if}}

CURRENT DOCUMENT:
{{previous_output}}

Output the improved document. If no changes needed, output "CONVERGED" followed by the document.
{{/if}}
```

### Implementation Plan

#### Phase 1: Fix Draft Visibility Bug
1. Add comprehensive logging to `create_plan_draft()`
2. Verify StateManager connection before creation
3. Add draft count to Executions view header
4. Test draft creation and visibility

#### Phase 2: Prompt Template System
1. Create `prompts/` directory
2. Create `requirements.pmt` template
3. Add `PromptLoader` to load and render templates
4. Replace hardcoded prompts in runner.rs

#### Phase 3: Rule of Five Implementation
1. Modify `/plan` (rename from `/create`) to iterate
2. Pass 1: Extract requirements from conversation
3. Passes 2-5: Review with rotating focus areas
4. Detect convergence (output unchanged or "CONVERGED")
5. Stop early if 2 consecutive passes have no changes

#### Phase 4: Integration
1. Update help text for `/plan` command
2. Add progress indicator during refinement passes
3. Test end-to-end flow
4. Update documentation

## Alternatives Considered

### Alternative 1: Keep Single-Pass with Better Prompt
- **Description:** Just improve the prompt without iteration
- **Pros:** Simpler, faster
- **Cons:** Still won't achieve Rule of Five quality
- **Why not chosen:** Research shows 4-5 passes are needed for convergence

### Alternative 2: External Prompt Files (YAML/JSON)
- **Description:** Use structured YAML for prompts
- **Pros:** Could include metadata (model, temperature)
- **Cons:** More complex, harder to read/write
- **Why not chosen:** Plain text `.pmt` files are simpler and sufficient

### Alternative 3: Fix Bug Only, Skip Rule of Five
- **Description:** Just fix visibility bug, improve prompt later
- **Pros:** Faster to ship
- **Cons:** Users still get poor quality output
- **Why not chosen:** Quality is the main complaint

## Technical Considerations

### Dependencies
- Handlebars crate for template rendering (already in Cargo.toml v6.4.0)
- No new external dependencies required

### Performance
- Rule of Five means 3-5 LLM calls per plan creation
- Each pass ~2-5 seconds with streaming
- Total: 10-25 seconds for a quality plan
- Acceptable for a thoughtful requirements document

### Security
- Prompt templates are read-only from filesystem
- No user-supplied template execution
- Conversation content is already sanitized for LLM

### Testing Strategy
1. Unit tests for `PromptLoader`
2. Integration test for draft creation and visibility
3. Manual testing of Rule of Five convergence
4. Test with various conversation lengths

### Rollout Plan
1. Fix draft visibility bug first (critical)
2. Add prompt template system
3. Implement Rule of Five
4. Gather feedback on output quality

## Error Handling

| Scenario | Handling |
|----------|----------|
| Empty conversation | Show error: "No conversation to create plan from" |
| LLM timeout mid-iteration | Retry once, then save partial progress with warning |
| Template file missing | Fall back to embedded default prompt |
| User cancels (Esc) during iteration | Save current draft as-is, mark incomplete |
| LLM returns empty response | Retry with same pass, max 2 retries |
| Conversation exceeds context | Truncate oldest messages, keep last 10 exchanges |

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| LLM doesn't converge | Medium | Medium | Cap at 5 passes, detect loops |
| Prompts too long for context | Low | High | Truncate conversation, summarize if needed |
| Template parsing errors | Low | Medium | Validate templates on load, fallback to embedded |
| Slow for users | Medium | Low | Show progress per pass, allow cancel |
| LLM rate limiting | Medium | Medium | Respect retry-after, show waiting status |
| Oscillating output | Low | Medium | Detect repetition, force stop after 2 repeats |

## Decisions Made

| Question | Decision | Rationale |
|----------|----------|-----------|
| `/plan` vs `/create` | `/plan` replaces, `/create` deprecated with warning | Cleaner UX, no confusion |
| `prompts/` location | Repo root + `.taskdaemon/` override | Version control + user customization |
| Custom user prompts | Yes, via `.taskdaemon/prompts/` | Power users can tune |
| Model settings | Same model, temperature 0.3 for review passes | Lower temp = more consistent refinement |

## Open Questions

- [ ] Should we show diffs between passes for debugging?
- [ ] Max conversation length before truncation?

## References

- Rule of Five research: `~/.config/pais/research/tech/rule-of-five/2026-01-10.md`
- Current /create implementation: `src/tui/runner.rs:745-904`
- StateManager: `src/state/manager.rs`
