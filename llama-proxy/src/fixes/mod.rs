//! Response fix modules for correcting malformed LLM responses

mod registry;
mod toolcall_bad_filepath_fix;
mod toolcall_malformed_arguments_fix;

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

pub use registry::{AsAny, FixRegistry};
pub use toolcall_bad_filepath_fix::ToolcallBadFilepathFix;
pub use toolcall_malformed_arguments_fix::ToolcallMalformedArgumentsFix;

/// Result of applying a fix, used for standardized logging
#[derive(Debug, Clone)]
pub enum FixAction {
    /// Fix did not apply (content was fine or fix doesn't handle this)
    NotApplicable,
    /// Malformed content detected and successfully fixed
    Fixed {
        original_snippet: String,
        fixed_snippet: String,
    },
    /// Malformed content detected but fix failed
    Failed {
        original_snippet: String,
        attempted_fix: String,
    },
}

impl FixAction {
    /// Create a Fixed action with original and fixed snippets
    pub fn fixed(original: &str, fixed: &str) -> Self {
        Self::Fixed {
            original_snippet: original.to_string(),
            fixed_snippet: fixed.to_string(),
        }
    }

    /// Create a Failed action with original and attempted fix snippets
    pub fn failed(original: &str, attempted: &str) -> Self {
        Self::Failed {
            original_snippet: original.to_string(),
            attempted_fix: attempted.to_string(),
        }
    }

    /// Returns true if malformed content was detected (Fixed or Failed)
    pub fn detected(&self) -> bool {
        matches!(self, Self::Fixed { .. } | Self::Failed { .. })
    }
}

impl Default for FixAction {
    fn default() -> Self {
        Self::NotApplicable
    }
}

/// Accumulates tool call arguments across streaming chunks for fixing
#[derive(Default)]
pub struct ToolCallAccumulator {
    /// Map of tool call index -> accumulated arguments string
    accumulated: HashMap<usize, String>,
}

impl ToolCallAccumulator {
    /// Create a new empty accumulator
    pub fn new() -> Self {
        Self::default()
    }

    /// Add chunk arguments for a tool call and return the accumulated string
    pub fn accumulate(&mut self, index: usize, chunk_args: &str) -> String {
        let accumulated = self.accumulated.entry(index).or_default();
        accumulated.push_str(chunk_args);
        accumulated.clone()
    }

    /// Add chunk arguments and return the accumulated string
    /// Also checks for malformed patterns and logs warnings
    pub fn accumulate_and_check(&mut self, index: usize, chunk_args: &str, fix_name: &str) -> String {
        let accumulated = self.accumulated.entry(index).or_default();
        accumulated.push_str(chunk_args);

        // NEW: Eager detection - check for malformed patterns as we accumulate
        let accumulated_str = accumulated.clone();

        // Check for duplicate "filePath" keys
        let filepath_count = accumulated_str.matches(r#""filePath""#).count();
        if filepath_count > 1 {
            // Log warning IMMEDIATELY when duplicate detected
            tracing::warn!(
                fix_name = fix_name,
                index = index,
                filepath_count = filepath_count,
                accumulated_length = accumulated_str.len(),
                snippet = Self::create_snippet(&accumulated_str, 100),
                "DETECTED: Duplicate filePath in accumulated arguments"
            );
        }

        // Debug logging to trace accumulation
        tracing::debug!(
            fix_name = fix_name,
            index = index,
            chunk_length = chunk_args.len(),
            accumulated_length = accumulated_str.len(),
            filepath_count = filepath_count,
            "Accumulating tool call arguments"
        );

        accumulated_str
    }

    fn create_snippet(text: &str, max_len: usize) -> String {
        if text.len() > max_len {
            format!("{}...", &text[..max_len])
        } else {
            text.to_string()
        }
    }

    /// Clear accumulated arguments for a tool call (after sending fixed version)
    pub fn clear(&mut self, index: usize) {
        self.accumulated.remove(&index);
    }

    /// Get the accumulated arguments for a tool call index (for testing)
    #[cfg(test)]
    pub fn get(&self, index: usize) -> Option<&str> {
        self.accumulated.get(&index).map(|s| s.as_str())
    }
}

/// Trait for response fix modules
#[async_trait]
pub trait ResponseFix: Send + Sync {
    /// Unique identifier for the fix
    fn name(&self) -> &str;

    /// Human-readable description
    fn description(&self) -> &str;

    /// Check if this fix applies to the response
    fn applies(&self, response: &Value) -> bool;

    /// Apply the fix to the response (non-streaming), returning modified value AND action taken
    /// Implementations MUST return appropriate FixAction for logging
    fn apply(&self, response: Value) -> (Value, FixAction);

    /// Apply fix to a streaming chunk, returning modified value AND action taken
    fn apply_stream(&self, chunk: Value) -> (Value, FixAction) {
        (chunk, FixAction::NotApplicable) // Default: no action
    }

    // Context-aware methods with default implementations for backward compatibility

    /// Check if this fix applies to the response with request context
    /// Default implementation delegates to applies() for backward compatibility
    fn applies_with_context(&self, response: &Value, _request: &Value) -> bool {
        self.applies(response)
    }

    /// Apply the fix to the response with request context (non-streaming)
    /// Default implementation delegates to apply() for backward compatibility
    fn apply_with_context(&self, response: Value, _request: &Value) -> (Value, FixAction) {
        self.apply(response)
    }

    /// Apply fix to a streaming chunk with request context (optional)
    /// Default implementation delegates to apply_stream() for backward compatibility
    fn apply_stream_with_context(&self, chunk: Value, _request: &Value) -> (Value, FixAction) {
        self.apply_stream(chunk)
    }

    /// Apply fix to a streaming chunk with accumulation support (with request context)
    /// Default implementation delegates to context-aware version without accumulation
    fn apply_stream_with_accumulation(
        &self,
        chunk: Value,
        request: &Value,
        _accumulator: &mut ToolCallAccumulator,
    ) -> (Value, FixAction) {
        self.apply_stream_with_context(chunk, request)
    }

    /// Apply fix to a streaming chunk with accumulation support (without request context)
    /// Default implementation delegates to regular stream version without accumulation
    fn apply_stream_with_accumulation_default(
        &self,
        chunk: Value,
        _accumulator: &mut ToolCallAccumulator,
    ) -> (Value, FixAction) {
        self.apply_stream(chunk)
    }
}

/// Create the default fix registry with all available fixes
pub fn create_default_registry() -> FixRegistry {
    let mut registry = FixRegistry::new();
    // Register malformed arguments fix first - it handles the more specific {}":" pattern
    // This ensures it runs before the broader filepath fix
    registry.register(Arc::new(ToolcallMalformedArgumentsFix::new()));
    registry.register(Arc::new(ToolcallBadFilepathFix::new(true)));
    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_accumulate_and_check_logs_warning_on_duplicate() {
        let mut acc = ToolCallAccumulator::new();

        // Simulate streaming chunks with duplicate filePath
        let chunk1 = r#"{"content":"code","filePath":"/path","#;
        let chunk2 = r#""filePath":"/corrupted"}"#;

        let acc1 = acc.accumulate_and_check(0, chunk1, "test_fix");
        // Should not warn yet (only 1 filePath)
        assert_eq!(acc1.matches(r#""filePath""#).count(), 1);

        let acc2 = acc.accumulate_and_check(0, chunk2, "test_fix");
        // Should WARN (now has 2 filePath strings)
        // The warning will be logged by tracing, which we can't easily test in unit tests
        // but we can verify the count
        assert!(acc2.contains("filePath"));
        assert_eq!(acc2.matches(r#""filePath""#).count(), 2);
    }

    #[test]
    fn test_accumulate_and_check_no_warning_on_single_filepath() {
        let mut acc = ToolCallAccumulator::new();

        let chunk = r#"{"content":"code","filePath":"/path"}"#;
        let result = acc.accumulate_and_check(0, chunk, "test_fix");

        // Should only have 1 filePath - no warning
        assert_eq!(result.matches(r#""filePath""#).count(), 1);
    }

    #[test]
    fn test_create_snippet_truncates_long_text() {
        let long_text = "a".repeat(200);
        let snippet = ToolCallAccumulator::create_snippet(&long_text, 100);

        assert!(snippet.len() <= 103); // 100 + "..."
        assert!(snippet.ends_with("..."));
    }

    #[test]
    fn test_create_snippet_preserves_short_text() {
        let short_text = "short text";
        let snippet = ToolCallAccumulator::create_snippet(short_text, 100);

        assert_eq!(snippet, short_text);
        assert!(!snippet.ends_with("..."));
    }
}
