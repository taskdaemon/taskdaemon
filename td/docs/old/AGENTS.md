# Warning: Superseded Documents

**DO NOT implement directly from documents in this directory.**

These documents represent earlier iterations of the TaskDaemon design that have been superseded. The current authoritative design is in the parent `docs/` directory:

- `taskdaemon-design.md` - Main design document
- `coordinator-design.md` - Inter-loop communication protocol
- `execution-model-design.md` - Git worktree management, crash recovery
- `tui-design.md` - Terminal UI design

## What Changed

| Old Concept | Current Approach |
|-------------|------------------|
| AWL (Agentic Workflow Language) | Ralph loops with YAML config |
| Hardcoded loop types | All loop types defined via YAML configuration |
| PRD/TS terminology | Plan/Spec terminology |
| Complex workflow DSL | Simple validator command (exit 0 = done) |

## Why These Exist

These documents are preserved for historical context:
- Understanding design evolution
- Referencing discarded approaches to avoid repeating mistakes
- Some implementation patterns (actor model, TaskStore integration) still apply

## If You're an Agent

1. **Read the current docs first** - Start with `../taskdaemon-design.md`
2. **Don't copy old patterns** - AWL, hardcoded enums, Python validators are all gone
3. **Ask if unsure** - If something in old docs contradicts current docs, current wins
