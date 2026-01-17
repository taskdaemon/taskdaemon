//! Embedded fallback prompts
//!
//! These are compiled into the binary and used when template files are not found.

/// System prompt for the plan creation loop
pub const PLAN_SYSTEM: &str = r#"You are a senior software architect creating a Plan document for a software feature.

Your goal is to produce a Plan that is:
- Complete: All sections filled, no placeholders
- Correct: Logically sound, technically feasible
- Robust: Edge cases and errors handled
- Architectural: Fits the existing system
- Clear: Implementable without ambiguity

You will go through 5 review passes, each focusing on one quality dimension.
Take feedback seriously and revise thoroughly between passes.

Output format: Write the complete Plan as markdown.
Do not output partial Plans or diffs - always output the full document.
"#;

/// Pass 1: COMPLETENESS
pub const PLAN_PASS_1: &str = r#"# Pass 1: COMPLETENESS

Focus ONLY on completeness. Ignore correctness, edge cases, architecture, clarity.

## Required Sections

The Plan MUST have all of these:

### 1. Summary (2-3 sentences)
- What is being built?
- Why is it needed?
- Scope boundary

### 2. Problem Statement
- Background context
- The specific problem to solve
- Who is affected?

### 3. Goals (3-7 bullets)
- What will be true when done?
- Measurable where possible

### 4. Non-Goals (2-5 bullets)
- Explicitly out of scope
- Common assumptions that are wrong

### 5. Proposed Solution
- High-level approach
- Key components
- Data flow (if applicable)

### 6. Implementation Approach
- Phases (if multi-phase)
- Validation criteria per phase

### 7. Risks and Mitigations
- What could go wrong?
- Prevention/detection/recovery

### 8. Open Questions (optional)
- Unresolved questions
- Who can answer them

## Your Task

If any section is missing or has placeholder text (TODO, TBD, etc.):
1. Add the missing content
2. Fill in placeholders with real content

Write the complete updated Plan.
"#;

/// Pass 2: CORRECTNESS
pub const PLAN_PASS_2: &str = r#"# Pass 2: CORRECTNESS

Focus ONLY on correctness. Assume completeness was verified in Pass 1.

## Check For

### Logical Errors
- Does the proposed solution actually solve the stated problem?
- Are the phases in the right order?
- Do dependencies make sense?

### Invalid Assumptions
- Are we assuming APIs/features that don't exist?
- Are we assuming behavior that isn't guaranteed?
- Are we assuming resources that may not be available?

### Technical Feasibility
- Can this actually be built as described?
- Are the technologies mentioned appropriate?
- Are performance claims realistic?

### Consistency
- Do different sections contradict each other?
- Are terms used consistently?
- Do numbers add up?

### Dependency Correctness
- Are all dependencies actually needed?
- Are dependency versions compatible?
- Are there circular dependencies?

## Your Task

For each error found:
1. Identify the specific error
2. Explain why it's wrong
3. Fix it in the Plan

Write the complete corrected Plan.
"#;

/// Pass 3: EDGE CASES
pub const PLAN_PASS_3: &str = r#"# Pass 3: EDGE CASES

Focus ONLY on edge cases and error handling. Assume completeness and correctness.

## Categories to Consider

### Resource Failures
- Network unavailable / timeout
- Disk full / write failure
- Memory exhausted
- File locked by another process
- Permission denied
- Service unavailable

### Timing Issues
- Operation timeout
- Partial completion before failure
- Concurrent access
- Clock skew between systems
- Race conditions

### Data Issues
- Empty input
- Malformed input
- Extremely large input (GB+)
- Unicode / encoding problems
- Null / missing fields
- Duplicate entries

### State Issues
- Operation interrupted mid-way
- Retry after partial failure
- Stale cache / data
- Version mismatch
- Inconsistent state across components

### Security Issues
- Malicious input (injection)
- Unauthorized access attempts
- Credential expiration
- Man-in-the-middle

## Your Task

For each operation in the Plan:
1. List relevant edge cases
2. Verify the Plan addresses them
3. If not addressed, add handling

Write the complete Plan with edge case handling added.
"#;

/// Pass 4: ARCHITECTURE
pub const PLAN_PASS_4: &str = r#"# Pass 4: ARCHITECTURE

Focus ONLY on architectural fit. Assume completeness, correctness, edge cases handled.

## Check For

### Consistency with Existing Patterns
- Does this follow established conventions in the codebase?
- Are naming patterns consistent?
- Does the module structure match existing code?
- Are similar problems solved the same way?

### Integration Points
- Where does this connect to existing code?
- Are those interfaces stable?
- Will this break existing functionality?
- Are there version compatibility concerns?

### Scalability Implications
- Will this work at 10x scale? 100x?
- Are there bottlenecks being created?
- Is state management appropriate?
- Are there single points of failure?

### Technical Debt
- Is this creating shortcuts that will need fixing?
- Are we duplicating existing functionality?
- Are we over-engineering for current needs?
- Will this be hard to change later?

### Maintainability
- Can someone else understand and modify this?
- Are responsibilities clearly separated?
- Is the complexity justified?
- Are there adequate extension points?

## Your Task

For each architectural concern:
1. Identify the specific issue
2. Explain the impact if not addressed
3. Update the Plan with fixes or explicit trade-off documentation

Write the complete Plan with architectural concerns addressed.
"#;

/// Pass 5: CLARITY
pub const PLAN_PASS_5: &str = r#"# Pass 5: CLARITY

Focus ONLY on clarity and implementability. This is the final pass.

## Check For

### Precise Language
- No "should", "might", "could", "probably"
- No "etc.", "and so on", "and more"
- No undefined jargon
- Specific quantities, not "fast", "many", "large"

### Concrete Examples
- Abstract concepts have examples
- Data formats are shown, not just described
- Workflows have step-by-step examples

### Clear Acceptance Criteria
- How do we know when done?
- What tests must pass?
- What metrics must be met?

### Unambiguous Instructions
- Only one valid interpretation
- No room for "I thought it meant..."
- Edge cases explicitly addressed

### Measurable Outcomes
- Performance targets are numbers
- Success criteria are binary (pass/fail)
- Timelines are concrete (if applicable)

## Ambiguity Patterns to Eliminate

BAD: "Make it performant"
GOOD: "P95 latency under 100ms for 1000 concurrent requests"

BAD: "Handle errors appropriately"
GOOD: "On network timeout, retry 3 times with exponential backoff (1s, 2s, 4s), then return error to caller"

BAD: "Support multiple formats"
GOOD: "Support JSON and YAML input formats, detected by file extension"

BAD: "Integrate with the existing system"
GOOD: "Call AuthService.validate_token() before processing requests"

## Your Task

For each ambiguous statement:
1. Identify the ambiguity
2. Replace with precise language

Write the complete final Plan, ready for implementation.
"#;

/// Get the embedded prompt by name
pub fn get_embedded(name: &str) -> Option<&'static str> {
    match name {
        "plan-system" => Some(PLAN_SYSTEM),
        "plan-pass-1" => Some(PLAN_PASS_1),
        "plan-pass-2" => Some(PLAN_PASS_2),
        "plan-pass-3" => Some(PLAN_PASS_3),
        "plan-pass-4" => Some(PLAN_PASS_4),
        "plan-pass-5" => Some(PLAN_PASS_5),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_embedded_plan_system() {
        assert!(get_embedded("plan-system").is_some());
        assert!(get_embedded("plan-system").unwrap().contains("software architect"));
    }

    #[test]
    fn test_get_embedded_all_passes() {
        for i in 1..=5 {
            let name = format!("plan-pass-{}", i);
            assert!(get_embedded(&name).is_some(), "Missing embedded prompt: {}", name);
        }
    }

    #[test]
    fn test_get_embedded_unknown() {
        assert!(get_embedded("unknown-template").is_none());
    }

    #[test]
    fn test_pass_content_differs() {
        // Each pass should have distinct content
        let pass1 = get_embedded("plan-pass-1").unwrap();
        let pass2 = get_embedded("plan-pass-2").unwrap();
        let pass3 = get_embedded("plan-pass-3").unwrap();
        let pass4 = get_embedded("plan-pass-4").unwrap();
        let pass5 = get_embedded("plan-pass-5").unwrap();

        assert!(pass1.contains("COMPLETENESS"));
        assert!(pass2.contains("CORRECTNESS"));
        assert!(pass3.contains("EDGE CASES"));
        assert!(pass4.contains("ARCHITECTURE"));
        assert!(pass5.contains("CLARITY"));
    }
}
