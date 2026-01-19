# TaskDaemon Implementation Guide

This document guides the implementation of TaskDaemon using the Ralph Wiggum loop pattern.

## Quick Start

**Read these docs first:**
1. [taskdaemon.yml](./taskdaemon.yml) - Full example config with all builtin loop definitions
2. [docs/taskdaemon-design.md](./docs/taskdaemon-design.md) - Master architecture document

## Recommended Implementation Order

Begin with **Phase 1: Core Ralph Loop Engine** to validate the core pattern before adding orchestration complexity:

| Phase | Focus | Key Docs |
|-------|-------|----------|
| **1** | Core Ralph Loop Engine | [loop-engine.md](./docs/loop-engine.md), [llm-client.md](./docs/llm-client.md), [progress-strategy.md](./docs/progress-strategy.md) |
| **2** | TaskStore Integration | [domain-types.md](./docs/domain-types.md), [implementation-details.md](./docs/implementation-details.md) |
| **3** | Coordination Protocol | [coordinator-design.md](./docs/coordinator-design.md) |
| **4** | Multi-Loop Orchestration | [loop-manager.md](./docs/loop-manager.md), [scheduler.md](./docs/scheduler.md) |
| **5** | Full Pipeline | [config-schema.md](./docs/config-schema.md), [tools.md](./docs/tools.md) |
| **6** | Advanced Loop Features | [execution-model-design.md](./docs/execution-model-design.md) |
| **7** | TUI & Polish | [tui-design.md](./docs/tui-design.md) |

### Phase 1 Deliverable

A working single-loop foundation:
- `LlmClient` trait + `AnthropicClient` implementation with streaming
- `LoopEngine` that executes iterations with fresh context
- `SystemCapturedProgress` for cross-iteration state
- Basic tool execution (read_file, write_file, edit_file, run_command)

This validates the core Ralph Wiggum pattern before adding orchestration complexity.

---

## Execution Workflow

For each phase, follow this loop:

```
┌─────────────────────────────────────────────┐
│  1. Read the phase requirements             │
│  2. Implement code                          │
│  3. Write tests for the implementation      │
│  4. Run `otto ci` to validate               │
│  5. Fix issues until CI passes              │
│  6. Commit with meaningful message          │
│  7. Move to next phase                      │
└─────────────────────────────────────────────┘
```

**CRITICAL: Do NOT pause between phases. Execute ALL phases in sequence.**

---

## Rust Conventions

### Project Structure

```
src/
├── lib.rs              # Public API exports
├── config.rs           # Configuration types and loading
├── llm/
│   ├── mod.rs
│   ├── client.rs       # LlmClient trait
│   └── anthropic.rs    # AnthropicClient implementation
├── loop/
│   ├── mod.rs
│   ├── engine.rs       # LoopEngine iteration logic
│   ├── manager.rs      # LoopManager orchestration
│   └── progress.rs     # ProgressStrategy trait + impls
├── tools/
│   ├── mod.rs
│   ├── context.rs      # ToolContext (worktree-scoped)
│   ├── executor.rs     # ToolExecutor
│   └── builtin/        # read_file, write_file, etc.
├── coordinator/
│   ├── mod.rs
│   └── protocol.rs     # Alert, Share, Query
├── store/
│   ├── mod.rs
│   └── manager.rs      # StateManager actor
└── tui/                # Phase 7
    ├── mod.rs
    └── views/
```

### Coding Principles

1. **Use dependency injection** - Accept traits, not concrete types
2. **Return data, not side effects** - Functions return `Result<T>`, callers decide what to do
3. **Keep the shell thin** - `main.rs` wires dependencies, delegates to library
4. **Async all the way** - Use `tokio` runtime, `async fn` throughout
5. **Structured errors** - Use `thiserror` for error types, `eyre` for propagation

### Example Pattern

```rust
// Trait in client.rs
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse>;
    async fn stream(&self, request: CompletionRequest) -> Result<CompletionStream>;
}

// Implementation in anthropic.rs
pub struct AnthropicClient {
    http: reqwest::Client,
    config: LlmConfig,
}

#[async_trait]
impl LlmClient for AnthropicClient {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        // Implementation
    }
    // ...
}

// Consumer accepts trait
pub struct LoopEngine {
    llm: Arc<dyn LlmClient>,
    // ...
}
```

### Testing

```rust
// Unit tests in same file
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_happy_path() { ... }

    #[test]
    fn test_function_error_case() { ... }
}

// Integration tests in tests/
// tests/loop_engine_test.rs
```

**Coverage target**: Every public function should have at least one test.

### Validation

Run before each commit:

```bash
otto ci
```

This runs:
- `cargo check` - compilation
- `cargo clippy` - linting
- `cargo fmt --check` - formatting
- `cargo test` - all tests

---

## Commit Message Format

```
<type>(<scope>): <description>

<body explaining what this phase accomplishes>

Phase N of M: <phase name>
```

| Type | Use When |
|------|----------|
| `feat` | New functionality |
| `fix` | Bug fix |
| `refactor` | Code restructuring |
| `test` | Test additions |
| `docs` | Documentation changes |

**Example:**
```
feat(llm): implement AnthropicClient with streaming support

Add LlmClient trait and AnthropicClient implementation with
SSE streaming for token-by-token response handling.

Phase 1 of 7: Core Ralph Loop Engine
```

---

## Naming Conventions

| Context | Convention | Example |
|---------|------------|---------|
| Rust structs/fields | snake_case | `loop_type`, `exec_id` |
| YAML keys | kebab-case | `max-iterations`, `api-key-env` |
| Template variables | kebab-case | `{{phase-name}}`, `{{git-status}}` |
| JSON fields | kebab-case | `"created-at"`, `"loop-type"` |
| Environment variables | SCREAMING_SNAKE | `TASKDAEMON_LLM_MODEL` |

---

## Adding Dependencies

**CRITICAL: Always use `cargo add` to add dependencies. Never manually write version numbers.**

LLM training data contains outdated package versions. Use `cargo add` to get the latest:

```bash
# Core
cargo add tokio --features full
cargo add serde --features derive
cargo add serde_json
cargo add eyre
cargo add thiserror
cargo add tracing

# LLM Client
cargo add reqwest --features json,stream
cargo add reqwest-eventsource
cargo add futures
cargo add async-trait

# Tools
cargo add glob

# Storage
cargo add uuid --features v7

# TUI (Phase 7)
cargo add ratatui
cargo add crossterm
```

**Why this matters:**
- `cargo add` fetches the latest compatible version from crates.io
- LLM-suggested versions like `tokio = "1.28"` may be months/years out of date
- Outdated versions may have security vulnerabilities or missing features
- `cargo add` also handles feature flags correctly

---

## What NOT to Do

- **Don't manually write dependency versions** - use `cargo add` to get latest from crates.io
- Don't skip `otto ci` validation
- Don't commit without tests
- Don't combine multiple phases in one commit
- Don't deviate from design docs without updating them first
- Don't gold-plate - implement exactly what the phase specifies
- Don't push to remote until requested

---

## References

- [taskdaemon.yml](./taskdaemon.yml) - Builtin loop definitions (plan, spec, phase, ralph)
- [docs/](./docs/) - All design documentation
- [taskstore/](../taskstore/) - Sibling crate for persistent state
