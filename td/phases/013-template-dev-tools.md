# Phase: Template Dev Tools (validation + preview + diagnostics)

**ID:** 013-template-dev-tools
**Spec:** 020-template-rendering
**Status:** Ready
**Dependencies:** 010-template-engine-core, 011-template-helpers

## Goal
Improve developer experience with template validation and richer diagnostics, plus a small preview utility suitable for CLI integration later.

## Scope
- Add a validation API that checks template syntax at load time.
- Add an optional debug context injection (e.g., `__debug`) when debug mode is enabled.
- Provide a `render_preview(...)` helper that returns rendered string plus metadata (template used, available helpers, top-level context keys).

## Acceptance Criteria
1. `TemplateEngine::validate(template_name)` returns `Ok(())` for valid templates and a typed error for invalid templates.
2. When debug mode is enabled, render context contains `__debug.template` and `__debug.render_time`.
3. Preview API returns:
   - rendered output
   - template name
   - list of registered helpers (at least their names)
4. Errors include template name and (when available) line/column information from Handlebars.

## Files to Create/Modify
- Create: `td/src/templates/debug.rs`
- Modify: `td/src/templates/engine.rs`
- Modify: `td/src/templates/error.rs`
- Modify: `td/src/templates/mod.rs`

## Test Cases
- Unit: validate catches syntax errors (e.g., unclosed `{{#if}}`).
- Unit: debug context is present only when debug mode enabled.
- Unit: preview returns helper list containing `json` and `default`.
