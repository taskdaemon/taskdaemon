# Spec: Domain Types

**ID:** 006-domain-types
**Status:** Draft
**Dependencies:** None

## Summary

Define the core domain types that represent the fundamental entities in the TaskDaemon system: Plan, Spec, Phase, and LoopExecution. These types form the foundation of the system's data model and state management.

## Acceptance Criteria

1. **Type Definitions**
   - Complete domain model for Plan, Spec, Phase
   - LoopExecution tracking type
   - Status enumerations
   - Relationship modeling

2. **Serialization**
   - JSON serialization for persistence
   - Human-readable formats for debugging
   - Version compatibility
   - Migration support

3. **Validation**
   - Business rule enforcement
   - Relationship integrity
   - Status transition rules
   - Data constraints

4. **Type Safety**
   - Strong typing throughout
   - Builder patterns for construction
   - Immutability where appropriate
   - Error types for violations

## Implementation Phases

### Phase 1: Core Types
- Define Plan, Spec, Phase structures
- Create status enumerations
- Add relationship fields
- Implement basic validation

### Phase 2: Execution Tracking
- LoopExecution type
- Progress tracking fields
- Timestamp management
- Status transitions

### Phase 3: Serialization
- Serde implementations
- Custom serializers where needed
- Format versioning
- Migration utilities

### Phase 4: Type Extensions
- Builder patterns
- Validation methods
- Helper functions
- Display implementations

## Technical Details

### Module Structure
```
src/domain/
├── mod.rs
├── plan.rs        # Plan type and logic
├── spec.rs        # Spec type and logic
├── phase.rs       # Phase type and logic
├── execution.rs   # LoopExecution type
├── status.rs      # Status enums
└── validation.rs  # Validation rules
```

### Core Types
```rust
pub struct Plan {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub status: ExecutionStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: PlanMetadata,
}

pub struct Spec {
    pub id: Uuid,
    pub plan_id: Uuid,
    pub name: String,
    pub description: String,
    pub dependencies: Vec<Uuid>,
    pub acceptance_criteria: Vec<String>,
    pub status: ExecutionStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct Phase {
    pub id: Uuid,
    pub spec_id: Uuid,
    pub name: String,
    pub description: String,
    pub order: u32,
    pub status: ExecutionStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub enum ExecutionStatus {
    Draft,
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Blocked,
}
```

### Relationships
- Plan → many Specs (1:N)
- Spec → many Phases (1:N)
- Spec → many Spec dependencies (N:N)
- All types → many LoopExecutions (1:N)

## Notes

- IDs should use UUID v7 for time-ordered generation
- All timestamps should use UTC
- Consider implementing soft deletes for audit trails
- Status transitions should be validated to prevent invalid states