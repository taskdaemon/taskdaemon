# Spec: Loop Type Definitions

**ID:** 018-loop-type-definitions  
**Status:** Draft  
**Dependencies:** [003-loop-engine-core, 017-config-system]

## Summary

Create extensible loop type definitions that specify the behavior, templates, and configuration for different loop types (Plan, Spec, Phase, Ralph). Support dynamic loading and validation of loop type configurations.

## Acceptance Criteria

1. **Loop Type Schema**
   - Define structure for loop types
   - Template configuration
   - Tool specifications
   - Progress strategies

2. **Type Registry**
   - Dynamic registration
   - Type discovery
   - Validation rules
   - Version management

3. **Template System**
   - Handlebars integration
   - Template validation
   - Variable passing
   - Helper functions

4. **Extensibility**
   - Custom loop types
   - Plugin architecture
   - Type inheritance
   - Override mechanisms

## Implementation Phases

### Phase 1: Type Definitions
- Core loop type structure
- Built-in types (Plan, Spec, Phase)
- Type registry implementation
- Basic validation

### Phase 2: Template Integration
- Handlebars setup
- Template loading
- Variable resolution
- Helper registration

### Phase 3: Configuration
- YAML schema for types
- Inheritance system
- Override logic
- Validation framework

### Phase 4: Extensions
- Plugin interface
- Custom type loading
- Type composition
- Advanced features

## Technical Details

### Module Structure
```
src/loops/types/
├── mod.rs
├── definition.rs  # Type definitions
├── registry.rs    # Type registry
├── builtin.rs     # Built-in types
├── template.rs    # Template system
├── validation.rs  # Type validation
└── loader.rs      # Dynamic loading
```

### Type Definition
```rust
pub struct LoopTypeDefinition {
    pub name: String,
    pub version: Version,
    pub description: String,
    pub config: LoopTypeConfig,
    pub templates: TemplateConfig,
    pub tools: Vec<String>,
    pub progress_strategy: ProgressStrategy,
    pub validators: Vec<Box<dyn LoopValidator>>,
}

pub struct TemplateConfig {
    pub system_template: PathBuf,
    pub user_template: Option<PathBuf>,
    pub iteration_template: Option<PathBuf>,
    pub completion_template: Option<PathBuf>,
    pub helpers: HashMap<String, Helper>,
}

pub enum ProgressStrategy {
    SystemCaptured {
        extract_patterns: Vec<Regex>,
        summarize: bool,
    },
    Explicit {
        progress_field: String,
        merge_strategy: MergeStrategy,
    },
    Custom {
        handler: Box<dyn ProgressHandler>,
    },
}

pub struct LoopTypeConfig {
    pub max_iterations: u32,
    pub timeout: Duration,
    pub required_tools: HashSet<String>,
    pub optional_tools: HashSet<String>,
    pub inherits_from: Option<String>,
    pub variables: HashMap<String, VariableDefinition>,
}
```

### Built-in Types
```rust
pub fn builtin_loop_types() -> Vec<LoopTypeDefinition> {
    vec![
        LoopTypeDefinition {
            name: "plan".to_string(),
            version: Version::new(1, 0, 0),
            description: "High-level planning loop".to_string(),
            config: LoopTypeConfig {
                max_iterations: 50,
                timeout: Duration::from_secs(1800),
                required_tools: hashset!["read", "write"],
                optional_tools: hashset!["search"],
                inherits_from: None,
                variables: hashmap! {
                    "goal" => VariableDefinition::required(VarType::String),
                    "context" => VariableDefinition::optional(VarType::Object),
                },
            },
            templates: TemplateConfig {
                system_template: "templates/plan/system.hbs".into(),
                user_template: Some("templates/plan/user.hbs".into()),
                ..Default::default()
            },
            progress_strategy: ProgressStrategy::SystemCaptured {
                extract_patterns: vec![
                    Regex::new(r"## Next Steps\n([\s\S]+?)(?:\n##|$)").unwrap(),
                ],
                summarize: true,
            },
            ..Default::default()
        },
        // Spec, Phase, Ralph types...
    ]
}
```

### Type Registry
```rust
pub struct LoopTypeRegistry {
    types: HashMap<String, LoopTypeDefinition>,
    validators: Vec<Box<dyn TypeValidator>>,
}

impl LoopTypeRegistry {
    pub fn register(&mut self, definition: LoopTypeDefinition) -> Result<(), RegistryError> {
        // Validate definition
        for validator in &self.validators {
            validator.validate(&definition)?;
        }
        
        // Check for conflicts
        if let Some(existing) = self.types.get(&definition.name) {
            if existing.version >= definition.version {
                return Err(RegistryError::VersionConflict);
            }
        }
        
        // Register
        self.types.insert(definition.name.clone(), definition);
        Ok(())
    }
    
    pub fn get(&self, name: &str) -> Option<&LoopTypeDefinition> {
        self.types.get(name)
    }
    
    pub fn resolve_inheritance(&mut self) -> Result<(), RegistryError> {
        // Resolve inherits_from references
        // Merge configurations
        // Validate final types
    }
}
```

### Template System
```rust
pub struct TemplateEngine {
    handlebars: Handlebars<'static>,
    helpers: HashMap<String, Helper>,
}

impl TemplateEngine {
    pub fn render_loop_prompt(
        &self,
        loop_type: &LoopTypeDefinition,
        context: &LoopContext,
    ) -> Result<String, TemplateError> {
        // Load templates
        let system = self.load_template(&loop_type.templates.system_template)?;
        let user = loop_type.templates.user_template
            .as_ref()
            .map(|p| self.load_template(p))
            .transpose()?;
        
        // Build context
        let mut render_context = json!({
            "loop_type": loop_type.name,
            "iteration": context.iteration,
            "progress": context.progress,
            "variables": context.variables,
        });
        
        // Render
        let system_content = self.handlebars.render_template(&system, &render_context)?;
        let user_content = user.map(|t| self.handlebars.render_template(&t, &render_context)).transpose()?;
        
        Ok(format_prompt(system_content, user_content))
    }
}
```

## Notes

- Loop types should be validated at load time to catch errors early
- Template errors should provide helpful debugging information
- Consider supporting WASM plugins for custom loop types
- Provide good defaults while allowing full customization