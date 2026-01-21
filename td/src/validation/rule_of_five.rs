//! Rule of Five validation methodology
//!
//! Jeffrey Emanuel's Rule of Five: agents produce best output when forced to review
//! their work 4-5 times until convergence. Each pass examines the plan from a
//! different perspective.
//!
//! Task size guidelines: Small features: 2-3 passes. Large/critical: 4-5 passes.

use std::path::PathBuf;
use tracing::debug;

/// Rule of Five pass definitions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ReviewPass {
    /// Pass 1: Breadth over depth. Get the shape right.
    #[default]
    Draft = 1,
    /// Pass 2: Fix errors, bugs, invalid assumptions. Is the logic sound?
    Correctness = 2,
    /// Pass 3: Can someone else understand and implement this?
    Clarity = 3,
    /// Pass 4: What could go wrong? What's missing? Are we solving the right problem?
    EdgeCases = 4,
    /// Pass 5: Is this something you'd be proud to ship?
    Excellence = 5,
}

impl ReviewPass {
    /// Get the description of what this pass checks
    pub fn description(&self) -> &'static str {
        debug!(?self, "ReviewPass::description: called");
        match self {
            Self::Draft => {
                debug!("ReviewPass::description: Draft branch");
                "Breadth over depth, get the shape right"
            }
            Self::Correctness => {
                debug!("ReviewPass::description: Correctness branch");
                "Fix errors, bugs, invalid assumptions"
            }
            Self::Clarity => {
                debug!("ReviewPass::description: Clarity branch");
                "Can someone else understand and implement this?"
            }
            Self::EdgeCases => {
                debug!("ReviewPass::description: EdgeCases branch");
                "What could go wrong? What's missing?"
            }
            Self::Excellence => {
                debug!("ReviewPass::description: Excellence branch");
                "Is this something you'd be proud to ship?"
            }
        }
    }

    /// Get detailed instructions for this pass
    pub fn instructions(&self) -> &'static str {
        debug!(?self, "ReviewPass::instructions: called");
        match self {
            Self::Draft => {
                debug!("ReviewPass::instructions: Draft branch");
                "Create the initial draft:\n\
                 - Focus on breadth over depth\n\
                 - Get the shape right using the template\n\
                 - Start with the problem, not the solution\n\
                 - Be explicit about non-goals\n\
                 - Include alternatives considered"
            }
            Self::Correctness => {
                debug!("ReviewPass::instructions: Correctness branch");
                "Review with 'fresh eyes' for correctness:\n\
                 - Are there logical errors or contradictions?\n\
                 - Are assumptions stated and validated?\n\
                 - Are there technical inaccuracies?\n\
                 - Is the logic sound?\n\
                 - Fix what you find."
            }
            Self::Clarity => {
                debug!("ReviewPass::instructions: Clarity branch");
                "Review as a new team member who must implement this:\n\
                 - Can someone else understand and implement this?\n\
                 - What's confusing? Simplify.\n\
                 - Are the steps concrete and actionable?\n\
                 - Is the language precise and unambiguous?\n\
                 - Are code examples provided where helpful?"
            }
            Self::EdgeCases => {
                debug!("ReviewPass::instructions: EdgeCases branch");
                "Review for edge cases and completeness:\n\
                 - What are the weakest parts?\n\
                 - What could go wrong?\n\
                 - What's missing?\n\
                 - Are we solving the right problem?\n\
                 - Are error scenarios documented?"
            }
            Self::Excellence => {
                debug!("ReviewPass::instructions: Excellence branch");
                "Final pass - make it shine:\n\
                 - Is this something you'd be proud to ship?\n\
                 - Does it fit the larger system?\n\
                 - Is the approach consistent with existing patterns?\n\
                 - Are there better alternative approaches?\n\
                 - Polish and refine."
            }
        }
    }

    /// Get the validation command/script name for this pass
    pub fn validation_command(&self) -> &'static str {
        debug!(?self, "ReviewPass::validation_command: called");
        match self {
            Self::Draft => {
                debug!("ReviewPass::validation_command: Draft branch");
                "plan-pass-1.sh"
            }
            Self::Correctness => {
                debug!("ReviewPass::validation_command: Correctness branch");
                "plan-pass-2.sh"
            }
            Self::Clarity => {
                debug!("ReviewPass::validation_command: Clarity branch");
                "plan-pass-3.sh"
            }
            Self::EdgeCases => {
                debug!("ReviewPass::validation_command: EdgeCases branch");
                "plan-pass-4.sh"
            }
            Self::Excellence => {
                debug!("ReviewPass::validation_command: Excellence branch");
                "plan-pass-5.sh"
            }
        }
    }

    /// Get the numeric value (1-5)
    pub fn number(&self) -> u8 {
        debug!(?self, "ReviewPass::number: called");
        *self as u8
    }

    /// Create from numeric value
    pub fn from_number(n: u8) -> Option<Self> {
        debug!(%n, "ReviewPass::from_number: called");
        match n {
            1 => {
                debug!("ReviewPass::from_number: n=1 Draft branch");
                Some(Self::Draft)
            }
            2 => {
                debug!("ReviewPass::from_number: n=2 Correctness branch");
                Some(Self::Correctness)
            }
            3 => {
                debug!("ReviewPass::from_number: n=3 Clarity branch");
                Some(Self::Clarity)
            }
            4 => {
                debug!("ReviewPass::from_number: n=4 EdgeCases branch");
                Some(Self::EdgeCases)
            }
            5 => {
                debug!("ReviewPass::from_number: n=5 Excellence branch");
                Some(Self::Excellence)
            }
            _ => {
                debug!("ReviewPass::from_number: invalid number branch");
                None
            }
        }
    }

    /// Get the next pass, or None if this is the last
    pub fn next(&self) -> Option<Self> {
        debug!(?self, "ReviewPass::next: called");
        Self::from_number(self.number() + 1)
    }

    /// Check if this is the final pass
    pub fn is_final(&self) -> bool {
        debug!(?self, "ReviewPass::is_final: called");
        *self == Self::Excellence
    }
}

impl std::fmt::Display for ReviewPass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        debug!(?self, "ReviewPass::fmt: called");
        write!(
            f,
            "Pass {} ({})",
            self.number(),
            match self {
                Self::Draft => {
                    debug!("ReviewPass::fmt: Draft branch");
                    "Draft"
                }
                Self::Correctness => {
                    debug!("ReviewPass::fmt: Correctness branch");
                    "Correctness"
                }
                Self::Clarity => {
                    debug!("ReviewPass::fmt: Clarity branch");
                    "Clarity"
                }
                Self::EdgeCases => {
                    debug!("ReviewPass::fmt: EdgeCases branch");
                    "Edge Cases"
                }
                Self::Excellence => {
                    debug!("ReviewPass::fmt: Excellence branch");
                    "Excellence"
                }
            }
        )
    }
}

/// Result of a single pass
#[derive(Debug, Clone)]
pub struct PassResult {
    /// Which pass was executed
    pub pass: ReviewPass,
    /// Issues found during review
    pub issues_found: Vec<String>,
    /// Changes made to address issues
    pub changes_made: Vec<String>,
    /// Whether the pass converged (no issues found)
    pub converged: bool,
}

impl PassResult {
    /// Create a converged result (no issues found)
    pub fn converged(pass: ReviewPass) -> Self {
        debug!(?pass, "PassResult::converged: called");
        Self {
            pass,
            issues_found: vec![],
            changes_made: vec![],
            converged: true,
        }
    }

    /// Create a result with issues
    pub fn with_issues(pass: ReviewPass, issues: Vec<String>, changes: Vec<String>) -> Self {
        debug!(?pass, issues_count = %issues.len(), changes_count = %changes.len(), "PassResult::with_issues: called");
        Self {
            pass,
            issues_found: issues,
            changes_made: changes,
            converged: false,
        }
    }
}

/// Plan refinement loop context
#[derive(Debug, Clone)]
pub struct PlanRefinementContext {
    /// Plan identifier
    pub plan_id: String,
    /// Path to the plan file being refined
    pub plan_file: PathBuf,
    /// Current review pass
    pub current_pass: ReviewPass,
    /// History of pass results
    pub pass_history: Vec<PassResult>,
}

impl PlanRefinementContext {
    /// Create a new refinement context
    pub fn new(plan_id: impl Into<String>, plan_file: impl Into<PathBuf>) -> Self {
        let plan_id = plan_id.into();
        let plan_file = plan_file.into();
        debug!(%plan_id, ?plan_file, "PlanRefinementContext::new: called");
        Self {
            plan_id,
            plan_file,
            current_pass: ReviewPass::default(),
            pass_history: vec![],
        }
    }

    /// Record a pass result and advance if converged
    pub fn record_result(&mut self, result: PassResult) {
        debug!(?result.pass, %result.converged, "PlanRefinementContext::record_result: called");
        let converged = result.converged;
        self.pass_history.push(result);

        if converged {
            debug!("PlanRefinementContext::record_result: converged branch - advancing pass");
            self.advance_pass();
        } else {
            debug!("PlanRefinementContext::record_result: not converged branch - staying on current pass");
        }
    }

    /// Check if refinement is complete
    ///
    /// Refinement completes when:
    /// 1. All 5 passes complete with final pass converged, OR
    /// 2. Two consecutive passes converge (stable state)
    pub fn is_complete(&self) -> bool {
        debug!(history_len = %self.pass_history.len(), ?self.current_pass, "PlanRefinementContext::is_complete: called");
        if self.pass_history.len() < 2 {
            debug!("PlanRefinementContext::is_complete: history too short branch");
            return false;
        }

        // Check for two consecutive converged passes
        let last_two = &self.pass_history[self.pass_history.len() - 2..];
        let consecutive_converged = last_two.iter().all(|r| r.converged);

        if consecutive_converged {
            debug!("PlanRefinementContext::is_complete: two consecutive converged branch");
            return true;
        }

        // Check if we completed pass 5 with convergence
        if self.current_pass.is_final()
            && let Some(last) = self.pass_history.last()
        {
            debug!("PlanRefinementContext::is_complete: checking pass 5 convergence branch");
            return last.pass.is_final() && last.converged;
        }

        debug!("PlanRefinementContext::is_complete: not complete branch");
        false
    }

    /// Advance to next pass
    pub fn advance_pass(&mut self) {
        debug!(?self.current_pass, "PlanRefinementContext::advance_pass: called");
        if let Some(next) = self.current_pass.next() {
            debug!(
                ?next,
                "PlanRefinementContext::advance_pass: advancing to next pass branch"
            );
            self.current_pass = next;
        } else {
            debug!("PlanRefinementContext::advance_pass: already at final pass branch");
        }
        // If already at Clarity (pass 5), stay there
    }

    /// Get total iterations completed
    pub fn total_iterations(&self) -> usize {
        debug!(history_len = %self.pass_history.len(), "PlanRefinementContext::total_iterations: called");
        self.pass_history.len()
    }

    /// Get iterations for current pass
    pub fn current_pass_iterations(&self) -> usize {
        debug!(?self.current_pass, "PlanRefinementContext::current_pass_iterations: called");
        self.pass_history.iter().filter(|r| r.pass == self.current_pass).count()
    }

    /// Get the validation command for current pass
    pub fn validation_command(&self) -> String {
        debug!(?self.current_pass, ?self.plan_file, "PlanRefinementContext::validation_command: called");
        format!(
            ".taskdaemon/validators/{} {}",
            self.current_pass.validation_command(),
            self.plan_file.display()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_review_pass_progression() {
        let mut pass = ReviewPass::Draft;
        assert_eq!(pass.number(), 1);

        pass = pass.next().unwrap();
        assert_eq!(pass, ReviewPass::Correctness);

        pass = pass.next().unwrap();
        assert_eq!(pass, ReviewPass::Clarity);

        pass = pass.next().unwrap();
        assert_eq!(pass, ReviewPass::EdgeCases);

        pass = pass.next().unwrap();
        assert_eq!(pass, ReviewPass::Excellence);
        assert!(pass.is_final());
        assert!(pass.next().is_none());
    }

    #[test]
    fn test_review_pass_from_number() {
        assert_eq!(ReviewPass::from_number(1), Some(ReviewPass::Draft));
        assert_eq!(ReviewPass::from_number(5), Some(ReviewPass::Excellence));
        assert_eq!(ReviewPass::from_number(0), None);
        assert_eq!(ReviewPass::from_number(6), None);
    }

    #[test]
    fn test_context_completion_two_consecutive() {
        let mut ctx = PlanRefinementContext::new("plan-1", "/tmp/plan.md");

        // Not complete with just one pass
        ctx.record_result(PassResult::converged(ReviewPass::Draft));
        assert!(!ctx.is_complete());

        // Complete with two consecutive converged
        ctx.record_result(PassResult::converged(ReviewPass::Correctness));
        assert!(ctx.is_complete());
    }

    #[test]
    fn test_context_completion_pass_five() {
        let mut ctx = PlanRefinementContext::new("plan-1", "/tmp/plan.md");
        ctx.current_pass = ReviewPass::Excellence;

        // Not complete if pass 5 didn't converge
        ctx.record_result(PassResult::with_issues(
            ReviewPass::Excellence,
            vec!["issue".into()],
            vec![],
        ));
        ctx.record_result(PassResult::with_issues(
            ReviewPass::Excellence,
            vec!["issue".into()],
            vec![],
        ));
        assert!(!ctx.is_complete());

        // Complete when pass 5 converges
        ctx.record_result(PassResult::converged(ReviewPass::Excellence));
        assert!(ctx.is_complete());
    }

    #[test]
    fn test_context_not_complete_early() {
        let ctx = PlanRefinementContext::new("plan-1", "/tmp/plan.md");
        assert!(!ctx.is_complete());

        let mut ctx2 = PlanRefinementContext::new("plan-1", "/tmp/plan.md");
        ctx2.record_result(PassResult::with_issues(
            ReviewPass::Draft,
            vec!["missing section".into()],
            vec!["added section".into()],
        ));
        assert!(!ctx2.is_complete());
    }

    #[test]
    fn test_pass_result_construction() {
        let converged = PassResult::converged(ReviewPass::Draft);
        assert!(converged.converged);
        assert!(converged.issues_found.is_empty());

        let with_issues = PassResult::with_issues(ReviewPass::Correctness, vec!["issue1".into()], vec!["fix1".into()]);
        assert!(!with_issues.converged);
        assert_eq!(with_issues.issues_found.len(), 1);
    }

    #[test]
    fn test_validation_command() {
        let ctx = PlanRefinementContext::new("plan-1", "/tmp/my-plan.md");
        assert!(ctx.validation_command().contains("plan-pass-1.sh"));
        assert!(ctx.validation_command().contains("/tmp/my-plan.md"));
    }

    #[test]
    fn test_pass_descriptions() {
        for n in 1..=5 {
            let pass = ReviewPass::from_number(n).unwrap();
            assert!(!pass.description().is_empty());
            assert!(!pass.instructions().is_empty());
            assert!(!pass.validation_command().is_empty());
        }
    }
}
