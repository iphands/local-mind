//! Fix module registry

use super::{FixAction, FixError, FixLogLevel, ResponseFix, ToolCallAccumulator};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Registry for all available response fixes
pub struct FixRegistry {
    fixes: Vec<Arc<dyn ResponseFix>>,
    enabled: HashMap<String, bool>,
}

/// How [`truncate_snippet`] counts its clip budget.
#[derive(Clone, Copy)]
pub(crate) enum SnippetLimit {
    /// Budget counts characters; the clip keeps `max - 3` chars + `...`.
    Chars,
    /// Budget counts bytes; the clip keeps up to `max` bytes + `...`, widened
    /// down to the nearest char boundary. Faithful to the historical
    /// byte-slicing helpers it replaces (which panicked on mid-sequence cuts).
    Bytes,
}

/// Clip a snippet for a log field, marking that it was clipped. Char-safe:
/// model output routinely contains multibyte sequences. The single truncator
/// shared by all `fixes/` modules.
pub(crate) fn truncate_snippet(s: &str, max: usize, limit: SnippetLimit) -> String {
    match limit {
        SnippetLimit::Chars => {
            if s.chars().count() <= max {
                s.to_string()
            } else {
                let kept: String = s.chars().take(max.saturating_sub(3)).collect();
                format!("{}...", kept)
            }
        }
        SnippetLimit::Bytes => {
            if s.len() <= max {
                s.to_string()
            } else {
                // is_char_boundary(0) is always true, so the scan terminates.
                let mut end = max;
                while !s.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}...", &s[..end])
            }
        }
    }
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
    ///
    /// Applies-gate note [B-L5]: this and the chunk methods below skip
    /// `applies()` because a partial delta chunk can never satisfy the
    /// whole-response predicates the [`ResponseFix`] contract evaluates
    /// them on; the buffered paths (`apply_fixes`,
    /// `apply_fixes_with_context`, `detect_fixes`) do gate on it, per that
    /// contract. The historical asymmetry - streamed fixes bypassing the
    /// gate - is resolved: default `fake` streaming buffers the complete
    /// response and runs it through the gated buffered path, and
    /// `passthrough` only detects via [`Self::detect_fixes`], which gates
    /// too. The chunk family below survives only on the legacy streaming
    /// fallback.
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
                // Fail-safe: `FixError` carries no response, so an original is
                // kept to forward when the fixer errors. The clone costs one
                // deep-copy only once a fix actually applies, never on the
                // no-op path.
                let original = result.clone();
                match fix.apply_with_context(original, request) {
                    Ok((new_result, action)) => {
                        Self::log_fix_action(fix.name(), &action, fix.log_level());
                        result = new_result;
                    }
                    Err(error) => {
                        let action = Self::fixer_error_action(&result, &error);
                        Self::log_fix_action(fix.name(), &action, fix.log_level());
                    }
                }
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

    /// Detect (but never repair) malformed content in a complete response.
    ///
    /// Used by the verbatim streaming path, where fixes must not run because
    /// repairing partial deltas corrupts tool calls. Returns the names of
    /// fixes that would have fired; logs them as "detected, NOT repaired" so
    /// operators can see what the client received unmodified.
    ///
    /// `reason` states why repairs are skipped on *this* call path; it is
    /// supplied by the caller so the log can never claim an attribution the
    /// config doesn't actually select.
    ///
    /// Runs each fix's apply() on a throwaway clone and discards the result:
    /// apply() embeds the precise malformed-content predicates, so this gives
    /// fix-grade detection precision without any new per-fix code.
    pub fn detect_fixes(&self, response: &Value, request: Option<&Value>, reason: &str) -> Vec<String> {
        let mut detected = Vec::new();

        for fix in &self.fixes {
            if !self.is_enabled(fix.name()) {
                continue;
            }
            // applies() borrows: no clone in the common (no-match) case
            let applies = match request {
                Some(req) => fix.applies_with_context(response, req),
                None => fix.applies(response),
            };
            if !applies {
                continue;
            }
            let candidate = response.clone();
            let action = match request {
                Some(req) => match fix.apply_with_context(candidate, req) {
                    Ok((_, action)) => action,
                    Err(error) => Self::fixer_error_action(response, &error),
                },
                None => fix.apply(candidate).1,
            };
            if action.detected() {
                let snippet = match &action {
                    FixAction::Fixed { original_snippet, .. } => original_snippet.clone(),
                    FixAction::Failed { original_snippet, .. } => original_snippet.clone(),
                    FixAction::NotApplicable => String::new(),
                };
                let snippet = truncate_snippet(&snippet, 200, SnippetLimit::Chars);
                tracing::debug!(
                    fix_name = fix.name(),
                    reason = reason,
                    original = %snippet,
                    "streaming_pass_through: {} detected, NOT repaired - client received it as-is.",
                    fix.name()
                );
                detected.push(fix.name().to_string());
            }
        }

        detected
    }

    /// The `Failed` action recorded when a fixer returns `Err`: the error
    /// message rides in `attempted_fix`; the response — which the registry
    /// forwards untouched — is clipped into `original_snippet`.
    fn fixer_error_action(response: &Value, error: &FixError) -> FixAction {
        FixAction::Failed {
            original_snippet: truncate_snippet(&response.to_string(), 200, SnippetLimit::Chars),
            attempted_fix: format!("fixer error: {error}"),
        }
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
    ///
    /// Accepts both spellings with and without trailing "_fix" for backward compatibility.
    /// For example, both "toolcall_null_index" and "toolcall_null_index_fix" will match
    /// a fix named "toolcall_null_index_fix".
    ///
    /// The canonical fix name is used for internal tracking, regardless of which spelling
    /// the user provides in the config.
    pub fn configure(&mut self, config: &HashMap<String, crate::config::FixModuleConfig>) {
        for (name, module_config) in config {
            // Normalize: strip trailing "_fix" for comparison
            let normalized_name = name.strip_suffix("_fix").unwrap_or(name);
            let mut matched = false;

            for fix in &self.fixes {
                let fix_name = fix.name().strip_suffix("_fix").unwrap_or(fix.name());
                if normalized_name == fix_name {
                    // Insert with canonical fix name for consistency
                    self.enabled.insert(fix.name().to_string(), module_config.enabled);
                    tracing::debug!(
                        config_key = %name,
                        fix_name = %fix.name(),
                        enabled = module_config.enabled,
                        "Configured fix (normalized config key)"
                    );
                    matched = true;
                    break;
                }
            }

            // A key that matches nothing is almost always a typo. Silently ignoring it
            // means a fix the operator believes they disabled stays enabled.
            if !matched {
                let known: Vec<&str> = self.fixes.iter().map(|f| f.name()).collect();
                tracing::warn!(
                    config_key = %name,
                    known_fixes = ?known,
                    "Unknown fix name in config - ignoring this entry (check for a typo)"
                );
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
        let fix = Arc::new(ToolcallBadFilepathFix::new());
        registry.register(fix);

        assert_eq!(registry.list_fixes().len(), 1);
        assert!(registry.is_enabled("toolcall_bad_filepath"));
    }

    #[test]
    fn test_registry_set_enabled() {
        let mut registry = FixRegistry::new();
        registry.register(Arc::new(ToolcallBadFilepathFix::new()));

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
        registry.register(Arc::new(ToolcallBadFilepathFix::new()));

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
        registry.register(Arc::new(ToolcallBadFilepathFix::new()));
        registry.set_enabled("toolcall_bad_filepath", false);

        let response = serde_json::json!({"test": "value"});
        let result = registry.apply_fixes(response.clone());
        assert_eq!(result, response);
    }

    #[test]
    fn test_registry_apply_fixes_doesnt_apply() {
        let mut registry = FixRegistry::new();
        registry.register(Arc::new(ToolcallBadFilepathFix::new()));

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
        registry.register(Arc::new(ToolcallBadFilepathFix::new()));

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
        registry.register(Arc::new(ToolcallBadFilepathFix::new()));

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
        registry.register(Arc::new(ToolcallBadFilepathFix::new()));

        let mut modules = HashMap::new();
        modules.insert(
            "toolcall_bad_filepath".to_string(),
            crate::config::FixModuleConfig {
                enabled: false,
                options: HashMap::new(),
            },
        );

        registry.configure(&modules);
        assert!(!registry.is_enabled("toolcall_bad_filepath"));
    }

    #[test]
    fn test_registry_configure_accepts_fix_suffix_spelling() {
        // Both "toolcall_bad_filepath" and "toolcall_bad_filepath_fix" address the
        // same fix, and either spelling must key off the canonical name.
        let mut registry = FixRegistry::new();
        registry.register(Arc::new(ToolcallBadFilepathFix::new()));

        let mut modules = HashMap::new();
        modules.insert(
            "toolcall_bad_filepath_fix".to_string(),
            crate::config::FixModuleConfig {
                enabled: false,
                options: HashMap::new(),
            },
        );

        registry.configure(&modules);
        assert!(!registry.is_enabled("toolcall_bad_filepath"));
    }

    #[test]
    fn test_registry_configure_typo_leaves_fix_untouched() {
        // A misspelled key is ignored (and warned about) rather than silently
        // matching some other fix.
        let mut registry = FixRegistry::new();
        registry.register(Arc::new(ToolcallBadFilepathFix::new()));

        let mut modules = HashMap::new();
        modules.insert(
            "toolcall_badfilepath".to_string(),
            crate::config::FixModuleConfig {
                enabled: false,
                options: HashMap::new(),
            },
        );

        registry.configure(&modules);
        assert!(registry.is_enabled("toolcall_bad_filepath"));
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
        registry.register(Arc::new(ToolcallBadFilepathFix::new()));
        registry.register(Arc::new(ToolcallBadFilepathFix::new()));

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
        registry.register(Arc::new(ToolcallBadFilepathFix::new()));

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

    #[test]
    fn test_truncate_snippet_multibyte_is_char_safe() {
        // CJK + emoji: the byte-budget mode must never panic on a mid-sequence cut.
        let s = "ファイル🔧".repeat(40);

        let clipped = truncate_snippet(&s, 10, SnippetLimit::Bytes);
        assert!(clipped.ends_with("..."));
        assert!(clipped.len() <= 13);
        assert!(s.starts_with(clipped.trim_end_matches("...")));

        let clipped = truncate_snippet(&s, 10, SnippetLimit::Chars);
        assert!(clipped.ends_with("..."));
        assert_eq!(clipped.chars().count(), 10);
    }

    // --- task 31: Result-typed fail-safe apply_with_context ---

    /// Fixer whose `apply_with_context` always fails structurally, exercising
    /// the registry's Err path.
    struct FailingFix;

    impl ResponseFix for FailingFix {
        fn name(&self) -> &str {
            "failing_fix"
        }

        fn description(&self) -> &str {
            "test fixer that always returns Err from apply_with_context"
        }

        fn applies(&self, _response: &Value) -> bool {
            true
        }

        fn apply(&self, response: Value) -> (Value, FixAction) {
            (response, FixAction::NotApplicable)
        }

        fn apply_with_context(&self, _response: Value, _request: &Value) -> Result<(Value, FixAction), crate::fixes::FixError> {
            Err(crate::fixes::FixError::Parse("boom".to_string()))
        }
    }

    /// Fixer whose `apply_with_context` returns Ok with a Fixed action and a
    /// visibly-marked response.
    struct OkFixedFix;

    impl ResponseFix for OkFixedFix {
        fn name(&self) -> &str {
            "ok_fixed_fix"
        }

        fn description(&self) -> &str {
            "test fixer that always returns Ok(Fixed) from apply_with_context"
        }

        fn applies(&self, _response: &Value) -> bool {
            true
        }

        fn apply(&self, response: Value) -> (Value, FixAction) {
            (response, FixAction::NotApplicable)
        }

        fn apply_with_context(&self, _response: Value, _request: &Value) -> Result<(Value, FixAction), crate::fixes::FixError> {
            Ok((serde_json::json!({"marked": true}), FixAction::fixed("original", "marked")))
        }
    }

    /// Minimal fixer: implements ONLY the required trait methods. Its
    /// `apply_with_context` therefore comes from the trait default.
    struct BareFix;

    impl ResponseFix for BareFix {
        fn name(&self) -> &str {
            "bare_fix"
        }

        fn description(&self) -> &str {
            "minimal test fixer relying on the trait default apply_with_context"
        }

        fn applies(&self, _response: &Value) -> bool {
            false
        }

        fn apply(&self, response: Value) -> (Value, FixAction) {
            (response, FixAction::NotApplicable)
        }
    }

    #[test]
    fn test_apply_fixes_with_context_fixer_error_keeps_response_byte_identical() {
        // Given: a registry whose only fixer always returns Err
        let mut registry = FixRegistry::new();
        registry.register(Arc::new(FailingFix));

        let request = serde_json::json!({"model": "test"});
        let response = serde_json::json!({"choices": [{"message": {"content": "hello"}}]});

        // When: fixes run with request context
        let result = registry.apply_fixes_with_context(response.clone(), &request);

        // Then: the response flows out untouched — byte-identical, not merely
        // structurally equal (Value equality would forgive key reordering).
        assert_eq!(
            serde_json::to_string(&result).unwrap(),
            serde_json::to_string(&response).unwrap()
        );
    }

    #[test]
    fn test_apply_fixes_with_context_fixer_error_does_not_swallow_later_fix() {
        // CRITICAL fail-safe property: after a fixer errors, the ORIGINAL
        // response must keep flowing to the remaining fixes.
        let mut registry = FixRegistry::new();
        registry.register(Arc::new(FailingFix));
        registry.register(Arc::new(crate::fixes::ToolcallMalformedArgumentsFix::new()));

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
        assert!(args.contains(r#""file_path":"#));
        assert!(!args.contains(r#"{}":"#));
    }

    #[test]
    fn test_apply_fixes_with_context_ok_fixed_still_applies() {
        let mut registry = FixRegistry::new();
        registry.register(Arc::new(OkFixedFix));

        let request = serde_json::json!({"model": "test"});
        let response = serde_json::json!({"choices": []});

        let result = registry.apply_fixes_with_context(response, &request);

        assert_eq!(result["marked"], serde_json::json!(true));
    }

    #[test]
    fn test_fixer_error_action_records_reason_and_clips_snippet() {
        let response = serde_json::json!({"choices": [{"message": {"content": "x".repeat(500)}}]});

        let action = FixRegistry::fixer_error_action(&response, &crate::fixes::FixError::Parse("boom".to_string()));

        match action {
            FixAction::Failed {
                original_snippet,
                attempted_fix,
            } => {
                assert_eq!(attempted_fix, "fixer error: parse error: boom");
                assert_eq!(original_snippet.chars().count(), 200);
                assert!(original_snippet.ends_with("..."));
            }
            other => panic!("expected Failed action, got {other:?}"),
        }
    }

    #[test]
    fn test_fixer_error_action_attempted_fix_round_trips_cjk() {
        let action = FixRegistry::fixer_error_action(
            &serde_json::json!({}),
            &crate::fixes::FixError::Rebuild("再構築に失敗しました 🔧".to_string()),
        );

        match action {
            FixAction::Failed { attempted_fix, .. } => {
                assert_eq!(attempted_fix, "fixer error: rebuild error: 再構築に失敗しました 🔧");
            }
            other => panic!("expected Failed action, got {other:?}"),
        }
    }

    #[test]
    fn test_apply_with_context_trait_default_returns_ok_not_applicable() {
        let fix = BareFix;
        let response = serde_json::json!({"a": 1});

        let (out, action) = fix.apply_with_context(response.clone(), &serde_json::json!({})).unwrap();

        assert_eq!(out, response);
        assert!(matches!(action, FixAction::NotApplicable));
    }

    // --- task 22: registry-level fail-safe acceptance (REAL fixer, log capture) ---

    /// Minimal in-file tracing capture: collects every formatted event into a
    /// shared byte buffer. tracing-subscriber is a regular dependency (Cargo.toml),
    /// and no capture helper exists elsewhere in the repo, so this is the
    /// dependency-free seam for "exactly one Failed log record" assertions.
    #[derive(Clone)]
    struct CaptureWriter(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for CaptureWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CaptureWriter {
        type Writer = Self;
        fn make_writer(&self) -> Self::Writer {
            self.clone()
        }
    }

    fn response_with_arguments(arguments: &str) -> Value {
        serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "index": 0,
                        "function": { "name": "write", "arguments": arguments }
                    }]
                }
            }]
        })
    }

    #[test]
    fn test_real_fixer_unparseable_payload_keeps_input_and_emits_exactly_one_failed() {
        // Given: the REAL bad_filepath fixer (no mock) and a payload that
        // triggers it (unparseable) but whose schema surgery is impossible.
        let mut registry = FixRegistry::new();
        registry.register(Arc::new(ToolcallBadFilepathFix::new()));

        let request = serde_json::json!({"model": "test"});
        let response = response_with_arguments(r#"{"content":"x",,"filePath":garbage}"#);
        let input_json = serde_json::to_string(&response).unwrap();

        let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .with_writer(CaptureWriter(buf.clone()))
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        // When: fixes run with request context.
        let result = registry.apply_fixes_with_context(response.clone(), &request);

        // Then: output is BYTE-IDENTICAL to input (not Value equality — that
        // would forgive key reordering), and never the destructive "{}".
        let output_json = serde_json::to_string(&result).unwrap();
        assert_eq!(output_json, input_json);
        let args_out = result["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        assert_eq!(args_out, r#"{"content":"x",,"filePath":garbage}"#);
        assert_ne!(args_out, "{}");

        // And: exactly ONE "Failed to fix malformed content" (ERROR) record, and
        // no "Successfully fixed" success line — the baseline's misleading win.
        let logs = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        let failed_records = logs.matches("Failed to fix malformed content").count();
        let success_records = logs.matches("Successfully fixed malformed content").count();
        assert_eq!(failed_records, 1, "expected exactly one Failed record, logs:\n{logs}");
        assert_eq!(success_records, 0, "a failed fix must never log success, logs:\n{logs}");
    }

    #[test]
    fn test_real_fixer_parseable_duplicate_payload_still_fixed_via_context() {
        // Byte-identity guard for the OK path: a parseable duplicate-key
        // payload must still come out normalized + valid through the override.
        let mut registry = FixRegistry::new();
        registry.register(Arc::new(ToolcallBadFilepathFix::new()));

        let request = serde_json::json!({"model": "test"});
        let response = response_with_arguments(r#"{"filePath":"/path1","filePath":"/path2"}"#);

        let result = registry.apply_fixes_with_context(response, &request);

        let args_out = result["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        assert!(serde_json::from_str::<Value>(args_out).is_ok(), "normalized args: {args_out}");
        let parsed: Value = serde_json::from_str(args_out).unwrap();
        assert_eq!(parsed["filePath"], "/path1", "task 25 (B-M2): duplicates resolve FIRST-wins");
    }
}
