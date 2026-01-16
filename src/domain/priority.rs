//! Priority levels for task scheduling

use serde::{Deserialize, Serialize};

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
        match self {
            Self::Low => write!(f, "low"),
            Self::Normal => write!(f, "normal"),
            Self::High => write!(f, "high"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

impl std::str::FromStr for Priority {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "low" => Ok(Self::Low),
            "normal" => Ok(Self::Normal),
            "high" => Ok(Self::High),
            "critical" => Ok(Self::Critical),
            _ => Err(format!("Unknown priority: {}", s)),
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
