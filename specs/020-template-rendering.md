# Spec: Template Rendering System

**ID:** 020-template-rendering  
**Status:** Draft  
**Dependencies:** [018-loop-type-definitions]

## Summary

Implement a Handlebars-based template rendering system for loop prompts that supports custom helpers, partials, and dynamic context building. The system should be extensible and provide useful debugging capabilities.

## Acceptance Criteria

1. **Handlebars Integration**
   - Template loading and caching
   - Custom helper registration
   - Partial support
   - Error handling with context

2. **Built-in Helpers**
   - String manipulation
   - Date/time formatting
   - JSON operations
   - Conditional logic helpers

3. **Context Building**
   - Dynamic context assembly
   - Variable resolution
   - Nested data access
   - Safe defaults

4. **Developer Experience**
   - Template validation
   - Render previews
   - Error diagnostics
   - Hot reload in development

## Implementation Phases

### Phase 1: Core Setup
- Handlebars integration
- Basic template loading
- Context building
- Error handling

### Phase 2: Helper System
- Built-in helpers
- Custom helper API
- Helper registration
- Documentation

### Phase 3: Advanced Features
- Template caching
- Partial support
- Template inheritance
- Performance optimization

### Phase 4: Developer Tools
- Template validation
- Preview system
- Debug helpers
- Documentation generator

## Technical Details

### Module Structure
```
src/templates/
├── mod.rs
├── engine.rs      # Template engine
├── helpers.rs     # Built-in helpers
├── context.rs     # Context building
├── loader.rs      # Template loading
├── cache.rs       # Template caching
└── debug.rs       # Debug utilities
```

### Template Engine
```rust
pub struct TemplateEngine {
    handlebars: Handlebars<'static>,
    loader: TemplateLoader,
    cache: TemplateCache,
    debug_mode: bool,
}

impl TemplateEngine {
    pub fn new(config: TemplateConfig) -> Result<Self, Error> {
        let mut handlebars = Handlebars::new();
        
        // Configure
        handlebars.set_strict_mode(config.strict_mode);
        handlebars.set_dev_mode(config.debug_mode);
        
        // Register built-in helpers
        register_builtin_helpers(&mut handlebars)?;
        
        // Register custom helpers
        for (name, helper) in config.custom_helpers {
            handlebars.register_helper(&name, helper);
        }
        
        Ok(Self {
            handlebars,
            loader: TemplateLoader::new(config.template_dirs),
            cache: TemplateCache::new(config.cache_size),
            debug_mode: config.debug_mode,
        })
    }
    
    pub async fn render(
        &self,
        template_name: &str,
        context: &Value,
    ) -> Result<String, RenderError> {
        // Load template (with caching)
        let template = self.loader.load(template_name).await?;
        
        // Add debug info if enabled
        let context = if self.debug_mode {
            self.enrich_context_for_debug(context, template_name)
        } else {
            context.clone()
        };
        
        // Render
        self.handlebars
            .render_template(&template, &context)
            .map_err(|e| self.enhance_error(e, template_name, &context))
    }
}
```

### Built-in Helpers
```rust
pub fn register_builtin_helpers(handlebars: &mut Handlebars) -> Result<(), Error> {
    // String helpers
    handlebars_helper!(truncate: |s: str, len: u64| {
        if s.len() <= len as usize {
            s.to_string()
        } else {
            format!("{}...", &s[..len as usize - 3])
        }
    });
    
    handlebars_helper!(snake_case: |s: str| {
        s.to_case(Case::Snake)
    });
    
    // Date/time helpers
    handlebars_helper!(format_date: |date: str, format: str| {
        DateTime::parse_from_rfc3339(date)
            .map(|d| d.format(format).to_string())
            .unwrap_or_else(|_| date.to_string())
    });
    
    handlebars_helper!(time_ago: |date: str| {
        DateTime::parse_from_rfc3339(date)
            .map(|d| {
                let duration = Utc::now() - d.with_timezone(&Utc);
                humanize_duration(duration)
            })
            .unwrap_or_else(|_| date.to_string())
    });
    
    // JSON helpers
    handlebars_helper!(json: |value: Value| {
        serde_json::to_string_pretty(&value).unwrap_or_default()
    });
    
    handlebars_helper!(json_path: |obj: Value, path: str| {
        jsonpath::select(&obj, path).ok()
            .and_then(|v| v.first().cloned())
            .unwrap_or(Value::Null)
    });
    
    // Logic helpers
    handlebars_helper!(default: |value: Value, default: Value| {
        if value.is_null() || (value.is_string() && value.as_str() == Some("")) {
            default
        } else {
            value
        }
    });
    
    handlebars_helper!(includes: |array: Vec<Value>, item: Value| {
        array.contains(&item)
    });
    
    // Register all helpers
    handlebars.register_helper("truncate", Box::new(truncate));
    handlebars.register_helper("snake_case", Box::new(snake_case));
    handlebars.register_helper("format_date", Box::new(format_date));
    handlebars.register_helper("time_ago", Box::new(time_ago));
    handlebars.register_helper("json", Box::new(json));
    handlebars.register_helper("json_path", Box::new(json_path));
    handlebars.register_helper("default", Box::new(default));
    handlebars.register_helper("includes", Box::new(includes));
    
    Ok(())
}
```

### Context Building
```rust
pub struct ContextBuilder {
    base: Value,
    overlays: Vec<Value>,
}

impl ContextBuilder {
    pub fn new() -> Self {
        Self {
            base: json!({}),
            overlays: Vec::new(),
        }
    }
    
    pub fn with_base(mut self, base: Value) -> Self {
        self.base = base;
        self
    }
    
    pub fn overlay(mut self, overlay: Value) -> Self {
        self.overlays.push(overlay);
        self
    }
    
    pub fn add_standard_context(mut self) -> Self {
        self.overlay(json!({
            "timestamp": Utc::now().to_rfc3339(),
            "env": std::env::var("TASKDAEMON_ENV").unwrap_or_else(|_| "production".to_string()),
            "version": env!("CARGO_PKG_VERSION"),
        }))
    }
    
    pub fn build(self) -> Value {
        let mut result = self.base;
        
        for overlay in self.overlays {
            merge_json(&mut result, overlay);
        }
        
        result
    }
}
```

### Template Examples
```handlebars
{{!-- System prompt template --}}
You are a helpful AI assistant working on {{loop_type}} tasks.

{{#if progress}}
## Previous Progress
{{#each progress.iterations}}
### Iteration {{@index}}
{{this.summary}}
{{/each}}
{{/if}}

## Current Task
{{description}}

{{#if constraints}}
## Constraints
{{#each constraints}}
- {{this}}
{{/each}}
{{/if}}

Available tools: {{#each tools}}{{this}}{{#unless @last}}, {{/unless}}{{/each}}

{{!-- User prompt template --}}
{{#if (eq loop_type "spec")}}
Please create a spec for: {{spec_name}}

Requirements:
{{requirements}}

Dependencies: {{#if dependencies}}{{json dependencies}}{{else}}None{{/if}}
{{/if}}
```

### Debug Features
```rust
impl TemplateEngine {
    fn enrich_context_for_debug(&self, context: &Value, template_name: &str) -> Value {
        let mut enriched = context.clone();
        
        if let Some(obj) = enriched.as_object_mut() {
            obj.insert("__debug".to_string(), json!({
                "template": template_name,
                "render_time": Utc::now().to_rfc3339(),
                "available_helpers": self.list_helpers(),
                "context_keys": obj.keys().collect::<Vec<_>>(),
            }));
        }
        
        enriched
    }
    
    fn enhance_error(&self, error: RenderError, template: &str, context: &Value) -> RenderError {
        RenderError::WithContext {
            error: Box::new(error),
            template: template.to_string(),
            line: self.find_error_line(&error),
            context_snippet: self.extract_context_snippet(context),
        }
    }
}
```

## Notes

- Templates should be validated at load time to catch syntax errors early
- Consider implementing a template test framework
- Provide good examples and documentation for template authors
- Support template versioning for backward compatibility