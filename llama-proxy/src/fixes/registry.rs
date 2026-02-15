//! Fix module registry

use super::ResponseFix;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Registry for all available response fixes
pub struct FixRegistry {
    fixes: Vec<Arc<dyn ResponseFix>>,
    enabled: HashMap<String, bool>,
}

impl FixRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            fixes: Vec::new(),
            enabled: HashMap::new(),
        }
    }

    /// Register a fix module
    pub fn register(&mut self, fix: Arc<dyn ResponseFix>) {
        let name = fix.name().to_string();
        self.fixes.push(fix);
        self.enabled.insert(name, true);
    }

    /// Enable or disable a fix by name
    pub fn set_enabled(&mut self, name: &str, enabled: bool) {
        if self.enabled.contains_key(name) {
            self.enabled.insert(name.to_string(), enabled);
        }
    }

    /// Check if a fix is enabled
    pub fn is_enabled(&self, name: &str) -> bool {
        self.enabled.get(name).copied().unwrap_or(false)
    }

    /// Get all registered fixes
    pub fn list_fixes(&self) -> &[Arc<dyn ResponseFix>] {
        &self.fixes
    }

    /// Get a fix by name
    pub fn get_fix(&self, name: &str) -> Option<&Arc<dyn ResponseFix>> {
        self.fixes.iter().find(|f| f.name() == name)
    }

    /// Apply all enabled fixes that apply to the response
    pub fn apply_fixes(&self, response: Value) -> Value {
        let mut result = response;

        for fix in &self.fixes {
            if self.is_enabled(fix.name()) && fix.applies(&result) {
                tracing::debug!(
                    fix_name = fix.name(),
                    "Applying response fix"
                );
                result = fix.apply(result);
            }
        }

        result
    }

    /// Apply fixes to a streaming chunk
    pub fn apply_fixes_stream(&self, chunk: Value) -> Value {
        let mut result = chunk;

        for fix in &self.fixes {
            if self.is_enabled(fix.name()) {
                result = fix.apply_stream(result);
            }
        }

        result
    }

    /// Configure from config map
    pub fn configure(&mut self, config: &HashMap<String, crate::config::FixModuleConfig>) {
        for (name, module_config) in config {
            if let Some(fix) = self.fixes.iter().find(|f| f.name() == name) {
                self.enabled.insert(name.clone(), module_config.enabled);

                // Apply fix-specific options
                if name == "toolcall_bad_filepath" {
                    if let Some(casted) = Arc::clone(fix)
                        .as_any()
                        .downcast_ref::<super::ToolcallBadFilepathFix>()
                    {
                        if let Some(remove_dup) = module_config
                            .options
                            .get("remove_duplicate")
                            .and_then(|v| v.as_bool())
                        {
                            casted.set_remove_duplicate(remove_dup);
                        }
                    }
                }
            }
        }
    }
}

impl Default for FixRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Extension trait to allow downcasting
pub trait AsAny: std::any::Any {
    fn as_any(&self) -> &dyn std::any::Any;
}

impl<T: std::any::Any> AsAny for T {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
