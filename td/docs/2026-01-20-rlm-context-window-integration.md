# Design Document: RLM-style Context Window Integration in TaskDaemon

**Author:** taskdaemon AI coding assistant
**Date:** 2026-01-20
**Status:** Draft
**Review Passes:** 5/5

## Summary

TaskDaemon currently mitigates context-rot by using a stateless `LlmClient` (fresh context each call), and by relying on tool use (read/grep/glob/run) rather than stuffing large corpora into prompts. However, when a loop must reason over *very large* corpora (huge repos, many documents, long histories), we still risk hitting model context limits or paying high token costs.

This design proposes adding an **RLM-style “external context environment”** to TaskDaemon: store arbitrarily large context outside the model, expose structured tools to *query and navigate it*, and optionally enable recursive deepening. The goal is to support “unlimited context window” workflows without lossy summarization.

The integration is accomplished by:

1. Adding a new **ContextStore** abstraction (file-backed + indexed), optionally persisted via `taskstore`.
2. Introducing new tools (available to loops) for **searching, slicing, and retrieving** chunks from the external context store.
3. Adding loop/prompt conventions so the model treats external context as the canonical source of truth and pulls evidence as needed.
4. (Optional) Implementing a higher-level “recursive query controller” that encourages iterative refinement and bounded cost.

## Problem Statement

### Background

TaskDaemon is an agentic orchestrator that runs loop iterations (`LoopEngine`) against a stateless `LlmClient` and a tool suite. The model’s context is rebuilt every turn/iteration, which reduces context rot.

But TaskDaemon still faces constraints:

- Some tasks require reasoning across **millions of tokens** (large codebases, documentation sets, log histories, multi-worktree artifacts).
- Naively feeding context into prompts is expensive and degrades quality.
- Summarization/compaction loses detail and can corrupt downstream reasoning.

The docs in `docs/rlm-context-window/*` describe the RLM concept: **don’t feed long prompts directly to the neural network; store them externally and allow the model to query them**.

### Problem

We need a robust mechanism for TaskDaemon loops to:

- Access extremely large contexts
- Preserve fidelity (avoid lossy summarization)
- Control cost/latency despite potentially recursive querying
- Integrate cleanly with existing ToolContext sandboxing and worktree-based execution

### Goals

- **Unlimited effective context** for tasks that need it (bounded by disk, not tokens).
- **Fidelity-first** retrieval: no mandatory summarization/compaction.
- **Model-agnostic**: works with OpenAI/Anthropic clients (`LlmClient`).
- **Cost control**: predictable limits, budgets, and guardrails.
- **Good ergonomics**: minimal changes for existing loops; opt-in per loop type/config.

### Non-Goals

- Re-training or modifying core LLMs.
- Implementing a full RAG/vector database system.
- Replacing existing file tools (`read`, `grep`, etc.)—this complements them.
- Guaranteeing globally optimal retrieval/reasoning (we aim for practical reliability).

## Proposed Solution

### Overview

Introduce an **External Context Window** subsystem that can ingest large bodies of text/code into a **ContextStore**, then allow loops to query it using dedicated tools.

At a high level:

1. A loop (or pre-loop step) ingests content into the ContextStore (e.g., entire repo snapshot, docs, conversation logs, taskstore history).
2. During an iteration, the LLM uses new tools:
   - search (regex/substring)
   - list chunks / metadata
   - fetch chunk by id
   - fetch surrounding window (“open around match”)
3. The LLM composes its answer using cited chunk IDs and minimal in-context excerpts.
4. Optional recursion: the model repeats steps (query → narrow → retrieve → synthesize), bounded by a budget.

This aligns with TaskDaemon’s existing architecture: stateless model + rich tool use.

### Architecture

#### New module: `src/context_window/`

Proposed components:

- `ContextStore` (trait)
  - `ingest_text(source_id, text, metadata)` → chunk records
  - `ingest_files(glob/path list)` → chunk records
  - `search(query, options)` → matches (chunk_id, offsets, snippets)
  - `get_chunk(chunk_id)` → full chunk text + metadata
  - `get_window(chunk_id, center_offset, radius)` → windowed text
  - `stats()` → sizes, chunk counts

- `FileContextStore` (implementation)
  - Stores chunks as files under a loop-scoped directory, e.g.:
    - `<worktree>/.taskdaemon/context/<context_id>/chunks/*.txt`
    - `<worktree>/.taskdaemon/context/<context_id>/index.jsonl`

- `TaskStoreBackedIndex` (optional)
  - Use `taskstore` (already in dependencies) to persist metadata/index records.
  - Keep raw text on disk; store indexes/metadata in JSONL/SQLite for fast queries.

#### Tooling

Add tools under `src/tools/builtin/` (or a new `context_window` tool group):

- `context_ingest`
  - Input: paths/globs, max bytes, file filters
  - Output: context_id, chunk ids, stats

- `context_search`
  - Input: context_id, query, mode (regex/plain), case sensitivity, max_results
  - Output: matches w/ chunk_id + offsets + snippet

- `context_get_chunk`
  - Input: chunk_id
  - Output: chunk text + metadata

- `context_get_window`
  - Input: chunk_id, center, radius
  - Output: windowed text

- `context_list`
  - Input: context_id
  - Output: chunk metadata list

These tools mirror the “Ripple environment” idea from the RLM paper/video, but implemented as TaskDaemon-native tools.

#### Integration points

- `LoopEngine::run_agentic_loop` already supports tool-calls with a stateless LLM and structured tool definitions.
- `ToolContext` already enforces sandboxing to a worktree; the ContextStore should live inside the worktree to inherit this security property.
- `StateManager`/`Store` (taskstore) can optionally persist context metadata for crash recovery / replay.

### Data Model

Minimum viable chunk record:

```rust
struct ContextChunkMeta {
  context_id: String,
  chunk_id: String,
  source: String,        // file path or logical id
  byte_start: u64,
  byte_end: u64,
  content_hash: String,  // to detect staleness
  created_at: i64,
}
```

Index storage:

- **Disk**: chunk bodies (plaintext) + a JSONL index file.
- **Optional TaskStore**: store `ContextChunkMeta` as records to enable:
  - fast lookups
  - queries by source
  - crash recovery
  - git-friendly sharing if desired

Note: TaskDaemon’s primary store currently tracks loops/executions; we should keep context indexing separate to avoid bloating existing collections.

### API Design

This feature is primarily internal (tools + loop prompts). No public HTTP API is present today.

User-visible surfaces:

- CLI/config: enable context window for a loop type
  - e.g. in `taskdaemon.yml` or loop config:
    - `context_window: { enabled: true, max_context_bytes: ..., chunk_size: ..., budget: ... }`

- Loop prompt templates: add conventions:
  - “Do not paste large files; instead ingest/search/get relevant chunks.”
  - “Cite chunk IDs when making claims.”

### Implementation Plan

#### Phase 1 — Minimal external context store + tools

1. Create `src/context_window/mod.rs` with `ContextStore` + `FileContextStore`.
2. Define chunking strategy:
   - default chunk size (e.g. 8–32KB) with overlap (e.g. 512–2048 bytes)
   - chunk per file for small files
3. Implement `context_ingest` tool:
   - accept file globs
   - read files (within sandbox)
   - chunk and write chunk bodies + index
4. Implement `context_search` tool:
   - naive scan over chunks (acceptable at first)
   - later optimize using ripgrep or sqlite indexes
5. Implement `context_get_chunk` and `context_get_window`.
6. Wire tool definitions into `ToolExecutor::standard()` and allow enabling via loop config.

#### Phase 2 — Tight integration with TaskDaemon workflows

1. Add prompt snippets (embedded or file prompts) for loops that benefit:
   - codebase understanding
   - deep research over many docs
2. Add a “prelude” convention in `LoopEngine` template context:
   - provide context_id if already ingested
   - expose stats in the prompt
3. Add a standard “evidence discipline”:
   - require citing chunk IDs for factual claims

#### Phase 3 — Persistence and recovery

1. Persist context metadata per execution so a restarted daemon can reuse the same context store.
2. Decide storage:
   - simplest: reuse `.taskdaemon/context/<exec_id>` directory and rebuild index on startup
   - stronger: store `ContextChunkMeta` via `taskstore` for quick listing and integrity checks

#### Phase 4 — Optimization + bounded recursion

1. Add a “budget” mechanism:
   - max tool calls per turn
   - max retrieved bytes per iteration
   - max total context queries per execution
2. Improve search performance:
   - optionally call `rg` (ripgrep) against chunk files
   - or build a sqlite FTS index (optional)
3. Add a controller prompt pattern:
   - the model must first propose a search plan
   - then iteratively retrieve evidence
   - then synthesize

## Alternatives Considered

### Alternative 1: Increase model context window / bigger models

- **Pros:** simplest mental model
- **Cons:** expensive; still suffers context rot and performance degradation at long contexts; vendor-dependent
- **Why not chosen:** RLM-style approach is model-agnostic and often cheaper.

### Alternative 2: Summarization / compaction loops

- **Pros:** easy to implement; reduces tokens
- **Cons:** lossy; repeated summarization compounds errors
- **Why not chosen:** requirement is fidelity-first.

### Alternative 3: Full RAG with embeddings/vector DB

- **Pros:** fast semantic retrieval
- **Cons:** operational complexity; introduces embedding drift; requires additional models/infra; can still miss exact details
- **Why not chosen:** start with simpler deterministic retrieval that matches TaskDaemon’s existing “tools over tokens” philosophy.

## Technical Considerations

### Dependencies

- No new deps required for MVP (can reuse std + existing regex tooling).
- Optional optimization:
  - use existing `grep-*` crates already in `taskdaemon` dependencies
  - or shell out to `rg` via `run_command` tool (less ideal inside tools)

### Performance

- MVP linear scanning may be slow for very large corpora.
- Mitigation: chunk sizes + early exit (`max_results`), later add indexes.

### Security

- Store all context under the worktree to benefit from `ToolContext` sandbox enforcement.
- Ensure ingestion rejects paths outside worktree.
- Avoid executing untrusted code; context tools only read/write text.

### Testing Strategy

- Unit tests for:
  - chunking boundaries/overlap
  - deterministic search results
  - sandbox path validation behavior
- Integration tests:
  - ingest a synthetic repo/doc set
  - run `context_search` and verify retrieval

### Rollout Plan

- Opt-in via loop config.
- Start with a single loop type (e.g., “plan” or a new “research” loop).
- Add metrics:
  - number of chunks
  - bytes ingested
  - tool call counts
  - retrieved bytes per iteration

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Tool-driven querying loops become unbounded/costly | Med | High | Add strict budgets: tool calls, retrieved bytes, max recursion depth |
| Search quality is poor (too many false positives) | Med | Med | Provide query options (regex, case sensitivity), window retrieval, iterative narrowing |
| Index staleness when repo changes | Med | Med | Hash sources, invalidate/re-ingest on mismatch; tie context_id to git SHA |
| Disk usage grows without cleanup | Med | Med | TTL cleanup; store under per-exec directories and remove when exec completes |
| Security regression via ingestion paths | Low | High | Enforce `ToolContext::validate_path` for all file ops |

## Open Questions

- Should context stores be **per-execution** or shared across executions (deduplicated by git SHA)?
- Do we want `taskstore` to persist chunk metadata, or keep it purely file-backed?
- What should default chunking be for code: per-file vs fixed-size chunks?
- Should we expose a higher-level “answer_with_evidence” tool wrapper, or keep primitives?

## References

- `docs/rlm-context-window/summary.md`
- `docs/rlm-context-window/transcript.txt`
- TaskDaemon architecture:
  - `src/loop/engine.rs` (agentic loop + tool calls)
  - `src/llm/client.rs` (stateless calls)
  - `src/tools/context.rs` (sandbox)
- TaskStore:
  - `../taskstore/README.md`

---

## Review Process (Rule of Five)

=== REVIEW PASS 1: COMPLETENESS ===
- Added full architecture, data model, tools list, phases, risks, and rollout.
- Ensured integration points include `LoopEngine`, `ToolContext`, and optional `taskstore`.

=== REVIEW PASS 2: CORRECTNESS ===
- Aligned design with actual TaskDaemon patterns: stateless `LlmClient`, tool-driven loop in `LoopEngine`.
- Ensured sandboxing via worktree-local storage.

=== REVIEW PASS 3: EDGE CASES ===
- Added staleness (git SHA/hash), disk growth cleanup, and budget controls.
- Called out path validation and unbounded recursion as primary failure modes.

=== REVIEW PASS 4: ARCHITECTURE ===
- Chose file-backed store first, optional taskstore indexing later.
- Kept feature opt-in per loop type to avoid impacting baseline loops.

=== REVIEW PASS 5: CLARITY ===
- Tightened the plan into phases.
- Ensured naming is consistent (ContextStore/ContextStore tools).
- Explicitly referenced where it fits in code.
