//! Validation module for plan refinement
//!
//! Implements the Rule of Five methodology for systematic plan review and improvement.

mod rule_of_five;

pub use rule_of_five::{PassResult, PlanRefinementContext, ReviewPass};
