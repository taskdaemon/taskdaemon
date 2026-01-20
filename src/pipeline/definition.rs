// Main pipeline definition types and validation logic.

// Struct representing a mapping from one data structure to another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataMapping {
    pub source: String,
    pub target: String,
}

// Struct representing individual field mapping between data structures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldMapping {
    pub field_name: String,
    pub mapping: DataMapping,
}

// Enum representing different kinds of data transformations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transform {
    Uppercase,
    Lowercase,
    Custom(String), // Placeholder for custom transformations
}
