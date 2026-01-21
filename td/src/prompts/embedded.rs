//! Embedded prompts
//!
//! These are compiled into the binary from .pmt files at build time.

use tracing::debug;

/// Consolidated Rule of Five plan prompt
pub const PLAN: &str = include_str!("../../prompts/plan.pmt");

/// Title generator prompt
pub const TITLE_GENERATOR: &str = include_str!("../../prompts/title.pmt");

/// Get the embedded prompt by name
pub fn get_embedded(name: &str) -> Option<&'static str> {
    debug!(%name, "get_embedded: called");
    match name {
        "plan" => {
            debug!("get_embedded: matched plan");
            Some(PLAN)
        }
        "title" => {
            debug!("get_embedded: matched title");
            Some(TITLE_GENERATOR)
        }
        _ => {
            debug!("get_embedded: no match found");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_embedded_plan() {
        assert!(get_embedded("plan").is_some());
        let plan = get_embedded("plan").unwrap();
        assert!(plan.contains("software architect"));
        assert!(plan.contains("Rule of Five"));
        assert!(plan.contains("COMPLETENESS"));
        assert!(plan.contains("CORRECTNESS"));
        assert!(plan.contains("EDGE CASES"));
        assert!(plan.contains("ARCHITECTURE"));
        assert!(plan.contains("CLARITY"));
    }

    #[test]
    fn test_get_embedded_title() {
        assert!(get_embedded("title").is_some());
        assert!(get_embedded("title").unwrap().contains("title"));
    }

    #[test]
    fn test_get_embedded_unknown() {
        assert!(get_embedded("unknown-template").is_none());
    }
}
