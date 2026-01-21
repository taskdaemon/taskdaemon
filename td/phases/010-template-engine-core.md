# Phase: Template Engine Core (load + render + context)

**ID:** 010-template-engine-core
**Spec:** 020-template-rendering
**Status:** Ready
**Dependencies:** 018-loop-type-definitions

## Goal
Implement the minimal Handlebars-based template engine capable of loading a template by name/path, building a render context, and rendering prompt strings with good error messages.

## Scope
- Introduce `td/src/templates/` module with a `TemplateEngine` that wraps `handlebars::Handlebars`.
- Implement basic template loading (from filesystem) without caching.
- Implement a small context builder utility to merge base + overlays.
- Implement error types that include template name and underlying Handlebars error.

## Acceptance Criteria
1. `TemplateEngine::new(...)` constructs a Handlebars registry and supports strict/dev mode toggles.
2. `TemplateEngine::render(template_name, &serde_json::Value)` renders a template loaded from disk.
3. Missing template returns a typed error that includes the attempted path/name.
4. Render error returns a typed error that includes the template name and Handlebars render error string.
5. `ContextBuilder` can merge overlays onto a base JSON object deterministically.

## Files to Create/Modify
- Create: `td/src/templates/mod.rs`
- Create: `td/src/templates/engine.rs`
- Create: `td/src/templates/loader.rs`
- Create: `td/src/templates/context.rs`
- Create: `td/src/templates/error.rs`
- Modify: `td/src/lib.rs` (export templates module)
- Modify: `td/Cargo.toml` (add deps: `handlebars`, `serde`, `serde_json`, `thiserror` as needed)

## Implementation Notes
- Loader: accept `template_dirs: Vec<PathBuf>` and search in order; template names can be relative paths.
- Use `tokio::fs::read_to_string` if async is already pervasive; otherwise keep sync `std::fs` for now.
- Context merge: object keys from overlay override base; non-objects replace.

## Test Cases
- Unit: `ContextBuilder` merges nested objects; overlay overrides scalar.
- Unit: rendering a simple template `"Hello {{name}}"` with context `{ "name": "World" }` returns `"Hello World"`.
- Unit: rendering with missing template returns `TemplateError::NotFound { name, searched_paths: ... }`.
- Unit: rendering template with syntax error returns `TemplateError::Render { name, source: ... }`.
