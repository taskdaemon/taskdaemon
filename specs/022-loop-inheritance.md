# Spec: Loop Inheritance System

**ID:** 022-loop-inheritance
**Status:** Draft
**Dependencies:** [018-loop-type-definitions]

## Summary

Implement loop type inheritance to enable base configurations for reuse. This allows creating specialized loop types that inherit common settings, tools, and templates from base types while overriding specific behaviors.

## Acceptance Criteria

1. **Inheritance Model**
   - Single inheritance support
   - Override mechanisms
   - Merge strategies
   - Validation rules

2. **Configuration Merging**
   - Deep merge for objects
   - Array handling strategies
   - Explicit override syntax
   - Conflict resolution

3. **Template Inheritance**
   - Template override
   - Partial inheritance
   - Helper inheritance
   - Block system

4. **Validation**
   - Inheritance chain validation
   - Circular dependency detection
   - Type compatibility checks
   - Override validation

## Implementation Phases

### Phase 1: Inheritance Model
- Define inheritance structure
- Basic parent resolution
- Simple overrides
- Validation framework

### Phase 2: Merge Engine
- Deep merge implementation
- Array strategies
- Conflict detection
- Override syntax

### Phase 3: Template System
- Template inheritance
- Block definitions
- Partial overrides
- Helper inheritance

### Phase 4: Advanced Features
- Multiple inheritance paths
- Mixin support
- Dynamic inheritance
- Debug tools

## Technical Details

### Module Structure
```
src/loops/inheritance/
├── mod.rs
├── model.rs       # Inheritance model
├── merger.rs      # Configuration merger
├── resolver.rs    # Inheritance resolver
├── validator.rs   # Validation logic
├── templates.rs   # Template inheritance
└── debug.rs       # Debug utilities
```

### Inheritance Model
```rust
pub struct InheritableLoopType {
    pub definition: LoopTypeDefinition,
    pub parent: Option<String>,
    pub overrides: OverrideSpec,
    pub merge_strategy: MergeStrategy,
}

pub struct OverrideSpec {
    pub config: Option<PartialLoopConfig>,
    pub tools: ToolOverride,
    pub templates: TemplateOverride,
    pub validators: ValidatorOverride,
}

pub enum MergeStrategy {
    Deep,           // Deep merge all fields
    Shallow,        // Only merge top-level
    Replace,        // Complete replacement
    Custom(Box<dyn MergeFunction>),
}

pub struct ToolOverride {
    pub add: Vec<String>,
    pub remove: Vec<String>,
    pub replace: Option<Vec<String>>,
}

pub struct TemplateOverride {
    pub templates: HashMap<String, PathBuf>,
    pub blocks: HashMap<String, String>,
    pub helpers: HashMap<String, Helper>,
}
```

### Inheritance Resolution
```rust
pub struct InheritanceResolver {
    types: HashMap<String, InheritableLoopType>,
    resolved_cache: RwLock<HashMap<String, LoopTypeDefinition>>,
}

impl InheritanceResolver {
    pub async fn resolve(&self, type_name: &str) -> Result<LoopTypeDefinition, ResolveError> {
        // Check cache
        if let Some(resolved) = self.resolved_cache.read().await.get(type_name) {
            return Ok(resolved.clone());
        }

        // Build inheritance chain
        let chain = self.build_inheritance_chain(type_name)?;

        // Validate chain
        self.validate_chain(&chain)?;

        // Merge definitions
        let resolved = self.merge_chain(chain)?;

        // Cache result
        self.resolved_cache.write().await.insert(type_name.to_string(), resolved.clone());

        Ok(resolved)
    }

    fn build_inheritance_chain(&self, type_name: &str) -> Result<Vec<&InheritableLoopType>, ResolveError> {
        let mut chain = Vec::new();
        let mut current = type_name;
        let mut visited = HashSet::new();

        while let Some(loop_type) = self.types.get(current) {
            // Check for circular inheritance
            if !visited.insert(current) {
                return Err(ResolveError::CircularInheritance(current.to_string()));
            }

            chain.push(loop_type);

            if let Some(parent) = &loop_type.parent {
                current = parent;
            } else {
                break;
            }
        }

        // Reverse to get base-first order
        chain.reverse();
        Ok(chain)
    }
}
```

### Configuration Merging
```rust
pub struct ConfigMerger {
    strategies: HashMap<String, MergeStrategy>,
}

impl ConfigMerger {
    pub fn merge_chain(&self, chain: Vec<&InheritableLoopType>) -> Result<LoopTypeDefinition, MergeError> {
        let mut result = LoopTypeDefinition::default();

        for loop_type in chain {
            result = self.merge_single(result, loop_type)?;
        }

        Ok(result)
    }

    fn merge_single(
        &self,
        mut base: LoopTypeDefinition,
        inheritable: &InheritableLoopType,
    ) -> Result<LoopTypeDefinition, MergeError> {
        let strategy = &inheritable.merge_strategy;

        // Merge config
        if let Some(config_override) = &inheritable.overrides.config {
            base.config = self.merge_config(base.config, config_override, strategy)?;
        }

        // Merge tools
        base.tools = self.merge_tools(base.tools, &inheritable.overrides.tools)?;

        // Merge templates
        base.templates = self.merge_templates(base.templates, &inheritable.overrides.templates)?;

        // Apply other fields
        base.name = inheritable.definition.name.clone();
        base.version = inheritable.definition.version.clone();
        base.description = inheritable.definition.description.clone();

        Ok(base)
    }

    fn merge_tools(&self, mut base: Vec<String>, override: &ToolOverride) -> Result<Vec<String>, MergeError> {
        if let Some(replacement) = &override.replace {
            return Ok(replacement.clone());
        }

        // Remove specified tools
        base.retain(|tool| !override.remove.contains(tool));

        // Add new tools
        for tool in &override.add {
            if !base.contains(tool) {
                base.push(tool.clone());
            }
        }

        Ok(base)
    }
}
```

### Template Inheritance
```rust
pub struct TemplateInheritance {
    engine: TemplateEngine,
    block_registry: HashMap<String, HashMap<String, String>>, // type -> block_name -> content
}

impl TemplateInheritance {
    pub fn register_type_templates(
        &mut self,
        type_name: &str,
        templates: &TemplateConfig,
    ) -> Result<(), Error> {
        // Register blocks
        for (block_name, content) in &templates.blocks {
            self.block_registry
                .entry(type_name.to_string())
                .or_default()
                .insert(block_name.clone(), content.clone());
        }

        // Process templates with inheritance
        self.process_template_inheritance(type_name, templates)?;

        Ok(())
    }

    pub fn render_with_inheritance(
        &self,
        type_name: &str,
        template_name: &str,
        context: &Value,
    ) -> Result<String, RenderError> {
        // Build inheritance context
        let mut render_context = context.clone();

        // Add available blocks
        if let Some(blocks) = self.block_registry.get(type_name) {
            render_context["blocks"] = json!(blocks);
        }

        // Render with block support
        self.engine.render(template_name, &render_context)
    }
}
```

### Example Usage
```yaml
# Base loop type
loops:
  base_code_loop:
    description: "Base configuration for code generation loops"
    tools:
      - read
      - write
      - edit
      - run
    config:
      max_iterations: 30
      timeout: 20m
    templates:
      system_template: templates/base/system.hbs
      blocks:
        header: "You are an AI assistant that writes code."
        tools_intro: "You have access to the following tools:"

# Specialized loop inheriting from base
  python_loop:
    inherits: base_code_loop
    description: "Specialized loop for Python development"
    overrides:
      tools:
        add:
          - pytest
          - black
      config:
        max_iterations: 40  # Override specific field
      templates:
        blocks:
          header: "You are an AI assistant specialized in Python development."
        user_template: templates/python/user.hbs

# Another specialization
  rust_loop:
    inherits: base_code_loop
    description: "Specialized loop for Rust development"
    overrides:
      tools:
        add:
          - cargo
          - clippy
        remove:
          - run  # Remove generic run in favor of cargo
      templates:
        blocks:
          header: "You are an AI assistant specialized in Rust development."
        user_template: templates/rust/user.hbs
```

### Template with Blocks
```handlebars
{{!-- Base system template --}}
{{block "header"}}

{{block "tools_intro"}}
{{#each tools}}
- {{this}}: {{lookup ../tool_descriptions this}}
{{/each}}

{{#if (block "additional_context")}}
{{block "additional_context"}}
{{/if}}

{{!-- Child template can override blocks --}}
{{#override "header"}}
You are an AI assistant specialized in {{language}} development.
Follow {{language}} best practices and idioms.
{{/override}}

{{#override "additional_context"}}
## Language-Specific Guidelines
- Use type hints for all functions
- Follow PEP 8 style guide
- Write comprehensive docstrings
{{/override}}
```

## Notes

- Inheritance chains should be shallow (max 3-4 levels) for maintainability
- Provide clear documentation on override behavior
- Consider implementing a visualization tool for inheritance hierarchies
- Support for mixins could be added in future versions