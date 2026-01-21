# TaskDaemon Workspace Status

This document summarizes the migration to a Rust workspace structure and current state of each crate.

## Workspace Overview

The taskdaemon project has been restructured into a Cargo workspace with three crates:

| Crate | Directory | Binary | Description |
|-------|-----------|--------|-------------|
| **taskdaemon** | `td/` | `td` | Extensible Ralph Wiggum Loop Orchestrator |
| **taskstore** | `ts/` | `taskstore` | Generic persistent state management with SQLite+JSONL+Git |
| **contextstore** | `cs/` | `cs` | RLM-style external context store for unlimited context windows |

## Migration Sources

| Destination | Source | Status |
|-------------|--------|--------|
| `td/` | `~/repos/taskdaemon/taskdaemon` (main branch) | 100% migrated |
| `ts/` | `~/repos/taskdaemon/taskstore` | 100% migrated |
| `cs/` | New crate (scaffolded in workspace) | Complete |

## Shared Dependencies

All scaffold-standard dependencies are defined at workspace level in the root `Cargo.toml`:

### Runtime Dependencies
- `clap` - CLI argument parsing
- `colored` - Terminal colors
- `dirs` - Standard directories
- `env_logger` - Logging facade
- `eyre` - Error handling
- `log` - Logging
- `serde` / `serde_json` / `serde_yaml` - Serialization
- `chrono`, `uuid`, `tokio`, `tracing`, etc.

### Dev Dependencies (from scaffold)
- `assert_cmd` - CLI integration testing
- `criterion` - Benchmarking
- `predicates` - Assertion matchers
- `proptest` - Property-based testing
- `serial_test` - Serial test execution
- `tempfile` - Temporary files for tests

## Build Configuration

### Otto Task Runner

Single `.otto.yml` at workspace root (not per-crate). Tasks include:

| Task | Description |
|------|-------------|
| `otto ci` | Full CI pipeline (lint + check + test) |
| `otto check` | Clippy, format check, compile check |
| `otto test` | Run all workspace tests |
| `otto cov` | Coverage report with detailed per-file breakdown |
| `otto build` | Release build for all crates |
| `otto install` | Install all binaries to ~/.cargo/bin |

### Running CI

```bash
cd ~/repos/taskdaemon/taskdaemon-workspace
otto ci          # Full pipeline
otto cov         # With coverage
otto cov --details --fail-under 60  # Detailed coverage with threshold
```

## Current Build Status

### Compilation
- All three crates compile successfully
- Clippy passes with `-D warnings`
- Format check passes

### Tests
| Crate | Tests | Status |
|-------|-------|--------|
| contextstore (cs) | 2 | PASS |
| taskdaemon (td) | 351 | PASS |
| taskstore (ts) | 18 | 3 FAILING |

The taskstore tests were pre-existing failures from the original repo - the CRUD operations need completion.

## Crate Implementation Status

### contextstore (cs/) - COMPLETE
RLM-style external context storage for unlimited context windows.

**Features:**
- File ingestion with 32KB chunking and 2KB overlap
- Regex-based search across chunks
- Chunk retrieval and windowing
- CLI with all operations

### taskstore (ts/) - ~70% Complete
Generic persistent state management.

**Implemented:**
- SQLite + JSONL storage backend
- Basic CRUD scaffolding
- CLI with sync, list, get, collections, indexes, sql commands

**Needs work:**
- Record write/update/delete operations
- Filter system application
- Some test fixes

### taskdaemon (td/) - ~40% Complete (Infrastructure Phase)
Ralph Wiggum Loop orchestrator.

**Implemented:**
- LLM client trait with Anthropic & OpenAI support
- Domain types (Loop, Execution, Priority, etc.)
- Tool system with 13+ builtin tools
- State manager for persistence
- CLI structure
- 351 passing unit tests

**Needs work:**
- Loop execution engine
- Coordinator message routing
- Scheduler priority queue
- File system watcher triggers
- TUI completion

## Directory Structure

```
taskdaemon-workspace/
├── Cargo.toml          # Workspace manifest
├── Cargo.lock          # Dependency lock
├── .otto.yml           # Build/CI configuration
├── WORKSPACE-STATUS.md # This file
├── td/                 # taskdaemon crate
│   ├── Cargo.toml
│   ├── src/
│   ├── docs/
│   └── tests/
├── ts/                 # taskstore crate
│   ├── Cargo.toml
│   ├── src/
│   └── docs/
└── cs/                 # contextstore crate
    ├── Cargo.toml
    └── src/
```

## Next Steps

1. Fix taskstore test failures (CRUD operations)
2. Complete taskdaemon loop execution engine
3. Set up GitHub Actions CI (workflow files copied to ts/.github/workflows/ need adjustment)
4. Integration testing across crates
