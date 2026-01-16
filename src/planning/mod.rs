//! Planning module - conversational plan creation and decomposition
//!
//! This module handles the "input pipeline" for TaskDaemon:
//! 1. PlanningSession - multi-turn conversation to create and refine Plans
//! 2. PlanDecomposer - LLM-driven decomposition of Plans into Specs
//!
//! # Architecture
//!
//! ```text
//! User Input → PlanningSession → Plan → PlanDecomposer → Specs
//!                    ↑                        ↓
//!              LLM (clarify)           StateManager
//! ```
//!
//! The PlanningSession maintains conversation history and uses an LLM to
//! drive a clarification loop until a Plan is agreed upon. Once complete,
//! the PlanDecomposer breaks the Plan into executable Specs.

mod decomposer;
mod session;

pub use decomposer::{DecomposedPlan, DecomposerConfig, PlanDecomposer};
pub use session::{PlanDraft, PlanningSession, SessionConfig, SessionState};
