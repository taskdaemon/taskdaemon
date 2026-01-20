// TriggerCondition enum and related types.

// Enum representing possible conditions under which a trigger is executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerCondition {
    OnDataChange,
    OnSchedule(String),  // Placeholder for cron-like expressions
    OnEvent(String),     // Placeholder for specific event names
}
