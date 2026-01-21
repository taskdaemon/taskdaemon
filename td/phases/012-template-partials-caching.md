# Phase: Template Partials + Caching

**ID:** 012-template-partials-caching
**Spec:** 020-template-rendering
**Status:** Ready
**Dependencies:** 010-template-engine-core

## Goal
Support Handlebars partials and add a simple template cache to avoid repeated filesystem reads.

## Scope
- Implement a `TemplateCache` keyed by template name/path.
- Loader should optionally cache loaded template source.
- Add support for loading/registering partials (either explicit list or discoverable under a `partials/` dir).

## Acceptance Criteria
1. Rendering a template that includes `{{> header}}` works when `header` partial exists.
2. Template content is loaded from disk once per name when caching is enabled.
3. Cache can be disabled (e.g., debug/dev mode).

## Files to Create/Modify
- Create: `td/src/templates/cache.rs`
- Modify: `td/src/templates/loader.rs`
- Modify: `td/src/templates/engine.rs`
- Modify: `td/src/templates/mod.rs`

## Test Cases
- Unit: partial render: `base.hbs` includes `{{> p}}` and `p.hbs` is loaded; output matches expected.
- Unit: cache hit: load counter or spy loader proves only one disk read for repeated renders.
- Unit: cache disabled: repeated renders cause multiple loads.
