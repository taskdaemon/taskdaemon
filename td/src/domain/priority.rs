//! Priority levels for task scheduling

use serde::{Deserialize, Serialize};
use tracing::debug;

/// Priority level for Plans and Specs
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Low,
    #[default]
    Normal,
    High,
    Critical,
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        debug!(?self, "Priority::fmt: called");
        match self {
            Self::Low => {
                debug!("Priority::fmt: Low branch");
                write!(f, "low")
            }
            Self::Normal => {
                debug!("Priority::fmt: Normal branch");
                write!(f, "normal")
            }
            Self::High => {
                debug!("Priority::fmt: High branch");
                write!(f, "high")
            }
            Self::Critical => {
                debug!("Priority::fmt: Critical branch");
                write!(f, "critical")
            }
        }
    }
}

impl std::str::FromStr for Priority {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        debug!(%s, "Priority::from_str: called");
        match s.to_lowercase().as_str() {
            "low" => {
                debug!("Priority::from_str: matched low");
                Ok(Self::Low)
            }
            "normal" => {
                debug!("Priority::from_str: matched normal");
                Ok(Self::Normal)
            }
            "high" => {
                debug!("Priority::from_str: matched high");
                Ok(Self::High)
            }
            "critical" => {
                debug!("Priority::from_str: matched critical");
                Ok(Self::Critical)
            }
            _ => {
                debug!(%s, "Priority::from_str: unknown priority");
                Err(format!("Unknown priority: {}", s))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::Low < Priority::Normal);
        assert!(Priority::Normal < Priority::High);
        assert!(Priority::High < Priority::Critical);
    }

    #[test]
    fn test_priority_display() {
        assert_eq!(Priority::Low.to_string(), "low");
        assert_eq!(Priority::Normal.to_string(), "normal");
        assert_eq!(Priority::High.to_string(), "high");
        assert_eq!(Priority::Critical.to_string(), "critical");
    }

    #[test]
    fn test_priority_parse() {
        assert_eq!("low".parse::<Priority>().unwrap(), Priority::Low);
        assert_eq!("HIGH".parse::<Priority>().unwrap(), Priority::High);
        assert!("invalid".parse::<Priority>().is_err());
    }

    #[test]
    fn test_priority_serde() {
        let json = serde_json::to_string(&Priority::High).unwrap();
        assert_eq!(json, "\"high\"");

        let priority: Priority = serde_json::from_str("\"critical\"").unwrap();
        assert_eq!(priority, Priority::Critical);
    }
}
