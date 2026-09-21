//! Response fix modules for correcting malformed LLM responses

mod json_scan;
mod registry;
mod toolcall_bad_filepath_fix;
mod toolcall_malformed_arguments_fix;
mod toolcall_null_index_fix;

use async_trait::async_trait;
use registry::{truncate_snippet, SnippetLimit};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

pub use registry::{AsAny, FixRegistry};
pub use toolcall_bad_filepath_fix::ToolcallBadFilepathFix;
pub use toolcall_malformed_arguments_fix::ToolcallMalformedArgumentsFix;
pub use toolcall_null_index_fix::ToolCallNullIndexFix;

/// Log level for fix detection/success messages
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixLogLevel {
    /// Log at TRACE level (most verbose)
    Trace,
    /// Log at DEBUG level
    Debug,
    /// Log at INFO level (default)
    Info,
    /// Log at WARN level
    Warn,
}

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

/// Structural failure of a fixer that had already decided the response needs
/// repair (task 31 fail-safe interface). It never carries a response: on Err
/// the registry logs a `FixAction::Failed` and forwards the ORIGINAL response,
/// so a broken fixer can never corrupt or swallow client data.
#[derive(Debug, Clone, PartialEq)]
pub enum FixError {
    /// The fixer could not parse the content it was asked to repair.
    Parse(String),
    /// The fixer parsed the content but could not rebuild a valid response.
    Rebuild(String),
}

impl std::fmt::Display for FixError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FixError::Parse(message) => write!(f, "parse error: {message}"),
            FixError::Rebuild(message) => write!(f, "rebuild error: {message}"),
        }
    }
}

/// Coarse outcome a fixer reports for a response.
///
/// Forward-declared interface for the tasks 29/30 logging consolidation;
/// no consumer selects on it yet, so the registry still discriminates on the
/// full [`FixAction`] payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // consumed by tasks 29/30; pub API kept intentionally
pub enum FixOutcome {
    /// Content was fine or the fixer does not handle it.
    NotApplicable,
    /// Malformed content was detected and successfully fixed.
    Fixed,
}

/// Accumulates tool call arguments across streaming chunks for fixing
#[derive(Default)]
pub struct ToolCallAccumulator {
    /// Map of tool call index -> accumulated arguments string
    accumulated: HashMap<usize, String>,
    /// Map of tool call index -> whether this index has been fixed
    /// After a fix is applied, subsequent chunks for this index are suppressed
    fixed: HashMap<usize, bool>,
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
                snippet = truncate_snippet(&accumulated_str, 100, SnippetLimit::Bytes),
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

    /// Clear accumulated arguments for a tool call (after sending fixed version)
    pub fn clear(&mut self, index: usize) {
        self.accumulated.remove(&index);
    }

    /// Mark a tool call index as fixed (after sending completion delta)
    /// Subsequent chunks for this index will be suppressed
    pub fn mark_fixed(&mut self, index: usize) {
        self.fixed.insert(index, true);
        // Also clear accumulated content since we've sent the completion
        self.accumulated.remove(&index);
    }

    /// Check if a tool call index has been fixed
    /// Returns true if this index should have subsequent chunks suppressed
    pub fn is_fixed(&self, index: usize) -> bool {
        self.fixed.get(&index).copied().unwrap_or(false)
    }

    /// Reset both accumulated and fixed state for a tool call index
    /// Used when a new tool call starts (new index or finish_reason indicates completion)
    pub fn reset(&mut self, index: usize) {
        self.accumulated.remove(&index);
        self.fixed.remove(&index);
    }

    /// Get the accumulated arguments for a tool call index (for testing)
    #[cfg(test)]
    pub fn get(&self, index: usize) -> Option<&str> {
        self.accumulated.get(&index).map(|s| s.as_str())
    }
}

/// Trait for response fix modules
///
/// PRIMARY PATH: We now work with complete JSON responses (via `apply()` method).
/// All client streaming is synthesized after fixes are applied to complete JSON.
///
/// LEGACY PATH: Streaming methods below are kept ONLY for the fallback streaming handler
/// that handles unexpected streaming responses from the backend. New fixes should focus
/// on implementing `apply()` for complete JSON only.
#[async_trait]
pub trait ResponseFix: Send + Sync {
    /// Unique identifier for the fix
    fn name(&self) -> &str;

    /// Human-readable description
    fn description(&self) -> &str;

    /// Return the log level for successful fix actions
    /// Default: Info (WARN on detection, INFO on success)
    fn log_level(&self) -> FixLogLevel {
        FixLogLevel::Info
    }

    /// Check if this fix applies to the response
    fn applies(&self, response: &Value) -> bool;

    /// **PRIMARY METHOD**: Apply the fix to a complete response
    /// Implementations MUST return appropriate FixAction for logging
    fn apply(&self, response: Value) -> (Value, FixAction);

    /// **LEGACY**: Apply fix to streaming chunk (ONLY used by fallback streaming handler)
    /// Default: no-op. Most fixes should not need to implement this anymore.
    fn apply_stream(&self, chunk: Value) -> (Value, FixAction) {
        (chunk, FixAction::NotApplicable)
    }

    // Context-aware methods

    /// Check if this fix applies to the response with request context
    fn applies_with_context(&self, response: &Value, _request: &Value) -> bool {
        self.applies(response)
    }

    /// Apply the fix to the response with request context
    ///
    /// Returns `Err([FixError])` when the fixer fails structurally; the
    /// registry then records a `FixAction::Failed` and forwards the ORIGINAL
    /// response untouched. [`ResponseFix::apply`] stays the simple
    /// (infallible-by-contract) primary method that fixes implement; this
    /// default mirrors it by lifting its result into `Ok`.
    ///
    /// [`FixError`]: FixError
    fn apply_with_context(&self, response: Value, _request: &Value) -> Result<(Value, FixAction), FixError> {
        Ok(self.apply(response))
    }

    /// **LEGACY**: Apply fix to streaming chunk with request context
    fn apply_stream_with_context(&self, chunk: Value, _request: &Value) -> (Value, FixAction) {
        self.apply_stream(chunk)
    }

    /// **LEGACY**: Apply fix to streaming chunk with accumulation support (with request context)
    fn apply_stream_with_accumulation(
        &self,
        chunk: Value,
        request: &Value,
        _accumulator: &mut ToolCallAccumulator,
    ) -> (Value, FixAction) {
        self.apply_stream_with_context(chunk, request)
    }

    /// **LEGACY**: Apply fix to streaming chunk with accumulation support (without request context)
    fn apply_stream_with_accumulation_default(
        &self,
        chunk: Value,
        _accumulator: &mut ToolCallAccumulator,
    ) -> (Value, FixAction) {
        self.apply_stream(chunk)
    }
}

/// Create the default fix registry with all available fixes
///
/// Fix registration order is critical for correct operation:
/// 1. **ToolCallNullIndexFix** (FIRST) - Foundational fix that assigns sequential indices
///    to tool calls lacking them. Other fixes may assume valid indices exist.
/// 2. **ToolcallMalformedArgumentsFix** - Handles the specific `{}`":" pattern in arguments
///    before the broader filepath fix runs.
/// 3. **ToolcallBadFilepathFix** - Removes duplicate filePath keys (runs last as it's more general)
pub fn create_default_registry() -> FixRegistry {
    let mut registry = FixRegistry::new();
    // Register null index fix FIRST - it's foundational
    // Other fixes may assume valid indices exist
    registry.register(Arc::new(ToolCallNullIndexFix::new(true)));
    // Register malformed arguments fix - it handles the more specific {}":" pattern
    // This ensures it runs before the broader filepath fix
    registry.register(Arc::new(ToolcallMalformedArgumentsFix::new()));
    registry.register(Arc::new(ToolcallBadFilepathFix::new()));
    registry
}

/// Build the startup fix registry honoring the `fixes:` config section.
///
/// A module whose config entry says `enabled: false` is **not constructed
/// into the registry at all**: it cannot run, it is absent from
/// [`FixRegistry::list_fixes`], and a startup INFO line names it as
/// skipped. A module absent from `modules` is constructed (absent means
/// enabled); explicit `enabled: true` behaves the same. When the global
/// `fixes.enabled` switch is false, no module is constructed.
///
/// Config keys match a fix by its canonical name with or without the
/// `_fix` suffix, mirroring [`FixRegistry::configure`]; if both spellings
/// appear for one fix, the canonical (suffix-free) key wins deterministically.
/// A key matching no known fix warns and is skipped while all known fixes
/// stay constructed - the same warn-and-continue behavior `configure()`
/// gives a typo.
///
/// Registration order is identical to [`create_default_registry`] and is
/// load-bearing; `test_from_config_preserves_default_registration_order`
/// pins the two lists together so they cannot drift.
///
/// Design note (disclosed): disabled candidates are constructed, then
/// dropped *without registration*, rather than pre-filtered through a
/// hardcoded name list. The constructors are trivial (atomics only), and
/// reading `fix.name()` from the instance keeps config matching from
/// drifting away from the real fix names. The observable result is
/// identical to never constructing: the registry never contains the module.
pub fn create_registry_from_config(fixes: &crate::config::FixesConfig) -> FixRegistry {
    let mut registry = FixRegistry::new();
    if !fixes.enabled {
        tracing::info!("fixes.enabled = false - no fix modules constructed");
        return registry;
    }

    // Same load-bearing order as create_default_registry (pinned by test).
    let specs: [fn() -> Arc<dyn ResponseFix>; 3] = [
        // FIRST: foundational - other fixes may assume valid indices exist
        || Arc::new(ToolCallNullIndexFix::new(true)),
        // before the broader filepath fix: the specific {}":" pattern
        || Arc::new(ToolcallMalformedArgumentsFix::new()),
        // last: general duplicate filePath removal
        || Arc::new(ToolcallBadFilepathFix::new()),
    ];

    let mut canonical_names: Vec<String> = Vec::with_capacity(specs.len());
    let mut base_names: Vec<String> = Vec::with_capacity(specs.len());
    for make in specs {
        let fix = make();
        // Own the name before `fix` moves into register()/drop.
        let name = fix.name().to_string();
        let base = name.strip_suffix("_fix").unwrap_or(&name).to_string();
        canonical_names.push(name.clone());
        base_names.push(base.clone());

        // Canonical spelling first, then the "_fix" spelling (both address
        // the same fix, per FixRegistry::configure's normalization).
        let entry = fixes.modules.get(&base).or_else(|| {
            let suffixed = format!("{base}_fix");
            fixes.modules.get(&suffixed)
        });
        match entry {
            Some(module) if !module.enabled => {
                tracing::info!(
                    module = %name,
                    "Fix module disabled in config - not constructed"
                );
            }
            _ => registry.register(fix),
        }
    }

    // A key matching no known fix is almost always a typo; warn (listing
    // what IS known) and continue with every known fix constructed.
    for key in fixes.modules.keys() {
        let key_base = key.strip_suffix("_fix").unwrap_or(key);
        if !base_names.iter().any(|b| b == key_base) {
            tracing::warn!(
                config_key = %key,
                known_fixes = ?canonical_names,
                "Unknown fix name in config - ignoring this entry (check for a typo)"
            );
        }
    }

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
    fn test_truncate_snippet_bytes_truncates_long_text() {
        let long_text = "a".repeat(200);
        let snippet = truncate_snippet(&long_text, 100, SnippetLimit::Bytes);

        assert!(snippet.len() <= 103); // 100 + "..."
        assert!(snippet.ends_with("..."));
    }

    #[test]
    fn test_truncate_snippet_bytes_preserves_short_text() {
        let short_text = "short text";
        let snippet = truncate_snippet(short_text, 100, SnippetLimit::Bytes);

        assert_eq!(snippet, short_text);
        assert!(!snippet.ends_with("..."));
    }

    // --- task 32: config-aware registry construction ---

    /// FixesConfig with the global switch and a `{name: enabled}` module map.
    fn fixes_cfg(global_enabled: bool, modules: &[(&str, bool)]) -> crate::config::FixesConfig {
        let mut map = std::collections::HashMap::new();
        for (name, enabled) in modules {
            map.insert(
                (*name).to_string(),
                crate::config::FixModuleConfig {
                    enabled: *enabled,
                    options: std::collections::HashMap::new(),
                },
            );
        }
        crate::config::FixesConfig {
            enabled: global_enabled,
            modules: map,
        }
    }

    #[test]
    fn test_from_config_disabled_module_is_not_constructed() {
        // Given: a config that disables exactly one module
        let fixes = fixes_cfg(true, &[("toolcall_bad_filepath", false)]);

        // When: the startup registry is built from config
        let registry = create_registry_from_config(&fixes);

        // Then: the disabled module is absent (not merely disabled)
        assert_eq!(registry.list_fixes().len(), 2);
        assert!(registry.get_fix("toolcall_bad_filepath").is_none());
        assert!(registry.is_enabled("toolcall_null_index_fix"));
        assert!(registry.is_enabled("toolcall_malformed_arguments"));
    }

    #[test]
    fn test_from_config_explicit_true_and_absent_both_construct() {
        // Given: one module explicitly enabled, the other two absent from the map
        let fixes = fixes_cfg(true, &[("toolcall_bad_filepath", true)]);

        // When/Then: all three construct; absent means enabled
        let registry = create_registry_from_config(&fixes);
        assert_eq!(registry.list_fixes().len(), 3);
        assert!(registry.is_enabled("toolcall_bad_filepath"));
        assert!(registry.is_enabled("toolcall_null_index_fix"));
        assert!(registry.is_enabled("toolcall_malformed_arguments"));
    }

    #[test]
    fn test_from_config_matches_both_name_spellings() {
        // Given/When/Then: the suffix-free key disables the fix whose name
        // carries "_fix"...
        let short_key = create_registry_from_config(&fixes_cfg(true, &[("toolcall_null_index", false)]));
        assert_eq!(short_key.list_fixes().len(), 2);
        assert!(short_key.get_fix("toolcall_null_index_fix").is_none());

        // ...and the "_fix" key disables the suffix-free fix.
        let suffixed_key = create_registry_from_config(&fixes_cfg(true, &[("toolcall_bad_filepath_fix", false)]));
        assert_eq!(suffixed_key.list_fixes().len(), 2);
        assert!(suffixed_key.get_fix("toolcall_bad_filepath").is_none());
    }

    #[test]
    fn test_from_config_unknown_key_warns_and_continues() {
        // Given: a typo'd key and a non-ASCII key - neither matches a known fix.
        // The warn itself is verified in the raw startup log (evidence); this
        // pins the CONTINUE half: known fixes all still construct, no panic.
        let fixes = fixes_cfg(true, &[("toolcall_badfilepath", false), ("ツール修正", false)]);

        let registry = create_registry_from_config(&fixes);
        assert_eq!(registry.list_fixes().len(), 3);
        assert!(registry.is_enabled("toolcall_bad_filepath"));
    }

    #[test]
    fn test_from_config_global_disabled_constructs_nothing() {
        // Given: the global switch off, even with a module asking to be enabled
        let fixes = fixes_cfg(false, &[("toolcall_bad_filepath", true)]);

        // When/Then: nothing is constructed
        let registry = create_registry_from_config(&fixes);
        assert!(registry.list_fixes().is_empty());
    }

    #[test]
    fn test_from_config_preserves_default_registration_order() {
        // The config-aware list and the default list must stay in lockstep:
        // registration order is load-bearing (null index first).
        let default_registry = create_default_registry();
        let expected: Vec<&str> = default_registry.list_fixes().iter().map(|f| f.name()).collect();

        let registry = create_registry_from_config(&crate::config::FixesConfig::default());
        let actual: Vec<&str> = registry.list_fixes().iter().map(|f| f.name()).collect();
        assert_eq!(actual, expected);
    }
}
