//! Prompt Template System
//!
//! Loads and renders `.pmt` (prompt template) files for the Rule of Five methodology.
//!
//! Template loading chain:
//! 1. `.taskdaemon/prompts/{name}.pmt` (user override)
//! 2. `prompts/{name}.pmt` (repo default)
//! 3. Embedded fallback in code
//!
//! Templates use Handlebars syntax for variable substitution.

pub mod embedded;
mod loader;

pub use loader::{FocusArea, PromptContext, PromptLoader};
