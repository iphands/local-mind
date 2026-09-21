//! Response fix modules for correcting malformed LLM responses

mod json_scan;
mod registry;
mod toolcall_bad_filepath_fix;
mod toolcall_malformed_arguments_fix;
mod toolcall_null_index_fix;

use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

// Only the snippet tests below use these since task 29 deleted the streaming
// call sites that needed them in the library build.
#[cfg(test)]
use registry::{truncate_snippet, SnippetLimit};

/// Install a global registry subscriber ONCE per test process.
///
/// tracing-core caches per-callsite Interest process-wide and REBUILDS it on
/// every Dispatch::new (any `fmt().finish()` in any test) using the CALLING
/// thread's current dispatcher (JustOne mode when <=1 dispatch is registered).
/// A thread without a thread-local subscriber evaluates as NoSubscriber ->
/// Interest::never, killing capture callsites that a guarded thread registered
/// first (measured: WARN captured, ERROR from the SAME Failed arm dropped on
/// the capturing thread itself). A global registry makes every thread's
/// get_default return Interest::sometimes at worst, so `never` can never be
/// re-cached, and each event is routed per-event to the thread's own guard.
#[cfg(test)]
pub(crate) fn pin_interest_cache_for_tests() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = tracing::subscriber::set_global_default(tracing_subscriber::registry());
    });
}

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

    /// Fold the per-call `Fixed` actions collected while walking one response
    /// into the SINGLE action the registry logs per fix [B-M3]. The historical
    /// `overall_action` variable was overwritten by every repair, so a response
    /// with several repaired tool calls reported only the LAST call's
    /// snippets — the other repairs ran but were invisible in the log.
    ///
    /// One repair passes through untouched (snippets byte-identical to the
    /// single-call shape); several concatenate in traversal order behind a
    /// `"<N> repairs: "` count prefix; none yields `NotApplicable`.
    /// Callers push only completed repairs, so the other variants can
    /// contribute nothing — the fold stays a total function anyway.
    pub(crate) fn aggregate_repairs(actions: &[FixAction]) -> FixAction {
        match actions {
            [] => FixAction::NotApplicable,
            [single] => single.clone(),
            many => {
                let mut originals = String::new();
                let mut fixeds = String::new();
                for action in many {
                    let (original, fixed) = match action {
                        FixAction::Fixed {
                            original_snippet,
                            fixed_snippet,
                        } => (original_snippet, fixed_snippet),
                        FixAction::NotApplicable | FixAction::Failed { .. } => continue,
                    };
                    if !originals.is_empty() {
                        originals.push_str(" ;; ");
                        fixeds.push_str(" ;; ");
                    }
                    originals.push_str(original);
                    fixeds.push_str(fixed);
                }
                FixAction::Fixed {
                    original_snippet: format!("{} repairs: {originals}", many.len()),
                    fixed_snippet: format!("{} repairs: {fixeds}", many.len()),
                }
            }
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

/// Trait for response fix modules
///
/// Fixes run on complete JSON responses (`apply()` / `apply_with_context()`):
/// the default `fake` streaming mode buffers the backend and synthesizes SSE
/// after the fixes ran, and `passthrough` only detects via
/// [`FixRegistry::detect_fixes`]. The legacy per-chunk streaming machinery
/// (trait streaming methods + `ToolCallAccumulator`) was dead code and was
/// deleted (task 29).
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
    registry.register(Arc::new(ToolCallNullIndexFix::new()));
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
        || Arc::new(ToolCallNullIndexFix::new()),
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

    // TASK 30 (B-M3): fold contract of the aggregation that replaced the
    // overwritten `overall_action` variable (fixer-level end-to-end coverage:
    // task30_two_repaired_calls_* in both fix modules).
    #[test]
    fn task30_aggregate_repairs_fold_contract() {
        assert!(matches!(FixAction::aggregate_repairs(&[]), FixAction::NotApplicable));

        let one = FixAction::fixed("o1", "f1");
        match FixAction::aggregate_repairs(std::slice::from_ref(&one)) {
            FixAction::Fixed {
                original_snippet,
                fixed_snippet,
            } => {
                assert_eq!(original_snippet, "o1", "single repair passes through verbatim");
                assert_eq!(fixed_snippet, "f1");
            }
            other => panic!("single repair must stay Fixed, got {other:?}"),
        }

        let many = vec![FixAction::fixed("o1", "f1"), FixAction::fixed("o2 日本", "f2 🔧")];
        match FixAction::aggregate_repairs(&many) {
            FixAction::Fixed {
                original_snippet,
                fixed_snippet,
            } => {
                assert_eq!(original_snippet, "2 repairs: o1 ;; o2 日本");
                assert_eq!(fixed_snippet, "2 repairs: f1 ;; f2 🔧");
            }
            other => panic!("multi repair must fold into Fixed, got {other:?}"),
        }
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
