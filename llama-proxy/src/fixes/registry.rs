//! Fix module registry

use super::{FixAction, FixLogLevel, ResponseFix, ToolCallAccumulator};
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

    /// Apply all enabled fixes that apply to the response, with centralized logging
    pub fn apply_fixes(&self, response: Value) -> Value {
        let mut result = response;

        for fix in &self.fixes {
            if self.is_enabled(fix.name()) && fix.applies(&result) {
                let (new_result, action) = fix.apply(result);
                Self::log_fix_action(fix.name(), &action, fix.log_level());
                result = new_result;
            }
        }

        result
    }

    /// Apply fixes to a streaming chunk, with centralized logging
    pub fn apply_fixes_stream(&self, chunk: Value) -> Value {
        let mut result = chunk;

        for fix in &self.fixes {
            if self.is_enabled(fix.name()) {
                let (new_result, action) = fix.apply_stream(result);
                Self::log_fix_action(fix.name(), &action, fix.log_level());
                result = new_result;
            }
        }

        result
    }

    /// Apply all enabled fixes with request context, with centralized logging
    pub fn apply_fixes_with_context(&self, response: Value, request: &Value) -> Value {
        let mut result = response;

        for fix in &self.fixes {
            if self.is_enabled(fix.name()) && fix.applies_with_context(&result, request) {
                let (new_result, action) = fix.apply_with_context(result, request);
                Self::log_fix_action(fix.name(), &action, fix.log_level());
                result = new_result;
            }
        }

        result
    }

    /// Apply fixes to a streaming chunk with request context, with centralized logging
    pub fn apply_fixes_stream_with_context(&self, chunk: Value, request: &Value) -> Value {
        let mut result = chunk;

        for fix in &self.fixes {
            if self.is_enabled(fix.name()) {
                let (new_result, action) = fix.apply_stream_with_context(result, request);
                Self::log_fix_action(fix.name(), &action, fix.log_level());
                result = new_result;
            }
        }

        result
    }

    /// Apply fixes to a streaming chunk with tool call accumulation, with centralized logging
    pub fn apply_fixes_stream_with_accumulation(
        &self,
        chunk: Value,
        request: &Value,
        accumulator: &mut ToolCallAccumulator,
    ) -> Value {
        let mut result = chunk;

        for fix in &self.fixes {
            if self.is_enabled(fix.name()) {
                let (new_result, action) = fix.apply_stream_with_accumulation(result, request, accumulator);
                Self::log_fix_action(fix.name(), &action, fix.log_level());
                result = new_result;
            }
        }

        result
    }

    /// Apply fixes without request context but with accumulation, with centralized logging
    pub fn apply_fixes_stream_with_accumulation_default(&self, chunk: Value, accumulator: &mut ToolCallAccumulator) -> Value {
        let mut result = chunk;

        for fix in &self.fixes {
            if self.is_enabled(fix.name()) {
                let (new_result, action) = fix.apply_stream_with_accumulation_default(result, accumulator);
                Self::log_fix_action(fix.name(), &action, fix.log_level());
                result = new_result;
            }
        }

        result
    }

    /// Centralized logging for fix actions
    fn log_fix_action(fix_name: &str, action: &FixAction, log_level: FixLogLevel) {
        match action {
            FixAction::NotApplicable => {
                tracing::trace!(fix_name = fix_name, "Fix did not apply");
            }
            FixAction::Fixed {
                original_snippet,
                fixed_snippet,
            } => {
                // Use log level provided by the fix
                match log_level {
                    FixLogLevel::Trace => {
                        tracing::trace!(
                            fix_name = fix_name,
                            original = %original_snippet,
                            fixed = %fixed_snippet,
                            "Detected and fixed malformed content"
                        );
                    }
                    FixLogLevel::Debug => {
                        tracing::debug!(
                            fix_name = fix_name,
                            original = %original_snippet,
                            "Detected malformed content (fixed)"
                        );
                        tracing::trace!(
                            fix_name = fix_name,
                            original = %original_snippet,
                            fixed = %fixed_snippet,
                            "Successfully fixed malformed content"
                        );
                    }
                    FixLogLevel::Info => {
                        tracing::warn!(
                            fix_name = fix_name,
                            original = %original_snippet,
                            "Detected malformed content"
                        );
                        tracing::info!(
                            fix_name = fix_name,
                            original = %original_snippet,
                            fixed = %fixed_snippet,
                            "Successfully fixed malformed content"
                        );
                    }
                    FixLogLevel::Warn => {
                        tracing::warn!(
                            fix_name = fix_name,
                            original = %original_snippet,
                            "Detected malformed content"
                        );
                        tracing::warn!(
                            fix_name = fix_name,
                            original = %original_snippet,
                            fixed = %fixed_snippet,
                            "Successfully fixed malformed content"
                        );
                    }
                }
            }
            FixAction::Failed {
                original_snippet,
                attempted_fix,
            } => {
                // ALWAYS use WARN/ERROR for failures regardless of log level
                tracing::warn!(
                    fix_name = fix_name,
                    original = %original_snippet,
                    "Detected malformed content"
                );
                tracing::error!(
                    fix_name = fix_name,
                    original = %original_snippet,
                    attempted = %attempted_fix,
                    "Failed to fix malformed content"
                );
            }
        }
    }

    /// Configure from config map
    pub fn configure(&mut self, config: &HashMap<String, crate::config::FixModuleConfig>) {
        for (name, module_config) in config {
            if let Some(fix) = self.fixes.iter().find(|f| f.name() == name) {
                self.enabled.insert(name.clone(), module_config.enabled);

                // Apply fix-specific options
                if name == "toolcall_bad_filepath" {
                    if let Some(casted) = Arc::clone(fix).as_any().downcast_ref::<super::ToolcallBadFilepathFix>() {
                        if let Some(remove_dup) = module_config.options.get("remove_duplicate").and_then(|v| v.as_bool()) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixes::ToolcallBadFilepathFix;

    #[test]
    fn test_registry_new() {
        let registry = FixRegistry::new();
        assert!(registry.fixes.is_empty());
        assert!(registry.enabled.is_empty());
    }

    #[test]
    fn test_registry_default() {
        let registry = FixRegistry::default();
        assert!(registry.list_fixes().is_empty());
    }

    #[test]
    fn test_registry_register() {
        let mut registry = FixRegistry::new();
        let fix = Arc::new(ToolcallBadFilepathFix::new(true));
        registry.register(fix);

        assert_eq!(registry.list_fixes().len(), 1);
        assert!(registry.is_enabled("toolcall_bad_filepath"));
    }

    #[test]
    fn test_registry_set_enabled() {
        let mut registry = FixRegistry::new();
        registry.register(Arc::new(ToolcallBadFilepathFix::new(true)));

        assert!(registry.is_enabled("toolcall_bad_filepath"));

        registry.set_enabled("toolcall_bad_filepath", false);
        assert!(!registry.is_enabled("toolcall_bad_filepath"));

        registry.set_enabled("toolcall_bad_filepath", true);
        assert!(registry.is_enabled("toolcall_bad_filepath"));
    }

    #[test]
    fn test_registry_set_enabled_unknown_fix() {
        let mut registry = FixRegistry::new();
        // Should not panic, just do nothing
        registry.set_enabled("unknown_fix", false);
        assert!(!registry.is_enabled("unknown_fix"));
    }

    #[test]
    fn test_registry_is_enabled_unknown() {
        let registry = FixRegistry::new();
        assert!(!registry.is_enabled("nonexistent"));
    }

    #[test]
    fn test_registry_get_fix() {
        let mut registry = FixRegistry::new();
        registry.register(Arc::new(ToolcallBadFilepathFix::new(true)));

        let fix = registry.get_fix("toolcall_bad_filepath");
        assert!(fix.is_some());
        assert_eq!(fix.unwrap().name(), "toolcall_bad_filepath");

        let missing = registry.get_fix("nonexistent");
        assert!(missing.is_none());
    }

    #[test]
    fn test_registry_apply_fixes_no_fixes() {
        let registry = FixRegistry::new();
        let response = serde_json::json!({"test": "value"});
        let result = registry.apply_fixes(response.clone());
        assert_eq!(result, response);
    }

    #[test]
    fn test_registry_apply_fixes_disabled() {
        let mut registry = FixRegistry::new();
        registry.register(Arc::new(ToolcallBadFilepathFix::new(true)));
        registry.set_enabled("toolcall_bad_filepath", false);

        let response = serde_json::json!({"test": "value"});
        let result = registry.apply_fixes(response.clone());
        assert_eq!(result, response);
    }

    #[test]
    fn test_registry_apply_fixes_doesnt_apply() {
        let mut registry = FixRegistry::new();
        registry.register(Arc::new(ToolcallBadFilepathFix::new(true)));

        // Response without tool calls - fix doesn't apply
        let response = serde_json::json!({
            "choices": [{
                "message": {"content": "Hello"}
            }]
        });
        let result = registry.apply_fixes(response.clone());
        assert_eq!(result, response);
    }

    #[test]
    fn test_registry_apply_fixes_applies() {
        let mut registry = FixRegistry::new();
        registry.register(Arc::new(ToolcallBadFilepathFix::new(true)));

        // Response with malformed tool call
        let response = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "name": "write",
                            "arguments": "{\"filePath\":\"/path\",\"filePath\"/broken\"}"
                        }
                    }]
                }
            }]
        });

        let result = registry.apply_fixes(response);
        // The fix should have been applied (arguments should be valid JSON now)
        let args = &result["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"];
        let args_str = args.as_str().unwrap();
        // Should be valid JSON after fix
        assert!(serde_json::from_str::<serde_json::Value>(args_str).is_ok());
    }

    #[test]
    fn test_registry_apply_fixes_stream() {
        let mut registry = FixRegistry::new();
        registry.register(Arc::new(ToolcallBadFilepathFix::new(true)));

        let chunk = serde_json::json!({
            "choices": [{
                "delta": {"content": "test"}
            }]
        });

        let result = registry.apply_fixes_stream(chunk.clone());
        assert_eq!(result, chunk);
    }

    #[test]
    fn test_registry_configure() {
        let mut registry = FixRegistry::new();
        registry.register(Arc::new(ToolcallBadFilepathFix::new(true)));

        let mut options = HashMap::new();
        options.insert("remove_duplicate".to_string(), serde_yaml::Value::Bool(false));

        let mut modules = HashMap::new();
        modules.insert(
            "toolcall_bad_filepath".to_string(),
            crate::config::FixModuleConfig { enabled: false, options },
        );

        registry.configure(&modules);
        assert!(!registry.is_enabled("toolcall_bad_filepath"));
    }

    #[test]
    fn test_registry_configure_unknown_fix() {
        let mut registry = FixRegistry::new();

        let mut modules = HashMap::new();
        modules.insert(
            "unknown_fix".to_string(),
            crate::config::FixModuleConfig {
                enabled: true,
                options: HashMap::new(),
            },
        );

        // Should not panic
        registry.configure(&modules);
    }

    #[test]
    fn test_multiple_fixes() {
        let mut registry = FixRegistry::new();
        registry.register(Arc::new(ToolcallBadFilepathFix::new(true)));
        registry.register(Arc::new(ToolcallBadFilepathFix::new(false)));

        assert_eq!(registry.list_fixes().len(), 2);
        // Both should be enabled by default
        assert!(registry.is_enabled("toolcall_bad_filepath"));
    }

    #[test]
    fn test_apply_fixes_with_context() {
        let mut registry = FixRegistry::new();
        let fix = Arc::new(crate::fixes::ToolcallMalformedArgumentsFix::new());
        registry.register(fix);

        let request = serde_json::json!({
            "tools": [{
                "type": "function",
                "function": {
                    "name": "write",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "file_path": {"type": "string"},
                            "content": {"type": "string"}
                        }
                    }
                }
            }]
        });

        let response = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "name": "write",
                            "arguments": r#"{"content":"data",{}":"/tmp/file.txt"}"#
                        }
                    }]
                }
            }]
        });

        let result = registry.apply_fixes_with_context(response, &request);

        let args = result["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();

        // Should have fixed the malformed argument
        assert!(args.contains(r#""file_path":"#));
        assert!(!args.contains(r#"{}":"#));

        // Should be valid JSON
        let parsed: Result<Value, _> = serde_json::from_str(args);
        assert!(parsed.is_ok());
    }

    #[test]
    fn test_apply_fixes_with_context_disabled() {
        let mut registry = FixRegistry::new();
        let fix = Arc::new(crate::fixes::ToolcallMalformedArgumentsFix::new());
        registry.register(fix);
        registry.set_enabled("toolcall_malformed_arguments", false);

        let request = serde_json::json!({
            "tools": [{
                "type": "function",
                "function": {
                    "name": "write",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "file_path": {"type": "string"},
                            "content": {"type": "string"}
                        }
                    }
                }
            }]
        });

        let response = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "name": "write",
                            "arguments": r#"{"content":"data",{}":"/tmp/file.txt"}"#
                        }
                    }]
                }
            }]
        });

        let result = registry.apply_fixes_with_context(response.clone(), &request);

        // Should not have applied the fix (disabled)
        assert_eq!(result, response);
    }

    #[test]
    fn test_apply_fixes_stream_with_context() {
        let mut registry = FixRegistry::new();
        let fix = Arc::new(crate::fixes::ToolcallMalformedArgumentsFix::new());
        registry.register(fix);

        let request = serde_json::json!({
            "tools": [{
                "type": "function",
                "function": {
                    "name": "write",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "file_path": {"type": "string"},
                            "content": {"type": "string"}
                        }
                    }
                }
            }]
        });

        let chunk = serde_json::json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "function": {
                            "name": "write",
                            "arguments": r#"{"content":"data",{}":"/path.txt"}"#
                        }
                    }]
                }
            }]
        });

        let result = registry.apply_fixes_stream_with_context(chunk, &request);

        let args = result["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();

        assert!(args.contains(r#""file_path":"#));
        assert!(!args.contains(r#"{}":"#));
    }

    #[test]
    fn test_backward_compatibility_with_old_fixes() {
        // Old fixes (like ToolcallBadFilepathFix) should still work with context-aware methods
        let mut registry = FixRegistry::new();
        registry.register(Arc::new(ToolcallBadFilepathFix::new(true)));

        let request = serde_json::json!({"model": "test"});

        let response = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "name": "write",
                            "arguments": "{\"filePath\":\"/path\",\"filePath\":\"/broken\"}"
                        }
                    }]
                }
            }]
        });

        // Old fix should work with context-aware method
        let result = registry.apply_fixes_with_context(response, &request);

        let args = result["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();

        // Should be valid JSON after fix (old fix should have been applied)
        assert!(serde_json::from_str::<serde_json::Value>(args).is_ok());
    }
}
