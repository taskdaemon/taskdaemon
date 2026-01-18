# Design Document: Tool Naming Alignment with Claude Code

**Author:** Claude
**Date:** 2026-01-18
**Status:** Draft
**Review Passes:** 5/5

## Summary

Rename TaskDaemon's tools to use single-word names matching Claude Code conventions, and add four new tools (`todo`, `search`, `fetch`, `tree`) to expand capabilities.

## Problem Statement

### Background

TaskDaemon implements a set of 10 tools for LLM agents to interact with the filesystem, execute commands, and coordinate with other agents. These tools currently use `snake_case` naming (e.g., `read_file`, `write_file`, `list_directory`).

Claude Code, Anthropic's official CLI, uses single-word PascalCase tool names (e.g., `Read`, `Write`, `Glob`). While TaskDaemon uses lowercase tool names internally, aligning on single-word naming improves consistency and reduces cognitive overhead when working across both systems.

### Problem

1. **Naming inconsistency**: TaskDaemon uses `read_file` while Claude Code uses `Read`
2. **Missing capabilities**: TaskDaemon lacks tools for task management, web fetching, and directory tree visualization that Claude Code provides

### Goals

- Rename 5 existing tools to single-word names
- Add 4 new tools to expand TaskDaemon's capabilities
- Maintain backward compatibility in configuration files (document migration path)
- Keep all existing functionality intact

### Non-Goals

- Adding all Claude Code tools (many are specific to Claude Code's architecture)
- Changing the internal Rust struct names (only the tool name strings)
- Supporting both old and new names simultaneously

## Proposed Solution

### Overview

Rename tools by changing the `name()` method return value in each tool implementation, then update all references throughout the codebase. Add four new tools following the existing `Tool` trait pattern.

### Tool Naming Changes

| Current Name | New Name | Rationale |
|--------------|----------|-----------|
| `read_file` | `read` | Matches Claude Code's `Read` |
| `write_file` | `write` | Matches Claude Code's `Write` |
| `edit_file` | `edit` | Matches Claude Code's `Edit` |
| `list_directory` | `list` | Simpler; Claude Code uses `ls` via Bash |
| `run_command` | `bash` | Matches Claude Code's `Bash` |

**Unchanged tools** (already single-word or unique to TaskDaemon):
- `glob` - Already single-word
- `grep` - Already single-word
- `complete_task` - TaskDaemon-specific, two words acceptable for clarity
- `query` - Already single-word
- `share` - Already single-word

### New Tools

#### 1. `todo` - Task List Management

**Purpose**: Allow agents to track and manage tasks during execution.

**Comparison with Claude Code**: Claude Code has `TodoWrite` for task list management. TaskDaemon's version will be simpler, focused on the agent's internal task tracking.

```rust
// src/tools/builtin/todo.rs
fn name(&self) -> &'static str { "todo" }
fn description(&self) -> &'static str {
    "Manage a task list. Actions: add, complete, list, clear"
}
fn input_schema(&self) -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["add", "complete", "list", "clear"],
                "description": "Action to perform"
            },
            "task": {
                "type": "string",
                "description": "Task description (for add) or task ID (for complete)"
            }
        },
        "required": ["action"]
    })
}
```

**Storage**: Tasks stored in `ToolContext` as `Arc<Mutex<Vec<TodoItem>>>`.

#### 2. `search` - Web Search

**Purpose**: Allow agents to search the web for information.

**Comparison with Claude Code**: Claude Code has `WebSearch` which performs web searches and returns results.

```rust
// src/tools/builtin/search.rs
fn name(&self) -> &'static str { "search" }
fn description(&self) -> &'static str {
    "Search the web for information. Returns summarized results."
}
fn input_schema(&self) -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Search query"
            },
            "max_results": {
                "type": "integer",
                "description": "Maximum results to return (default: 5)"
            }
        },
        "required": ["query"]
    })
}
```

**Implementation**: Use a search API (configurable, e.g., SerpAPI, Tavily, or Brave Search).

#### 3. `fetch` - Web Content Fetching

**Purpose**: Fetch and process content from URLs.

**Comparison with Claude Code**: Claude Code has `WebFetch` which fetches URLs and processes content.

```rust
// src/tools/builtin/fetch.rs
fn name(&self) -> &'static str { "fetch" }
fn description(&self) -> &'static str {
    "Fetch content from a URL. Converts HTML to markdown."
}
fn input_schema(&self) -> Value {
    json!({
        "type": "object",
        "properties": {
            "url": {
                "type": "string",
                "description": "URL to fetch"
            },
            "selector": {
                "type": "string",
                "description": "Optional CSS selector to extract specific content"
            }
        },
        "required": ["url"]
    })
}
```

**Implementation**: Use `reqwest` for HTTP, `scraper` for HTML parsing, convert to markdown.

#### 4. `tree` - Directory Tree Visualization

**Purpose**: Display directory structure as a tree, similar to `eza --tree`.

**Comparison with Claude Code**: Claude Code doesn't have a dedicated tree tool; agents use `Bash` with tree/eza commands. TaskDaemon's native tool avoids shell spawning overhead.

```rust
// src/tools/builtin/tree.rs
fn name(&self) -> &'static str { "tree" }
fn description(&self) -> &'static str {
    "Display directory structure as a tree"
}
fn input_schema(&self) -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Directory path (default: current directory)"
            },
            "depth": {
                "type": "integer",
                "description": "Maximum depth to traverse (default: 3)"
            },
            "show_hidden": {
                "type": "boolean",
                "description": "Show hidden files (default: false)"
            }
        }
    })
}
```

**Implementation**: Pure Rust using `walkdir` crate, format output like `eza --tree`.

### Architecture

```
src/tools/
├── builtin/
│   ├── mod.rs              # Export all tools
│   ├── read_file.rs        # name() returns "read"
│   ├── write_file.rs       # name() returns "write"
│   ├── edit_file.rs        # name() returns "edit"
│   ├── list_directory.rs   # name() returns "list"
│   ├── run_command.rs      # name() returns "bash"
│   ├── glob.rs             # unchanged
│   ├── grep.rs             # unchanged
│   ├── complete_task.rs    # unchanged
│   ├── query.rs            # unchanged
│   ├── share.rs            # unchanged
│   ├── todo.rs             # NEW
│   ├── search.rs           # NEW
│   ├── fetch.rs            # NEW
│   └── tree.rs             # NEW
├── context.rs
├── executor.rs             # Register new tools
├── traits.rs
└── mod.rs
```

### Files to Modify

#### Phase 1: Renames (5 tools)

| File | Changes |
|------|---------|
| `src/tools/builtin/read_file.rs:15` | `"read_file"` → `"read"` |
| `src/tools/builtin/write_file.rs:15` | `"write_file"` → `"write"` |
| `src/tools/builtin/edit_file.rs:15,19,72` | `"edit_file"` → `"edit"`, update error messages |
| `src/tools/builtin/list_directory.rs:15` | `"list_directory"` → `"list"` |
| `src/tools/builtin/run_command.rs:15` | `"run_command"` → `"bash"` |
| `src/tools/executor.rs:24-32,125-129` | Update HashMap keys and test assertions |
| `src/tui/runner.rs:209-276` | Update system prompts and tool lists |
| `src/tui/views.rs:286` | Update match pattern |
| `src/tui/conversation_log.rs:231,235` | Update test tool names |
| `src/llm/types.rs:315,327` | Update test tool names |
| `src/llm/anthropic.rs:394,409` | Update test tool names |
| `src/loop/type_loader.rs:175,524-534` | Update tool name strings |
| `src/loop/config.rs:83` | Update default tool list |
| `src/loop/builtin_types/ralph.yml:76` | Update tool name |
| `taskdaemon.yml:122-125,188-191,271-274,340-346` | Update all tool lists |

#### Phase 2: New Tools (4 tools)

| File | Changes |
|------|---------|
| `src/tools/builtin/todo.rs` | NEW - Task management tool |
| `src/tools/builtin/search.rs` | NEW - Web search tool |
| `src/tools/builtin/fetch.rs` | NEW - URL fetching tool |
| `src/tools/builtin/tree.rs` | NEW - Directory tree tool |
| `src/tools/builtin/mod.rs` | Export new tools |
| `src/tools/executor.rs` | Register new tools |
| `src/tools/context.rs` | Add todo storage to context |
| `Cargo.toml` | Add dependencies (scraper, walkdir) |

### Data Model

#### TodoItem (for `todo` tool)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: usize,
    pub task: String,
    pub status: TodoStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}
```

### API Design

All tools follow the existing `Tool` trait:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn input_schema(&self) -> Value;
    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult;
}
```

### Implementation Plan

**Phase 1: Tool Renames**
1. Update tool name strings in all 5 tool files
2. Update executor HashMap keys
3. Update all test files
4. Update TUI runner prompts and tool lists
5. Update configuration files
6. Update documentation

**Phase 2: New Tools**
1. Add `tree` tool (simplest, no external deps beyond walkdir)
2. Add `todo` tool (requires context changes)
3. Add `fetch` tool (requires HTTP client)
4. Add `search` tool (requires API key configuration)

## Alternatives Considered

### Alternative 1: Keep snake_case Names

- **Description:** Leave tool names as `read_file`, `write_file`, etc.
- **Pros:** No migration effort, existing configs continue to work
- **Cons:** Inconsistent with Claude Code, longer names add noise
- **Why not chosen:** User requested alignment with Claude Code conventions

### Alternative 2: Support Both Old and New Names

- **Description:** Accept both `read_file` and `read` as valid tool names
- **Pros:** Backward compatible, gradual migration
- **Cons:** Adds complexity, confusing for agents which name to use
- **Why not chosen:** Clean break is simpler; migration path is documented

### Alternative 3: Use PascalCase Like Claude Code

- **Description:** Use `Read`, `Write`, `Edit` instead of `read`, `write`, `edit`
- **Pros:** Exact match with Claude Code
- **Cons:** Inconsistent with TaskDaemon's lowercase convention elsewhere
- **Why not chosen:** Lowercase single-word names are a reasonable middle ground

## Technical Considerations

### Dependencies

**Existing:**
- `async_trait` - For async Tool trait
- `serde_json` - For input schemas
- `tokio` - For async file operations

**New (for new tools):**
- `walkdir` - For tree traversal
- `scraper` - For HTML parsing in fetch tool
- `reqwest` - For HTTP requests (already likely present)
- Search API client (TBD based on chosen provider)

### Performance

- Tool renames have zero runtime impact
- `tree` tool should limit depth to prevent slow traversal
- `fetch` tool should have timeouts and size limits
- `search` tool performance depends on external API

### Security

- `fetch` tool: Validate URLs, prevent SSRF attacks, limit to HTTP/HTTPS
- `search` tool: Sanitize query strings, rate limit API calls
- `tree` tool: Respect sandbox boundaries (already enforced by ToolContext)

### Testing Strategy

1. **Unit tests**: Each tool has its own test module
2. **Integration tests**: Test tool execution through ToolExecutor
3. **Manual testing**: Run TUI and verify tools work in conversation

### Rollout Plan

1. Implement and test all changes on a feature branch
2. Update CHANGELOG documenting the breaking changes
3. Merge to main
4. Tag new version

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Breaking existing configs | High | Medium | Document migration, update example configs |
| Search API costs | Medium | Low | Make search tool optional, configurable |
| Fetch tool abuse | Low | Medium | Rate limiting, URL allowlisting option |
| Missing API keys | Medium | Low | Return helpful error message, don't panic |

## Open Questions

- [ ] Which search API provider to use? (SerpAPI, Tavily, Brave Search, or make configurable?)
- [ ] Should `todo` persist tasks across sessions or be ephemeral?
- [ ] Should `fetch` support authentication for private URLs?

## Appendix: Tool Comparison Matrix

### TaskDaemon vs Claude Code - Complete Comparison

| Category | TaskDaemon Tool | Claude Code Tool | Notes |
|----------|-----------------|------------------|-------|
| **File Reading** | `read` | `Read` | Same purpose |
| **File Writing** | `write` | `Write` | Same purpose |
| **File Editing** | `edit` | `Edit` | Both use string replacement |
| **Directory Listing** | `list` | (via Bash `ls`) | TaskDaemon has dedicated tool |
| **Pattern Search** | `glob` | `Glob` | Same purpose |
| **Content Search** | `grep` | `Grep` | Both use ripgrep |
| **Shell Commands** | `bash` | `Bash` | Same purpose |
| **Task Completion** | `complete_task` | - | TaskDaemon-specific |
| **Inter-agent Query** | `query` | - | TaskDaemon's multi-agent feature |
| **Inter-agent Share** | `share` | - | TaskDaemon's multi-agent feature |
| **Task Management** | `todo` (NEW) | `TodoWrite` | Similar purpose |
| **Web Search** | `search` (NEW) | `WebSearch` | Similar purpose |
| **URL Fetching** | `fetch` (NEW) | `WebFetch` | Similar purpose |
| **Directory Tree** | `tree` (NEW) | (via Bash `eza`) | TaskDaemon gets dedicated tool |
| **Subagent Launch** | - | `Task` | Claude Code-specific |
| **Task Output** | - | `TaskOutput` | Claude Code-specific |
| **User Questions** | - | `AskUserQuestion` | Claude Code-specific |
| **Notebook Edit** | - | `NotebookEdit` | Claude Code-specific |
| **Kill Shell** | - | `KillShell` | Claude Code-specific |
| **Plan Mode** | - | `EnterPlanMode`/`ExitPlanMode` | Claude Code-specific |
| **Skills** | - | `Skill` | Claude Code-specific |

### Final Tool Count

- **TaskDaemon (after changes):** 14 tools
- **Claude Code:** 17 tools

---

## Review Log

### Pass 1: Completeness
- [x] Summary section complete
- [x] Problem statement with background, goals, non-goals
- [x] Proposed solution with architecture and implementation plan
- [x] Alternatives considered (3 alternatives)
- [x] Technical considerations (deps, perf, security, testing)
- [x] Risks and mitigations table
- [x] Open questions listed
- [x] Tool comparison appendix added

**Changes made in Pass 1:**
- Added full tool comparison matrix as appendix
- Specified file paths and line numbers for all changes
- Added data model for TodoItem
- Clarified implementation phases

### Pass 2: Correctness
- [x] Verified tool names match Claude Code conventions (lowercase vs PascalCase noted)
- [x] File paths and line numbers verified against codebase exploration
- [x] Tool trait definition matches actual implementation
- [x] Dependencies are realistic (walkdir, scraper, reqwest all exist)

**Changes made in Pass 2:**
- No corrections needed; technical details verified against codebase

### Pass 3: Edge Cases
- [x] What if `fetch` receives an invalid URL? → Return error via ToolResult::error
- [x] What if `tree` traverses a huge directory? → Depth limit (default 3) mitigates
- [x] What if `search` API key is missing? → Tool should return helpful error, not panic
- [x] What if `todo` list grows unbounded? → Ephemeral per-session, cleared on task completion
- [x] What about symlink loops in `tree`? → walkdir handles this by default

**Changes made in Pass 3:**
- Added risk row for missing API keys
- Clarified todo list lifecycle (ephemeral)

### Pass 4: Architecture
- [x] New tools follow existing Tool trait pattern
- [x] ToolContext extension for todo storage is minimal
- [x] No circular dependencies introduced
- [x] Phased implementation allows incremental delivery
- [x] Search/fetch tools are optional (can be disabled via config)

**Changes made in Pass 4:**
- Confirmed architecture fits existing patterns
- No structural changes needed

### Pass 5: Clarity
- [x] Implementation plan is step-by-step executable
- [x] File paths are specific enough to locate changes
- [x] Tool schemas are copy-pasteable
- [x] Open questions are actionable
- [x] Someone could implement from this document

**Changes made in Pass 5:**
- Document converged; no further changes needed
