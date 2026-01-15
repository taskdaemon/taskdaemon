# Config Schema

**Author:** Scott A. Idler
**Date:** 2026-01-15
**Status:** Active

---

## Configuration Hierarchy

Configuration is resolved in layers, each overriding the previous:

```
┌─────────────────────────────────────────────────────────────────┐
│ Layer 1: Hardcoded Defaults (in binary)                         │
├─────────────────────────────────────────────────────────────────┤
│ Layer 2: Global Config                                          │
│ ~/.config/taskdaemon/taskdaemon.yml                             │
│ - API key env var                                               │
│ - Preferred model                                               │
│ - Personal defaults                                             │
├─────────────────────────────────────────────────────────────────┤
│ Layer 3: Per-Project Config                                     │
│ <project>/.taskdaemon.yml                                       │
│ - Validator command for THIS project                            │
│ - Concurrency tuned for THIS project                            │
│ - Project-specific loop types                                   │
├─────────────────────────────────────────────────────────────────┤
│ Layer 4: Environment Variables                                  │
│ TASKDAEMON_<KEY>=value                                          │
├─────────────────────────────────────────────────────────────────┤
│ Layer 5: CLI Flags                                              │
│ --<key>=value                                                   │
└─────────────────────────────────────────────────────────────────┘
```

---

## File Locations

| File | Purpose | Checked In? |
|------|---------|-------------|
| `~/.config/taskdaemon/taskdaemon.yml` | User's global preferences | No (personal) |
| `<project>/.taskdaemon.yml` | Project-specific settings | Yes (to project repo) |
| `~/.config/taskdaemon/loops/*.yml` | User-defined loop types | No (personal) |
| `<project>/.taskdaemon/loops/*.yml` | Project-specific loop types | Yes (to project repo) |

---

## Full Schema

```yaml
# === LLM Configuration ===
llm:
  provider: anthropic                    # anthropic (others later)
  model: claude-sonnet-4-20250514        # Model ID
  api-key-env: ANTHROPIC_API_KEY         # Env var name containing API key
  base-url: https://api.anthropic.com    # Optional, for proxies/custom endpoints
  max-tokens: 16384                      # Max output tokens per request
  timeout-ms: 300000                     # 5 min request timeout

# === Concurrency Limits ===
concurrency:
  max-loops: 50                          # Max concurrent loop tasks
  max-api-calls: 10                      # Max concurrent LLM API calls
  max-worktrees: 50                      # Max git worktrees on disk

# === Validation Defaults ===
validation:
  command: "otto ci"                     # Default validator command
  iteration-timeout-ms: 300000           # Max time per iteration (5 min)
  max-iterations: 100                    # Default safety limit

# === Git Configuration ===
git:
  worktree-dir: /tmp/taskdaemon/worktrees  # Where to create worktrees
  disk-quota-gb: 100                       # Warn if disk usage exceeds this

# === Storage Configuration ===
storage:
  taskstore-dir: .taskstore              # Relative to project root
  jsonl-warn-mb: 50                      # JSONL size warning threshold
  jsonl-error-mb: 200                    # JSONL size error threshold

# === Loop Type Paths ===
loops:
  paths:                                 # Searched in order, later overrides earlier
    - builtin                            # Embedded plan, spec, phase, ralph
    - ~/.config/taskdaemon/loops         # User global customs
    - .taskdaemon/loops                  # Project-specific customs
```

---

## Defaults

If no config files exist, these defaults are used:

```yaml
llm:
  provider: anthropic
  model: claude-sonnet-4-20250514
  api-key-env: ANTHROPIC_API_KEY
  base-url: https://api.anthropic.com
  max-tokens: 16384
  timeout-ms: 300000

concurrency:
  max-loops: 50
  max-api-calls: 10
  max-worktrees: 50

validation:
  command: "otto ci"
  iteration-timeout-ms: 300000
  max-iterations: 100

git:
  worktree-dir: /tmp/taskdaemon/worktrees
  disk-quota-gb: 100

storage:
  taskstore-dir: .taskstore
  jsonl-warn-mb: 50
  jsonl-error-mb: 200

loops:
  paths:
    - builtin
    - ~/.config/taskdaemon/loops
    - .taskdaemon/loops
```

---

## Minimal Configs

### Minimal Global Config

```yaml
# ~/.config/taskdaemon/taskdaemon.yml
llm:
  api-key-env: ANTHROPIC_API_KEY
```

### Minimal Per-Project Config

```yaml
# .taskdaemon.yml
validation:
  command: "make test"
```

---

## Environment Variables

All config keys can be overridden via environment variables:

| Config Key | Environment Variable |
|------------|---------------------|
| `llm.model` | `TASKDAEMON_LLM_MODEL` |
| `llm.api-key-env` | `TASKDAEMON_LLM_API_KEY_ENV` |
| `concurrency.max-loops` | `TASKDAEMON_CONCURRENCY_MAX_LOOPS` |
| `validation.command` | `TASKDAEMON_VALIDATION_COMMAND` |

**Pattern:** `TASKDAEMON_<SECTION>_<KEY>` (uppercase, hyphens become underscores)

---

## CLI Flags

Common overrides available as CLI flags:

```bash
taskdaemon start \
  --max-loops=5 \
  --max-iterations=50 \
  --model=claude-opus-4-20250514
```

| Flag | Overrides |
|------|-----------|
| `--max-loops` | `concurrency.max-loops` |
| `--max-iterations` | `validation.max-iterations` |
| `--model` | `llm.model` |
| `--timeout` | `validation.iteration-timeout-ms` |
| `--validator` | `validation.command` |

---

## What Goes Where

| Setting | Global | Per-Project | Why |
|---------|--------|-------------|-----|
| `llm.api-key-env` | ✓ | - | Personal secret |
| `llm.model` | ✓ | ✓ | Personal pref, project can override |
| `validation.command` | - | ✓ | Project-specific |
| `concurrency.*` | ✓ | ✓ | Defaults, project can tune |
| `git.worktree-dir` | ✓ | ✓ | Machine-specific |
| `storage.taskstore-dir` | - | ✓ | Project-specific |
| Custom loop types | ✓ | ✓ | Both levels |

---

## Loop Type Loading Order

Loop types are loaded from paths in order. Later definitions override earlier:

1. **builtin** - plan, spec, phase, ralph (embedded in binary)
2. **~/.config/taskdaemon/loops/** - User's custom loop types
3. **.taskdaemon/loops/** - Project-specific loop types

**Example override:** Project wants a different `phase` loop:

```yaml
# .taskdaemon/loops/phase.yml
# The key IS the name (no separate 'name' field needed)
phase:
  prompt-template: |
    # Custom phase prompt for this project
    {{spec-content}}
    ...
  validation-command: "npm test"  # Different validator
  success-exit-code: 0
  max-iterations: 50
```

This overrides the builtin `phase` for this project only.

---

## References

- [Implementation Details](./implementation-details.md) - Loop schema, domain types
- [Main Design](./taskdaemon-design.md) - Architecture overview
