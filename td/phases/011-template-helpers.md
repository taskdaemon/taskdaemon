# Phase: Template Helpers (built-in + custom registration)

**ID:** 011-template-helpers
**Spec:** 020-template-rendering
**Status:** Ready
**Dependencies:** 010-template-engine-core

## Goal
Add a helper registration layer with a small set of built-in helpers and an API for registering custom helpers via configuration.

## Scope
- Add `helpers.rs` with built-in helpers from the spec subset.
- Wire helper registration into `TemplateEngine::new`.
- Define a configuration struct for template engine options including custom helpers.

## Acceptance Criteria
1. Built-in helpers are available in all renders:
   - `truncate(s, len)`
   - `snake_case(s)`
   - `default(value, default)`
   - `json(value)` (pretty JSON)
2. `TemplateEngine::new(config)` registers custom helpers provided in config.
3. Helper failures produce render errors that include helper name in the message (via Handlebars error propagation).

## Files to Create/Modify
- Create: `td/src/templates/helpers.rs`
- Modify: `td/src/templates/engine.rs`
- Modify: `td/src/templates/mod.rs`
- Modify: `td/Cargo.toml` (add deps: `handlebars`, `convert_case`, `serde_json` if not already)

## Test Cases
- Unit: `{{truncate name 5}}` on `"TaskDaemon"` yields `"Ta..."` (or equivalent per chosen semantics).
- Unit: `{{snake_case "HelloWorld"}}` yields `"hello_world"`.
- Unit: `{{default missing "x"}}` yields `"x"` when `missing` is null/empty.
- Unit: `{{json obj}}` yields valid JSON string containing expected keys.
- Unit: custom helper registered in test config can be invoked in template.
