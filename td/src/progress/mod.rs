//! Progress tracking for Ralph loop iterations
//!
//! Since each iteration starts with a fresh LLM context window (the core Ralph
//! Wiggum pattern), we must explicitly tell the LLM what happened in previous
//! iterations. The `ProgressStrategy` trait abstracts this, with
//! `SystemCapturedProgress` as the default implementation.

mod strategy;
mod system_captured;

pub use strategy::{IterationContext, ProgressStrategy};
pub use system_captured::SystemCapturedProgress;
